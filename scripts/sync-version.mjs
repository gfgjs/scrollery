#!/usr/bin/env node
// 版本号单一事实源工具(Part7-T5 / Part7 §3.2 R3)。
// 锚 = 根 Cargo.toml 的 [workspace.package] version;零依赖,node 直跑。
//
// 用法:
//   node scripts/sync-version.mjs --check            比对 package.json 与锚(CI 门控;失配 exit 1 并提示 --write)
//   node scripts/sync-version.mjs --write            把锚写回 package.json(幂等,定点替换不重排)
//   node scripts/sync-version.mjs --print            打印锚版本(供脚本/流水线取值)
//   node scripts/sync-version.mjs --check-tag vX.Y.Z 断言 release tag 与锚一致(防 tag 与产物版本漂移)
//
// 附带结构断言(--check / --write / --check-tag 前置跑;--print 为纯查询不跑):
//   ① tauri.conf.json 不得含顶层 "version" —— 缺省时 Tauri codegen 回退 CARGO_PKG_VERSION,
//      这才是单源;字段若指向 Cargo.toml 会 JSON 解析失败(tauri-utils config.rs,§3.2 已核)。
//   ② src-tauri/Cargo.toml 须为 version.workspace = true,禁独立版本回潮。

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const p = (rel) => join(root, rel);

function fail(msg) {
  console.error(`[sync-version] ${msg}`);
  process.exit(1);
}

// 从根 Cargo.toml 提取 [workspace.package] 段内的 version。
// 手解而非引 TOML 库(控依赖):段边界 = 下一个以 [ 开头的行。
function readAnchor() {
  const toml = readFileSync(p('Cargo.toml'), 'utf8');
  let inSection = false;
  for (const line of toml.split(/\r?\n/)) {
    const t = line.trim();
    if (t.startsWith('[')) {
      inSection = t === '[workspace.package]';
      continue;
    }
    if (!inSection) continue;
    const m = t.match(/^version\s*=\s*"([^"]+)"/);
    if (m) return m[1];
  }
  return fail('根 Cargo.toml 未找到 [workspace.package] version —— 版本锚缺失');
}

function assertStructure() {
  const conf = JSON.parse(readFileSync(p('src-tauri/tauri.conf.json'), 'utf8'));
  if ('version' in conf) {
    fail(
      'tauri.conf.json 含独立 "version" 字段,违反单一事实源 —— 删除该字段(Tauri 自动回退 CARGO_PKG_VERSION)',
    );
  }
  const srcTauri = readFileSync(p('src-tauri/Cargo.toml'), 'utf8');
  if (!/^\s*version\.workspace\s*=\s*true/m.test(srcTauri)) {
    fail('src-tauri/Cargo.toml 未用 version.workspace = true —— 独立版本回潮,改回 workspace 继承');
  }
}

const mode = process.argv[2];
const anchor = readAnchor();

if (mode === '--print') {
  console.log(anchor);
  process.exit(0);
}

assertStructure();

if (mode === '--check-tag') {
  const tag = process.argv[3];
  if (!tag) fail('--check-tag 需要 tag 参数(如 v0.1.0)');
  if (tag !== `v${anchor}`) {
    fail(`tag [${tag}] 与版本锚 [v${anchor}] 不一致 —— 先升锚(根 Cargo.toml workspace.package.version)再打 tag`);
  }
  console.log(`[sync-version] tag 一致:${tag} == v${anchor}`);
  process.exit(0);
}

const pkgPath = p('package.json');
const pkgRaw = readFileSync(pkgPath, 'utf8');
const pkg = JSON.parse(pkgRaw);

if (mode === '--check') {
  if (pkg.version !== anchor) {
    fail(
      `package.json version [${pkg.version}] != 锚 [${anchor}] —— 跑 \`node scripts/sync-version.mjs --write\` 同步`,
    );
  }
  console.log(`[sync-version] 一致:${anchor}`);
} else if (mode === '--write') {
  if (pkg.version === anchor) {
    console.log(`[sync-version] 已同步,无需写回:${anchor}`);
  } else {
    // 定点替换首个 "version" 行(顶层 version 位于 name/private 之后、任何嵌套对象之前),
    // 不走 JSON.stringify 整文件重排,保留既有格式与键序。
    const next = pkgRaw.replace(/("version"\s*:\s*")[^"]+(")/, `$1${anchor}$2`);
    if (next === pkgRaw) fail('package.json 未找到 "version" 字段');
    writeFileSync(pkgPath, next);
    console.log(`[sync-version] 已写回 package.json:${pkg.version} → ${anchor}`);
  }
} else {
  fail('用法:--check | --write | --print | --check-tag <tag>');
}
