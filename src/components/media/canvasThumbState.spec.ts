// canvasThumbState characterization 测(深审 defer ②):锁死 canvas 缩略图状态机的既有行为。
// 此前这套语义内联在 MediaGridCanvas 组件里零测试——与 useThumbLoader(DOM 版,有测)是
// 同语义双实现,漂移风险最高的一份。src 用普通对象 + 注入 closeSrc 计数,node 环境零 DOM。
import { describe, it, expect, vi } from 'vitest'
import { createCanvasThumbState } from './canvasThumbState'

interface FakeSrc {
  tag: string
}

function harness(maxCache = 3) {
  const closed: string[] = []
  const closeSrc = vi.fn((s: FakeSrc) => closed.push(s.tag))
  const st = createCanvasThumbState<FakeSrc>(maxCache, closeSrc)
  return { st, closed, closeSrc }
}

const ent = (tag: string, renderSig = 'r1') => ({ src: { tag }, renderSig })

describe('LRU 语义', () => {
  it('命中即重插队尾:被 get 触碰的条目躲过驱逐,最久未用者出局并被释放', () => {
    const { st, closed } = harness(3)
    st.syncSig(1, 's')
    st.syncSig(2, 's')
    st.syncSig(3, 's')
    st.syncSig(4, 's')
    expect(st.commitLoad(1, 's', ent('a'))).toBe(true)
    expect(st.commitLoad(2, 's', ent('b'))).toBe(true)
    expect(st.commitLoad(3, 's', ent('c'))).toBe(true)
    st.get(1) // 触碰 1 → 最久未用变成 2
    st.commitLoad(4, 's', ent('d'))
    expect(st.size()).toBe(3)
    // 驱逐最久未用的 2 并 closeSrc
    expect(closed).toEqual(['b'])
    expect(st.get(1)?.src.tag).toBe('a')
    expect(st.get(2)).toBeUndefined()
  })

  it('同 id 覆盖:换 src 释放旧 src,同 src(规格更新)不自释放', () => {
    const { st, closed } = harness()
    st.syncSig(1, 's')
    const first = ent('old')
    st.commitLoad(1, 's', first)
    st.commitLoad(1, 's', ent('new'))
    expect(closed).toEqual(['old'])
    // 同一 src 仅更新 renderSig → 不得 close(还在被缓存引用)。
    st.commitLoad(1, 's', { src: first.src, renderSig: 'r2' })
    expect(closed).toEqual(['old', 'new'])
  })
})

describe('sig 换代硬失效', () => {
  it('sig 变化:清失败/请求/自愈标记,关闭并作废缓存;同 sig 幂等不动', () => {
    const { st, closed } = harness()
    st.syncSig(1, 'sigA')
    st.commitLoad(1, 'sigA', ent('a'))
    expect(st.requestThumbOnce(1)).toBe(true)
    st.failLoad(1, 1, 'sigA')
    expect(st.isFailed(1)).toBe(true)

    st.syncSig(1, 'sigA') // 同 sig:全部保持
    expect(st.isFailed(1)).toBe(true)
    expect(st.requestThumbOnce(1)).toBe(false)
    expect(closed).toEqual([])

    st.syncSig(1, 'sigB') // 换代:全清
    expect(closed).toEqual(['a'])
    expect(st.get(1)).toBeUndefined()
    expect(st.isFailed(1)).toBe(false)
    expect(st.requestThumbOnce(1)).toBe(true)
    // 换代后 status=1 失败可再次上抛自愈(requestedHealSet 已清)。
    expect(st.failLoad(1, 1, 'sigB')).toBe('heal')
  })
})

describe('请求去重', () => {
  it('requestThumbOnce:同代次首问 true、复问 false(防持续失败死循环)', () => {
    const { st } = harness()
    st.syncSig(7, 's')
    expect(st.requestThumbOnce(7)).toBe(true)
    expect(st.requestThumbOnce(7)).toBe(false)
  })
})

describe('commitLoad 陈旧守卫', () => {
  it('现行 sig:清在途、落缓存、返回 true;陈旧 sig:关闭产物、拒收、返回 false', () => {
    const { st, closed } = harness()
    st.syncSig(1, 'sigA')
    st.markLoading(1)
    st.syncSig(1, 'sigB') // 在途中数据换代
    expect(st.commitLoad(1, 'sigA', ent('stale'))).toBe(false)
    expect(closed).toEqual(['stale'])
    expect(st.get(1)).toBeUndefined()
    // 无论新旧,在途标记都清(该 id 的加载已终结)
    expect(st.isLoading(1)).toBe(false)

    st.markLoading(1)
    expect(st.commitLoad(1, 'sigB', ent('fresh'))).toBe(true)
    expect(st.get(1)?.src.tag).toBe('fresh')
    expect(st.isLoading(1)).toBe(false)
  })
})

describe('loadingCount', () => {
  it('准确反映在途加载，并在成功、失败和 sig 换代时释放槽位', () => {
    const st = createCanvasThumbState<{ tag: string }>(10, () => undefined)
    st.syncSig(1, 'a')
    st.syncSig(2, 'a')
    st.markLoading(1)
    st.markLoading(2)
    expect(st.loadingCount()).toBe(2)

    st.commitLoad(1, 'a', ent('one'))
    expect(st.loadingCount()).toBe(1)
    st.failLoad(2, 0, 'a')
    expect(st.loadingCount()).toBe(0)

    st.markLoading(1)
    st.syncSig(1, 'b')
    expect(st.loadingCount()).toBe(0)
  })

  it('主动取消只释放在途槽，不污染失败态', () => {
    const st = createCanvasThumbState<FakeSrc>(10, () => {})
    st.syncSig(1, 'a')
    st.markLoading(1)
    expect(st.cancelLoad(1)).toBe(true)
    expect(st.loadingCount()).toBe(0)
    expect(st.isFailed(1)).toBe(false)
    expect(st.cancelLoad(1)).toBe(false)
  })
})

describe('failLoad 裁决(0597a19 回归钉)', () => {
  it('status=1 首败 → heal(上抛自愈)且标失败;复败 → fail(自愈只上抛一次)', () => {
    const { st } = harness()
    st.syncSig(1, 's')
    expect(st.failLoad(1, 1, 's')).toBe('heal')
    expect(st.isFailed(1)).toBe(true)
    expect(st.failLoad(1, 1, 's')).toBe('fail')
  })

  it('status≠1(0/2/3)失败 → fail,从不上抛自愈(0/2/3 由 pending/失败流程管)', () => {
    const { st } = harness()
    st.syncSig(1, 's')
    expect(st.failLoad(1, 0, 's')).toBe('fail')
    st.syncSig(2, 's')
    expect(st.failLoad(2, 3, 's')).toBe('fail')
  })

  it('陈旧 sig 的迟到失败 → stale:不标失败、不耗自愈额度——新代次不被毒化(0597a19)', () => {
    const { st } = harness()
    st.syncSig(1, 'sigA')
    st.markLoading(1)
    st.syncSig(1, 'sigB') // 在途中换代(自愈复位 → 重生成回填)
    expect(st.failLoad(1, 1, 'sigA')).toBe('stale')
    // 新 sig 不得被旧失败毒化成 failed 短路
    expect(st.isFailed(1)).toBe(false)
    expect(st.isLoading(1)).toBe(false)
    // 新代次的真实失败仍可正常上抛自愈。
    expect(st.failLoad(1, 1, 'sigB')).toBe('heal')
  })
})

// ── 字节预算(2026-07-10 审查 B11)+ syncSig 清在途(C24) ─────────────────────────
interface SizedSrc {
  tag: string
  cost: number
}

function budgetHarness(maxCache: number, maxBytes: number) {
  const closed: string[] = []
  const st = createCanvasThumbState<SizedSrc>(maxCache, (s) => closed.push(s.tag), {
    maxBytes,
    byteCost: (s) => s.cost,
  })
  return { st, closed }
}

const sent = (tag: string, cost: number, renderSig = 'r1') => ({ src: { tag, cost }, renderSig })

describe('字节预算双约束(B11)', () => {
  it('累计超 maxBytes:条目数未超也从队首驱逐最久未用,账面同步扣减', () => {
    const { st, closed } = budgetHarness(10, 100)
    st.syncSig(1, 's')
    st.syncSig(2, 's')
    st.syncSig(3, 's')
    st.commitLoad(1, 's', sent('a', 60))
    st.commitLoad(2, 's', sent('b', 30))
    expect(st.bytes()).toBe(90)
    st.commitLoad(3, 's', sent('c', 30)) // 120 > 100 → 驱逐队首 a 后 60 ≤ 100 停
    expect(closed).toEqual(['a'])
    expect(st.size()).toBe(2)
    expect(st.bytes()).toBe(60)
  })

  it('单条即超预算:绝不驱逐刚插入项(独存),下一条插入时才轮到它出局', () => {
    const { st, closed } = budgetHarness(10, 100)
    st.syncSig(1, 's')
    st.syncSig(2, 's')
    st.commitLoad(1, 's', sent('big', 150))
    expect(closed).toEqual([]) // 独存,否则该格永远画不出来
    expect(st.bytes()).toBe(150)
    st.commitLoad(2, 's', sent('d', 10))
    expect(closed).toEqual(['big'])
    expect(st.bytes()).toBe(10)
  })

  it('同 id 覆盖:旧账扣除新账计入,不重复计费', () => {
    const { st } = budgetHarness(10, 100)
    st.syncSig(1, 's')
    st.commitLoad(1, 's', sent('old', 60))
    st.commitLoad(1, 's', sent('new', 20))
    expect(st.bytes()).toBe(20)
  })

  it('syncSig 换代作废与 clear:字节账清零', () => {
    const { st } = budgetHarness(10, 100)
    st.syncSig(1, 'sigA')
    st.commitLoad(1, 'sigA', sent('a', 60))
    st.syncSig(1, 'sigB')
    expect(st.bytes()).toBe(0)
    st.syncSig(2, 's')
    st.commitLoad(2, 's', sent('b', 30))
    st.clear()
    expect(st.bytes()).toBe(0)
  })

  it('条目上限独立生效:字节富余时仍按 maxCache 驱逐(极密模式行为不变)', () => {
    const { st, closed } = budgetHarness(2, 1000)
    st.syncSig(1, 's')
    st.syncSig(2, 's')
    st.syncSig(3, 's')
    st.commitLoad(1, 's', sent('a', 1))
    st.commitLoad(2, 's', sent('b', 1))
    st.commitLoad(3, 's', sent('c', 1))
    expect(closed).toEqual(['a'])
    expect(st.size()).toBe(2)
  })

  it('未配置 budget:bytes() 恒 0,行为与旧版一致(向后兼容)', () => {
    const { st } = harness(3)
    st.syncSig(1, 's')
    st.commitLoad(1, 's', ent('a'))
    expect(st.bytes()).toBe(0)
  })
})

describe('syncSig 清在途(C24)', () => {
  it('在途请求永不落定时,换代解除 isLoading 短路——新代次可重新发起加载', () => {
    const { st } = harness()
    st.syncSig(1, 'sigA')
    st.markLoading(1)
    expect(st.isLoading(1)).toBe(true)
    st.syncSig(1, 'sigB') // 在途中数据换代,旧请求(挂死)再无人清
    expect(st.isLoading(1)).toBe(false) // 不清则该格在新代次下永远停在占位
  })
})

describe('clear', () => {
  it('释放全部 src 并清空所有状态(卸载路径)', () => {
    const { st, closed } = harness()
    st.syncSig(1, 's')
    st.syncSig(2, 's')
    st.commitLoad(1, 's', ent('a'))
    st.commitLoad(2, 's', ent('b'))
    st.markLoading(3)
    st.clear()
    expect(closed.sort()).toEqual(['a', 'b'])
    expect(st.size()).toBe(0)
    expect(st.isLoading(3)).toBe(false)
    // sigMap 亦清:同 id 再入场按新代次从头来。
    expect(st.requestThumbOnce(1)).toBe(true)
  })
})
