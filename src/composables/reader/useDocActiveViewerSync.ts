// src/composables/reader/useDocActiveViewerSync.ts
// 将阅读器接入 activeViewer，顶栏命令与阅读器工具栏共用各面板的开关函数。
import { computed, onBeforeUnmount, watch, type ComputedRef, type Ref, type WritableComputedRef } from 'vue'
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
  prepareClose: () => Promise<boolean>
  readerCommands: ComputedRef<Pick<ViewerApi, 'toggleToc' | 'toggleSearch' | 'toggleBookmarks' | 'toggleSettings'>>
  viewer: ReturnType<typeof useViewerStore>
}

export function useDocActiveViewerSync(deps: UseDocActiveViewerSyncDeps) {
  // 方法名供统一命令层寻址，直接复用本地操作。
  const viewerApi = computed<ViewerApi>(() => ({
    close: deps.goBack,
    prepareClose: deps.prepareClose,
    ...deps.readerCommands.value,
    // 与图/视 api 面对称(命令层可统一寻址沉浸);写 computed 代理即写 viewerStore。
    toggleImmersive: () => {
      deps.immersive.value = !deps.immersive.value
    },
  }))
  let viewerToken: number | null = null
  // 文件分类保持稳定；可用面板按当前引擎暴露，切引擎或编辑时不保留空操作按钮。
  function readerSnapshot(): Omit<ActiveViewer, 'immersive'> {
    const d = deps.detail.value
    return {
      kind: resolveViewerKind('document', d?.fileFormat ?? ''),
      mediaType: 'document',
      fileFormat: d?.fileFormat ?? '',
      id: Number.isFinite(deps.id.value) ? deps.id.value : null,
      path: d?.absPath ?? null,
      title: d?.fileName ?? deps.title.value,
      api: viewerApi.value,
      // 底栏文件信息:detail 即 MediaDetail 全字段,直接投影(watch(detail) 守 null,d 实非空)。
      fileInfo: d ? toViewerFileInfo(d) : null,
    }
  }
  // 详情就绪后发布当前资产；同一篇的能力变化只更新快照。token 时序防御见 viewerStore。
  watch([deps.detail, deps.readerCommands], ([d]) => {
    if (!d) {
      // 换文档等待或失败时，底栏不得继续对上一资产评分、收藏或执行旧命令。
      if (viewerToken !== null) deps.viewer.clear(viewerToken)
      viewerToken = null
      return
    }
    if (viewerToken === null) {
      viewerToken = deps.viewer.populate({ ...readerSnapshot(), immersive: false })
    } else {
      deps.viewer.patch(viewerToken, readerSnapshot())
    }
  })

  onBeforeUnmount(() => {
    // token 防止迟到的卸载清掉新查看器上下文。
    if (viewerToken !== null) deps.viewer.clear(viewerToken)
  })

  return { viewerApi }
}
