import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';

// 所有入口复用调用方的产物证明、独立实例身份和受控测试库。
export async function verifyThumbnailLifecycle(ctx, cdp, api, saved) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree } = api;
  const invoke = (command, args) => ipc(cdp, command, args, ctx.timeoutMs);
  const hash = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  const settled = async () => {
    const deadline = Date.now() + ctx.timeoutMs;
    let status;
    do {
      status = await invoke('full_thumb_gen_status');
      if (status.status !== 'running') return status;
      await new Promise((resolve) => setTimeout(resolve, 100));
    } while (Date.now() < deadline);
    throw new Error('Generation did not settle');
  };
  if (saved) {
    await runStep('thumbnail-lifecycle:restart-persistence', async () => {
      const roots = await invoke('list_scan_roots');
      if (roots.length !== 1 || roots[0].id !== saved.rootId) throw new Error('Root lost on restart');
      for (const item of saved.artifacts) {
        const detail = await invoke('get_media_detail', { id: item.id });
        if (detail.thumbStatus !== 1 || detail.thumbPath !== item.thumbPath || hash(item.file) !== item.hash) {
          throw new Error('Restart changed artifact: ' + item.id);
        }
      }
      const status = await invoke('full_thumb_gen_status');
      if (status.status === 'running') throw new Error('Restart revived a completed run');
      return { artifacts: saved.artifacts.length, status };
    });
    if (saved.restore) {
      await runStep('thumbnail-restore:swapped-library', async () => {
        const detail = await invoke('get_media_detail', { id: saved.probeId });
        if (detail.rating !== 4 || fs.existsSync(path.join(ctx.appDataDir, 'pending-restore.json'))) throw new Error('Backup library not restored');
        const status = await invoke('full_thumb_gen_status');
        if (status.status !== 'idle') throw new Error('Old run survived restored database');
        for (const item of ctx.fixtures.allMediaItems) {
          if (hash(item.file) !== item.sha256) throw new Error('Restore changed original media');
        }
        return { rating: detail.rating, discardedRating: 2, restoredArtifacts: saved.artifacts.length, status };
      });
      return;
    }
    if (saved.crash) {
      await runStep('thumbnail-crash:recover-leases', async () => {
        const db = new DatabaseSync(path.join(ctx.appDataDir, 'scrollery.db'), { readOnly: true });
        try {
          const stale = saved.leases.filter((lease) => db.prepare('SELECT count(*) AS n FROM media_derivations WHERE lease_id=?').get(lease).n !== 0);
          if (stale.length) throw new Error('Orphan leases survived startup: ' + JSON.stringify(stale));
        } finally { db.close(); }
        await invoke('start_incremental_thumbnail_generation');
        const status = await settled();
        if (status.status !== 'completed' || status.generated !== status.total) throw new Error('Crash recovery incomplete');
        for (const item of saved.allArtifacts) {
          const detail = await invoke('get_media_detail', { id: item.id });
          if (detail.thumbStatus !== 1 || !fs.existsSync(path.resolve(saved.cache, 'thumbnails', detail.thumbPath))) throw new Error('Unrecovered artifact: ' + item.id);
        }
        for (const item of saved.artifacts) {
          if (hash(item.file) !== item.hash) throw new Error('Previously committed artifact changed after resume');
        }
        for (const item of ctx.fixtures.allMediaItems) {
          if (hash(item.file) !== item.sha256) throw new Error('Source changed after recovery');
        }
        return { leaseKind: saved.leaseKind, oldLeasesCleared: saved.leases.length, preserved: saved.artifacts.length, allAvailable: saved.allArtifacts.length, status };
      });
      return;
    }
    await runStep('thumbnail-lifecycle:database-epoch', async () => {
      await invoke('start_full_thumbnail_generation');
      const before = await invoke('full_thumb_gen_status');
      if (before.status !== 'running') throw new Error('No active run before clear');
      await invoke('clear_database');
      const status = await settled();
      const roots = await invoke('list_scan_roots');
      if (roots.length !== 0 || status.status === 'running') throw new Error('Old database generation survived clear');
      return { before, after: status, roots: roots.length, scope: 'clear_database epoch replacement, not backup restore' };
    });
    return;
  }
  await evaluate(cdp, "location.hash = '#/settings/media'");
  const settings = await invoke('get_settings_snapshot');
  await invoke('set_app_settings', { generation: settings.generation,
    patch: { thumb_strategy: ctx.stage === 'thumbnail-ui' ? 'gpu' : 'cpu', thumb_size: '128', thumb_skip_max_kb: '0', enable_video_cover: ctx.stage === 'thumbnail-video-crash' ? 'true' : 'false', ...(ctx.stage === 'thumbnail-ui' ? { pinned_settings: JSON.stringify(['fullThumbGen']) } : {}) } });
  if (ctx.stage === 'thumbnail-video-crash') {
    fs.copyFileSync(path.resolve('target/thumbnail-acceptance/bbb-h264-720-10s.mp4'), path.join(ctx.mediaDir, 'sample.mp4'));
  }
  const root = await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Thumbnail Lifecycle' });
  const channel = await openChannel(cdp);
  await invoke('start_scan', { rootId: root.id, runId: 'lifecycle-' + Date.now(), onProgress: channel,
    groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false });
  const { hit } = await waitForChannel(cdp, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, 'lifecycle scan');
  if (hit.type !== 'completed') throw new Error('Scan failed');
  const files = [];
  for (const directory of await walkDirectoryTree(cdp, root.id, ctx.timeoutMs)) {
    files.push(...await invoke('list_directory_files', { directoryId: directory.id, limit: 500, offset: 0, categories: null }));
  }
  await invoke('start_full_thumbnail_generation');
  const status = await settled();
  if (status.generated !== files.length || status.status !== 'completed') throw new Error('Lifecycle fixture generation failed');
  if (ctx.stage !== 'thumbnail-video-crash') await runStep('thumbnail-lifecycle:settings-label', async () => {
    const deadline = Date.now() + 5000;
    let text = '';
    do {
      text = await evaluate(cdp, "document.querySelector('.thumb-gen-status')?.innerText || ''");
      if (ctx.stage === 'thumbnail-ui' ? /核显|独显|GPU/.test(text) : /CPU/.test(text)) break;
      await new Promise((resolve) => setTimeout(resolve, 100));
    } while (Date.now() < deadline);
    if (ctx.stage === 'thumbnail-ui') {
      const gpu = status.executions.filter((e) => ['imageD2d', 'imageVpl'].includes(e.native?.backend));
      if (!gpu.length) throw new Error('No actual GPU image execution; GPU UI unverified');
      const integrated = gpu.every((e) => e.native.adapterKind === 'integrated');
      if (!(integrated ? /核显|integrated/i : /GPU|独显/).test(text)) throw new Error('GPU label mismatch: ' + text);
    } else if (!/CPU/.test(text) || /GPU/.test(text)) throw new Error('CPU result label mismatch: ' + text);
    if (cdp.exceptions.length) throw new Error('Page exception: ' + JSON.stringify(cdp.exceptions[0]));
    return { text, status };
  });
  if (ctx.stage === 'thumbnail-ui') {
    await runStep('thumbnail-ui:sidebar-label', async () => {
      let labels = await evaluate(cdp, "Array.from(document.querySelectorAll('.tool__progress .thumb-execution')).map(e => e.innerText)");
      if (!labels.length) {
        await evaluate(cdp, "Array.from(document.querySelectorAll('.acc-header__toggle')).find(e => /工具|Tools/.test(e.innerText))?.click()");
        await new Promise((resolve) => setTimeout(resolve, 300));
        labels = await evaluate(cdp, "Array.from(document.querySelectorAll('.tool__progress .thumb-execution')).map(e => e.innerText)");
      }
      const setting = await evaluate(cdp, "document.querySelector('.thumb-gen-status .thumb-execution')?.innerText");
      if (!labels.includes(setting)) throw new Error('Sidebar/settings execution labels differ: ' + JSON.stringify({ labels, setting }));
      return { labels, setting };
    });
    await runStep('thumbnail-ui:log-window', async () => {
      await invoke('open_log_window');
      return { opened: true, scope: 'command completed; window content and focus transitions require separate observation' };
    });
  }
  const cache = await invoke('get_thumb_cache_dir');
  const artifacts = [];
  for (const item of files) {
    const detail = await invoke('get_media_detail', { id: item.id });
    const file = path.resolve(cache, 'thumbnails', detail.thumbPath);
    artifacts.push({ id: item.id, thumbPath: detail.thumbPath, file, hash: hash(file) });
  }
  if (ctx.stage === 'thumbnail-restore') {
    const probeId = files[0].id;
    await runStep('thumbnail-restore:backup-and-arm', async () => {
      await invoke('set_rating', { itemId: probeId, rating: 4 });
      const destination = path.join(ctx.root, 'backup-' + Date.now());
      fs.mkdirSync(destination);
      await invoke('start_backup', { dest: destination });
      const deadline = Date.now() + ctx.timeoutMs;
      let backup;
      do {
        backup = await invoke('backup_status');
        if (backup.status !== 'running' && backup.status !== 'idle') break;
        await new Promise((resolve) => setTimeout(resolve, 200));
      } while (Date.now() < deadline);
      if (backup.status !== 'completed' || !backup.path) throw new Error('Backup incomplete: ' + JSON.stringify(backup));
      const staged = await invoke('restore_stage', { packagePath: backup.path });
      await invoke('set_rating', { itemId: probeId, rating: 2 });
      const changed = await invoke('get_media_detail', { id: probeId });
      if (changed.rating !== 2) throw new Error('Library mutation did not apply');
      const settings = await invoke('get_settings_snapshot');
      await invoke('set_app_settings', { generation: settings.generation, patch: { thumb_size: '256' } });
      await invoke('restore_arm', { backupId: staged.backupId, stagingDir: staged.stagingDir });
      await invoke('start_full_thumbnail_generation');
      const active = await invoke('full_thumb_gen_status');
      if (active.status !== 'running') throw new Error('No active replacement generation before restore restart');
      if (!fs.existsSync(path.join(ctx.appDataDir, 'pending-restore.json'))) throw new Error('Restore marker absent');
      return { backupId: staged.backupId, backedUpRating: 4, changedRating: changed.rating, active };
    });
    return { rootId: root.id, artifacts, cache, restore: true, probeId };
  }
  return { rootId: root.id, artifacts, cache };
}


export async function interruptThumbnailRun(ctx, cdp, api, saved, terminate) {
  return api.runStep('thumbnail-crash:interrupt-active-lease', async () => {
    await api.ipc(cdp, 'start_full_thumbnail_generation', null, ctx.timeoutMs);
    const db = new DatabaseSync(path.join(ctx.appDataDir, 'scrollery.db'), { readOnly: true });
    const leaseKind = ctx.stage === 'thumbnail-video-crash' ? 'video_cover' : 'image_thumb';
    let leases = [];
    let committed = 0;
    try {
      const deadline = Date.now() + 5000;
      do {
        leases = db.prepare("SELECT lease_id FROM media_derivations WHERE status=1 AND lease_id IS NOT NULL AND kind=?").all(leaseKind).map((row) => row.lease_id);
        committed = db.prepare('SELECT count(*) AS n FROM media_items WHERE thumb_status=1').get().n;
        if (leases.length && committed > 0) break;
        await new Promise((resolve) => setTimeout(resolve, 5));
      } while (Date.now() < deadline);
      if (!leases.length || committed === 0) throw new Error('No partial completion with active lease observed for crash fixture');
      await terminate();
      const stale = db.prepare("SELECT lease_id FROM media_derivations WHERE status=1 AND lease_id IS NOT NULL AND kind=?").all(leaseKind).map((row) => row.lease_id);
      if (!stale.length) throw new Error('No orphan lease left by forced exit');
      const completed = new Set(db.prepare('SELECT id FROM media_items WHERE thumb_status=1').all().map((row) => row.id));
      return { ...saved, crash: true, leaseKind, leases: stale, allArtifacts: saved.artifacts, artifacts: saved.artifacts.filter((item) => completed.has(item.id)) };
    } finally { db.close(); }
  });
}
