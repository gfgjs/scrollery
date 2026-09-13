// src/composables/reader/useDocActiveViewerSync.ts
// activeViewer 单源接线（结构拆分自 DocumentViewer.vue §S34，顶栏重构 P5 余项）。
//
// 红线（§3 风险 3）：`src/commands/builtins/viewer-reader.ts` 通过
// ctx.activeViewer.api.toggleToc/toggleSearch/toggleBookmarks/toggleSettings **按方法名**调用
// （顶栏统一命令层，与阅读器自带工具栏「双入口」并存，P5-5 裁决）。装配 viewerApi 时必须**引用**
// 各来源域导出的同一个函数对象，不能重新包一层新函数——否则两个入口触发的是不同函数实例。
import { onBeforeUnmount, watch, type ComputedRef, type Ref, type WritableComputedRef } from 'vue'
import {
  toViewerFileInfo,
  type ActiveViewer,
  type ViewerApi,
  type useViewerStore,
} from '../../stores/viewerStore'
import { resolveViewerKind } from '../../utils/viewerKind'
import type { MediaDetail } from '../../types/media'

export interface UseDocActiveViewerSyncDeps {
  id: Ref<number>
  detail: Ref<MediaDetail | null>
  title: ComputedRef<string>
  immersive: WritableComputedRef<boolean>
  goBack: () => void
  toggleToc: () => void
  toggleSearch: () => void
  toggleBookmarks: () => void
  toggleSettings: () => void
  viewer: ReturnType<typeof useViewerStore>
}

export function useDocActiveViewerSync(deps: UseDocActiveViewerSyncDeps) {
  // 让阅读器进 viewerStore, 点亮标题栏上下文工具栏(foliate 的 TOC/搜索/书签/排版命令); ViewerApi
  // 映射到本地面板开关函数(引用透传,不重建)。与 DocumentViewer 自带阅读工具栏并存(双入口, 承
  // P5-5「保留局部控件」决策)。沉浸已并入 viewerStore.immersive, 与 P4-c 图/视沉浸统一。
  const viewerApi: ViewerApi = {
    close: deps.goBack,
    toggleToc: deps.toggleToc,
    toggleSearch: deps.toggleSearch,
    toggleBookmarks: deps.toggleBookmarks,
    toggleSettings: deps.toggleSettings,
    // 与图/视 api 面对称(命令层可统一寻址沉浸);写 computed 代理即写 viewerStore。
    toggleImmersive: () => {
      deps.immersive.value = !deps.immersive.value
    },
  }
  let viewerToken: number | null = null
  // kind 用 resolveViewerKind('document', fmt) 单点推导(pdf/epub/text/markdown), 命令 when 据此过滤:
  // foliate(epub/text/markdown)显示 TOC/搜索/书签/设置, pdf 不显示。
  function readerSnapshot(): Omit<ActiveViewer, 'immersive'> {
    const d = deps.detail.value
    return {
      kind: resolveViewerKind('document', d?.fileFormat ?? ''),
      mediaType: 'document',
      fileFormat: d?.fileFormat ?? '',
      id: Number.isFinite(deps.id.value) ? deps.id.value : null,
      path: d?.absPath ?? null,
      title: d?.fileName ?? deps.title.value,
      api: viewerApi,
      // 底栏文件信息:detail 即 MediaDetail 全字段,直接投影(watch(detail) 守 null,d 实非空)。
      fileInfo: d ? toViewerFileInfo(d) : null,
    }
  }
  // detail 加载完成(每篇一次)→ 首次 populate、后续换文档 patch。token 时序防御见 viewerStore。
  watch(deps.detail, (d) => {
    if (!d) return
    if (viewerToken === null) {
      viewerToken = deps.viewer.populate({ ...readerSnapshot(), immersive: false })
    } else {
      deps.viewer.patch(viewerToken, readerSnapshot())
    }
  })

  onBeforeUnmount(() => {
    // 离开阅读器:清 activeViewer 上下文(token 时序防御, 迟到 clear 不误清新查看器)。
    // 卸载次序注:本次拆分后 viewer.clear 由旧实现的最后一步变为最先——clear 只写 viewerStore,
    // 而 flushProgress/pager.detach/removeEventListener 三者均不读它,无观察面,顺序调整安全。
    if (viewerToken !== null) deps.viewer.clear(viewerToken)
  })

  return { viewerApi }
}
