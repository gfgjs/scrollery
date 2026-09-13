// pdf.js —— Scrollery vendored stub(替换上游同名文件)。
//
// 上游此文件基于 pdf.js(`vendor/pdfjs/pdf.mjs`,约 13M cmaps blob)为 <foliate-view> 提供
// PDF 后端。Scrollery 的 PDF 走独立的 PdfReader.vue(pdfjs-dist npm 依赖),不经 foliate-js,
// 故在 vendoring 时**剥离整个 PDF 后端与 13M blob**(见同目录 VENDOR.md「剥离项」)。
//
// 保留 `makePDF` 导出仅为满足 view.js 中 `const { makePDF } = await import('./pdf.js')` 的
// **构建期动态 import 解析**——运行期永不触达此路径:DocumentViewer 依 kind 把 PDF 分发到
// PdfReader,只有 epub/txt/md 才走 <foliate-view>。若因误路由触达,抛错优于静默返回空书。
export const makePDF = async () => {
  throw new Error(
    '[foliate-js vendored] PDF backend was stripped; Scrollery renders PDF via PdfReader.vue',
  )
}
