import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn } from 'node:child_process'
import puppeteer from 'puppeteer-core'

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

/**
 * 启动用于渲染与检查的浏览器。
 *
 * 本机 Edge 153 实测：启动器进程只做转发，给后台真实浏览器进程传参后自己约 15ms 内以 0 退出，
 * 而 puppeteer.launch 的握手依赖被启动进程存活，于是稳定报 “Code: 0、stderr 为空”。
 * 该现象是本次环境实测结论，不对其它 Edge/Chromium 版本作断言。
 * 这里改为自行指定调试端口，读 DevToolsActivePort 后连接，不依赖启动器存活；
 * 关闭走 CDP Browser.close，因为被连接的不再是启动器子进程。
 */
export async function launchGuideBrowser(timeoutMs = 30000) {
  const executablePath = findBrowser()
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'scrollery-guide-'))
  const child = spawn(executablePath, [
    `--user-data-dir=${userDataDir}`,
    '--headless=new',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-gpu',
    '--remote-debugging-port=0',
    'about:blank',
  ], { stdio: 'ignore', windowsHide: true })
  let spawnError = null
  child.on('error', (error) => { spawnError = error })
  const portFile = path.join(userDataDir, 'DevToolsActivePort')
  let port = null
  let wsPath = null
  const deadline = Date.now() + timeoutMs
  while (!port && Date.now() < deadline) {
    if (spawnError) break
    if (fs.existsSync(portFile)) {
      const [line1, line2] = fs.readFileSync(portFile, 'utf8').split('\n')
      port = line1?.trim() || null
      wsPath = line2?.trim() || null
    }
    if (!port) await new Promise((resolve) => setTimeout(resolve, 100))
  }
  const owned = { child, userDataDir, port, wsPath }
  if (spawnError) {
    await shutdownOwnedBrowser(owned)
    throw new Error(`浏览器启动失败：${spawnError.message}`)
  }
  if (!port) {
    await shutdownOwnedBrowser(owned)
    throw new Error(`浏览器未在 ${timeoutMs / 1000} 秒内就绪：${executablePath}`)
  }
  let browser
  try {
    browser = await puppeteer.connect({ browserURL: `http://127.0.0.1:${port}`, defaultViewport: { width: 1280, height: 800 } })
  } catch (error) {
    await shutdownOwnedBrowser(owned)
    throw new Error(`无法连接浏览器：${error.message}`)
  }
  return { browser, close: () => shutdownOwnedBrowser({ ...owned, browser }) }
}

/**
 * 关闭本次自有的浏览器并回收临时 profile。
 * 无论正常结束、启动失败、未就绪超时还是 connect 失败都走这里，避免遗留自有进程与目录。
 */
async function shutdownOwnedBrowser({ browser, child, userDataDir, port, wsPath }) {
  let closed = false
  if (browser) {
    try {
      const client = await browser.target().createCDPSession()
      await client.send('Browser.close')
      closed = true
    } catch { /* 浏览器可能已自行退出 */ }
    try { browser.disconnect() } catch { /* 忽略重复断开 */ }
  }
  // connect 失败或会话不可用时的兜底：按自有调试端口直接请求关闭。
  if (!closed && port) await requestBrowserClose(port, wsPath)
  try { child?.kill() } catch { /* 启动器通常已退出 */ }
  // 真实浏览器进程退出会略滞后，profile 可能短暂被占用；清理失败不影响产物。
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      fs.rmSync(userDataDir, { recursive: true, force: true })
      return
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 200))
    }
  }
}

/** 经 DevTools 协议发送 Browser.close；仅关闭本次以独立 profile 启动的浏览器。 */
async function requestBrowserClose(port, wsPath) {
  if (typeof WebSocket !== 'function') return false
  return new Promise((resolve) => {
    let settled = false
    const finish = (value) => { if (!settled) { settled = true; resolve(value) } }
    let socket
    try {
      socket = new WebSocket(`ws://127.0.0.1:${port}${wsPath || '/devtools/browser'}`)
    } catch {
      finish(false)
      return
    }
    const timer = setTimeout(() => { try { socket.close() } catch { /* 忽略 */ } finish(false) }, 3000)
    socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Browser.close' })))
    socket.addEventListener('message', () => { clearTimeout(timer); try { socket.close() } catch { /* 忽略 */ } finish(true) })
    socket.addEventListener('error', () => { clearTimeout(timer); finish(false) })
    socket.addEventListener('close', () => { clearTimeout(timer); finish(false) })
  })
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
