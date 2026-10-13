// src/composables/useContentViewerRouteNav.ts
// 查看器路由驱动翻页。不含 close/closeViewer——
// 那两个跨多个功能域被引用,留在编排层(ContentViewer.vue)。
// 缓存与实时翻页经 mediaStore 的同一上下文守卫提交,随后同步当前路由。

import { computed, onScopeDispose } from 'vue'
import type { RouteLocationNormalizedLoaded, Router } from 'vue-router'
import type { AdjacentDetail, useMediaStore } from '../stores/mediaStore'
import { requestDiscardEdits, type useImageEditor } from './useImageEditor'

export function useContentViewerRouteNav(options: {
  media: ReturnType<typeof useMediaStore>
  editor: ReturnType<typeof useImageEditor>
  router: Router
  route: RouteLocationNormalizedLoaded
  onNavigate?: (offset: number) => void
  getCachedAdjacent?: (id: number, offset: number) => AdjacentDetail | undefined
  /** 由画廊镜头状态提供；镜头上下文缺失时也必须禁止普通邻接降级。 */
  isLensActive?: () => boolean
}) {
  const { media, editor, router, route, onNavigate, getCachedAdjacent, isLensActive } = options

  // /view/:id 的 :id 是「当前查看项」的单一事实源。路由变化(深链 / 前进后退 / 翻页后同步)→
  // 加载该项;翻页经 navigate() 走 mediaStore 邻接后 replace 路由,loadFromRoute 守卫防重复加载。
  const routeId = computed(() => Number(route.params.id))
  let navigationEpoch = 0
  let disposed = false
  let pendingDiscard: Promise<boolean> | null = null
  // 查看器位于 RouterView 外，侧栏、历史记录和换图都需在路由提交前保护当前草稿。
  const removeRouteGuard = router.beforeEach(async (to, from) => {
    if (to.path === from.path || editor.status.value === 'idle') return true
    if (editor.status.value === 'saving') return false
    // 连续导航共用一次确认，避免后一次请求替换全局对话框的回调。
    pendingDiscard ??= requestDiscardEdits(editor).finally(() => { pendingDiscard = null })
    const confirmed = await pendingDiscard
    if (disposed || !confirmed) return false
    editor.close()
    return true
  })
  onScopeDispose(() => {
    disposed = true
    navigationEpoch++
    removeRouteGuard()
  })

  function ensureLensContext() {
    if (!isLensActive?.() || media.navContext?.type === 'lens') return
    media.setLensNavContext(media.layoutVersion, media.viewTotalItems)
  }

  async function loadFromRoute(id: number) {
    if (disposed || !Number.isFinite(id)) return
    navigationEpoch++
    // 深链/刷新没有经过 MediaGrid 点击路径时，仍建立无数组的镜头上下文，
    // 以便后续方向键只走带 layoutVersion 的镜头 IPC。
    ensureLensContext()
    if (!isLensActive?.() && !media.navContext) media.setLayoutNavContext(id)
    // 翻页已把 detailItem 换到目标(随后 replace 路由触发本加载)→ 免重复 IPC。
    if (media.detailItem?.id === id) return
    await media.openDetail(id)
  }

  // 统一翻页:镜头上下文经带 layoutVersion 的镜头 IPC；普通上下文保持缓存/普通邻接路径。
  // replace 同步 URL(loadFromRoute 守卫跳过重载)。onKeydown/onWheel/移动后跳转共用。
  async function navigate(offset: number) {
    if (disposed || editor.status.value !== 'idle') return // 编辑中禁翻页；顶栏工具栏按钮同经此函数
    const epoch = ++navigationEpoch
    onNavigate?.(offset)
    const curId = media.detailItem?.id
    const lensContext = media.navContext?.type === 'lens' || isLensActive?.() === true
    if (lensContext && media.navContext?.type !== 'lens') {
      ensureLensContext()
    }
    const context = media.navContext
    if (!media.isNavContextCurrent(context) || curId == null) return
    const cached = getCachedAdjacent?.(curId, offset)
    if (cached && media.applyAdjacentDetail(context, curId, cached)) {
      if (cached.detail.id !== routeId.value) void router.replace(`/view/${cached.detail.id}`)
      return
    }
    await media.navigateDetail(offset)
    if (disposed || epoch !== navigationEpoch || !media.isNavContextCurrent(context)) return
    const id = media.detailItem?.id
    if (id != null && id !== routeId.value) void router.replace(`/view/${id}`)
  }

  return { routeId, loadFromRoute, navigate }
}
