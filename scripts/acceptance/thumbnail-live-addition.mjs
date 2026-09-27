import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

// T1「持续新增」的真实隔离证据：全库 run 的成员在启动时按 thumb_status=0 快照固定到
// media_derivations(run_id)；运行中经真实扫描入库的新媒体不进入本轮尾批，留待下一轮增量 run。
//
// 已核对的实际契约（2026-09-27 源码，不猜 DTO）：
// - start_scan：{rootId, runId, onProgress, groupBy, sortWithinGroup, sortOrder, quick}，快扫完成后返回，
//   新文件只按真实扫描写入 media_items（新图片 thumb_status=0）。
// - 图片缩略图**不随扫描自动派生**（派生管线只对启用时的视频封面登记）；只有
//   start_full/incremental_thumbnail_generation 在开始时经 enroll_image_thumbnail_run 登记本轮成员。
//   `image_thumbnail_lane_page` 只取 `run_id` 命中项，故运行中新增不会混入当前轮。
//
// 调用方已完成构建证明与 CDP 身份校验；写操作仅限其受控夹具目录与验收库。
export async function verifyThumbnailLiveAddition(ctx, main, api) {
  const { runStep, ipc, openChannel, waitForChannel, walkDirectoryTree } = api;
  const invoke = (command, args) => ipc(main, command, args, ctx.timeoutMs);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const status = () => invoke('full_thumb_gen_status');
  const runKey = (p) => [p.status, p.total, p.generated, p.phase ?? null].join('|');
  const hashFile = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');

  const listFiles = async (rootId) => {
    const files = [];
    for (const directory of await walkDirectoryTree(main, rootId, ctx.timeoutMs)) {
      files.push(...await invoke('list_directory_files', { directoryId: directory.id, limit: 500, offset: 0, categories: null }));
    }
    return files;
  };
  const scan = async (rootId, label) => {
    const channel = await openChannel(main);
    const scanning = invoke('start_scan', {
      rootId, runId: label + '-' + Date.now(), onProgress: channel,
      groupBy: null, sortWithinGroup: null, sortOrder: null, quick: false,
    });
    const observed = waitForChannel(main, (e) => e?.type === 'completed' || e?.type === 'error', ctx.timeoutMs, label);
    await scanning;
    const { hit } = await observed;
    if (hit.type !== 'completed') throw new Error(label + ' scan failed: ' + JSON.stringify(hit));
    return hit;
  };
  // 等待出现「新的一轮」状态：以状态键变化为准，避免读到上一轮遗留的 completed 载荷。
  const awaitRunChange = async (label, beforeKey) => {
    const deadline = Date.now() + Math.min(ctx.timeoutMs, 15000);
    let current = await status();
    while (runKey(current) === beforeKey && Date.now() < deadline) {
      await sleep(50);
      current = await status();
    }
    if (runKey(current) === beforeKey) throw new Error(label + ' 未产生新的运行状态');
    return current;
  };
  const settleRun = async (label, beforeKey) => {
    const deadline = Date.now() + ctx.timeoutMs;
    let current = await awaitRunChange(label, beforeKey);
    while (current.status === 'running' && Date.now() < deadline) {
      await sleep(200);
      current = await status();
    }
    if (current.status === 'running') throw new Error(label + ' 超时未收口: ' + JSON.stringify(current));
    return current;
  };
  // 当前轮已在运行（见上一断言）时直接等到收口，无需构造状态键。
  const waitTerminal = async (label) => {
    const deadline = Date.now() + ctx.timeoutMs;
    let current = await status();
    while (current.status === 'running' && Date.now() < deadline) {
      await sleep(200);
      current = await status();
    }
    if (current.status === 'running') throw new Error(label + ' 超时未收口: ' + JSON.stringify(current));
    return current;
  };

  const settings = await invoke('get_settings_snapshot');
  await invoke('set_app_settings', { generation: settings.generation, patch: {
    thumb_strategy: 'cpu', thumb_size: '128', thumb_skip_max_kb: '0', enable_video_cover: 'false',
  } });
  const existing = (await invoke('list_scan_roots')) || [];
  const normalize = (value) => path.resolve(value).toLowerCase();
  const found = existing.find((r) => normalize(r.path) === normalize(ctx.mediaDir));
  const root = found || await invoke('add_scan_root', { path: ctx.mediaDir, alias: 'Thumbnail Live Addition' });

  const originals = [...ctx.fixtures.items, ...ctx.fixtures.movable.items]
    .map(({ name, file, sha256 }) => ({ name, file, sha256 }));
  const addedName = 'live_addition_01.png';
  const addedFile = path.join(ctx.mediaDir, addedName);
  const addedSource = ctx.fixtures.items[0].file;
  let baseline;

  await runStep('thumbnail-live-addition:start-fixed-run', async () => {
    const scanned = await scan(root.id, 'live-addition-initial');
    const files = await listFiles(root.id);
    const images = files.filter((item) => item.fileName.endsWith('.png'));
    if (images.length !== originals.length) throw new Error('扫描图片数 ' + images.length + ' != 夹具 ' + originals.length);
    const beforeKey = runKey(await status());
    await invoke('start_full_thumbnail_generation');
    const running = await awaitRunChange('full run', beforeKey);
    if (running.status !== 'running') throw new Error('全库 run 在追加前已收口（请增大 --media）: ' + JSON.stringify(running));
    if (running.total !== images.length) throw new Error('本轮 total ' + running.total + ' != 扫描图片数 ' + images.length);
    baseline = { total: running.total };
    return { scannedItems: scanned.totalItems, runTotal: running.total, phase: running.phase, images: images.length };
  });

  await runStep('thumbnail-live-addition:running-addition-isolated', async () => {
    const beforeAdd = await status();
    if (beforeAdd.status !== 'running') throw new Error('追加前 run 未在运行（请增大 --media）: ' + JSON.stringify(beforeAdd));
    fs.copyFileSync(addedSource, addedFile);
    const addedHash = hashFile(addedFile);
    await scan(root.id, 'live-addition-rescan');
    const afterAdd = await status();
    if (afterAdd.status !== 'running') throw new Error('追加后 run 已收口，未取到运行中新增窗口（请增大 --media）: ' + JSON.stringify(afterAdd));
    if (afterAdd.total !== baseline.total) throw new Error('运行中新增改变了本轮固定成员: ' + JSON.stringify({ before: baseline.total, after: afterAdd.total }));
    const files = await listFiles(root.id);
    const added = files.find((item) => item.fileName === addedName);
    if (!added) throw new Error('运行中新增媒体未入库: ' + addedName);
    const detail = await invoke('get_media_detail', { id: added.id });
    if (detail.thumbStatus !== 0) throw new Error('新增媒体被当前 run 生成: ' + JSON.stringify(detail));
    baseline.addedId = added.id;
    baseline.addedHash = addedHash;
    return { runTotalBefore: baseline.total, runTotalAfter: afterAdd.total, runGeneratedAfterAdd: afterAdd.generated, addedId: added.id, addedThumbStatus: detail.thumbStatus };
  });

  await runStep('thumbnail-live-addition:incremental-completes-new-item', async () => {
    const settled = await waitTerminal('full run');
    if (settled.status !== 'completed' || settled.total !== baseline.total || settled.generated !== baseline.total || settled.results.failed !== 0) {
      throw new Error('全库 run 收口异常: ' + JSON.stringify(settled));
    }
    const afterFull = await invoke('get_media_detail', { id: baseline.addedId });
    if (afterFull.thumbStatus !== 0) throw new Error('新增媒体被固定成员轮生成: ' + JSON.stringify(afterFull));
    const beforeKey = runKey(await status());
    await invoke('start_incremental_thumbnail_generation');
    const incremental = await settleRun('incremental run', beforeKey);
    if (incremental.status !== 'completed' || incremental.total !== 1 || incremental.generated !== 1 ||
        incremental.results.newlyGenerated !== 1 || incremental.results.failed !== 0) {
      throw new Error('增量 run 未只完成新增项: ' + JSON.stringify(incremental));
    }
    const cache = await invoke('get_thumb_cache_dir');
    const detail = await invoke('get_media_detail', { id: baseline.addedId });
    if (detail.thumbStatus !== 1 || !detail.thumbPath) throw new Error('新增项产物不可用: ' + JSON.stringify(detail));
    const artifact = path.resolve(cache, 'thumbnails', detail.thumbPath);
    const relative = path.relative(path.resolve(cache), artifact);
    if (relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('新增项产物越出缓存目录: ' + artifact);
    if (!fs.existsSync(artifact) || fs.statSync(artifact).size === 0) throw new Error('新增项产物缺失或为空: ' + artifact);
    for (const original of originals) {
      if (hashFile(original.file) !== original.sha256) throw new Error('原有源被改动: ' + original.file);
    }
    if (hashFile(addedFile) !== baseline.addedHash) throw new Error('新增源被改动: ' + addedFile);
    return {
      fullRun: { total: settled.total, generated: settled.generated, newlyGenerated: settled.results.newlyGenerated },
      addedItemAfterFullRun: afterFull.thumbStatus,
      incrementalRun: { total: incremental.total, generated: incremental.generated, newlyGenerated: incremental.results.newlyGenerated, failed: incremental.results.failed },
      addedArtifact: { thumbStatus: detail.thumbStatus, thumbPath: detail.thumbPath, bytes: fs.statSync(artifact).size, sha256: hashFile(artifact) },
      originalsUnchanged: originals.length,
      scope: '运行中经真实 start_scan 追加同一根内新 PNG；新增未进入当前固定成员并保持 thumb_status=0，由后续 start_incremental_thumbnail_generation 单独完成',
    };
  });
}
