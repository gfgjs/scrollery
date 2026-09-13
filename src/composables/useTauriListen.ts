// useTauriListen —— 安全监听 Tauri 事件，消除「await listen 卸载竞态」(审查 P1-11)。
//
// 竞态机理:旧写法 `onMounted(async () => { unlisten = await listen(...) })` 配
// `onBeforeUnmount(() => unlisten?.())` 存在时序错配——组件在 listen() 的 promise
// 落定前卸载(如画廊↔设置路由切换每次重建 MediaGrid)时,onBeforeUnmount 先跑,
// 此刻 unlisten 仍为 null(空操作);随后 promise 落定把活句柄写入变量却再无人调用
// → 监听器永久泄漏,之后每次事件都在已死组件的闭包上重跑 compute/updateVisible。
//
// 修法:引入 disposed 标志作时序仲裁——谁后到谁负责收尾。listen 落定时若作用域
// 已销毁,就地解绑;否则存句柄,待作用域销毁时统一解绑。清理锚点用 onScopeDispose
// (绑定当前 effect scope)而非 onBeforeUnmount,使本 composable 既能服务组件也能
// 服务其它拥有 effect scope 的 composable。

import { onScopeDispose } from 'vue'
import type { EventCallback, UnlistenFn } from '@tauri-apps/api/event'
import { listenAppEvent } from '../utils/appEvents'
import { EVENTS } from '../constants/ipc'

/** 所有已登记 Tauri 事件名的联合类型(EVENTS 常量的值)。仅接受此类型 → 杜绝裸事件字符串(承接 F12)。 */
export type AppEvent = (typeof EVENTS)[keyof typeof EVENTS]

/**
 * 安全监听一个 Tauri 事件,作用域销毁时自动解绑,且不受「await listen 卸载竞态」影响。
 *
 * 必须在组件 `setup`(或拥有 effect scope 的 composable)的**同步**上下文中调用,
 * 以便 onScopeDispose 绑定到正确的作用域;不要放进 `onMounted` 等异步回调内。
 *
 * @param event 事件名(必须取自 EVENTS 常量;裸字符串被类型层拒绝)
 * @param handler 事件回调(可忽略 payload,`() => void` 亦可赋值)
 */
export function useTauriListen<T = unknown>(event: AppEvent, handler: EventCallback<T>): void {
  let unlisten: UnlistenFn | null = null
  let disposed = false

  void listenAppEvent<T>(event, handler)
    .then((un) => {
      // 若 listen 落定前作用域已销毁:就地解绑,避免泄漏;否则存句柄待销毁时解绑。
      if (disposed) un()
      else unlisten = un
    })
    .catch(() => {
      // listen 失败(罕见,如 IPC 早期尚不可用):无句柄可解绑,静默不阻断组件初始化。
    })

  onScopeDispose(() => {
    disposed = true
    unlisten?.()
    unlisten = null
  })
}
