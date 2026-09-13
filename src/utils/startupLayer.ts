const STARTUP_LAYER_ID = 'startup-layer'
const STARTUP_LAYER_EXIT_DELAY_MS = 360

/**
 * 将入口静态启动层交给已挂载的应用，并在淡出完成后移除节点。
 *
 * 启动层不做成 Vue 组件：它必须早于入口 bundle 绘制，且日志窗口也复用同一个入口 HTML。
 */
export function dismissStartupLayer(): void {
  if (typeof document === 'undefined') return

  const layer = document.getElementById(STARTUP_LAYER_ID)
  if (!layer || layer.dataset.state === 'ready') return

  layer.dataset.state = 'ready'
  globalThis.setTimeout(() => {
    if (layer.dataset.state === 'ready') layer.remove()
  }, STARTUP_LAYER_EXIT_DELAY_MS)
}
