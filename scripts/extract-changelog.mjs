#!/usr/bin/env node
// 提取 docs/CHANGELOG.md 中指定版本小节，供公开版发布说明使用。
// 用法:node scripts/extract-changelog.mjs <version> [changelog-path]
//   <version> 不带 v 前缀(如 0.1.0);正文写 stdout。
// 缺小节 / 小节为空 → exit 1(发布失败):公开仓历史是 squash 同步提交,发布说明
// 取自 CHANGELOG 的版本小节；提取失败时中止发布。

import { readFileSync } from 'node:fs';

const [version, file = 'docs/CHANGELOG.md'] = process.argv.slice(2);
if (!version) {
  console.error('用法:node scripts/extract-changelog.mjs <version> [changelog-path]');
  process.exit(2);
}

const lines = readFileSync(file, 'utf8').replace(/\r\n/g, '\n').split('\n');
// 小节头形如 "## [0.1.0] - 2026-07-04";只锚版本号部分,日期后缀自由。
const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
if (start < 0) {
  console.error(`❌ ${file} 无「## [${version}]」小节——打 tag 前须把 Unreleased 条目移入该小节(发布硬门)`);
  process.exit(1);
}
let end = lines.length;
for (let i = start + 1; i < lines.length; i++) {
  if (lines[i].startsWith('## ')) {
    end = i;
    break;
  }
}
const body = lines.slice(start + 1, end).join('\n').trim();
if (!body) {
  console.error(`❌ 「## [${version}]」小节为空——发布说明不可为空`);
  process.exit(1);
}
process.stdout.write(body + '\n');
