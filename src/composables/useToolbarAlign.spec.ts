// useToolbarAlign contract 测试(新需求 2)。node 环境无 localStorage,故持久化经 try/catch 静默降级
// 不在 node 可验;此处覆盖单例内存态的默认回落与响应式读写。
import { describe, it, expect } from 'vitest'
import { useToolbarAlign } from './useToolbarAlign'

describe('useToolbarAlign（顶栏对齐单例内存态）', () => {
  it("默认 'center'（node 无持久值 → 回落居中,零惊讶）", () => {
    const { align } = useToolbarAlign()
    expect(align.value).toBe('center')
  })

  it('setAlign 更新响应式 align（三态切换,单例共享）', () => {
    const a = useToolbarAlign()
    a.setAlign('left')
    expect(a.align.value).toBe('left')
    // 同模块单例:第二次调用取到同一 ref。
    const b = useToolbarAlign()
    expect(b.align.value).toBe('left')
    a.setAlign('right')
    expect(b.align.value).toBe('right')
    // 复位,避免污染同文件后续读者。
    a.setAlign('center')
    expect(a.align.value).toBe('center')
  })
})
