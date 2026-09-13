// 插件商店 dev registry 生成器(开发期专用;Part8 D1 签发端的本地原型)。
// 签名/打包原语已抽至 lib/exotic-signing.mjs(2026-07-05,与内测生成器/license 签发共用)。
//
// 产出(全部落 <repo>/.dev-registry/,已 gitignore,cargo clean 不波及):
//   dev-release.pem / dev-license.pem  Ed25519 私钥(仅本机 dev 用,勿外传)
//   dev-keyset.json                    公钥集(host 经 PICASA_EXOTIC_DEV_KEYSET 注入,debug-only)
//   exotic-image-psd.zip               插件包(package-manifest.json+sig+plugin-manifest+psd-worker.exe)
//   index.json / index.sig             签名 Registry index(package_url=file:///…,debug-only 放行)
//   seq                                单调序号(registry_sequence/package_sequence 防回滚基线)
//
// 用法:node scripts/exotic-dev-registry.mjs
// 前置:cargo build -p psd-worker(需 target/debug/psd-worker.exe)
//
// 安全边界:dev 密钥/file:// 传输仅在 debug 构建 + 两个环境变量显式开启时生效
// (PICASA_EXOTIC_DEV_FILE_URLS=1 + PICASA_EXOTIC_DEV_KEYSET=<dev-keyset.json>);
// Release 构建两条旁路整体不编入(SEC-02 姿态)。验签/sha256/zip 白名单等校验
// 与生产完全同一套代码,本工具产物须能通过 installer.rs 的 #[ignore] 核验测试。

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  buildPluginZip,
  ensureKey,
  keysetEntry,
  nextSeq,
  sha256hex,
  signIndex,
} from './lib/exotic-signing.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(repo, '.dev-registry');
fs.mkdirSync(out, { recursive: true });

const PLUGIN_ID = process.env.EXOTIC_PLUGIN_ID || 'exotic-image-psd';
const TARGET = process.env.EXOTIC_TARGET || 'x86_64-pc-windows-msvc';
const MEDIA_KIND = process.env.EXOTIC_MEDIA_KIND || 'image';
const FORMATS = (process.env.EXOTIC_FORMATS || 'psd').split(',').map((s) => s.trim()).filter(Boolean);
const CAPABILITIES = (process.env.EXOTIC_CAPABILITIES || 'thumbnail').split(',').map((s) => s.trim()).filter(Boolean);
const SKU = process.env.EXOTIC_SKU || 'psd-engine-2026';
const MIN_HOST_VERSION = process.env.EXOTIC_MIN_HOST_VERSION || '0.1.0';
const COMPLIANCE_REVIEW_ID = process.env.EXOTIC_COMPLIANCE_REVIEW_ID || 'dev-local';
const WORKER_NAME = process.env.EXOTIC_WORKER_NAME || 'psd-worker.exe';
const WORKER_CRATE = process.env.EXOTIC_WORKER_CRATE || 'psd-worker';
const WORKER_PATH = process.env.EXOTIC_WORKER_PATH || path.join(repo, 'target', 'debug', WORKER_NAME);

// ── 1. 密钥对(存在即复用,保证 keyset 稳定) ─────────────────────────────────
const releaseKey = ensureKey(path.join(out, 'dev-release.pem'));
const licenseKey = ensureKey(path.join(out, 'dev-license.pem'));

const keyset = {
  schema: 1,
  keys: [
    keysetEntry('dev-release-local', 'release', releaseKey),
    keysetEntry('dev-license-local', 'license', licenseKey),
  ],
};
fs.writeFileSync(path.join(out, 'dev-keyset.json'), JSON.stringify(keyset, null, 2));

// ── 2. worker 载荷 ────────────────────────────────────────────────────────────
const workerExe = WORKER_PATH;
if (!fs.existsSync(workerExe)) {
  console.error(`缺 ${workerExe}\n先构建:cargo build -p ${WORKER_CRATE} 或设置 EXOTIC_WORKER_PATH`);
  process.exit(1);
}
const workerBytes = fs.readFileSync(workerExe);

// ── 3. 序号与版本 ─────────────────────────────────────────────────────────────
// sequence/package_sequence = 生成时刻 epoch 秒:天然单调、无状态。教训(2026-07-05
// 内测首刷实证):设备防回滚基线(appdata exotic/registry/index.seq)在 dev 与内测
// 安装包间共享(同 identifier 同 appdata),小计数器序号会互相砸盘;且计数器状态
// 文件 gitignored、跨机重置从 1 重启,会令已装设备永久 rollback_rejected。
const seq = Math.floor(Date.now() / 1000);
// 展示版本号仍用本地小计数器(仅可读性;权威排序是 package_sequence)。
const version = `1.0.${nextSeq(path.join(out, 'seq'))}`;

// ── 4. 插件包(清单对 + 签名 + 存储式 zip;布局与生产解包白名单一致) ──────────
const { zipBytes } = buildPluginZip({
  pluginId: PLUGIN_ID,
  version,
  seq,
  target: TARGET,
  keyId: 'dev-release-local',
  releaseKey,
  workerBytes,
  workerName: WORKER_NAME,
  formats: FORMATS,
  capabilities: CAPABILITIES,
  minHostVersion: MIN_HOST_VERSION,
  complianceReviewId: COMPLIANCE_REVIEW_ID,
});
const zipPath = path.join(out, `${PLUGIN_ID}.zip`);
fs.writeFileSync(zipPath, zipBytes);

// ── 5. 签名 index ─────────────────────────────────────────────────────────────
const now = Math.floor(Date.now() / 1000);
const fileUrl = 'file:///' + zipPath.replace(/\\/g, '/');
const { indexBytes, sigBytes } = signIndex({
  keyId: 'dev-release-local',
  releaseKey,
  seq,
  generatedAt: now,
  expiresAt: now + 30 * 86400,
  plugins: [
    {
      plugin_id: PLUGIN_ID,
      version,
      package_sequence: seq,
      media_kind: MEDIA_KIND,
      formats: FORMATS,
      capabilities: CAPABILITIES,
      sku: SKU,
      min_host_version: MIN_HOST_VERSION,
      target: TARGET,
      package_url: fileUrl,
      package_size: zipBytes.length,
      package_sha256: sha256hex(zipBytes),
    },
  ],
});
fs.writeFileSync(path.join(out, 'index.json'), indexBytes);
fs.writeFileSync(path.join(out, 'index.sig'), sigBytes);

const regBase = 'file:///' + out.replace(/\\/g, '/');
console.log(`dev registry 已生成(seq=${seq}, version=${version})
  keyset : ${path.join(out, 'dev-keyset.json')}
  index  : ${path.join(out, 'index.json')}
  zip    : ${zipPath}(${(zipBytes.length / 1048576).toFixed(1)} MB)

在启动 tauri dev 的同一 PowerShell 里设置:
  $env:PICASA_EXOTIC_DEV_FILE_URLS = '1'
  $env:PICASA_EXOTIC_DEV_KEYSET = '${path.join(out, 'dev-keyset.json')}'
  $env:PICASA_REGISTRY_BASE = '${regBase}'
需要授权 gate 也放行时(激活/运行付费插件),dev 构建加 feature:
  npm run tauri dev -- -- --features exotic-dev-fixtures
产物自检(生产同套校验器):
  $env:PICASA_EXOTIC_DEV_FILE_URLS='1'; cargo test -p scrollery --lib dev_registry_artifacts -- --ignored`);
