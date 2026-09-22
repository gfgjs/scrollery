#!/usr/bin/env node
/** 只验证生成页面，不启动或测试产品。截图保存在已忽略的目录。 */
import assert from 'node:assert/strict'
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import puppeteer from 'puppeteer-core'
import { findBrowser, inspectPage, inspectSourceLinks } from './checks.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const output = path.join(root, 'docs/architecture/index.html')
const sampleSource = 'src-tauri/tauri.conf.json'
const screenshots = path.join(root, '.screenshots/architecture-guide')
fs.mkdirSync(screenshots, { recursive: true })
const browser = await puppeteer.launch({ executablePath: findBrowser(), headless: true })
const server = http.createServer((request, response) => {
  const files = new Map([
    ['/docs/architecture/index.html', { file: output, type: 'text/html' }],
    [`/${sampleSource}`, { file: path.join(root, sampleSource), type: 'application/json' }],
  ])
  const resource = files.get(request.url)
  if (!resource) {
    response.writeHead(404).end()
    return
  }
  response.writeHead(200, { 'Content-Type': `${resource.type}; charset=utf-8` })
  response.end(fs.readFileSync(resource.file))
})
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
const hosted = `http://127.0.0.1:${server.address().port}/docs/architecture/index.html`
const file = pathToFileURL(output).href
const report = {}
try {
  const page = await browser.newPage()
  const errors = []
  const subresources = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('request', (request) => { if (!request.isNavigationRequest()) subresources.push(request.url()) })
  async function open(url) {
    await page.goto(url, { waitUntil: 'load' })
    await page.waitForSelector('html[data-ready="true"]')
  }
  async function visibleDoc() {
    return page.$eval('.document:not([hidden])', (e) => e.id)
  }
  async function searchFor(value) {
    await page.$eval('#search', (e, v) => { e.value = v; e.dispatchEvent(new Event('input')) }, value)
  }

  await page.setViewport({ width: 1440, height: 1000, deviceScaleFactor: 1 })
  await open(file)
  report.structure = await inspectPage(page)
  assert.deepEqual(report.structure.issues, [])
  report.sourceLinks = await inspectSourceLinks(page, output)
  assert.deepEqual(report.sourceLinks.issues, [])
  const sourceHref = await page.$eval(`.source-link[href="../../${sampleSource}"]`, (e) => e.getAttribute('href'))
  const sourcePage = await browser.newPage()
  try {
    for (const base of [file, hosted]) {
      const response = await sourcePage.goto(new URL(sourceHref, base).href, { waitUntil: 'load' })
      assert.equal(response.status(), 200)
      assert.deepEqual(JSON.parse(await response.text()), JSON.parse(fs.readFileSync(path.join(root, sampleSource), 'utf8')))
    }
  } finally { await sourcePage.close() }
  report.sourceLinks.browser = 'file 与 HTTP 相对链接均返回实际文件内容'
  assert.equal(await visibleDoc(), 'doc-readme')
  await page.screenshot({ path: path.join(screenshots, 'desktop.png') })
  await page.click('.quick-nav a[href="#readme--分层总览"]')
  await page.waitForFunction(() => decodeURIComponent(location.hash) === '#readme--分层总览')
  const figure = '#doc-readme .diagram'
  const before = await page.$eval(`${figure} svg`, (e) => e.getBoundingClientRect().width)
  await page.click(`${figure} [data-zoom="in"]`)
  const after = await page.$eval(`${figure} svg`, (e) => e.getBoundingClientRect().width)
  assert.ok(after > before * 1.2, '放大需改变真实画布宽度')
  await page.click(`${figure} [data-zoom="reset"]`)
  assert.equal(await page.$eval(`${figure} output`, (e) => e.value), '100%')
  await page.screenshot({ path: path.join(screenshots, 'overview.png') })
  await page.click(`${figure} [data-zoom="expand"]`)
  assert.equal(await page.$eval('#diagram-dialog', (e) => e.open), true)
  assert.equal(await page.$$eval('[id]', (es) => new Set(es.map((e) => e.id)).size === es.length), true)
  await page.keyboard.press('Escape')
  await page.waitForSelector(`${figure} [data-zoom="expand"]:not([hidden])`)
  report.zoom = '放大 / 复位 / 对话框 / Esc 归位通过'

  await searchFor('enrichment:completed')
  assert.ok(await page.$$eval('.search-result', (es) => es.length) > 0)
  await page.focus('#search')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement.className), 'search-result')
  await page.keyboard.press('Enter')
  await page.waitForFunction(() => document.querySelector('.document:not([hidden])')?.id === 'doc-library')
  await searchFor('P18')
  assert.ok((await page.$eval('#search-items', (e) => e.textContent)).includes('影像增强'))
  await searchFor('不存在的检索词xyz')
  assert.equal(await page.$$eval('.search-result', (es) => es.length), 0)
  await searchFor('')
  assert.equal(await page.$eval('#search-results', (e) => e.hidden), true)
  report.search = '正文关键词 / ID / 无结果 / 键盘跳转通过'

  await open(`${file}#operations--p23`)
  assert.equal(await visibleDoc(), 'doc-operations')
  const headingTop = await page.$eval('#operations--p23', (e) => e.getBoundingClientRect().top)
  assert.ok(headingTop >= 76 && headingTop < 250, `深链位置不正确 ${headingTop}`)
  await page.screenshot({ path: path.join(screenshots, 'pipeline.png') })
  await page.click('.quick-nav a[href="#doc-pipelines"]')
  await page.waitForFunction(() => document.querySelector('.document:not([hidden])')?.id === 'doc-pipelines')
  await page.goBack({ waitUntil: 'load' })
  await page.waitForFunction(() => document.querySelector('.document:not([hidden])')?.id === 'doc-operations')
  report.local = 'file 深链 / 刷新 / 文档导航 / 返回通过'

  await page.setViewport({ width: 390, height: 844, deviceScaleFactor: 1 })
  await open(hosted)
  assert.equal(await visibleDoc(), 'doc-readme')
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(screenshots, 'mobile.png') })
  await page.click('#menu-toggle')
  assert.equal(await page.$eval('#menu-toggle', (e) => e.getAttribute('aria-expanded')), 'true')
  await page.click('#sidebar a[href="#doc-pipelines"]')
  await page.waitForFunction(() => document.querySelector('.document:not([hidden])')?.id === 'doc-pipelines')
  assert.equal(await page.$eval('#menu-toggle', (e) => e.getAttribute('aria-expanded')), 'false')
  await page.click('#pipelines--p18 h3 a')
  await page.waitForFunction(() => location.hash === '#intelligence--p18' && document.querySelector('.document:not([hidden])')?.id === 'doc-intelligence')
  assert.equal(await visibleDoc(), 'doc-intelligence')
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  await page.screenshot({ path: path.join(screenshots, 'mobile-pipeline.png') })
  await open(`${hosted}#readme--分层总览`)
  await page.click(`${figure} [data-zoom="in"]`)
  await page.click(`${figure} [data-zoom="reset"]`)
  assert.equal(await page.$eval(`${figure} output`, (e) => e.value), '100%')
  await page.click(`${figure} [data-zoom="actual"]`)
  const labelSize = await page.$eval(`${figure} svg text`, (e) => e.getBoundingClientRect().height)
  assert.ok(labelSize >= 13, `原尺寸图中文字应可读：${labelSize}`)
  await page.click(`${figure} [data-zoom="reset"]`)
  assert.equal(await page.$eval(`${figure} .reading-path`, (e) => e.open), true)
  report.mobile = 'HTTP 子目录 / 390px 布局 / 目录抽屉 / 流程直达 / 缩放通过'
  assert.deepEqual(errors, [])
  assert.deepEqual(subresources, [])
  report.runtime = { errors, subresources }
  console.log(JSON.stringify(report, null, 2))
} finally {
  await browser.close()
  await new Promise((resolve) => server.close(resolve))
}
