// parseAppError 的「目录移动半完成」恢复定位契约（P0-1）。
//
// 关注点：move_db_pending 额外带的 recoveryId/targetAbsPath 必须**有类型地**保留下来——前端正是
// 靠它显示文件真实位置并按日志 id 重试。字段缺失或类型不符时不得凭猜补齐（猜出来的落点比没有更糟），
// 另外二次解析（IpcError → parseAppError）不得把定位丢掉，否则「登记恢复入口」会在中转处静默失效。
import { describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { IpcError, moveRecoveryOf, parseAppError } from './ipc'

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

  it('字段缺失或类型不符时视作没有恢复定位', () => {
    const raws = [
      { code: 'move_db_pending', message: 'm', recoveryId: '7', targetAbsPath: 'D:/a' },
      { code: 'move_db_pending', message: 'm', recoveryId: 7.5, targetAbsPath: 'D:/a' },
      { code: 'move_db_pending', message: 'm', recoveryId: 7 },
      { code: 'move_db_pending', message: 'm' },
    ]

    for (const raw of raws) {
      expect(parseAppError(raw).recovery).toBeNull()
      expect(moveRecoveryOf(raw)).toBeNull()
    }
  })

  it('其它 code 不受影响：code/message 分流照旧，且不带恢复定位', () => {
    const err = parseAppError({
      code: 'DirectoryExists',
      message: '旅行',
      recoveryId: 7,
      targetAbsPath: 'D:/a',
    })

    expect(err.code).toBe('DirectoryExists')
    expect(err.recovery).toBeNull()
    expect(moveRecoveryOf(err)).toBeNull()
    // 直接构造也不放行：非 move_db_pending 带恢复定位只会变成「像有重试入口」的假象。
    expect(new IpcError('Io', 'm', { recoveryId: 7, targetAbsPath: 'D:/a' }).recovery).toBeNull()
  })

  it('二次解析不丢定位：IpcError 原样返回', () => {
    const first = new IpcError('move_db_pending', 'm', {
      recoveryId: 9,
      targetAbsPath: 'D:/x',
    })

    expect(parseAppError(first)).toBe(first)
    expect(moveRecoveryOf(first)?.recoveryId).toBe(9)
  })

  it('裸字符串与 Error 仍降级为 Unknown，且没有恢复定位', () => {
    expect(parseAppError('boom').code).toBe('Unknown')
    expect(moveRecoveryOf('boom')).toBeNull()
    expect(parseAppError(new Error('boom')).message).toBe('boom')
    expect(moveRecoveryOf(new Error('boom'))).toBeNull()
  })
})
