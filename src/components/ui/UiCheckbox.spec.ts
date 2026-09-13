// UiCheckbox contract 测试（S1）。复用 UiButton/UiIconButton/UiToggle 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp,无 @vue/test-utils）。选中态由原生 checkbox + accent-color 驱动,
// 无 JS 态可断,故验**结构契约**:根 .checkbox-field label（隐式关联 label 文本）+ native checkbox、
// checked 映射 modelValue、disabled 透传 + --disabled 类、label prop / 默认插槽渲染、class fallthrough。
// change→emit 属 DOM 事件,SSR 不可断（与 UiToggle 同），交由 v-model 结构保证。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiCheckbox from './UiCheckbox.vue'

async function render(
  props: Record<string, unknown> = { modelValue: false },
  slot?: string,
): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiCheckbox as Component, props, slot ? { default: () => slot } : undefined),
  })
  return renderToString(app)
}

describe('UiCheckbox（S1 原语 contract）', () => {
  it('渲染 .checkbox-field label + 原生 checkbox（label 包裹隐式关联）', async () => {
    const html = await render()
    expect(html).toContain('class="checkbox-field"')
    expect(html).toContain('type="checkbox"')
  })

  it('modelValue=true → input 带 checked', async () => {
    expect(await render({ modelValue: true })).toContain('checked')
  })

  it('modelValue=false → input 不带 checked', async () => {
    // 'checkbox' 不含子串 'checked',故此断言不被 type="checkbox" 误伤。
    expect(await render({ modelValue: false })).not.toContain('checked')
  })

  it('label prop → 渲染为标签文本', async () => {
    expect(await render({ modelValue: false, label: '记住选择' })).toContain('记住选择')
  })

  it('默认插槽优先于 label prop（更复杂内容）', async () => {
    expect(await render({ modelValue: false, label: 'X' }, '正则 .*')).toContain('正则 .*')
  })

  it('disabled=true → input 禁用 + --disabled 类', async () => {
    const html = await render({ modelValue: false, disabled: true })
    expect(html).toContain('disabled')
    expect(html).toContain('checkbox-field--disabled')
  })

  it('消费者 class fallthrough 合并到根 label（如 CloseConfirm 的 margin-top 类）', async () => {
    const app = createSSRApp({
      render: () => h(UiCheckbox as Component, { modelValue: false, class: 'close-remember' }),
    })
    const html = await renderToString(app)
    expect(html).toContain('checkbox-field')
    expect(html).toContain('close-remember')
  })
})
