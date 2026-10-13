// settingsPersistence 回归测试(设置集中保存,2026-09-16)。
//
// 覆盖的不只是 happy path:在途 A + 新 patch B、防抖失败回滚、跨窗口 reset 丢弃旧 pending、
// 重置失败/重复点击去重、flush 错误上报、权威快照不吞未确认预览。
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useConfigStore } from './configStore'
import { getThumbCacheDir, setThumbCacheDir } from '../utils/thumbCacheDir'

vi.mock('../utils/ipc', () => ({ invokeIpc: vi.fn() }))
vi.mock('../utils/appEvents', () => ({ listenAppEvent: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('../utils/logger', () => ({
  logger: { info: vi.fn(), warn: vi.fn(), error: vi.fn() },
  setLoggerEnabled: vi.fn(),
}))

import { invokeIpc } from '../utils/ipc'
import { listenAppEvent } from '../utils/appEvents'
import { IPC } from '../constants/ipc'
import type { SettingsChange, SettingsSnapshot } from '../types/config'
import { flushSettings, initializeSettings, installSettingsBridge, readSetting, resetSettings, resetSettingsModuleStateForTests, writeSettings } from './settingsPersistence'

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
    vi.unstubAllGlobals();
  });

  it.each(['not_ready', 'resetting', 'cancelled'] as const)('目录保存 %s 不执行成功续体或改共享缓存', async (reason) => {
    setActivePinia(createPinia());
    vi.stubGlobal('document', { documentElement: { style: { setProperty: vi.fn() }, classList: { add: vi.fn(), remove: vi.fn() } } });
    const handler = captureEventHandler();
    const config = useConfigStore();
    if (reason !== 'not_ready') await boot({ thumb_cache_dir: 'old' });
    let release!: (value: SettingsChange) => void;
    vi.mocked(invokeIpc).mockImplementation(() => new Promise(resolve => { release = resolve; }));
    const resetting = reason === 'resetting' ? resetSettings() : null;
    setThumbCacheDir('old');
    const success = vi.fn(() => setThumbCacheDir('new'));
    const saving = config.saveConfig('thumb_cache_dir', 'new').then(success);
    const rejected = expect(saving).rejects.toMatchObject({ reason });
    if (reason === 'cancelled') {
      handler()({ payload: { ...change({ thumb_cache_dir: 'old' }, 2), snapshot: snapshot({ thumb_cache_dir: 'old' }, 2, 1) } });
      release(change({ thumb_cache_dir: 'new' }, 1));
    }
    await rejected;
    expect(success).not.toHaveBeenCalled();
    expect(await getThumbCacheDir()).toBe('old');
    if (resetting) { release(change({ thumb_cache_dir: 'old' }, 2)); await resetting; }
  });

  it('DOM 由中央预览更新，拒绝不改显示且落盘失败回退', async () => {
    setActivePinia(createPinia());
    const setProperty = vi.fn();
    vi.stubGlobal('document', { documentElement: { style: { setProperty }, classList: { add: vi.fn(), remove: vi.fn() } } });
    const config = useConfigStore();
    await expect(config.setUiFontSize(24)).rejects.toMatchObject({ reason: 'not_ready' });
    await nextTick();
    expect(setProperty).not.toHaveBeenCalled();
    await boot({ ui_font_size: '13' });
    let fail!: (error: Error) => void;
    vi.mocked(invokeIpc).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject; }));
    const saving = config.setUiFontSize(24);
    await nextTick();
    expect(config.uiFontSize).toBe(24);
    expect(setProperty).toHaveBeenCalled();
    setProperty.mockClear();
    const rejected = expect(saving).rejects.toThrow('disk');
    fail(new Error('disk'));
    await rejected;
    await nextTick();
    expect(config.uiFontSize).toBe(13);
    expect(setProperty).toHaveBeenCalled();
  });

  it('画廊融合配置保存后响应式生效，写入失败回退且重置恢复默认', async () => {
    setActivePinia(createPinia());
    vi.stubGlobal('document', { documentElement: { style: { setProperty: vi.fn() }, classList: { add: vi.fn(), remove: vi.fn() } } });
    const config = useConfigStore();
    const defaults = {
      gallery_blend_enabled: 'false',
      gallery_blend_edge_width: '2',
      gallery_blend_blur_radius: '3',
      gallery_blend_strength: '100',
    };
    await boot(defaults);
    expect(config.galleryBlend).toEqual({ enabled: false, edgeWidth: 2, blurRadius: 3, strength: 100 });
    let saved: Record<string, string> = { ...defaults };
    let revision = 1;
    vi.mocked(invokeIpc).mockImplementation(async (_command, args) => {
      saved = { ...saved, ...(args?.patch as Record<string, string>) };
      return change(saved, ++revision);
    });
    await config.setGalleryBlendEnabled(true);
    await config.setGalleryBlendEdgeWidth(4);
    await config.setGalleryBlendBlurRadius(12);
    await config.setGalleryBlendStrength(60);
    expect(setAppSettingsCalls().map((call) => call[1]?.patch)).toEqual([
      { gallery_blend_enabled: 'true' },
      { gallery_blend_edge_width: '4' },
      { gallery_blend_blur_radius: '12' },
      { gallery_blend_strength: '60' },
    ]);
    expect(config.galleryBlend).toEqual({ enabled: true, edgeWidth: 4, blurRadius: 12, strength: 60 });

    let fail!: (error: Error) => void;
    vi.mocked(invokeIpc).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject; }));
    const saving = config.setGalleryBlendEnabled(false);
    expect(config.galleryBlend.enabled).toBe(false);
    const rejected = expect(saving).rejects.toThrow('disk');
    fail(new Error('disk'));
    await rejected;
    expect(config.galleryBlend.enabled).toBe(true);

    vi.mocked(invokeIpc).mockResolvedValueOnce(change(defaults, ++revision, { snapshot: snapshot(defaults, revision, 1) }));
    await resetSettings();
    expect(config.galleryBlend).toEqual({ enabled: false, edgeWidth: 2, blurRadius: 3, strength: 100 });
  });

  describe('flush', () => {

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

    it('恢复默认取消已排队但未发送的修改，只等待实际在途写入', async () => {
      await boot({ thumb_webp_quality: '80', ai_batch_size: '0' });
      let releaseActive!: (value: SettingsChange) => void;
      vi.mocked(invokeIpc).mockImplementationOnce(() => new Promise((resolve) => {
        releaseActive = resolve;
      }));
      const defaults = { thumb_webp_quality: '80', ai_batch_size: '0' };
      vi.mocked(invokeIpc).mockImplementation(async (command) => {
        if (command === IPC.CLEAR_SETTINGS) {
          return change(defaults, 4, { snapshot: snapshot(defaults, 4, 1) });
        }
        return change({ thumb_webp_quality: '100', ai_batch_size: '32' }, 3);
      });

      const active = writeSettings({ thumb_webp_quality: '100' });
      const queued = writeSettings({ ai_batch_size: '32' }).then(
        () => 'saved',
        (error: unknown) => error,
      );
      expect(setAppSettingsCalls()).toHaveLength(1);
      expect(readSetting('ai_batch_size')).toBe('32');

      const resetting = resetSettings();
      releaseActive(change({ thumb_webp_quality: '100', ai_batch_size: '0' }, 2));
      await active;
      await resetting;

      expect(await queued).toMatchObject({ reason: 'cancelled' });
      expect(setAppSettingsCalls().map((call) => call[1]?.patch)).toEqual([
        { thumb_webp_quality: '100' },
      ]);
      expect(vi.mocked(invokeIpc).mock.calls.filter(([command]) => command === IPC.CLEAR_SETTINGS)).toHaveLength(1);
      expect(readSetting('thumb_webp_quality')).toBe('80');
      expect(readSetting('ai_batch_size')).toBe('0');
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

      // 取消不得进入保存成功续体，也不算磁盘故障。
      await expect(stale).rejects.toMatchObject({ reason: 'cancelled' });
      await expect(flushSettings()).resolves.toBeUndefined();
      expect(readSetting('player_volume')).toBe('1');

      // 且永远不会再落盘。
      await vi.advanceTimersByTimeAsync(2000);
      expect(setAppSettingsCalls()).toHaveLength(0);
    });
  });
});
