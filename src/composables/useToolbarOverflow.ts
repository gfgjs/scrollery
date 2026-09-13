// src/composables/useToolbarOverflow.ts
// L5 响应式溢出收纳(Priority+,顶栏重构 §3.6)。容器放不下时,低优先命令按显示顺序从尾部移入
// ⋯ 溢出菜单;变宽时逐个放回。
//
// 核心 = 纯函数 computeOverflowSplit(可单测);composable 用 ResizeObserver 观察容器宽 + 缓存各项
// offsetWidth——**仅在 observer/remeasureKey 触发时重测**,避免每帧读宽触发强制同步布局(layout thrash)。
// 缓存失效时机(§3.6):命令集变化 / locale 切换 / 字号·字体变化 → 经 remeasureKey 重测。

import {
  ref,
  shallowRef,
  watch,
  onMounted,
  onBeforeUnmount,
  nextTick,
  type Ref,
} from 'vue'

export interface OverflowSplit {
  /** 前 visibleCount 项可见,其余进溢出。 */
  visibleCount: number
  /** ⋯ 溢出按钮是否显示。 */
  hasOverflow: boolean
}

/**
 * Priority+ 纯核:按显示顺序(高优先在前)的项宽数组,在给定可用宽内贪心从头塞。全放得下→无溢出;
 * 否则预留 ⋯ 按钮宽,塞不下的进溢出。纯函数,与 DOM 无关,可单测。
 *
 * `gap`:flex 项间距(px)。offsetWidth 不含 flex gap / margin,不校正会低估总宽致末项被裁。
 * 计入项间 (N−1) 个 gap, 溢出时再为 ⋯ 按钮前留一个 gap。默认 0 → 与无间距容器/旧行为一致。
 *
 * @param widths 各项宽(显示顺序,px)
 * @param available 可用宽(容器内容宽 − 预留区如窗口三键,已扣容器 padding,px)
 * @param overflowButtonWidth ⋯ 按钮宽(仅需溢出时计入)
 * @param gap 项间距(px);缺省 0
 */
export function computeOverflowSplit(
  widths: number[],
  available: number,
  overflowButtonWidth: number,
  gap = 0,
): OverflowSplit {
  if (widths.length === 0) return { visibleCount: 0, hasOverflow: false }
  // 总宽含项间 (N−1) 个 gap。
  const total = widths.reduce((s, w) => s + w, 0) + gap * (widths.length - 1)
  // 全放得下(含不需 ⋯ 按钮):全可见。
  if (total <= available) return { visibleCount: widths.length, hasOverflow: false }
  // 需溢出:预留 ⋯ 按钮宽 + 其前一个 gap 后, 从头贪心塞(每个非首项前加一个 gap)。
  const budget = available - overflowButtonWidth - gap
  let used = 0
  let visible = 0
  for (const w of widths) {
    const add = visible === 0 ? w : gap + w
    if (used + add <= budget) {
      used += add
      visible++
    } else break
  }
  return { visibleCount: visible, hasOverflow: true }
}

type ReservedWidth = Ref<number> | (() => number) | number

export interface UseToolbarOverflowOptions {
  /** 工具栏项容器;其内带 `data-toolbar-item` 的元素即测量对象。 */
  containerRef: Ref<HTMLElement | null>
  /** ⋯ 溢出按钮宽(CSS 已知);缺省 40。 */
  overflowButtonWidth?: number
  /** 右侧预留宽(窗口三键等,占容器宽但不参与项流)。缺省 0。 */
  reservedWidth?: ReservedWidth
  /** 重测触发键:命令集/locale/字号变化时改它 → 重测各项宽。 */
  remeasureKey?: Ref<unknown>
  /**
   * 容器宽度是否恒等于可用宽:
   * - true(默认):容器 flex:1 填满可用宽(如 AppToolbar 的 .toolbar__foldable),clientWidth 即可用宽,
   *   用 ResizeObserver 观察容器 + 廉价 recompute(变宽/变窄都能被观察到)。
   * - false:容器是**内容宽**(如浮动胶囊内 flow,flex:0 1 auto,宽随折叠自缩;一旦折叠就再不因父级变宽而
   *   变宽 → 自身尺寸不变 → RO 观察不到「有空间可展开」→ 折叠自锁棘轮)。改为监听 window resize → 完整
   *   measure(测量帧全渲染时,容器被父级 max-width 钳到的 clientWidth = 真可用宽,是内容宽容器唯一能测到
   *   可用宽的时机)。**若提供 measureAvailable 探针,则 resize 改走廉价重算(见下),不再每帧全展开。**
   */
  containerFillsWidth?: boolean
  /**
   * 内容宽容器(containerFillsWidth:false)专用的**可用宽探针**:返回当前可用宽(px)。提供后,window
   * resize 只调探针 + applySplit(用缓存项宽廉价重算切分), **不再全展开各项**——从而折叠动画不被「量尺帧
   * 展开→折回」污染(那会让折叠项每帧闪动)。项宽仍在 mount/remeasureKey 时经全量 measure 缓存(那两处稀有)。
   * 典型实现:给内容宽 flow 容器临时 flex-grow 使其填满可用宽,同步读 clientWidth 后立即还原(全程一次
   * microtask 内、不经 paint;只动容器自身 flex 而非折叠项 grid 宽 → 不触发项过渡)。缺省=旧全量 measure 路径。
   */
  measureAvailable?: () => number
  /**
   * 暂停重测的临时态(如锚定弹层开启)。为 true 期间, remeasureKey 变化只置 dirty 标志、不立即
   * measure()——量尺帧会让内联项瞬时全展开(measure 里读 offsetWidth 强制 reflow, 把该中间态具现给
   * 浏览器), 关测量帧后带过渡收回; 若此时有锚定弹层正 autoUpdate 跟随折叠项旁的 ⋯ 锚点按钮, 锚点
   * 漂移会让弹层横跳。改为等 deferWhile 落回 false 时若 dirty 再补测一次(弹层已关, 锚点不再被跟随,
   * 收回无副作用)。缺省不暂停。
   */
  deferWhile?: Ref<boolean>
}

/**
 * 溢出收纳 composable。返回 `visibleCount`/`hasOverflow`(响应式)+ `isMeasuring`(测量帧标志,
 * 消费方据此临时全渲染以便测得各项自然宽)+ `isSettling`(折叠过渡抑制标志,消费方据此在内容驱动的重测
 * 期间给 fold-item transition:none, 使折叠瞬时落定不走「展再收」动画)+ `remeasure`(手动重测)。
 *
 * DOM 测量属 ⏸GUI(需真机布局);纯切分逻辑见 computeOverflowSplit(已单测)。
 */
export function useToolbarOverflow(opts: UseToolbarOverflowOptions) {
  const visibleCount = ref(0)
  const hasOverflow = ref(false)
  // 仅**首测前**的初始展开标志:true 时消费方渲染全部项(否则未测量前 visibleCount=0 会把项全折成空)。
  // measure() 完成即置 false 且此后恒 false——运行期的量尺展开改走 measure() 内的**同步命令式加类**
  // (见其注释),不再响应式 show-all(那会绘制出「顶栏先全展开再收起」的闪动)。
  const isMeasuring = ref(true)
  // 折叠过渡抑制标志(消「展再收」的第二层)。**不变量**:.fold-item 的 0.28s 折叠过渡只服务「窗口 resize
  // 平滑折叠」;**内容驱动的重测**(命令集/chip 增减/组内排序 select 显隐/locale 换文案)引发的折叠须**瞬时
  // 落定**、不走过渡——否则折叠边界跳变被逐项动画成「顶栏展开一截再收回」。measure() 在内容重测末尾置此为 true,
  // 令消费方 CSS 给 fold-item transition:none, applySplit 的折叠瞬时完成; 提交样式基线后再置回 false 恢复过渡
  // (见 measure() 尾段)。窗口 resize 走 recompute/探针路径不经 measure() → 过渡照常, 平滑折叠不受影响。
  const isSettling = ref(false)
  // 缓存各项宽(shallowRef 承数组,遵项目前端规范);仅重测时更新。
  const widthCache = shallowRef<number[]>([])

  const overflowBtnW = opts.overflowButtonWidth ?? 40
  // 容器盒模型缓存:gap(项间距)+ 水平 padding。二者都不在项的 offsetWidth 里, 需从容器读并校正,
  // 否则可用宽/总宽失真致末项被裁。随各项宽在 measure() 时一并缓存(布局稳定期间不变)。
  let gapCache = 0
  let paddingXCache = 0

  function readReserved(): number {
    const r = opts.reservedWidth
    if (r == null) return 0
    if (typeof r === 'number') return r
    return typeof r === 'function' ? r() : r.value
  }

  // 从缓存宽 + 给定可用宽算切分并写响应式结果(纯计算,不触 DOM)。
  function applySplit(available: number) {
    const split = computeOverflowSplit(widthCache.value, available, overflowBtnW, gapCache)
    visibleCount.value = split.visibleCount
    hasOverflow.value = split.hasOverflow
  }

  // 从当前容器 clientWidth 算切分(不重测各项,避免 layout thrash)。仅对 flex:1 全宽容器可靠——其
  // clientWidth 恒等于可用宽;内容宽容器折叠后 clientWidth 会塌回内容宽,故那类容器改走 measure(见下)。
  function recompute() {
    const el = opts.containerRef.value
    if (!el) return
    // clientWidth 含 padding, 故先扣 padding 得内容宽, 再扣消费方声明的预留区。
    applySplit(el.clientWidth - paddingXCache - readReserved())
  }

  // applySplit 的**折叠瞬时落定**版(消「展再收」的统一执行器)。置 isSettling → 消费方 CSS 令 fold-item
  // transition:none → applySplit 触发的折叠瞬时完成 → 等折叠类经 nextTick 落 DOM → 强制一次同步 reflow
  // (getBoundingClientRect)把「已折叠 + 无过渡」提交为浏览器样式基线 → 再置回 false(grid 值不变、仅过渡
  // 属性 none→0.28s 无数值变化 → 不触发动画)。全程在 microtask 内无中途 paint。**不变量**:唯有真·窗口
  // resize 才让折叠走 0.28s 动画;内容驱动的重测(measure 尾段)与非窗口原因的容器宽变化(RO 判别后的
  // recomputeSettled)都走此路径瞬时落定。
  async function applySplitSettled(available: number) {
    isSettling.value = true
    applySplit(available)
    await nextTick()
    const el = opts.containerRef.value
    // 强制同步 reflow:提交「已折叠 + transition:none」为样式基线(见上)。可能在 await 期间卸载, 判空。
    if (el) el.getBoundingClientRect()
    isSettling.value = false
  }

  // recompute 的瞬时落定版:非窗口 resize 引发的容器宽变化(侧栏开关 / gallery scrollbar 增减等)走此
  // 路径 → 折叠瞬时、无动画(触发判别见 RO 回调)。B 方案落地后计数已不再撑动 foldable, 此路径主要兜住
  // 侧栏/scrollbar 一类「布局附带的宽度再分配」。
  function recomputeSettled() {
    const el = opts.containerRef.value
    if (!el) return
    applySplitSettled(el.clientWidth - paddingXCache - readReserved())
  }

  // 测量:等一帧让新增/移除项进入 DOM → **同步命令式**加 is-measuring 类(CSS 强制各项展开到自然宽)→
  // 读各项 offsetWidth + 容器盒模型 + 可用宽 → 立即去类 → 切分。可用宽取全展开时容器被父级 max-width 钳到的
  // clientWidth(对内容宽容器这是唯一能测到可用宽的时机)。全展开-读-去类在同一同步段内完成(不 await),
  // 浏览器不绘制中间展开态 → 无闪(见函数体内注释)。对 flex:1 全宽容器,clientWidth 恒等可用宽,行为一致。
  async function measure() {
    // 先等一帧,让本次触发新增/移除的项进入 DOM(如激活筛选新出现的「清除」chip),否则读不到它。
    await nextTick()
    const el = opts.containerRef.value
    if (!el) {
      isMeasuring.value = false
      return
    }
    // ── 同步命令式量尺(消闪核心)────────────────────────────────────────────────
    // 临时给容器加 MEASURING_CLASS:消费方 CSS(.is-measuring :deep(.fold-item))据此**强制所有 fold-item
    // 展开到自然宽**(含已折叠项,覆盖 --folded 的 0fr)+ 禁过渡。读毕**立即移除**——加类/读宽/移类全在
    // 本函数**同一同步段内、无 await**完成,浏览器只在同步段结束后才绘制,故绝不绘制这一瞬的展开态
    // (根除「顶栏先全展开再收起」的闪动)。对比旧法:旧法把 isMeasuring=true 做**响应式** show-all,那次
    // 渲染帧会被真实绘制 = 闪动根因。类名须与消费方容器 CSS 约定的 'is-measuring' 一致。
    const MEASURING_CLASS = 'is-measuring'
    el.classList.add(MEASURING_CLASS)
    const items = el.querySelectorAll<HTMLElement>('[data-toolbar-item]')
    // 项占位宽 = offsetWidth + 左右 margin。offsetWidth 不含 margin;当消费方用逐项 margin 作间距
    // (容器 gap:0, 见 G6 折叠动画——折叠项 margin:0 可随之收拢无碎屑)时, 必须计入 margin 才不低估总宽。
    // 用 gap 间距的容器 margin=0, 退化为纯 offsetWidth, 向后兼容。
    widthCache.value = Array.from(items, (it) => {
      const s = getComputedStyle(it)
      return it.offsetWidth + (parseFloat(s.marginLeft) || 0) + (parseFloat(s.marginRight) || 0)
    })
    const cs = getComputedStyle(el)
    gapCache = parseFloat(cs.columnGap) || parseFloat(cs.gap) || 0
    paddingXCache = (parseFloat(cs.paddingLeft) || 0) + (parseFloat(cs.paddingRight) || 0)
    const available = el.clientWidth - paddingXCache - readReserved()
    el.classList.remove(MEASURING_CLASS)
    // 首测后关初始展开:此后消费方按 visibleCount 折叠(运行期展开只走上面的命令式路径)。
    isMeasuring.value = false
    // 内容驱动的重测:折叠瞬时落定、不走 0.28s 过渡(那是真·窗口 resize 专用;内容变化走过渡=「展再收」)。
    // 统一执行器见 applySplitSettled(注释详述基线提交原理)。
    await applySplitSettled(available)
  }

  let ro: ResizeObserver | null = null
  let rafId = 0
  // 上次已知窗口宽:RO 回调据此判「真·窗口 resize(动画)」还是「兄弟元素宽变化(瞬时)」(见 onMounted RO 回调)。
  let lastWindowWidth = typeof window !== 'undefined' ? window.innerWidth : 0
  const fillsWidth = opts.containerFillsWidth ?? true

  // 内容宽容器专用触发:window resize → rAF 节流 → 完整 measure。为何不用 RO 观察容器自身:内容宽容器
  // 折叠后自身变窄=自反馈,再不因窗口变宽而变宽 → RO 收不到「有空间展开」的信号(真机症状:窄窗折叠后拉宽
  // 不回弹,须刷新)。window resize 是能覆盖「可用宽变化」的稳定信号。measure 的展开-读-收全在同一同步段内
  // 命令式完成(见 measure 注释),浏览器只 paint 最终态,无闪烁。
  function onWindowResize() {
    if (rafId) return
    rafId = requestAnimationFrame(() => {
      rafId = 0
      if (opts.measureAvailable) {
        // 有可用宽探针(Route A):resize 只按缓存项宽重算切分,不全展开各项 → 折叠动画干净无闪。
        // 探针自身负责临时填满读宽再还原(见 measureAvailable 注释)。
        applySplit(opts.measureAvailable() - paddingXCache - readReserved())
      } else {
        measure()
      }
    })
  }

  onMounted(() => {
    measure()
    const el = opts.containerRef.value
    if (fillsWidth) {
      if (el && typeof ResizeObserver !== 'undefined') {
        // 全宽容器:变宽/变窄仅重算切分(用缓存宽),不重测各项。RO 会因两类原因触发:①**真·窗口 resize**;
        // ②**兄弟元素宽变化**(标题旁计数文字变宽、侧栏开关、gallery scrollbar 增减 → flex:1 的 foldable 被
        // 重新分配宽度)。仅①应走 0.28s 平滑折叠动画;②是「布局附带的宽度再分配」,应瞬时落定,否则每次筛选
        // /切视图计数一变就动画一次折叠 =「闪展再收」。判据:比对 window.innerWidth——变了=真窗口 resize
        // (recompute 动画),没变=兄弟宽变化(recomputeSettled 瞬时)。统一「唯有真·窗口 resize 才动画」不变量。
        ro = new ResizeObserver(() => {
          const ww = typeof window !== 'undefined' ? window.innerWidth : lastWindowWidth
          if (ww !== lastWindowWidth) {
            lastWindowWidth = ww
            recompute()
          } else {
            recomputeSettled()
          }
        })
        ro.observe(el)
      }
    } else if (typeof window !== 'undefined') {
      window.addEventListener('resize', onWindowResize)
    }
  })

  onBeforeUnmount(() => {
    ro?.disconnect()
    if (typeof window !== 'undefined') window.removeEventListener('resize', onWindowResize)
    if (rafId) cancelAnimationFrame(rafId)
  })

  // 命令集/locale/字号变化 → 重测(缓存失效)。deferWhile 为真(如溢出弹层开启)时暂缓: 攒 dirty 标志,
  // 待其落回 false 再补测一次 —— 避免弹层开启期量尺帧闪动 + ⋯ 锚点漂移致弹层横跳(见 deferWhile 注释)。
  let pendingRemeasure = false
  if (opts.remeasureKey) {
    watch(opts.remeasureKey, () => {
      if (opts.deferWhile?.value) {
        pendingRemeasure = true
        return
      }
      measure()
    })
  }
  if (opts.deferWhile) {
    watch(opts.deferWhile, (deferring) => {
      if (!deferring && pendingRemeasure) {
        pendingRemeasure = false
        measure()
      }
    })
  }

  return { visibleCount, hasOverflow, isMeasuring, isSettling, remeasure: measure }
}
