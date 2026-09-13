// src/composables/useRovingTabindex.ts
// L5 roving tabindex(工具栏键盘导航,设计 §6.3 / §S3)。使一组同质控件(工具栏图标按钮)
// 在 Tab 序里**只占一个停靠位**:活动项 tabindex=0、其余 -1,方向键在组内移动焦点。此前 ContextualToolbar
// 每个按钮都是独立 Tab stop(审计 §「Toolbar 无 roving tabindex」),键盘用户须逐个 Tab 穿过整条工具栏。
//
// 核心 = 纯函数 nextRovingIndex(可穷举单测:方向键→下一焦点索引,跳过 disabled,环绕);composable 只做
// DOM 焦点管理(属 ⏸GUI,需真机布局/焦点)。DOM 测量/聚焦不入纯核,与 useToolbarOverflow 同构。

import { ref, watch, onMounted, nextTick, type Ref } from 'vue'

export type RovingOrientation = 'horizontal' | 'vertical' | 'both'

/**
 * 从 `from` 起沿 `dir`(±1)方向找首个 enabled 项索引,环绕遍历全部 N 项(末步回到 `from` 本身)。
 * 全不可用→null;仅 `from` 可用→返回 `from`(焦点不动,无害)。
 */
function seekEnabled(from: number, dir: 1 | -1, enabled: boolean[]): number | null {
  const n = enabled.length
  for (let i = 1; i <= n; i++) {
    const idx = (((from + dir * i) % n) + n) % n
    if (enabled[idx]) return idx
  }
  return null
}

/** 从一端找首个 enabled 项:dir=+1 取最前(Home),dir=-1 取最后(End)。全不可用→null。 */
function edgeEnabled(dir: 1 | -1, enabled: boolean[]): number | null {
  const n = enabled.length
  if (dir === 1) {
    for (let i = 0; i < n; i++) if (enabled[i]) return i
  } else {
    for (let i = n - 1; i >= 0; i--) if (enabled[i]) return i
  }
  return null
}

/**
 * roving tabindex 的纯核:给定按键、当前索引、各项可用性与朝向,返回**下一应聚焦项**的索引
 * (跳过 disabled 项、到端点环绕);按键与朝向不匹配或无可聚焦项时返回 null(调用方据此不 preventDefault)。
 *
 * 朝向:horizontal 只响应 ArrowLeft/Right,vertical 只响应 ArrowUp/Down,both 全响应;Home/End 恒响应。
 *
 * @param key    KeyboardEvent.key
 * @param current 当前焦点项索引
 * @param enabled 各项是否可聚焦(disabled 项为 false),按显示顺序
 * @param orientation 组的朝向;缺省 horizontal
 */
export function nextRovingIndex(
  key: string,
  current: number,
  enabled: boolean[],
  orientation: RovingOrientation = 'horizontal',
): number | null {
  if (enabled.length === 0) return null
  const horiz = orientation !== 'vertical'
  const vert = orientation !== 'horizontal'
  switch (key) {
    case 'ArrowRight':
      return horiz ? seekEnabled(current, 1, enabled) : null
    case 'ArrowLeft':
      return horiz ? seekEnabled(current, -1, enabled) : null
    case 'ArrowDown':
      return vert ? seekEnabled(current, 1, enabled) : null
    case 'ArrowUp':
      return vert ? seekEnabled(current, -1, enabled) : null
    case 'Home':
      return edgeEnabled(1, enabled)
    case 'End':
      return edgeEnabled(-1, enabled)
    default:
      return null
  }
}

export interface UseRovingTabindexOptions {
  /** 组容器;其内匹配 `itemSelector` 的元素即 roving 成员(按 DOM 顺序)。 */
  containerRef: Ref<HTMLElement | null>
  /** 成员选择器;缺省 `[data-toolbar-item]`(与 useToolbarOverflow 复用同一标记)。 */
  itemSelector?: string
  /** 组朝向;缺省 horizontal(工具栏)。 */
  orientation?: RovingOrientation
  /** 成员集变化(命令增删/禁用态翻转)时改它 → 校正 tab 停靠位,防其落在已移除/disabled 项。 */
  remeasureKey?: Ref<unknown>
}

/**
 * roving tabindex composable。返回 `tabindexFor(i)`(模板逐项绑定)、`onKeydown`(容器 keydown)、
 * `onFocusin`(容器 focusin,指针/Tab 直接聚焦某项时同步停靠位)与 `refresh`(手动校正)。
 *
 * disabled 判定读元素 `disabled`;disabled button 本不可聚焦,故 tab 停靠位
 * 必须避开它(否则整条工具栏无 Tab 落点)——refresh 在挂载与 remeasureKey 变化时做此校正。
 */
export function useRovingTabindex(opts: UseRovingTabindexOptions) {
  const activeIndex = ref(0)
  const selector = opts.itemSelector ?? '[data-toolbar-item]'
  const orientation = opts.orientation ?? 'horizontal'

  function items(): HTMLElement[] {
    const el = opts.containerRef.value
    return el ? Array.from(el.querySelectorAll<HTMLElement>(selector)) : []
  }

  function enabledMask(list: HTMLElement[]): boolean[] {
    return list.map((it) => !it.hasAttribute('disabled'))
  }

  /** 活动项 tabindex=0,其余 -1 → 全组只占一个 Tab 停靠位。 */
  function tabindexFor(index: number): 0 | -1 {
    return index === activeIndex.value ? 0 : -1
  }

  /** 校正 activeIndex 指向首个可聚焦项(现值仍有效则不动);防停靠位落在已移除/disabled 项。 */
  function refresh(): void {
    const list = items()
    if (!list.length) {
      activeIndex.value = 0
      return
    }
    const mask = enabledMask(list)
    if (activeIndex.value < list.length && mask[activeIndex.value]) return
    const first = mask.findIndex(Boolean)
    activeIndex.value = first >= 0 ? first : 0
  }

  function onKeydown(e: KeyboardEvent): void {
    const list = items()
    if (!list.length) return
    // 以真实焦点项为起点(指针可能刚点了非停靠项);焦点不在组内则退回 activeIndex。
    const focusedIdx = list.indexOf(document.activeElement as HTMLElement)
    const from = focusedIdx >= 0 ? focusedIdx : activeIndex.value
    const next = nextRovingIndex(e.key, from, enabledMask(list), orientation)
    if (next == null) return
    e.preventDefault() // 防方向键滚动容器
    activeIndex.value = next
    list[next]?.focus()
  }

  /** 指针/Tab 直接聚焦某项时同步停靠位,使其成为后续 Shift+Tab 离开再回来的落点。 */
  function onFocusin(e: FocusEvent): void {
    const list = items()
    const idx = list.indexOf(e.target as HTMLElement)
    if (idx >= 0) activeIndex.value = idx
  }

  onMounted(() => {
    void nextTick().then(refresh)
  })
  if (opts.remeasureKey) {
    watch(opts.remeasureKey, () => {
      void nextTick().then(refresh)
    })
  }

  return { activeIndex, tabindexFor, onKeydown, onFocusin, refresh }
}
