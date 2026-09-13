// UiEmptyState contract 测试（S1）。复用 UiButton/UiField 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp,无 @vue/test-utils）。UiEmptyState 不 Teleport,renderToString 直取。
// 原语同构包裹全局 .empty-state* 类(视觉属全局 CSS + 真机),此处钉死**结构契约**:title 必渲染、
// description 条件渲染、icon/actions 插槽条件包裹进对应 BEM 容器、class 结构与既有内联空状态一致。
//
// ⚠ 断言一律用**属性形式** class="empty-state__X" 而非裸子串:SSR 会把模板 HTML 注释也渲染进串,
// 裸子串负向断言会被注释里出现的类名字面量误伤(uiux-refactor 会话续12 教训)。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component, type Slots } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiEmptyState from './UiEmptyState.vue'

async function render(props: Record<string, unknown>, slots?: Slots): Promise<string> {
  const app = createSSRApp({ render: () => h(UiEmptyState as Component, props, slots) })
  return await renderToString(app)
}

describe('UiEmptyState（S1 原语 contract）', () => {
  it('title 必渲染进 .empty-state__title,root 为 .empty-state', async () => {
    const html = await render({ title: '这里空空如也' })
    expect(html).toContain('class="empty-state"')
    expect(html).toContain('class="empty-state__title"')
    expect(html).toContain('这里空空如也')
  })

  it('description 存在→渲染 .empty-state__desc;缺省→不渲染', async () => {
    const withDesc = await render({ title: 'x', description: '说明文字' })
    expect(withDesc).toContain('class="empty-state__desc"')
    expect(withDesc).toContain('说明文字')
    expect(await render({ title: 'x' })).not.toContain('class="empty-state__desc"')
  })

  it('icon 插槽存在→包进 .empty-state__icon;缺省→不渲染图标容器', async () => {
    const html = await render({ title: 'x' }, {
      icon: () => h('svg', { 'data-i': 'ic' }),
    } as unknown as Slots)
    expect(html).toContain('class="empty-state__icon"')
    expect(html).toContain('data-i="ic"')
    expect(await render({ title: 'x' })).not.toContain('class="empty-state__icon"')
  })

  it('actions 插槽存在→包进 .empty-state__actions;缺省→不渲染动作区', async () => {
    const html = await render({ title: 'x' }, {
      actions: () => h('button', 'A'),
    } as unknown as Slots)
    expect(html).toContain('class="empty-state__actions"')
    expect(html).toContain('>A</button>')
    expect(await render({ title: 'x' })).not.toContain('class="empty-state__actions"')
  })

  it('仅 title(无 icon/desc/actions):最小空状态只有标题一枝', async () => {
    const html = await render({ title: '仅标题' })
    expect(html).toContain('class="empty-state__title"')
    expect(html).not.toContain('class="empty-state__icon"')
    expect(html).not.toContain('class="empty-state__desc"')
    expect(html).not.toContain('class="empty-state__actions"')
  })
})
