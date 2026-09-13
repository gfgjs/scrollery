// vendored 编译产物的钉定与校验——唯一实现。
// 这些产物随前端 bundle 分发、不在任何 lockfile 里,版本只能由产物自身与上游来源坐实:
//   Graphviz 2.40.1(EPL-1.0) 只以 Viz.js 2.1.2 的 Emscripten 单文件产物形式存在;
//   Lute 1.7.6(MulanPSL-2.0) 只以 Go 编译产物 lute.min.js 形式存在(Vditor 3.11.3 vendored)。
// 三处调用共用本模块,避免各自维护一份校验而漂移:
//   scripts/vendor-license-sources.mjs(写 third-party/license-sources.json 前自校验)
//   scripts/generate-notice.mjs(NOTICE/SBOM 生成与 --check)
//   scripts/prepare-legal-resources.mjs(打包 legal/ 前)
// 目的不是判定许可兼容性,而是保证 NOTICE/legal 里写的版本与字节确有所指。

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

export const GRAPHVIZ_VERSION = '2.40.1';
export const VIZJS_VERSION = '2.1.2';
export const LUTE_VERSION = '1.7.6';

// 每个组件一个钉定版本字段(写入 manifest)与一份产物规格。versionMarker 由版本常量拼出,
// 因此改版本必然连带改产物期望,两者不可能各自漂移。
export const VENDORED_ARTIFACTS = [
  {
    id: 'graphviz-full-render',
    component: 'graphviz',
    versionField: 'graphvizVersion',
    version: GRAPHVIZ_VERSION,
    path: 'public/vditor/dist/js/graphviz/full.render.js',
    size: 1979941,
    sha256: '9e49a783fb9ad41d0ba3ca6e6c424e712eaa5e25562eb2c6cff102e6d586b9c4',
    versionMarker: `Viz.js ${VIZJS_VERSION} (Graphviz ${GRAPHVIZ_VERSION}, Expat 2.2.5, Emscripten 1.37.36)`,
  },
  {
    id: 'graphviz-viz',
    component: 'graphviz',
    versionField: 'graphvizVersion',
    version: GRAPHVIZ_VERSION,
    path: 'public/vditor/dist/js/graphviz/viz.js',
    size: 11468,
    sha256: 'f111f22be005ceaf625f06c5a36ba7aa27703dbc0753914559edd1c24715b6e5',
    versionMarker: `Viz.js ${VIZJS_VERSION} (Graphviz ${GRAPHVIZ_VERSION}`,
  },
  {
    id: 'lute-min',
    component: 'lute',
    versionField: 'luteVersion',
    version: LUTE_VERSION,
    path: 'public/vditor/dist/js/lute/lute.min.js',
    size: 3739251,
    sha256: '8e7a18344afd870f195248f5de189b9e130b53b991470cb171f788d8908bafc9',
    versionMarker: `new $String("${LUTE_VERSION}")`,
  },
];

// 写进 manifest 的纯数据形态(去掉校验期的期望字段)。
export function vendoredArtifactManifest() {
  return VENDORED_ARTIFACTS.map(({ id, component, path: artifactPath, size, sha256, versionMarker }) => ({
    id,
    component,
    path: artifactPath,
    size,
    sha256,
    versionMarker,
  }));
}

// 与 VENDORED_ARTIFACTS 对应的版本字段(供调用方做 manifest 级交叉校验)。
export const VENDORED_VERSION_FIELDS = {
  graphviz: GRAPHVIZ_VERSION,
  lute: LUTE_VERSION,
};

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/**
 * 校验 manifest 中的 vendored 产物清单与磁盘字节。
 * 失败即抛出:缺条、重复、未知路径、字节/hash/尺寸不符、版本标记缺失、钉定版本字段不符。
 * 空数组或整段缺失一律视为失败——跳过校验等于放弃归依据。
 */
export function verifyVendoredArtifacts(manifest, repo) {
  const entries = manifest?.vendoredArtifacts;
  if (!Array.isArray(entries) || entries.length === 0) {
    throw new Error('钉定清单缺少 vendoredArtifacts(缺失或空数组均不接受);请先运行 npm run vendor:licenses');
  }
  for (const spec of VENDORED_ARTIFACTS) {
    const matches = entries.filter((entry) => entry?.path === spec.path);
    if (matches.length !== 1) {
      throw new Error(
        `vendored 产物清单必须恰好一条 ${spec.path},实际 ${matches.length} 条` +
          '(缺条/重复都意味着归属版本失去依据,不接受静默跳过)',
      );
    }
  }
  for (const entry of entries) {
    const spec = VENDORED_ARTIFACTS.find((candidate) => candidate.path === entry.path);
    if (!spec) throw new Error(`vendored 产物清单含未登记路径: ${entry.path}`);
    if (entry.id !== spec.id || entry.component !== spec.component) {
      throw new Error(`vendored 产物清单条目与规格不符: ${entry.path}`);
    }
    if (!Number.isInteger(entry.size) || entry.size <= 0 || !/^[0-9a-f]{64}$/.test(entry.sha256 ?? '')) {
      throw new Error(`vendored 产物清单条目缺少有效 size/sha256: ${entry.path}`);
    }
    if (typeof entry.versionMarker !== 'string' || !entry.versionMarker.includes(spec.version)) {
      throw new Error(`vendored 产物清单条目的版本标记未含钉定版本 ${spec.version}: ${entry.path}`);
    }
    const absolute = path.join(repo, ...entry.path.split('/'));
    if (!existsSync(absolute)) throw new Error(`vendored 产物缺失: ${entry.path}`);
    const bytes = readFileSync(absolute);
    const actualSize = statSync(absolute).size;
    const actualDigest = sha256(bytes);
    if (actualSize !== entry.size || actualDigest !== entry.sha256) {
      throw new Error(
        `vendored 产物与钉定值不符: ${entry.path}\n` +
          `期望 ${entry.size} B / ${entry.sha256},实际 ${actualSize} B / ${actualDigest}\n` +
          '若为有意的上游升级,请同步更新版本常量、期望值与已固定的上游法律材料。',
      );
    }
    if (!bytes.toString('utf8').includes(entry.versionMarker)) {
      throw new Error(
        `vendored 产物缺少版本标记: ${entry.path}\n期望包含 ${entry.versionMarker}`,
      );
    }
  }
  for (const [component, expected] of Object.entries(VENDORED_VERSION_FIELDS)) {
    const field = VENDORED_ARTIFACTS.find((spec) => spec.component === component).versionField;
    if (manifest[field] !== expected) {
      throw new Error(`钉定清单 ${field} 应为 ${expected},实际 ${manifest[field] ?? 'missing'}`);
    }
  }
  return entries;
}
