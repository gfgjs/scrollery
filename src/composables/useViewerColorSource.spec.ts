import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'

type InvokeHandler = (cmd: string, args: unknown) => unknown
const { state } = vi.hoisted(() => ({ state: { handler: (() => null) as InvokeHandler } }))
const calls: Array<{ cmd: string; args: unknown }> = []
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: unknown) => {
    calls.push({ cmd, args })
    return state.handler(cmd, args)
  },
  convertFileSrc: (p: string) => `asset://${p}`,
}))

import { useViewerColorSource } from './useViewerColorSource'

type Detail = { id: number; mediaType: string } | null

beforeEach(() => {
  calls.length = 0
  state.handler = () => null
})

describe('useViewerColorSource', () => {
  it('target=srgb 不发起 IPC,displayUrl 恒 null', async () => {
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('srgb')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    expect(calls).toEqual([])
    expect(displayUrl.value).toBeNull()
  })

  it('target=display-p3 换源成功', async () => {
    // 用 Windows 风格绝对路径钉住路径归一与 convertFileSrc 接线；Unix 绝对路径另由
    // assetUrl.spec.ts 覆盖，二者都必须经 asset protocol。
    state.handler = () => 'C:/cache/viewer_color/display-p3/ab/xxxx.jpg'
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    await vi.waitFor(() => expect(displayUrl.value).not.toBeNull())
    expect(calls).toEqual([{ cmd: 'get_viewer_color_url', args: { itemId: 1 } }])
    expect(displayUrl.value).toBe('asset://C:/cache/viewer_color/display-p3/ab/xxxx.jpg')
  })

  it('请求中途切换查看项,迟到响应被丢弃(过期守卫),新结果换源成功', async () => {
    const resolvers: Array<(v: string) => void> = []
    state.handler = () => new Promise((resolve) => resolvers.push(resolve))
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    // 切到另一查看项:令牌推进,第二次请求同样悬挂(handler 未 resolve)。
    detail.value = { id: 2, mediaType: 'image' }
    await nextTick()
    resolvers[0]('C:/cache/stale.jpg') // 第一次(id1,已过期)迟到应答
    await nextTick()
    await nextTick()
    expect(displayUrl.value).toBeNull() // 过期响应被丢弃,不应换源
    resolvers[1]('C:/cache/fresh.jpg') // 第二次(id2,当前令牌)应答
    await vi.waitFor(() => expect(displayUrl.value).not.toBeNull())
    expect(displayUrl.value).toBe('asset://C:/cache/fresh.jpg')
  })

  it('custom target 且 customId 为空:不发 IPC,displayUrl 恒 null', async () => {
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('custom')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    expect(calls).toEqual([])
    expect(displayUrl.value).toBeNull()
  })

  it('image→image 切项瞬间 displayUrl 立即归 null(原图先显)', async () => {
    const resolvers: Array<(v: string) => void> = []
    state.handler = () => new Promise((resolve) => resolvers.push(resolve))
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    resolvers[0]('/first.jpg')
    await nextTick()
    await vi.waitFor(() => expect(displayUrl.value).not.toBeNull())
    // 切到新项:immediate 同步生效——displayUrl 应立即落回 null,不等待新 IPC 返回。
    detail.value = { id: 2, mediaType: 'image' }
    await nextTick()
    expect(displayUrl.value).toBeNull()
  })

  it('IPC 失败静默回退原图', async () => {
    state.handler = () => {
      throw { code: 'System', message: 'boom' }
    }
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    await vi.waitFor(() => expect(calls.length).toBe(1))
    expect(displayUrl.value).toBeNull()
  })

  it('移动端恒不发起 IPC(D-414 平台门控)', async () => {
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: true,
    })
    await nextTick()
    expect(calls).toEqual([])
    expect(displayUrl.value).toBeNull()
  })

  it('handleDisplayUrlError:显示派生源时复位并返回 true,原图时返回 false', async () => {
    state.handler = () => 'C:/cache/viewer_color/display-p3/ab/xxxx.jpg'
    const detail = ref<Detail>({ id: 1, mediaType: 'image' })
    const target = ref('display-p3')
    const customId = ref('')
    const { displayUrl, handleDisplayUrlError } = useViewerColorSource({
      detail: () => detail.value,
      target: () => target.value,
      customId: () => customId.value,
      isMobile: false,
    })
    await nextTick()
    await vi.waitFor(() => expect(displayUrl.value).not.toBeNull())
    expect(handleDisplayUrlError()).toBe(true)
    expect(displayUrl.value).toBeNull()
    expect(handleDisplayUrlError()).toBe(false)
  })
})
