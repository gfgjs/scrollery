import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

// T2/T4 代表性性能与资源收口的受控只读取证模块（2026-09-27）。
//
// 调用方（主会话）负责：canonicalize ctx.sampleDir 并确认它与 ctx.mediaDir、验收 appData 互不重叠；
// 注入与 thumbnail-interaction 相同的 api 并额外提供 ownedPid；采集 ctx.logDir/owned-resources.jsonl；
// 启动与收尾进程。本模块本身只读：经真实 start_scan IPC 扫描 ctx.sampleDir，按目录分页
// list_directory_files 取成员；不写 ctx.sampleDir / ctx.mediaDir，不复制、不转换、不改名，
// 不 clear_database，也不触碰真实媒体库。
//
// 证据里的 startTimeMs / endTimeMs 是 Date.now()，供 owned-resources.jsonl 按 epoch 对齐每轮窗口。
//
// 断言口径不放宽：CPU→GPU 各一次完整重建，终态有效项必须全部成功；合法 JPEG 出现失败即按失败
// 返回事实，不写成「部分成功」。样本只有 ctx.sampleDir 内的少量图片，且两轮开始前都顺序整读
// （显式暖文件缓存），因此结论不得外推为大库冷读或混合资源倍数。

const POLL_INTERVAL_MS = 100;
const LIST_PAGE_LIMIT = 200;
const LIST_MAX_ITEMS = 5000;
// 模块自身的最长等待。超时按「未收口」失败并返回事实，绝不当作通过，也不让主会话被单轮挂住。
const MODULE_BUDGET_MS = 9 * 60 * 1000;
// start_full 立即返回、后台异步执行：只接受「先观察到本轮 running、再等到非 running」的收口。
// 旧轮已收口时 full_thumb_gen_status 在本轮首次发布之前仍会回旧 completed 快照，若一直没等到
// 本轮 running，按未观察到本轮启动快速失败，而不是把旧载荷当成本轮结果，也不空等到模块预算。
const RUNNING_OBSERVE_MS = 60 * 1000;
const IMAGE_EXTENSIONS = new Set(['.jpg', '.jpeg']);
const EXPECTED_TIER = '512';
const SAMPLE_KEEP_SAMPLES = 300;

const sha256 = (data) => createHash('sha256').update(data).digest('hex');
const comparable = (value) => path.resolve(value).toLowerCase();

// 含边界比较；Windows 上按小写归一，避免盘符/路径大小写差异被当成越界或漏判。
const isUnder = (candidate, root) => {
  const child = comparable(candidate);
  const parent = comparable(root);
  return child === parent || child.startsWith(parent + path.sep);
};

// 与 source-snapshot 同口径的聚合摘要：只由内容与元数据决定，不含时间戳，可跨机复算。
const aggregateDigest = (entries) => {
  const hash = createHash('sha256');
  for (const entry of entries) {
    hash.update(
      entry.name + '\u0000' + entry.sha256 + '\u0000' + entry.bytes + '\u0000' + Math.round(entry.mtimeMs) + '\n'
    );
  }
  return hash.digest('hex');
};

const percentile = (values, percent) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((percent / 100) * sorted.length) - 1));
  return sorted[index];
};

/** 枚举样本目录内的真实图片文件。目录内出现 reparse/symlink 一律拒绝：既不跟随，
 * 也不自创授权外的递归（授权边界由主会话负责，本模块只拒绝逃逸）。 */
function listSampleImages(root) {
  const realRoot = fs.realpathSync(root);
  const found = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const abs = path.join(dir, entry.name);
      const stat = fs.lstatSync(abs);
      if (stat.isSymbolicLink()) {
        throw new Error('样本目录内存在 reparse/symlink，拒绝跟随: ' + abs);
      }
      if (stat.isDirectory()) {
        walk(abs);
        continue;
      }
      if (!stat.isFile()) continue;
      const realFile = fs.realpathSync(abs);
      if (!isUnder(realFile, realRoot)) {
        throw new Error('样本文件解析越出样本目录: ' + abs + ' → ' + realFile);
      }
      if (IMAGE_EXTENSIONS.has(path.extname(entry.name).toLowerCase())) found.push(path.resolve(abs));
    }
  };
  walk(root);
  return found.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

/** 顺序整读源文件，记录 hash/size/mtime。顺序整读既产出源清单，也显式暖文件缓存，
 * 使 CPU/GPU 两轮比较不被冷读盘面差异主导。 */
function readSourceManifest(files) {
  const entries = [];
  for (const file of files) {
    const before = fs.statSync(file);
    const data = fs.readFileSync(file);
    const after = fs.statSync(file);
    entries.push({
      name: path.basename(file),
      sha256: sha256(data),
      bytes: before.size,
      mtimeMs: before.mtimeMs,
      sizeStable: before.size === after.size,
      mtimeStable: before.mtimeMs === after.mtimeMs,
    });
  }
  return {
    entries,
    digest: aggregateDigest(entries),
    totalBytes: entries.reduce((sum, entry) => sum + entry.bytes, 0),
  };
}

function assertManifestUnchanged(current, baseline, phase) {
  const unstable = current.entries.filter((entry) => !entry.sizeStable || !entry.mtimeStable);
  if (unstable.length) {
    throw new Error(phase + '读取期间源文件元数据变化: ' + JSON.stringify(unstable.slice(0, 3)));
  }
  if (current.digest !== baseline.digest || current.entries.length !== baseline.entries.length) {
    const changed = [];
    for (let index = 0; index < Math.max(current.entries.length, baseline.entries.length); index += 1) {
      const now = current.entries[index];
      const then = baseline.entries[index];
      if (!now || !then || now.sha256 !== then.sha256 || now.bytes !== then.bytes || now.mtimeMs !== then.mtimeMs || now.name !== then.name) {
        changed.push({ before: then ?? null, after: now ?? null });
      }
    }
    throw new Error(phase + '源清单变化: ' + JSON.stringify({ digest: current.digest, baseline: baseline.digest, changed: changed.slice(0, 3) }));
  }
  return current.digest;
}

function compactPayload(payload, at) {
  return {
    at,
    status: payload?.status ?? null,
    total: payload?.total ?? null,
    generated: payload?.generated ?? null,
    phase: payload?.phase ?? null,
    failed: payload?.results?.failed ?? null,
    newlyGenerated: payload?.results?.newlyGenerated ?? null,
    cacheHit: payload?.results?.cacheHit ?? null,
  };
}

// 调用方已完成构建证明、独立实例身份校验与样本目录边界确认；本模块只用其受控只读样本。
export async function verifyThumbnailRepresentative(ctx, main, api) {
  const { runStep, ipc, evaluate, openChannel, waitForChannel, walkDirectoryTree, ownedPid } = api;
  const invoke = (command, args) => ipc(main, command, args, ctx.timeoutMs);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const moduleDeadline = Date.now() + MODULE_BUDGET_MS;

  if (!ctx.sampleDir) throw new Error('缺少 ctx.sampleDir（由主会话 canonicalize 并确认边界后传入）');
  const sampleDir = path.resolve(ctx.sampleDir);
  const mediaDir = path.resolve(ctx.mediaDir);
  const appDataDir = ctx.appDataDir ? path.resolve(ctx.appDataDir) : null;

  let baseline = null;
  let sources = [];
  const prepared = await runStep('thumbnail-representative:sample-root', async () => {
    if (!fs.existsSync(sampleDir) || !fs.statSync(sampleDir).isDirectory()) {
      throw new Error('样本目录不可用: ' + sampleDir);
    }
    if (isUnder(sampleDir, mediaDir) || isUnder(mediaDir, sampleDir)) {
      throw new Error('样本目录与验收夹具目录重叠，拒绝: ' + sampleDir);
    }
    if (appDataDir && (isUnder(sampleDir, appDataDir) || isUnder(appDataDir, sampleDir))) {
      throw new Error('样本目录落在验收 appData 内，拒绝: ' + sampleDir);
    }
    sources = listSampleImages(sampleDir);
    if (!sources.length) throw new Error('样本目录内没有 JPEG: ' + sampleDir);
    baseline = readSourceManifest(sources);
    return {
      sampleDir,
      fileCount: baseline.entries.length,
      totalBytes: baseline.totalBytes,
      manifestDigest: baseline.digest,
      names: baseline.entries.map((entry) => entry.name),
      note: '源 hash/size/mtime 已在所有轮次前建立基线；每轮前后复读同一清单',
    };
  });

  let members = [];
  const membership = await runStep('thumbnail-representative:scan-membership', async () => {
    const roots = (await invoke('list_scan_roots')) || [];
    const root =
      roots.find((entry) => comparable(entry.path) === comparable(sampleDir)) ||
      (await invoke('add_scan_root', { path: sampleDir, alias: 'Thumbnail Representative' }));
    const channel = await openChannel(main);
    const scanning = invoke('start_scan', {
      rootId: root.id,
      runId: 'thumbnail-representative-' + Date.now(),
      onProgress: channel,
      groupBy: null,
      sortWithinGroup: null,
      sortOrder: null,
      quick: false,
    });
    const observed = waitForChannel(
      main,
      (event) => event?.type === 'completed' || event?.type === 'error',
      ctx.timeoutMs,
      'representative scan'
    );
    await scanning;
    const { hit } = await observed;
    if (hit.type !== 'completed') throw new Error('真实扫描未完成: ' + JSON.stringify(hit).slice(0, 300));
    const listed = [];
    for (const directory of await walkDirectoryTree(main, root.id, ctx.timeoutMs)) {
      for (let offset = 0; ; offset += LIST_PAGE_LIMIT) {
        const page = await invoke('list_directory_files', {
          directoryId: directory.id,
          limit: LIST_PAGE_LIMIT,
          offset,
          categories: null,
        });
        if (!Array.isArray(page)) throw new Error('list_directory_files 返回异常: ' + JSON.stringify(page).slice(0, 200));
        listed.push(...page);
        if (page.length < LIST_PAGE_LIMIT) break;
        if (offset + LIST_PAGE_LIMIT > LIST_MAX_ITEMS) throw new Error('目录分页超上限: ' + directory.id);
      }
    }
    const names = new Set(listed.map((item) => item.fileName));
    const missing = baseline.entries.filter((entry) => !names.has(entry.name)).map((entry) => entry.name);
    const unexpected = listed.filter((item) => !baseline.entries.some((entry) => entry.name === item.fileName)).map((item) => item.fileName);
    if (missing.length || unexpected.length) {
      throw new Error('扫描成员与样本目录不一致: ' + JSON.stringify({ scanned: listed.length, expected: baseline.entries.length, missing, unexpected }));
    }
    for (const item of listed) {
      const detail = await invoke('get_media_detail', { id: item.id });
      if (!detail?.absPath || !isUnder(detail.absPath, sampleDir)) {
        throw new Error('入库成员 absPath 越出样本目录: ' + JSON.stringify({ id: item.id, name: item.fileName, absPath: detail?.absPath ?? null }));
      }
    }
    members = listed;
    return {
      rootId: root.id,
      scannedItems: hit.totalItems,
      members: listed.length,
      mediaType: listed[0]?.mediaType ?? null,
      scope: '真实 start_scan + 分页 list_directory_files；成员 absPath 经 get_media_detail 复核在样本目录内',
    };
  });

  // 阶段事件：与轮询快照互为佐证，取「何种阶段实际发生、何时发生」。
  await evaluate(
    main,
    `(() => {
      window.__thumbRepEvents = [];
      return window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
        event: 'thumb:gen_progress',
        target: { kind: 'Any' },
        handler: window.__TAURI_INTERNALS__.transformCallback((event) =>
          window.__thumbRepEvents.push({ at: Date.now(), payload: event.payload })),
      });
    })()`
  );
  const drainEvents = async () => {
    const raw = await evaluate(main, 'window.__thumbRepEvents ? window.__thumbRepEvents.splice(0) : []');
    return (raw || []).map((entry) => ({
      at: entry.at,
      status: entry.payload?.status ?? null,
      phase: entry.payload?.phase ?? null,
      total: entry.payload?.total ?? null,
      generated: entry.payload?.generated ?? null,
      failed: entry.payload?.results?.failed ?? null,
    }));
  };

  const applyExpectedSettings = async (strategy) => {
    const expected = {
      thumb_strategy: strategy,
      thumb_size: '512',
      thumb_skip_max_kb: '0',
      enable_video_cover: 'false',
    };
    const snapshot = await invoke('get_settings_snapshot');
    await invoke('set_app_settings', { generation: snapshot.generation, patch: { ...expected } });
    const applied = await invoke('get_settings_snapshot');
    const mismatched = Object.entries(expected).filter(([key, value]) => applied.values?.[key] !== value);
    if (mismatched.length) {
      throw new Error(strategy + ' 轮设置未生效: ' + JSON.stringify({ mismatched, values: applied.values }));
    }
    return {
      expected,
      applied: Object.fromEntries(Object.entries(expected).map(([key]) => [key, applied.values[key]])),
      signature: ['thumb_size', 'thumb_skip_max_kb', 'enable_video_cover']
        .map((key) => key + '=' + applied.values[key])
        .join('|'),
    };
  };

  // start_full 立即返回且后台异步执行：必须先真实观察到本轮的 running（total 还会从初始化值变化），
  // 才允许接受终态；否则会把上一轮遗留的 completed 快照当成本轮收口。轮询可能漏掉极短一轮的
  // running 窗口，故同时以本轮新增的 thumb:gen_progress 事件是否出现过 running 作交叉判据。
  const observeTerminal = async (strategy, expectedTotal, priorPayload) => {
    const latencies = [];
    const samples = [];
    const phaseTimeline = [];
    const events = [];
    let sawRunning = false;
    let firstRunning = null;
    const startedAt = Date.now();
    for (;;) {
      const requested = performance.now();
      const payload = await invoke('full_thumb_gen_status');
      latencies.push(performance.now() - requested);
      const sample = compactPayload(payload, Date.now());
      samples.push(sample);
      if (samples.length > SAMPLE_KEEP_SAMPLES) samples.shift();
      // 事件在「写快照之后」才发出，所以事件已见 running 就说明快照已不再停在旧载荷。
      // 但本次 payload 是在抽事件之前读的，可能仍是旧 completed：这种情况本轮先不判终态，
      // 下一轮重读必须看到新快照，避免把上一轮的 completed 误当本轮收口。
      let runningFromEventOnly = false;
      if (payload.status === 'running') {
        sawRunning = true;
        if (!firstRunning) firstRunning = sample;
        const last = phaseTimeline.at(-1);
        if (!last || last.phase !== sample.phase || last.total !== sample.total) {
          phaseTimeline.push({ at: sample.at, phase: sample.phase, total: sample.total, generated: sample.generated });
        }
      } else {
        const batch = await drainEvents();
        events.push(...batch);
        if (!sawRunning && batch.some((entry) => entry.status === 'running')) {
          sawRunning = true;
          runningFromEventOnly = true;
        }
      }
      if (sawRunning && payload.status !== 'running' && !runningFromEventOnly) {
        return {
          terminal: payload,
          waitedMs: Date.now() - (samples[0]?.at ?? Date.now()),
          firstRunning,
          samples,
          phaseTimeline,
          events,
          ipcLatencyMs: {
            count: latencies.length,
            max: Math.max(...latencies),
            p50: percentile(latencies, 50),
            p95: percentile(latencies, 95),
          },
        };
      }
      if (!sawRunning && Date.now() - startedAt > RUNNING_OBSERVE_MS) {
        throw new Error(
          strategy + ' 轮在 ' + RUNNING_OBSERVE_MS + 'ms 内未观察到 running（不接受旧 completed 载荷）: ' +
            JSON.stringify({ priorPayload: priorPayload ?? null, expectedTotal, samples: samples.slice(-3) })
        );
      }
      if (Date.now() > moduleDeadline) {
        throw new Error(
          strategy + ' 轮未在模块预算内收口: ' +
            JSON.stringify({ sawRunning, expectedTotal, firstRunning, samples: samples.slice(-3), ipcLatencyMs: { count: latencies.length } })
        );
      }
      await sleep(POLL_INTERVAL_MS);
    }
  };

  const verifyArtifacts = async (strategy) => {
    const cacheDir = await invoke('get_thumb_cache_dir');
    const cacheRoot = path.resolve(cacheDir, 'thumbnails');
    const artifacts = [];
    for (const item of members) {
      const detail = await invoke('get_media_detail', { id: item.id });
      if (!detail?.absPath || !isUnder(detail.absPath, sampleDir)) {
        throw new Error(strategy + ' 轮成员 absPath 越出样本目录: ' + JSON.stringify({ id: item.id, absPath: detail?.absPath ?? null }));
      }
      if (detail.thumbStatus !== 1 || !detail.thumbPath) {
        throw new Error(strategy + ' 轮成员未得到有效产物: ' + JSON.stringify({ id: item.id, name: item.fileName, thumbStatus: detail.thumbStatus, thumbPath: detail.thumbPath ?? null }));
      }
      const tier = String(detail.thumbPath).split('/')[0];
      if (tier !== EXPECTED_TIER) {
        throw new Error(strategy + ' 轮产物档位不是 ' + EXPECTED_TIER + ': ' + detail.thumbPath);
      }
      const artifact = path.resolve(cacheRoot, detail.thumbPath);
      if (!isUnder(artifact, cacheRoot)) throw new Error('产物越出缓存目录: ' + artifact);
      if (!fs.existsSync(artifact) || fs.statSync(artifact).size === 0) throw new Error('产物缺失或为空: ' + artifact);
      artifacts.push({ id: item.id, name: item.fileName, thumbPath: detail.thumbPath, bytes: fs.statSync(artifact).size });
    }
    return {
      count: artifacts.length,
      tier: EXPECTED_TIER,
      cacheRoot,
      totalBytes: artifacts.reduce((sum, entry) => sum + entry.bytes, 0),
      artifacts,
    };
  };

  const roundReports = [];
  const runRound = (strategy) =>
    runStep('thumbnail-representative:round-' + strategy, async () => {
      const warm = readSourceManifest(sources);
      assertManifestUnchanged(warm, baseline, '轮前');
      const settings = await applyExpectedSettings(strategy);
      const before = await invoke('full_thumb_gen_status');
      if (before.status === 'running') {
        throw new Error('开始 ' + strategy + ' 轮前仍有运行中的任务: ' + JSON.stringify(before));
      }
      const startTimeMs = Date.now();
      await invoke('start_full_thumbnail_generation');
      const observed = await observeTerminal(strategy, members.length, before);
      const endTimeMs = Date.now();
      // 终态事件与「token 收口」之间可能有投递延迟，补一次抽取后再合并，避免漏掉收尾事件。
      await sleep(200);
      const events = [...observed.events, ...(await drainEvents())];

      const terminal = observed.terminal;
      const results = terminal.results ?? {};
      const count = members.length;
      if (terminal.status !== 'completed') {
        throw new Error(strategy + ' 轮未正常收口: ' + JSON.stringify({ terminal, firstRunning: observed.firstRunning }));
      }
      if (terminal.total !== count || terminal.generated !== count) {
        throw new Error(strategy + ' 轮成员数不符: ' + JSON.stringify({ total: terminal.total, generated: terminal.generated, expected: count }));
      }
      if (results.failed !== 0 || results.temporarilyUnavailable !== 0) {
        throw new Error(
          strategy + ' 轮存在失败或暂不可用项（不放宽断言）: ' +
            JSON.stringify({ failed: results.failed ?? null, temporarilyUnavailable: results.temporarilyUnavailable ?? null, executions: terminal.executions })
        );
      }
      if (results.newlyGenerated !== count || results.cacheHit !== 0 || results.direct !== 0) {
        throw new Error(strategy + ' 轮不是一次完整重建: ' + JSON.stringify(results));
      }
      if (results.available !== (results.newlyGenerated ?? 0) + (results.cacheHit ?? 0)) {
        throw new Error(strategy + ' 轮结果守恒被破坏: ' + JSON.stringify(results));
      }
      if (!Array.isArray(terminal.executions) || !terminal.executions.length) {
        throw new Error(strategy + ' 轮没有真实后端执行记录: ' + JSON.stringify(terminal.executions));
      }
      const gpuBackends = ['imageD2d', 'imageVpl'];
      if (strategy === 'cpu' && terminal.executions.some((entry) => gpuBackends.includes(entry.native?.backend))) {
        throw new Error('CPU 轮发布了 GPU 图像执行: ' + JSON.stringify(terminal.executions));
      }
      const artifacts = await verifyArtifacts(strategy);
      const postDigest = assertManifestUnchanged(readSourceManifest(sources), baseline, '轮后');
      const report = {
        strategy,
        startTimeMs,
        endTimeMs,
        elapsedMs: endTimeMs - startTimeMs,
        settings,
        before,
        firstRunning: observed.firstRunning,
        terminal: {
          status: terminal.status,
          total: terminal.total,
          generated: terminal.generated,
          phase: terminal.phase ?? null,
          results,
          executions: terminal.executions,
        },
        phaseTimeline: observed.phaseTimeline,
        events,
        ipcLatencyMs: observed.ipcLatencyMs,
        polls: observed.samples.length,
        artifacts,
        sourceManifestDigest: postDigest,
        sourceUnchanged: postDigest === baseline.digest,
      };
      roundReports.push(report);
      return report;
    });

  await runRound('cpu');
  await runRound('gpu');

  return await runStep('thumbnail-representative:report', async () => {
    const [cpu, gpu] = roundReports;
    if (!cpu || !gpu) throw new Error('缺少完整两轮结果: ' + roundReports.map((entry) => entry.strategy));
    const signatures = new Set(roundReports.map((entry) => entry.settings.signature));
    if (signatures.size !== 1) {
      throw new Error('两轮非策略设置不一致: ' + JSON.stringify(roundReports.map((entry) => entry.settings.signature)));
    }
    const unchanged = roundReports.every((entry) => entry.sourceUnchanged);
    if (!unchanged) throw new Error('源文件在轮次间发生变化: ' + JSON.stringify(roundReports.map((entry) => entry.sourceManifestDigest)));
    return {
      sampleDir,
      ownedPid: Number.isInteger(ownedPid) ? ownedPid : null,
      sample: {
        fileCount: prepared.fileCount,
        totalBytes: prepared.totalBytes,
        manifestDigest: prepared.manifestDigest,
        scanMembers: membership.members,
      },
      warmCache: '两轮开始前均按文件名顺序整读全部源 JPEG（显式暖文件缓存）',
      settingsConsistent: [...signatures][0],
      rounds: roundReports,
      elapsedMs: gpu.endTimeMs - cpu.startTimeMs,
      speedupClaim: null,
      limits: [
        '样本为 ctx.sampleDir 内 ' + prepared.fileCount + ' 张 JPEG（' + prepared.totalBytes + ' 字节），且两轮前均整读暖缓存；不代表大库冷读或混合资源峰值',
        'CPU/GPU 各一次完整重建，单次耗时与 IPC 时延只作同轮事实记录，不构成稳定倍数结论',
        'GPU 轮是否实际走独显/核显取决于本机设备与驱动，executions 为事实记录，未就此放宽或加强断言',
      ],
      resources: ctx.logDir
        ? {
            file: path.join(ctx.logDir, 'owned-resources.jsonl'),
            alignBy: 'startTimeMs/endTimeMs 为 Date.now() epoch，按同一时钟对齐采样窗口',
          }
        : null,
      scope: '真实扫描与全库重建 IPC；只读样本目录与验收缓存，不写真实目录、不复制/转换/改名、不清真实库',
    };
  });
}
