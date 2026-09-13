// UiIconButton contract 测试（S1）。复用 UiButton 确立的 SSR 断言范式（@vue/server-renderer +
// createSSRApp,无 @vue/test-utils）。重点验 label→title、active→.active 类、title 可覆盖。
// UiIconButton 无 store/router/i18n 依赖,直接挂载。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiIconButton from './UiIconButton.vue'

async function render(
  props: Record<string, unknown> = { label: '缩放' },
  slot?: string,
): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiIconButton as Component, props, slot ? { default: () => slot } : undefined),
  })
  return renderToString(app)
}

describe('UiIconButton（S1 原语 contract）', () => {
  it('渲染全局 btn-icon 类', async () => {
    expect(await render()).toContain('btn-icon')
  })

  it('label 必填 → title 缺省回落 label', async () => {
    expect(await render({ label: '放大' })).toContain('title="放大"')
  })

  it('title 显式覆盖(如附快捷键)', async () => {
    const html = await render({ label: '信息', title: '信息 (I)' })
    expect(html).toContain('title="信息 (I)"')
    expect(html).not.toContain('title="信息"')
  })

  it('active=true → 含 .active 类', async () => {
    expect(await render({ label: '收藏', active: true })).toContain('active')
  })

  it('默认 type=button（避免 form 内误提交）', async () => {
    expect(await render()).toContain('type="button"')
  })

  it('disabled → 禁用属性', async () => {
    expect(await render({ label: 'x', disabled: true })).toContain('disabled')
  })

  it('渲染默认插槽(图标)内容', async () => {
    expect(await render({ label: 'x' }, '★')).toContain('★')
  })
})
