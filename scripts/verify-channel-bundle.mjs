#!/usr/bin/env node
// Part7 §3.6.5⑤ 发布合规联合扫描 —— conf / capability / dist(JS bundle)三面。
// (原「第四面 Rust 依赖树断言」随空渠道 channel-msstore/steam 一起退役,P16 2026-09-15。)
//
// 背景:cargo feature 只作用 Rust 编译链,不排除非 Rust 产物;updater 配置/前端 import/
// capability 权限若意外泄进非 direct 产物,商店静态扫描会命中(上架硬阻断),dev 无 overlay
// 构建也会因基座 conf 带块而误注册 updater。
// 落地形态(2026-07-04 施工裁决,详见 Part7 §3.6.5 落地修订):
//   ① 基座 tauri.conf.json 物理无 plugins.updater(conf 合并只能加不能删,基座必须天生干净);
//      updater 块只住 tauri.direct-release.conf.json —— 本脚本双向断言(基座净 + direct 有)。
//   ② 前端现无任何 updater 包/UI(Part7-T7 有意不加),本脚本扫 dist 作防回潮守卫;
//      原型实证(2026-06-28):须扫 minify 后生产 bundle,且只扫模块说明符 / IPC 前缀 /
//      endpoint host —— 函数名(checkUpdate 等)在未 minify 死块中会残留导致误报,不扫。
//   ③ capability 现无 updater 权限,同样作防回潮断言。
//
// 用法:node scripts/verify-channel-bundle.mjs [--no-dist]
//   --no-dist:跳过 dist 扫描(未跑 vite build 的场合;CI 中禁用,必须全量)。
// 任何命中即 exit 1。脚本启动时先跑内置 selftest(正反样本喂检查器),防止检查器
// 退化成「只会 PASS 的空壳」(e2e 自洽盲区教训)。

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join, dirname, extname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const NO_DIST = process.argv.includes('--no-dist');

// ── 检查器(纯函数,输入解析后的数据,输出违例字符串数组) ──────────────────────────

/** 基座 conf:不得含 plugins.updater(updater 只住 direct 发布 overlay,基座必须天生干净)。 */
function checkBaseConf(conf) {
  const v = [];
  if (conf?.plugins?.updater !== undefined) {
    v.push('基座 tauri.conf.json 含 plugins.updater —— 必须只住 tauri.direct-release.conf.json(§3.6.5①)');
  }
  return v;
}

/** 非 direct 的 overlay(perf / acceptance):同样不得携带 updater 块。 */
function checkOverlayClean(name, conf) {
  const v = [];
  if (conf?.plugins?.updater !== undefined) {
    v.push(`${name} 含 plugins.updater —— 非 direct 发布 overlay 不得携带 updater 配置`);
  }
  return v;
}

/** direct 发布 overlay 的反向守卫:updater 配置必须完整(防止被误删致 direct 版丢更新能力)。 */
function checkDirectOverlay(conf) {
  const v = [];
  const u = conf?.plugins?.updater;
  if (!u || typeof u.pubkey !== 'string' || u.pubkey.length === 0) {
    v.push('tauri.direct-release.conf.json 缺 plugins.updater.pubkey(direct 版更新签名公钥被误删?)');
  }
  if (!Array.isArray(u?.endpoints) || u.endpoints.length === 0) {
    v.push('tauri.direct-release.conf.json 缺 plugins.updater.endpoints');
  } else if (!u.endpoints.every((e) => e.startsWith('https://'))) {
    v.push('tauri.direct-release.conf.json updater endpoints 存在非 https 项');
  }
  if (conf?.bundle?.createUpdaterArtifacts !== true) {
    v.push('tauri.direct-release.conf.json 缺 bundle.createUpdaterArtifacts=true');
  }
  return v;
}

/** capability 清单:任何 updater: 前缀权限即违例(entry 可为字符串或 {identifier} 对象)。 */
function checkCapability(name, cap) {
  const v = [];
  for (const p of cap?.permissions ?? []) {
    const id = typeof p === 'string' ? p : p?.identifier ?? '';
    if (id === 'updater' || id.startsWith('updater:')) {
      v.push(`${name} 含 updater 权限「${id}」—— Part7-T7 裁决不开放 webview updater ACL`);
    }
  }
  return v;
}

/** dist 文本产物:模块说明符 / IPC 命令前缀 / endpoint host 任一命中即违例。 */
function checkDistText(relPath, text, hosts) {
  const v = [];
  const patterns = ['@tauri-apps/plugin-updater', 'plugin:updater|', ...hosts];
  for (const p of patterns) {
    if (p && text.includes(p)) v.push(`dist/${relPath} 命中「${p}」`);
  }
  return v;
}

// ── selftest:正反样本喂检查器,反样本漏检 / 正样本误报均视为脚本自身损坏 ────────────

function selftest() {
  const dirty = { plugins: { updater: { pubkey: 'x', endpoints: ['https://u.example/{{target}}'] } } };
  const clean = { bundle: { active: true } };
  const fails = [];
  const expect = (cond, msg) => { if (!cond) fails.push(msg); };

  expect(checkBaseConf(dirty).length === 1, '基座检查器漏检 updater 块');
  expect(checkBaseConf(clean).length === 0, '基座检查器对干净 conf 误报');
  expect(checkOverlayClean('t.json', dirty).length === 1, 'overlay 检查器漏检');
  expect(checkDirectOverlay(dirty).length === 1, 'direct 守卫应报缺 createUpdaterArtifacts');
  expect(
    checkDirectOverlay({ plugins: { updater: { pubkey: 'x', endpoints: ['https://u.example/a'] } }, bundle: { createUpdaterArtifacts: true } }).length === 0,
    'direct 守卫对完整配置误报'
  );
  expect(checkDirectOverlay({ plugins: { updater: { pubkey: 'x', endpoints: ['http://u.example/a'] } }, bundle: { createUpdaterArtifacts: true } }).length === 1, 'direct 守卫漏检非 https endpoint');
  expect(checkCapability('c.json', { permissions: ['core:default', 'updater:default'] }).length === 1, 'capability 检查器漏检字符串权限');
  expect(checkCapability('c.json', { permissions: [{ identifier: 'updater:allow-check' }] }).length === 1, 'capability 检查器漏检对象权限');
  expect(checkCapability('c.json', { permissions: ['core:default'] }).length === 0, 'capability 检查器误报');
  expect(checkDistText('a.js', 'import("@tauri-apps/plugin-updater")', []).length === 1, 'dist 检查器漏检模块说明符');
  expect(checkDistText('a.js', 'invoke("plugin:updater|check")', []).length === 1, 'dist 检查器漏检 IPC 前缀');
  expect(checkDistText('a.js', 'fetch("https://updates.example.com/x")', ['updates.example.com']).length === 1, 'dist 检查器漏检 endpoint host');
  // 原型教训回归:函数名不构成命中(用户代码无害死调用不应误报)。
  expect(checkDistText('a.js', 'function checkUpdate(){}', ['updates.example.com']).length === 0, 'dist 检查器误报函数名');

  if (fails.length) {
    console.error('❌ verify-channel-bundle selftest 失败(扫描器自身已损坏,结果不可信):');
    for (const f of fails) console.error('   - ' + f);
    process.exit(2);
  }
}

// ── 实扫 ─────────────────────────────────────────────────────────────────────

function readJson(p) {
  return JSON.parse(readFileSync(p, 'utf8'));
}

function main() {
  selftest();
  const violations = [];
  let scanned = 0;

  // ① conf 面:基座 + 全部 overlay。
  violations.push(...checkBaseConf(readJson(join(ROOT, 'src-tauri/tauri.conf.json'))));
  scanned++;
  const overlays = readdirSync(join(ROOT, 'src-tauri')).filter(
    (f) => /^tauri\..+\.conf\.json$/.test(f)
  );
  let directHosts = [];
  for (const f of overlays) {
    const conf = readJson(join(ROOT, 'src-tauri', f));
    scanned++;
    if (f === 'tauri.direct-release.conf.json') {
      violations.push(...checkDirectOverlay(conf));
      directHosts = (conf?.plugins?.updater?.endpoints ?? [])
        .map((e) => { try { return new URL(e.replace(/\{\{[^}]+\}\}/g, 'x')).hostname; } catch { return null; } })
        .filter(Boolean);
    } else {
      violations.push(...checkOverlayClean(f, conf));
    }
  }

  // ③ capability 面。
  const capDir = join(ROOT, 'src-tauri/capabilities');
  for (const f of readdirSync(capDir, { recursive: true })) {
    const p = join(capDir, String(f));
    if (statSync(p).isFile() && extname(p) === '.json') {
      violations.push(...checkCapability(String(f), readJson(p)));
      scanned++;
    }
  }

  // ② dist 面(minify 后生产 bundle;文本类扩展名全扫)。
  if (!NO_DIST) {
    const dist = join(ROOT, 'dist');
    if (!existsSync(dist)) {
      console.error('❌ dist/ 不存在 —— 先 npm run build(minify 后扫描是原型实证的硬前提),或传 --no-dist 显式跳过');
      process.exit(1);
    }
    const TEXT_EXT = new Set(['.js', '.mjs', '.css', '.html', '.json', '.svg', '.txt', '.webmanifest', '.map']);
    for (const f of readdirSync(dist, { recursive: true })) {
      const p = join(dist, String(f));
      if (statSync(p).isFile() && TEXT_EXT.has(extname(p).toLowerCase())) {
        violations.push(...checkDistText(relative(dist, p).replace(/\\/g, '/'), readFileSync(p, 'utf8'), directHosts));
        scanned++;
      }
    }
  }

  if (violations.length) {
    console.error(`❌ 渠道合规扫描:${violations.length} 项违例`);
    for (const v of violations) console.error('   - ' + v);
    process.exit(1);
  }
  console.log(`✅ 渠道合规扫描通过(selftest 14 断言 + 实扫 ${scanned} 文件${NO_DIST ? ',dist 已跳过' : ''})`);
}

main();
