// cardPointerAction.ts 的分流决策穷举单测。
// 交互路由是回归高发区(右键拖拽曾在此被误伤消失),此前零单测——本组穷举「按钮×已选中×手柄」
// 与「镜头态×修饰键×选择态×布局形态」两个矩阵钉死。

import { describe, it, expect } from 'vitest'
import { resolveCardPointerAction, resolveCardClickAction } from './cardPointerAction'

const LEFT = 0
const MIDDLE = 1
const RIGHT = 2

describe('resolveCardPointerAction（卡片 pointerdown 分流,穷举）', () => {
  it('左键 + 已选中 + 手柄 → drag(拖到文件夹)', () => {
    expect(resolveCardPointerAction(LEFT, true, true)).toBe('drag')
  })

  it('左键 + 已选中 + 本体(非手柄) → sweep(反转扫选,让位给手柄)', () => {
    expect(resolveCardPointerAction(LEFT, true, false)).toBe('sweep')
  })

  it('左键 + 未选中 + 本体 → sweep', () => {
    expect(resolveCardPointerAction(LEFT, false, false)).toBe('sweep')
  })

  it('左键 + 未选中 + 手柄(入参鲁棒:未选中本不渲染手柄) → 仍 sweep', () => {
    // 手柄仅在已选中格渲染,onSelectedItem=false 时 onHandle 理论上不为真;
    // 纯函数仍须鲁棒:左键非 drag 一律 sweep。
    expect(resolveCardPointerAction(LEFT, false, true)).toBe('sweep')
  })

  it('右键 + 已选中 + 本体 → drag(保持旧行为:右键整卡可拖=落点菜单)', () => {
    expect(resolveCardPointerAction(RIGHT, true, false)).toBe('drag')
  })

  it('右键 + 已选中 + 手柄 → drag', () => {
    expect(resolveCardPointerAction(RIGHT, true, true)).toBe('drag')
  })

  it('右键 + 未选中 → none(不处理,交给常规右键菜单)', () => {
    expect(resolveCardPointerAction(RIGHT, false, false)).toBe('none')
    expect(resolveCardPointerAction(RIGHT, false, true)).toBe('none')
  })

  it('中键(button 1) → none(既非左键扫选也非右键拖拽)', () => {
    expect(resolveCardPointerAction(MIDDLE, true, true)).toBe('none')
    expect(resolveCardPointerAction(MIDDLE, false, false)).toBe('none')
  })
})

// 镜头态 browse-only(§8.1)是本函数的优先分支:镜头激活时任何修饰键组合都不得产生选区。
describe('resolveCardClickAction（卡片 click 分流:§8.1 browse-only + 既有语义,穷举）', () => {
  it('镜头态:Ctrl/Cmd+Click 不切选中,一律 open(不产生选区)', () => {
    expect(resolveCardClickAction({ lensActive: true, modifier: true, shift: false, selectionMode: false, compact: false })).toBe('open')
    expect(resolveCardClickAction({ lensActive: true, modifier: true, shift: false, selectionMode: true, compact: false })).toBe('open')
    expect(resolveCardClickAction({ lensActive: true, modifier: true, shift: false, selectionMode: false, compact: true })).toBe('open')
  })

  it('镜头态:Shift+Click 不范围选择,一律 open', () => {
    expect(resolveCardClickAction({ lensActive: true, modifier: false, shift: true, selectionMode: true, compact: false })).toBe('open')
    expect(resolveCardClickAction({ lensActive: true, modifier: false, shift: true, selectionMode: true, compact: true })).toBe('open')
  })

  it('镜头态:普通单击 open(不进入选择态/不切选中)', () => {
    expect(resolveCardClickAction({ lensActive: true, modifier: false, shift: false, selectionMode: false, compact: false })).toBe('open')
    expect(resolveCardClickAction({ lensActive: true, modifier: false, shift: false, selectionMode: true, compact: true })).toBe('open')
  })

  it('非镜头:Ctrl/Cmd+Click → toggle(优先级最高,选择态与否皆然)', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: true, shift: false, selectionMode: false, compact: false })).toBe('toggle')
    expect(resolveCardClickAction({ lensActive: false, modifier: true, shift: true, selectionMode: true, compact: false })).toBe('toggle')
  })

  it('非镜头:选择态 + Shift+Click → range', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: true, selectionMode: true, compact: false })).toBe('range')
  })

  it('非镜头:非选择态 + Shift+Click → open(Shift 无选择语义)', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: true, selectionMode: false, compact: false })).toBe('open')
  })

  it('非镜头:选择态 + compact → select(普通点击替代点不动的 checkbox)', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: false, selectionMode: true, compact: true })).toBe('select')
  })

  it('非镜头:选择态 + 非 compact → open(点击开图,checkbox 才选择)', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: false, selectionMode: true, compact: false })).toBe('open')
  })

  it('非镜头:非选择态普通点击 → open', () => {
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: false, selectionMode: false, compact: false })).toBe('open')
    expect(resolveCardClickAction({ lensActive: false, modifier: false, shift: false, selectionMode: false, compact: true })).toBe('open')
  })
})
