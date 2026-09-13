// 模态对话框焦点陷阱(Focus Trap)。
// 职责:激活时把焦点移入容器(优先 [data-autofocus]),Tab/Shift+Tab 在容器内循环,
// 关闭时把焦点归还给打开前的元素——否则键盘用户关闭对话框后焦点丢回 body。
// 设计取舍:自研 ~70 行而非引入 focus-trap 库(依赖节制;需求仅「循环 + 归还」两条)。
// 纯逻辑(循环索引计算)抽为 nextTrapIndex 以便 node 环境单测;DOM 接线部分无 jsdom,
// 由手测验证(见 R1-8 验收:纯键盘完成删除确认全流程)。

import { watch, nextTick, onBeforeUnmount, type Ref } from 'vue'

/** 可聚焦元素选择器(禁用态排除;tabindex=-1 排除,容器自身除外)。 */
const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

/**
 * Tab 循环的下一个焦点索引(纯函数,便于单测)。
 * @param count   可聚焦元素总数(>0)
 * @param current 当前焦点所在索引;-1 表示焦点不在列表内(如落在容器上)
 * @param shift   是否 Shift+Tab(反向)
 */
export function nextTrapIndex(count: number, current: number, shift: boolean): number {
  if (current === -1) return shift ? count - 1 : 0
  // 环形步进:首尾相接,保证焦点永不逃出容器
  return shift ? (current - 1 + count) % count : (current + 1) % count
}

/**
 * 焦点陷阱。container 为对话框根元素 ref(v-if 存在性切换),active 为开阖状态取值器。
 * 激活 → 记录当前焦点 → nextTick 后聚焦 [data-autofocus] 或首个可聚焦元素或容器自身;
 * 关闭 → 焦点归还(原元素仍在文档中时)。
 *
 * 兼容两种挂载范式:
 *   ① 恒挂载 + open 翻转(如 ConfirmDialog):挂载时 active=false,open 后 false→true 触发 engage;
 *   ② 父 v-if「挂载即开」(如 FolderCreateDialog):挂载时 active 已为 true,无 false→true 跳变。
 * 故 watch 用 immediate 覆盖范式②(挂载即 engage);对范式①挂载时 active=false,immediate 回调
 * 两分支皆不命中,行为不变。范式②在 open=true 时被父卸载,watch 见不到 true→false,焦点归还
 * 改由 onBeforeUnmount 直接 release 兜底(对范式①正常关闭已归还并置空,此处为幂等空操作)。
 */
export function useFocusTrap(container: Ref<HTMLElement | null>, active: () => boolean) {
  let previouslyFocused: HTMLElement | null = null

  function focusables(): HTMLElement[] {
    const root = container.value
    if (!root) return []
    return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab') return
    const items = focusables()
    const root = container.value
    if (!root) return
    if (items.length === 0) {
      // 容器内无可聚焦元素:焦点钉在容器上,阻止 Tab 逃逸
      e.preventDefault()
      root.focus()
      return
    }
    const current = items.indexOf(document.activeElement as HTMLElement)
    e.preventDefault()
    items[nextTrapIndex(items.length, current, e.shiftKey)]!.focus()
  }

  function engage() {
    // SSR / 非浏览器环境无 DOM:焦点陷阱是纯客户端关注点。immediate watch 会在 SSR setup 期
    // (open=true 时)也触发本函数,若不守卫会同步触碰 document 抛 ReferenceError——直接空操作。
    if (typeof document === 'undefined') return
    previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null
    // nextTick:等 v-if 渲染出容器后再聚焦
    void nextTick(() => {
      const root = container.value
      if (!root) return
      root.addEventListener('keydown', onKeydown)
      const target = root.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0] ?? root
      target.focus()
    })
  }

  function release() {
    container.value?.removeEventListener('keydown', onKeydown)
    // isConnected 守卫:原焦点元素可能已随视图销毁(如从被删卡片打开的确认框)
    if (previouslyFocused?.isConnected) previouslyFocused.focus()
    previouslyFocused = null
  }

  watch(
    active,
    (isActive, wasActive) => {
      if (isActive && !wasActive) engage()
      else if (!isActive && wasActive) release()
    },
    { immediate: true },
  )

  // release 内含移除监听 + 焦点归还:覆盖「挂载即开」对话框在 open 态被卸载(watch 见不到关闭)的场景。
  onBeforeUnmount(release)
}
