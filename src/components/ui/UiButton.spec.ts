// UiButton contract 测试（S1）。项目无 @vue/test-utils,改用 @vue/server-renderer 的 renderToString
// 建立原语 contract 测试范式:SSR 渲染后断言 class 变体映射、loading/disabled 契约、插槽。
// UiButton 不依赖 store/router/i18n,故 createSSRApp 直接挂载即可,无 window 依赖。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiButton from './UiButton.vue'

async function render(
  props: Record<string, unknown> = {},
  slot?: string,
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(UiButton as Component, props, slot ? { default: () => slot } : undefined),
  })
  return renderToString(app)
}

describe('UiButton（S1 原语 contract）', () => {
  it('默认 variant=secondary → class 含 btn 与 btn-secondary', async () => {
    const html = await render()
    expect(html).toContain('btn')
    expect(html).toContain('btn-secondary')
  })

  it.each(['primary', 'secondary', 'danger', 'ghost'] as const)(
    'variant=%s → 对应 btn-%s',
    async (variant) => {
      expect(await render({ variant })).toContain(`btn-${variant}`)
    },
  )

  it('默认 type=button（避免 form 内误提交）', async () => {
    expect(await render()).toContain('type="button"')
  })

  it('type=submit 透传', async () => {
    expect(await render({ type: 'submit' })).toContain('type="submit"')
  })

  it('loading → 禁用并渲染 spinner', async () => {
    const html = await render({ loading: true })
    expect(html).toContain('disabled')
    expect(html).toContain('ui-btn__spinner')
  })

  it('disabled → 禁用属性', async () => {
    expect(await render({ disabled: true })).toContain('disabled')
  })

  it('非 loading 时不渲染 spinner', async () => {
    expect(await render()).not.toContain('ui-btn__spinner')
  })

  it('渲染默认插槽内容', async () => {
    expect(await render({}, '保存设置')).toContain('保存设置')
  })
})
