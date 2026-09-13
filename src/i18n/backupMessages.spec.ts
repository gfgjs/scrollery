import { describe, expect, it } from 'vitest'
import { backupMessages } from './backupMessages'

function leafKeys(value: unknown, prefix = ''): string[] {
  if (!value || typeof value !== 'object') return [prefix]

  return Object.entries(value)
    .flatMap(([key, child]) => leafKeys(child, prefix ? `${prefix}.${key}` : key))
    .sort()
}

describe('backupMessages', () => {
  it('中英文包含完全相同的翻译键', () => {
    expect(leafKeys(backupMessages['en-US'])).toEqual(leafKeys(backupMessages['zh-CN']))
  })
})
