#!/usr/bin/env node
// exotic 协议版本跨语言同步门禁。
//
// Rust 帧协议版本与 JS 打包器版本分别定义；不一致时插件清单会被安装器拒绝。
// 检查两处常量的数值，变更协议时需同步更新。
//
// 用法:node scripts/check-exotic-protocol-sync.mjs           校验

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

function main() {
  if (process.argv.includes('--selftest')) throw new Error('协议自检入口已删除，请运行实际同步检查');
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
