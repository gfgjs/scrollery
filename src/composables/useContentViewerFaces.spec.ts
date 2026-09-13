// useContentViewerFaces characterization spec(超长文件拆分方案 §5 补测要求 / 复核建议)。
//
// 覆盖面:faceToken 并发丢弃——loadFacesFor 连续调用两次,前次(旧图)异步应答晚于后次(新图)到达时,
// 其结果被 my !== faceToken 判定丢弃,不覆盖 faces。红线注释(源文件顶部)要求该判定逐字保留。
// 不覆盖:recomputeFaceLayout 的几何计算(需要真实 DOM 尺寸测量,超出本测试范围)。
//
// 环境:node,无 DOM。personStore 换最小 stub(deferred promise 手控到达顺序),viewerRef/
// currentImageElement 均返回 null(不触发 recomputeFaceLayout 的 DOM 分支)。
import { describe, it, expect, vi } from 'vitest'
import { ref, effectScope } from 'vue'
import { useContentViewerFaces } from './useContentViewerFaces'
import type { FaceBox } from '../types/person'

function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('useContentViewerFaces: faceToken 并发丢弃', () => {
  it('前次(旧图)异步返回晚到时其结果被 token 判定丢弃,不覆盖新图的 faces', async () => {
    // node 无 localStorage:被测文件顶层 showFaces 初始化直接读 localStorage.getItem,需最小 stub
    // (参照 useGalleryAxisControls.spec.ts 范式)。
    vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} })

    const d1 = deferred<FaceBox[]>() // 第一次调用(旧图,id=1)
    const d2 = deferred<FaceBox[]>() // 第二次调用(新图,id=2)

    const personStore = {
      getFacesForItem: vi.fn((id: number) => (id === 1 ? d1.promise : d2.promise)),
    } as unknown as Parameters<typeof useContentViewerFaces>[0]['personStore']

    const scope = effectScope()
    const c = scope.run(() =>
      useContentViewerFaces({
        viewerRef: ref(null),
        currentImageElement: () => null,
        transform: () => '',
        personStore,
      }),
    )!

    // 连续两次 loadFacesFor(方向键快速翻图场景):第二次递增 faceToken,使第一次的 token 失效。
    const p1 = c.loadFacesFor({ id: 1, mediaType: 'image' })
    const p2 = c.loadFacesFor({ id: 2, mediaType: 'image' })

    // 新图(第二次)先应答到达。
    const facesB: FaceBox[] = [
      { id: 2, personId: null, personName: null, bbox: [0, 0, 1, 1], detScore: 0.9 },
    ]
    d2.resolve(facesB)
    await p2
    expect(c.faces.value).toEqual(facesB)

    // 旧图(第一次)迟到应答:token 已被第二次调用推进,判定丢弃,不得覆盖当前 faces。
    const facesA: FaceBox[] = [
      { id: 1, personId: null, personName: null, bbox: [0.5, 0.5, 0.1, 0.1], detScore: 0.8 },
    ]
    d1.resolve(facesA)
    await p1
    expect(c.faces.value).toEqual(facesB) // 仍是新图的结果,未被旧图覆盖

    scope.stop()
  })
})
