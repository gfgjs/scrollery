// useConfirm 契约测试:验证 danger / requireText 选项正确进入共享 state,且默认值不破坏既有调用。
// 背景:危险操作确认是数据丢失的最后一道门——真机事故中原生 window.confirm 在 Tauri webview 静默穿透
// (不弹框却返回 truthy),危险区四项无提示直接执行。修复=全部改走本 useConfirm 单例 + ConfirmDialog,
// 并新增 danger(红色确认按钮)/requireText(输入确认强门)。故对确认 state 层做特征化测试锁定契约。
import { describe, it, expect } from 'vitest'
import { useConfirm, useConfirmDialogState } from './useConfirm'

describe('useConfirm — 危险确认选项(danger / requireText)', () => {
  const { confirm } = useConfirm()
  const { state, close } = useConfirmDialogState()

  it('confirm 打开对话框并把 danger / requireText 写入共享 state', () => {
    void confirm({ title: 't', message: 'm', danger: true, requireText: 'DELETE' })
    expect(state.isOpen).toBe(true)
    expect(state.danger).toBe(true)
    expect(state.requireText).toBe('DELETE')
    close(false)
  })

  it('省略时 danger 默认 false、requireText 默认空串(不破坏既有 confirm 调用)', () => {
    void confirm({ title: 't', message: 'm' })
    expect(state.danger).toBe(false)
    expect(state.requireText).toBe('')
    close(false)
  })

  it('close(true)/close(false) 各自解析 confirmed 布尔并关闭', async () => {
    const p1 = confirm({ title: 't', message: 'm' })
    close(true)
    await expect(p1).resolves.toMatchObject({ confirmed: true })

    const p2 = confirm({ title: 't', message: 'm' })
    close(false)
    await expect(p2).resolves.toMatchObject({ confirmed: false })
    expect(state.isOpen).toBe(false)
  })
})
