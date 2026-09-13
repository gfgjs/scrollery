// useWindowDrag 单测。测点跟着风险走:
//   ① shouldStartDrag —— 点击/拖动的判定就压在这条阈值上,是本特性最核心的数值逻辑。
//   ② 排除选择器契约 —— 漏掉 `input` 会让行高滑块被移窗抢走(用户明确要求排除),故把
//      「哪些控件排除」钉成回归门;双击最大化的排除须比拖拽更宽(额外含 button)。
//   ③ isDragExcluded / isMaximizeExcluded 的 null 安全 + 「查对选择器」—— 用带 closest 的
//      桩验证布尔归一与「拖拽查拖拽选择器、最大化查最大化选择器」,不引入真实 DOM(node 环境无 DOM)。
//   ④ isDragSurface 的 **matches 而非 closest** —— 这是「即时移窗不吞点击」的 fail-safe 关键:
//      若误用 closest,标记容器里的可点 div 会被当成空隙面即时移窗、吞掉点击。用同时带 matches/closest
//      的桩钉死:target 自身不是空隙面(matches=false)但祖先是(closest=true)→ 必须判 false。
//   ⑤ WINDOW_DRAG_HOLD_MS sanity —— 按住时间旋钮须为正、且落在能护住慢速点击的合理区间。
//   ⑥ isDoubleClick —— 空隙面双击最大化不走 DOM dblclick(被首击 startDragging 打断)、改手动判定,
//      故把「时间窗 + 位移容差 + 无上次按下恒 false」钉成回归门(判定逻辑不进 DOM,纯函数直测)。

import { describe, it, expect } from 'vitest'
import {
  shouldStartDrag,
  isDoubleClick,
  isDragExcluded,
  isMaximizeExcluded,
  isDragSurface,
  DRAG_EXCLUDE_SELECTOR,
  MAXIMIZE_EXCLUDE_SELECTOR,
  WINDOW_DRAG_SURFACE_SELECTOR,
  WINDOW_DRAG_THRESHOLD,
  WINDOW_DRAG_HOLD_MS,
  WINDOW_DBLCLICK_MS,
  WINDOW_DBLCLICK_SLOP,
} from './useWindowDrag'

describe('shouldStartDrag', () => {
  it('原地不动不算拖动', () => {
    expect(shouldStartDrag(0, 0)).toBe(false)
  })

  it('阈值内的小位移仍算点击(不拖动)', () => {
    // 用相对阈值取略小的位移,不写死具体值 → 阈值旋钮调整时本例仍成立。
    const under = WINDOW_DRAG_THRESHOLD - 1
    expect(shouldStartDrag(under, 0)).toBe(false)
    expect(shouldStartDrag(under, 0, under + 1)).toBe(false) // under < 阈值
  })

  it('恰好等于阈值不触发(须严格越过)', () => {
    expect(shouldStartDrag(WINDOW_DRAG_THRESHOLD, 0)).toBe(false)
    expect(shouldStartDrag(0, WINDOW_DRAG_THRESHOLD)).toBe(false)
  })

  it('越过阈值判定为拖动(任意方向,含负位移)', () => {
    const over = WINDOW_DRAG_THRESHOLD + 1
    expect(shouldStartDrag(over, 0)).toBe(true)
    expect(shouldStartDrag(0, -over)).toBe(true)
    expect(shouldStartDrag(-over, -over)).toBe(true)
  })

  it('尊重自定义阈值', () => {
    expect(shouldStartDrag(8, 0, 10)).toBe(false)
    expect(shouldStartDrag(11, 0, 10)).toBe(true)
  })
})

describe('排除选择器契约', () => {
  it('拖拽排除覆盖所有原生表单控件 + 逃生舱(用户裁决)', () => {
    for (const token of [
      'input',
      'textarea',
      'select',
      'contenteditable',
      '[data-no-window-drag]',
    ]) {
      expect(DRAG_EXCLUDE_SELECTOR).toContain(token)
    }
  })

  it('双击最大化排除是拖拽排除的超集,额外含可点交互元素', () => {
    // 拖拽排除的每一类,最大化也应排除。
    for (const token of ['input', 'textarea', 'select', 'contenteditable', '[data-no-window-drag]']) {
      expect(MAXIMIZE_EXCLUDE_SELECTOR).toContain(token)
    }
    // 且额外含按钮类(双击按钮不最大化)。
    expect(MAXIMIZE_EXCLUDE_SELECTOR).toContain('button')
  })
})

describe('isDragExcluded / isMaximizeExcluded', () => {
  // 带 closest 的桩:仅当传入指定选择器时返回命中,借此验证「查对了选择器」。
  const stub = (matchSelector: string) => ({
    closest: (sel: string) => (sel === matchSelector ? ({} as Element) : null),
  })

  it('对 null / 无 closest 的目标安全返回 false', () => {
    expect(isDragExcluded(null)).toBe(false)
    expect(isMaximizeExcluded(null)).toBe(false)
    expect(isDragExcluded({} as unknown as EventTarget)).toBe(false)
  })

  it('isDragExcluded 用拖拽选择器查询并归一为布尔', () => {
    expect(isDragExcluded(stub(DRAG_EXCLUDE_SELECTOR) as unknown as EventTarget)).toBe(true)
    // 只匹配最大化选择器的目标不应被拖拽排除(证明查的是拖拽选择器,非最大化)。
    expect(isDragExcluded(stub(MAXIMIZE_EXCLUDE_SELECTOR) as unknown as EventTarget)).toBe(false)
  })

  it('isMaximizeExcluded 用最大化选择器查询并归一为布尔', () => {
    expect(isMaximizeExcluded(stub(MAXIMIZE_EXCLUDE_SELECTOR) as unknown as EventTarget)).toBe(true)
    expect(isMaximizeExcluded(stub(DRAG_EXCLUDE_SELECTOR) as unknown as EventTarget)).toBe(false)
  })
})

describe('isDragSurface(matches 契约:命中本体、绝不命中后代 → fail-safe)', () => {
  it('对 null / 无 matches 的目标安全返回 false', () => {
    expect(isDragSurface(null)).toBe(false)
    expect(isDragSurface({} as unknown as EventTarget)).toBe(false)
  })

  it('e.target 自身即空隙面 → true', () => {
    const selfSurface = {
      matches: (sel: string) => sel === WINDOW_DRAG_SURFACE_SELECTOR,
      closest: () => null,
    }
    expect(isDragSurface(selfSurface as unknown as EventTarget)).toBe(true)
  })

  it('自身非空隙面、仅祖先是(closest 命中)→ 必须 false(证明用 matches 非 closest)', () => {
    // fail-safe 关键:标记容器内的可点 div —— matches=false(自身不是空隙面),
    // closest≠null(祖先容器是空隙面)。若实现误用 closest 会判 true → 即时移窗吞点击。
    const descendantOfSurface = {
      matches: () => false,
      closest: (sel: string) => (sel === WINDOW_DRAG_SURFACE_SELECTOR ? ({} as Element) : null),
    }
    expect(isDragSurface(descendantOfSurface as unknown as EventTarget)).toBe(false)
  })

  it('自身非空隙面、祖先也不是 → false', () => {
    const unrelated = { matches: () => false, closest: () => null }
    expect(isDragSurface(unrelated as unknown as EventTarget)).toBe(false)
  })
})

describe('WINDOW_DRAG_HOLD_MS', () => {
  it('为正数、且落在护住慢速点击的合理区间', () => {
    expect(WINDOW_DRAG_HOLD_MS).toBeGreaterThan(0)
    // 下界:须明显长于典型点击时长(避免慢速点击被误判成拖动);上界:不至久到「按住拖动」无从触发。
    expect(WINDOW_DRAG_HOLD_MS).toBeGreaterThanOrEqual(150)
    expect(WINDOW_DRAG_HOLD_MS).toBeLessThanOrEqual(1000)
  })
})

describe('isDoubleClick(空隙面手动双击判定)', () => {
  it('无上次按下(elapsed=Infinity)恒非双击', () => {
    // 组件里首击以 ts=-Infinity 起手 → elapsed=Infinity;位移再小也不得判双击。
    expect(isDoubleClick(Number.POSITIVE_INFINITY, 0, 0)).toBe(false)
  })

  it('时间窗内 + 位移在容差内 → 双击', () => {
    expect(isDoubleClick(0, 0, 0)).toBe(true)
    expect(isDoubleClick(WINDOW_DBLCLICK_MS, WINDOW_DBLCLICK_SLOP, -WINDOW_DBLCLICK_SLOP)).toBe(true)
  })

  it('超出时间窗 → 非双击(视作两次独立单击)', () => {
    expect(isDoubleClick(WINDOW_DBLCLICK_MS + 1, 0, 0)).toBe(false)
  })

  it('位移超出容差 → 非双击(移动过远视作两次独立单击)', () => {
    expect(isDoubleClick(0, WINDOW_DBLCLICK_SLOP + 1, 0)).toBe(false)
    expect(isDoubleClick(0, 0, WINDOW_DBLCLICK_SLOP + 1)).toBe(false)
  })

  it('尊重自定义时间窗 / 容差', () => {
    expect(isDoubleClick(200, 2, 2, 300, 3)).toBe(true)
    expect(isDoubleClick(400, 2, 2, 300, 3)).toBe(false) // 超时间窗
    expect(isDoubleClick(200, 5, 0, 300, 3)).toBe(false) // 超容差
  })
})
