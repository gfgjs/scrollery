// SelectionToolbar contract 测试(C1→C2)。C2 后本组件收敛为**浮动壳**:拖拽手柄 + 委托
// SelectionActions 渲染动作簇。命令级 parity(顺序/tooltip/danger/colors)已下沉到
// SelectionActions.spec;本 spec 只验壳职责:拖拽手柄在位、动作簇被正确委托(命令与 ✕ 透过子组件出现)。
// SSR 断言范式(@vue/server-renderer + createSSRApp)。useSelection 单例 vi.mock,i18n 真实装配。

import { describe, it, expect, vi } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createI18n } from 'vue-i18n'
import zhCN from '../../i18n/locales/zh-CN'
import type { SelectionCommand } from '../../types/selectionCommand'

vi.mock('../../composables/useSelection', async () => {
  const { ref } = await import('vue')
  return {
    useSelection: () => ({
      isSelectionMode: ref(true),
      selectedCount: ref(3),
      clearSelection: () => {},
    }),
  }
})

import SelectionToolbar from './SelectionToolbar.vue'

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })
const t = i18n.global.t

function icon(name: string): Component {
  return { render: () => h('span', { 'data-icon': name }) } as Component
}

function makeCommands(): SelectionCommand[] {
  return [
    { key: 'delete', icon: icon('delete'), labelKey: 'selection.delete', danger: true, run: () => {} },
    { key: 'copy', icon: icon('copy'), labelKey: 'common.copyTo', run: () => {} },
  ]
}

async function render(commands: SelectionCommand[]): Promise<string> {
  const app = createSSRApp({
    render: () => h(SelectionToolbar as Component, { commands }),
  })
  app.use(i18n)
  return renderToString(app)
}

describe('SelectionToolbar（C2 浮动壳 contract）', () => {
  it('渲染拖拽手柄（drag 提示 title）', async () => {
    expect(await render(makeCommands())).toContain(`title="${t('selection.drag')}"`)
  })

  it('胶囊壳存在（selection-toolbar 根类）', async () => {
    expect(await render(makeCommands())).toContain('selection-toolbar')
  })

  it('委托 SelectionActions:命令 tooltip 透过子组件出现', async () => {
    const html = await render(makeCommands())
    expect(html).toContain(`data-tooltip="${t('selection.delete')}"`)
    expect(html).toContain(`data-tooltip="${t('common.copyTo')}"`)
  })

  it('委托 SelectionActions:✕ 取消选择与计数出现', async () => {
    const html = await render(makeCommands())
    expect(html).toContain(`data-tooltip="${t('selection.cancel')}"`)
    expect(html).toContain(t('selection.selected', { count: 3 }))
  })
})
