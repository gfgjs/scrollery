// 第三方依赖归属清单(NOTICE.md)+ SBOM(CycloneDX 1.5 JSON)生成器。
// G3「基建/法务(发行前置)」中「NOTICE + SBOM 生成」项的落地(2026-07-05)。
//
// 覆盖范围 = 实际分发物的依赖闭包:
//   Rust 侧:主程序、ai-worker、psd-worker，以及独立 workspace 的 raw-worker。
//           raw-worker 用 x86_64-pc-windows-gnu 编译但随 Windows 安装包分发，不能漏扫。
//   npm 侧:package-lock.json(v3)中非 dev 的生产依赖闭包，加上虽标为 devDependency、
//           但其原生 DLL 被 build-ai-worker.mjs 抽取进安装包的 onnxruntime-node。
//   运行时/内嵌侧:LibRaw、BtbN FFmpeg 等不由上述 lockfile 完整表达的实际发货组件，
//           由下方手工登记表补上并进入 NOTICE/SBOM 与 strong-copyleft 门控。
//   vendored 侧:非包管理器依赖(不在两份 lockfile),但源码 vendored 进仓、打进前端 bundle
//           随产品分发(如 foliate-js)。此类无法从 lockfile 采集,由下方 VENDORED_FRONTEND
//           手工登记表补上——新增/升级 vendored 源时须同步该表(见对应 VENDOR.md)。
//
// 产物:
//   NOTICE.md(仓库根,tracked)——归属清单;内容完全由两份 lockfile 决定,无时间戳,
//     重复生成逐字节稳定(--check 的前提)。
//   target/sbom/scrollery.cdx.json(untracked)——CycloneDX SBOM,发布时作为工件附带
//     (接入 release 流水线归 Part7-T16;此前手动生成)。
//
// 用法:
//   node scripts/generate-notice.mjs           # 生成 NOTICE.md + SBOM
//   node scripts/generate-notice.mjs --check   # 只重算并与现有 NOTICE.md 比对,漂移即非零退出
//   node scripts/generate-notice.mjs --check --sbom  # 比对通过后再产出 SBOM(不写 NOTICE.md)
//
// 法务边界(如实):本工具产出的是**归属清单与机器可读物料**,不是法律意见;
// license 兼容性终审、完整 license 文本捆绑(cargo-about 级)与各商店政策核验
// 属 G3「上架前须重新核验」的人工环节,不因本工具存在而免除。

import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { VENDORED_VERSION_FIELDS, verifyVendoredArtifacts } from './lib/vendored-artifacts.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CHECK_MODE = process.argv.includes('--check');
// SBOM 产出:默认模式本来就写;--check 下须显式 --sbom(校验通过才写,校验失败不写)。
const SBOM_MODE = !CHECK_MODE || process.argv.includes('--sbom');

// 发货 Rust 根与平台。raw-worker 是独立 workspace，必须保留 manifest_path。
const SHIP_RUST_ROOTS = [
  { manifest: 'Cargo.toml', package: 'scrollery', target: 'x86_64-pc-windows-msvc' },
  { manifest: 'Cargo.toml', package: 'ai-worker', target: 'x86_64-pc-windows-msvc' },
  { manifest: 'Cargo.toml', package: 'psd-worker', target: 'x86_64-pc-windows-msvc' },
  {
    manifest: 'crates/exotic-workers/raw-worker/Cargo.toml',
    package: null,
    target: 'x86_64-pc-windows-gnu',
  },
];

// 原生 DLL 实际来自 devDependency；只要它被复制进安装包，就不能继续按 dev 工具排除。
const SHIPPED_NPM_RUNTIME_PACKAGES = new Set(['onnxruntime-node']);

// ── vendored 第三方源码(手工登记)─────────────────────────────────────────────
// 不经包管理器、但 vendored 进仓并打进前端 bundle 随产品分发的第三方源码。lockfile 采集不到,
// 故在此手工登记归属;新增/升级 vendored 源时同步维护(权威细节见各自 VENDOR.md)。
// 计入 license summary 与 strong-copyleft 门控(vendored 若混入 copyleft,门须能拦下)。
const VENDORED_FRONTEND = [
  {
    name: 'foliate-js',
    version: '78914ae', // 钉定 commit 短号(见 src/vendor/foliate-js/VENDOR.md)
    license: 'MIT',
    homepage: 'https://github.com/johnfactotum/foliate-js',
    purl: 'pkg:github/johnfactotum/foliate-js@78914aef4466eb960965702401634c2cb348e9b1',
    note: 'vendored at pinned commit; PDF backend stripped',
  },
  // Vditor 3.11.3 的发行物里 vendored 的两个独立上游编译件。二者都不在任何 lockfile:
  // Graphviz 2.40.1 只以 Viz.js 2.1.2 的 Emscripten 单文件产物形式存在(EPL-1.0,未修改),
  // Lute 1.7.6 只以 Go 编译产物 lute.min.js 形式存在(MulanPSL-2.0,未修改)。
  // 版本由产物内声明坐实,并与 third-party/license-sources.json 的钉定值交叉校验:
  // 版本常量、产物字节、上游正文三者不可能各自漂移(见 verifyPinnedVendoredArtifacts)。
  {
    name: 'Graphviz (embedded in Viz.js full.render.js)',
    version: '2.40.1',
    license: 'EPL-1.0',
    homepage: 'https://gitlab.com/graphviz/graphviz',
    purl: 'pkg:generic/graphviz@2.40.1',
    note:
      'Emscripten object code built by Viz.js 2.1.2 (MIT); unmodified upstream, ' +
      'source availability entry recorded in legal/graphviz',
  },
  {
    name: 'Lute (lute.min.js)',
    version: '1.7.6',
    license: 'MulanPSL-2.0',
    homepage: 'https://github.com/88250/lute',
    purl: 'pkg:github/88250/lute@v1.7.6',
    note: 'Go object code shipped by Vditor 3.11.3; unmodified upstream, text in legal/lute',
  },
];

// 不在根 lockfile 中、但会随产品或运行时分发的闭包成员。许可证表达式进入
// NOTICE/SBOM；完整文本由 prepare-legal-resources.mjs 生成到安装包 legal/ 目录。
const SHIPPED_RUNTIME_COMPONENTS = [
  {
    name: 'LibRaw (embedded by rsraw-sys)',
    version: 'rsraw-sys-0.1.1',
    license: 'CDDL-1.0 OR LGPL-2.1-only',
    homepage: 'https://www.libraw.org',
    purl: 'pkg:generic/LibRaw@rsraw-sys-0.1.1',
    // 许可表达式为择一；本项目选 LGPL-2.1-only 分支，上游 CDDL-1.0 正文一并保留。
    note:
      'embedded in raw-worker; LGPL-2.1-only branch elected from CDDL-1.0 OR LGPL-2.1-only, ' +
      'upstream texts kept under legal/LibRaw; the OSS source release ships no RAW binary, and a ' +
      'future static-link distribution still owes LGPL-2.1 source/relink materials or the §3 ' +
      'GPL-3.0 route — the installer is not thereby declared compliant',
    eco: 'embedded-runtime',
  },
  {
    name: 'FFmpeg (BtbN LGPL-shared runtime)',
    version: 'autobuild-2026-07-24-13-32',
    license: 'LGPL-2.1-or-later',
    homepage: 'https://github.com/BtbN/FFmpeg-Builds',
    purl: 'pkg:github/BtbN/FFmpeg-Builds@autobuild-2026-07-24-13-32',
    note: 'downloaded on demand; fixed FFmpeg/BtbN license texts are emitted under legal/ffmpeg; source link is shown in the app',
    eco: 'runtime-download',
  },
];

// ── vendored 编译产物的版本/字节锚定 ─────────────────────────────────────────
// 校验实现只有一处(scripts/lib/vendored-artifacts.mjs),这里只补最后一段接线:
//   ① 登记表版本 = 钉定版本:VENDORED_FRONTEND 与 third-party/license-sources.json 不一致
//      = 有人只改了一边;
//   ②③ 产物尺寸/字节/版本标记由 verifyVendoredArtifacts 对磁盘产物逐项复核。
// 目的不是判定许可兼容性,而是保证 NOTICE/legal 里写的版本确有所指。
const PINNED_MANIFEST_PATH = path.join(repo, 'third-party', 'license-sources.json');
const VENDORED_COMPONENT_BY_NAME = {
  'Graphviz (embedded in Viz.js full.render.js)': 'graphviz',
  'Lute (lute.min.js)': 'lute',
};

export function verifyPinnedVendoredArtifacts() {
  for (const [name, component] of Object.entries(VENDORED_COMPONENT_BY_NAME)) {
    const declared = VENDORED_FRONTEND.find((item) => item.name === name)?.version;
    const pinned = VENDORED_VERSION_FIELDS[component];
    if (declared !== pinned) {
      throw new Error(
        `vendored 版本与钉定清单不一致: ${name}\n` +
          `VENDORED_FRONTEND ${declared ?? 'missing'} vs ${pinned ?? 'missing'}`,
      );
    }
  }
  return verifyVendoredArtifacts(JSON.parse(fs.readFileSync(PINNED_MANIFEST_PATH, 'utf8')), repo);
}

// ── cargo tree 行解析 ─────────────────────────────────────────────────────────
// 输入格式(--prefix none --format "{p}~{l}"):
//   name v1.2.3~MIT OR Apache-2.0            常规 registry crate
//   name v1.2.3~MIT OR Apache-2.0 (*)        dedupe 重复标记
//   name v1.2.3 (proc-macro)~MPL-2.0         proc-macro 注记(编译期件;从宽计入归属)
//   name v0.1.0 (D:\path\to\crate)~          path 依赖 = 第一方 workspace crate,跳过
export function parseCargoLine(line) {
  // 先剥 ANSI 转义: CI 若设 CARGO_TERM_COLOR=always 会给管道输出着色
  // (实测 dedupe 标记被染成 \e[33m\e[2m(*)\e[39m\e[22m 击穿尾部剥离正则),
  // 采集端已 --color never 钉死,此处为纵深防御。
  const plain = line.replace(/\x1b\[[0-9;]*m/g, '');
  const trimmed = plain.replace(/\s*\(\*\)\s*$/, '').trim();
  if (!trimmed) return null;
  const m = trimmed.match(/^(\S+) v(\S+)( \(([^)]*)\))?~(.*)$/);
  if (!m) return null;
  const [, name, version, , annotation, license] = m;
  // 绝对路径注记(win 盘符或 posix 根)= 本仓 workspace 成员,非第三方。
  const firstParty = !!annotation && /^([A-Za-z]:[\\/]|\/)/.test(annotation);
  return { name, version, license: license.trim(), firstParty };
}

// ── license 表达式处理 ────────────────────────────────────────────────────────
// 老式斜杠写法(MIT/Apache-2.0)语义即 OR,归一化后才是合法 SPDX 表达式(SBOM 用);
// NOTICE 保留原文,忠实于上游声明。
export function normalizeSpdx(raw) {
  const s = (raw || '').trim();
  if (!s) return null;
  return s.replace(/\s*\/\s*/g, ' OR ').replace(/\s+/g, ' ');
}

// 复核分级:strong = 强 copyleft(出现即须人工审;OR 择一可解也要显式确认),
// weak = 文件级/弱 copyleft(未修改使用通常合规,NOTICE 列明即可),null = 无旗标。
export function flagLicense(expr) {
  if (!expr) return 'unknown';
  if (/\b(A?GPL|SSPL|EUPL)\b|\bGPL-\d/i.test(expr) && !/\bLGPL\b/i.test(expr)) return 'strong';
  if (/\bLGPL\b/i.test(expr)) return 'strong';
  if (/\b(MPL|CDDL|EPL)\b/i.test(expr)) return 'weak';
  return null;
}

// ── 采集:Rust 三棵发货树并集 ─────────────────────────────────────────────────
function collectRust() {
  const seen = new Map(); // key = name@version
  for (const root of SHIP_RUST_ROOTS) {
    const args = ['tree', '--manifest-path', path.join(repo, root.manifest)];
    if (root.package) args.push('-p', root.package);
    args.push(
      '-e', 'normal', '--locked', '--prefix', 'none',
      '--color', 'never', '--target', root.target, '--format', '{p}~{l}'
    );
    const out = execFileSync('cargo', args, {
      cwd: repo,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    for (const line of out.split(/\r?\n/)) {
      const dep = parseCargoLine(line);
      if (!dep || dep.firstParty) continue;
      seen.set(`${dep.name}@${dep.version}`, dep);
    }
  }
  return [...seen.values()];
}

// ── 采集:npm 生产依赖闭包(lockfile v3) ─────────────────────────────────────
export function collectNpmFromLock(lock) {
  const out = [];
  for (const [key, entry] of Object.entries(lock.packages || {})) {
    if (key === '' || (entry.dev && !SHIPPED_NPM_RUNTIME_PACKAGES.has(key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length)))) continue;
    if (!key.includes('node_modules/')) continue;
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);
    out.push({
      name,
      version: entry.version,
      license: (entry.license || '').trim(),
      note: SHIPPED_NPM_RUNTIME_PACKAGES.has(name)
        ? 'devDependency; native DLLs are copied into the installer by build-ai-worker.mjs'
        : '',
    });
  }
  return out;
}

// ── 渲染 NOTICE.md(确定性:仅由 lockfile 内容决定,无时间戳) ─────────────────
// 排序禁用 localeCompare: ICU collation 随环境 locale 变(本机 zh-CN vs runner en-US),
// 排序即字节序,必须用纯码点比较才守得住「内容只由 lockfile 决定」的确定性承诺。
const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const byName = (a, b) =>
  cmp(a.name.toLowerCase(), b.name.toLowerCase()) || cmp(a.version, b.version);

function renderNotice(rustDeps, npmDeps, appVersion) {
  const all = [
    ...rustDeps.map((d) => ({ ...d, eco: 'crates.io' })),
    ...npmDeps.map((d) => ({ ...d, eco: 'npm' })),
    ...VENDORED_FRONTEND.map((d) => ({ ...d, eco: 'vendored' })),
    ...SHIPPED_RUNTIME_COMPONENTS,
  ];
  const licCount = new Map();
  for (const d of all) {
    const k = d.license || '(no license field — see review notes)';
    licCount.set(k, (licCount.get(k) || 0) + 1);
  }
  const summary = [...licCount.entries()].sort((a, b) => b[1] - a[1] || cmp(a[0], b[0]));

  const weak = all.filter((d) => flagLicense(normalizeSpdx(d.license)) === 'weak').sort(byName);
  const strong = all.filter((d) => flagLicense(normalizeSpdx(d.license)) === 'strong').sort(byName);
  const unknown = all.filter((d) => !normalizeSpdx(d.license)).sort(byName);

  const L = [];
  L.push('# Third-Party Notices');
  L.push('');
  // NOTICE 署名头(第三方归属清单的分发载体;署名主体 2026-07-06 拍板为集体式,换法律主体零改动)
  L.push('Scrollery');
  L.push('Copyright 2026 The Scrollery Authors');
  L.push('');
  L.push('Scrollery incorporates third-party open-source software.');
  L.push('This file lists the packages distributed with the application (desktop app,');
  L.push('bundled workers, and the compiled frontend) together with their declared licenses.');
  L.push('');
  L.push('本文件由 `scripts/generate-notice.mjs` 从 `Cargo.lock` / `package-lock.json` 生成,');
  L.push('**请勿手改**;依赖变更后重新生成(CI/发布流程以 `--check` 校验新鲜度)。');
  L.push('完整 license 文本由 `scripts/prepare-legal-resources.mjs` 生成到安装包 `legal/`;');
  L.push('包根未携带文本的条目使用仓库内固定 hash 的 SPDX 正文,映射记录在 `legal/MANIFEST.json`;');
  L.push('构建阶段不联网,只校验并复制 `third-party/licenses/` 中的已固定材料。');
  L.push('');
  L.push(`Application version at generation time: ${appVersion}`);
  L.push('');
  L.push('## License summary');
  L.push('');
  L.push('| License (as declared) | Packages |');
  L.push('| --- | ---: |');
  for (const [lic, n] of summary) L.push(`| ${lic} | ${n} |`);
  L.push('');
  L.push(`## Rust crates (${rustDeps.length}) — desktop application and workers`);
  L.push('');
  L.push('Dependency closure (normal deps) of the shipped binaries `scrollery`,');
  L.push('`ai-worker` and `psd-worker`, resolved for `x86_64-pc-windows-msvc`.');
  L.push('');
  L.push('| Crate | Version | License |');
  L.push('| --- | --- | --- |');
  for (const d of [...rustDeps].sort(byName)) {
    L.push(`| [${d.name}](https://crates.io/crates/${d.name}) | ${d.version} | ${d.license || '—'} |`);
  }
  L.push('');
  L.push(`## npm packages (${npmDeps.length}) — frontend bundle`);
  L.push('');
  L.push('Production dependency closure from `package-lock.json`, plus explicitly shipped native runtime packages.');
  L.push('');
  L.push('| Package | Version | License | Shipping note |');
  L.push('| --- | --- | --- | --- |');
  for (const d of [...npmDeps].sort(byName)) {
    L.push(`| [${d.name}](https://www.npmjs.com/package/${d.name}) | ${d.version} | ${d.license || '—'} | ${d.note || ''} |`);
  }
  L.push('');
  L.push(`## Vendored source (${VENDORED_FRONTEND.length}) — bundled into frontend`);
  L.push('');
  L.push('第三方源码以钉定版本 vendored 进本仓(不经包管理器),打进前端 bundle 随产品分发。');
  L.push('对应源目录的许可证文件会进入安装包 `legal/`(见 VENDOR.md / LICENSE)。');
  L.push('');
  L.push('| Source | Version (pinned) | License | Notes |');
  L.push('| --- | --- | --- | --- |');
  for (const d of [...VENDORED_FRONTEND].sort(byName)) {
    L.push(`| [${d.name}](${d.homepage}) | ${d.version} | ${d.license || '—'} | ${d.note || ''} |`);
  }
  L.push('');
  L.push(`## Additional shipped runtimes (${SHIPPED_RUNTIME_COMPONENTS.length})`);
  L.push('');
  L.push('这些组件不由根 lockfile 的普通依赖闭包完整表达，但其代码或二进制会进入产品运行路径。');
  L.push('完整许可证/归属材料由 `node scripts/prepare-legal-resources.mjs` 生成到安装包 `legal/`。');
  L.push('');
  L.push('| Component | Version | License | Notes |');
  L.push('| --- | --- | --- | --- |');
  for (const d of [...SHIPPED_RUNTIME_COMPONENTS].sort(byName)) {
    L.push(`| [${d.name}](${d.homepage}) | ${d.version} | ${d.license || '—'} | ${d.note || ''} |`);
  }
  L.push('');
  L.push('## Review notes');
  L.push('');
  if (strong.length) {
    L.push('### ⚠ Strong-copyleft flagged (manual legal review REQUIRED before release)');
    L.push('');
    for (const d of strong) L.push(`- ${d.name}@${d.version} (${d.eco}): ${d.license}`);
    L.push('');
  } else {
    L.push('- No strong-copyleft (GPL/AGPL/LGPL/SSPL/EUPL) licenses detected in the shipped closure.');
  }
  if (weak.length) {
    L.push('- Weak/file-level copyleft packages (used in unmodified form; source available upstream):');
    for (const d of weak) L.push(`  - ${d.name}@${d.version} (${d.eco}): ${d.license}`);
  }
  if (unknown.length) {
    L.push('- Packages without a machine-readable license field (verify upstream before release):');
    for (const d of unknown) L.push(`  - ${d.name}@${d.version} (${d.eco})`);
  }
  L.push('');
  L.push('生成物为归属清单,非法律意见。上架前人工环节(G3):license 兼容性终审、');
  L.push('`legal/MANIFEST.json` 中的包级文件/SPDX 正文映射仍需按上游来源逐项复核、各分发渠道政策核验。');
  L.push('');
  return L.join('\n');
}

// ── SBOM(CycloneDX 1.5;确定性:无 serialNumber/时间戳) ─────────────────────
function purlNpm(name, version) {
  // scoped 包的 @ 须百分号编码(purl spec):@scope/pkg → %40scope/pkg
  return `pkg:npm/${name.replace(/^@/, '%40')}@${version}`;
}

function renderSbom(rustDeps, npmDeps, appVersion) {
  const components = [
    ...rustDeps.map((d) => ({
      type: 'library',
      name: d.name,
      version: d.version,
      purl: `pkg:cargo/${d.name}@${d.version}`,
      ...(normalizeSpdx(d.license) ? { licenses: [{ expression: normalizeSpdx(d.license) }] } : {}),
    })),
    ...npmDeps.map((d) => ({
      type: 'library',
      name: d.name,
      version: d.version,
      purl: purlNpm(d.name, d.version),
      ...(normalizeSpdx(d.license) ? { licenses: [{ expression: normalizeSpdx(d.license) }] } : {}),
    })),
    ...VENDORED_FRONTEND.map((d) => ({
      type: 'library',
      name: d.name,
      version: d.version,
      purl: d.purl,
      ...(normalizeSpdx(d.license) ? { licenses: [{ expression: normalizeSpdx(d.license) }] } : {}),
    })),
    ...SHIPPED_RUNTIME_COMPONENTS.map((d) => ({
      type: 'library',
      name: d.name,
      version: d.version,
      purl: d.purl,
      ...(normalizeSpdx(d.license) ? { licenses: [{ expression: normalizeSpdx(d.license) }] } : {}),
    })),
  ].sort((a, b) => cmp(a.purl, b.purl));
  return {
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    version: 1,
    metadata: {
      component: { type: 'application', name: 'scrollery', version: appVersion },
      tools: [{ name: 'generate-notice.mjs', vendor: 'scrollery' }],
    },
    components,
  };
}

// ── selftest(仓例:verify-channel-bundle 同款纪律,防解析器退化成只会 PASS 的空壳) ──
function selftest() {
  const assert = (cond, msg) => {
    if (!cond) {
      console.error(`selftest FAILED: ${msg}`);
      process.exit(2);
    }
  };
  const a = parseCargoLine('anyhow v1.0.103~MIT OR Apache-2.0');
  assert(a && a.name === 'anyhow' && a.version === '1.0.103' && !a.firstParty, 'registry 行解析');
  const b = parseCargoLine('bitflags v2.13.0~MIT OR Apache-2.0 (*)');
  assert(b && b.name === 'bitflags' && b.license === 'MIT OR Apache-2.0', 'dedupe 标记剥离');
  // CI 实测字节序列(run 28732061603):CARGO_TERM_COLOR=always 下的着色 dedupe 标记
  const b2 = parseCargoLine('brotli v8.0.4~BSD-3-Clause AND MIT \x1b[33m\x1b[2m(*)\x1b[39m\x1b[22m');
  assert(b2 && b2.license === 'BSD-3-Clause AND MIT', 'ANSI 着色 dedupe 标记剥离');
  const c = parseCargoLine('cssparser-macros v0.6.1 (proc-macro)~MPL-2.0');
  assert(c && c.name === 'cssparser-macros' && !c.firstParty && c.license === 'MPL-2.0', 'proc-macro 注记');
  const d = parseCargoLine('scrollery v0.1.0 (D:\\photoapp\\scrollery\\src-tauri)~');
  assert(d && d.firstParty, 'windows path 依赖判第一方');
  const e = parseCargoLine('some-crate v0.1.0 (/home/u/repo/crates/x)~');
  assert(e && e.firstParty, 'posix path 依赖判第一方');
  assert(parseCargoLine('') === null && parseCargoLine('  (*)  ') === null, '空行/纯标记行');
  assert(normalizeSpdx('MIT/Apache-2.0') === 'MIT OR Apache-2.0', '斜杠归一化');
  assert(normalizeSpdx('Apache-2.0 / MIT') === 'Apache-2.0 OR MIT', '带空格斜杠归一化');
  assert(normalizeSpdx('') === null, '空 license');
  assert(flagLicense('GPL-3.0-only') === 'strong', 'GPL 旗标');
  assert(flagLicense('LGPL-2.1 OR MIT') === 'strong', 'LGPL 择一仍旗标');
  assert(flagLicense('MPL-2.0') === 'weak', 'MPL 旗标');
  assert(flagLicense('MIT OR Apache-2.0') === null, '宽松无旗标');
  const lock = {
    packages: {
      '': { version: '0.1.0' },
      'node_modules/vue': { version: '3.5.0', license: 'MIT' },
      'node_modules/@scope/pkg/node_modules/inner': { version: '1.0.0', license: 'ISC' },
      'node_modules/devtool': { version: '1.0.0', dev: true, license: 'MIT' },
      'node_modules/onnxruntime-node': { version: '1.26.0', dev: true, license: 'MIT' },
    },
  };
  const npm = collectNpmFromLock(lock);
  assert(npm.length === 3, 'dev 排除 + 根排除 + shipped runtime 保留');
  assert(npm.find((p) => p.name === 'inner'), '嵌套 node_modules 取末段包名');
  assert(npm.find((p) => p.name === 'onnxruntime-node'), 'native runtime devDependency 保留');
  assert(purlNpm('@tauri-apps/api', '2.0.0') === 'pkg:npm/%40tauri-apps/api@2.0.0', 'scoped purl 编码');
  // vendored 登记表字段完整性(防有人加条目漏字段导致 NOTICE 渲染出空单元格/坏 purl)
  assert(
    VENDORED_FRONTEND.every((v) => v.name && v.version && v.license && v.homepage && v.purl),
    'vendored 登记表字段完整(name/version/license/homepage/purl)'
  );
}

// ── 主流程 ────────────────────────────────────────────────────────────────────
function writeSbom(rustDeps, npmDeps, appVersion) {
  const sbomDir = path.join(repo, 'target', 'sbom');
  fs.mkdirSync(sbomDir, { recursive: true });
  const sbomPath = path.join(sbomDir, 'scrollery.cdx.json');
  fs.writeFileSync(sbomPath, JSON.stringify(renderSbom(rustDeps, npmDeps, appVersion), null, 2) + '\n');
  return sbomPath;
}

selftest();
verifyPinnedVendoredArtifacts();

const appVersion = JSON.parse(fs.readFileSync(path.join(repo, 'package.json'), 'utf8')).version;
const lock = JSON.parse(fs.readFileSync(path.join(repo, 'package-lock.json'), 'utf8'));
const rustDeps = collectRust();
const npmDeps = collectNpmFromLock(lock);
const notice = renderNotice(rustDeps, npmDeps, appVersion);
const noticePath = path.join(repo, 'NOTICE.md');

const strongCount = [...rustDeps, ...npmDeps, ...VENDORED_FRONTEND, ...SHIPPED_RUNTIME_COMPONENTS].filter(
  (d) => flagLicense(normalizeSpdx(d.license)) === 'strong'
).length;

if (CHECK_MODE) {
  // 检出文件在 autocrlf 环境为 CRLF 而生成串恒为 LF 故先归一化行尾再比对
  const normEol = (s) => s.replace(/\r\n/g, '\n');
  const existing = fs.existsSync(noticePath) ? fs.readFileSync(noticePath, 'utf8') : '';
  if (normEol(existing) !== normEol(notice)) {
    console.error('NOTICE.md 与 lockfile 不同步——依赖已变更,请重跑 node scripts/generate-notice.mjs 并提交。');
    // 门失败须能自诊断: 打印前 10 处差异行(行号 + 两侧内容),否则 CI 红只有一句废话
    const aL = normEol(existing).split('\n');
    const bL = normEol(notice).split('\n');
    let shown = 0;
    for (let i = 0; i < Math.max(aL.length, bL.length) && shown < 10; i++) {
      if (aL[i] !== bL[i]) {
        console.error(`  L${i + 1} 现有: ${aL[i] ?? '<EOF>'}`);
        console.error(`  L${i + 1} 应为: ${bL[i] ?? '<EOF>'}`);
        shown++;
      }
    }
    process.exit(1);
  }
  console.log(`✓ NOTICE.md 新鲜(rust ${rustDeps.length} + npm ${npmDeps.length} + vendored ${VENDORED_FRONTEND.length} + runtime ${SHIPPED_RUNTIME_COMPONENTS.length};strong-copyleft 旗标 ${strongCount})`);
  if (SBOM_MODE) console.log(`SBOM 已生成:${writeSbom(rustDeps, npmDeps, appVersion)}`);
} else {
  fs.writeFileSync(noticePath, notice);
  console.log(`NOTICE.md 已生成(rust ${rustDeps.length} + npm ${npmDeps.length} + vendored ${VENDORED_FRONTEND.length} + runtime ${SHIPPED_RUNTIME_COMPONENTS.length} 个第三方组件)`);
  console.log(`SBOM 已生成:${writeSbom(rustDeps, npmDeps, appVersion)}`);
  if (strongCount > 0) {
    console.warn(`⚠ 检出 ${strongCount} 个 strong-copyleft 旗标包——发布前必须人工法务复核(见 NOTICE.md Review notes)`);
  }
}
