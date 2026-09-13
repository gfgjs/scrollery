import { describe, expect, it } from 'vitest'
import { backupErrorKey, parentDirectoryPath, restoreErrorKey } from './backupPresentation'

describe('备份/恢复稳定错误码呈现', () => {
  it('安全关键恢复错误各自映射专属文案', () => {
    expect(restoreErrorKey('restore_schema_too_new')).toBe('backup.restoreErrorSchema')
    expect(restoreErrorKey('restore_corrupt')).toBe('backup.restoreErrorCorrupt')
    expect(restoreErrorKey('restore_document_missing')).toBe('backup.restoreErrorDocument')
    expect(restoreErrorKey('restore_size_limit')).toBe('backup.restoreErrorSpace')
  })

  it('未知码走安全兜底，不展示后端原始 message', () => {
    expect(restoreErrorKey('new_backend_code')).toBe('backup.restoreErrorUnknown')
    expect(backupErrorKey(undefined)).toBe('backup.errorUnknown')
  })

  it('打开备份包所在目录时保留跨平台根路径语义', () => {
    expect(parentDirectoryPath('C:\\Backups\\one.scrollerybackup')).toBe('C:\\Backups')
    expect(parentDirectoryPath('C:\\one.scrollerybackup')).toBe('C:\\')
    expect(parentDirectoryPath('/one.scrollerybackup')).toBe('/')
    expect(parentDirectoryPath('one.scrollerybackup')).toBe('')
  })
})
