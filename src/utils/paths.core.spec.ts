// src/utils/paths.core.spec.ts
// 本地路径 → 受控 URL / 路由的映射契约。2026-09-16 由 assetUrl.spec.ts(用户路径 → asset protocol)
// 与 mediaRoute.spec.ts(查看器路由 push/replace 分流)集中而来:同目录、环境兼容(后者不读 Tauri IPC),
// 原两文件删去;每条场景仍是独立 it。
//
// assetUrl:用户给的绝对路径必须经 Tauri asset protocol,已是受控 URL 的不重复转换。
// mediaRoute:主视图打开 = push(back 一次回来处);查看器内再开 = replace(防 history 叠层,
// 否则关闭要按 N+1 次 ESC)(2026-07-10 深审问题2)。
import { describe, it, expect, vi } from 'vitest'

const { convertFileSrc } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => 'asset://' + path),
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
})
