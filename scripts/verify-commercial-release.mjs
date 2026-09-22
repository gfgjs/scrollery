#!/usr/bin/env node
// 正式销售就绪检查（2026-09-22，随「官方版一次购买一次激活」建立）。
//
// 官方版是唯一商品主体（`src-tauri/resources/official-product.json`）。本脚本按
// `sales_open` 显式分流，回答两个不同问题：
//   • 未开放销售（默认）：输出「未开放，非销售可用」结论并**成功退出**——此时不做
//     商品页/公钥/发行源/模型交付的销售就绪断言，避免把「尚未开卖」误报成「可以开卖」。
//   • 要求销售就绪（`--require-sales`，或 sales_open=true）：四项交付硬门槛必须全过，
//     任一缺失即失败，不得以「检查通过」形态放行。
//
// 销售就绪四门槛（缺一即阻断销售）：
//   ① 商品页：store_url 是有效、非占位的 HTTPS 地址（与 `src-tauri/src/official.rs`
//      的 `valid_store_url` 同判据：禁凭据、禁 localhost/IP、禁 example/invalid/test 域）。
//   ② 信任根：`PICASA_EXOTIC_KEYSET_FILE` 指向正式公钥集——禁仓内占位落点、禁 dev/内测
//      键集，并按 **public_key_b64 值**禁仓内占位公钥（防重命名绕过）；按 schema 与运行期
//      时间窗（status/not_before/not_after）校验 license 与 release 两用途当前均可验签。
//   ③ 发行源：`PICASA_REGISTRY_BASE_DEFAULT` 是有效、非占位的 HTTPS 地址（未注入时退回
//      `exotic_commands.rs` 的编译期默认常量，现为 example.invalid 占位 → 阻断）。
//   ④ 增强模型清单：当前 `src-tauri/src/enhance/registry.rs` 仍是已核实的空清单形态
//      （`url: String::new()`，五档两件 fp32/fp16 资产运行期均不可下载/安装），**据此阻断销售**。
//      本检查不做「看起来钉定就算就绪」的正判定：源码一旦不再是该空形态，仍阻断并报告
//      「缺与运行期共用的结构化发行清单校验，暂不能确认就绪」，待增强资产交付任务接入
//      实际资产事实源（运行期同一份 `ModelAsset` 清单）后再放行。
//
// 判据取真实落点、结构化字段检查，不做全仓字符串扫描，也不靠「任意字符串看起来像就通过」。
// 启动先跑自测（喂隔离样本，不触正式秘密），防止检查器退化成只会 PASS 的空壳
// （同 verify-channel-bundle.mjs 姿态）。
//
// 用法：
//   node scripts/verify-commercial-release.mjs                  # 按 sales_open 分流
//   node scripts/verify-commercial-release.mjs --require-sales  # 要求销售就绪（未就绪即失败）
//   node scripts/verify-commercial-release.mjs --selftest       # 只跑检查器自测
// 调试用覆盖（默认走仓内正式落点）：`--product <json>`、`--keyset <json>`、`--registry-base <url>`。
// 增强清单无覆盖开关——其就绪不能由文本特征断言，只能在增强资产交付任务中接入实际事实源。

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
function argOf(flag, fallback) {
  const i = args.indexOf(flag)
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback
}
const REQUIRE_SALES = args.includes('--require-sales')

// ── 共享判据 ─────────────────────────────────────────────────────────────────

/** 占位/不可发布的主机名后缀（与官方版商店 URL 判据同集）。 */
const PLACEHOLDER_HOSTS = [
  'invalid',
  'example',
  'test',
  'example.com',
  'example.net',
  'example.org',
]

/** 非正式主机：localhost 域、占位 TLD、裸 IP。 */
function isPlaceholderHost(host) {
  if (!host) return true
  const h = host.toLowerCase()
  if (h === 'localhost' || h.endsWith('.localhost')) return true
  if (PLACEHOLDER_HOSTS.some((s) => h === s || h.endsWith('.' + s))) return true
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(h) || h.includes(':')
}

/** 有效且非占位的 HTTPS 地址（无凭据、非占位主机）。 */
function validHttpsUrl(raw) {
  if (typeof raw !== 'string' || raw.length === 0) return false
  let url
  try {
    url = new URL(raw)
  } catch {
    return false
  }
  return (
    url.protocol === 'https:' &&
    url.username === '' &&
    url.password === '' &&
    !isPlaceholderHost(url.hostname)
  )
}

/**
 * 仓内占位公钥集的确定值（`crates/scrollery-exotic-trust/resources/exotic-keyset.json`）。
 * 按 **public_key_b64 值**比对，而非路径或 key_id —— 占位集被复制/重命名到仓库外、
 * 或 key_id 被改写成不带 placeholder 字样，仍须被拦下。
 */
const PLACEHOLDER_PUBLIC_KEYS = new Set([
  'CJ0l4jfzOpJHZMGg5/SDTEGbM1kl8KAmG0ZoZR3yDOM=',
  '2n5maeyvPja+n8Ynt/muNLlX9Byd6K7B1lClUmXV6gw=',
])

/** 仓库内或非正式的 keyset 落点（禁用于正式销售）。 */
function forbiddenKeysetSource(file) {
  const p = path.resolve(file).replace(/\\/g, '/').toLowerCase()
  const reasons = []
  if (p.endsWith('/crates/scrollery-exotic-trust/resources/exotic-keyset.json')) {
    reasons.push('仓内占位公钥集落点')
  }
  if (p.includes('/.dev-registry/') || p.endsWith('/dev-keyset.json')) reasons.push('dev 键集')
  if (p.includes('/.internal-signing/') || p.endsWith('/internal-keyset.json')) {
    reasons.push('内测键集')
  }
  return reasons
}

/** 32 字节 Ed25519 裸公钥（标准 base64）。 */
function isRawEd25519Pub(b64) {
  if (typeof b64 !== 'string' || b64.length === 0) return false
  try {
    return Buffer.from(b64, 'base64').length === 32
  } catch {
    return false
  }
}

// ── 检查器（纯函数：输入解析后的数据，输出阻断原因数组）────────────────────────

/** ① 商品页 URL：仅销售就绪路径要求（未开卖时 store_url 允许为 null）。 */
export function checkStoreUrl(product) {
  const blocks = []
  if (typeof product.storeUrl !== 'string' || product.storeUrl.length === 0) {
    blocks.push('official-product.json 的 store_url 缺失 —— 正式销售须给出有效商品页地址')
  } else if (!validHttpsUrl(product.storeUrl)) {
    blocks.push(
      `store_url 非有效非占位 HTTPS 地址：「${product.storeUrl}」（禁 http/凭据/localhost/IP/example·invalid·test 域）`,
    )
  }
  return blocks
}

/**
 * ② 信任根：来源禁占位/dev/内测落点，**按 public_key_b64 值**禁仓内占位公钥，
 * 并按 `VerifyingKeyset::parse` 的实际契约（schema 1）与运行期时间窗
 * （`status`/`not_before`/`not_after`，见 crypto.rs::verify）校验 license 与 release 两用途。
 */
export function checkKeyset(keyset, sourceFile, nowSecs = Math.floor(Date.now() / 1000)) {
  const blocks = []
  for (const reason of forbiddenKeysetSource(sourceFile)) {
    blocks.push(`keyset 来源不可用于正式销售（${reason}）：${sourceFile}`)
  }
  if (!keyset || typeof keyset !== 'object') {
    blocks.push('keyset 非对象')
    return blocks
  }
  if (keyset.schema !== 1) blocks.push(`keyset schema 非 1：${keyset.schema}`)
  const keys = Array.isArray(keyset.keys) ? keyset.keys : []
  if (keys.length === 0) blocks.push('keyset 无任何公钥条目')
  for (const [i, k] of keys.entries()) {
    const where = `keyset.keys[${i}](${k && k.key_id})`
    if (!k || typeof k !== 'object') {
      blocks.push(`${where} 非对象`)
      continue
    }
    if (typeof k.key_id !== 'string' || k.key_id.length === 0) blocks.push(`${where} 缺 key_id`)
    else if (/placeholder|test|dev|internal/i.test(k.key_id)) {
      blocks.push(`${where} key_id 含占位/dev/内测字样，不得用于正式销售`)
    }
    if (!isRawEd25519Pub(k.public_key_b64)) blocks.push(`${where} 公钥非 32 字节 Ed25519（base64）`)
    else if (PLACEHOLDER_PUBLIC_KEYS.has(k.public_key_b64)) {
      blocks.push(`${where} 公钥值等于仓内占位公钥集，不得用于正式销售`)
    }
    if (k.purpose !== 'license' && k.purpose !== 'release')
      blocks.push(`${where} purpose 非法：${k.purpose}`)
    if (k.status !== 'active')
      blocks.push(`${where} status 非 active 不得用于正式销售：${k.status}`)
    if (!Number.isFinite(k.not_before)) {
      blocks.push(`${where} 缺 not_before（运行期时间窗字段）`)
    } else if (nowSecs < k.not_before) {
      blocks.push(`${where} not_before 晚于当前时间，销售时不可验签`)
    }
    if (k.not_after != null) {
      if (!Number.isFinite(k.not_after)) blocks.push(`${where} not_after 非法：${k.not_after}`)
      else if (nowSecs > k.not_after) {
        blocks.push(`${where} not_after 早于当前时间，销售时不可验签`)
      }
    }
  }
  for (const purpose of ['license', 'release']) {
    const usable = keys.some(
      (k) =>
        k &&
        k.purpose === purpose &&
        k.status === 'active' &&
        Number.isFinite(k.not_before) &&
        nowSecs >= k.not_before &&
        (k.not_after == null || (Number.isFinite(k.not_after) && nowSecs <= k.not_after)),
    )
    if (!usable) blocks.push(`keyset 缺 purpose=${purpose} 且当前时间窗内可验签的 active 正式键`)
  }
  return blocks
}

/** ③ 发行源：有效非占位 HTTPS。 */
export function checkRegistryBase(base) {
  const blocks = []
  if (typeof base !== 'string' || base.length === 0) {
    blocks.push('PICASA_REGISTRY_BASE_DEFAULT 未提供 —— 正式销售须注入发行源基址')
  } else if (!validHttpsUrl(base)) {
    blocks.push(
      `PICASA_REGISTRY_BASE_DEFAULT 非有效非占位 HTTPS 地址：「${base}」（编译期默认常量为占位 host 即阻断销售）`,
    )
  }
  return blocks
}

/**
 * ④ 增强模型清单：**只识别当前已核实的空清单形态**，据此阻断销售。
 *
 * 就绪与否的唯一事实源是运行期实际使用的结构化发行清单（`enhance/registry.rs` 的
 * `ModelAsset`，每档 fp32/fp16 两件带 HTTPS 直链、字节数与 sha256）。本脚本当前没有
 * 与运行期共用的清单读取/校验能力，文本正则会误认注释或无效代码 —— 故此处刻意**不做**
 * 「看起来钉定就算就绪」的判定：源码一旦不再是已知空形态，仍阻断并报告缺结构化校验，
 * 接入实际资产事实源（后续增强资产交付任务）后再放行。
 */
export function checkEnhanceDelivery(text) {
  const blocks = []
  if (typeof text !== 'string' || text.length === 0) {
    blocks.push('无法读取 src-tauri/src/enhance/registry.rs')
    return blocks
  }
  // 已核实的当前形态：资产 URL 由空串构造（`url: String::new()`）→ 每档 url 为空、
  // 字节数 0、sha256 无 → 运行期下载与安装恒关闭。
  if (/url:\s*String::new\(\)/.test(text)) {
    blocks.push(
      '增强模型资产未交付：src-tauri/src/enhance/registry.rs 仍以空 URL 构造清单（url: String::new()），' +
        '运行期两件 fp32/fp16 资产均不可下载或安装 —— 增强不可售，正式销售须阻断',
    )
    return blocks
  }
  blocks.push(
    '增强模型清单已非已知空形态：本检查缺与运行期共用的结构化发行清单校验，' +
      '无法确认五档各 fp32/fp16 资产是否真正就绪（注释或无效代码亦可命中文本特征）—— ' +
      '正式销售仍须阻断；待增强资产交付任务接入实际资产事实源后再放行',
  )
  return blocks
}

// ── 自测：正反样本喂检查器（隔离样本，不触正式秘密）───────────────────────────

function selftest() {
  const fails = []
  const expect = (cond, msg) => {
    if (!cond) fails.push(msg)
  }
  const ACTIVE_KEY = (id, purpose) => ({
    key_id: id,
    purpose,
    public_key_b64: 'A'.repeat(43) + '=',
    status: 'active',
    not_before: 0,
    not_after: null,
  })

  // ① 商品页：占位/非 HTTPS/凭据/IP 判非；正式 HTTPS 判是。
  expect(checkStoreUrl({ storeUrl: null }).length === 1, '① store_url 缺失应阻断')
  expect(
    checkStoreUrl({ storeUrl: 'https://example.invalid/plugins/psd' }).length === 1,
    '① 占位 host 应阻断',
  )
  expect(checkStoreUrl({ storeUrl: 'http://shop.scrollery.app/p' }).length === 1, '① http 应阻断')
  expect(
    checkStoreUrl({ storeUrl: 'https://shop.scrollery.app/products/official' }).length === 0,
    '① 正式 HTTPS 误报',
  )

  // ② 信任根：占位来源、dev 键、revoked、缺用途均判非；两用途 active 判是。
  const goodKeyset = {
    schema: 1,
    keys: [
      ACTIVE_KEY('license-prod-2026-09', 'license'),
      ACTIVE_KEY('release-prod-2026-09', 'release'),
    ],
  }
  expect(
    checkKeyset(goodKeyset, 'crates/scrollery-exotic-trust/resources/exotic-keyset.json').length >=
      1,
    '② 仓内占位公钥集应阻断',
  )
  expect(
    checkKeyset(goodKeyset, '/tmp/.dev-registry/dev-keyset.json').length >= 1,
    '② dev 键集应阻断',
  )
  expect(
    checkKeyset(goodKeyset, '/tmp/.internal-signing/internal-keyset.json').length >= 1,
    '② 内测键集应阻断',
  )
  const NOW = 1_800_000_000
  expect(
    checkKeyset(goodKeyset, '/etc/scrollery/keyset-prod.json', NOW).length === 0,
    '② 正式键集误报',
  )
  expect(
    checkKeyset({ schema: 2, keys: goodKeyset.keys }, '/etc/scrollery/keyset-prod.json', NOW)
      .length >= 1,
    '② schema 非 1 应阻断',
  )
  // 占位公钥值：即使改了 key_id（不带 placeholder 字样）、换了仓库外路径也须拦下。
  expect(
    checkKeyset(
      {
        schema: 1,
        keys: [
          {
            ...ACTIVE_KEY('license-prod-2026-09', 'license'),
            public_key_b64: 'CJ0l4jfzOpJHZMGg5/SDTEGbM1kl8KAmG0ZoZR3yDOM=',
          },
          goodKeyset.keys[1],
        ],
      },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② 占位公钥值(重命名绕过)应阻断',
  )
  expect(
    checkKeyset(goodKeyset, '/etc/scrollery/keyset-prod.json', NOW).length === 0,
    '② 非占位公钥值误报',
  )
  const revoked = {
    schema: 1,
    keys: [
      ACTIVE_KEY('license-prod-2026-09', 'license'),
      { ...ACTIVE_KEY('release-prod-2026-09', 'release'), status: 'revoked' },
    ],
  }
  expect(
    checkKeyset(revoked, '/etc/scrollery/keyset-prod.json', NOW).length >= 1,
    '② release 非 active 应阻断',
  )
  const placeholderId = {
    schema: 1,
    keys: [
      { ...ACTIVE_KEY('license-prod-placeholder', 'license') },
      ACTIVE_KEY('release-prod-2026-09', 'release'),
    ],
  }
  expect(
    checkKeyset(placeholderId, '/etc/scrollery/keyset-prod.json', NOW).length >= 1,
    '② 占位 key_id 应阻断',
  )
  expect(
    checkKeyset(
      { schema: 1, keys: [ACTIVE_KEY('license-prod-2026-09', 'license')] },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② 缺 release 用途应阻断',
  )
  expect(
    checkKeyset(
      { schema: 1, keys: [{ ...goodKeyset.keys[0], public_key_b64: 'short' }, goodKeyset.keys[1]] },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② 非法公钥长度应阻断',
  )
  // 有效时间窗：尚未生效、已过期、缺 not_before 均判非（与 crypto.rs::verify 同语义）。
  expect(
    checkKeyset(
      {
        schema: 1,
        keys: [
          { ...ACTIVE_KEY('license-prod-2026-09', 'license'), not_before: NOW + 86400 },
          goodKeyset.keys[1],
        ],
      },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② not_before 未到应阻断',
  )
  expect(
    checkKeyset(
      {
        schema: 1,
        keys: [
          goodKeyset.keys[0],
          { ...ACTIVE_KEY('release-prod-2026-09', 'release'), not_after: NOW - 1 },
        ],
      },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② not_after 已过应阻断',
  )
  expect(
    checkKeyset(
      { schema: 1, keys: [{ ...goodKeyset.keys[0], not_before: undefined }, goodKeyset.keys[1]] },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length >= 1,
    '② 缺 not_before 应阻断',
  )
  expect(
    checkKeyset(
      {
        schema: 1,
        keys: [
          goodKeyset.keys[0],
          { ...ACTIVE_KEY('release-prod-2026-09', 'release'), not_after: NOW + 86400 },
        ],
      },
      '/etc/scrollery/keyset-prod.json',
      NOW,
    ).length === 0,
    '② 时间窗内的 not_after 误报',
  )

  // ③ 发行源：编译期占位常量、非 HTTPS、空值均判非；正式 HTTPS 判是。
  expect(
    checkRegistryBase('https://registry.example.invalid/exotic/v1').length === 1,
    '③ 占位发行源应阻断',
  )
  expect(
    checkRegistryBase('http://registry.scrollery.app/exotic/v1').length === 1,
    '③ 非 HTTPS 应阻断',
  )
  expect(checkRegistryBase(null).length === 1, '③ 空发行源应阻断')
  expect(
    checkRegistryBase('https://registry.scrollery.app/exotic/v1').length === 0,
    '③ 正式发行源误报',
  )

  // ④ 增强清单：**只**有两类判定——已知空形态阻断；非空形态因缺结构化校验也阻断（不得放行）。
  expect(checkEnhanceDelivery('url: String::new(),').length === 1, '④ 已知空形态应阻断')
  expect(
    checkEnhanceDelivery(
      '// 注释里写着 url: "https://models.scrollery.app/x.onnx" 但真实构造仍是空串',
    ).length === 1,
    '④ 非空形态（含注释/无效文本）不得据文本特征放行',
  )
  expect(checkEnhanceDelivery('').length === 1, '④ 读不到源码应阻断')

  if (fails.length) {
    console.error('✗ verify-commercial-release selftest 失败（检查器自身已损坏，结果不可信）：')
    for (const f of fails) console.error('  - ' + f)
    process.exit(2)
  }
  console.log(
    '✓ verify-commercial-release selftest 通过（商品页/信任根/发行源/增强清单 四向正反样本）',
  )
}

// ── 实扫 ─────────────────────────────────────────────────────────────────────

function readJson(file) {
  return JSON.parse(readFileSync(file, 'utf8'))
}

/** 从 exotic_commands.rs 取编译期默认发行源常量（None 分支的字面量）。 */
function builtinRegistryBase() {
  const file = path.join(repo, 'src-tauri/src/ipc/exotic_commands.rs')
  const text = readFileSync(file, 'utf8')
  const at = text.indexOf('DEFAULT_REGISTRY_BASE_URL')
  const m = at >= 0 ? /None\s*=>\s*"([^"]+)"/.exec(text.slice(at, at + 400)) : null
  return m ? m[1] : null
}

function main() {
  selftest()
  if (args.includes('--selftest')) return

  const productFile = path.resolve(
    argOf('--product', path.join(repo, 'src-tauri/resources/official-product.json')),
  )
  const product = readJson(productFile)
  const salesOpen = product.sales_open === true

  console.log(
    `官方版商品配置：${path.relative(repo, productFile).replace(/\\/g, '/')}（product_id=${product.product_id}, sku=${product.sku}, sales_open=${salesOpen}）`,
  )

  if (!salesOpen && !REQUIRE_SALES) {
    // 未开放销售：明确给结论并成功退出，不做销售就绪断言，避免「未开卖」被读成「可开卖」。
    console.log(
      '结论：销售未开放，本构建非销售可用（未做商品页/公钥/发行源/模型交付的销售就绪断言）。',
    )
    console.log('如需按销售就绪校验（例如发售前演练），加 --require-sales。')
    return
  }

  const keysetFile = path.resolve(
    argOf(
      '--keyset',
      process.env.PICASA_EXOTIC_KEYSET_FILE ||
        path.join(repo, 'crates/scrollery-exotic-trust/resources/exotic-keyset.json'),
    ),
  )
  const registryBase = argOf(
    '--registry-base',
    process.env.PICASA_REGISTRY_BASE_DEFAULT || builtinRegistryBase(),
  )
  console.log(
    `检查落点：keyset=${path.relative(repo, keysetFile).replace(/\\/g, '/') || keysetFile}；registry_base=${registryBase}`,
  )

  const blocks = []
  blocks.push(...checkStoreUrl({ storeUrl: product.store_url == null ? null : product.store_url }))
  let keyset = null
  try {
    keyset = readJson(keysetFile)
  } catch (e) {
    blocks.push(`无法读取 keyset：${keysetFile}（${e.message}）`)
  }
  if (keyset) blocks.push(...checkKeyset(keyset, keysetFile))
  blocks.push(...checkRegistryBase(registryBase))
  const enhanceFile = path.join(repo, 'src-tauri/src/enhance/registry.rs')
  blocks.push(...checkEnhanceDelivery(readFileSync(enhanceFile, 'utf8')))

  if (blocks.length) {
    console.error(`✗ 销售未就绪：${blocks.length} 项阻断`)
    for (const b of blocks) console.error('  - ' + b)
    console.error('销售就绪的门槛见本脚本头部；缺交付时不得以「检查通过」形态放行。')
    process.exit(1)
  }
  console.log('✓ 销售就绪检查通过（商品页 / 正式公钥集 / 发行源 / 增强模型清单 四项齐备）')
}

main()
