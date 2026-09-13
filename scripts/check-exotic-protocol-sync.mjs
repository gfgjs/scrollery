#!/usr/bin/env node
// exotic 协议版本跨语言同步门禁(2026-07-13)
//
// 起因:exotic 帧协议版本号在两处各存一份——
//   Rust : crates/exotic-protocol/src/frame.rs  `pub const PROTOCOL_VERSION: u16 = N;`(host 真相)
//   JS   : scripts/lib/exotic-signing.mjs        `export const PROTOCOL_VERSION = N;`(打包器烙进包清单)
// 二者此前只靠一句注释「请保持同步」约束。2026-07-13 事故:host 升到 3、JS 漏改停在 2,打包器把 2
// 烙进已发布 registry 的 psd 插件清单,被 installer 启动前复核判 protocol_mismatch 永久拒装
// (用户「无法安装 psd-worker」的根因)。本门禁把「注释级同步」升成「门禁级同步」:两值不等即红,
// 升 host 时强制同步改 JS。
//
// 用法:node scripts/check-exotic-protocol-sync.mjs           校验
//       node scripts/check-exotic-protocol-sync.mjs --selftest  正反自检(防门禁退化成只会 PASS 的空壳)

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const RUST_FILE = 'crates/exotic-protocol/src/frame.rs';
const JS_FILE = 'scripts/lib/exotic-signing.mjs';
// Rust: `pub const PROTOCOL_VERSION: u16 = 3;`(frame.rs 里其余 {PROTOCOL_VERSION} 插值不含 `= N;`,不误匹配)。
const RUST_RE = /pub\s+const\s+PROTOCOL_VERSION\s*:\s*u16\s*=\s*(\d+)\s*;/;
// JS: `export const PROTOCOL_VERSION = 3;`
const JS_RE = /export\s+const\s+PROTOCOL_VERSION\s*=\s*(\d+)\s*;/;

/** 从文本抽取协议版本号(十进制);找不到返回 null。 */
function extractVersion(text, re) {
  const m = re.exec(text);
  return m ? Number(m[1]) : null;
}

/** 正反自检:证明抽取器/比较器都能给出正确方向,避免门禁退化成只会 PASS。 */
function selftest() {
  const fails = [];
  // 夹具用拼接构造(不写整句字面量),避免任何扫描门(含本门未来自扫)误读常量(experience §12 教训)。
  const V = 'PROTOCOL_VERSION';
  const rustDecl = (n) => 'pub const ' + V + ': u16 = ' + n + ';';
  const jsDecl = (n) => 'export const ' + V + ' = ' + n + ';';
  if (extractVersion('a\n' + rustDecl(3) + '\nb', RUST_RE) !== 3) fails.push('rust 抽取应=3');
  if (extractVersion('a\n' + jsDecl(3) + '\nb', JS_RE) !== 3) fails.push('js 抽取应=3');
  if (extractVersion('a\n' + jsDecl(2) + '\nb', JS_RE) !== 2) fails.push('js 抽取应=2(负样本)');
  if (extractVersion('no const here', RUST_RE) !== null) fails.push('缺失应=null');
  if (fails.length) {
    console.error('✗ check-exotic-protocol-sync selftest 失败:\n  ' + fails.join('\n  '));
    process.exit(1);
  }
  console.log('✓ check-exotic-protocol-sync selftest 通过(抽取/负样本/缺失四向)');
}

function main() {
  if (process.argv.includes('--selftest')) return selftest();
  const rustV = extractVersion(readFileSync(path.join(repo, RUST_FILE), 'utf8'), RUST_RE);
  const jsV = extractVersion(readFileSync(path.join(repo, JS_FILE), 'utf8'), JS_RE);
  const errs = [];
  if (rustV === null) errs.push(`未能在 ${RUST_FILE} 找到 PROTOCOL_VERSION 常量(门禁正则或源已漂移)`);
  if (jsV === null) errs.push(`未能在 ${JS_FILE} 找到 PROTOCOL_VERSION 常量(门禁正则或源已漂移)`);
  if (rustV !== null && jsV !== null && rustV !== jsV) {
    errs.push(
      `协议版本跨语言失配:${RUST_FILE}=${rustV} != ${JS_FILE}=${jsV}。升 host 时必须同步改 JS,` +
        `否则打包器会把旧版本号烙进插件清单,被 installer 启动前复核判 protocol_mismatch 拒装。`
    );
  }
  if (errs.length) {
    console.error(`✗ exotic 协议版本同步门禁不过(${errs.length} 处):`);
    for (const e of errs) console.error(`  ${e}`);
    process.exit(1);
  }
  console.log(`✓ exotic 协议版本同步门禁通过(Rust=${rustV} === JS=${jsV})`);
}

main();
