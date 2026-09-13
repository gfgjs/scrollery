// canvasThumbState —— MediaGridCanvas 的缩略图加载状态机(纯逻辑,框架无关)。
//
// 深审 defer ②:这套 status/heal/LRU 语义是 useThumbLoader(DOM 路径)的 canvas 版
// 重写,原先内联在组件里零测试——两份同语义实现只有 DOM 版有测,是 P1-4 类「两条
// 流水线各改各的」病灶的前端温床。抽出为纯模块后由 characterization 测锁行为
// (含 0597a19 的「迟到失败 sig 陈旧守卫」回归钉);ImageBitmap 等资源释放经注入的
// `closeSrc` 回调完成,node 环境测试无需 DOM。
//
// 语义总览(与 useThumbLoader 的对应关系):
//  - sigMap(数据签名 status|path)变化 → 硬失效:清失败/请求标记、关闭并作废缓存
//    (等价 useThumbLoader 的 [thumbPath, thumbStatus] watch 重载)。
//  - requestedThumbSet / requestedHealSet:每 id 每 sig 代次只上抛一次生成/自愈请求
//    (等价 hasRequested / hasRequestedHeal 守卫,防后端持续失败时死循环)。
//  - 迟到的成功/失败必须比对发起时的 dataSig:数据已换代一律作废,失败尤其不得
//    把 failedSet 毒化到新 sig 头上(0597a19)。

/** 缓存条目:解码产物 + 渲染规格签名(数据签名+高度桶+格纵横比,规格过期走 stale 续画)。 */
export interface CachedThumb<S> {
  src: S
  renderSig: string
}

/** 失败裁决:stale=数据已换代(作废,不标记)/ heal=首次 404 需上抛自愈 / fail=仅标失败。 */
export type LoadFailure = 'stale' | 'heal' | 'fail'

export interface CanvasThumbState<S> {
  /** 数据签名对账:变化即硬失效(清标记 + 关闭缓存)。每次取图前调用。 */
  syncSig(id: number, sig: string): void
  /** LRU 读:命中即重插队尾(标记最近使用)。 */
  get(id: number): CachedThumb<S> | undefined
  /** 只读探查:命中返回条目但**不**改变 LRU 顺序。可见区逐帧分类(有位图可画 /
   *  真实缺图)用,避免给 get 的 LRU 重插造成双重无谓写入。 */
  peek(id: number): CachedThumb<S> | undefined
  isFailed(id: number): boolean
  isLoading(id: number): boolean
  markLoading(id: number): void
  /** 主动取消仍可中止的 IO：只清在途账，不标失败、不触发自愈。 */
  cancelLoad(id: number): boolean
  /** status 0/3-无路径的生成请求去重:首问 true(调用方上抛),同代次再问恒 false。 */
  requestThumbOnce(id: number): boolean
  /**
   * 加载成功落缓存。发起时的 dataSig 已过期 → 关闭 src 并返回 false(调用方不重绘);
   * 现行 → 清在途、LRU 写入(覆盖旧规格即释放其 src,超上限从队首驱逐)并返回 true。
   */
  commitLoad(id: number, dataSig: string, entry: CachedThumb<S>): boolean
  /**
   * 加载失败裁决。dataSig 已过期 → 'stale'(仅清在途,**不**标失败不记自愈——陈旧失败
   * 毒化新代次是 0597a19 修的病);现行且 status=1 首败 → 'heal'(调用方上抛自愈)并标失败;
   * 其余 → 'fail' 仅标失败。
   */
  failLoad(id: number, status: number, dataSig: string): LoadFailure
  /** 释放全部缓存 src 并清空所有状态(组件卸载用)。 */
  clear(): void
  /** 当前缓存条目数(测试/诊断用)。 */
  size(): number
  /** 当前缓存累计字节估算(未配置 budget 时恒 0;测试/诊断用)。 */
  bytes(): number
  /** 当前正在 fetch/decode 的条目数；组件用它限制全局在途量，避免闸门放行时任务风暴。 */
  loadingCount(): number
}

/** 字节预算(2026-07-10 审查 B11):条目数上限与格子尺寸倒挂——大格模式一条 ImageBitmap
 * 可达 1-3MB,固定数千条理论驻留 GB 级;极密模式一条约几十 KB,同一上限又可能过于保守。字节预算
 * 与条目上限**双约束**,任一超限即从队首驱逐。 */
export interface CanvasThumbBudget<S> {
  /** 累计字节上限(按 byteCost 估算)。 */
  maxBytes: number
  /** 单条解码产物的字节代价(ImageBitmap ≈ width*height*4)。 */
  byteCost: (src: S) => number
}

/**
 * @param maxCache LRU 容量上限(超出从队首驱逐最久未用)
 * @param closeSrc 释放解码产物(ImageBitmap.close;HTMLImageElement 传 no-op 交 GC)
 * @param budget 可选字节预算(B11):与条目上限双约束;不传则行为与旧版逐字节一致
 */
export function createCanvasThumbState<S>(
  maxCache: number,
  closeSrc: (src: S) => void,
  budget?: CanvasThumbBudget<S>,
): CanvasThumbState<S> {
  const imageCache = new Map<number, CachedThumb<S>>()
  const loadingSet = new Set<number>()
  const failedSet = new Set<number>()
  const requestedThumbSet = new Set<number>()
  const requestedHealSet = new Set<number>()
  const sigMap = new Map<number, string>()
  // 字节账本:仅配置 budget 时记账。cost 在插入时一次性估算并随条目存放,
  // 驱逐/覆盖/作废时按账面扣减,避免驱逐路径重算(位图尺寸不变量)。
  const costMap = new Map<number, number>()
  let totalBytes = 0

  function dropEntry(id: number, entry: CachedThumb<S>): void {
    closeSrc(entry.src)
    imageCache.delete(id)
    totalBytes -= costMap.get(id) ?? 0
    costMap.delete(id)
  }

  function syncSig(id: number, sig: string): void {
    if (sigMap.get(id) === sig) return
    sigMap.set(id, sig)
    failedSet.delete(id)
    requestedHealSet.delete(id)
    requestedThumbSet.delete(id)
    // 在途请求一并出账(2026-07-10 审查 C24):否则请求永不落定(网络/Image 挂起)时,
    // 新代次被 isLoading 永久短路、该格再无重试路径;迟到落地本就被 dataSig 守卫拒收,
    // 清掉无副作用。
    loadingSet.delete(id)
    const prev = imageCache.get(id)
    if (prev) {
      dropEntry(id, prev) // 数据换代(自愈/重生成):旧内容不做 stale,直接作废
    }
  }

  function get(id: number): CachedThumb<S> | undefined {
    const ent = imageCache.get(id)
    if (ent) {
      imageCache.delete(id)
      imageCache.set(id, ent)
    }
    return ent
  }

  function commitLoad(id: number, dataSig: string, entry: CachedThumb<S>): boolean {
    loadingSet.delete(id)
    if (sigMap.get(id) !== dataSig) {
      closeSrc(entry.src) // 数据已换代(自愈/重生成),旧内容作废
      return false
    }
    const prev = imageCache.get(id)
    if (prev) {
      if (prev.src !== entry.src) closeSrc(prev.src)
      totalBytes -= costMap.get(id) ?? 0
      costMap.delete(id)
    }
    imageCache.delete(id) // 覆盖也算最近使用:重插到队尾
    imageCache.set(id, entry)
    if (budget) {
      const cost = budget.byteCost(entry.src)
      costMap.set(id, cost)
      totalBytes += cost
    }
    // 双约束驱逐(B11):条目数或字节任一超限即从队首(最久未用)出局;**绝不驱逐刚插入项**
    // ——单图即超预算时保它独存,否则该格永远画不出来。驱逐勿命中可见格由预算余量保证
    // (组件侧预算 ≥ 2× 预取窗口驻留),不在此做可见性判断(纯模块不识别视口)。
    for (const [key, e] of imageCache) {
      const over = imageCache.size > maxCache || (budget != null && totalBytes > budget.maxBytes)
      if (!over || key === id) break
      dropEntry(key, e)
    }
    return true
  }

  function failLoad(id: number, status: number, dataSig: string): LoadFailure {
    loadingSet.delete(id)
    if (sigMap.get(id) !== dataSig) return 'stale'
    let verdict: LoadFailure = 'fail'
    if (status === 1 && !requestedHealSet.has(id)) {
      requestedHealSet.add(id)
      verdict = 'heal'
    }
    failedSet.add(id)
    return verdict
  }

  return {
    syncSig,
    get,
    peek: (id) => imageCache.get(id),
    isFailed: (id) => failedSet.has(id),
    isLoading: (id) => loadingSet.has(id),
    markLoading: (id) => void loadingSet.add(id),
    cancelLoad: (id) => loadingSet.delete(id),
    requestThumbOnce: (id) => {
      if (requestedThumbSet.has(id)) return false
      requestedThumbSet.add(id)
      return true
    },
    commitLoad,
    failLoad,
    clear: () => {
      for (const e of imageCache.values()) closeSrc(e.src) // ImageBitmap 须显式释放,不能只清 Map
      imageCache.clear()
      loadingSet.clear()
      failedSet.clear()
      requestedThumbSet.clear()
      requestedHealSet.clear()
      sigMap.clear()
      costMap.clear()
      totalBytes = 0
    },
    size: () => imageCache.size,
    bytes: () => totalBytes,
    loadingCount: () => loadingSet.size,
  }
}
