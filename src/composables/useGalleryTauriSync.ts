// 画廊 Tauri 事件 + 布局脏标/版本联动(自 MediaGrid.vue 结构拆分抽出,watcher 顺序逐字保留)。
//
// watcher 注册顺序即 flush 顺序,四个布局相关 watcher(totalItems / layoutDirty / layoutVersion /
// viewport-meta)必须保持此文件内的声明次序。
import { watch, onScopeDispose } from 'vue'
import { useTauriListen } from './useTauriListen'
import { useViewIds } from './useViewIds'
import { useMediaStore } from '../stores/mediaStore'
import { useDedupStore } from '../stores/dedupStore'
import { useUiStore } from '../stores/uiStore'
import { EVENTS } from '../constants/ipc'
import { logger } from '../utils/logger'
import { needsViewportMeta } from '../components/media/mediaGrid.helpers'
import type { LayoutRow } from '../types/layout'

export interface GalleryTauriSyncDeps {
  /** 自动重算入口：离屏时登记 deferred，激活后补算当前视图。 */
  requestCompute: () => boolean
  refreshCacheDir: () => Promise<void>
  /** browse-only 镜头不需要把布局 flat_ids 物化到前端。 */
  shouldLoadViewIds: () => boolean
  isLensActive: () => boolean
  /** 当前已挂载段行(视口元数据取 id 用)。 */
  activeRows: () => LayoutRow[]
}

export function useGalleryTauriSync(deps: GalleryTauriSyncDeps) {
  const ui = useUiStore()
  const media = useMediaStore()
  const dedup = useDedupStore()
  const viewIds = useViewIds()

  // ── 监听增强事件 ────────────────────────────────────────────

  let enrichRefreshTimer: ReturnType<typeof setTimeout> | null = null
  let lastCompletedRunId: string | null = null

  function clearEnrichRefreshTimer() {
    if (enrichRefreshTimer !== null) {
      clearTimeout(enrichRefreshTimer)
      enrichRefreshTimer = null
    }
  }

  // useTauriListen 处理 disposed 竞态并在作用域销毁时解绑，故直接在 setup 同步注册。
  useTauriListen(EVENTS.MEDIA_ENRICHED, () => {
    // 数据增强每批触发一次:2s 首沿窗口——每个待刷新窗口自首事件起最多等待 2s,
    // 窗口内的后续事件不延后。(旧写法是纯尾沿防抖:持续富化期间事件不断推后,
    // 只能等富化结束才重算一次。)富化终态不再另开冲刷:stop/清库的 tombstone 与正常
    // 完成同 runId,仅凭事件无法区分,代之以窗口自然到期。
    if (enrichRefreshTimer !== null) return
    enrichRefreshTimer = setTimeout(() => {
      // 定时器即消费:触发后置空,后续事件再起新窗口。
      enrichRefreshTimer = null
      deps.requestCompute()
    }, 2000)
  })

  // 卷插拔监听：卷在线态变化时刷新 availability 徽标。
  // 卷态变化频率极低（拔插），无需防抖，直接重算即可。
  useTauriListen(EVENTS.VOLUMES_CHANGED, () => {
    deps.requestCompute()
  })

  // 文件监听对账完成（fs_watch 广播 { rootId, directories }）：磁盘内容变化后
  // 统计数与会入库项可能已变，刷新统计并请求重算当前视图（requestCompute 自带离屏 deferred）。
  // 目录树侧由 useFolderTreeSync 各自监听同一事件做原位刷新，与此处互不依赖。
  //
  // 可视元数据（名称/路径/EXIF）会因磁盘改动而变，而 id 不变 → viewport-meta 的 watch 源
  // （layoutSemanticKey + 元素列表）不变，watch 不会重跑，旧名称/路径会滞留。故这里先清空
  // meta 窗口，再按当前可视行主动重取一次（口径与下方 viewport-meta watch 完全一致）。
  function refreshViewportMeta() {
    media.ensureMeta([]) // 清空待发集合并撤销在途响应，避免旧结果回写
    if (!media.layoutSemanticKey || !ui.showThumbInfo || !needsViewportMeta(ui.thumbInfoElements)) return
    const rows = deps.activeRows()
    if (rows.length === 0 && media.totalRows > 0) return // 换代中：与 watch 一致，等首段就绪
    const ids: number[] = []
    for (const row of rows) {
      if (row.rowType === 'normal') {
        for (const it of row.items) ids.push(it.id)
      }
    }
    if (ids.length > 0) media.ensureMeta(ids)
  }

  useTauriListen(EVENTS.FILES_CHANGED, () => {
    void media.loadStats().catch((error) => {
      logger.error('[GalleryTauriSync] files:changed 刷新统计失败', { error })
    })
    deps.requestCompute()
    refreshViewportMeta()
  })

  // 组件卸载与(父 composable / effectScope)销毁都会走到这里:监听器解绑由
  // useTauriListen 自己的 onScopeDispose 负责,此处只清尾窗口定时器(与其它
  // 可脱离组件单测的 composable 同款锚点)。
  onScopeDispose(() => {
    clearEnrichRefreshTimer()
    media.ensureMeta([])
  })

  // 当 totalItems 发生变化（扫描完成 / 清除数据）时，走统一重算闸门。
  watch(
    () => media.totalItems,
    () => {
      deps.requestCompute()
    },
  )

  // 当布局被标记为脏时（例如全量缩略图生成完成且网格已挂载），走统一重算闸门。
  watch(
    () => media.layoutDirtyRevision,
    async () => {
      if (!media.layoutDirty) return
      await deps.refreshCacheDir()
      // 缓存目录读取期间可能已有布局完成；仍有未消费的失效才请求，成功后由布局入口消费。
      if (media.layoutDirty) deps.requestCompute()
      // updateVisible 由下方的 layoutVersion watcher 处理
    },
  )

  // 去重分析完成后，镜头集合才有新一版可用内容。只对「完成 + 新 runId」响应，
  // 避免相同快照/重复 completed 事件反复重算；requestCompute 自带离屏 deferred。
  watch(
    () => [dedup.status.status, dedup.status.runId] as const,
    ([status, runId]) => {
      if (status !== 'completed' || !runId || !deps.isLensActive()) return
      if (runId === lastCompletedRunId) return
      lastCompletedRunId = runId
      deps.requestCompute()
    },
  )

  // 当布局发生变化时（由于调整大小、文件夹切换、过滤器等原因），刷新可见的行
  watch(
    () => media.layoutVersion,
    () => {
      // 布局变了 → 普通画廊刷新选区用的布局序全集；镜头 browse-only 由查看器走
      // get_lens_adjacent_media，不把全量 flat_ids 灌入前端。fire-and-forget 不阻塞滚动恢复。
      if (deps.shouldLoadViewIds() && media.orderVersion > 0) void viewIds.ensureFresh(media.orderVersion)
      // 目标解析、段表与滚动恢复由 bucket 换代统一提交。
    },
  )

  // 当信息浮层开启时，仅为可视项懒加载重型元数据（EXIF/GPS/名称/路径）——
  // 这些字段已从常驻布局缓存剥离（A1），经 get_meta_for_viewport 按需提供。
  watch(
    // 源即消费面:段挂载/卸载与开关/元素变化都经本 getter 依赖追踪(取 id 的同时建立依赖)。
    () => {
      // 只有勾了真正消费元数据的元素才按视口拉(S7):此前只看总开关,勾 size/status 这类
      // 纯 item 字段的元素也会每屏拉一遍 EXIF/GPS/路径,拉回来无人渲染。元素列表本身也
      // 须入 watch 源,否则中途勾上 camera 要等到行变化才补拉。
      if (!media.layoutSemanticKey || !ui.showThumbInfo || !needsViewportMeta(ui.thumbInfoElements)) return ''
      const rows = deps.activeRows()
      // 换代期间显示种子不属于新窗口；等首段就绪再替换窗口,避免几何重排先清空同集合元数据。
      if (rows.length === 0 && media.totalRows > 0) return null
      const ids: number[] = []
      for (const row of rows) {
        if (row.rowType === 'normal') {
          for (const it of row.items) ids.push(it.id)
        }
      }
      return ids.join(',')
    },
    (key) => {
      if (key === null) return
      media.ensureMeta(key ? key.split(',').map(Number) : [])
    },
    { immediate: true },
  )
}
