<template>
  <!-- Hidden offscreen renderer (§3.4 Lite 路径). 无可见 DOM —— canvas 程序化创建后即弃。 -->
  <div class="doc-thumb-renderer"></div>
</template>

<script setup lang="ts">
// DocThumbRenderer —— 文档缩略图的前端离屏渲染器（需求4, §3.4 Lite 路径）。
//
// 设计要点：
//  - **轻量取舍**：Lite 变体无 native 栅格化器 → pdf/svg 由前端 Webview 离屏渲染截图，
//    经 store_doc_thumbnail 回传后端落盘（epub 封面由后端 zip 处理，不经此组件）。
//  - **单线程节流**：同一时刻只渲染一个文档；每个之间让出主线程，避免卡主窗口。
//  - **仅窗口可见时跑**：visibilitychange 暂停/恢复（风险表：占用主窗口/单线程 → 严格节流）。
//  - **pdf.js 懒加载**：仅当真的遇到 PDF 时才动态 import，避免拖慢启动 / 膨胀首屏包。
//  - **失败即标记**：渲染失败回传空字节，后端标 status=3，避免无限重试坏文件。

import { onMounted, onBeforeUnmount } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { invokeIpc, invokeIpcRaw } from '../../utils/ipc'
import { useTauriListen } from '../../composables/useTauriListen'
import { IPC, EVENTS } from '../../constants/ipc'
import { getPdfjs } from '../../utils/pdfjs'

interface PendingDocThumb {
  itemId: number
  absPath: string
  fileFormat: string
}

// 渲染目标长边（px）。后端会再按缩略图档位缩放，这里只需给一个清晰的中间分辨率。
const TARGET = 512
// 每轮领取的批量（与后端默认一致即可）。
const BATCH = 4

function canvasToPng(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('toBlob returned null'))), 'image/png')
  })
}

// 渲染 PDF 首页 → PNG + 总页数（T10:文档已打开,顺带取 numPages 回填 document_meta,
// 免为页数二次解析文档）。PDF 透明 → 先铺白底，避免黑/透明缩略图。
async function renderPdf(url: string): Promise<{ blob: Blob; pages: number | null }> {
  const lib = await getPdfjs()
  // isEvalSupported:false —— 关闭 pdf.js 字型渲染的 eval 优化路径(P1-23,配合 CSP 删 unsafe-eval)。
  const doc = await lib.getDocument({ url, isEvalSupported: false }).promise
  try {
    const pages = doc.numPages ?? null
    const page = await doc.getPage(1)
    const base = page.getViewport({ scale: 1 })
    const scale = TARGET / Math.max(base.width, base.height)
    const viewport = page.getViewport({ scale })
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(viewport.width))
    canvas.height = Math.max(1, Math.round(viewport.height))
    const ctx = canvas.getContext('2d')!
    ctx.fillStyle = '#fff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    await page.render({ canvasContext: ctx, viewport }).promise
    return { blob: await canvasToPng(canvas), pages }
  } finally {
    doc.destroy().catch(() => {})
  }
}

// 渲染 SVG → PNG（Webview 原生解析 <img>，再绘到离屏 canvas）。SVG 无页概念 → pages: null。
function renderSvg(url: string): Promise<{ blob: Blob; pages: number | null }> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      // 部分 SVG 无 intrinsic 尺寸 → 退化为方形目标尺寸。
      const nw = img.naturalWidth || TARGET
      const nh = img.naturalHeight || TARGET
      const scale = TARGET / Math.max(nw, nh)
      const w = Math.max(1, Math.round(nw * scale))
      const h = Math.max(1, Math.round(nh * scale))
      const canvas = document.createElement('canvas')
      canvas.width = w
      canvas.height = h
      const ctx = canvas.getContext('2d')!
      ctx.fillStyle = '#fff'
      ctx.fillRect(0, 0, w, h)
      try {
        ctx.drawImage(img, 0, 0, w, h)
        resolve(canvasToPng(canvas).then((blob) => ({ blob, pages: null })))
      } catch (e) {
        reject(e) // 跨源污染等 → toBlob 会抛，标记失败
      }
    }
    img.onerror = () => reject(new Error('svg image load failed'))
    img.src = url
  })
}

async function renderOne(doc: PendingDocThumb): Promise<{ blob: Blob; pages: number | null }> {
  const url = convertFileSrc(doc.absPath)
  // 格式白名单:后端队列只播种 pdf/svg。未知格式防御性抛错(走失败预算),
  // 而非落进 renderSvg 被 <img> 误渲染——那会把不支持的格式伪装成"渲染失败"甚至空白成功。
  if (doc.fileFormat === 'pdf') return renderPdf(url)
  if (doc.fileFormat === 'svg') return renderSvg(url)
  throw new Error(`unsupported doc thumb format: ${doc.fileFormat}`)
}

// ── 失败预算:首败不判死,第 2 次才上报空字节(后端标 status=3 停止重试)────────────
// pdf.js 懒加载/asset 协议偶发抖动等瞬态失败原先一击即永久裂图(空字节 → 派生 status=3,
// 无自愈路径,只能清缓存)。给每个 item 两次机会:首败留在 pending(下轮泵重试),
// 复败才盖棺。成功/盖棺即清计数,Map 大小受 pending 文档数约束。
const MAX_ATTEMPTS = 2
const failedAttempts = new Map<number, number>()

// ── 主泵：可见时循环领取并处理，直到无待处理 ───────────────────────────────────
let running = false
async function pump() {
  if (running || document.visibilityState !== 'visible') return
  running = true
  try {
    // 自行播种队列：pdf/svg 为前端驱动，需在用户未启动后端派生流水线时也能工作（幂等、廉价）。
    await invokeIpc(IPC.ENSURE_DOC_THUMB_QUEUE).catch(() => {})
    while (document.visibilityState === 'visible') {
      const pending = await invokeIpc<PendingDocThumb[]>(IPC.LIST_PENDING_DOC_THUMBS, { limit: BATCH })
      if (!pending.length) break
      let stored = 0
      for (const doc of pending) {
        if (document.visibilityState !== 'visible') break
        try {
          const { blob, pages } = await renderOne(doc)
          // raw body 直传字节(①):PNG 经 JSON 数字数组每字节膨胀 ~4 字符(300KB→1-2MB 串),
          // Uint8Array 走 Tauri raw InvokeBody 零膨胀;元数据走自定义 headers。
          const bytes = new Uint8Array(await blob.arrayBuffer())
          const headers: Record<string, string> = { 'x-item-id': String(doc.itemId) }
          if (pages !== null) headers['x-page-count'] = String(pages)
          await invokeIpcRaw(IPC.STORE_DOC_THUMBNAIL, bytes, headers)
          stored++
          failedAttempts.delete(doc.itemId)
        } catch {
          const attempts = (failedAttempts.get(doc.itemId) ?? 0) + 1
          if (attempts >= MAX_ATTEMPTS) {
            // 复败 → 回传空 body，后端标错，停止无限重试。
            failedAttempts.delete(doc.itemId)
            const reported = await invokeIpcRaw(IPC.STORE_DOC_THUMBNAIL, new Uint8Array(0), {
              'x-item-id': String(doc.itemId),
            })
              .then(() => true)
              .catch(() => false)
            if (reported) stored++
          } else {
            failedAttempts.set(doc.itemId, attempts)
          }
        }
        // 让出主线程一帧，保持窗口响应。
        await new Promise((r) => setTimeout(r, 30))
      }
      // 无进展守卫:本轮一个都没落库(渲染全败且失败上报也没写进后端)则退出本次泵。
      // 否则同一批 pending 会被无限重复领取——两层 catch 都吞错时曾是潜在热循环。
      // 下次 media_enriched / 可见性变化会再触发泵重试。
      if (stored === 0) break
    }
  } finally {
    running = false
  }
}

// 事件去抖：扫描/封面落地会高频触发 db:media_enriched，避免每条都重入。
let debounceTimer: ReturnType<typeof setTimeout> | null = null
function pumpDebounced() {
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => pump(), 800)
}

function onVisible() {
  if (document.visibilityState === 'visible') pump()
}

// MEDIA_ENRICHED 监听解绑交由 useTauriListen(P1-11);onMounted 只留 DOM 监听与首泵。
useTauriListen(EVENTS.MEDIA_ENRICHED, pumpDebounced)

onMounted(() => {
  document.addEventListener('visibilitychange', onVisible)
  pump()
})

onBeforeUnmount(() => {
  if (debounceTimer) clearTimeout(debounceTimer)
  document.removeEventListener('visibilitychange', onVisible)
})
</script>

<style scoped>
.doc-thumb-renderer {
  display: none;
}
</style>
