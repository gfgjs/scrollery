// src/composables/useSidebarSections.ts
// 侧边栏 VSCode 风格手风琴控制器（provide/inject）。
//
// 职责:
//  1. 每个区块的展开/折叠状态，持久化到设置以在重启后保留。折叠仅隐藏
//     主体（调用方用 v-show），因此文件夹树等嵌套展开状态得以保留——「多级展开状态记忆」。
//  2. 粘性标题堆叠计算：每个可见区块标题登记其 order；从当前可见区块的排序
//     列表推导出各标题的 index/total，使其能粘顶与粘底（堆叠），且条件区块
//     （如「管理」）不会在偏移中留下空档。
//
// 存储（设置集中保存，批次B）：键 sidebar_sections_expanded 纳入注册设置键（此前经 GET/SET_APP_CONFIG
// 直连且未注册，写入被后端拒绝），读经 readSetting、写经 writeSettings，快照与重置自动生效。

import { inject, provide, reactive, computed, type ComputedRef, type InjectionKey } from 'vue'
import { readSetting, settingsReady, writeSettings } from '../stores/settingsPersistence'
import { parseSettingJson } from './settingsValues'

/** 保存展开状态映射的设置键 */
const SETTING_KEY = 'sidebar_sections_expanded'

/** 吸顶判定所需的一组元素：同一区块内、同一父级下的相邻兄弟。 */
export interface SidebarStickyEls {
  /** 分组标题（position: sticky）。 */
  header: HTMLElement
  /** 零高文档流标记（非 sticky）：其 rect.top 即标题的文档流位置。 */
  marker: HTMLElement
  /** 区块主体：只作「尺寸变了」的信号（ResizeObserver），不参与几何判定。 */
  body: HTMLElement
}

export interface SidebarSectionsApi {
  /** 区块 id 是否展开（未知时默认展开）。 */
  isExpanded: (id: string) => boolean
  /** 切换区块 id 并持久化。 */
  toggle: (id: string) => void
  /** 登记一个已可见的区块（挂载时调用）。 */
  register: (id: string, order: number) => void
  /** 登记吸顶判定所需的元素：控制器据此判定「是否被 sticky 顶住」并切换 .is-pinned（挂载时调用）。 */
  registerSticky: (id: string, els: SidebarStickyEls) => void
  /** 注销区块（卸载时调用）。 */
  unregister: (id: string) => void
  /** 当前可见区块的 id，按 order 排序。 */
  visibleIds: ComputedRef<string[]>
}

const KEY: InjectionKey<SidebarSectionsApi> = Symbol('sidebar-sections')

/** 读展开映射（规范 JSON 文本 → 表）；缺键或非法文本一律空表。 */
function readExpanded(): Record<string, boolean> {
  return parseSettingJson<Record<string, boolean>>(readSetting(SETTING_KEY), {})
}

/**
 * 创建控制器并向后代区块组件提供。仅在侧边栏容器的 setup() 中调用一次。
 */
export function provideSidebarSections(): SidebarSectionsApi {
  // sectionId -> order，仅包含当前已挂载/可见的区块。
  const registered = reactive<Record<string, number>>({})

  function isExpanded(id: string): boolean {
    // 缺失的键表示「使用默认值（true）」。
    return readExpanded()[id] !== false
  }

  function toggle(id: string) {
    const next = readExpanded()
    next[id] = !isExpanded(id)
    // 折展会改变各标题的文档流位置，重新判定吸顶态。
    schedulePinned()
    // 权威快照未到达前读到的是空表,此刻提交会把其他区块的展开态一并抹掉;故未就绪不提交。
    if (!settingsReady.value) return
    // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
    writeSettings({ [SETTING_KEY]: JSON.stringify(next) }).catch(() => {})
  }

  function register(id: string, order: number) {
    registered[id] = order
  }

  // ── 吸顶判定（玻璃模式的分组标题材质开关）────────────────────────────────────
  // 分组标题是 position: sticky：滚动时它会盖住从下方穿过的行，那一刻需要一层局部衬底；
  // 静止未遮挡时必须完全无材质，否则共享玻璃上会重新显影成横条。判据只有一个——标题是否
  // 被 sticky 挪离了它在文档流中的位置（顶部顶住、底部堆叠都算）。
  // 文档流位置取自区块自带的零高标记（AccordionSection 的 .acc-flow-marker，非 sticky）：
  // 它恒在文档流原位，与标题相邻同父，两者 rect.top 之差就是 sticky 位移。不用标题自身的
  // offsetTop——sticky 位移是否计入 offsetTop 各引擎不一致（Chromium 会反映吸附后的位置），
  // 拿它当基准等于自比。
  // 整侧栏只挂一个 passive scroll 监听 + 一次 rAF 合并，四个标题共用；.is-pinned 在非玻璃
  // 模式没有对应样式，不改变其它语义。
  // 标题与主体高度变化（折叠动画、文件夹树异步展开/收起）同样会挪动下方标题的文档流位置，
  // 故一并观察主体：ResizeObserver 回调与 scroll 共用同一次 rAF 复判。
  // 折叠中的主体是 display:none、rect 恒为 0——它只被当信号用，不参与几何，故折叠瞬间不会
  // 把自己误判成钉住。标记与标题恒在同一父级，几何基准必然同源。
  const stickyEls = new Map<string, SidebarStickyEls>()
  let scrollerEl: HTMLElement | null = null
  let pinRaf = 0
  const bodySizes: ResizeObserver | null =
    typeof ResizeObserver === 'function' ? new ResizeObserver(() => schedulePinned()) : null

  function updatePinned() {
    pinRaf = 0
    for (const { header, marker } of stickyEls.values()) {
      // 差值 > 0.5px（亚像素容差）即 sticky 位移；marker 与 header 同父相邻，基准同源。
      const moved = Math.abs(header.getBoundingClientRect().top - marker.getBoundingClientRect().top)
      header.classList.toggle('is-pinned', moved > 0.5)
    }
  }

  function schedulePinned() {
    if (pinRaf || typeof requestAnimationFrame !== 'function') return
    pinRaf = requestAnimationFrame(updatePinned)
  }

  function detachPinned() {
    if (!scrollerEl) return
    scrollerEl.removeEventListener('scroll', schedulePinned)
    window.removeEventListener('resize', schedulePinned)
    bodySizes?.unobserve(scrollerEl)
    // 待执行的复判一并取消：区块全部卸载后再跑一次只会去读已解绑的元素。
    if (pinRaf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(pinRaf)
    pinRaf = 0
    scrollerEl = null
  }

  function registerSticky(id: string, els: SidebarStickyEls) {
    stickyEls.set(id, els)
    bodySizes?.observe(els.body)
    // 滚动容器取自标题自身（侧栏结构唯一），不必额外传参。
    const scroller = els.header.closest('.sidebar__scroll-area') as HTMLElement | null
    if (scroller && scroller !== scrollerEl) {
      detachPinned()
      scrollerEl = scroller
      scroller.addEventListener('scroll', schedulePinned, { passive: true })
      window.addEventListener('resize', schedulePinned)
      // 滚动区自身高度变化（窗口高度、页脚高度）不必然派发 window.resize，直接观察更稳。
      bodySizes?.observe(scroller)
    }
    schedulePinned()
  }

  function unregister(id: string) {
    delete registered[id]
    const els = stickyEls.get(id)
    if (!els) return
    els.header.classList.remove('is-pinned')
    bodySizes?.unobserve(els.body)
    stickyEls.delete(id)
    // 区块卸载同样挪动其余标题的文档流位置，复判一次；最后一个区块走完则整体解绑。
    if (stickyEls.size === 0) detachPinned()
    else schedulePinned()
  }

  const visibleIds = computed(() =>
    Object.keys(registered).sort((a, b) => registered[a] - registered[b]),
  )

  const api: SidebarSectionsApi = {
    isExpanded,
    toggle,
    register,
    registerSticky,
    unregister,
    visibleIds,
  }
  provide(KEY, api)
  return api
}

/** 在区块组件中注入控制器。 */
export function useSidebarSections(): SidebarSectionsApi {
  const api = inject(KEY)
  if (!api) {
    throw new Error(
      '[useSidebarSections] must be used inside a sidebar that called provideSidebarSections()',
    )
  }
  return api
}
