#!/usr/bin/env node
// 可复验源快照(P1-2 验收基线,2026-09-12)。
//
// why 需要它:本轮改动不自动 commit,构建产物却要能回溯到「究竟是哪份源码」。只记 HEAD commit
// 是不够的——工作区里还带着未提交改动,HEAD 里**没有**这些代码。本脚本因此记两样东西:
//   ① HEAD 提交号(供人对照);
//   ② 参与构建文件的内容 SHA256 清单 + 一条聚合摘要(才是权威的源码指纹)。
// 聚合摘要 = 对「排序后的 path + 内容 sha256」逐行 sha256,不含时间戳,故同内容任意机器重算一致。
// 任何一处源码/配置/lockfile 漂移都会改变它,可据此断言「产物来自这份快照」。
//
// 产物/worker 的 SHA256 是另一条线(它们不含源码,改动源码不改产物哈希):用 --artifacts 附加采集。
//
// 用法:
//   node scripts/acceptance/source-snapshot.mjs [--out=target/acceptance/source-snapshot.json] [--label=<tag>]
//        [--artifacts] [--check=<snapshot.json>] [--selftest]
//   --artifacts  同时采集已构建产物(exe / sidecar worker / 安装包)的 SHA256
//   --check      拿现有快照复验当前工作区:HEAD 与聚合摘要都必须一致,否则非零退出
// 退出码:一致/写入成功 = 0;复验不一致 = 1;脚本自身损坏(selftest) = 2。

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const EOL = '\n';
// 构建产物与依赖目录不进源码清单:它们是「被构建的东西」或外部依赖,不是本仓源码。
// 目录名精确匹配(不是前缀),否则 `crates/**/target/` 里的同名文件会漏滤。
const EXCLUDED_DIR_NAMES = new Set(['node_modules', 'target', 'dist', '.git', '.vite', 'coverage']);

function arg(name, fallback) {
  const hit = process.argv.slice(2).find((a) => a.startsWith('--' + name + '='));
  return hit ? hit.slice(name.length + 3) : fallback;
}
function hasFlag(name) {
  return process.argv.slice(2).includes('--' + name);
}

function git(args, opts) {
  return execFileSync('git', args, Object.assign({ cwd: REPO, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }, opts || {}));
}

export function isExcluded(relPath) {
  return relPath.split('/').some((seg) => EXCLUDED_DIR_NAMES.has(seg));
}

// 构建范围 = 全树摘要去掉「改不动产物」的目录与文件:文档、CI 工作流、代理配置。
// 用途:打包前记两条摘要——全树(证明「就是这棵工作树」)与构建范围(证明「就是这份产物源码」)。
// 两者一起看,文档改动不会伪装成产物源码改动,产物源码改动也不会藏在文档噪音里。
const NON_BUILD_PREFIXES = ['docs/', '.github/', '.claude/', '.commandcode/'];
const NON_BUILD_FILES = new Set(['CHANGELOG.md']);

export function isBuildRelevant(relPath) {
  if (NON_BUILD_FILES.has(relPath)) return false;
  return !NON_BUILD_PREFIXES.some((p) => relPath.startsWith(p));
}

// git ls-files -co --exclude-standard = tracked + untracked(未忽略)。
// 未提交改动若不在这里,就说明它被 .gitignore 排除了——那种文件不属于本仓源码。
export function listSourceFiles(repo = REPO) {
  const out = execFileSync(
    'git',
    // 🔴 `-z` + NUL 分割,而不是按换行取文本。why:git 默认 core.quotepath=true 会把非 ASCII
    // 路径转义成 `"docs/...\\344\\273\\223..."`(含八进制转义与引号),按行拿到的是这种**变形**
    // 路径;下游 existsSync 一判就 false 然后 continue 静默跳过——中文命名文件因此从快照里消失,
    // 而摘要照样算得出来(本机 core.quotepath=false 时看不出,换台机器就漏)。`-z` 输出原始字节
    // 且以 NUL 分隔,与 quotepath 设置无关,路径不再被转义。
    ['ls-files', '-z', '-co', '--exclude-standard', '--deduplicate'],
    { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }
  );
  return out
    .split('\u0000')
    .filter((p) => p && !isExcluded(p))
    .sort();
}

export function sha256File(abs) {
  return createHash('sha256').update(fs.readFileSync(abs)).digest('hex');
}

// 聚合摘要:只由 path+sha256 决定(不含 HEAD、不含时间戳),故可跨机器/跨提交独立复算。
export function aggregateDigest(entries) {
  const h = createHash('sha256');
  for (const e of entries) h.update(e.path + '\u0000' + e.sha256 + '\n');
  return h.digest('hex');
}

function collectSource(repo = REPO) {
  const files = listSourceFiles(repo);
  const entries = [];
  const missing = [];
  for (const rel of files) {
    const abs = path.join(repo, rel);
    // 🔴 路径解析不到就**记下来**,不能在循环里静默 continue:静默跳过会让快照少文件而摘要照旧
    // 算得出来——漏掉的文件在任何比对里都不存在,正是最难发现的一类漂移。
    if (!fs.existsSync(abs)) {
      missing.push(rel);
      continue;
    }
    entries.push({ path: rel, sha256: sha256File(abs), bytes: fs.statSync(abs).size });
  }
  // 已知的无害来源只有「索引里还留着已删除文件」(git status 显示 D 但未提交)。这种情况允许跳过,
  // 但要显式报数;其余解析失败一律硬失败,逼人先查清路径为何对不上(转义/编码问题都属此类)。
  if (missing.length) {
    const suspicious = missing.filter((p) => /\\\d{3}|^\"/.test(p));
    if (suspicious.length) {
      throw new Error(
        '源清单里出现被转义/加引号的路径(git quotepath 形态),路径解析必然失败:' + EOL +
          '  ' + suspicious.slice(0, 5).join(EOL + '  ')
      );
    }
    console.warn('[source-snapshot] 跳过 ' + missing.length + ' 个索引中已不存在的路径:' + missing.slice(0, 5).join(', '));
  }
  return entries;
}

// 产物线:已构建的 exe、sidecar worker、安装包。缺项不算错(未构建时本就该缺)。
const ARTIFACT_CANDIDATES = [
  'target/release/scrollery.exe',
  'target/release/ai-worker.exe',
  'target/release/video-worker.exe',
  'src-tauri/binaries/ai-worker-x86_64-pc-windows-msvc.exe',
  'src-tauri/binaries/raw-worker-x86_64-pc-windows-msvc.exe',
  'src-tauri/binaries/video-worker-x86_64-pc-windows-msvc.exe',
  'crates/exotic-workers/raw-worker/target/x86_64-pc-windows-gnu/release/raw-worker.exe',
];
const ARTIFACT_DIRS = ['target/release/bundle'];

function collectArtifacts(repo = REPO) {
  const out = [];
  const push = (rel) => {
    const abs = path.join(repo, rel);
    if (fs.existsSync(abs) && fs.statSync(abs).isFile()) {
      out.push({ path: rel, sha256: sha256File(abs), bytes: fs.statSync(abs).size });
    }
  };
  for (const rel of ARTIFACT_CANDIDATES) push(rel);
  for (const dir of ARTIFACT_DIRS) {
    const abs = path.join(repo, dir);
    if (!fs.existsSync(abs)) continue;
    const walk = (d) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else if (/\.(exe|msi)$/i.test(e.name)) push(path.relative(repo, p).split(path.sep).join('/'));
      }
    };
    walk(abs);
  }
  return out.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

function headCommit(repo = REPO) {
  try {
    return git(['rev-parse', 'HEAD'], { cwd: repo }).trim();
  } catch (_) {
    return null;
  }
}

function headSubject(repo = REPO) {
  try {
    return git(['log', '-1', '--pretty=%s'], { cwd: repo }).trim();
  } catch (_) {
    return null;
  }
}

function dirtyPaths(repo = REPO) {
  try {
    return git(['status', '--porcelain'], { cwd: repo }).split(/\r?\n/).filter(Boolean).length;
  } catch (_) {
    return null;
  }
}

// 快照刻意**不**声称「HEAD 就是被测源码」:字段名与 dirty 标志都要把这点说死。
function buildSnapshot(label, withArtifacts) {
  const source = collectSource();
  const buildScope = source.filter((f) => isBuildRelevant(f.path));
  return {
    format: 'scrollery-source-snapshot/1',
    label: label || null,
    capturedAtUtc: new Date().toISOString(),
    head: { commit: headCommit(), subject: headSubject() },
    // 未提交改动计数:>0 时源真相只在下面的 source.files,不在 head.commit。
    uncommittedPathCount: dirtyPaths(),
    sourceNote:
      'source.digest 由工作区实际文件内容算出(含未提交改动);head.commit 仅供对照,不代表被测源码。',
    source: { fileCount: source.length, digest: aggregateDigest(source), files: source },
    // 构建范围摘要:同一算法,只是滤掉改不动产物的文档/CI/代理配置。
    buildScope: { fileCount: buildScope.length, digest: aggregateDigest(buildScope) },
    artifacts: withArtifacts ? collectArtifacts() : [],
  };
}

function verifySnapshot(snapshotPath) {
  const prev = JSON.parse(fs.readFileSync(snapshotPath, 'utf8'));
  const now = buildSnapshot(prev.label, Array.isArray(prev.artifacts) && prev.artifacts.length > 0);
  const problems = [];
  if (prev.head && prev.head.commit !== now.head.commit) {
    problems.push('HEAD 漂移:' + prev.head.commit + ' → ' + now.head.commit);
  }
  if (prev.source.digest !== now.source.digest) {
    const prevMap = new Map(prev.source.files.map((f) => [f.path, f.sha256]));
    const nowMap = new Map(now.source.files.map((f) => [f.path, f.sha256]));
    const changed = [];
    for (const [p, h] of nowMap) {
      if (!prevMap.has(p)) changed.push('新增 ' + p);
      else if (prevMap.get(p) !== h) changed.push('改动 ' + p);
    }
    for (const p of prevMap.keys()) if (!nowMap.has(p)) changed.push('删除 ' + p);
    problems.push(
      '源码摘要漂移(' + changed.length + ' 处:' + changed.slice(0, 8).join('; ') + (changed.length > 8 ? ' …' : '') + ')'
    );
  }
  if (prev.buildScope && now.buildScope && prev.buildScope.digest !== now.buildScope.digest) {
    problems.push(
      '构建范围摘要漂移(产物源码已变:文件 ' + prev.buildScope.fileCount + ' → ' + now.buildScope.fileCount + ')'
    );
  }
  return { prev, now, problems };
}

function selftest() {
  const fails = [];
  const expect = (cond, msg) => {
    if (!cond) fails.push(msg);
  };
  // 聚合摘要必须只由内容决定:顺序敏感、内容敏感、与时间无关。
  const a = [{ path: 'a.ts', sha256: '11' }, { path: 'b.ts', sha256: '22' }];
  const b = [{ path: 'b.ts', sha256: '22' }, { path: 'a.ts', sha256: '11' }];
  expect(aggregateDigest(a) === aggregateDigest(a), '聚合摘要自身不稳定');
  expect(aggregateDigest(a) !== aggregateDigest(b), '聚合摘要未随顺序变化(排序前提被破坏)');
  expect(
    aggregateDigest(a) !== aggregateDigest([{ path: 'a.ts', sha256: '11' }, { path: 'b.ts', sha256: '23' }]),
    '聚合摘要未随内容变化'
  );
  // 排除规则:构建产物目录必须滤掉,源码目录必须保留。
  for (const p of ['target/release/x.exe', 'node_modules/a/index.js', 'dist/index.html', 'crates/x/target/y']) {
    expect(isExcluded(p), '排除规则漏滤:' + p);
  }
  for (const p of ['src/main.ts', 'src-tauri/tauri.conf.json', 'scripts/acceptance/isolated-app-smoke.mjs']) {
    expect(!isExcluded(p), '排除规则误滤:' + p);
  }
  // 构建范围:文档/CI/代理配置不是产物源码;LICENSE/NOTICE/lockfile/配置/脚本是。
  for (const p of ['docs/todo.md', '.github/workflows/ci.yml', '.claude/x.md', 'CHANGELOG.md']) {
    expect(!isBuildRelevant(p), '构建范围误收:' + p);
  }
  for (const p of ['NOTICE.md', 'LICENSE', 'src-tauri/tauri.conf.json', 'Cargo.lock', 'package-lock.json']) {
    expect(isBuildRelevant(p), '构建范围误滤:' + p);
  }
  // 真实仓库自检:清单非空,且重算两次摘要一致(可复验性的最低要求)。
  const one = collectSource();
  const two = collectSource();
  expect(one.length > 100, '源码清单过少(' + one.length + '),疑似排除规则误伤');
  expect(aggregateDigest(one) === aggregateDigest(two), '两次采集摘要不一致');
  expect(one.some((f) => f.path === 'NOTICE.md'), '源码清单缺 NOTICE.md');
  expect(one.some((f) => f.path.startsWith('src-tauri/')), '源码清单缺 src-tauri/');
  if (fails.length) {
    console.error('selftest 失败:');
    for (const f of fails) console.error('  - ' + f);
    process.exit(2);
  }
  console.log('✓ selftest 通过(摘要确定性/排除规则/真实仓清单 ' + one.length + ' 文件)');
}

function main() {
  selftest();
  if (hasFlag('selftest')) process.exit(0);
  const checkPath = arg('check', null);
  if (checkPath) {
    const { prev, now, problems } = verifySnapshot(checkPath);
    console.log('快照复验:' + checkPath);
    if (prev.buildScope) console.log('  构建范围: ' + prev.buildScope.digest + ' → ' + now.buildScope.digest);
    console.log('  HEAD    : ' + (prev.head && prev.head.commit) + ' → ' + now.head.commit);
    console.log('  源码摘要: ' + prev.source.digest);
    console.log('  当前摘要: ' + now.source.digest + '(文件 ' + now.source.fileCount + ')');
    if (problems.length) {
      console.error('✗ 快照与当前工作区不一致:');
      for (const p of problems) console.error('  - ' + p);
      process.exit(1);
    }
    console.log('✓ 一致:当前工作区就是该快照记录的源码');
    return;
  }
  const withArtifacts = hasFlag('artifacts');
  const snapshot = buildSnapshot(arg('label', null), withArtifacts);
  const outPath = path.resolve(REPO, arg('out', 'target/acceptance/source-snapshot.json'));
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, JSON.stringify(snapshot, null, 2) + EOL);
  console.log('源快照已写入:' + path.relative(REPO, outPath));
  console.log('  HEAD    : ' + snapshot.head.commit + '  ' + snapshot.head.subject);
  console.log('  构建范围: ' + snapshot.buildScope.digest + '(' + snapshot.buildScope.fileCount + ' 文件,滤除文档/CI/代理配置)');
  console.log('  未提交项: ' + snapshot.uncommittedPathCount + '(source.digest 已含其内容,HEAD 不含)');
  console.log('  源码摘要: ' + snapshot.source.digest + '(' + snapshot.source.fileCount + ' 文件)');
  if (withArtifacts) {
    console.log('  产物    : ' + snapshot.artifacts.length + ' 项');
    for (const a of snapshot.artifacts) console.log('    ' + a.sha256.slice(0, 16) + '  ' + a.path);
  }
}

main();
