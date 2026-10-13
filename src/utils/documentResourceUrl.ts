/** 还原 asset URL 的目录层级，保留文件名自身的百分号编码。 */
export function getDocumentResourceUrl(documentUrl: string): URL {
  const url = new URL(documentUrl)
  // Tauri 将整个磁盘路径编码为单段，只展开分隔符，不能对文件名再次解码。
  url.pathname = url.pathname.replace(/%2f|%5c/gi, '/')
  return url
}

/** 相对图片等资源始终以原文档目录为基址，包括正在阅读内部版本时。 */
export function getDocumentResourceBaseUrl(documentUrl: string): string {
  return new URL('.', getDocumentResourceUrl(documentUrl)).href
}
