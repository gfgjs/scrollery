// public/theme-snapshot.js
// 防 FOUC:样式解析前按本地快照(uiStore.applyAppearance/setThemeTintStrength/
// setThemeTextStrength 每次应用时刷新)先落 data-theme 与底色/文字浓度(--theme-tint-scale/
// --theme-text-scale),固定主题的老用户首帧即着对色;权威源仍是 app_config,挂载后由
// 启动配置批(get_startup_config)校正。快照缺失/损坏 → 回退系统偏好。
//
// 外置为 public/ 静态文件而非 index.html 内联(2026-07-16 安全审查 #4):Tauri 生产态会给
// 内联 <script> 自动算 CSP 哈希放行(manager/mod.rs set_csp),但那条路径只走它自己服务的
// 资源(tauri:// 协议 get_asset());本项目 dev 用 devUrl,WebView 直连 Vite,完全绕过
// get_asset(),该哈希机制不会跑。外置后 prod/dev 用同一条 script-src 'self' 规则覆盖,
// 不必为 dev 单独维护一份哈希或放行 unsafe-inline。
// 必须是**经典脚本**(无 type="module"):模块脚本默认 defer,会晚于首次绘制执行,FOUC 防护失效;
// 经典 <script src> 与原内联脚本一样阻塞 HTML 解析、在 <body> 渲染前同步跑完。
;(function () {
  var mode = 'system'
  var light = 'fresh-light'
  var dark = 'fresh-dark'
  // 两个浓度默认与 themes/strength.ts 的 THEME_TINT_DEFAULT/THEME_TEXT_DEFAULT 一致;
  // 快照缺值(旧版本)或越界时落默认,避免首帧底色/文字浓度与水合后不一致而闪变。
  var tint = 60
  var text = 100
  // 首帧与注册表使用相同映射，升级后的旧快照也能立即命中新配色。
  var legacy = {
    moonlight: 'fresh-light',
    porcelain: 'minimal-light',
    xuan: 'tech-light',
    ink: 'fresh-dark',
    obsidian: 'minimal-dark',
    dai: 'tech-dark',
    light: 'fresh-light',
    dark: 'fresh-dark',
  }
  function normalize(raw, kind) {
    var id = Object.prototype.hasOwnProperty.call(legacy, raw) ? legacy[raw] : raw
    return ['fresh-' + kind, 'minimal-' + kind, 'tech-' + kind].indexOf(id) >= 0
      ? id
      : 'fresh-' + kind
  }
  try {
    var s = JSON.parse(localStorage.getItem('scrollery.themeSnapshot.v1') || 'null')
    if (s) {
      if (s.appearance === 'light' || s.appearance === 'dark' || s.appearance === 'system')
        mode = s.appearance
      light = normalize(s.light, 'light')
      dark = normalize(s.dark, 'dark')
      if (typeof s.tint === 'number' && s.tint >= 0 && s.tint <= 100) tint = Math.round(s.tint)
      if (typeof s.text === 'number' && s.text >= 40 && s.text <= 100) text = Math.round(s.text)
    }
  } catch (_e) {
    /* 快照损坏走系统偏好兜底 */
  }
  var isDark =
    mode === 'dark' ||
    (mode === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  var d = document.documentElement
  d.setAttribute('data-theme', isDark ? dark : light)
  d.setAttribute('data-color-scheme', isDark ? 'dark' : 'light')
  d.style.setProperty('--theme-tint-scale', String(tint / 100))
  d.style.setProperty('--theme-text-scale', String(text / 100))
})()
