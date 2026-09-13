// UiPopover contract 测试（S1）。复用 UiDialog 确立的 Teleport-SSR 断言范式
// （@vue/server-renderer + createSSRApp，无 @vue/test-utils）：UiPopover 恒 Teleport 到 body，
// renderToString 默认不把 teleport 内容放进返回串——须传 context 二参并合并 context.teleports。
// 钉死**结构契约**：open 门控、backdrop + 定位壳存在性、默认插槽投影、tabindex 焦点兜底。
// (定位数学 flip/shift/autoUpdate、焦点陷阱、backdrop/Esc dismiss 属交互运行期行为，SSR 不触 DOM，
//  归真机验收——见 useFocusTrap 注释同款取舍。)
//
// ⚠ 断言一律用**属性形式** class="ui-popover"(带闭引号)而非裸子串：backdrop 的 class="ui-popover__backdrop"
// 含 `ui-popover` 前缀，带闭引号才能精确区分定位壳与 backdrop(uiux-refactor 会话续12 SSR 注释教训同源)。

import { describe, it, expect } from 'vitest'
import { createSSRApp, h, type Component, type Slots } from 'vue'
import { renderToString } from '@vue/server-renderer'
import UiPopover from './UiPopover.vue'

async function render(
  props: Record<string, unknown>,
  slots?: Record<string, () => unknown>,
): Promise<string> {
  const app = createSSRApp({
    render: () => h(UiPopover as Component, props, slots as unknown as Slots),
  })
  const ctx: { teleports?: Record<string, string> } = {}
  const main = await renderToString(app, ctx)
  return main + Object.values(ctx.teleports ?? {}).join('')
}

const base = { open: true, anchor: null }

describe('UiPopover（S1 原语 contract）', () => {
  it('open=false → 不渲染 backdrop 或定位壳', async () => {
    const html = await render({ open: false, anchor: null })
    expect(html).not.toContain('class="ui-popover"')
    expect(html).not.toContain('class="ui-popover__backdrop"')
  })

  it('open=true → 渲染 backdrop + 定位壳', async () => {
    const html = await render(base)
    expect(html).toContain('class="ui-popover__backdrop"')
    expect(html).toContain('class="ui-popover"')
  })

  it('默认插槽 → 投影进定位壳', async () => {
    const html = await render(base, { default: () => 'POPOVER_BODY' })
    expect(html).toContain('class="ui-popover"')
    expect(html).toContain('POPOVER_BODY')
  })

  it('定位壳带 tabindex="-1"(无可聚焦内容时焦点兜底钉在壳上)', async () => {
    expect(await render(base)).toContain('tabindex="-1"')
  })
})
