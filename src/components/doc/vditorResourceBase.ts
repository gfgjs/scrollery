import { getDocumentResourceUrl as getVditorDocumentUrl } from '../../utils/documentResourceUrl'
export { getDocumentResourceBaseUrl as getVditorResourceBaseUrl } from '../../utils/documentResourceUrl'

/** 按当前文档解析 IR 原始链接和 WYS 链接，不开放脚本或绕过 asset 授权的文件协议。 */
export function resolveVditorLinkUrl(destination: string, documentUrl: string): string | null {
  if (!destination.trim()) return null
  try {
    const base = getVditorDocumentUrl(documentUrl)
    const directory = new URL('.', base).href
    // Lute 的 linkBase 会把 WYS 的 #fragment 展开到目录，须恢复到当前文档。
    const link = destination.startsWith(`${directory}#`) ? destination.slice(directory.length) : destination
    const url = new URL(link, base)
    return ['http:', 'https:', 'asset:', 'mailto:', 'tel:'].includes(url.protocol) ? url.href : null
  } catch { return null }
}

/** 当前文档锚点命中活动正文的实际标题 ID 时，沿用目录的滚动定位且不改写正文。 */
export function scrollToVditorHeading(url: string, documentUrl: string, root: HTMLElement, mode: string): boolean {
  let fragment: string
  try {
    const target = new URL(url)
    if (!target.hash) return false
    fragment = decodeURIComponent(target.hash.slice(1))
    const current = getVditorDocumentUrl(documentUrl)
    target.hash = current.hash = ''
    if (target.href !== current.href) return false
  } catch { return false }
  const headings = Array.from(root.querySelectorAll<HTMLElement>('h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]'))
  // Lute 为 ID 加模式前缀，outlineRender 再加正文块索引；去重后缀仍由上游决定。
  const prefix = `${mode}-`
  const heading = headings.find(item => item.id === fragment)
    ?? headings.find(item => item.id.startsWith(prefix) && item.id.slice(prefix.length).replace(/_\d+$/, '') === fragment)
  if (!heading) return false
  root.scrollTop = heading.offsetTop
  return true
}

interface ResourceMarkdownSerializer {
  VditorIRDOM2Md: (html: string) => string
  VditorDOM2Md: (html: string) => string
}

function encodeResourceDestinationSpaces(html: string, document: Document, instantRendering: boolean): string {
  if (!html.includes('<img') && !html.includes('<a ') && !(instantRendering && html.includes('vditor-ir__marker--link'))) return html
  const template = document.createElement('template')
  template.innerHTML = html
  let changed = false
  for (const resource of template.content.querySelectorAll('img, a')) {
    const attribute = resource.tagName === 'IMG' ? 'src' : 'href'
    const source = resource.getAttribute(attribute)
    if (source?.includes(' ')) {
      resource.setAttribute(attribute, source.replace(/ /g, '%20'))
      changed = true
    }
  }
  if (instantRendering) {
    for (const destination of template.content.querySelectorAll('.vditor-ir__marker--link')) {
      const path = destination.textContent
      if (path?.includes(' ')) {
        destination.textContent = path.replace(/ /g, '%20')
        changed = true
      }
    }
  }
  return changed ? template.innerHTML : html
}

/** 修正本实例图片及链接目的地的序列化；离屏副本保留活 DOM、选区及撤销历史。 */
export function installVditorResourceSerialization(lute: ResourceMarkdownSerializer, document: Document): void {
  const serializeIR = lute.VditorIRDOM2Md.bind(lute)
  const serializeWysiwyg = lute.VditorDOM2Md.bind(lute)
  // Lute 编辑模式会去掉目的地的角括号但保留空格；编码目的地后再交原序列化器处理。
  lute.VditorIRDOM2Md = (html) => serializeIR(encodeResourceDestinationSpaces(html, document, true))
  lute.VditorDOM2Md = (html) => serializeWysiwyg(encodeResourceDestinationSpaces(html, document, false))
}
