// settingsPersistence 回归测试(设置集中保存,2026-09-16)。
//
// 覆盖的不只是 happy path:在途 A + 新 patch B、防抖失败回滚、跨窗口 reset 丢弃旧 pending、
// 重置失败/重复点击去重、flush 错误上报、权威快照不吞未确认预览。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'

vi.mock('../utils/ipc', () => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/appEvents', () => ({ listenAppEvent: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('../utils/logger', () => ({
  logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

import { invokeIpc } from '../utils/ipc'
import { listenAppEvent } from '../utils/appEvents'
import { IPC } from '../constants/ipc'
import type { SettingsChange, SettingsSnapshot } from '../types/config'
import {
  flushSettings,
  initializeSettings,
  installSettingsBridge,
  readSetting,
  resetSettings,
  resetSettingsModuleStateForTests,
  settingsReady,
  writeSettings,
} from './settingsPersistence'

function snapshot(values: Record<string, string>, revision: number, generation = 0): SettingsSnapshot {
  return { values, revision, generation }
}

function change(
  values: Record<string, string>,
  revision: number,
  extra?: Partial<SettingsChange>,
): SettingsChange {
  return {
    snapshot: snapshot(values, revision),
    keys: Object.keys(values),
    restart_required: [],
    apply_failed: [],
    ...extra,
  }
}

type EventHandler = (e: { payload: unknown }) => void

/** 捕获 config-file-changed 的处理器,便于用例直接投递事件。 */
function captureEventHandler(): () => EventHandler {
  let handler: EventHandler | null = null
  vi.mocked(listenAppEvent).mockImplementation((_event, cb) => {
    handler = cb as unknown as EventHandler
    return Promise.resolve(() => {})
  })
  return () => {
    if (!handler) throw new Error('事件处理器未注册')
    return handler
  }
}

/** 让 initializeSettings 拿到一份初始快照(所有后续用例的共同起点)。 */
async function boot(values: Record<string, string> = { player_volume: '1' }) {
  vi.mocked(invokeIpc).mockResolvedValueOnce({
    settings: snapshot(values, 1),
    state: { firstLaunch: null, guideSeen: null },
  });
  await initializeSettings();
}

function setAppSettingsCalls() {
  return vi.mocked(invokeIpc).mock.calls.filter((c) => c[0] === IPC.SET_APP_SETTINGS)
}

describe('settingsPersistence', () => {
  beforeEach(() => {
    resetSettingsModuleStateForTests();
    vi.mocked(invokeIpc).mockReset();
    vi.mocked(listenAppEvent).mockReset();
    vi.mocked(listenAppEvent).mockImplementation(() => Promise.resolve(() => {}));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('就绪门控', () => {
    it('快照到达前读取落 undefined、写入被拒(不把占位值当用户修改写回)', async () => {
      expect(settingsReady.value).toBe(false);
      expect(readSetting('player_volume')).toBeUndefined();

      await writeSettings({ player_volume: '0.5' });

      expect(invokeIpc).not.toHaveBeenCalled();
      expect(settingsReady.value).toBe(false);
    });

    it('初始快照不覆盖已通过事件到达的更新快照(按 revision 守卫)', async () => {
      const getHandler = captureEventHandler();
      installSettingsBridge();
      expect(getHandler).toBeTypeOf('function');
      // 事件先到:更新 revision 的快照已落地。
      getHandler()({
        payload: {
          snapshot: snapshot({ demo_privacy: 'true' }, 9),
          keys: [],
          restart_required: [],
          apply_failed: [],
        },
      });
      expect(readSetting('demo_privacy')).toBe('true');

      // 随后到达的初始快照更旧:不得把它装回来。
      vi.mocked(invokeIpc).mockResolvedValueOnce({
        settings: snapshot({ demo_privacy: 'false' }, 3),
        state: { firstLaunch: null, guideSeen: null },
      });
      await initializeSettings();
      expect(readSetting('demo_privacy')).toBe('true');
    });
  });

  describe('提交与落盘', () => {
    it('即时写入立即预览,失败后回滚到确认值', async () => {
      await boot({ player_volume: '1' });
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('write failed'));

      const promise = writeSettings({ player_volume: '0.4' });
      // 即时预览:同步可见(不等后端回执)。
      expect(readSetting('player_volume')).toBe('0.4');

      await expect(promise).rejects.toThrow('write failed');
      // 失败回滚到最后确认值。
      expect(readSetting('player_volume')).toBe('1');
    });

    it('在途 A 期间到达新 patch B:两次提交串行,最终值为 B', async () => {
      await boot({ player_volume: '1' });
      let releaseA: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseA = () => resolve(change({ player_volume: '0.5' }, 2));
          }),
      );
      vi.mocked(invokeIpc).mockImplementationOnce(() =>
        Promise.resolve(change({ player_volume: '0.8' }, 3)),
      );

      const a = writeSettings({ player_volume: '0.5' });
      const b = writeSettings({ player_volume: '0.8' });
      // B 已预览,但 A 仍在途:B 不得抢跑。
      expect(readSetting('player_volume')).toBe('0.8');
      expect(setAppSettingsCalls()).toHaveLength(1);

      releaseA!();
      await Promise.all([a, b]);

      const calls = setAppSettingsCalls();
      expect(calls).toHaveLength(2);
      expect((calls[1][1] as { patch: Record<string, string> }).patch).toEqual({
        player_volume: '0.8',
      });
      expect(readSetting('player_volume')).toBe('0.8');
    });
  });

  describe('flush', () => {

    it('flush 对在等待窗口内失败的写盘可靠抛错(退出流程据此拒绝退出)', async () => {
      await boot();
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('disk full'));
      // 消费方按既有姿态吞掉 rejection,失败仍需经 flush 上报。
      void writeSettings({ player_volume: '0.5' }).catch(() => {});
      await expect(flushSettings()).rejects.toThrow('disk full');
    });

    it('flush 等待在途请求期间新到的防抖写入不会漏(窗口内 B 必须一并落盘)', async () => {
      vi.useFakeTimers();
      await boot({ player_volume: '1' });
      let releaseA: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseA = () => resolve(change({ player_volume: '0.5' }, 2));
          }),
      );
      vi.mocked(invokeIpc).mockResolvedValue(change({ player_volume: '0.9' }, 3));

      // A:立刻提交并卡在途。
      void writeSettings({ player_volume: '0.5' }).catch(() => {});
      const flushing = flushSettings();

      // B:flush 等待 A 期间用户又拖了一下(防抖写入,尚未排队)。
      const b = writeSettings({ player_volume: '0.9' }, { debounce: true });
      await vi.advanceTimersByTimeAsync(400);
      releaseA!();
      await flushing;
      await b;

      const calls = setAppSettingsCalls();
      expect(calls).toHaveLength(2);
      expect((calls[1][1] as { patch: Record<string, string> }).patch).toEqual({
        player_volume: '0.9',
      });
      expect(readSetting('player_volume')).toBe('0.9');
    });

    it('flush 窗口内 A 失败、随后 B 成功:仍然抛错(成功不得抹掉前面的失败)', async () => {
      vi.useFakeTimers();
      await boot({ player_volume: '1' });
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('first batch failed'));
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ 'x_two': '1' }, 3));

      void writeSettings({ player_volume: '0.5' }).catch(() => {});
      void writeSettings({ 'x_two': '1' }).catch(() => {});
      await expect(flushSettings()).rejects.toThrow('first batch failed');
    });
  });

  describe('恢复默认设置', () => {

    it('重置等待已发出的在途请求结束(不让旧 patch 在重置后落盘)', async () => {
      await boot({ player_volume: '1' });
      let releaseA: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseA = () => resolve(change({ player_volume: '0.5' }, 2));
          }),
      );
      void writeSettings({ player_volume: '0.5' }).catch(() => {});
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '1' }, 9));

      const resetting = resetSettings();
      releaseA!();
      await resetting;
      const clearCalls = vi
        .mocked(invokeIpc)
        .mock.calls.filter((c) => c[0] === IPC.CLEAR_SETTINGS);
      expect(clearCalls).toHaveLength(1);
    });

    it('重置回执在途期间收到更新的外部写入:过期的重置结果不倒退配置', async () => {
      const getHandler = captureEventHandler();
      await boot({ player_volume: '1' });
      installSettingsBridge();

      // 重置发出但尚未回执。
      let releaseReset: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            // 重置回执内容很旧(revision 2):它已经过期。
            releaseReset = () => resolve(change({ player_volume: '1' }, 2, {}));
          }),
      );
      const resetting = resetSettings();

      // 另一窗口的写入/外部编辑带来更高 revision 的快照。
      getHandler()({
        payload: {
          snapshot: snapshot({ player_volume: '0.35' }, 9),
          keys: ['player_volume'],
          restart_required: [],
          apply_failed: [],
        },
      });
      expect(readSetting('player_volume')).toBe('0.35');

      releaseReset!();
      await resetting;

      // 过期的重置回执不得把 revision/值倒退回 2 的那份默认值。
      expect(readSetting('player_volume')).toBe('0.35');
    });
  });

  describe('跨窗口同步', () => {

    it('他窗口触发的重置(代次推进)丢弃本窗口旧代次的待提交', async () => {
      vi.useFakeTimers();
      const getHandler = captureEventHandler();
      await boot({ player_volume: '1' });
      installSettingsBridge();

      // 本窗口有一份尚未发送的旧代次预览。
      const stale = writeSettings({ player_volume: '0.2' }, { debounce: true });
      expect(readSetting('player_volume')).toBe('0.2');

      // 其他窗口完成重置:事件带新一代次与默认快照。
      getHandler()({
        payload: {
          snapshot: snapshot({ player_volume: '1' }, 20, 1),
          keys: [],
          restart_required: [],
          apply_failed: [],
        },
      });

      // 旧代次的待提交被取消(等待者 resolve,不算失败),显示值回到默认。
      await expect(stale).resolves.toBeUndefined();
      expect(readSetting('player_volume')).toBe('1');

      // 且永远不会再落盘。
      await vi.advanceTimersByTimeAsync(2000);
      expect(setAppSettingsCalls()).toHaveLength(0);
    });
  });
});
