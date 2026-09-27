import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

// 调用方已完成构建证明和CDP身份校验；所有写入仅面向其受控夹具及验收库。
export async function verifyThumbnails(ctx, cdp, api) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, waitChannelCount, walkDirectoryTree } = api;
  const invoke = (cmd, args) => ipc(cdp, cmd, args, ctx.timeoutMs);
  const hash = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  const video = path.resolve('target/thumbnail-acceptance/bbb-h264-720-10s.mp4');
  const jpeg = path.resolve('target/native-vpl/worker-smoke/oriented-icc.jpg');
  const originals = ctx.fixtures.allMediaItems.map(({ file, sha256 }) => ({ file, sha256 }));
  await runStep('thumbnail:fixtures-and-settings', async () => {
    for (const [source, name] of [[video, 'sample.mp4'], [jpeg, 'oriented.jpg']]) {
      const destination = path.join(ctx.mediaDir, name);
      fs.copyFileSync(source, destination);
      originals.push({ file: destination, sha256: hash(destination) });
    }
    fs.writeFileSync(path.join(ctx.mediaDir, 'broken.jpg'), 'invalid jpeg fixture');
    const snapshot = await invoke('get_settings_snapshot');
    await invoke('set_app_settings', {
      generation: snapshot.generation,
      patch: { thumb_strategy: 'gpu', thumb_size: '128', thumb_skip_max_kb: '0', enable_video_cover: 'true' },
    });
    return { sourceCount: originals.length, sourceManifestHash: createHash("sha256").update(JSON.stringify(originals)).digest("hex"), broken: 'broken.jpg', png: ctx.fixtures.allMediaItems.length };
  });
  const root = await runStep('thumbnail:scan-mixed-root', async () => {
    const root = await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Thumbnail Acceptance' });
    const channel = await openChannel(cdp);
    const scanning = invoke('start_scan', {
      rootId: root.id, runId: 'thumb-acceptance-' + Date.now(), onProgress: channel,
      groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false,
    });
    const observed = waitForChannel(cdp, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, 'thumbnail scan');
    await scanning;
    const { hit } = await observed;
    if (hit.type !== 'completed') throw new Error(JSON.stringify(hit));
    return { id: root.id, scanned: hit.totalItems };
  });
  const files = [];
  for (const directory of await walkDirectoryTree(cdp, root.id, ctx.timeoutMs)) {
    files.push(...await invoke('list_directory_files', { directoryId: directory.id, limit: 500, offset: 0, categories: null }));
  }
  const ids = files.map((f) => f.id);
  if (ids.length !== ctx.fixtures.allMediaItems.length + 3) throw new Error('Mixed fixture membership mismatch: ' + ids.length);
  await evaluate(cdp, `(() => {
    window.__thumbAcceptanceEvents = [];
    return window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
      event: 'thumb:gen_progress', target: { kind: 'Any' },
      handler: window.__TAURI_INTERNALS__.transformCallback(e => window.__thumbAcceptanceEvents.push(e.payload))
    });
  })()`);
  await runStep('thumbnail:full-and-viewport', async () => {
    const started = Date.now();
    const generation = invoke('start_full_thumbnail_generation');
    // 全库命令仍在运行时发视口需求；实际是否重叠由事件和日志取证。
    const channel = await openChannel(cdp);
    const viewport = invoke('batch_request_thumbnails', {
      itemIds: ids, targetSize: 128, requestId: 'thumbnail-acceptance-' + started, onResult: channel,
    });
    const replies = waitChannelCount(cdp, ids.length, ctx.timeoutMs, 'thumbnail viewport');
    await Promise.all([generation, viewport]);
    const seen = await replies;
    let progress = await invoke('full_thumb_gen_status');
    while (progress.status === 'running' && Date.now() - started < ctx.timeoutMs) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      progress = await invoke('full_thumb_gen_status');
    }
    const events = await evaluate(cdp, 'window.__thumbAcceptanceEvents');
    if (progress.status !== 'completed') throw new Error('Full generation: ' + JSON.stringify(progress));
    const r = progress.results;
    if (r.available !== r.newlyGenerated + r.cacheHit || progress.generated !== r.available + r.direct + r.failed + r.temporarilyUnavailable) {
      throw new Error('Result conservation: ' + JSON.stringify(progress));
    }
    if (progress.generated !== progress.total) throw new Error('Incomplete fixed membership');
    const returned = new Set(seen.map((e) => e.itemId));
    if (ids.some((id) => !returned.has(id))) throw new Error('Viewport result missing');
    return { elapsedMs: Date.now() - started, progress, events, viewport: seen };
  });
  const waitGeneration = async () => {
    const deadline = Date.now() + ctx.timeoutMs;
    let current = await invoke('full_thumb_gen_status');
    while (current.status === 'running' && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      current = await invoke('full_thumb_gen_status');
    }
    return current;
  };
  await runStep('thumbnail:cache-reuse', async () => {
    const cache = await invoke('get_thumb_cache_dir');
    const artifacts = [];
    for (const item of files) {
      const detail = await invoke('get_media_detail', { id: item.id });
      if (detail.thumbStatus !== 1) continue;
      const file = path.resolve(cache, 'thumbnails', detail.thumbPath);
      artifacts.push({ id: item.id, file, thumbPath: detail.thumbPath, sha256: hash(file), mtimeMs: fs.statSync(file).mtimeMs });
    }
    const seen = [];
    const tiers = new Set(artifacts.map((a) => Number(a.thumbPath.split('/')[0])));
    for (const targetSize of tiers) {
      const group = artifacts.filter((a) => Number(a.thumbPath.split('/')[0]) === targetSize);
      const channel = await openChannel(cdp);
      await invoke('batch_request_thumbnails', {
        itemIds: group.map((a) => a.id), targetSize,
        requestId: 'thumbnail-cache-' + targetSize + '-' + Date.now(), onResult: channel,
      });
      seen.push(...await waitChannelCount(cdp, group.length, ctx.timeoutMs, 'cached viewport'));
    }
    for (const artifact of artifacts) {
      const reply = seen.find((r) => r.itemId === artifact.id);
      if (reply?.thumbStatus !== 1 || reply.thumbPath !== artifact.thumbPath || reply.pending ||
          hash(artifact.file) !== artifact.sha256 || fs.statSync(artifact.file).mtimeMs !== artifact.mtimeMs) {
        throw new Error('Cached artifact changed: ' + artifact.id);
      }
    }
    return { reused: artifacts.length, unchangedFiles: artifacts.length };
  });
  if (api.beforeCancel) await runStep('thumbnail:attach-native-debuggers', api.beforeCancel);
  await runStep('thumbnail:cancel-and-resume', async () => {
    await invoke('clear_all_thumbnails');
    await invoke('start_full_thumbnail_generation');
    const before = await invoke('full_thumb_gen_status');
    if (before.status !== 'running') throw new Error('Fixture completed before cancellation; increase --media');
    await invoke('stop_full_thumbnail_generation');
    const stopped = await waitGeneration();
    if (stopped.status !== 'cancelled') throw new Error('Cancellation did not settle: ' + JSON.stringify(stopped));
    await invoke('start_incremental_thumbnail_generation');
    const resumed = await waitGeneration();
    if (resumed.status !== 'completed' || resumed.generated !== resumed.total) throw new Error('Resume incomplete: ' + JSON.stringify(resumed));
    return { before, stopped, resumed };
  });
  await runStep('thumbnail:published-artifacts', async () => {
    const cache = await invoke('get_thumb_cache_dir');
    const results = [];
    for (const item of files) {
      const detail = await invoke('get_media_detail', { id: item.id });
      if (item.fileName === 'broken.jpg') {
        if (detail.thumbStatus !== 2) throw new Error('Broken input not closed as failed: ' + JSON.stringify(detail));
      } else {
        if (detail.thumbStatus !== 1 || !detail.thumbPath) throw new Error('Missing generated artifact: ' + JSON.stringify(detail));
        const output = path.resolve(cache, 'thumbnails', detail.thumbPath);
        const relative = path.relative(path.resolve(cache), output);
        if (relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('Artifact escaped cache');
        if (!fs.existsSync(output)) throw new Error('Artifact missing: ' + output);
      }
      results.push({ id: item.id, name: item.fileName, status: detail.thumbStatus, path: detail.thumbPath });
    }
    for (const original of originals) {
      if (hash(original.file) !== original.sha256) throw new Error('Source changed: ' + original.file);
    }
    if (cdp.exceptions.length) throw new Error("Page exception: " + JSON.stringify(cdp.exceptions[0]));
    return results;
  });
  await runStep('thumbnail:cpu-gpu-sample', async () => {
    const measurements = [];
    for (const strategy of ['cpu', 'gpu']) {
      const settings = await invoke('get_settings_snapshot');
      await invoke('set_app_settings', { generation: settings.generation, patch: { thumb_strategy: strategy } });
      const start = Date.now();
      await invoke('start_full_thumbnail_generation');
      const roundTrips = [];
      let current;
      do {
        const requested = performance.now();
        current = await invoke('full_thumb_gen_status');
        roundTrips.push(performance.now() - requested);
        if (current.status !== 'running') break;
        await new Promise((resolve) => setTimeout(resolve, 200));
      } while (Date.now() - start < ctx.timeoutMs);
      if (current.status !== 'completed' || current.generated !== current.total || current.total !== ids.length ||
          current.results.newlyGenerated !== ids.length - 1 || current.results.failed !== 1 ||
          current.results.temporarilyUnavailable !== 0) {
        throw new Error(strategy + ' sample did not publish every valid fixture: ' + JSON.stringify(current));
      }
      if (strategy === 'cpu' && current.executions.some((e) => ['imageD2d', 'imageVpl'].includes(e.native?.backend))) {
        throw new Error('CPU strategy published a GPU image execution');
      }
      measurements.push({ strategy, elapsedMs: Date.now() - start, statusIpcMaxMs: Math.max(...roundTrips),
        statusIpcSamples: roundTrips.length, documentFocused: await evaluate(cdp, 'document.hasFocus()'), progress: current });
    }
    return { scope: 'single small warm-filesystem sample; full regeneration for each strategy, no speedup claim', measurements };
  });
  await runStep('thumbnail:direct-display', async () => {
    const settings = await invoke('get_settings_snapshot');
    await invoke('set_app_settings', {
      generation: settings.generation,
      patch: { thumb_strategy: 'direct', enable_video_cover: 'false' },
    });
    await invoke('start_full_thumbnail_generation');
    const current = await waitGeneration();
    const imageCount = files.filter((item) => item.fileName !== 'sample.mp4').length;
    if (current.status !== 'completed' || current.total !== imageCount ||
        current.generated !== imageCount || current.results.direct !== imageCount ||
        current.results.newlyGenerated !== 0 || current.executions.length !== 0) {
      throw new Error('Direct mode generated artifacts: ' + JSON.stringify(current));
    }
    for (const original of originals) {
      if (hash(original.file) !== original.sha256) throw new Error('Source changed: ' + original.file);
    }
    return current;
  });
  await runStep('thumbnail:backend-filter-reload', async () => {
    const prefix = 'thumbnail-filter-' + Date.now();
    const markers = { hiddenInfo: prefix + '-hidden-info', visibleWarn: prefix + '-warn', visibleInfo: prefix + '-info' };
    try {
      const settings = await invoke('get_settings_snapshot');
      await invoke('set_app_settings', { generation: settings.generation, patch: { log_level: 'warn' } });
      await invoke('log_frontend_events', { events: [
        { level: 'info', msg: markers.hiddenInfo, source: 'thumbnail-acceptance' },
        { level: 'warn', msg: markers.visibleWarn, source: 'thumbnail-acceptance' },
      ] });
    } finally {
      const settings = await invoke('get_settings_snapshot');
      await invoke('set_app_settings', { generation: settings.generation, patch: { log_level: 'info' } });
    }
    await invoke('log_frontend_events', { events: [
      { level: 'info', msg: markers.visibleInfo, source: 'thumbnail-acceptance' },
    ] });
    // 文件层在正常退出后验证，避免把未刷盘误判为被过滤。
    ctx.thumbnailFilterMarkers = markers;
    return { markers, scope: 'actual backend EnvFilter reload through settings and frontend log bridge' };
  });

}
