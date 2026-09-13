#!/usr/bin/env node
// 仓库路径卫生门禁(2026-07-11 加固批 A-5)
//
// 起因:src-tauri/.cargo/config.toml 里硬编码的绝对路径被 R2-7 改名施工机械替换成
// 死路径,cargo 「就近优先」使其劫持 tauri dev → worker ort 装载无限阻塞(事故全链
// 见 docs/experience.md §5)。本门禁把这一类雷挡在入仓时刻:
//
//   R1 嵌套 cargo config:`.cargo/config(.toml)` 只允许存在于仓库根——子目录副本
//      就是「就近覆盖」劫持面,且极易被遗忘。
//   R2 跟踪的配置文件(toml/json/yml/yaml)不得含 Windows 盘符绝对路径——改名/换机
//      的机械替换只对绝对路径有杀伤力,相对路径(如根 config 的 relative=true)天然免疫。
//
// 豁免通道:行内含 `path-gate-allow` 标记(须附注原因);`docs/**` 整体豁免
// (文档谈路径是本分)。范围刻意只收配置文件不扫源码:源码里的路径多为测试夹具/注释,
// 误伤率高;事故类别是「被工具消费的配置」。
import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const files = execSync('git ls-files', { encoding: 'utf8' })
  .split('\n')
  .filter(Boolean)
  .map((f) => f.replace(/\\/g, '/'));

const errors = [];

// ── R1:嵌套 cargo config ──────────────────────────────────────────────────
for (const f of files) {
  if (/(^|\/)\.cargo\/config(\.toml)?$/.test(f) && f !== '.cargo/config.toml') {
    errors.push(`R1 嵌套 cargo config(就近覆盖劫持面,唯一合法位置是仓库根):${f}`);
  }
}

// ── R2:配置文件盘符绝对路径 ──────────────────────────────────────────────
const CONFIG_EXT = /\.(toml|json|ya?ml)$/i;
const SKIP = [/^docs\//, /^package-lock\.json$/];
// 盘符路径 = 字母+冒号+斜杠,且前一字符不是字母数字(排除 URL 的 `s://`、`e://` 等)。
const DRIVE_PATH = /(?<![A-Za-z0-9])[A-Za-z]:[\\/]/;

let scanned = 0;
for (const f of files) {
  if (!CONFIG_EXT.test(f)) continue;
  if (SKIP.some((re) => re.test(f))) continue;
  let text;
  try {
    text = readFileSync(f, 'utf8');
  } catch {
    continue; // 索引里有但工作区缺失(稀疏检出等):不判
  }
  scanned += 1;
  const commentable = /\.(toml|ya?ml)$/i.test(f); // JSON 无注释语法,整文件都是值
  text.split('\n').forEach((line, i) => {
    // 整行注释豁免:事故类别是「被工具消费的配置值」,注释不被机器消费(值行的
    // 行尾注释不豁免——该行仍含值)。
    if (commentable && line.trimStart().startsWith('#')) return;
    if (DRIVE_PATH.test(line) && !line.includes('path-gate-allow')) {
      errors.push(`R2 配置文件含盘符绝对路径:${f}:${i + 1} → ${line.trim().slice(0, 120)}`);
    }
  });
}

if (errors.length) {
  console.error(`✗ 路径卫生门禁不过(${errors.length} 处):`);
  for (const e of errors) console.error(`  ${e}`);
  console.error('  修法:改相对路径(cargo [env] 用 relative=true);确属合法请行内加 `path-gate-allow` 并注明原因。');
  process.exit(1);
}
console.log(`✓ 路径卫生门禁通过(R1 cargo config 唯一;R2 扫描 ${scanned} 个配置文件无盘符绝对路径)`);
