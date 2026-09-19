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
  onSettingsApplied,
  readSetting,
  resetSettings,
  resetSettingsModuleStateForTests,
  setSettingsErrorReporter,
  settingsApplyFailed,
  settingsReady,
  settingsRestartRequired,
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

    it('initializeSettings 应用设置快照但不动首启/引导状态', async () => {
      vi.mocked(invokeIpc).mockResolvedValueOnce({
        settings: snapshot({ appearance: 'dark' }, 4),
        state: { firstLaunch: 'false', guideSeen: 'true' },
      });
      const payload = await initializeSettings();

      expect(settingsReady.value).toBe(true);
      expect(readSetting('appearance')).toBe('dark');
      expect(payload.state).toEqual({ firstLaunch: 'false', guideSeen: 'true' });
      // 引导标记不进设置值表:重置设置不得重放首启/引导。
      expect(readSetting('first_launch')).toBeUndefined();
      expect(readSetting('guide_seen')).toBeUndefined();
    });

    it('initializeSettings 只发一次请求(多处调用共享同一 Promise)', async () => {
      vi.mocked(invokeIpc).mockResolvedValue({
        settings: snapshot({}, 1),
        state: { firstLaunch: null, guideSeen: null },
      });
      await Promise.all([initializeSettings(), initializeSettings(), initializeSettings()]);
      const startupCalls = vi
        .mocked(invokeIpc)
        .mock.calls.filter((c) => c[0] === IPC.GET_STARTUP_CONFIG);
      expect(startupCalls).toHaveLength(1);
    });

    it('初始快照读取失败保持未就绪,并允许重试', async () => {
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('boom'));
      await expect(initializeSettings()).rejects.toThrow('boom');
      expect(settingsReady.value).toBe(false);

      vi.mocked(invokeIpc).mockResolvedValueOnce({
        settings: snapshot({ player_volume: '0.2' }, 1),
        state: { firstLaunch: null, guideSeen: null },
      });
      await initializeSettings();
      expect(readSetting('player_volume')).toBe('0.2');
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

    it('监听注册先于取初始快照(注册未完成的窗口期不丢事件)', async () => {
      // listen 注册是异步的:取快照必须等它完成,否则注册窗口期内的重置事件会丢失。
      let registered = false;
      let releaseRegistration: (() => void) | null = null;
      vi.mocked(listenAppEvent).mockImplementation(
        () =>
          new Promise((resolve) => {
            releaseRegistration = () => {
              registered = true;
              resolve(() => {});
            };
          }),
      );
      vi.mocked(invokeIpc).mockResolvedValueOnce({
        settings: snapshot({ player_volume: '1' }, 1),
        state: { firstLaunch: null, guideSeen: null },
      });

      const booting = initializeSettings();
      await Promise.resolve();
      // 注册尚未完成时,快照请求还不能发出。
      expect(
        vi.mocked(invokeIpc).mock.calls.filter((c) => c[0] === IPC.GET_STARTUP_CONFIG),
      ).toHaveLength(0);

      releaseRegistration!();
      await booting;
      expect(registered).toBe(true);
      expect(
        vi.mocked(invokeIpc).mock.calls.filter((c) => c[0] === IPC.GET_STARTUP_CONFIG),
      ).toHaveLength(1);
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

    it('防抖写入同样在失败时回滚并 reject(不再静默 resolve)', async () => {
      vi.useFakeTimers();
      await boot({ selection_bar_offset: '{"x":0,"y":0}' });
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('debounce failed'));

      const promise = writeSettings(
        { selection_bar_offset: '{"x":10,"y":0}' },
        { debounce: true },
      );
      expect(readSetting('selection_bar_offset')).toBe('{"x":10,"y":0}');

      await vi.advanceTimersByTimeAsync(400);
      await expect(promise).rejects.toThrow('debounce failed');
      expect(readSetting('selection_bar_offset')).toBe('{"x":0,"y":0}');
    });

    it('连续防抖操作合并为一次提交,且最长 1s 强制落盘', async () => {
      vi.useFakeTimers();
      await boot();
      vi.mocked(invokeIpc).mockResolvedValue(change({ player_volume: '0.3' }, 2));

      const first = writeSettings({ player_volume: '0.2' }, { debounce: true });
      await vi.advanceTimersByTimeAsync(200);
      const second = writeSettings({ player_volume: '0.3' }, { debounce: true });
      // 持续拖动:即使不满空闲 300ms,到 1s 也必须落盘一次。
      await vi.advanceTimersByTimeAsync(800);

      const calls = setAppSettingsCalls();
      expect(calls).toHaveLength(1);
      expect((calls[0][1] as { patch: Record<string, string> }).patch).toEqual({
        player_volume: '0.3',
      });
      await Promise.all([first, second]);
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

    it('权威快照不吞掉尚未发送的新预览(拖拽不被打断)', async () => {
      vi.useFakeTimers();
      const getHandler = captureEventHandler();
      await boot({ player_volume: '1' });
      installSettingsBridge();

      // 一次防抖写入把它留在待提交集合(未发送)。
      const dragged = writeSettings({ player_volume: '0.7' }, { debounce: true });
      expect(readSetting('player_volume')).toBe('0.7');

      // 此时外部编辑事件带来权威快照:不能把拖拽中的预览抹掉。
      getHandler()({
        payload: {
          snapshot: snapshot({ player_volume: '1', appearance: 'dark' }, 5),
          keys: ['appearance'],
          restart_required: [],
          apply_failed: [],
        },
      });
      // 权威的非相关键生效,未确认的预览仍在。
      expect(readSetting('appearance')).toBe('dark');
      expect(readSetting('player_volume')).toBe('0.7');

      vi.mocked(invokeIpc).mockResolvedValue(change({ player_volume: '0.7' }, 6));
      await vi.advanceTimersByTimeAsync(400);
      await dragged;
    });

    it('过期回执(revision 更低)不被采纳', async () => {
      await boot({ player_volume: '1' });
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.9' }, 0));
      await writeSettings({ player_volume: '0.9' });
      // 回执 revision 低于当前(1):丢弃,值表回到确认值。
      expect(readSetting('player_volume')).toBe('1');
    });

    it('apply_failed 如实上抛并提示(不报成完全成功)', async () => {
      await boot({ thumb_size: '512' });
      const reporter = vi.fn();
      setSettingsErrorReporter(reporter);
      vi.mocked(invokeIpc).mockResolvedValueOnce(
        change({ thumb_size: '256' }, 2, { apply_failed: ['thumb_size'] }),
      );

      await writeSettings({ thumb_size: '256' });

      expect(readSetting('thumb_size')).toBe('256');
      expect(settingsApplyFailed.value).toEqual(['thumb_size']);
      expect(reporter).toHaveBeenCalledTimes(1);
    });

    it('onSettingsApplied 在每次应用权威快照时回调,并可取消订阅', async () => {
      await boot();
      const seen: number[] = [];
      const off = onSettingsApplied((s) => seen.push(s.revision));
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.5' }, 7));
      await writeSettings({ player_volume: '0.5' });
      off();
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.6' }, 8));
      await writeSettings({ player_volume: '0.6' });
      expect(seen).toEqual([7]);
    });
  });

  describe('flush', () => {
    it('flush 提交待保存改动并等待落盘', async () => {
      vi.useFakeTimers();
      await boot();
      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.4' }, 2));
      void writeSettings({ player_volume: '0.4' }, { debounce: true });

      // 不等空闲期:flush 必须自己把待提交项发出去。
      await flushSettings();
      expect(setAppSettingsCalls()).toHaveLength(1);
      expect(readSetting('player_volume')).toBe('0.4');
    });

    it('flush 对在等待窗口内失败的写盘可靠抛错(退出流程据此拒绝退出)', async () => {
      await boot();
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('disk full'));
      // 消费方按既有姿态吞掉 rejection,失败仍需经 flush 上报。
      void writeSettings({ player_volume: '0.5' }).catch(() => {});
      await expect(flushSettings()).rejects.toThrow('disk full');
    });

    it('flush 无待保存内容且无在途请求时直接完成', async () => {
      await boot();
      await expect(flushSettings()).resolves.toBeUndefined();
      expect(setAppSettingsCalls()).toHaveLength(0);
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
    it('重置取消未发送的防抖预览,并以返回快照替换显示值', async () => {
      vi.useFakeTimers();
      await boot({ player_volume: '0.7', appearance: 'dark' });
      // 未发送的预览:重置后不得写回、也不得继续显示。
      void writeSettings({ player_volume: '0.1' }, { debounce: true });
      expect(readSetting('player_volume')).toBe('0.1');

      vi.mocked(invokeIpc).mockResolvedValueOnce(
        change({ player_volume: '1', appearance: 'system' }, 10),
      );
      const result = await resetSettings();

      expect(result.snapshot.revision).toBe(10);
      expect(readSetting('player_volume')).toBe('1');
      expect(readSetting('appearance')).toBe('system');
      // 已取消的预览没有落盘。
      expect(setAppSettingsCalls()).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(1500);
      expect(setAppSettingsCalls()).toHaveLength(0);
    });

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

    it('重置失败保留最后确认值并抛出', async () => {
      await boot({ player_volume: '0.6' });
      const reporter = vi.fn();
      setSettingsErrorReporter(reporter);
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('reset failed'));

      await expect(resetSettings()).rejects.toThrow('reset failed');
      expect(readSetting('player_volume')).toBe('0.6');
      expect(reporter).toHaveBeenCalledTimes(1);
    });

    it('重置期间重复点击去重:只发一次清除请求', async () => {
      await boot();
      let releaseReset: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseReset = () => resolve(change({ player_volume: '1' }, 10));
          }),
      );

      const first = resetSettings();
      const second = resetSettings();
      expect(first).toBe(second);
      releaseReset!();
      await Promise.all([first, second]);
      const clearCalls = vi
        .mocked(invokeIpc)
        .mock.calls.filter((c) => c[0] === IPC.CLEAR_SETTINGS);
      expect(clearCalls).toHaveLength(1);
    });

    it('重置期间暂停采集;重置结束后恢复采集', async () => {
      await boot({ player_volume: '1' });
      let releaseReset: (() => void) | null = null;
      vi.mocked(invokeIpc).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            releaseReset = () => resolve(change({ player_volume: '1' }, 10));
          }),
      );

      const resetting = resetSettings();
      await writeSettings({ player_volume: '0.2' });
      // 重置在途:不采集(否则会把旧代次的值带进新代次)。
      expect(setAppSettingsCalls()).toHaveLength(0);

      releaseReset!();
      await resetting;

      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.2' }, 11));
      await writeSettings({ player_volume: '0.2' });
      expect(setAppSettingsCalls()).toHaveLength(1);
    });

    it('重置失败后 finally 恢复采集(不永久禁用保存)', async () => {
      await boot({ player_volume: '1' });
      vi.mocked(invokeIpc).mockRejectedValueOnce(new Error('reset failed'));
      await expect(resetSettings()).rejects.toThrow('reset failed');

      vi.mocked(invokeIpc).mockResolvedValueOnce(change({ player_volume: '0.3' }, 2));
      await writeSettings({ player_volume: '0.3' });
      expect(readSetting('player_volume')).toBe('0.3');
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
    it('主动重取(refresh)得到新一代次时,同样丢弃旧代次的待提交预览', async () => {
      vi.useFakeTimers();
      await boot({ player_volume: '1' });

      // 本窗口已有一份未发送的旧代次预览。
      const stale = writeSettings({ player_volume: '0.2' }, { debounce: true });
      expect(readSetting('player_volume')).toBe('0.2');

      // 主动重取(如外部编辑后的兜底刷新)拿到新一代次快照:废弃旧预览的处理与事件路径一致。
      const mod = await import('./settingsPersistence');
      vi.mocked(invokeIpc).mockResolvedValueOnce(snapshot({ player_volume: '1' }, 20, 1));
      await mod.refreshSettingsFromBackend();

      await expect(stale).resolves.toBeUndefined();
      expect(readSetting('player_volume')).toBe('1');
      await vi.advanceTimersByTimeAsync(2000);
      expect(setAppSettingsCalls()).toHaveLength(0);
    });

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

    it('事件携带完整快照时整体应用,并按 revision 丢弃更旧的事件', async () => {
      const getHandler = captureEventHandler();
      await boot({ player_volume: '1' });
      installSettingsBridge();

      getHandler()({
        payload: {
          snapshot: snapshot({ product: 'x', player_volume: '0.4' }, 30),
          keys: [],
          restart_required: [],
          apply_failed: [],
        },
      });
      expect(readSetting('product')).toBe('x');
      expect(readSetting('player_volume')).toBe('0.4');

      getHandler()({
        payload: {
          snapshot: snapshot({ product: 'y', player_volume: '0.9' }, 29),
          keys: [],
          restart_required: [],
          apply_failed: [],
        },
      });
      expect(readSetting('product')).toBe('x');
    });

    it('事件回调的键名与后端字面量一致(restart_required / apply_failed)', async () => {
      const getHandler = captureEventHandler();
      await boot();
      installSettingsBridge();
      const reporter = vi.fn();
      setSettingsErrorReporter(reporter);

      getHandler()({
        payload: {
          snapshot: snapshot({ thumb_size: '256' }, 5),
          keys: ['thumb_size'],
          restart_required: ['window_geometry'],
          apply_failed: ['thumb_size'],
        },
      });
      // 事件本身不弹「保存失败」,但两类键都要能被设置页读到。
      expect(reporter).not.toHaveBeenCalled();
      expect(settingsApplyFailed.value).toEqual(['thumb_size']);
      expect(settingsRestartRequired.value).toEqual(['window_geometry']);
    });
  });
});
