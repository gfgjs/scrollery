// UiField contract 测试（S1）。复用 UiButton/UiDialog 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp，无 @vue/test-utils）。UiField 不 Teleport，故 renderToString 直取。
// 重点验结构契约：nest vs for 两关联模式、控件插槽收到 id、hint/error 渲染与
// orientation 朝向。
// （字段实际交互/视觉属真机；此处钉死结构与关联契约。）

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component, type Slots } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiField from './UiField.vue'

type FieldSlotProps = { id?: string }

async function render(
  props: Record<string, unknown>,
  defaultSlot?: (p: FieldSlotProps) => unknown,
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(
        UiField as Component,
        props,
        defaultSlot ? ({ default: defaultSlot } as unknown as Slots) : undefined,
      ),
  })
  return await renderToString(app)
}

describe('UiField（S1 原语 contract）', () => {
  it('nest 模式(默认):label 包裹 caption+控件(隐式关联),caption 为 span 且无 for', async () => {
    const html = await render({ label: '名称' }, () => h('input'))
    expect(html).toContain('<label class="ui-field__group')
    expect(html).toContain('ui-field__label')
    expect(html).toContain('名称')
    expect(html).toContain('<input')
    expect(html).not.toContain('for="') // nest 模式无显式 for
  })

  it('for 模式:group 为 div,label[for] 与控件[id] 显式关联', async () => {
    const html = await render({ label: '主题', associate: 'for', fieldId: 'fx' }, (p) =>
      h('select', { id: p.id }),
    )
    expect(html).toContain('<div class="ui-field__group')
    expect(html).toContain('for="fx"')
    expect(html).toContain('id="fx"') // 插槽控件收到 id
  })

  it('orientation:缺省 stacked,inline 切横排', async () => {
    expect(await render({ label: 'x' })).toContain('ui-field__group--stacked')
    expect(await render({ label: 'x', orientation: 'inline' })).toContain('ui-field__group--inline')
  })

  it('hint:渲染 ui-field__hint 与派生 id', async () => {
    const html = await render(
      { label: 'x', associate: 'for', fieldId: 'fx', hint: '帮助文字' },
      () => h('input'),
    )
    expect(html).toContain('ui-field__hint')
    expect(html).toContain('id="fx-hint"')
    expect(html).toContain('帮助文字')
  })

  it('error:渲染 ui-field__error、错误 id 与 root 错误类', async () => {
    const html = await render({ label: 'x', fieldId: 'fx', error: '必填' }, () => h('input'))
    expect(html).toContain('ui-field--error')
    expect(html).toContain('ui-field__error')
    expect(html).toContain('id="fx-error"')
    expect(html).toContain('必填')
  })

  it('hint+error 并存:两个派生 id 均渲染', async () => {
    const html = await render({ label: 'x', fieldId: 'fx', hint: 'h', error: 'e' }, () => h('input'))
    expect(html).toContain('id="fx-hint"')
    expect(html).toContain('id="fx-error"')
  })

  it('无 hint/error:不渲染 hint/error 元素', async () => {
    const html = await render({ label: 'x', fieldId: 'fx' }, () => h('input'))
    expect(html).not.toContain('ui-field__hint')
    expect(html).not.toContain('ui-field__error')
  })
})
