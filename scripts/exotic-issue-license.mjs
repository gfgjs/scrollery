// License token 签发器(Part8 D1 签发端;内测与生产签发机双形态,2026-07-06 硬化)。
//
// 产出一枚 `payload.sig` 形态的激活 token(exotic-trust §5.2),用户在应用
// 「插件商店 → 激活」里粘贴即可解锁付费插件(统一信任根在 crates/scrollery-exotic-trust;
// 安装包的信任根须含对应公钥:内测经 PICASA_EXOTIC_KEYSET_FILE 注入 internal-keyset.json,
// 生产为 ceremony 产出的 exotic-keyset-prod.json)。
//
// 用法:
//   node scripts/exotic-issue-license.mjs                      # PSD 插件,永久授权(内测)
//   node scripts/exotic-issue-license.mjs --days 30            # 30 天后过期
//   node scripts/exotic-issue-license.mjs --plugin X --sku Y   # 其他插件/SKU
//   node scripts/exotic-issue-license.mjs --key <pem> --key-id <id>   # 生产签发机形态(D1)
//
// 密钥两形态:
//   内测(默认):.internal-signing/internal-license.pem(缺失则自动生成——但那样生成的
//     新键必须重跑 exotic-internal-registry.mjs 让 keyset 收录后重新构建安装包才可验过,
//     故常规顺序是先跑 registry 生成器再签发)。
//   生产(--key):显式指向 ceremony 产物(exotic-prod-ceremony.mjs init 生成的
//     prod-license.pem)。文件缺失即 fail-fast,**绝不自动生钥**——签发机上静默换钥
//     等于把已售 license 全部作废;--key 与 --key-id 必须成对给出。

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureKey, signLicenseToken } from './lib/exotic-signing.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// 极简参数解析(仅 --key value 形态;脚本级工具不引依赖)。
const args = process.argv.slice(2);
function argOf(flag, fallback) {
  const i = args.indexOf(flag);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
}
const pluginId = argOf('--plugin', 'exotic-image-psd');
const sku = argOf('--sku', 'psd-engine-2026');
const days = argOf('--days', null);
const keyPath = argOf('--key', null);
const keyIdArg = argOf('--key-id', null);

let licenseKey;
let keyId;
let mode;
if (keyPath || keyIdArg) {
  // 生产形态:显式钥 + 显式 key_id,残缺或缺文件一律 fail-fast(防静默生钥/串 key_id)。
  if (!keyPath || !keyIdArg) {
    console.error('✗ --key 与 --key-id 必须成对给出(生产签发机形态)。');
    process.exit(2);
  }
  if (!fs.existsSync(keyPath)) {
    console.error(`✗ 私钥文件不存在: ${keyPath}(生产形态绝不自动生钥,请核对 ceremony 产物路径)`);
    process.exit(2);
  }
  licenseKey = crypto.createPrivateKey(fs.readFileSync(keyPath));
  keyId = keyIdArg;
  mode = '生产';
} else {
  const out = path.join(repo, '.internal-signing');
  fs.mkdirSync(out, { recursive: true });
  const licensePem = path.join(out, 'internal-license.pem');
  const existedBefore = fs.existsSync(licensePem);
  licenseKey = ensureKey(licensePem);
  if (!existedBefore) {
    console.warn(
      '⚠ 新生成了 internal-license.pem——须重跑 exotic-internal-registry.mjs 更新 keyset 并重建安装包,token 才能验过。'
    );
  }
  keyId = 'license-internal-2026-07';
  mode = '内测';
}

const now = Math.floor(Date.now() / 1000);
const token = signLicenseToken({
  licenseKey,
  keyId,
  licenseId: `lic-${mode === '生产' ? 'prod' : 'internal'}-${crypto.randomUUID()}`,
  pluginId,
  sku,
  issuedAt: now,
  // 留 1 小时时钟偏差余量:测试机时钟略慢也不至于 NotYetValid。
  notBefore: now - 3600,
  // null = 永久授权(v3 首发主路径);--days N 则 N 天后过期。
  expiresAt: days ? now + parseInt(days, 10) * 86400 : null,
});

console.log(`${mode} License token 已签发(plugin=${pluginId}, sku=${sku}, key_id=${keyId}, ${days ? `${days} 天有效` : '永久'})
在应用「插件商店 → 该插件 → 激活」中粘贴下面整行:

${token}`);
