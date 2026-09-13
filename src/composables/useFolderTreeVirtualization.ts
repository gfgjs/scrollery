// src/composables/useFolderTreeVirtualization.ts
// 文件树虚拟化(方案 B,共享滚动,T1-a/T1-b)——从 FoldersSection.vue 域 3/4 迁出(见
// docs/planning/2026-07-25-超长文件拆分方案/analysis/FoldersSection-vue.md §2.1)。
// 行窗口化、行高实测、粘性区块堆叠占位、粘性目录头链、结构轴滚动定位;是 KeyboardNav /
// AutoLoadMore / DragDrop 三个 composable 共同依赖的基础设施层,应尽早稳定接口。
//
// 树复用侧栏共享滚动区(.sidebar__scroll-area)的 scrollTop,不自持滚动,避免改动 accordion
// 单一块级流 + 跨区 sticky 架构。定高 ROW_H,只渲染视口窗口内的行,DOM 从 O(展开行) 降到 O(视口)。
import {
  ref,
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
  nextTick,
  shallowRef,
  type ComputedRef,
} from 'vue'
import { useSidebarSections } from './useSidebarSections'
import {
  computeTreeWindow,
  treeScrollTopForIndex,
  stickyHeaderChain,
} from '../components/sidebar/sections/folderTree.helpers'
import type { TreeRow } from '../components/sidebar/sections/folderTree.helpers'

// 统一行高(文件行 26 升 28,+2px 纯观感);须与 CSS .tree-item/.file-item height 同步
export const ROW_H = 28
// 树行基础左缩进:.tree 横向 padding 4 + 26 = 30 = --sidebar-indent(侧栏统一内容轨,见 AppSidebar)。
export const TREE_INDENT = 26
const BUFFER = 6 // 上下预渲染行数,防快滚边缘瞬白

export function useFolderTreeVirtualization(displayRows: ComputedRef<TreeRow[]>) {
  const treeRef = ref<HTMLElement | null>(null)
  // §2.3-①:scrollAreaEl 现状是模块作用域的裸 let,要跨 composable 共享读写(拖拽边缘自动滚动
  // 要写 scrollTop,键盘导航/自动加载都要读),改为 shallowRef 导出;运行时行为不变,只是把闭包
  // 变量包了一层 ref 以便传递。
  const scrollAreaEl = shallowRef<HTMLElement | null>(null)
  const startIndex = ref(0)
  const endIndex = ref(0)
  const offsetY = ref(0)
  const visibleRows = computed(() => displayRows.value.slice(startIndex.value, endIndex.value))
  // 视口顶所在行(不含 buffer),驱动粘性目录头栈(T1-b)。随滚动在 updateWindow 内更新。
  const firstVisibleIndex = ref(0)
  const stickyRows = computed(() => stickyHeaderChain(displayRows.value, firstVisibleIndex.value))
  // 行高:CSS 名义 28,但为防 border/box-sizing 差异致 spacer 偏短、滚动错位,从首个已渲染行实测一次。
  const rowH = ref(ROW_H)
  // ── 粘性区块标题堆叠占位(遮挡修复)────────────────────────────────────────────
  // 滚进树内后,「图库/工具/文件夹」标题粘顶堆叠、后续区块标题粘底,均不透明且 z-index 高于树内
  // 浮层——视口上/下两条边各有一段"不可用区"。粘性目录头链和滚动定位都必须避开它们。
  const sections = useSidebarSections()
  const headerH = ref(32) // 与 --sidebar-header-h 同步,initScroll 时实测一次
  const foldersIndex = computed(() => sections.visibleIds.value.indexOf('folders'))
  // 树主体上方粘顶的标题数 = folders 的 index + 1(含自身标题)。
  const stackTopPx = computed(() => (foldersIndex.value + 1) * headerH.value)
  // 树主体下方粘底的标题数 = folders 之后的区块数(如「管理」)。
  const stackBottomPx = computed(
    () => Math.max(0, sections.visibleIds.value.length - 1 - foldersIndex.value) * headerH.value,
  )
  let rowHMeasured = false
  function measureRowH() {
    if (rowHMeasured) return
    const el = treeRef.value?.querySelector('.tree-item, .file-item') as HTMLElement | null
    if (el && el.offsetHeight > 0) {
      rowH.value = el.offsetHeight
      rowHMeasured = true
    }
  }
  const spacerHeight = computed(() => displayRows.value.length * rowH.value)

  // 树容器顶部在滚动内容坐标系的偏移(受上方 section 展开/折叠影响,故每次读实时 rect)。
  function treeOffsetTop(): number {
    const t = treeRef.value
    if (!t || !scrollAreaEl.value) return 0
    return (
      t.getBoundingClientRect().top -
      scrollAreaEl.value.getBoundingClientRect().top +
      scrollAreaEl.value.scrollTop
    )
  }

  let rafId: number | null = null
  function updateWindow() {
    if (!scrollAreaEl.value) return
    measureRowH()
    // viewportH 钳到 innerHeight:flex 布局收敛前的瞬态里,clientHeight 会短暂读成整段内容高
    // (真机实证:初始态误渲 8467 行),把窗口算成全量。视口不可能大于窗口高,故封顶自愈。
    const viewportH = Math.min(scrollAreaEl.value.clientHeight, window.innerHeight)
    const offTop = treeOffsetTop()
    const scrollTop = scrollAreaEl.value.scrollTop
    const w = computeTreeWindow(
      scrollTop,
      offTop,
      viewportH,
      displayRows.value.length,
      rowH.value,
      BUFFER,
    )
    startIndex.value = w.startIndex
    endIndex.value = w.endIndex
    offsetY.value = w.offsetY
    // 视口顶所在行(不含 buffer)→ 粘性目录头栈(T1-b)。以「粘顶标题堆叠底沿」为有效视口顶:
    // 物理视口顶那几行被不透明标题栈盖住,真正可见的首行在 scrollTop+stackTopPx 处。
    // relTop<=0(尚未滚入树/首行头完全可见无需钉)→ 置 -1,stickyHeaderChain 返回空 →
    // 覆盖层隐藏,避免树顶重影+多余投影。
    const relTop = scrollTop + stackTopPx.value - offTop
    firstVisibleIndex.value = relTop <= 0 ? -1 : Math.floor(relTop / rowH.value)
  }
  function scheduleUpdate() {
    if (rafId !== null) return
    rafId = requestAnimationFrame(() => {
      rafId = null
      updateWindow()
    })
  }

  // ── 静止门(问题2:快滚刷屏)状态 ─────────────────────────────────────────────────
  // 快速飞掠超长树时,被扫过的每个目录 more 行都会进缓冲窗口触发自动分页;单飞只是串行化,接力仍
  // 追着滚动条给一长串「未停留」目录发 IPC,且在视口上方插行会抽动虚拟内容(刷屏)。lastScrollTs 记录
  // 最近一次「用户滚动」时刻,供 useFolderTreeAutoLoadMore 的静止门判定。§2.3-③:改暴露取值函数
  // 而非共享裸变量。
  let lastScrollTs = 0
  function getLastScrollTs(): number {
    return lastScrollTs
  }

  // 用户滚动专用处理器:打时间戳供静止门判定,再走常规窗口重算。transitionend / ResizeObserver 仍
  // 直接调 scheduleUpdate,不污染 lastScrollTs(它们不是用户滚动,不应压制自动加载)。
  function onScroll() {
    lastScrollTs = performance.now()
    scheduleUpdate()
  }

  let treeResizeObserver: ResizeObserver | null = null
  // 惰性初始化:树在有扫描根后才渲染(v-if),treeRef 可能晚于 onMounted 才就位;幂等。
  function initScroll() {
    if (scrollAreaEl.value || !treeRef.value) return
    scrollAreaEl.value = treeRef.value.closest('.sidebar__scroll-area') as HTMLElement | null
    if (!scrollAreaEl.value) return
    // 标题高实测:--sidebar-header-h 定义在 .sidebar 上,可从任何后代经继承读取;失败留默认 32。
    const h = parseFloat(
      getComputedStyle(scrollAreaEl.value).getPropertyValue('--sidebar-header-h'),
    )
    if (h > 0) headerH.value = h
    scrollAreaEl.value.addEventListener('scroll', onScroll, { passive: true })
    // 兄弟 section 展开/折叠(高度过渡)会改变树在滚动内容中的偏移,但不触发 scroll/RO;
    // 监听冒泡上来的 accordion 高度 transitionend → 过渡结束重算窗口。
    scrollAreaEl.value.addEventListener('transitionend', scheduleUpdate)
    if (typeof ResizeObserver !== 'undefined') {
      treeResizeObserver = new ResizeObserver(scheduleUpdate)
      treeResizeObserver.observe(scrollAreaEl.value)
    }
    updateWindow()
  }
  watch(treeRef, initScroll)
  // displayRows 变化(展开/折叠/reload)→ spacer 高变,下一帧重算窗口。
  watch(displayRows, () => nextTick(updateWindow))
  onMounted(initScroll)
  onBeforeUnmount(() => {
    scrollAreaEl.value?.removeEventListener('scroll', onScroll)
    scrollAreaEl.value?.removeEventListener('transitionend', scheduleUpdate)
    treeResizeObserver?.disconnect()
    treeResizeObserver = null
    if (rafId !== null) cancelAnimationFrame(rafId)
    // 静止门尾随定时器、拖拽自动滚动 rAF 分别归属 AutoLoadMore / DragDrop composable 自己的
    // onBeforeUnmount(§3 风险④:stopAutoScroll 必须由 DragDrop 自己注册,不能遗漏)。
  })

  // 把某目录行滚入可视区(虚拟化后 scrollIntoView 失效,改索引→共享 scrollTop,M5)。
  // insets:上=粘顶标题堆叠+滚动到位后将钉住的祖先链(≈node.depth 行,同链上限 6),
  // 下=粘底标题堆叠;否则目标行恰好停在不透明浮层底下,「滚到了」但看不见。
  // 走**结构轴**(nodeKey):把树滚到某一行是纯结构操作,FS-only 目录同样该能滚到(设计 §4.1
  // 明列「展开、键盘导航」为允许项;禁止的「画廊滚动锚点」是另一回事——那是 ui.pendingScrollDirId)。
  async function scrollTreeToNodeKey(nodeKey: string, isCurrent: () => boolean = () => true) {
    await nextTick()
    // latest-wins 队列可能在 nextTick 期间收到新目标；真正写共享 scrollTop 前再验一次。
    if (!isCurrent()) return
    const idx = displayRows.value.findIndex((r) => r.kind === 'dir' && r.node.nodeKey === nodeKey)
    if (idx < 0 || !scrollAreaEl.value) return
    const row = displayRows.value[idx]
    const chainRows = row.kind === 'dir' ? Math.min(row.node.depth, 6) : 0
    const target = treeScrollTopForIndex(
      idx,
      treeOffsetTop(),
      scrollAreaEl.value.scrollTop,
      Math.min(scrollAreaEl.value.clientHeight, window.innerHeight),
      rowH.value,
      stackTopPx.value + chainRows * rowH.value,
      stackBottomPx.value,
    )
    scrollAreaEl.value.scrollTo({ top: target, behavior: 'smooth' })
  }

  return {
    treeRef,
    scrollAreaEl,
    spacerHeight,
    offsetY,
    visibleRows,
    startIndex,
    firstVisibleIndex,
    stickyRows,
    rowH,
    stackTopPx,
    stackBottomPx,
    treeOffsetTop,
    scheduleUpdate,
    updateWindow,
    scrollTreeToNodeKey,
    getLastScrollTs,
  }
}
