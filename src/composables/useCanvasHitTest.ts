// Canvas 网格算术命中(点击/右键/拖拽手柄),从 MediaGridCanvas.vue 下沉(方案 2.2 ⑥)。
// 工厂在宿主 <script setup> 顶层只调用一次。HANDLE_INSET/HANDLE_SIZE 从
// mediaGridCanvas.painters.ts 单一来源导入(§3.3:防绘制/命中两处几何漂移)。
import type { Ref } from 'vue'
import { hitTestCell, hitTestCellWithRow } from '../components/media/mediaGridCanvas.helpers'
import { HANDLE_INSET, HANDLE_SIZE } from '../components/media/mediaGridCanvas.painters'
import type { LayoutRow, LayoutRowItem } from '../types/layout'

export interface CanvasHitTestDeps {
  canvasRef: Ref<HTMLCanvasElement | null>
  rows: () => LayoutRow[]
  currentY: () => number
  isSelected: (id: number) => boolean
  showDragHandle: () => boolean
}

export function useCanvasHitTest(deps: CanvasHitTestDeps) {
  /** canvas 视口坐标 → 逻辑坐标(currentY + 视口内偏移)——三处坐标换算共享(顺手发现)。 */
  function clientToLogical(clientX: number, clientY: number): { x: number; logicalY: number } | null {
    const cv = deps.canvasRef.value
    if (!cv) return null
    const rect = cv.getBoundingClientRect()
    return { x: clientX - rect.left, logicalY: deps.currentY() + (clientY - rect.top) }
  }

  function idAtClient(clientX: number, clientY: number): number | null {
    const p = clientToLogical(clientX, clientY)
    if (!p) return null
    const item = hitTestCell(deps.rows(), p.x, p.logicalY)
    return item ? item.id : null
  }

  function pick(e: MouseEvent | PointerEvent): LayoutRowItem | null {
    const p = clientToLogical(e.clientX, e.clientY)
    if (!p) return null
    return hitTestCell(deps.rows(), p.x, p.logicalY)
  }

  /** 命中(带格子视口矩形):悬停卡定位需要 row.y,与 pick 的差异仅在返回行坐标。 */
  function pickWithRow(e: PointerEvent | MouseEvent) {
    const p = clientToLogical(e.clientX, e.clientY)
    if (!p) return null
    return hitTestCellWithRow(deps.rows(), p.x, p.logicalY)
  }

  /** 几何命中左上拖拽手柄(仅已选中格):按内容原尺寸左上角矩形判定。
   *  Canvas 无逐格 DOM,拖到文件夹的分流靠此显式告知 onCardPointerDown(见 useMediaDragToFolder);
   *  含触屏(无悬停卡)场景。悬停卡路径另有真实 DOM 手柄,但此处几何判定对两路径都成立。 */
  function hitHandleAt(e: PointerEvent): boolean {
    // 手柄被用户关闭(#5)即不可命中——只藏不停用会出现「看不见却能拖走」的幽灵手柄。
    if (!deps.showDragHandle()) return false
    const cv = deps.canvasRef.value
    if (!cv) return false
    const hit = pickWithRow(e)
    if (!hit || !deps.isSelected(hit.item.id)) return false
    const cr = cv.getBoundingClientRect()
    const px = e.clientX - cr.left
    const py = e.clientY - cr.top
    const it = hit.item
    const cx = it.x
    const cyTop = hit.rowY - deps.currentY()
    // 命中区 = 绘制矩形外扩 pad(触屏/边缘容错)。
    const pad = 3
    const hx = cx + HANDLE_INSET - pad
    const hy = cyTop + HANDLE_INSET - pad
    const s = HANDLE_SIZE + pad * 2
    return px >= hx && px <= hx + s && py >= hy && py <= hy + s
  }

  return { idAtClient, pick, pickWithRow, hitHandleAt }
}

export type CanvasHitTest = ReturnType<typeof useCanvasHitTest>
