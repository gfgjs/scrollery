// 大图查看器路由加载器：保持路由级分包，同时允许画廊在空闲期提前预取。
// 预取与路由实际加载共用同一个 Promise，避免首次点击重复请求/解析约 153KB 的查看器代码块。

type ViewerComponentModule = typeof import('../components/media/ContentViewer.vue')

let viewerComponentPromise: Promise<ViewerComponentModule> | null = null

export function loadViewerComponent(): Promise<ViewerComponentModule> {
  return (viewerComponentPromise ??= import('../components/media/ContentViewer.vue'))
}

/** 画廊空闲时预取查看器代码；失败交给实际路由加载时重试并呈现。 */
export function preloadViewerComponent(): void {
  void loadViewerComponent().catch(() => {
    // 预取是性能优化，失败不能阻断画廊；实际进入路由时会再次尝试加载。
    viewerComponentPromise = null
  })
}
