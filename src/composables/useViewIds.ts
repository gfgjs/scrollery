// src/composables/useViewIds.ts
// Part5 T4a · 视图布局序全集 id —— 选区脱离 DOM 的第二支柱。
//
// range / select-all / invert 的「顺序」与「全集」必须来自**布局序 flat_ids**,不来自可视 DOM。
// 旧实现用 container.querySelectorAll('[data-item-id]') 只覆盖已渲染节点 → Shift 跨视口失效、
// Ctrl+A 只选一屏、框选漏掉滚出屏幕的项（Part5 G1 三症状）。本 composable 经后端 get_view_ids
// 取布局缓存里已物化的 flat_ids（cache.rs:217 直接 clone,O(1) 无 DB），从根上解除对 DOM 的依赖。
//
// 设计依据：docs/refactor_2026/2026-06-30-Part5-选区契约与可插拔多模式设计.md §4。

import { shallowRef, readonly } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { IPC } from '../constants/ipc'

// 模块级单例：同一时刻只有一个「当前视图」,与 useSelection 的单例模式一致。
// 大数组(可达百万)用 shallowRef,避免 Vue 深代理对海量元素的 CPU/内存开销(前端规约)。
const viewIds = shallowRef<readonly number[]>([])
// id → 布局序 index 的 O(1) 索引。非 ref:它是随 viewIds 重建的派生查找结构,
// 消费方对「视图变化」的反应由 viewIds.value 引用变更驱动,index 无需自身响应式。
let idIndex = new Map<number, number>()
// 已加载对应的 layout_version;null = 尚未加载 / 已失效清空,供 ensureFresh 判定。
let loadedVersion: number | null = null
// in-flight token(2026-07-06 审查 P0-2):refresh 必须是「最后发起者赢」而非「最后落地者赢」。
// 布局版本快速连跳(缩略图滑块/enrichment 每 2s 重算)时多个 refresh 并发在途,
// 若无 token,迟到的旧版本应答(尤其 ViewStale 拒绝走 catch)会把刚落地的新版本全集清空,
// 且无人再触发重取 → Shift 区间选择静默退化为单项 toggle(Part5 G1 症状无声回归)。
let refreshToken = 0

/**
 * 拉取指定布局版本的全集 id 并重建索引。
 * 失败（ViewStale 版本不符 / LayoutNotReady 无布局 / 其它）→ 清空,等下次重取
 * (锚点失效保护:宁可让 range/全选暂时取不到,也不基于过期 flat_ids 误选)。
 * 落地前复核 token:自己已不是最新发起者 → 结果直接丢弃(成功与失败路径同规)。
 */
async function refresh(layoutVersion: number): Promise<void> {
  const my = ++refreshToken
  try {
    // 后端 layout_version: Option<u64>,IPC 层自动 snake→camel
    const ids = await invokeIpc<number[]>(IPC.GET_VIEW_IDS, { layoutVersion })
    if (my !== refreshToken) return // 迟到应答,已有更新的 refresh 在途/落地
    const idx = new Map<number, number>()
    for (let i = 0; i < ids.length; i++) idx.set(ids[i], i)
    viewIds.value = ids
    idIndex = idx
    loadedVersion = layoutVersion
  } catch (err) {
    if (my !== refreshToken) return // 迟到的拒绝不得清掉新版本数据
    // ViewStale / LayoutNotReady 属预期路径;其它错误记录便于排查,但同样降级为「清空待重取」。
    logger.warn('[useViewIds] get_view_ids 失败,清空待重取', { error: err })
    viewIds.value = []
    idIndex = new Map()
    loadedVersion = null
  }
}

/** 仅当版本变化（或未加载）时才重取,避免无谓 IPC。 */
async function ensureFresh(layoutVersion: number): Promise<void> {
  if (loadedVersion !== layoutVersion) await refresh(layoutVersion)
}

/** id 的布局序下标;不在当前视图返回 -1。 */
function indexOf(id: number): number {
  return idIndex.get(id) ?? -1
}

/** 当前已加载是否对应给定版本(且非空失效态)。 */
function isFresh(layoutVersion: number): boolean {
  return loadedVersion === layoutVersion
}

/**
 * 布局序上的闭区间 [anchor, to]（含端点），与方向无关。
 * 任一端点不在当前视图（如布局已失效尚未重取）→ 返回空数组,由调用侧决定降级行为。
 */
function rangeBetween(anchorId: number, toId: number): number[] {
  const ai = idIndex.get(anchorId)
  const bi = idIndex.get(toId)
  if (ai === undefined || bi === undefined) return []
  const lo = Math.min(ai, bi)
  const hi = Math.max(ai, bi)
  return viewIds.value.slice(lo, hi + 1)
}

/** 当前视图布局序全集（过渡期物化 all 态、invert 全集运算用）。 */
function allIds(): readonly number[] {
  return viewIds.value
}

/** 全集元素数（all 态计数基数）。 */
function totalCount(): number {
  return viewIds.value.length
}

/**
 * 视图布局序全集 id 单例。
 * 用法:布局摘要的 layoutVersion 变化时调 ensureFresh(version);
 *      选区策略经 rangeBetween / allIds / totalCount 取序与全集,不再依赖可视 DOM。
 */
export function useViewIds() {
  return {
    viewIds: readonly(viewIds),
    refresh,
    ensureFresh,
    indexOf,
    isFresh,
    rangeBetween,
    allIds,
    totalCount,
  }
}
