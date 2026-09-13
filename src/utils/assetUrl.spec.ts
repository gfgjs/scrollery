import { describe, expect, it, vi } from 'vitest'

const { convertFileSrc } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc }))

import { resolveAssetUrl } from './assetUrl'

describe('resolveAssetUrl', () => {
  it('Windows 与 Unix 绝对文件路径都经 Tauri asset protocol', () => {
    expect(resolveAssetUrl('C:\\photos\\a.jpg')).toBe('asset://C:/photos/a.jpg')
    expect(resolveAssetUrl('/Users/alice/photos/a.jpg')).toBe(
      'asset:///Users/alice/photos/a.jpg',
    )
  })

  it('已有受控 URL 不重复转换', () => {
    for (const url of [
      'data:image/png;base64,AA==',
      'blob:https://app.local/id',
      'https://example.invalid/a.jpg',
      'asset://localhost/a.jpg',
      'tauri://localhost/a.jpg',
      'ipc://localhost/a.jpg',
    ]) {
      expect(resolveAssetUrl(url)).toBe(url)
    }
    expect(convertFileSrc).toHaveBeenCalledTimes(2)
  })
})
