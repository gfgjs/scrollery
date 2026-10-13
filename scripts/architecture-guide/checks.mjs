import fs from 'node:fs'
import path from 'node:path'

/** 核对产物中的实际链接，确保从导览位置可解析到仓库内现有目标。 */
export async function inspectSourceLinks(page, documentFile) {
  const links = await page.$$eval('.source-link', (elements) => elements.map((e) => e.getAttribute('href')))
  const issues = []
  const root = path.resolve(path.dirname(documentFile), '../..')
  for (const href of links) {
    if (!href.startsWith('../../') || /[#?:\\]/.test(href)) {
      issues.push(`源码链接不是本地相对路径：${href}`)
      continue
    }
    const target = path.resolve(path.dirname(documentFile), decodeURIComponent(href))
    const relative = path.relative(root, target)
    if (relative.startsWith('..') || path.isAbsolute(relative) || !fs.existsSync(target)) issues.push(`源码链接目标不存在或越界：${href}`)
  }
  return { links: links.length, targets: new Set(links).size, issues }
}

/** 使用现有浏览器，避免生成文档时下载额外浏览器。 */
export function findBrowser() {
  const candidates = [
    process.env.SCROLLERY_GUIDE_BROWSER,
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
  ]
  const browser = candidates.find((p) => p && fs.existsSync(p))
  if (!browser) throw new Error('未找到浏览器，请用 SCROLLERY_GUIDE_BROWSER 指定 Chrome/Edge/Chromium。')
  return browser
}

/** 构建与冒烟共用的结构、资源和实际渲染检查。 */
export async function inspectPage(page) {
  return page.evaluate(async () => {
    await document.fonts.ready
    const issues = []
    const ids = [...document.querySelectorAll('[id]')].map((e) => e.id)
    if (new Set(ids).size !== ids.length) issues.push(`重复 DOM ID: ${ids.filter((id, i) => ids.indexOf(id) !== i).join(', ')}`)
    const anchors = [...document.querySelectorAll('a[href^="#"]')]
    for (const a of anchors) {
      if (!document.getElementById(decodeURIComponent(a.hash.slice(1)))) issues.push(`失效锚点 ${a.hash}`)
    }
    for (const e of document.querySelectorAll('[src],link[href],image[href],use[href]')) {
      const value = e.getAttribute('src') || e.getAttribute('href')
      if (value && !value.startsWith('#') && !value.startsWith('data:')) issues.push(`外部资源 ${value}`)
    }
    const hidden = [...document.querySelectorAll('.document[hidden]')]
    hidden.forEach((e) => { e.hidden = false })
    let labels = 0
    for (const svg of document.querySelectorAll('.diagram svg')) {
      const bounds = svg.getBoundingClientRect()
      const texts = [...svg.querySelectorAll('text, foreignObject')]
      if (!texts.length || bounds.width <= 0) issues.push(`图形无可测文字 ${svg.id}`)
      for (const text of texts) {
        labels++
        const box = text.getBoundingClientRect()
        if (box.left < bounds.left - 2 || box.top < bounds.top - 2 || box.right > bounds.right + 2 || box.bottom > bounds.bottom + 2) {
          issues.push(`SVG 文字越界 ${svg.id}: ${text.textContent}`)
        }
        if (text.tagName === 'foreignObject') {
          const range = document.createRange()
          range.selectNodeContents(text)
          for (const content of range.getClientRects()) {
            if (content.left < box.left - 2 || content.top < box.top - 2 || content.right > box.right + 2 || content.bottom > box.bottom + 2) issues.push(`HTML 图标签裁切 ${svg.id}: ${text.textContent}`)
          }
        }
        // 使用屏幕坐标，包含祖先 transform；同时检查标签容器自身裁切。
        const label = text.closest('.node')
        const shape = label?.querySelector(':scope > rect, :scope > polygon, :scope > path, :scope > circle, :scope > ellipse')
        if (shape) {
          const shapeBox = shape.getBoundingClientRect()
          if (box.left < shapeBox.left - 2 || box.top < shapeBox.top - 2 || box.right > shapeBox.right + 2 || box.bottom > shapeBox.bottom + 2) issues.push(`节点文字越界 ${svg.id}: ${text.textContent}`)
        }
      }
    }
    hidden.forEach((e) => { e.hidden = true })
    const search = JSON.parse(document.querySelector('#search-index').textContent)
    for (const entry of search) if (!document.getElementById(entry.id)) issues.push(`搜索锚点不存在 ${entry.id}`)
    return {
      issues, anchors: anchors.length, labels,
      documents: document.querySelectorAll('.document').length,
      figures: document.querySelectorAll('.diagram').length,
      features: document.querySelectorAll('[data-entry^="F"]').length,
      pipelines: document.querySelectorAll('[data-entry^="P"]').length,
      searchEntries: search.length,
    }
  })
}
