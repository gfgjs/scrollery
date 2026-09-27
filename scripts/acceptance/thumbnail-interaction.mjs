import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

// 仅在驱动完成独立实例身份校验后运行；复用已授权的媒体及播放入口。
export async function verifyThumbnailInteraction(ctx, main, api) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree, attachLogs } = api;
  const invoke = (command, args) => ipc(main, command, args, ctx.timeoutMs);
  const sample = path.join(ctx.mediaDir, 'sample.mp4');
  fs.copyFileSync(path.resolve('target/thumbnail-acceptance/bbb-h264-720-10s.mp4'), sample);
  const hash = () => createHash('sha256').update(fs.readFileSync(sample)).digest('hex');
  const sourceHash = hash();
  const settings = await invoke('get_settings_snapshot');
  await invoke('set_app_settings', { generation: settings.generation, patch: {
    thumb_strategy: 'gpu', thumb_size: '128', thumb_skip_max_kb: '0', enable_video_cover: 'true',
  } });
  const root = await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Thumbnail Interaction' });
  const channel = await openChannel(main);
  await invoke('start_scan', { rootId: root.id, runId: 'interaction-' + Date.now(), onProgress: channel,
    groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false });
  const { hit } = await waitForChannel(main, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, 'interaction scan');
  if (hit.type !== 'completed') throw new Error('Interaction scan failed');
  const files = [];
  for (const directory of await walkDirectoryTree(main, root.id, ctx.timeoutMs)) {
    files.push(...await invoke('list_directory_files', { directoryId: directory.id, limit: 500, offset: 0, categories: null }));
  }
  const video = files.find((item) => item.fileName === 'sample.mp4');
  if (!video) throw new Error('Video fixture not found: ' + JSON.stringify(files[0]));
  await runStep('thumbnail-interaction:playback-during-generation', async () => {
    const resolution = await invoke('resolve_video_playback', { itemId: video.id });
    if (resolution.mode !== 'direct' || path.resolve(resolution.src) !== path.resolve(sample)) throw new Error('Unexpected playback resolution: ' + JSON.stringify(resolution));
    try {
      await evaluate(main, `(async () => {
        const video = document.createElement('video');
        video.id = 'thumbnail-acceptance-playback'; video.muted = true; video.loop = true;
        video.style.cssText = 'position:fixed;bottom:0;right:0;width:240px;z-index:2147483647';
        video.src = window.__TAURI_INTERNALS__.convertFileSrc(${JSON.stringify(resolution.src)}, 'asset');
        document.body.appendChild(video); await video.play();
      })()`);
      await invoke('start_full_thumbnail_generation');
      const samples = [];
      const deadline = Date.now() + ctx.timeoutMs;
      let progress;
      do {
        progress = await invoke('full_thumb_gen_status');
        const playback = await evaluate(main, `(() => { const v=document.getElementById('thumbnail-acceptance-playback'); return { time:v.currentTime, paused:v.paused, ready:v.readyState, error:v.error?.code ?? null, frames:v.getVideoPlaybackQuality().totalVideoFrames }; })()`);
        samples.push({ generation: progress.status, generated: progress.generated, ...playback });
        if (playback.error) throw new Error('Playback failed: ' + JSON.stringify(playback));
        if (progress.status !== 'running') break;
        await new Promise((resolve) => setTimeout(resolve, 150));
      } while (Date.now() < deadline);
      const overlap = samples.filter((s) => s.generation === 'running' && !s.paused && s.ready >= 2);
      if (overlap.length < 2 || overlap.at(-1).frames <= overlap[0].frames) throw new Error('No decoded playback progress during generation: ' + JSON.stringify(samples));
      if (progress.status !== 'completed' || progress.generated !== files.length || progress.results.newlyGenerated !== files.length || progress.results.failed !== 0) throw new Error('Generation failed: ' + JSON.stringify(progress));
      if (hash() !== sourceHash) throw new Error('Video source changed');
      return { resolution, sourceHash, progress, samples, scope: 'existing playback resolver and WebView asset video decode; excludes player control UI and transcoding formats' };
    } finally {
      await evaluate(main, "(() => { const v=document.getElementById('thumbnail-acceptance-playback'); if(v){v.pause();v.removeAttribute('src');v.load();v.remove();} })()");
    }
  });
  await runStep('thumbnail-interaction:window-focus-observation', async () => {
    await main.send('Page.bringToFront');
    const mainBefore = await evaluate(main, "window.__TAURI_INTERNALS__.invoke('plugin:window|is_focused', {label:'main'})");
    await invoke('open_log_window');
    const session = await attachLogs();
    try {
      await session.cdp.send('Page.bringToFront');
      const mainAfter = await evaluate(main, "window.__TAURI_INTERNALS__.invoke('plugin:window|is_focused', {label:'main'})");
      const logsAfter = await evaluate(session.cdp, "window.__TAURI_INTERNALS__.invoke('plugin:window|is_focused', {label:'logs'})");
      return { mainBefore, mainAfter, logsAfter, verified: mainBefore && !mainAfter && logsAfter,
        scope: 'Tauri native window focus query after activation; unavailable activation is not a pass of focus/QoS acceptance' };
    } finally { session.close(); }
  });
}
