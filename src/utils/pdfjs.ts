// src/utils/pdfjs.ts
// pdf.js 懒加载单例（§3.4/§5.1）。首次使用时才动态 import，并配置同源 worker
// （Vite 打包，CSP worker-src 回退 script-src 'self' → 允许）。文档缩略图与阅读器共用。

let pdfjs: typeof import('pdfjs-dist') | null = null
let loading: Promise<typeof import('pdfjs-dist')> | null = null

export function getPdfjs(): Promise<typeof import('pdfjs-dist')> {
  if (pdfjs) return Promise.resolve(pdfjs)
  if (loading) return loading
  loading = (async () => {
    const lib = await import('pdfjs-dist')
    const workerUrl = (await import('pdfjs-dist/build/pdf.worker.min.mjs?url')).default
    lib.GlobalWorkerOptions.workerSrc = workerUrl
    pdfjs = lib
    return lib
  })()
  // 失败不缓存 rejected promise(2026-07-06 审查 F10):否则一次瞬时加载失败会让整个进程
  // 生命周期内 PDF 阅读/文档缩略图永久不可用。清 loading 使下次调用重试。
  loading.catch(() => {
    loading = null
  })
  return loading
}
