// Canvas 网格悬停跟踪 + 单例悬停卡状态机(T1/T12),从 MediaGridCanvas.vue 下沉(方案 2.2 ⑦)。
// 工厂在宿主 <script setup> 顶层只调用一次;返回值名与 template 现有绑定逐一对应(模板零改动)。
import { ref, shallowRef, computed } from 'vue'
import type { Ref } from 'vue'
import { buildThumbUrl } from './useThumbLoader'
import { isThumbLoadDeferred } from './useThumbLoadGate'
import { computeHoverRect, type HoverRect } from '../components/media/mediaGridCanvas.helpers'
import type { LayoutRowItem } from '../types/layout'
import type { CanvasHitTest } from './useCanvasHitTest'

interface HoverCard {
  item: LayoutRowItem
  rect: HoverRect
}

/** 本 composable 需要上抛的三个格级事件,签名对齐 MediaGridCanvas 的 defineEmits 子集。 */
export interface CanvasHoverCardEmit {
  (event: 'cell-click', item: LayoutRowItem, e: MouseEvent): void
  (event: 'cell-contextmenu', item: LayoutRowItem, e: MouseEvent): void
  (event: 'cell-pointerdown', id: number, e: PointerEvent, onHandle: boolean): void
}

export interface CanvasHoverCardDeps {
  canvasRef: Ref<HTMLCanvasElement | null>
  scrolling: () => boolean
  isSelectionMode: () => boolean
  enableHoverScale: () => boolean
  /** 演示打码开关(2026-09-16):开启时悬停卡完全不可用(它渲染的是真实缩略图与真实文案)。 */
  demoPrivacy: () => boolean
  isPendingDelete: (id: number) => boolean
  isSelected: (id: number) => boolean
  cacheDir: () => string
  currentY: () => number
  selectionVersion: () => number
  viewport: () => { w: number; h: number }
  hitTest: Pick<CanvasHitTest, 'pickWithRow' | 'hitHandleAt'>
  /** 宿主 scheduleDraw:卡的显隐会翻转「画布镜像格」(styles --mirror / cellRenderer
   *  mirrorCellId),canvas 必须同帧重绘(弹卡撤 chrome、收卡还 chrome),否则该格
   *  chrome 悬空或与卡内 DOM 双重成像。 */
  requestRedraw: () => void
  emit: CanvasHoverCardEmit
}

export function useCanvasHoverCard(deps: CanvasHoverCardDeps) {
  const hoverLayerRef = ref<HTMLElement | null>(null)
  const hoverCard = shallowRef<HoverCard | null>(null)
  // 悬停卡的选中态:isSelected 是谓词非响应源,挂上 selectionVersion 依赖驱动重算。
  const hoverSelected = computed(() => {
    void deps.selectionVersion()
    return hoverCard.value ? deps.isSelected(hoverCard.value.item.id) : false
  })

  // 无缩略图格(视频/文档未生成 thumb):卡内占位/文本卡就是内容本身,150ms 延迟显形反而
  // 先透出 canvas 底再弹占位,多一次视觉台阶(真机反馈「中间插一次主题色」链的一环)——
  // 标记 --bare 让占位免延迟直接显形,放大即画格占位的延续。
  const hoverCardBare = computed(() =>
    hoverCard.value
      ? !buildThumbUrl(hoverCard.value.item.thumbStatus, hoverCard.value.item.thumbPath, deps.cacheDir())
      : false,
  )

  /**
   * 画布镜像态:未放大(scale0=1)且格有图源(非 --bare)。此时卡内不铺任何图像面,
   * 直接透出下方画布已绘的同一张位图——悬停卡若用 <img> 重新渲染这张图,是第二条解码
   * 管线(object-fit 浮点 cover 裁剪 + 布局盒设备像素吸附),与 canvas 管线(解码期整数化
   * 裁剪 + 桶高度预缩放 + 亚像素抗锯齿)必有亚像素相位/落点差,图片整帧替换瞬间即
   * 「移入轻微位移」(真机三轮反馈的残留根因)。透出画布 = 换源归零,像素逐位不变;
   * 该格 canvas 侧 chrome 同步停画(cellRenderer.mirrorCellId),交由卡内 DOM 重绘。
   * 放大态内容有意重缩,镜像无意义,仍走 <img>。
   */
  const hoverCardMirror = computed(
    () => !!hoverCard.value && !hoverCardBare.value && hoverCard.value.rect.scale0 === 1,
  )

  /** 供 drawCell 热路径判「该格 chrome 是否让位给悬停卡」:-1 = 无镜像格。 */
  function mirrorCellId(): number {
    const card = hoverCard.value
    return card !== null && !hoverCardBare.value && card.rect.scale0 === 1 ? card.item.id : -1
  }

  // 弹卡即时(真机反馈:意图延迟 100ms 不跟手,已撤;无封面视频「等首帧才弹」方案同理撤销——
  // 移入必须立即有放大反馈,视频首帧就绪后渐显接管)。唯一异步是预解码,缓存命中时一帧内完成、
  // 不可感;token 只防「换格后旧解码滞后落卡」的竞态。
  let hoverPrepToken = 0

  /** 作废在途的弹卡准备(换格/划走/滚动/清除时)。 */
  function cancelHoverPrep() {
    hoverPrepToken++
  }

  function clearHover() {
    cancelHoverPrep()
    // 有卡才重绘:镜像格的 canvas 侧 chrome 折返需一帧接上;无卡空转调用(滚动中逐次
    // pointermove)不排帧。
    if (hoverCard.value !== null) deps.requestRedraw()
    hoverCard.value = null
    const cv = deps.canvasRef.value
    if (cv) cv.style.cursor = 'default'
  }

  /** 意图延迟到点 → 预解码 → 弹卡。预解码把图先热进浏览器解码缓存,卡内 MediaThumb 首帧
   *  即出图,不再「图→占位色→图」闪一下(真机问题1);无 URL(待生成)时占位与 canvas 底层
   *  同为平均色,弹卡本就无感。token 防换格/划走后的滞后弹卡。 */
  async function prepareHoverCard(hit: { item: LayoutRowItem; rowY: number }, token: number) {
    const url = buildThumbUrl(hit.item.thumbStatus, hit.item.thumbPath, deps.cacheDir())
    if (url) {
      const img = new Image()
      img.src = url
      try {
        await img.decode()
      } catch {
        // 解码失败(404/坏图):照常弹卡,MediaThumb 内部自有懒自愈路径
      }
    }
    if (token !== hoverPrepToken) return
    // 预解码等待期开关被打开:不落卡(打码期间不得出现真实内容)。
    if (deps.demoPrivacy()) return
    const cell = { x: hit.item.x, y: hit.rowY - deps.currentY(), w: hit.item.w, h: hit.item.h }
    // 选择模式和“悬停放大”关闭时都保持原位原尺寸(对齐 DOM 的 transform:none);
    // 常规态 1.06 对齐 DOM hover;小格放大镜倍率封顶 1.2(真机反馈 1.5 仍突兀,防深入邻格)。
    const { w: viewW, h: viewH } = deps.viewport()
    const rect = computeHoverRect(
      cell,
      viewW,
      viewH,
      120,
      1.06,
      1.2,
      deps.enableHoverScale() && !deps.isSelectionMode(),
    )
    hoverCard.value = { item: hit.item, rect }
    deps.requestRedraw() // 镜像格 canvas 侧撤 chrome,与卡内 DOM 同帧交接
  }

  /**
   * 悬停跟踪统一入口:canvas 与悬停卡的 pointermove 都汇到这里。
   * fromCard=true 时事件仍在当前悬停卡内部，不能再按底层几何命中相邻格；只有
   * pointerleave 离开整张悬停卡后，canvas 才重新接管并准备下一张卡。
   */
  function handleHoverTracking(e: PointerEvent, fromCard: boolean) {
    if (deps.scrolling() || isThumbLoadDeferred()) {
      clearHover()
      return
    }
    // 演示打码:打码期间不弹新卡(卡内是真实缩略图与真实信息文案)。已存在的卡不在这一支清,
    // 由宿主在开关翻转时统一 clearHover——但打码态下每次指针移动都会走到这里,清掉更稳。
    if (deps.demoPrivacy()) {
      clearHover()
      return
    }
    // 放大卡覆盖相邻格时，卡内所有区域(图片、信息和控件)都仍属于当前图片；
    // 只在离开悬停卡后重新走 canvas 的几何命中，避免指针在卡内误切相邻图。
    if (fromCard) {
      cancelHoverPrep()
      return
    }
    // 按键拖动中(框选/拖拽)不弹新卡:卡在按下起点保持,避免飞过的格子逐个弹卡干扰框选视觉。
    if (e.buttons !== 0) return
    const hit = deps.hitTest.pickWithRow(e)
    if (!fromCard) {
      const cv = deps.canvasRef.value
      if (cv) cv.style.cursor = hit ? 'pointer' : 'default'
    }
    const nextId = hit?.item.id ?? null
    if (nextId === (hoverCard.value?.item.id ?? null)) {
      cancelHoverPrep() // 指针回到现卡格:作废在途换卡
      return
    }
    if (!hit) {
      // 卡外扩部分悬在空隙/分隔行上:几何上无命中但指针仍在卡内,保持不灭;
      // canvas 收到的 null 则是真在空隙上(卡区域内事件不会落到 canvas)→ 清除。
      if (!fromCard) clearHover()
      else cancelHoverPrep()
      return
    }
    if (deps.isPendingDelete(hit.item.id)) {
      if (!fromCard) clearHover()
      return
    }
    // 换格:立即起预解码,解码毕即弹卡(等待期现卡保持,视觉无空窗)。
    cancelHoverPrep()
    void prepareHoverCard(hit, hoverPrepToken)
  }

  function onPointerMove(e: PointerEvent) {
    handleHoverTracking(e, false)
  }
  function onHoverCardPointerMove(e: PointerEvent) {
    handleHoverTracking(e, true)
  }

  /** canvas pointerleave:若是移入悬停卡(卡覆盖在格子上方)则保持,真正移出才清除。 */
  function onCanvasPointerLeave(e: PointerEvent) {
    const rt = e.relatedTarget as Node | null
    if (rt && hoverLayerRef.value?.contains(rt)) return
    clearHover()
  }

  /** 悬停卡 pointerleave:钳位后卡未必全盖住原格(贴边内推),移出点若仍命中同格则保持,
   *  防「出卡→命中同格→重弹卡→又在卡上」的闪烁循环。 */
  function onHoverCardPointerLeave(e: PointerEvent) {
    const cur = hoverCard.value
    if (cur) {
      const hit = deps.hitTest.pickWithRow(e)
      if (hit && hit.item.id === cur.item.id) return
    }
    clearHover()
  }

  // 悬停卡把格级交互原样转发宿主(与 canvas 命中路径同一出口,行为零分叉)。
  function onHoverCardClick(e: MouseEvent) {
    const cur = hoverCard.value
    if (cur) deps.emit('cell-click', cur.item, e)
  }
  function onHoverCardContextMenu(e: MouseEvent) {
    const cur = hoverCard.value
    if (cur) deps.emit('cell-contextmenu', cur.item, e)
  }
  function onHoverCardPointerDown(e: PointerEvent) {
    const cur = hoverCard.value
    if (cur) deps.emit('cell-pointerdown', cur.item.id, e, deps.hitTest.hitHandleAt(e))
  }

  return {
    hoverLayerRef,
    hoverCard,
    hoverSelected,
    hoverCardBare,
    hoverCardMirror,
    mirrorCellId,
    clearHover,
    cancelHoverPrep,
    onPointerMove,
    onHoverCardPointerMove,
    onCanvasPointerLeave,
    onHoverCardPointerLeave,
    onHoverCardClick,
    onHoverCardContextMenu,
    onHoverCardPointerDown,
  }
}

export type CanvasHoverCard = ReturnType<typeof useCanvasHoverCard>
