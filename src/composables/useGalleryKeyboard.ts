// 画廊语义子视图返回栏 + document 级键盘处理(自 MediaGrid.vue 结构拆分抽出,判据逐字保留)。
//
// **不变量：凡是从某个总览页钻进来的子视图，都必须有视图内的退出路径（可见返回栏 + ESC）。**
// 此前只有人物兑现了这条，收藏夹漏了——真机 round10 #4：进收藏夹后既无退出钮、ESC 也无效，只能按系统
// 后退键。注意 ESC 监听器本来就在、按键也收到了，缺的只是判据里的 collection 分支：这类「机制已在、
// 判据没覆盖」的漏，无编译/SSR 信号，只有真机能发现。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useMediaStore } from '../stores/mediaStore'
import { useToastStore } from '../stores/toastStore'
import { useFilterStore } from '../stores/filterStore'
import { useViewStore } from '../stores/viewStore'
import { usePersonStore } from '../stores/personStore'
import { useSelection, type BackendSelectionDescriptor } from './useSelection'
import type { LayoutRowItem } from '../types/layout'

export interface GalleryKeyboardDeps {
  selectionDescriptor: () => BackendSelectionDescriptor
  patchVisibleSelected: (apply: (item: LayoutRowItem) => void) => void
  compute: (width?: number) => Promise<void>
  /** §8.1 browse-only 谓词:重复镜头激活时旁路 document 级选择语义（宿主从 duplicateLensStore 注入）。 */
  lensActive: () => boolean
}

export function useGalleryKeyboard(deps: GalleryKeyboardDeps) {
  const { t } = useI18n()
  const router = useRouter()
  const media = useMediaStore()
  const toast = useToastStore()
  const filter = useFilterStore()
  const viewStore = useViewStore()
  const person = usePersonStore()
  const selection = useSelection()

  // viewStore 四维互斥（setActiveCollection 会清 person，反之亦然），故至多命中一条。
  const backBar = computed<{ label: string; to: string } | null>(() => {
    if (viewStore.activePersonId != null) {
      const p = person.persons.find((pp) => pp.id === viewStore.activePersonId)
      // 人物墙已加载则取名，否则回退「未命名人物」。
      return {
        label: t('persons.backTo', { name: p?.name || t('persons.unnamedPerson') }),
        to: '/persons',
      }
    }
    const collection = viewStore.activeCollection
    if (collection) {
      return { label: t('collections.backTo', { name: collection.name }), to: '/collections' }
    }
    return null
  })

  // 回总览页。维度状态（activePersonId / activeCollection）保留即可——总览页不依赖它，且使侧栏项保持高亮。
  function exitToOverview() {
    const target = backBar.value?.to
    if (!target) return
    if (router.currentRoute.value.path !== target) router.push(target)
  }

  function onKeyDown(e: KeyboardEvent) {
    // 图/视仍以 /view 路由为地址单一事实源，但当前由 MediaGrid 宿主保留底层画廊 DOM；宿主在
    // `/view` 覆盖层存续期不调用本处理器，ESC 归 ContentViewer。此处只保留纯画廊语义，避免
    // 再次把路由呈现策略耦进键盘 composable。
    // 语义子视图（人物 / 收藏夹）下 ESC 回其总览页；但若处于多选态，优先让 selection 清选区（标准相册行为）。
    if (e.key === 'Escape' && backBar.value && !selection.isSelectionMode.value) {
      e.preventDefault()
      exitToOverview()
      return
    }
    // 选择态下数字键 1-5 给选区批量评分、0 清空（标准相册快捷键）。守门:仅多选态、非输入
    // 元素、无修饰键,避免与浏览/搜索框输入/Ctrl 组合键冲突。
    if (
      selection.isSelectionMode.value &&
      !e.ctrlKey &&
      !e.metaKey &&
      !e.altKey &&
      /^[0-5]$/.test(e.key)
    ) {
      const el = e.target as HTMLElement | null
      const editing =
        el?.tagName === 'INPUT' || el?.tagName === 'TEXTAREA' || el?.isContentEditable === true
      if (!editing) {
        if (selection.selectedCount.value > 0) {
          e.preventDefault()
          const rating = Number(e.key)
          media
            .batchSetRating(deps.selectionDescriptor(), rating)
            .then(async (n) => {
              // 乐观刷新选区星级（rating 已入布局行，可即时显形）。
              deps.patchVisibleSelected((it) => {
                it.rating = rating
              })
              toast.addToast(
                'success',
                rating === 0
                  ? t('selection.ratingCleared', { count: n })
                  : t('selection.rated', { count: n, rating }),
              )
              // 「≥N 星」筛选激活且批量评分跌破阈值 → 这些项应离开视图，重算。
              if (filter.minRating > 0 && rating < filter.minRating) {
                await deps.compute()
              }
            })
            .catch((err) => {
              toast.addToast(
                'error',
                t('selection.rateFailed', {
                  error: err instanceof Error ? err.message : String(err),
                }),
              )
            })
          return
        }
      }
    }
    // §8.1 browse-only:重复镜头不进入选择态(isSelectionMode 恒 false),selection 的 document 级
    // 语义——Ctrl/Cmd+A 全选、ESC 清选区——显式旁路(上方数字键评分分支亦被 isSelectionMode 门控为
    // 死路,批量工具条不会出现);backBar 的 ESC 语义保留在其上方分支。
    if (!deps.lensActive()) selection.onKeyDown(e)
  }

  return { backBar, exitToOverview, onKeyDown }
}
