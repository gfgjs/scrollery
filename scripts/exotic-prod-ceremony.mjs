#!/usr/bin/env node
// 生产信任根 key ceremony 工具(Part8 D1 / ③b B1 真钥,2026-07-06)。
//
// 🔴 设计为**自包含单文件**:离线签发机上只需本文件 + Node ≥18,经 USB 拷入即可运行,
// 不依赖仓库其他文件。为此刻意内联复制了 lib/exotic-signing.mjs 的三个微助手
// (rawPubB64/keysetEntry/b64url)——启动 selftest 以 spki 重建往返 + 签验往返锁行为,
// 防两份实现漂移;keyset schema 若升版,两处须同步改。
//
// 用法(签发机上):
//   node exotic-prod-ceremony.mjs init [--out <dir>] [--tag 2026-07]
//     生成 release + license 两对 Ed25519 钥 → prod-release.pem / prod-license.pem(私钥,
//     永不出机)+ exotic-keyset-prod.json(公钥集,唯一出机产物)+ ceremony-record.txt
//     (指纹留痕,抄纸)。已有 pem 时拒绝覆盖(轮换须换 --out 或换 --tag 走新 ceremony)。
//   node exotic-prod-ceremony.mjs verify [--out <dir>]
//     用私钥重算公钥与 keyset 逐把比对 + 内存签验往返(ceremony 后/年检自查)。
//   node exotic-prod-ceremony.mjs verify-token <token> [--keyset <json>]
//     只用 keyset 公钥验一枚 license token(不触私钥,可在开发机上跑——验证「keyset
//     运回开发机后与签发机私钥确实配对」的闭环)。
//
// 产物去向:exotic-keyset-prod.json → 替换 crates/scrollery-exotic-trust/resources/exotic-keyset.json
// 内置占位集,或经 PICASA_EXOTIC_KEYSET_FILE 注入构建(通道见 crates/scrollery-exotic-trust/build.rs)。
// 红线:私钥永不入仓、不进 CI env、不出现在测试 fixture(仅 ceremony 产物目录 + 冷备)。

import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// ---- 内联微助手(与 lib/exotic-signing.mjs 同构,自包含所需) ----------------------

/** spki DER 尾 32 字节 = Ed25519 裸公钥(keyset 的 public_key_b64 用标准 base64)。 */
function rawPubB64(priv) {
  return crypto
    .createPublicKey(priv)
    .export({ type: 'spki', format: 'der' })
    .subarray(-32)
    .toString('base64');
}

/** 由裸 32 字节公钥重建 spki KeyObject(verify-token 用;12 字节 Ed25519 spki 头)。 */
function pubFromRaw(rawB64) {
  const raw = Buffer.from(rawB64, 'base64');
  if (raw.length !== 32) throw new Error(`公钥非 32 字节: ${raw.length}`);
  const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw]);
  return crypto.createPublicKey({ key: spki, format: 'der', type: 'spki' });
}

/** keyset 单键条目(schema 1;not_before 取 ceremony 时刻,轮换窗口语义见 ceremony 文档)。 */
function keysetEntry(keyId, purpose, priv, notBefore) {
  return {
    key_id: keyId,
    purpose,
    public_key_b64: rawPubB64(priv),
    status: 'active',
    not_before: notBefore,
    not_after: null,
  };
}

function sha256hex(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

// ---- selftest(启动即跑;防内联助手与主实现漂移/退化) ------------------------------

function selftest() {
  const { privateKey } = crypto.generateKeyPairSync('ed25519');
  // spki 重建往返:raw → KeyObject → raw 逐位一致
  const raw = rawPubB64(privateKey);
  if (rawPubB64OfPub(pubFromRaw(raw)) !== raw) {
    console.error('✗ selftest 失败: spki 重建往返不一致');
    process.exit(2);
  }
  // 签验往返(经重建公钥验签,即 verify-token 同路径)
  const msg = Buffer.from('ceremony-selftest');
  const sig = crypto.sign(null, msg, privateKey);
  if (!crypto.verify(null, msg, pubFromRaw(raw), sig)) {
    console.error('✗ selftest 失败: 签验往返不过');
    process.exit(2);
  }
  // 条目形状哨兵(与 Rust VerifyingKeyset 解析契约的最小面)
  const e = keysetEntry('k', 'license', privateKey, 0);
  for (const f of ['key_id', 'purpose', 'public_key_b64', 'status', 'not_before', 'not_after']) {
    if (!(f in e)) {
      console.error(`✗ selftest 失败: keyset 条目缺字段 ${f}`);
      process.exit(2);
    }
  }
  console.log('✓ selftest 通过(spki 往返 + 签验往返 + 条目形状)');
}
function rawPubB64OfPub(pub) {
  return pub.export({ type: 'spki', format: 'der' }).subarray(-32).toString('base64');
}

// ---- 参数与子命令 ----------------------------------------------------------------

const args = process.argv.slice(2);
const cmd = args[0];
function argOf(flag, fallback) {
  const i = args.indexOf(flag);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
}
const outDir = path.resolve(argOf('--out', './prod-signing'));

selftest();

if (cmd === 'init') {
  const tag = argOf('--tag', new Date().toISOString().slice(0, 7)); // 默认 YYYY-MM
  const relPem = path.join(outDir, 'prod-release.pem');
  const licPem = path.join(outDir, 'prod-license.pem');
  // 拒绝覆盖:已有私钥被覆盖=已售 license/已发插件包全部作废,轮换必须显式走新目录/新 tag。
  for (const p of [relPem, licPem]) {
    if (fs.existsSync(p)) {
      console.error(`✗ 拒绝覆盖已有私钥: ${p}\n  轮换请换 --out 目录另行 ceremony,旧钥按文档 §4 处置。`);
      process.exit(2);
    }
  }
  fs.mkdirSync(outDir, { recursive: true });
  const rel = crypto.generateKeyPairSync('ed25519').privateKey;
  const lic = crypto.generateKeyPairSync('ed25519').privateKey;
  // pem 落盘;POSIX 上收紧到 0600(Windows 依赖目录 ACL/BitLocker,文档 §1 有要求)。
  const writeOpts = process.platform === 'win32' ? {} : { mode: 0o600 };
  fs.writeFileSync(relPem, rel.export({ type: 'pkcs8', format: 'pem' }), writeOpts);
  fs.writeFileSync(licPem, lic.export({ type: 'pkcs8', format: 'pem' }), writeOpts);

  const notBefore = Math.floor(Date.now() / 1000);
  const keyset = {
    schema: 1,
    _note: `生产信任根公钥集(ceremony ${tag},由 exotic-prod-ceremony.mjs 生成)。私钥仅签发机本机+冷备,永不入仓/CI/fixture。替换 crates/scrollery-exotic-trust/resources/exotic-keyset.json 占位,或经 PICASA_EXOTIC_KEYSET_FILE 注入构建。`,
    keys: [
      keysetEntry(`release-prod-${tag}`, 'release', rel, notBefore),
      keysetEntry(`license-prod-${tag}`, 'license', lic, notBefore),
    ],
  };
  const keysetPath = path.join(outDir, 'exotic-keyset-prod.json');
  fs.writeFileSync(keysetPath, JSON.stringify(keyset, null, 2));

  // ceremony 留痕:指纹抄纸即可人工核对「运出的 keyset = 签发机生成的那份」。
  const record = [
    `ceremony 时间: ${new Date().toISOString()}`,
    `tag: ${tag}`,
    `机器: ${os.hostname()} (${os.platform()} ${os.release()}, node ${process.version})`,
    `release key_id: release-prod-${tag}`,
    `  公钥 sha256: ${sha256hex(Buffer.from(rawPubB64(rel), 'base64'))}`,
    `license key_id: license-prod-${tag}`,
    `  公钥 sha256: ${sha256hex(Buffer.from(rawPubB64(lic), 'base64'))}`,
    `keyset sha256: ${sha256hex(fs.readFileSync(keysetPath))}`,
    '',
    '出机清单(仅此两件): exotic-keyset-prod.json / ceremony-record.txt',
    '私钥冷备: 按 ceremony 文档 §4(第二介质 + 异地),完成后在本行手写勾选 [ ]',
  ].join('\n');
  fs.writeFileSync(path.join(outDir, 'ceremony-record.txt'), record + '\n');

  console.log(`✓ ceremony 完成(tag=${tag})\n${record}\n\n🔴 私钥(两个 .pem)永不出机;出机只带 keyset json + record。`);
} else if (cmd === 'verify') {
  const keyset = JSON.parse(fs.readFileSync(path.join(outDir, 'exotic-keyset-prod.json'), 'utf8'));
  let ok = true;
  for (const [pem, purpose] of [
    ['prod-release.pem', 'release'],
    ['prod-license.pem', 'license'],
  ]) {
    const priv = crypto.createPrivateKey(fs.readFileSync(path.join(outDir, pem)));
    const entry = keyset.keys.find((k) => k.purpose === purpose);
    const match = entry && entry.public_key_b64 === rawPubB64(priv);
    // 签验往返:私钥签 → keyset 公钥验(与生产验签同路径)
    const msg = Buffer.from(`verify-${purpose}`);
    const roundtrip = entry && crypto.verify(null, msg, pubFromRaw(entry.public_key_b64), crypto.sign(null, msg, priv));
    console.log(`${match && roundtrip ? '✓' : '✗'} ${purpose}: 公钥比对 ${match ? '一致' : '不一致'} / 签验往返 ${roundtrip ? '通过' : '失败'} (${entry ? entry.key_id : '条目缺失'})`);
    ok = ok && match && roundtrip;
  }
  process.exit(ok ? 0 : 1);
} else if (cmd === 'verify-token') {
  const token = args[1];
  if (!token || !token.includes('.')) {
    console.error('✗ 用法: verify-token <payload.sig 形态 token> [--keyset <json>]');
    process.exit(2);
  }
  const keysetPath = path.resolve(argOf('--keyset', path.join(outDir, 'exotic-keyset-prod.json')));
  const keyset = JSON.parse(fs.readFileSync(keysetPath, 'utf8'));
  const [p64, s64] = token.split('.');
  const payloadBytes = Buffer.from(p64, 'base64url');
  const payload = JSON.parse(payloadBytes.toString('utf8'));
  const entry = keyset.keys.find((k) => k.key_id === payload.key_id && k.purpose === 'license');
  if (!entry) {
    console.error(`✗ keyset 中无 key_id=${payload.key_id} 的 license 键`);
    process.exit(1);
  }
  const good = crypto.verify(null, payloadBytes, pubFromRaw(entry.public_key_b64), Buffer.from(s64, 'base64url'));
  console.log(
    `${good ? '✓ 验签通过' : '✗ 验签失败'}: key_id=${payload.key_id} plugin=${payload.plugin_id} sku=${payload.sku} license_id=${payload.license_id}`
  );
  process.exit(good ? 0 : 1);
} else {
  console.error('用法: node exotic-prod-ceremony.mjs <init|verify|verify-token> [--out <dir>] [--tag YYYY-MM] [--keyset <json>]');
  process.exit(2);
}
