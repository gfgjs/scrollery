// UiDialog contract 测试（S1）。复用 UiButton/UiIconButton 确立的 SSR 断言范式
// （@vue/server-renderer + createSSRApp,无 @vue/test-utils）。
// 关键处理:UiDialog 恒 Teleport 到 body,renderToString 默认不把 teleport 内容放进返回串——
// 须传 context 二参,teleport 内容落在 context.teleports 里,断言前须合并二者。
// 重点验结构契约:open 门控、dialog 结构、关闭键 title、showClose 门控、
// maxWidth/bodyPadding 特化、body/footer/header 插槽投影。
// (焦点陷阱/Escape/点遮罩关闭 + closeOnOverlay/closeOnEsc 门控属交互运行期行为,SSR 不触 DOM,归真机验收
//  ——见 useFocusTrap 注释同款取舍。)

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component, type Slots } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiDialog from './UiDialog.vue'

async function render(
  props: Record<string, unknown>,
  slots?: Record<string, () => unknown>,
): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiDialog as Component, props, slots as unknown as Slots),
  })
  // context.teleports 收集 <Teleport to="body"> 的产物,key 为目标选择器。
  const ctx: { teleports?: Record<string, string> } = {}
  const main = await renderToString(app, ctx)
  return main + Object.values(ctx.teleports ?? {}).join('')
}

const base = { open: true, title: '确认删除' }

describe('UiDialog（S1 原语 contract）', () => {
  it('open=false → 不渲染任何对话框结构', async () => {
    const html = await render({ open: false, title: 'x' })
    expect(html).not.toContain('dialog-content')
  })

  it('open=true → 渲染遮罩与 content', async () => {
    const html = await render(base)
    expect(html).toContain('dialog-overlay')
    expect(html).toContain('dialog-content')
  })

  it('title 渲染进 <h2>', async () => {
    expect(await render(base)).toContain('确认删除')
  })

  it('closeLabel → 关闭键 title 写入', async () => {
    expect(await render({ ...base, closeLabel: '取消' })).toContain('title="取消"')
  })

  it('closeLabel 缺省回落 Close', async () => {
    expect(await render(base)).toContain('title="Close"')
  })

  it('showClose=false → 不渲染关闭键', async () => {
    const html = await render({ ...base, showClose: false })
    expect(html).not.toContain('btn-close')
  })

  it('maxWidth → content 内联 max-width 特化', async () => {
    expect(await render({ ...base, maxWidth: '460px' })).toContain('max-width:460px')
  })

  it('maxHeight → content 内联 max-height + dialog-content--capped 类(正文可滚/头脚固定);缺省不加该类', async () => {
    const html = await render({ ...base, maxHeight: '84vh' })
    expect(html).toContain('max-height:84vh')
    expect(html).toContain('dialog-content--capped')
    expect(await render(base)).not.toContain('dialog-content--capped')
  })

  it('floatSurface 仅为 true 时添加 dialog-content--float 修饰类', async () => {
    expect(await render({ ...base, floatSurface: true })).toContain('dialog-content--float')
    expect(await render({ ...base, floatSurface: false })).not.toContain('dialog-content--float')
    expect(await render(base)).not.toContain('dialog-content--float')
  })

  it('bodyPadding → dialog-body 内联 padding 特化(满幅列表/树用);缺省则不产内联 padding', async () => {
    expect(await render({ ...base, bodyPadding: '0' })).toContain('padding:0')
    expect(await render(base)).not.toContain('padding:0')
  })

  it('#header 插槽 → 整体覆盖默认标题行(副标题/步骤点等自定义头部用);默认 title/关闭键均不渲染', async () => {
    const html = await render(base, { header: () => 'CUSTOM_HEADER' })
    expect(html).toContain('CUSTOM_HEADER')
    expect(html).not.toContain('确认删除') // base.title 的默认 <h2> 被整体覆盖
    expect(html).not.toContain('dialog-title')
    expect(html).not.toContain('btn-close')
  })

  it('默认插槽 → 投影进 dialog-body', async () => {
    const html = await render(base, { default: () => 'BODY_MARKER' })
    expect(html).toContain('dialog-body')
    expect(html).toContain('BODY_MARKER')
  })

  it('提供 #footer → 渲染 dialog-footer 并投影内容', async () => {
    const html = await render(base, { footer: () => 'FOOTER_MARKER' })
    expect(html).toContain('dialog-footer')
    expect(html).toContain('FOOTER_MARKER')
  })

  it('无 #footer → 不渲染 dialog-footer(避免空分隔线)', async () => {
    expect(await render(base)).not.toContain('dialog-footer')
  })
})
