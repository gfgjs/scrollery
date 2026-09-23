// src/themes/bootstrapScript.ts
// 首帧脚本的**唯一来源**(方案 §7:阻塞式、只恢复已生成变量、不含颜色算法与主题 ID 清单)。
//
// 为什么不放在 public/ 手写:校验需要「色板应有哪些变量」这份清单,而它的唯一真源是
// generate.ts 的 THEME_PALETTE_VARS。手写一份到静态脚本等于第二张表,迟早漂移;故脚本源码在
// 此处生成(期待键集由构建期注入),由 vite 插件在 dev 以中间件、build 以资产形式按同一 URL
// `/theme-bootstrap.js` 交付,index.html 只引用该 URL。
//
// 交付形态仍是**经典脚本**:模块脚本默认 defer,会晚于首次绘制;经典 <script src> 阻塞 HTML
// 解析、在 <body> 渲染前同步跑完。

import { THEME_CACHE_KEY, THEME_CACHE_VERSION } from './snapshot'
import { THEME_PALETTE_VARS } from './generate'

/** 启动脚本按此 URL 交付(dev 中间件与 build 资产同名)。 */
export const THEME_BOOTSTRAP_URL = '/theme-bootstrap.js'

/**
 * 生成首帧脚本源码。
 *
 * 校验口径(与 snapshot.ts 的 parseThemeCache 一致,核心恢复用例集中在 core.spec.ts):
 *  - 浅/深两套变量表都必须**键集与 THEME_PALETTE_VARS 完全一致**(缺项、多项、空表都判为无效);
 *  - 每个值必须是具体颜色(hex/rgb/rgba/transparent),不接受 var()/color-mix()/空串;
 *  - opacity 必须是 0–100 的整数、appearance/material 必须是已知字面量;
 *  - 任一不符即**整份丢弃**:一个颜色都不写,由构建期生成的默认主题 CSS 兜底(不做部分恢复)。
 */
export function buildBootstrapScript(): string {
  return `// 由 src/themes/bootstrapScript.ts 生成:首帧恢复脚本,勿手改(改生成器或 generate.ts 变量表)
// 防 FOUC:样式解析前按本地缓存还原**已生成**的 CSS 变量,并解析外观偏好/系统明暗写
// data-color-scheme;材质与不透明度(仅 Windows)一并还原。
//
// 缓存只由主题域在**后端已确认**配置后写入(${THEME_CACHE_KEY});草稿与未确认写入不进缓存。
// 缓存缺失、损坏、版本不符或结构不完整时一个颜色都不写,由 index.html 中由同一 generateTheme
// 生成的默认主题 CSS(:root / html[data-color-scheme='dark'])兜底。
;(function () {
  var KEY = ${JSON.stringify(THEME_CACHE_KEY)}
  var EXPECTED_KEYS = ${JSON.stringify(THEME_PALETTE_VARS)}
  var OPACITY_VAR = '--window-opacity'
  var CONCRETE_COLOR = /^(#[0-9a-f]{6}|rgba?[(][0-9., ]+[)]|transparent)$/
  var APPEARANCES = ['system', 'light', 'dark']
  var MATERIALS = ['none', 'mica', 'acrylic']
  var VISUAL_STYLES = ['standard', 'mint', 'forest']

  function has(object, name) {
    return Object.prototype.hasOwnProperty.call(object, name)
  }

  /** 变量表必须与期待键集一一对应,且每个值都是具体颜色。 */
  function isCompleteVars(vars) {
    if (!vars || typeof vars !== 'object') return false
    var count = 0
    for (var name in vars) {
      if (!has(vars, name)) continue
      if (typeof vars[name] !== 'string' || !CONCRETE_COLOR.test(vars[name])) return false
      count += 1
    }
    if (count !== EXPECTED_KEYS.length) return false
    for (var i = 0; i < EXPECTED_KEYS.length; i += 1) {
      if (!has(vars, EXPECTED_KEYS[i])) return false
    }
    return true
  }

  function readCache() {
    var parsed = null
    try {
      var raw = localStorage.getItem(KEY)
      parsed = raw ? JSON.parse(raw) : null
    } catch (_e) {
      return null
    }
    if (!parsed || typeof parsed !== 'object') return null
    if (parsed.v !== ${THEME_CACHE_VERSION}) return null
    if (APPEARANCES.indexOf(parsed.appearance) < 0) return null
    if (MATERIALS.indexOf(parsed.material) < 0) return null
    if (VISUAL_STYLES.indexOf(parsed.visualStyle) < 0) return null
    if (!isCompleteVars(parsed.light) || !isCompleteVars(parsed.dark)) return null
    if (typeof parsed.opacity !== 'number' || parsed.opacity !== Math.round(parsed.opacity)) return null
    if (parsed.opacity < 0 || parsed.opacity > 100) return null
    return parsed
  }

  /** 平台判定与 utils/platform.ts 同源(该值由 plugin-os 在启动期同步注入);此处不能 import,
      故读同一注入对象;注入缺失(裸浏览器 dev)按 UA 兜底——宁可不上玻璃,也不给非 Windows
      窗口画没有原生背板的透明层。 */
  function isWindows() {
    try {
      var os = window.__TAURI_OS_PLUGIN_INTERNALS__
      if (os && typeof os.platform === 'string') return os.platform === 'windows'
    } catch (_e) {
      /* 走 UA 兜底 */
    }
    var ua = (window.navigator && window.navigator.userAgent) || ''
    return /Windows/i.test(ua)
  }

  var cache = readCache()
  var dark =
    cache && cache.appearance === 'dark'
      ? true
      : cache && cache.appearance === 'light'
        ? false
        : window.matchMedia('(prefers-color-scheme: dark)').matches
  var root = document.documentElement
  root.setAttribute('data-color-scheme', dark ? 'dark' : 'light')
  if (!cache) return

  if (cache.visualStyle !== 'standard') root.setAttribute('data-visual-style', cache.visualStyle)

  var vars = dark ? cache.dark : cache.light
  for (var index = 0; index < EXPECTED_KEYS.length; index += 1) {
    var name = EXPECTED_KEYS[index]
    root.style.setProperty(name, vars[name])
  }
  if (!isWindows() || cache.material === 'none') return
  root.setAttribute('data-glass', cache.material)
  root.style.setProperty(OPACITY_VAR, String(cache.opacity) + '%')
})()
`
}
