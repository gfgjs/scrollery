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
import { scrollCache } from '../utils/scrollCache'
import { EVENTS } from '../constants/ipc'
import { needsViewportMeta } from '../components/media/mediaGrid.helpers'
import type { LayoutRow } from '../types/layout'

export interface GalleryTauriSyncDeps {
  /** 自动重算入口：离屏时登记 deferred，激活后补算当前视图。 */
  requestCompute: () => boolean
  refreshCacheDir: () => Promise<void>
  restoreReflowAnchor: () => Promise<boolean>
  gridRef: () => HTMLElement | null
  scrollToLogicalY: (y: number) => Promise<void>
  getViewKey: () => string
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

  // useTauriListen 内部处理 disposed 竞态并在作用域销毁时解绑(P1-11),故直接在 setup 同步注册。
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

  // 卷插拔监听（Part2 T2）：卷在线态变化 → 重算刷新 availability 徽标（离线灰显隐）。
  // 卷态变化频率极低（拔插），无需防抖，直接重算即可。
  useTauriListen(EVENTS.VOLUMES_CHANGED, () => {
    deps.requestCompute()
  })

  // 组件卸载与(父 composable / effectScope)销毁都会走到这里:监听器解绑由
  // useTauriListen 自己的 onScopeDispose 负责,此处只清尾窗口定时器(与其它
  // 可脱离组件单测的 composable 同款锚点)。
  onScopeDispose(clearEnrichRefreshTimer)

  // 当 totalItems 发生变化（扫描完成 / 清除数据）时，走统一重算闸门。
  watch(
    () => media.totalItems,
    () => {
      deps.requestCompute()
    },
  )

  // 当布局被标记为脏时（例如全量缩略图生成完成且网格已挂载），走统一重算闸门。
  watch(
    () => media.layoutDirty,
    async (dirty) => {
      if (!dirty) return
      await deps.refreshCacheDir()
      // 离屏时保留 dirty，激活补算时由 flushIfDeferred 一并消费。
      if (deps.requestCompute()) media.consumeLayoutDirty()
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
    async (v) => {
      // 布局变了 → 普通画廊刷新选区用的布局序全集；镜头 browse-only 由查看器走
      // get_lens_adjacent_media，不把全量 flat_ids 灌入前端。fire-and-forget 不阻塞滚动恢复。
      if (deps.shouldLoadViewIds()) void viewIds.ensureFresh(v)
      // 等待 DOM，允许在布局渲染之前设置 scrollTop
      const el = deps.gridRef()
      if (el) {
        // 整体重排(行高/分组/排序/宽度/布局模式):重新锚定到之前浏览的项。无锚的布局
        // 变化(文件夹/筛选/…)回退到保存的滚动位。
        const restored = await deps.restoreReflowAnchor()
        if (!restored) {
          const saved = scrollCache.get(deps.getViewKey()) || 0
          await deps.scrollToLogicalY(saved)
        }
      }
      // 段表重建由引擎的 layoutVersion watch 自驱,无需另发取数请求。
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
      if (!ui.showThumbInfo || !needsViewportMeta(ui.thumbInfoElements)) return ''
      const ids: number[] = []
      for (const row of deps.activeRows()) {
        if (row.rowType === 'normal') {
          for (const it of row.items) ids.push(it.id)
        }
      }
      return ids.join(',')
    },
    (key) => {
      if (!key) return
      media.ensureMeta(key.split(',').map(Number))
    },
    { immediate: true },
  )
}
