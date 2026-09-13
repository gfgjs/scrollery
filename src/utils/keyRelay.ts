// src/utils/keyRelay.ts
// 把 iframe 内 document 的键盘事件中继到父文档。
//
// 为什么需要它(2026-07-16 真机):EPUB 由 foliate-js 渲染在**真 iframe** 里
// (vendor/foliate-js/paginator.js:213)。iframe 是独立的浏览上下文——**键盘事件在它自己的 document
// 里派发,不跨界冒泡到父窗口**。捕获阶段也救不了:capture 只沿同一 document 的父链走。
// 于是父页所有 window 级键位在「焦点在书里」时全部失灵:
//   · useFullscreenExitGuard 的按住 Esc 退全屏(真机报的那条);
//   · usePager 的方向键/空格翻页(usePager.ts 挂 window);
//   · AppShell 的 F11、命令注册表分发。
// 雪上加霜:paginator.js:1118 的 focusView() 每次翻页把焦点**再推回** iframe——焦点是粘的,自己回不来。
//
// 边界是 iframe 的**产品**而非缺陷(EPUB 是不可信第三方 HTML/CSS/JS,且其 CSS 的 vh/vw、多列分页、
// 竖排都要真视口),故正解不是逐个键打补丁,而是建这一座桥:**每个全局键位都从此自动可达**。
//
// 安全:iframe 与父页同源(章节文档走 blob: URL,继承创建者的源),故父页可直接 addEventListener 到
// contentDocument,无需 postMessage 之类的跨源协议。

/** 中继到父页时需要保真的按键字段。合成事件不复制 target(它必然是 iframe 内的元素,父页够不到)。 */
const RELAYED_INIT_KEYS = [
  'key',
  'code',
  'location',
  'ctrlKey',
  'shiftKey',
  'altKey',
  'metaKey',
  'repeat',
  'isComposing',
] as const

/**
 * 命中目标是否是可编辑控件。**必须在源侧(iframe 内)判**——父页自己的守卫
 * (usePager.ts 的 e.target 判定、AppShell 的 inEditable)读的是合成事件的 target,
 * 那是我们派发时指定的宿主元素,它们**看不进** iframe。若不在此拦,书里的输入框(测验/笔记类 EPUB)
 * 打字时方向键会翻页、空格会翻页。
 */
function isEditableTarget(el: Element | null): boolean {
  if (!el) return false
  const tag = el.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  return (el as HTMLElement).isContentEditable === true
}

/**
 * 让 `doc`(iframe 内文档)的 keydown/keyup 在父文档上重新派发。
 *
 * keyup 同样要中继:按住 Esc 退全屏的守卫靠 keyup 取消未达阈值的按住(useFullscreenExitGuard),
 * 只中继 keydown 会让进度条填满后再也退不回来。
 *
 * 回传抑制:父页若消费了该键(preventDefault),则在 iframe 内一并 preventDefault——否则空格会
 * 「既翻页又滚动 iframe」。dispatchEvent 返回 false 即表示被 preventDefault。
 *
 * @param doc iframe 内的文档(foliate 'load' 事件的 detail.doc)
 * @param target 父文档中用于派发的宿主元素。用真实元素而非 window:事件路径由此含其全部祖先,
 *   window / document / 元素级监听一并可达(在 window 上派发则路径只有 window 自己,
 *   document 级监听——如 AppToolbar 的注册表分发——收不到)。
 * @returns 摘除中继的函数
 */
export function relayKeyboardEvents(doc: Document, target: HTMLElement): () => void {
  const relay = (e: Event) => {
    const src = e as KeyboardEvent
    // 书内输入控件:让它自己吃键(理由见 isEditableTarget)。
    if (isEditableTarget(doc.activeElement)) return
    // 已被 iframe 内其它处理器消费 → 不再转给父页,避免双执行。
    if (src.defaultPrevented) return

    const init: KeyboardEventInit = { bubbles: true, cancelable: true }
    for (const k of RELAYED_INIT_KEYS) {
      // 逐字段拷贝而非展开整个事件对象:KeyboardEvent 的属性在原型上,展开取不到。
      ;(init as Record<string, unknown>)[k] = (src as unknown as Record<string, unknown>)[k]
    }
    const consumed = !target.dispatchEvent(new KeyboardEvent(src.type, init))
    if (consumed) src.preventDefault()
  }

  // capture:书内脚本(EPUB 可带 JS)若在冒泡期 stopPropagation,冒泡期中继就收不到。
  doc.addEventListener('keydown', relay, true)
  doc.addEventListener('keyup', relay, true)
  return () => {
    doc.removeEventListener('keydown', relay, true)
    doc.removeEventListener('keyup', relay, true)
  }
}

/**
 * 把 iframe 内的**指针移动**上报给父页(连同该 iframe 在父页视口系里的 top 偏移)。
 *
 * 为什么不像键盘那样中继整个事件:指针事件 60–125Hz,合成并派发到父页会让 useWindowDrag /
 * useSelection / usePointerDrag 等一并收到**假的**指针流(它们据此起拖、框选),爆炸半径远大于收益。
 * 唯一需要跨界的消费者是「贴顶唤出顶栏」。故把**原始事件 + 偏移**交给它自己解读,不伪造事件。
 * (传原始事件而非单个 y:消费者要读 getCoalescedEvents 才能看到本帧被合并掉的中间采样点,
 *  见 useChromeReveal.sampleFrom——该判据本身正确,但「快速甩到顶不出顶栏」的真根因另有其人,
 *  见 useWindowMode.ts 的 setResizable(false) 修法,777824d。)
 *
 * 偏移是必须的:iframe 内的 clientY 相对**它自己的**视口。阅读页 flow="scrolled" 时 iframe
 * 顶缘≈父页 y=0(故两者近似相等),但 flow="paginated" 时 foliate 预留 48px header 带、iframe 顶缘
 * 在 y=48 —— 不换算则书里 y=0 会被当成父页 y=0,凭空唤出顶栏。
 *
 * @param doc iframe 内的文档(foliate 'load' 事件的 detail.doc)
 * @param report 接收原始 pointermove 与该 iframe 的 rect.top
 * @returns 摘除监听的函数
 */
export function relayPointerMoves(
  doc: Document,
  report: (e: PointerEvent, frameTop: number) => void,
): () => void {
  const onMove = (e: Event) => {
    // frameElement 同源可达(章节走 blob: URL,继承父页的源);拿不到就放弃换算而非猜。
    const frame = doc.defaultView?.frameElement as HTMLElement | null | undefined
    if (!frame) return
    report(e as PointerEvent, frame.getBoundingClientRect().top)
  }
  doc.addEventListener('pointermove', onMove, true)
  // pointerrawupdate 同样要接:采样密度高于 pointermove,父页侧同款理由(见 useChromeReveal.attach
  // 的长注,已更正)。iframe 的 defaultView 才是它自己的事件目标宿主,故特性探测查它而非父页 window。
  const frameWin = doc.defaultView
  const hasRaw = !!frameWin && 'onpointerrawupdate' in frameWin
  if (hasRaw) doc.addEventListener('pointerrawupdate', onMove, true)
  return () => {
    doc.removeEventListener('pointermove', onMove, true)
    if (hasRaw) doc.removeEventListener('pointerrawupdate', onMove, true)
  }
}
