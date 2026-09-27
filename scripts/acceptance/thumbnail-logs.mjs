import fs from 'node:fs';
import path from 'node:path';

export async function verifyThumbnailLogs(ctx, main, api) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, attachLogs } = api;
  const invoke = (command, args) => ipc(main, command, args, ctx.timeoutMs);
  await evaluate(main, "location.hash = '#/settings/media'");
  const settings = await invoke('get_settings_snapshot');
  await invoke('set_app_settings', { generation: settings.generation,
    patch: { thumb_strategy: 'cpu', thumb_size: '128', thumb_skip_max_kb: '0', enable_video_cover: 'false' } });
  fs.writeFileSync(path.join(ctx.mediaDir, 'broken.jpg'), 'invalid jpeg fixture');
  const root = await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Thumbnail Logs' });
  const channel = await openChannel(main);
  await invoke('start_scan', { rootId: root.id, runId: 'logs-' + Date.now(), onProgress: channel,
    groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false });
  const { hit } = await waitForChannel(main, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, 'log scan');
  if (hit.type !== 'completed') throw new Error('Log fixture scan failed');
  await invoke('open_log_window');
  const session = await attachLogs();
  const logs = session.cdp;
  try {
    await logs.send('Runtime.enable');
    await runStep('thumbnail-logs:window-mounted', async () => {
      const deadline = Date.now() + 5000;
      let probe;
      do {
        probe = await evaluate(logs, "({ mounted: !!document.querySelector('.log-window'), label: window.__TAURI_INTERNALS__.metadata.currentWindow.label, focused: document.hasFocus() })");
        if (probe.mounted) break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      } while (Date.now() < deadline);
      if (!probe.mounted || probe.label !== 'logs') throw new Error('Log window not mounted');
      return { ...probe, mainFocused: await evaluate(main, 'document.hasFocus()') };
    });
    await evaluate(main, "location.hash = '#/collections'");
    const routeDeadline = Date.now() + 5000;
    while (!await evaluate(main, "!!document.querySelector('.app-statusbar')")) {
      if (Date.now() > routeDeadline) throw new Error('Main statusbar route did not mount');
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    await invoke('start_full_thumbnail_generation');
    await runStep('thumbnail-logs:running-statusbar', async () => {
      const deadline = Date.now() + ctx.timeoutMs;
      const texts = new Set();
      let status;
      do {
        const text = await evaluate(main, "document.querySelector('.statusbar__scanning')?.innerText || ''");
        if (text) texts.add(text);
        status = await invoke('full_thumb_gen_status');
        if (status.status !== 'running') break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      } while (Date.now() < deadline);
      if (!texts.size || status.status !== 'completed' || status.generated !== status.total || status.results.failed !== 1) throw new Error('Missing running UI or terminal result: ' + JSON.stringify({ texts: [...texts], status }));
      return { texts: [...texts], status };
    });
    for (const level of ['WARN', 'INFO']) {
      await runStep('thumbnail-logs:filter-' + level.toLowerCase(), async () => {
        await evaluate(logs, `(() => {
          for (const label of document.querySelectorAll('.log-window__filters .checkbox-field')) {
            const input = label.querySelector('input');
            if (input.checked !== (label.innerText.trim() === ${JSON.stringify(level)})) input.click();
          }
        })()`);
        const deadline = Date.now() + 5000;
        let rows;
        do {
          rows = await evaluate(logs, "Array.from(document.querySelectorAll('.log-row')).map(e => ({ level: e.querySelector('.log-row__level')?.innerText, message: e.querySelector('.log-row__msg')?.innerText }))");
          if (rows.length && rows.every((row) => row.level === level)) break;
          await new Promise((resolve) => setTimeout(resolve, 100));
        } while (Date.now() < deadline);
        if (!rows.length || rows.some((row) => row.level !== level)) throw new Error('Incorrect visible level: ' + JSON.stringify(rows));
        return { level, visible: rows.length, messages: rows.map((row) => row.message) };
      });
    }
    if (logs.exceptions.length || main.exceptions.length) throw new Error('Page exception during log verification');
  } finally { session.close(); }
}
