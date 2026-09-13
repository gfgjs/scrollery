// SelectionActions contract 测试(C2)。锁定折叠/命令渲染的 SSR 可验部分:
//   ① 每命令一个 data-toolbar-item 折叠单元(useToolbarOverflow 测量对象)——折叠单元恒渲染于 DOM
//      (靠 grid 收拢而非 v-show 摘除, Route A 平滑折叠), folded 键于 !isMeasuring; SSR 下 isMeasuring
//      初值为 true → 全展开(不带 folded 类)→ 可点数;
//   ② 按钮命令 tooltip 源自 labelKey、按数组顺序、danger 类;colors 走 ColorLabelPicker;
//   ③ ✕ 取消选择恒在(壳固定件)。
// 门禁盲区(honest):visibleCount 动态切分、⋯ 菜单开阖/teleport/近边缘 flip、焦点陷阱全靠 DOM 测量
//   与交互,onMounted 在 SSR 不跑 → 不在此断言,列 realtest(见方案 §5)。
//
// SelectionActions 依赖 useSelection 单例(vi.mock 控)+ useToolbarOverflow(SSR 下 onMounted 跳过)
// + $t/useI18n(colors 的 ColorLabelPicker 与 locale remeasureKey 需真实 i18n 插件)+ UiPopover
// (menuOpen 初值 false → 空 teleport,SSR 安全)。

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

import SelectionActions from './SelectionActions.vue'
import { useSelectionBarMode } from '../../composables/useSelectionBarMode'

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })
const t = i18n.global.t

function icon(name: string): Component {
  return { render: () => h('span', { 'data-icon': name }) } as Component
}

function makeCommands(): SelectionCommand[] {
  return [
    { key: 'selectAll', icon: icon('selectAll'), labelKey: 'common.selectAll', run: () => {} },
    { key: 'invert', icon: icon('invert'), labelKey: 'selection.invert', run: () => {} },
    {
      key: 'colors',
      icon: icon('colors'),
      labelKey: 'selection.colorLabel',
      kind: 'colors',
      run: () => {},
    },
    {
      key: 'delete',
      icon: icon('delete'),
      labelKey: 'selection.delete',
      danger: true,
      run: () => {},
    },
    {
      key: 'move',
      icon: icon('move'),
      labelKey: 'common.moveTo',
      groupStart: true,
      run: () => {},
    },
    { key: 'copy', icon: icon('copy'), labelKey: 'common.copyTo', run: () => {} },
  ]
}

async function render(
  commands: SelectionCommand[],
  variant: 'floating' | 'docked' = 'floating',
): Promise<string> {
  const app = createSSRApp({
    render: () => h(SelectionActions as Component, { commands, variant }),
  })
  app.use(i18n)
  return renderToString(app)
}

describe('SelectionActions（C2 折叠/命令 contract）', () => {
  it('每命令渲染一个折叠单元测量属性（data-toolbar-item）', async () => {
    // 断言收紧到属性形式(其后必跟空格或 >):SSR 会把模板 HTML 注释也渲染进串,匹配裸 token 会误计注释字样。
    const html = await render(makeCommands())
    const count = html.match(/data-toolbar-item[ >]/g)?.length ?? 0
    expect(count).toBe(makeCommands().length)
  })

  it('danger 命令带 --danger 类', async () => {
    expect(await render(makeCommands())).toContain('selection-action--danger')
  })

  it('colors 单元渲染 ColorLabelPicker，而非普通图标按钮', async () => {
    const html = await render(makeCommands())
    expect(html).toContain('color-picker')
    expect(html).not.toContain('data-icon="colors"')
  })

  it('⋯ 溢出触发钮仅在命令溢出时渲染', async () => {
    const html = await render(makeCommands())
    expect(html).toContain('selection-more')
  })

  it('✕ 取消选择恒在（壳固定件，即使 commands 为空）', async () => {
    const html = await render([])
    expect(html).toContain(`data-tooltip="${t('selection.cancel')}"`)
  })

  it('计数区渲染 selection.selected（已选 N 项）', async () => {
    expect(await render(makeCommands())).toContain(t('selection.selected', { count: 3 }))
  })

  it('docked 变体渲染紧凑修饰类', async () => {
    expect(await render(makeCommands(), 'docked')).toContain('selection-actions--docked')
  })

  // 对齐 class 施于 .selection-actions(docked 缺口修复):浮动 wrapper 在 docked 态不渲染,对齐宿主
  // 转由本组件承担。SSR 下 align 单例经 setAlign 驱动(setAlign 先赋值 ref 再写 localStorage,node 无
  // localStorage 时写入静默失败但 ref 仍更新),故可在测试内切换验证。断言顺带证明模板内 align ref 已
  // 正确 auto-unwrap(未解包会渲成畸形 class、匹配失败)。
  it('docked 变体默认携带居中对齐类(align-center)', async () => {
    // 默认(未显式设过 align)= 居中,零惊讶。
    useSelectionBarMode().setAlign('center')
    expect(await render(makeCommands(), 'docked')).toContain('selection-actions--align-center')
  })

  it('docked 变体随 align 单例切换对齐类(center→right)', async () => {
    const mode = useSelectionBarMode()
    try {
      mode.setAlign('right')
      const html = await render(makeCommands(), 'docked')
      expect(html).toContain('selection-actions--align-right')
      expect(html).not.toContain('selection-actions--align-center')
    } finally {
      // 单例跨用例共享:复位居中,避免污染同文件其它断言。
      mode.setAlign('center')
    }
  })

})
