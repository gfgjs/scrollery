// src/utils/mediaRoute.ts
// 打开媒体的统一路由分发（2026-07-10 深审问题2）。此前 FoldersSection 与 MediaGrid 各自
// 复制「按类型分发 /doc /audio /view」三分支,且都无条件 router.push——查看器是路由页而
// 侧栏树仍可见可点,在 /view/33 再点树里一张图会把 /view/34 又压一条 history;三个查看器
// 的关闭都是 router.back(),于是连点 N 张要按 N+1 次 ESC 才能回画廊。
// 修法:已身处任一查看器路由时改用 replace(查看器换查看器,语义上是「换一张」而非「再进
// 一层」,与 ContentViewer 翻页 replace 同理);从画廊/集合页等主视图打开仍 push(back 一次
// 即回来处)。

import type { Router } from 'vue-router'

/** 三个内容查看器的路由前缀(App.vue 顶栏显隐同判据,单一事实源)。 */
export const VIEWER_ROUTE_PREFIXES = ['/view/', '/doc/', '/audio/'] as const

/** 当前路径是否处于某个内容查看器(大图 / 阅读页 / 音频播放器)。 */
export function isViewerRoute(path: string): boolean {
  return VIEWER_ROUTE_PREFIXES.some((p) => path.startsWith(p))
}

/** 图/视频统一查看器 `/view/:id` 的路由判据。 */
export function isContentViewerRoute(path: string): boolean {
  return path.startsWith('/view/')
}

/**
 * 该查看器视图是否**自持**侧栏开关(放在自己的工具栏里)。
 *
 * AppShell 的浮动侧栏开关(.viewer-sidebar-toggle,absolute top:10px left:12px)本是给**没有 chrome
 * 可放**的全幅表面兜底的——大图(/view/)就是这种。阅读页有自己的工具栏,浮动钮便与其左上角的返回
 * 控件**几何重叠**(工具栏 padding 8px 12px,浮动钮 top:10 left:12,侧栏收起时正好压上去):
 * 两个带框小按钮叠在一起,一个跳走一个开侧栏(2026-07-16 真机「跟左侧栏的展开/收起按钮放在一起,
 * 容易误导」)。故自持者不再浮动兜底,避免两个开关。
 *
 * 只列 /doc/ 而非「所有有工具栏的视图」:/audio/ 也有工具栏,但它**尚未**自持开关——一并停掉会让
 * 音频页的侧栏开关凭空消失。本谓词描述的是「已自持」这个事实,待 /audio/ 也自持时再加前缀。
 */
const SELF_HOSTED_SIDEBAR_TOGGLE_PREFIXES = ['/doc/'] as const

export function viewerHostsSidebarToggle(path: string): boolean {
  return SELF_HOSTED_SIDEBAR_TOGGLE_PREFIXES.some((p) => path.startsWith(p))
}

/**
 * 按媒体类型解析目标查看器路由:文档 → /doc,音频 → /audio,图/视等其余 → /view。
 * @param id 库内资产 id
 * @param mediaType DB 主类(LayoutRowItem.mediaType 为宽松 string,故不收窄为 MediaType)
 */
export function viewerRouteFor(id: number, mediaType: string): string {
  if (mediaType === 'document') return `/doc/${id}`
  if (mediaType === 'audio') return `/audio/${id}`
  return `/view/${id}`
}

/**
 * 打开媒体:主视图内 push(可 back 返回),查看器内 replace(防 history 叠层)。
 * 跨类型同样适用——在阅读页点图片 replace 到 /view 后,back 仍直接回画廊。
 */
export function openMediaRoute(router: Router, id: number, mediaType: string): void {
  const target = viewerRouteFor(id, mediaType)
  if (isViewerRoute(router.currentRoute.value.path)) void router.replace(target)
  else void router.push(target)
}
