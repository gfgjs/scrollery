// src/composables/useViewerColorSource.ts
// 查看器渲染色域换源状态机(2026-07-23 自定义 ICC 与色域切换,方案 B §0⑥):
// 「原图先显 + 完成后换源」——切换查看项/target 时保持当前显示不变,后台发起 IPC 求新派生 URL,
// 就绪后原地换源;srgb/移动端/非 image 恒不发起 IPC(D-413/D-414,零派生)。

import { ref, watch, type Ref } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { resolveAssetUrl } from '../utils/assetUrl'
import { logger } from '../utils/logger'

/** 消费方按需下发的最小查看项形状(id + mediaType),取 getter 形式随源实时求值(先例 useThumbLoader)。 */
export interface ViewerColorSourceInput {
  /** 当前查看项;非 image / null 时不发起换源,直接归 null。 */
  detail: () => { id: number; mediaType: string } | null | undefined
  /** viewer_color_target 配置值('srgb'/'display-p3'/'dci-p3'/'custom')。 */
  target: () => string
  /** 自定义 ICC id;后端从 config 单源读取实际生效值,这里仅作 watch 依赖(target 热切换感知)。 */
  customId: () => string
  /** 平台门控(D-414):移动端恒不发起,直显原图,不受 target 影响。 */
  isMobile: boolean
}

export interface ViewerColorSource {
  /** 派生渲染文件的 asset URL;null=直显原图(sRGB/移动端/非 image/尚未就绪/失败回退)。 */
  displayUrl: Ref<string | null>
  /**
   * `<img>` @error 前置分支调用(边界情况11:命中判定与前端加载间被 LRU 驱逐):当前显示源若是
   * 派生 URL,复位为 null 令消费方的 absPath 回落原图重试一次。返回 true=已处理(调用方应
   * return,不进入原图失败态);返回 false=当前本就显示原图,调用方按既有失败流程处理。
   */
  handleDisplayUrlError: () => boolean
}

export function useViewerColorSource(input: ViewerColorSourceInput): ViewerColorSource {
  const displayUrl = ref<string | null>(null)
  // 请求令牌:发起后查看项/target 变化则丢弃迟到响应(先例 ContentViewer.vue 原 poster watch)。
  let token = 0

  async function refresh() {
    const my = ++token
    const item = input.detail()
    const target = input.target()
    if (
      !item ||
      input.isMobile ||
      item.mediaType !== 'image' ||
      target === 'srgb' ||
      (target === 'custom' && !input.customId())
    ) {
      // 原图先显契约:非派生适用场景不发 IPC,直接置空——absPath 消费方回落原图。
      // custom 且 customId 为空是主线裁决短路:未选定自定义档位时不发 IPC(零派生)。
      displayUrl.value = null
      return
    }
    try {
      const path = await invokeIpc<string | null>(IPC.GET_VIEWER_COLOR_URL, { itemId: item.id })
      if (my !== token) return // 过期响应(查看项/target 已变):丢弃
      displayUrl.value = path ? resolveAssetUrl(path) : null
    } catch (e) {
      if (my !== token) return
      logger.warn('get_viewer_color_url failed, falling back to original image', { error: e })
      displayUrl.value = null
    }
  }

  watch(
    () => [input.detail()?.id, input.detail()?.mediaType] as const,
    (_next, prev) => {
      // 查看项(id)变化:新项先落回原图,再发请求换源(plan-B §0⑥ 语义)。
      // prev 为 undefined 时是首次挂载(immediate watch 无上一值),不属于「切项」不清空。
      if (prev !== undefined) displayUrl.value = null
      void refresh()
    },
    { immediate: true },
  )

  watch(
    () => [input.target(), input.customId()] as const,
    () => void refresh(),
    // target/customId 变化:保持当前显示不变直至新结果返回,故无需在此清空 displayUrl。
  )

  function handleDisplayUrlError(): boolean {
    if (displayUrl.value === null) return false
    displayUrl.value = null
    return true
  }

  return { displayUrl, handleDisplayUrlError }
}
