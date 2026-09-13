// UiToggle contract 测试（S1）。复用 UiButton/UiIconButton 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp,无 @vue/test-utils）。开关的选中/滑动/焦点态全走纯 CSS,
// 无 JS 态可断,故此处验**结构契约**:根 .toggle label + 隐藏 checkbox + thumb、checked 映射 modelValue、
// disabled 透传、消费者 class fallthrough 合并到根。change→emit 属 DOM 事件,
// SSR 不可断(与 UiIconButton 不测 click 同),交由 v-model 结构保证。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiToggle from './UiToggle.vue'

async function render(props: Record<string, unknown> = { modelValue: false }): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiToggle as Component, props),
  })
  return renderToString(app)
}

describe('UiToggle（S1 原语 contract）', () => {
  it('渲染全局 .toggle 结构（label + 隐藏 checkbox + thumb）', async () => {
    const html = await render()
    expect(html).toContain('class="toggle"')
    expect(html).toContain('type="checkbox"')
    expect(html).toContain('toggle__thumb')
  })

  it('modelValue=true → input 带 checked（:has(:checked) 命中,开启外观）', async () => {
    expect(await render({ modelValue: true })).toContain('checked')
  })

  it('modelValue=false → input 不带 checked（关闭外观）', async () => {
    // 'checkbox' 不含子串 'checked',故此断言不被 type="checkbox" 误伤。
    expect(await render({ modelValue: false })).not.toContain('checked')
  })

  it('disabled=true → input 禁用', async () => {
    expect(await render({ modelValue: false, disabled: true })).toContain('disabled')
  })

  it('消费者 class fallthrough 合并到根 label（如 compact-toggle 作用域变体）', async () => {
    const app = createSSRApp({
      render: () => h(UiToggle as Component, { modelValue: false, class: 'compact-toggle' }),
    })
    const html = await renderToString(app)
    // 根 label 同时携带自身 .toggle 与消费者透传的 compact-toggle。
    expect(html).toContain('toggle')
    expect(html).toContain('compact-toggle')
  })
})
