import { listen, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event'
import { isUiHarness } from '../harness/runtime'

/** browser harness 不存在 Rust event bus；监听降级为空解绑函数。 */
export function listenAppEvent<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  if (isUiHarness) return Promise.resolve(() => {})
  return listen<T>(event, handler)
}
