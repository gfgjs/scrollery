// 原生激活由用户完成；只观察 Tauri 焦点与真实生成回执，不注入系统焦点。
export async function verifyThumbnailManualFocus(ctx, main, api) {
  const { ipc, evaluate, runStep, openChannel, waitForChannel, attachLogs } = api;
  const invoke = (name, args) => ipc(main, name, args, ctx.timeoutMs);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const prompt = (cdp, message) => evaluate(cdp, `(() => {
    let box = document.getElementById('manual-focus-instruction');
    if (!box) { box = document.createElement('div'); box.id = 'manual-focus-instruction';
      box.style.cssText = 'position:fixed;top:40px;left:10%;width:80%;z-index:2147483647;padding:24px;background:#fff3bd;color:#161616;font-size:22px;border:3px solid #927000';
      document.body.appendChild(box); }
    box.textContent = ${JSON.stringify(message)};
  })()`);
  let logs;
  // Windows 的 is_focused 查询读取 Tao 缓存，而真实 Focused 事件由 WebView2 合成。
  // 点击 WebView 时二者可不同；用窗口事件与当前 DOM 焦点交叉观察，最终再核对后端 QoS。
  const observeWindow = (cdp, label) => evaluate(cdp, `(async () => {
    window.__manualNativeFocus = { focused: null, events: [] };
    for (const [event, focused] of [['tauri://focus', true], ['tauri://blur', false]]) {
      await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
        event, target: {kind:'Window',label:${JSON.stringify(label)}},
        handler: window.__TAURI_INTERNALS__.transformCallback(() => {
          window.__manualNativeFocus.focused = focused;
          window.__manualNativeFocus.events.push({at:Date.now(),focused});
        })
      });
    }
  })()`);
  const snapshot = (cdp) => evaluate(cdp, '({dom:document.hasFocus(),...window.__manualNativeFocus})');
  const focused = async () => {
    const mainWindow = await snapshot(main);
    const logWindow = logs ? await snapshot(logs.cdp) : { dom: false, focused: false, events: [] };
    return { main: mainWindow.dom, logs: logWindow.dom, windows: { main: mainWindow, logs: logWindow } };
  };
  const awaitFocus = async (label, predicate) => {
    const deadline = Date.now() + 90000;
    let stableSince = null;
    let last;
    while (Date.now() < deadline) {
      const value = await focused();
      last = value;
      if (predicate(value)) {
        stableSince ??= Date.now();
        if (Date.now() - stableSince >= 500) return { ...value, observedAt: Date.now(), label };
      } else stableSince = null;
      await sleep(100);
    }
    throw new Error('未观察到用户切窗: ' + label + ' ' + JSON.stringify(last));
  };
  const settings = await invoke('get_settings_snapshot');
  await invoke('set_app_settings', { generation: settings.generation, patch: {
    thumb_strategy: 'cpu', thumb_size: '512', thumb_skip_max_kb: '0', enable_video_cover: 'false',
  } });
  const root = await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Manual Focus Acceptance' });
  const channel = await openChannel(main);
  await invoke('start_scan', { rootId: root.id, runId: 'manual-focus-' + Date.now(), onProgress: channel,
    groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false });
  const { hit } = await waitForChannel(main, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, 'focus scan');
  if (hit.type !== 'completed') throw new Error('Focus fixture scan failed');
  await evaluate(main, `window.__manualFocusEvents = []; window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
    event:'thumb:gen_progress', target:{kind:'Any'}, handler:window.__TAURI_INTERNALS__.transformCallback(e => window.__manualFocusEvents.push(e.payload))
  })`);
  const rounds = [];
  const generate = async (focus) => {
    await evaluate(main, 'window.__manualFocusEvents.length = 0');
    const startTimeMs = Date.now();
    await invoke('start_full_thumbnail_generation');
    const deadline = Date.now() + ctx.timeoutMs;
    let running = false;
    let terminal;
    do {
      terminal = await invoke('full_thumb_gen_status');
      running ||= terminal.status === 'running' || await evaluate(main, "window.__manualFocusEvents.some(e => e.status === 'running')");
      if (running && terminal.status !== 'running') break;
      await sleep(100);
    } while (Date.now() < deadline);
    const after = await focused();
    if (after.main !== focus.main || after.logs !== focus.logs) throw new Error('生成期间焦点变化: ' + JSON.stringify({ focus, after }));
    const count = ctx.fixtures.items.length + ctx.fixtures.movable.items.length;
    if (!running || terminal.status !== 'completed' || terminal.total !== count || terminal.results.newlyGenerated !== count || terminal.results.failed !== 0 || terminal.results.temporarilyUnavailable !== 0) {
      throw new Error('焦点轮生成未完成: ' + JSON.stringify(terminal));
    }
    const result = { focus, after, startTimeMs, endTimeMs: Date.now(), terminal };
    rounds.push(result);
    return result;
  };
  try {
    await observeWindow(main, 'main');
    await prompt(main, '焦点验收 1/3：请点进这个主窗口，并停留。完成后会出现日志窗口。');
    await runStep('thumbnail-manual-focus:main', async () => generate(await awaitFocus('main', (s) => s.main && !s.logs)));
    await invoke('open_log_window');
    logs = await attachLogs();
    await observeWindow(logs.cdp, 'logs');
    await prompt(main, '焦点验收 2/3：请点进 Scrollery — Logs 日志窗口。');
    await prompt(logs.cdp, '焦点验收 2/3：请点进这个日志窗口，并停留，等待下一条提示。');
    await runStep('thumbnail-manual-focus:logs', async () => generate(await awaitFocus('logs', (s) => !s.main && s.logs)));
    await prompt(logs.cdp, '焦点验收 3/3：现在请切回 Codex，并停留。验收结束后本窗口会自动关闭。');
    await runStep('thumbnail-manual-focus:external', async () => generate(await awaitFocus('external', (s) => !s.main && !s.logs)));
    ctx.manualFocusRounds = rounds;
  } finally { logs?.close(); }
}
