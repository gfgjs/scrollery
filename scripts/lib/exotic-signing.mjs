// exotic 插件发行链共享签名/打包原语(Part8 D1 签发端的本地原型层)。
//
// 消费者:exotic-dev-registry.mjs(开发期 file:// 源)、exotic-internal-registry.mjs
// (内测 HTTPS 源)、exotic-issue-license.mjs(License token 签发)。
// 契约对端:src-tauri exotic::{registry,install} 与 crates/scrollery-exotic-trust——
// 本库产物必须能通过 installer.rs 的 #[ignore] 核验测试(生产同一套校验链,勿双轨漂移)。

import crypto from 'node:crypto';
import fs from 'node:fs';

/**
 * 与 crates/exotic-protocol/src/frame.rs 的 `PROTOCOL_VERSION` **手动**同步(worker 帧协议版本)。
 * ⚠ 无自动门禁:frame.rs 升版本时必须同步改这里,否则本工具生成/发布的插件包清单会烙上旧版本号,
 *   被 installer.rs 的启动前复核判为 protocol_mismatch 拒装(2026-07-13 事故:host=3 而此处滞留=2,
 *   已发布 registry 的 psd 包因此永久拒装;根因是升 host 时漏改本常量)。
 */
export const PROTOCOL_VERSION = 3;

/** Ed25519 私钥:pem 存在即复用(保证 keyset 稳定),否则生成并落盘 pkcs8 pem。 */
export function ensureKey(pemPath) {
  if (fs.existsSync(pemPath)) {
    return crypto.createPrivateKey(fs.readFileSync(pemPath));
  }
  const { privateKey } = crypto.generateKeyPairSync('ed25519');
  fs.writeFileSync(pemPath, privateKey.export({ type: 'pkcs8', format: 'pem' }));
  return privateKey;
}

/** spki DER 尾 32 字节 = Ed25519 裸公钥(keyset 的 public_key_b64 用标准 base64)。 */
export function rawPubB64(priv) {
  return crypto
    .createPublicKey(priv)
    .export({ type: 'spki', format: 'der' })
    .subarray(-32)
    .toString('base64');
}

/** 构造 keyset 的单键条目(schema 1;not_before=0 恒有效,轮换语义此层不涉)。 */
export function keysetEntry(keyId, purpose, priv) {
  return {
    key_id: keyId,
    purpose,
    public_key_b64: rawPubB64(priv),
    status: 'active',
    not_before: 0,
    not_after: null,
  };
}

export function sha256hex(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

/** Ed25519 对原始字节签名(index/package-manifest/license payload 统一用此形态)。 */
export function signBytes(privateKey, bytes) {
  return crypto.sign(null, bytes, privateKey);
}

/** base64url 无填充(License token 两段;与 exotic-trust b64url_encode 对齐)。 */
export function b64url(buf) {
  return Buffer.from(buf).toString('base64url');
}

function crc32(buf) {
  let c,
    crc = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c = (crc ^ buf[i]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/** 存储式 zip(method=0;高压缩比检查天然不触发,布局与生产解包白名单一致;时间戳恒 0 保确定性)。 */
export function storeZip(entries) {
  const locals = [],
    centrals = [];
  let offset = 0;
  for (const [name, data] of entries) {
    const n = Buffer.from(name, 'utf8');
    const crc = crc32(data);
    const head = Buffer.alloc(30);
    head.writeUInt32LE(0x04034b50, 0);
    head.writeUInt16LE(20, 4); // version needed
    head.writeUInt16LE(0, 6); // flags
    head.writeUInt16LE(0, 8); // method=store
    head.writeUInt32LE(0, 10); // dos time/date(固定 0,确定性产物)
    head.writeUInt32LE(crc, 14);
    head.writeUInt32LE(data.length, 18);
    head.writeUInt32LE(data.length, 22);
    head.writeUInt16LE(n.length, 26);
    head.writeUInt16LE(0, 28);
    locals.push(head, n, data);

    const cen = Buffer.alloc(46);
    cen.writeUInt32LE(0x02014b50, 0);
    cen.writeUInt16LE(20, 4); // made by
    cen.writeUInt16LE(20, 6); // needed
    cen.writeUInt16LE(0, 8);
    cen.writeUInt16LE(0, 10);
    cen.writeUInt32LE(0, 12);
    cen.writeUInt32LE(crc, 16);
    cen.writeUInt32LE(data.length, 20);
    cen.writeUInt32LE(data.length, 24);
    cen.writeUInt16LE(n.length, 28);
    // extra/comment/disk/attrs 全 0
    cen.writeUInt32LE(offset, 42);
    centrals.push(cen, n);
    offset += 30 + n.length + data.length;
  }
  const centralStart = offset;
  const centralBuf = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(centralStart, 16);
  return Buffer.concat([...locals, centralBuf, eocd]);
}

/**
 * 构造插件包 zip(清单对 + 签名 + worker 载荷)。
 * plugin-manifest.json:插件自声明 formats/capabilities,安装时须为 Catalog 子集。
 * package-manifest.json:安装真相(清单即白名单;kind=worker 项即运行期 exe 坐标)。
 * @returns {{ zipBytes: Buffer }}
 */
export function buildPluginZip({
  pluginId,
  version,
  seq,
  target,
  keyId,
  releaseKey,
  workerBytes,
  workerName,
  formats,
  capabilities,
  minHostVersion,
  complianceReviewId,
}) {
  const pluginManifest = Buffer.from(
    JSON.stringify({ plugin_id: pluginId, formats, capabilities }, null, 2)
  );
  const packageManifest = Buffer.from(
    JSON.stringify(
      {
        schema: 1,
        key_id: keyId,
        plugin_id: pluginId,
        version,
        package_sequence: seq,
        target,
        min_host_version: minHostVersion,
        protocol_version: PROTOCOL_VERSION,
        compliance_review_id: complianceReviewId,
        files: [
          {
            path: workerName,
            size: workerBytes.length,
            sha256: sha256hex(workerBytes),
            kind: 'worker',
            executable: true,
          },
          {
            path: 'plugin-manifest.json',
            size: pluginManifest.length,
            sha256: sha256hex(pluginManifest),
            kind: 'resource',
            executable: false,
          },
        ],
      },
      null,
      2
    )
  );
  const manifestSig = signBytes(releaseKey, packageManifest); // Ed25519 对原始字节
  const zipBytes = storeZip([
    ['package-manifest.json', packageManifest],
    ['package-manifest.sig', manifestSig],
    ['plugin-manifest.json', pluginManifest],
    [workerName, workerBytes],
  ]);
  return { zipBytes };
}

/** 签名 Registry index:返回 { indexBytes, sigBytes }(签名覆盖 index 原始字节)。 */
export function signIndex({ keyId, releaseKey, seq, generatedAt, expiresAt, plugins }) {
  const indexBytes = Buffer.from(
    JSON.stringify(
      {
        schema: 1,
        key_id: keyId,
        sequence: seq,
        generated_at: generatedAt,
        expires_at: expiresAt,
        plugins,
      },
      null,
      2
    )
  );
  return { indexBytes, sigBytes: signBytes(releaseKey, indexBytes) };
}

/**
 * 签发 License token(exotic-trust §5.2):`b64url(payload_json) + "." + b64url(sig)`,
 * 签名覆盖 payload 原始字节,Verifier 不重序列化。expiresAt=null 即永久授权(v3 首发主路径)。
 */
export function signLicenseToken({
  licenseKey,
  keyId,
  licenseId,
  pluginId,
  sku,
  issuedAt,
  notBefore,
  expiresAt,
}) {
  const payload = Buffer.from(
    JSON.stringify({
      version: 1,
      key_id: keyId,
      license_id: licenseId,
      plugin_id: pluginId,
      sku,
      subject_hash: null,
      issued_at: issuedAt,
      not_before: notBefore,
      expires_at: expiresAt,
    })
  );
  return `${b64url(payload)}.${b64url(signBytes(licenseKey, payload))}`;
}

/** 单调序号(registry_sequence/package_sequence 防回滚基线):读取 +1 并回写。 */
export function nextSeq(seqPath) {
  const seq = (fs.existsSync(seqPath) ? parseInt(fs.readFileSync(seqPath, 'utf8'), 10) : 0) + 1;
  fs.writeFileSync(seqPath, String(seq));
  return seq;
}
