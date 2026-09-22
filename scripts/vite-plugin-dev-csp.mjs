// dev 态注入 CSP(2026-07-16 安全审查 #4:此前 dev 环境完全没有 CSP)。
//
// 为什么必须由 Vite 注入、Tauri 管不到(已读 tauri 2.11.4 源码核实,非推理):
// Tauri 的 CSP 注入(tauri crate manager/mod.rs 的 set_csp,含内联脚本/样式的自动 CSP 哈希)
// 只发生在 AppManager::get_asset() —— 它服务的是**自己打包的 frontendDist 资源**,经
// tauri:// 自定义协议。本项目 tauri.conf.json 配了 devUrl(Vite dev server),`tauri dev`
// 时 WebView 直接导航到 http://127.0.0.1:1420,请求完全不经过 get_asset()。config 里的
// `app.security.devCsp` 字段虽存在,但在 devUrl 生效的配置下从未被任何代码路径读取 —— 写了
// 也没用(唯一消费点 AppManager::csp() 只在 get_asset() 内被调用)。故 CSP 只能由 Vite 自己
// 在 index.html 注入,且只能在 dev 态注入(prod 走 frontendDist,get_asset() 会用
// tauri.conf.json 的 app.security.csp 正常处理,与本插件无关)。
//
// 单一事实源:dev CSP 由 prod CSP(tauri.conf.json `app.security.csp`)派生,而非另写一份
// —— 避免两份 CSP 字符串各改各的、悄悄漂移出不一致的放行范围。**唯一**的追加放行是
// connect-src 的开发服务器 HMR WebSocket 来源(Vite HMR 走 WebSocket),不放宽 script-src 等其它指令。
//
// 来源收窄(F14):只放行开发服务器**实际**的 HMR 地址,不放行裸 `ws:`(那等于放行任意
// 主机的 WebSocket)。地址取自 Vite 已解析的 server 配置,不再另维护一份:默认取 server.host/port
// (本项目 127.0.0.1:1420),显式 `TAURI_DEV_HOST` 时取 hmr.host/hmr.port(本项目 host:1421)。

/**
 * 从 Vite resolved server 配置取 HMR WebSocket 来源(纯函数,便于核对)。
 * @param {{host?:string,port?:number,https?:unknown,hmr?:{protocol?:string,host?:string,port?:number}}} server
 * @returns {string} 形如 `ws://127.0.0.1:1420` 的来源
 */
export function hmrWsOrigin(server) {
  const hmr = server.hmr && typeof server.hmr === 'object' ? server.hmr : undefined
  const protocol = hmr?.protocol || (server.https ? 'wss' : 'ws')
  return `${protocol}://${hmr?.host || server.host}:${hmr?.port || server.port}`
}

/**
 * 从生产 CSP 派生开发态 CSP(纯函数,不碰 fs/Vite,便于单测)。
 * @param {string} prodCsp tauri.conf.json 的 app.security.csp 原始字符串
 * @param {string} wsOrigin HMR WebSocket 来源(由 {@link hmrWsOrigin} 从 resolved 配置得到)
 * @returns {string}
 */
export function deriveDevCsp(prodCsp, wsOrigin) {
  const directives = prodCsp
    .split(';')
    .map((d) => d.trim())
    .filter(Boolean)
    .map((d) => {
      const [name, ...values] = d.split(/\s+/)
      return { name, values }
    })

  const connectSrc = directives.find((d) => d.name === 'connect-src')
  if (connectSrc) {
    if (!connectSrc.values.includes(wsOrigin)) connectSrc.values.push(wsOrigin)
  } else {
    // prod CSP 目前恒有 connect-src,这支只是防御性兜底,避免上游改了配置就悄悄失去 HMR 放行。
    directives.push({ name: 'connect-src', values: ["'self'", wsOrigin] })
  }

  return directives.map((d) => [d.name, ...d.values].join(' ')).join('; ')
}

/**
 * Vite 插件:薄适配层 —— 读 tauri.conf.json、派生 dev CSP、注入 <meta> 标签。
 * `apply: 'serve'` 是唯一的环境判据:只在 `vite dev`/`tauri dev` 生效,`vite build` 从不调用
 * transformIndexHtml 的这个钩子实例,故不会污染产物 index.html(prod 的 CSP 仍完全来自
 * Tauri 侧注入,双方互不覆盖)。
 * @param {string} [tauriConfPath] tauri.conf.json 绝对路径;默认相对本文件定位仓库内路径
 */
export default function devCsp(tauriConfPath) {
  return {
    name: 'scrollery:dev-csp',
    apply: 'serve',
    async transformIndexHtml(_html, ctx) {
      const { readFileSync } = await import('node:fs')
      const { fileURLToPath } = await import('node:url')
      const { dirname, resolve } = await import('node:path')
      const confPath =
        tauriConfPath ??
        resolve(dirname(fileURLToPath(import.meta.url)), '../src-tauri/tauri.conf.json')

      let prodCsp
      try {
        const conf = JSON.parse(readFileSync(confPath, 'utf-8'))
        prodCsp = conf?.app?.security?.csp
      } catch (err) {
        console.warn('[dev-csp] 读取 tauri.conf.json 失败,dev 态不注入 CSP:', err)
        return
      }
      if (typeof prodCsp !== 'string' || !prodCsp) {
        console.warn('[dev-csp] tauri.conf.json 缺少 app.security.csp,dev 态不注入 CSP')
        return
      }

      const wsOrigin = hmrWsOrigin(ctx.server.config.server)
      return [
        {
          tag: 'meta',
          attrs: {
            'http-equiv': 'Content-Security-Policy',
            content: deriveDevCsp(prodCsp, wsOrigin),
          },
          injectTo: 'head-prepend',
        },
      ]
    },
  }
}
