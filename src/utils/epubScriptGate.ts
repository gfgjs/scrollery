// src/utils/epubScriptGate.ts
// 拒绝 EPUB 内的脚本资源 —— 让「脚本化 EPUB 不支持」从声明变成实现。
//
// 背景(2026-07-16 安全审查)。三件事叠在一起才是问题:
//   ① EPUB 是**不可信第三方 HTML/CSS/JS**(用户下载来的 zip,内含任意 XHTML/CSS/JS);
//   ② 章节文档经 **blob: URL** 载入 iframe(vendor/foliate-js/epub.js),blob URL **继承创建者的源**
//      → 该 iframe 与父页**同源**;
//   ③ 父页持有 Tauri IPC 桥(__TAURI_INTERNALS__),同源即意味着 iframe 内脚本可经
//      window.parent 直达全部已注册命令。
// iframe 上那句 `sandbox="allow-same-origin allow-scripts"`(paginator.js)**不构成边界** ——
// 这两个值同时给出是公认的互相抵消组合(同源 + 可执行脚本 = 跨 frame 访问由同源策略放行,
// sandbox 不拦)。写了 sandbox 却等于没写。
//
// `docs/designs/2026-07-07-阅读器完善方案.md` §4.8 早已规范性裁决「脚本化 EPUB 不支持」,
// 与 foliate-js 上游立场一致 —— 但审查查明:**执行这条裁决的代码从不存在**。上游为此预留的唯一
// seam(epub.js `loadItem` 派发的 'load' 事件,detail 带 `isScript` 与 `allow`)全仓零订阅,
// 于是 `allow` 恒真,`<script src>` 一路走到可用的 blob URL。本模块就是那个订阅者。
//
// **本模块只管资源型脚本**(<script src>、以及任何 JS MIME 的清单项),因为那是上游 seam 的作用域。
// 内联 <script> 与 on* 事件属性不经 loadItem,故不在此列 —— 它们由 CSP 挡,而非 sandbox:
//   · script-src 'self'(无 'unsafe-inline'、无 'unsafe-eval')同时禁止内联 <script>、on* 属性、
//     javascript: URI —— CSP3 明文禁的是这三类,不只是 <script src>。
//   · blob: iframe **继承创建者(父页)的 CSP**(CSP3 §"inherit a policy"、engine 实测一致:
//     https://csplite.com/csp/test404/),故这条防线在 iframe 内同样成立,不是只护到父页自己。
//   · 唯一让它在**全部环境**成立的前提是 dev 也有 CSP —— 2026-07-16 安全审查 #4 已补
//     (vite-plugin-dev-csp.mjs);此前 dev 全程无 CSP,这条防线彼时只在生产成立。
//
// 「摘掉 sandbox 的 allow-scripts」**已评估并否决**,理由三条:
//   ① VENDOR.md 硬性红线:仅 pdf.js 可 patch,paginator.js 逐字节钉死上游,不可改;
//   ② 运行时 DOM 改 sandbox 属性同样不可行:View 每次翻章都新建 iframe(#createView,
//      paginator.js:666),且 `.src` 赋值与 iframe 挂载在**同一段同步代码**里完成(无 await
//      间隙可插入 MutationObserver 回调,后者是微任务、必然排在其后)—— 没有安全的时机窗口;
//   ③ 即便能摘,也不该摘:上游那句注释指向的 WebKit bug 218086(已核实,2025-02 仍未修)是
//      「摘掉 allow-scripts 后 Safari/WKWebView 连事件监听都不派发」,且该 bug 自己给出的
//      workaround 正是「改用 CSP script-src 限制,而非摘 sandbox 的 allow-scripts」——
//      与本模块的选择完全一致,不是本模块绕开了正解,是本模块采的就是上游认证过的正解。
// 三层(loadItem 拒绝资源脚本 + CSP 挡内联 + CSP 经 blob 继承覆盖 iframe)独立生效、互不承重。

import { logger } from './logger'

/** 上游 epub.js `loadItem` 在每个清单项载入前派发的 'load' 事件 detail。 */
interface LoaderLoadDetail {
  /** 该项的 MIME。 */
  type?: string
  /** 上游按 MIME.JS 判定的「这是脚本」。 */
  isScript?: boolean
  /** 置 false 即拒绝该项(上游 `if (!allow) return null`)。上游对它取 await,故同步赋值即可。 */
  allow?: boolean
}

/** 最小的 book 面 —— 只要 transformTarget,便于测试与解耦(勿 import 整个 view.d.ts)。 */
export interface ScriptGateTarget {
  transformTarget?: EventTarget
}

/**
 * 订阅 book 的资源载入拦截口,拒绝一切脚本资源。
 *
 * 时序要求:必须在**首章载入前**调用。`view.open()` 只建 book、`renderer.open()` 只存 sections,
 * 都不载章;首章由随后的 goTo/init 发起 —— 故「await view.open() 之后、恢复位置之前」是唯一正确的窗口。
 * 早于 open 则 book 尚不存在,晚于首次 goTo 则漏掉首章(而首章恰是攻击者最想放脚本的地方)。
 *
 * @param book 已 open 的 book(EPUB 有 transformTarget;SyntheticBook/comic/fb2 无 → 静默不装,
 *   它们的内容由我们自己生成,不含第三方脚本)
 * @returns 摘除订阅的函数
 */
export function denyScriptResources(book: ScriptGateTarget | undefined): () => void {
  const target = book?.transformTarget
  if (!target) return () => {}

  const onLoad = (e: Event) => {
    const detail = (e as CustomEvent<LoaderLoadDetail>).detail
    if (!detail?.isScript) return
    detail.allow = false
    // 留诊断信号:静默拒绝会让「这本书打开后某功能没了」变成无从查起的玄学。
    // 走 warn 而非 error:拒绝是**预期**行为,不是故障。
    logger.warn('[epubScriptGate] blocked scripted resource in EPUB', { type: detail.type })
  }

  target.addEventListener('load', onLoad)
  return () => target.removeEventListener('load', onLoad)
}
