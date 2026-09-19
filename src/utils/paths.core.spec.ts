// src/utils/paths.core.spec.ts
// 本地路径 → 受控 URL / 路由的映射契约。2026-09-16 由 assetUrl.spec.ts(用户路径 → asset protocol)
// 与 mediaRoute.spec.ts(查看器路由 push/replace 分流)集中而来:同目录、环境兼容(后者不读 Tauri IPC),
// 原两文件删去;每条场景仍是独立 it。
//
// assetUrl:用户给的绝对路径必须经 Tauri asset protocol,已是受控 URL 的不重复转换。
// mediaRoute:主视图打开 = push(back 一次回来处);查看器内再开 = replace(防 history 叠层,
// 否则关闭要按 N+1 次 ESC)(2026-07-10 深审问题2)。
import { describe, it, expect, vi } from 'vitest'
import type { Router } from 'vue-router'

const { convertFileSrc } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => 'asset://' + path),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc }))

import { resolveAssetUrl } from './assetUrl'
import { isContentViewerRoute, isViewerRoute, viewerRouteFor, openMediaRoute } from './mediaRoute'

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

function fakeRouter(path: string) {
  const push = vi.fn()
  const replace = vi.fn()
  const router = {
    currentRoute: { value: { path } },
    push,
    replace,
  } as unknown as Router
  return { router, push, replace }
}

describe('viewerRouteFor', () => {
  it('按媒体类型分发到专用路由', () => {
    expect(viewerRouteFor(5, 'document')).toBe('/doc/5')
    expect(viewerRouteFor(5, 'audio')).toBe('/audio/5')
    expect(viewerRouteFor(5, 'image')).toBe('/view/5')
    expect(viewerRouteFor(5, 'video')).toBe('/view/5')
  })
})

describe('isViewerRoute', () => {
  it('识别三个查看器路由前缀', () => {
    expect(isViewerRoute('/view/33')).toBe(true)
    expect(isViewerRoute('/doc/7')).toBe(true)
    expect(isViewerRoute('/audio/2')).toBe(true)
  })

  it('主视图与形近路径不误判', () => {
    expect(isViewerRoute('/')).toBe(false)
    expect(isViewerRoute('/collections')).toBe(false)
    expect(isViewerRoute('/persons')).toBe(false)
    // 前缀含尾斜杠,'/viewer' 之类形近路径不命中
    expect(isViewerRoute('/viewer')).toBe(false)
  })
})

describe('isContentViewerRoute', () => {
  it('只识别图/视频统一查看器，不误把文档和音频归入覆盖层', () => {
    expect(isContentViewerRoute('/view/33')).toBe(true)
    expect(isContentViewerRoute('/doc/7')).toBe(false)
    expect(isContentViewerRoute('/audio/2')).toBe(false)
    expect(isContentViewerRoute('/viewer')).toBe(false)
  })
})

describe('openMediaRoute', () => {
  it('主视图(画廊)打开 → push,可 back 返回', () => {
    const { router, push, replace } = fakeRouter('/')
    openMediaRoute(router, 5, 'image')
    expect(push).toHaveBeenCalledWith('/view/5')
    expect(replace).not.toHaveBeenCalled()
  })

  it('大图查看器内再开一张 → replace,history 不叠层', () => {
    const { router, push, replace } = fakeRouter('/view/33')
    openMediaRoute(router, 34, 'image')
    expect(replace).toHaveBeenCalledWith('/view/34')
    expect(push).not.toHaveBeenCalled()
  })

  it('跨类型:阅读页内点图片 → replace 到 /view', () => {
    const { router, replace } = fakeRouter('/doc/9')
    openMediaRoute(router, 34, 'image')
    expect(replace).toHaveBeenCalledWith('/view/34')
  })

  it('跨类型:大图内点文档 → replace 到对应专用路由', () => {
    const { router, replace } = fakeRouter('/view/33')
    openMediaRoute(router, 7, 'document')
    expect(replace).toHaveBeenCalledWith('/doc/7')
  })

})
