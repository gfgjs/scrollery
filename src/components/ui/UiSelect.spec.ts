// UiSelect contract 测试（S1）。复用 UiButton/UiIconButton/UiToggle/UiCheckbox 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp,无 @vue/test-utils）。选项的选中态由 vModelSelect 指令在
// 运行期 mounted 钩子设置（依赖真实 DOM),SSR 不可断,故此处验**结构契约**:根 .select-wrap div +
// .select select、默认插槽 option 转发进 select 内部、disabled 透传、
// 消费者 class fallthrough 合并到根 .select-wrap（这正是 compact-select-wrap 迁移后仍命中 wrap 的机制）。
// change→emit 与 option 选中属运行期行为,SSR 不可断（与 UiToggle 不断 thumb 位同),交由 v-model 结构保证。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiSelect from './UiSelect.vue'

// 选项默认插槽（vnode 工厂,渲染真实 <option> 而非转义文本）。
const opts = () => [h('option', { value: 'a' }, 'Alpha'), h('option', { value: 'b' }, 'Beta')]

async function render(
  props: Record<string, unknown> = { modelValue: '' },
  slotFn: () => unknown = opts,
): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiSelect as Component, props, { default: slotFn }),
  })
  return renderToString(app)
}

describe('UiSelect（S1 原语 contract）', () => {
  it('渲染全局 .select-wrap 结构（wrap div + .select select）', async () => {
    const html = await render()
    expect(html).toContain('class="select-wrap"')
    expect(html).toContain('class="select"')
    expect(html).toContain('<select')
  })

  it('默认插槽 option 转发进 select 内部', async () => {
    const html = await render()
    expect(html).toContain('value="a"')
    expect(html).toContain('Alpha')
  })

  it('多选项插槽完整转发（非截断）', async () => {
    const html = await render()
    expect(html).toContain('Alpha')
    expect(html).toContain('Beta')
  })

  it('disabled=true → select 禁用', async () => {
    expect(await render({ modelValue: '', disabled: true })).toContain('disabled')
  })

  it('消费者 class fallthrough 合并到根 .select-wrap（compact-select-wrap 作用域变体）', async () => {
    const app = createSSRApp({
      render: () =>
        h(
          UiSelect as Component,
          { modelValue: '', class: 'compact-select-wrap' },
          { default: opts },
        ),
    })
    const html = await renderToString(app)
    // 根 .select-wrap 同时携带自身类与消费者透传的 compact-select-wrap;
    // 这是迁移后 `.compact-select-wrap :deep(.select)` 仍能锚到 wrap 的结构前提。
    expect(html).toContain('select-wrap')
    expect(html).toContain('compact-select-wrap')
  })
})
