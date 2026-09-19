// src/composables/usePluginEntitlement.ts
// 插件授权态 → gate 展示判定的映射（Part5 T12）。
//
// 前后端职责：本模块只把后端授权态映射为 UI 展示判定（是否显 gate / 购买引导），**不持任何验签逻辑**，
// 也不自持授权状态——取态的调用方（如 useExoticGate）负责状态，授权真相全在后端 EntitlementProvider。

import type { PluginEntitlement, Availability } from '../types/exotic'

/** 需要「购买 / 激活」引导的可用态（未授权但**有产品可领**）。其余为可运行或纯不可用（不引导购买）。 */
const GATED: ReadonlySet<Availability> = new Set<Availability>([
  'availableUninstalled',
  'installedUnlicensed',
  'licenseExpired',
])

/**
 * Gate 展示模式（`PluginGate.vue` 据此选择渲染分支）。
 * - `passthrough`：直接放行（无判定 / 无产品可售 → 不拦截，避免误藏功能）
 * - `authorized`：已授权 → 渲染真实功能
 * - `purchase`：未授权但有产品 → 显功能说明 + 购买/激活引导
 * - `blocked`：纯不可用（平台/版本/损坏/禁用）→ 只做信息提示，不引导购买
 */
export type GateMode = 'passthrough' | 'authorized' | 'purchase' | 'blocked'

/**
 * 把后端授权态映射为 gate 展示模式（纯函数，gate 逻辑的单一事实源）。
 * 对 `null` 与 `noOffering` 一律**放行**——gate 只在拿到明确「有产品但未授权」判定时才拦截，
 * 不确定时不藏功能（fail-open）。
 */
export function gateModeFor(e: PluginEntitlement | null): GateMode {
  if (!e) return 'passthrough'
  if (e.availability === 'authorized') return 'authorized'
  if (GATED.has(e.availability)) return 'purchase'
  if (e.availability === 'noOffering') return 'passthrough'
  return 'blocked'
}
