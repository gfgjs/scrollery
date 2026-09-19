/** 后端稳定错误码 → 前端本地化键。分流只认 code，不匹配双语 message。 */
const RESTORE_ERROR_KEYS: Readonly<Record<string, string>> = {
  restore_format_unsupported: 'backup.restoreErrorFormat',
  restore_schema_incompatible: 'backup.restoreErrorSchema',
  restore_corrupt: 'backup.restoreErrorCorrupt',
  restore_path_invalid: 'backup.restoreErrorPath',
  restore_size_limit: 'backup.restoreErrorSpace',
  restore_document_missing: 'backup.restoreErrorDocument',
  restore_rollback_failed: 'backup.restoreErrorRollback',
  restore_io: 'backup.restoreErrorIo',
  file_job_busy: 'backup.restoreErrorBusy',
}

const BACKUP_ERROR_KEYS: Readonly<Record<string, string>> = {
  backup_dir_unset: 'backup.errorDirUnset',
  backup_dir_not_writable: 'backup.errorNotWritable',
  backup_document_inconsistent: 'backup.errorDocumentInconsistent',
  backup_cancelled: 'backup.cancelledToast',
  backup_io: 'backup.errorIo',
  file_job_busy: 'backup.errorBusy',
}

export function restoreErrorKey(code: string | undefined): string {
  return (code && RESTORE_ERROR_KEYS[code]) || 'backup.restoreErrorUnknown'
}

export function backupErrorKey(code: string | undefined): string {
  return (code && BACKUP_ERROR_KEYS[code]) || 'backup.errorUnknown'
}

/** 从跨平台文件路径取父目录；保留 Windows 盘符根和 Unix 根目录的尾分隔符。 */
export function parentDirectoryPath(path: string): string {
  const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  if (separator < 0) return ''
  if (separator === 0) return path.slice(0, 1)
  if (separator === 2 && /^[A-Za-z]:[\\/]/.test(path)) return path.slice(0, 3)
  return path.slice(0, separator)
}
