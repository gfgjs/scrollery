// 内测(internal testing)registry 生成器——Part8 D1 签发端的内测形态(2026-07-05)。
//
// 与 exotic-dev-registry.mjs 的差异(其余打包/签名逻辑经 lib/exotic-signing.mjs 完全同源):
//   - worker 用 **release** 构建(target/release/psd-worker.exe),交真人测试的是生产性能形态;
//   - package_url 是**真实 HTTPS**(默认 raw.githubusercontent.com 内测仓),不是 file://,
//     因此 Release 安装包无需任何 dev 旁路即可走完「刷新→下载→验签→安装」生产链路;
//   - 密钥对/keyset 独立于 dev(.internal-signing/,已 gitignore):key_id 带 internal 字样,
//     与生产信任根(exotic-keyset-prod.json,ceremony 产物经 PICASA_EXOTIC_KEYSET_FILE 注入)彻底隔离;
//   - keyset 是「占位生产集 + 内测键」**超集**:注入构建后 builtin_keyset_parses 等
//     测试断言(release-2026-01/license-2026-01 存在)依然成立。
//
// 用法:node scripts/exotic-internal-registry.mjs
// 前置:cargo build --release -p psd-worker
// 产出(全部落 <repo>/.internal-signing/):
//   internal-release.pem / internal-license.pem   Ed25519 私钥(仅本机,勿外传/勿入 CI)
//   internal-keyset.json                          编译期注入用公钥集(PICASA_EXOTIC_KEYSET_FILE)
//   registry/{index.json,index.sig,exotic-image-psd.zip}   上传到内测发行源的三件套
//   seq                                           单调序号(防回滚基线,与 dev 独立)
//
// 发行源基址可经 PICASA_INTERNAL_REGISTRY_BASE 覆盖(默认 = 内测公开仓 raw URL)。

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
const out = path.join(repo, '.internal-signing');
const regOut = path.join(out, 'registry');
fs.mkdirSync(regOut, { recursive: true });

const PLUGIN_ID = process.env.EXOTIC_PLUGIN_ID || 'exotic-image-psd';
const TARGET = process.env.EXOTIC_TARGET || 'x86_64-pc-windows-msvc';
const MEDIA_KIND = process.env.EXOTIC_MEDIA_KIND || 'image';
const FORMATS = (process.env.EXOTIC_FORMATS || 'psd').split(',').map((s) => s.trim()).filter(Boolean);
const CAPABILITIES = (process.env.EXOTIC_CAPABILITIES || 'thumbnail').split(',').map((s) => s.trim()).filter(Boolean);
const SKU = process.env.EXOTIC_SKU || 'psd-engine-2026';
const MIN_HOST_VERSION = process.env.EXOTIC_MIN_HOST_VERSION || '0.1.0';
const COMPLIANCE_REVIEW_ID = process.env.EXOTIC_COMPLIANCE_REVIEW_ID || 'internal-2026-07';
const WORKER_NAME = process.env.EXOTIC_WORKER_NAME || 'psd-worker.exe';
const WORKER_CRATE = process.env.EXOTIC_WORKER_CRATE || 'psd-worker';
const WORKER_PATH = process.env.EXOTIC_WORKER_PATH || path.join(repo, 'target', 'release', WORKER_NAME);
const RELEASE_KEY_ID = process.env.EXOTIC_RELEASE_KEY_ID || 'release-internal-2026-07';
const LICENSE_KEY_ID = process.env.EXOTIC_LICENSE_KEY_ID || 'license-internal-2026-07';
// 内测发行源(公开仓 raw 直链;更新 registry = 向该仓 push 新的三件套)。
const REG_BASE =
  process.env.PICASA_INTERNAL_REGISTRY_BASE ||
  'https://raw.githubusercontent.com/gfgjs/scrollery-registry/main/exotic/v1';

// ── 1. 内测密钥对(存在即复用;与 dev/生产信任根均隔离) ───────────────────────
const releaseKey = ensureKey(path.join(out, 'internal-release.pem'));
const licenseKey = ensureKey(path.join(out, 'internal-license.pem'));

// keyset = 仓内占位生产集 + 内测键(超集,保注入态下既有测试断言成立)。
const placeholder = JSON.parse(
  fs.readFileSync(
    path.join(repo, 'crates', 'scrollery-exotic-trust', 'resources', 'exotic-keyset.json'),
    'utf8'
  )
);
const keyset = {
  schema: 1,
  _note:
    '内测信任根(占位生产集 + 内测键超集)。经 PICASA_EXOTIC_KEYSET_FILE 编译期注入内测安装包;' +
    '私钥仅签发机本机。与生产 keyset(exotic-keyset-prod.json,ceremony 产物经 PICASA_EXOTIC_KEYSET_FILE 注入)无关。',
  keys: [
    ...placeholder.keys,
    keysetEntry(RELEASE_KEY_ID, 'release', releaseKey),
    keysetEntry(LICENSE_KEY_ID, 'license', licenseKey),
  ],
};
fs.writeFileSync(path.join(out, 'internal-keyset.json'), JSON.stringify(keyset, null, 2));

// ── 2. worker 载荷(release 构建) ─────────────────────────────────────────────
const workerExe = WORKER_PATH;
if (!fs.existsSync(workerExe)) {
  console.error(`缺 ${workerExe}\n先构建:cargo build --release -p ${WORKER_CRATE} 或设置 EXOTIC_WORKER_PATH`);
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

// ── 4. 插件包 ─────────────────────────────────────────────────────────────────
const { zipBytes } = buildPluginZip({
  pluginId: PLUGIN_ID,
  version,
  seq,
  target: TARGET,
  keyId: RELEASE_KEY_ID,
  releaseKey,
  workerBytes,
  workerName: WORKER_NAME,
  formats: FORMATS,
  capabilities: CAPABILITIES,
  minHostVersion: MIN_HOST_VERSION,
  complianceReviewId: COMPLIANCE_REVIEW_ID,
});
const zipPath = path.join(regOut, `${PLUGIN_ID}.zip`);
fs.writeFileSync(zipPath, zipBytes);

// ── 5. 签名 index(package_url = 真实 HTTPS 直链) ────────────────────────────
const now = Math.floor(Date.now() / 1000);
const { indexBytes, sigBytes } = signIndex({
  keyId: RELEASE_KEY_ID,
  releaseKey,
  seq,
  generatedAt: now,
  // 60 天有效期:过期后商店只展示不允新装(§6.1),重跑本工具 + push 即续期。
  expiresAt: now + 60 * 86400,
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
      package_url: `${REG_BASE}/${PLUGIN_ID}.zip`,
      package_size: zipBytes.length,
      package_sha256: sha256hex(zipBytes),
    },
  ],
});
fs.writeFileSync(path.join(regOut, 'index.json'), indexBytes);
fs.writeFileSync(path.join(regOut, 'index.sig'), sigBytes);

console.log(`内测 registry 已生成(seq=${seq}, version=${version}, 有效期 60 天)
  keyset   : ${path.join(out, 'internal-keyset.json')}
  三件套   : ${regOut}\\{index.json,index.sig,${PLUGIN_ID}.zip}(zip ${(zipBytes.length / 1048576).toFixed(1)} MB)
  发行基址 : ${REG_BASE}

下一步:
  1) 把三件套推到发行源仓的 exotic/v1/ 目录(路径须与发行基址一致);
  2) 产物自检:cargo test -p scrollery --lib internal_registry_artifacts -- --ignored
  3) 内测安装包构建(编译期注入,见 scripts/build-internal-installer.ps1):
     $env:PICASA_EXOTIC_KEYSET_FILE = '${path.join(out, 'internal-keyset.json')}'
     $env:PICASA_REGISTRY_BASE_DEFAULT = '${REG_BASE}'
     npm run tauri build
  4) 给测试者签发激活 token:node scripts/exotic-issue-license.mjs`);
