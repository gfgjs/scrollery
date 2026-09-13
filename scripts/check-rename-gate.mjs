#!/usr/bin/env node
// Part8-D10 改名门禁:改名施工(Scrollery,见 docs/decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md)
// 第 7 步把 rename-gate.json 翻 active 后,任何把品牌旧词根带回 tracked 文件的提交在 CI 被拦。
//
// 设计三层:
//   1. git grep -I -i 扫全部 tracked 文本文件(含大小写变体 Picasa/picasanext/…);
//   2. 路径前缀豁免——历史快照文档(docs/**,2026-07-11 前名 plan-docs/**)按施工计划 §2-D 裁决不改名;
//   3. token 前缀豁免——PICASA_* env 常量族按 §2-E「缓改」裁决暂留(大小写敏感,
//      只豁免真 env 拼法,小写 picasa_xxx 不放行)。
// 休眠期(active=false)只跑 selftest 后 skip:改名前全仓合法满是旧名,门禁不应生效。
// 分工:本门只管「新名时代旧名残留回流」,与公开投影门禁(copy.bara.sky 内部文件过滤 +
// oss-gate 公开构建/密钥扫描)互不替代。
// 零依赖 + 启动即 selftest(仓例 verify-channel-bundle):防扫描器退化成只会 PASS 的空壳。
// 用法:node scripts/check-rename-gate.mjs [--force-active](本地彩排:无视配置强制按 active 跑)
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const CONFIG_PATH = join(ROOT, 'scripts', 'rename-gate.json');
const REPORT_CAP = 20; // 命中报告上限,防改名前误开门时刷屏

/**
 * 找出一行中未被豁免的命中词。
 * 命中向两侧扩展 [A-Za-z0-9_] 成完整词;词以任一豁免前缀开头(大小写敏感)即放行——
 * 这样 PICASA_EXOTIC_KEYSET_FILE 放行,而 "picasa-next"(连字符断词,词=“picasa”)照拦。
 */
function offendingMatches(line, forbidden, allowTokenPrefixes) {
  const hits = [];
  const re = new RegExp(forbidden, 'gi');
  let m;
  while ((m = re.exec(line)) !== null) {
    let s = m.index;
    let e = m.index + m[0].length;
    while (s > 0 && /[A-Za-z0-9_]/.test(line[s - 1])) s--;
    while (e < line.length && /[A-Za-z0-9_]/.test(line[e])) e++;
    const word = line.slice(s, e);
    if (!allowTokenPrefixes.some((p) => word.startsWith(p))) hits.push(word);
  }
  return hits;
}

function pathAllowed(path, allowPathPrefixes) {
  return allowPathPrefixes.some((p) => path.startsWith(p));
}

function selftest() {
  const cases = [
    // [行, 期望拦截?]
    ['const KEYRING_SERVICE: &str = "picasa-next";', true],
    ['PICASA_EXOTIC_KEYSET_FILE=/keys/prod.json', false], // env 族豁免
    ['  "identifier": "com.picasanext.app",', true],
    ['productName "Picasa Next"', true],
    ['scrollery everywhere, nothing to see', false],
    ['picasa_exotic_keyset_file', true], // 小写拼法不豁免(豁免大小写敏感)
  ];
  for (const [line, bad] of cases) {
    const got = offendingMatches(line, 'picasa', ['PICASA_']).length > 0;
    if (got !== bad) {
      console.error(`✗ selftest 失败: ${JSON.stringify(line)} 期望拦截=${bad} 实得=${got}`);
      process.exit(2);
    }
  }
  const allow = ['docs/', 'scripts/rename-gate.json'];
  if (!pathAllowed('docs/todo.md', allow) || pathAllowed('src-tauri/tauri.conf.json', allow)) {
    console.error('✗ selftest 失败: 路径豁免逻辑');
    process.exit(2);
  }
  console.log('✓ selftest 通过(6 词例 + 路径豁免)');
}

function loadConfig() {
  const raw = JSON.parse(readFileSync(CONFIG_PATH, 'utf8'));
  for (const [key, type] of [
    ['active', 'boolean'],
    ['forbidden', 'string'],
  ]) {
    if (typeof raw[key] !== type) {
      console.error(`✗ rename-gate.json 缺字段或类型错: ${key} 应为 ${type}`);
      process.exit(2);
    }
  }
  if (!Array.isArray(raw.allowPathPrefixes) || !Array.isArray(raw.allowTokenPrefixes)) {
    console.error('✗ rename-gate.json allowPathPrefixes/allowTokenPrefixes 应为数组');
    process.exit(2);
  }
  return raw;
}

selftest();
const cfg = loadConfig();
const forceActive = process.argv.includes('--force-active');

if (!cfg.active && !forceActive) {
  console.log('D10 改名门禁休眠中(rename-gate.json active=false)——改名施工第 7 步翻 true 生效。');
  process.exit(0);
}

// git grep 退出码:0=有命中,1=零命中,其余=错误。-I 跳过二进制,只扫 tracked。
// 🔴 core.quotepath=false:CJK 文件名默认被引号+八进制转义(如 "plan-docs/\345..."),
//    路径前缀豁免会因带引号前缀而失配(2026-07-06 改名彩排实抓)——关掉转义输出裸 UTF-8。
const res = spawnSync('git', ['-c', 'core.quotepath=false', 'grep', '-I', '-i', '-n', '-e', cfg.forbidden, '--', '.'], {
  cwd: ROOT,
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
});
if (res.status !== 0 && res.status !== 1) {
  console.error(`✗ git grep 执行失败(exit ${res.status}): ${res.stderr}`);
  process.exit(2);
}

const offending = [];
for (const line of (res.stdout || '').split('\n')) {
  if (!line) continue;
  const first = line.indexOf(':');
  const second = line.indexOf(':', first + 1);
  if (first < 0 || second < 0) continue;
  const path = line.slice(0, first);
  const content = line.slice(second + 1);
  if (pathAllowed(path, cfg.allowPathPrefixes)) continue;
  const words = offendingMatches(content, cfg.forbidden, cfg.allowTokenPrefixes);
  if (words.length > 0) offending.push({ loc: line.slice(0, second), words: [...new Set(words)] });
}

if (offending.length === 0) {
  console.log(`✓ 改名门禁通过:tracked 文件零「${cfg.forbidden}」残留(豁免面之外)。`);
  process.exit(0);
}
console.error(`✗ 改名门禁拦截:${offending.length} 处品牌旧词根残留(前 ${REPORT_CAP} 处):`);
for (const o of offending.slice(0, REPORT_CAP)) {
  console.error(`  ${o.loc}  [${o.words.join(', ')}]`);
}
if (offending.length > REPORT_CAP) console.error(`  … 另 ${offending.length - REPORT_CAP} 处从略`);
console.error('修法:改用新名 Scrollery;历史文档/env 缓改族若属合法豁免,更新 scripts/rename-gate.json。');
process.exit(1);
