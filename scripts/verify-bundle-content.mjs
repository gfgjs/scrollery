#!/usr/bin/env node
// 安装包内容断言：AI/RAW/video/enhance/native-thumbnail worker 及其运行时必须随包分发。
// externalBin/resources 缺项时失败。
//
// 三层校验:
//   ① staging 层:src-tauri/binaries/ 下 ai/video/raw/enhance/native-thumbnail worker + ORT 四件套必须存在;
//   ② conf 层:tauri.conf.json 的 externalBin 含五个 worker、bundle.resources 声明四 DLL;
//   ③ 安装包层:target/release/bundle/ 存在 MSI/NSIS 时,用 7z 拆包断言载荷和法律资源完整。
//      NSIS 保留原名,归一化后按名比对;MSI 的资源文件被 Tauri 匿名化为 PathFile_<hash>(无扩展名,
//      Bin_ 前缀侧车保留原名),只能按字节尺寸在条目尺寸集中匹配(缺项/截断都会错开尺寸)。
//      实测 7-Zip 24+ 会递归展开 MSI 内嵌 app.cab,单次 `7z l -slt` 即拿到全部载荷条目。
//
// 用法:node scripts/verify-bundle-content.mjs
//   --require-bundle:缺安装包/7z 均失败；--installer=<path> 可重复，限定本次待验收产物。
//   - 安装包缺失或 7z 不可用 → 只跑 ①②,打印说明;
//   - CI 且安装包存在但 7z 不可用 → 硬失败(发布路径必须做载荷级验证)。
// 脚本启动先跑 MSI 截断载荷自检。

import assert from 'node:assert/strict';
import { testCase } from './lib/test-case.mjs';
import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONFIG_PATH = path.join(ROOT, 'src-tauri', 'tauri.conf.json');
const BINARIES_DIR = path.join(ROOT, 'src-tauri', 'binaries');
const BUNDLE_DIR = path.join(ROOT, 'target', 'release', 'bundle');

// 安装包载荷必含项(归一化名)。AI/RAW/video/enhance/native-thumbnail worker + ORT 四件套。
const REQUIRED_PAYLOAD = [
  'ai_worker.exe',
  'onnxruntime.dll',
  'directml.dll',
  'dxcompiler.dll',
  'dxil.dll',
  'raw_worker.exe',
  'video_worker.exe',
  'enhance_worker.exe',
  'native_thumbnail_worker.exe',
];

// AGPL-3.0-only 要求随包提供许可证全文与对应源码入口(§4/§6)。
// 以下清单同时承载第三方归属与项目商业授权说明；商业授权说明是本项目额外的交付要求。
const REQUIRED_LEGAL_RESOURCES = [
  ['../LICENSE', 'LICENSE'],
  ['../NOTICE.md', 'NOTICE.md'],
  ['../SOURCE.md', 'SOURCE.md'],
  ['../COMMERCIAL.md', 'COMMERCIAL.md'],
  ['../ADDITIONAL-PERMISSION.md', 'ADDITIONAL-PERMISSION.md'],
  ['../target/legal', 'legal'],
];

// `legal/` 下的文件名是 prepare-legal-resources.mjs 的稳定输出；根三个文件直接映射。
const REQUIRED_BUNDLE_LEGAL = [
  'license',
  'notice.md',
  'source.md',
  'commercial.md',
  'additional-permission.md',
  'legal/foliate-js/license',
  'legal/libraw/copyright',
  'legal/libraw/license.cddl',
  'legal/libraw/license.lgpl',
  'legal/onnxruntime/license',
  'legal/onnxruntime/thirdpartynotices.txt',
  'legal/ffmpeg/license.md',
  'legal/ffmpeg/copying.lgplv2.1',
  'legal/ffmpeg/btbn-license',
  'legal/graphviz/graphviz-2.40.1-license.txt',
  'legal/graphviz/viz.js-2.1.2-license.txt',
  'legal/lute/lute-1.7.6-license.txt',
  'legal/manifest.json',
];

// ── 纯函数检查器(供 selftest)───────────────────────────────────────────────────

/** 归一化载荷条目名:小写、- → _、剥 MSI 的 Bin_ 前缀。例:Bin_raw_worker.exe → raw_worker.exe */
export function normalizeEntryName(name) {
  let n = name.toLowerCase().replace(/-/g, '_');
  if (n.startsWith('bin_')) n = n.slice(4);
  return n;
}

/** 检查归一化条目集合是否覆盖必含项,返回违例名数组。 */
export function checkPayloadEntries(entryNames) {
  const set = new Set(entryNames.map(normalizeEntryName));
  return REQUIRED_PAYLOAD.filter((r) => !set.has(r));
}

/** MSI 中资源文件被 Tauri 匿名化为 `PathFile_<hash>`(无扩展名,名字无从比对),只能按尺寸。 */
const MSI_ANONYMIZED = new Set(['onnxruntime.dll', 'directml.dll', 'dxcompiler.dll', 'dxil.dll']);

/**
 * 检查 MSI 载荷:侧车按名(Bin_ 前缀保留原名),被匿名化的 DLL 按字节尺寸在条目尺寸多重集中
 * 逐一匹配——缺项/截断(0 字节或尺寸不符)都会错开尺寸被擒。stagedSizes 缺文件的 DLL 跳过
 * (staging 层①已报),避免与 ① 重复。
 */
export function checkPayloadEntriesMs(entryNames, entrySizes, stagedSizes) {
  const byName = new Set(entryNames.map(normalizeEntryName));
  const pool = [...entrySizes];
  const v = [];
  for (const r of REQUIRED_PAYLOAD) {
    if (MSI_ANONYMIZED.has(r)) {
      const want = stagedSizes[r];
      if (want === undefined) continue; // staging 缺该 DLL → ① 层报,此处不重复
      const j = pool.indexOf(want);
      if (j < 0) v.push(r);
      else pool.splice(j, 1);
    } else if (!byName.has(r)) {
      v.push(r);
    }
  }
  return v;
}

/** 检查 staging 目录:侧车 exe + ORT DLL 齐备,返回违例描述数组。 */
export function checkStaged(binariesDir) {
  if (!existsSync(binariesDir)) return [`staging 目录缺失:${binariesDir}`];
  return checkStagedNames(readdirSync(binariesDir));
}

function checkStagedNames(names) {
  const v = [];
  if (!names.some((n) => /^enhance-worker-.+\.exe$/.test(n))) {
    v.push('staging 缺 enhance-worker-<triple>.exe(跑 npm run build:enhance-worker)');
  }
  if (!names.some((n) => /^ai-worker-.+\.exe$/.test(n))) {
    v.push('staging 缺 ai-worker-<triple>.exe(跑 npm run build:ai-worker)');
  }
  if (!names.some((n) => /^raw-worker-.+\.exe$/.test(n))) {
    v.push('staging 缺 raw-worker-<triple>.exe(跑 npm run build:raw-worker)');
  }
  if (!names.some((n) => /^video-worker-.+\.exe$/.test(n))) {
    v.push('staging 缺 video-worker-<triple>.exe(跑 npm run build:video-worker)');
  }
  if (!names.some((n) => /^native-thumbnail-worker-.+\.exe$/.test(n))) {
    v.push('staging 缺 native-thumbnail-worker-<triple>.exe(跑 npm run build:native-thumbnail-worker)');
  }
  for (const dll of ['onnxruntime.dll', 'DirectML.dll', 'dxcompiler.dll', 'dxil.dll']) {
    if (!names.includes(dll)) v.push(`staging 缺 ${dll}(跑 npm run build:ai-worker)`);
  }
  return v;
}

/** 检查 tauri.conf.json 接线:worker/DLL 与 AGPL 对应源码入口、许可与归属资源均须进安装包。 */
export function checkConfig(conf) {
  const v = [];
  const ext = conf?.bundle?.externalBin ?? [];
  if (!ext.includes('binaries/enhance-worker')) {
    v.push('tauri.conf.json externalBin 缺 binaries/enhance-worker');
  }
  if (!ext.includes('binaries/ai-worker')) {
    v.push('tauri.conf.json externalBin 缺 binaries/ai-worker');
  }
  if (!ext.includes('binaries/raw-worker')) {
    v.push('tauri.conf.json externalBin 缺 binaries/raw-worker');
  }
  if (!ext.includes('binaries/video-worker')) {
    v.push('tauri.conf.json externalBin 缺 binaries/video-worker');
  }
  if (!ext.includes('binaries/native-thumbnail-worker')) {
    v.push('tauri.conf.json externalBin 缺 binaries/native-thumbnail-worker');
  }
  const res = conf?.bundle?.resources ?? {};
  for (const dll of ['onnxruntime.dll', 'DirectML.dll', 'dxcompiler.dll', 'dxil.dll']) {
    const declared = Object.entries(res).some(([k, dst]) => k.endsWith(dll) && dst === dll);
    if (!declared) v.push(`tauri.conf.json resources 未声明 ${dll}`);
  }
  for (const [source, target] of REQUIRED_LEGAL_RESOURCES) {
    const declared = Object.entries(res).some(([k, dst]) => k === source && dst === target);
    if (!declared) v.push(`tauri.conf.json resources 未声明 ${target}`);
  }
  return v;
}

/** 检查法律资源源文件确实存在；只检查 conf 接线无法防止清洁 checkout 绿灯。 */
export function checkLegalSources(root = ROOT) {
  const violations = REQUIRED_LEGAL_RESOURCES
    .filter(([source]) => !existsSync(path.resolve(root, 'src-tauri', source)))
    .map(([source, target]) => `法律资源源文件缺失:${source}(目标 ${target})`);
  const legalRoot = path.join(root, 'target', 'legal');
  if (existsSync(legalRoot) && legalSourceFiles(root).length === 0) {
    violations.push('法律资源目录为空:target/legal');
  }
  return violations;
}

function legalSourceFiles(root = ROOT) {
  const legalRoot = path.join(root, 'target', 'legal');
  if (!existsSync(legalRoot)) return [];
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const absolute = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) {
        const relative = path.relative(legalRoot, absolute).replace(/\\/g, '/');
        files.push(`legal/${relative}`);
      }
    }
  };
  walk(legalRoot);
  return files.sort();
}

function archiveEntryKey(name) {
  return name.replace(/\\/g, '/').replace(/^\.\//, '').toLowerCase();
}

/** NSIS 等可见路径安装包的法律资源断言。 */
export function checkLegalEntries(entryNames, required = REQUIRED_BUNDLE_LEGAL) {
  const keys = entryNames.map(archiveEntryKey);
  return required.filter((item) =>
    !keys.some((key) => key === archiveEntryKey(item) || key.endsWith(`/${archiveEntryKey(item)}`))
  );
}

/** MSI 资源文件会被 Tauri 匿名化，按生成源文件的未压缩字节尺寸做多重集匹配。 */
export function checkLegalEntriesMs(entrySizes, stagedLegalSizes) {
  const pool = [...entrySizes];
  const missing = [];
  for (const item of stagedLegalSizes) {
    const index = pool.indexOf(item.size);
    if (index < 0) missing.push(item.name);
    else pool.splice(index, 1);
  }
  return missing;
}

// ── 安装包层(7z 拆包)────────────────────────────────────────────────────────────

function find7z() {
  if (process.platform !== 'win32') return null;
  const candidates = [
    process.env.SEVENZIP,
    'C:\\Program Files\\7-Zip\\7z.exe',
    'C:\\Program Files (x86)\\7-Zip\\7z.exe',
  ].filter(Boolean);
  for (const c of candidates) if (existsSync(c)) return c;
  const rv = spawnSync('where.exe', ['7z'], { encoding: 'utf8', shell: false });
  if (rv.status === 0 && rv.stdout.trim()) return rv.stdout.trim().split(/\r?\n/)[0];
  return null;
}

function findInstallers(bundleDir) {
  if (!existsSync(bundleDir)) return [];
  const out = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir)) {
      const p = path.join(dir, e);
      const st = statSync(p);
      if (st.isDirectory()) walk(p);
      else if (e.endsWith('.msi') || e.endsWith('-setup.exe')) out.push(p);
    }
  };
  walk(bundleDir);
  return out;
}

/** 7z -slt 列包:返回 { names, sizes }。`Path = ` 兼容旧版 `Name = `(2026-08-11 实测
 *  7-Zip 24.x/25.x 对 MSI/NSIS 均输出 Path);`Size = ` 与 `Packed Size = `/`Physical Size = `
 *  前缀不冲突,可安全按行首匹配。MSI 的 app.cab 被 7-Zip 递归展开,一次列包即含全部载荷条目。 */
function listArchiveEntries(sevenZip, archivePath) {
  const rv = spawnSync(sevenZip, ['l', '-slt', archivePath], {
    encoding: 'utf8',
    shell: false,
    windowsHide: true,
  });
  if (rv.status !== 0) {
    throw new Error(`7z 列包失败(exit=${rv.status}):${archivePath}\n${rv.stderr || rv.stdout}`);
  }
  const names = [];
  const sizes = [];
  for (const l of rv.stdout.split(/\r?\n/)) {
    if (l.startsWith('Path = ') || l.startsWith('Name = ')) {
      const n = l.slice(l.indexOf(' = ') + 3).trim();
      if (n) names.push(n);
    } else if (l.startsWith('Size = ')) {
      const s = Number(l.slice('Size = '.length).trim());
      if (Number.isFinite(s)) sizes.push(s);
    }
  }
  return { names, sizes };
}

/** staging 层四个 DLL 的字节尺寸(MSI 尺寸匹配的期望值来源);缺文件则跳过(① 层已报)。 */
function stagedPayloadSizes(binariesDir) {
  const out = {};
  if (!existsSync(binariesDir)) return out;
  for (const dll of MSI_ANONYMIZED) {
    const p = path.join(binariesDir, dll);
    if (existsSync(p)) out[dll] = statSync(p).size;
  }
  return out;
}

// ── selftest:匿名 MSI 载荷的尺寸完整性 ───────────────────────────────────────

function selftest() {
  testCase('bundle:msi-truncated-runtime-rejected', () => {
    const staged = {
      'onnxruntime.dll': 25355576,
      'directml.dll': 17986400,
      'dxcompiler.dll': 18527544,
      'dxil.dll': 1508664,
    };
    const missing = checkPayloadEntriesMs(
      ['app.cab', 'Path', 'Bin_ai_worker.exe', 'Bin_raw_worker.exe', 'Bin_video_worker.exe',
        'Bin_enhance_worker.exe', 'Bin_native_thumbnail_worker.exe',
        'PathFile_I4caaa07a', 'PathFile_I20acde8d', 'PathFile_Id320e07d', 'PathFile_I09b10b5e'],
      [50161992, 49535488, 6523904, 9287261, 25355576, 17986400, 18527544, 1500000],
      staged,
    );
    assert.deepEqual(missing, ['dxil.dll'], 'MSI 匿名载荷必须拒绝截断 DLL');
  });
}

// ── 实扫 ───────────────────────────────────────────────────────────────────────

function main() {
  selftest();
  if (process.argv.includes('--selftest')) return;
  const violations = [];

  // ① staging 层
  violations.push(...checkStaged(BINARIES_DIR).map((v) => `[staging] ${v}`));

  // 法律资源源文件层
  violations.push(...checkLegalSources().map((v) => `[legal-source] ${v}`));

  // ② conf 层
  const conf = JSON.parse(readFileSync(CONFIG_PATH, 'utf8'));
  violations.push(...checkConfig(conf).map((v) => `[conf] ${v}`));

  // ③ 安装包层
  const selected = process.argv.filter((arg) => arg.startsWith('--installer=')).map((arg) => path.resolve(arg.slice('--installer='.length)));
  const requireBundle = Boolean(process.env.CI) || process.argv.includes('--require-bundle');
  const installers = selected.length ? selected : findInstallers(BUNDLE_DIR);
  if (installers.length > 0) {
    const sevenZip = find7z();
    if (!sevenZip) {
      if (requireBundle) {
        violations.push('[bundle] 找不到 7z——发行验收必须做安装包载荷级断言');
      } else {
        console.warn('[verify-bundle-content] 未找到 7z,跳过安装包载荷断言(仅 staging/conf 层生效)');
      }
    } else {
      const stagedSizes = stagedPayloadSizes(BINARIES_DIR);
      for (const inst of installers) {
        const { names, sizes } = listArchiveEntries(sevenZip, inst);
        const isMsi = inst.toLowerCase().endsWith('.msi');
        const missing = isMsi
          ? checkPayloadEntriesMs(names, sizes, stagedSizes)
          : checkPayloadEntries(names);
        if (missing.length) {
          violations.push(
            `[bundle] ${path.relative(ROOT, inst)} 载荷缺失:${missing.join(', ')}`
          );
        } else {
          console.log(`✓ 安装包载荷完整:${path.relative(ROOT, inst)}(${names.length} 条目)`);
        }
        const legalTree = legalSourceFiles(ROOT);
        const legalSources = [
          { name: 'license', source: path.join(ROOT, 'LICENSE') },
          { name: 'notice.md', source: path.join(ROOT, 'NOTICE.md') },
          { name: 'source.md', source: path.join(ROOT, 'SOURCE.md') },
          { name: 'commercial.md', source: path.join(ROOT, 'COMMERCIAL.md') },
          { name: 'additional-permission.md', source: path.join(ROOT, 'ADDITIONAL-PERMISSION.md') },
          ...legalTree.map((name) => ({ name, source: path.join(ROOT, 'target', name) })),
        ].map(({ name, source }) => ({ name, size: statSync(source).size }));
        const legalNameMissing = isMsi ? [] : checkLegalEntries(names, legalSources.map(({ name }) => name));
        const legalSizeMissing = checkLegalEntriesMs(sizes, legalSources);
        const legalMissing = [...new Set([...legalNameMissing, ...legalSizeMissing])];
        if (legalMissing.length) {
          violations.push(
            `[bundle] ${path.relative(ROOT, inst)} 法律资源缺失:${legalMissing.join(', ')}`
          );
        } else {
          console.log(`✓ 安装包法律资源完整:${path.relative(ROOT, inst)}`);
        }
      }
    }
  } else {
    if (requireBundle) violations.push('[bundle] 缺少安装包，不能完成发行载荷校验');
    else console.warn('[verify-bundle-content] 未找到安装包(target/release/bundle)——仅 staging/conf 层生效');
  }

  if (violations.length) {
    console.error('❌ 发货闭包断言失败:');
    for (const v of violations) console.error('   - ' + v);
    process.exit(1);
  }
  console.log('✓ 发货闭包断言通过:sidecar + runtime + legal resources');
}

main();
