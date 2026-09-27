// parseAppError 的「目录移动半完成」恢复定位契约（P0-1）。
//
// 关注点：move_db_pending 额外带的 recoveryId/targetAbsPath 必须**有类型地**保留下来——前端正是
// 靠它显示文件真实位置并按日志 id 重试。字段缺失或类型不符时不得凭猜补齐（猜出来的落点比没有更糟），
// 另外二次解析（IpcError → parseAppError）不得把定位丢掉，否则「登记恢复入口」会在中转处静默失效。
import { describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { moveRecoveryOf, parseAppError } from './ipc'

const PENDING_RAW = {
  code: 'move_db_pending',
  message: '文件已移动到新位置，索引尚未更新',
  recoveryId: 7,
  targetAbsPath: 'D:/archive/旅行',
}

describe('parseAppError：目录移动半完成的恢复定位', () => {
  it('move_db_pending 的 recoveryId/targetAbsPath 原样保留', () => {
    const err = parseAppError(PENDING_RAW)

    expect(err.code).toBe('move_db_pending')
    expect(err.message).toBe(PENDING_RAW.message)
    expect(err.recovery).toEqual({ recoveryId: 7, targetAbsPath: 'D:/archive/旅行' })
    expect(moveRecoveryOf(err)).toEqual({ recoveryId: 7, targetAbsPath: 'D:/archive/旅行' })
  })
})
