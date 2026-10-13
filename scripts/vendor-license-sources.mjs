#!/usr/bin/env node
// 仅由维护者显式运行：把固定版本的上游法律材料抓取到仓库。
// 构建脚本不调用网络；后续构建只校验这些已纳入版本控制的文件 hash。
//
// 材料分三类：
//   ① 随包分发的运行时/模型侧文本（onnxruntime、ffmpeg）；
//   ② 嵌入前端产物、不在任何 lockfile 的编译件所属上游（graphviz=vendored Viz.js 内的
//      Graphviz 2.40.1 EPL-1.0、lute=Lute 1.7.6 MulanPSL-2.0；版本由产物内声明坐实，
//      钉定值与校验实现只有一处：scripts/lib/vendored-artifacts.mjs）；
//   ③ 包内无文本时兜底用的 SPDX 正文（spdx/*）。

import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  GRAPHVIZ_VERSION,
  LUTE_VERSION,
  VIZJS_VERSION,
  vendoredArtifactManifest,
  verifyVendoredArtifacts,
} from './lib/vendored-artifacts.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const vendorRoot = path.join(repo, 'third-party', 'licenses');
const manifestPath = path.join(repo, 'third-party', 'license-sources.json');
const ORT_VERSION = '1.26.0';
const FFMPEG_SOURCE_VERSION = '2aefd64d48';
const BTBN_BUILD_REVISION = '8c736b2';
const SPDX_VERSION = 'v3.27.0';

const SPDX_IDS = [
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'BSL-1.0',
  'EPL-1.0',
  'MIT',
  'MPL-2.0',
  'MulanPSL-2.0',
  'Unlicense',
  'Zlib',
];

const SOURCES = [
  {
    id: 'onnxruntime-license',
    kind: 'onnxruntime',
    target: 'onnxruntime/LICENSE',
    url: `https://raw.githubusercontent.com/microsoft/onnxruntime/v${ORT_VERSION}/LICENSE`,
    sha256: '2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c',
  },
  {
    id: 'onnxruntime-third-party-notices',
    kind: 'onnxruntime',
    target: 'onnxruntime/ThirdPartyNotices.txt',
    url: `https://raw.githubusercontent.com/microsoft/onnxruntime/v${ORT_VERSION}/ThirdPartyNotices.txt`,
    sha256: '0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2',
  },
  {
    id: 'ffmpeg-license',
    kind: 'ffmpeg',
    target: 'ffmpeg/LICENSE.md',
    url: `https://raw.githubusercontent.com/FFmpeg/FFmpeg/${FFMPEG_SOURCE_VERSION}/LICENSE.md`,
  },
  {
    id: 'ffmpeg-copying-lgplv2.1',
    kind: 'ffmpeg',
    target: 'ffmpeg/COPYING.LGPLv2.1',
    url: `https://raw.githubusercontent.com/FFmpeg/FFmpeg/${FFMPEG_SOURCE_VERSION}/COPYING.LGPLv2.1`,
  },
  {
    id: 'btbn-license',
    kind: 'ffmpeg',
    target: 'ffmpeg/BtbN-LICENSE',
    url: `https://raw.githubusercontent.com/BtbN/FFmpeg-Builds/${BTBN_BUILD_REVISION}/LICENSE`,
  },
  // Graphviz 2.40.1 的许可证正文取上游同标签的 LICENSE（EPL-1.0 正文，与 SPDX 的 EPL-1.0 同为
  // 该协议，但以 Graphviz 自己发布的文件为准；EPL-1.0 §7 允许分发者选择按该协议新版本分发，
  // §3 对目标码形式分发列有归属与源码获得说明要求）。本项目第一方为 AGPL-3.0-only，与 EPL 的
  // 组合结论属待裁定事项，不在本清单内作判断——本文件只固定版本、正文与来源。
  {
    id: 'graphviz-license',
    kind: 'graphviz',
    target: `graphviz/Graphviz-${GRAPHVIZ_VERSION}-LICENSE.txt`,
    url: `https://gitlab.com/graphviz/graphviz/-/raw/${GRAPHVIZ_VERSION}/LICENSE`,
  },
  // Viz.js 包装层的 MIT 正文（编译产物头部已声明 MIT，此处固定上游同标签正文）。
  {
    id: 'vizjs-license',
    kind: 'graphviz',
    target: `graphviz/Viz.js-${VIZJS_VERSION}-LICENSE.txt`,
    url: `https://raw.githubusercontent.com/mdaines/viz.js/v${VIZJS_VERSION}/LICENSE`,
  },
  // Lute 1.7.6（Vditor 3.11.3 内 vendored 的编译产物）的 MulanPSL-2.0 正文。
  {
    id: 'lute-license',
    kind: 'lute',
    target: `lute/Lute-${LUTE_VERSION}-LICENSE.txt`,
    url: `https://raw.githubusercontent.com/88250/lute/v${LUTE_VERSION}/LICENSE`,
  },
  ...SPDX_IDS.map((id) => ({
    id,
    kind: 'spdx',
    licenseId: id,
    target: `spdx/${id}.txt`,
    url: `https://raw.githubusercontent.com/spdx/license-list-data/${SPDX_VERSION}/text/${id}.txt`,
  })),
];

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function writeAtomic(target, bytes) {
  mkdirSync(path.dirname(target), { recursive: true });
  const temporary = `${target}.tmp`;
  writeFileSync(temporary, bytes);
  renameSync(temporary, target);
}

async function obtain(source, refresh, existingById) {
  const target = path.join(vendorRoot, source.target);
  const expectedSha = source.sha256 ?? existingById.get(source.id)?.sha256;
  if (existsSync(target) && !refresh) {
    const local = readFileSync(target);
    const got = sha256(local);
    if (!expectedSha) {
      throw new Error(`已存在的法律材料没有固定 hash: ${path.relative(repo, target)}\n请使用 --refresh 重新固定并同步审查版本。`);
    }
    if (got !== expectedSha) {
      throw new Error(`已存在的法律材料 hash 不符: ${path.relative(repo, target)}\n期望 ${expectedSha}\n实际 ${got}\n如需更新，请显式使用 --refresh 并同步审查版本。`);
    }
    source.sha256 = got;
    return 'reused';
  }

  const response = await fetch(source.url);
  if (!response.ok) throw new Error(`上游法律材料下载失败(${response.status}): ${source.url}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const got = sha256(bytes);
  if (source.sha256 && got !== source.sha256) {
    throw new Error(`上游法律材料 hash 不符: ${source.url}\n期望 ${source.sha256}\n实际 ${got}`);
  }
  if (!refresh && expectedSha && got !== expectedSha) {
    throw new Error(`固定版本法律材料已漂移: ${source.url}\n期望 ${expectedSha}\n实际 ${got}\n如需更新，请显式使用 --refresh 并同步审查版本。`);
  }
  source.sha256 = got;
  writeAtomic(target, bytes);
  return 'fetched';
}

async function main() {
  const refresh = process.argv.includes('--refresh');
  // vendored 产物的钉定校验只有一处实现(scripts/lib/vendored-artifacts.mjs)：
  // 清单条目/版本字段/磁盘字节三者须一致，否则本次固定出的清单连同 NOTICE 都不可信。
  const manifest = {
    version: 1,
    ortVersion: ORT_VERSION,
    ffmpegSourceVersion: FFMPEG_SOURCE_VERSION,
    btbnBuildRevision: BTBN_BUILD_REVISION,
    graphvizVersion: GRAPHVIZ_VERSION,
    vizjsVersion: VIZJS_VERSION,
    luteVersion: LUTE_VERSION,
    spdxVersion: SPDX_VERSION,
    vendoredArtifacts: vendoredArtifactManifest(),
    sources: SOURCES,
  };
  const artifacts = verifyVendoredArtifacts(manifest, repo);
  const existing = existsSync(manifestPath)
    ? JSON.parse(readFileSync(manifestPath, 'utf8'))
    : { sources: [] };
  const existingById = new Map((existing.sources ?? []).map((source) => [source.id, source]));
  let fetched = 0;
  let reused = 0;
  for (const source of SOURCES) {
    const state = await obtain(source, refresh, existingById);
    if (state === 'fetched') fetched += 1;
    else reused += 1;
  }

  writeAtomic(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(
    `✓ 法律材料已固定(third-party/licenses; fetched ${fetched}; reused ${reused}; ` +
      `vendored 产物 ${artifacts.length} 项已校验; refresh ${refresh})`,
  );
}

main().catch((error) => {
  console.error(`[vendor-license-sources] ${error.message}`);
  process.exit(1);
});
