<template>
  <div ref="scrollEl" class="pdf-reader">
    <div v-if="error" class="pdf-reader__error">{{ error }}</div>
    <div ref="pagesEl" class="pdf-reader__pages"></div>
  </div>
</template>

<script setup lang="ts">
// PDF 阅读器（§5.1）。pdf.js 渲染；按页懒渲染（IntersectionObserver），仅滚到附近才栅格化，
// 支撑大文件。位置格式 "page:N"；next/prev = 翻到上/下一页（吸附）。
import { ref, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { getPdfjs } from '../../utils/pdfjs'
import type { PDFDocumentProxy } from 'pdfjs-dist'

const props = defineProps<{ url: string; initial: string | null }>()
const emit = defineEmits<{
  (e: 'ready'): void
  (e: 'progress', pos: string): void
  (e: 'info', info: { page: number; pages: number }): void
}>()

const { t } = useI18n()

const scrollEl = ref<HTMLElement | null>(null)
const pagesEl = ref<HTMLElement | null>(null)
const error = ref('')

let pdfDoc: PDFDocumentProxy | null = null
let numPages = 0
let currentPage = 1
let destroyed = false
const rendered = new Set<number>()
let io: IntersectionObserver | null = null
const wrappers: HTMLElement[] = []

// 渲染上限：避免超大 canvas 爆内存（长边像素）。
const MAX_CANVAS_EDGE = 2200

async function renderPage(pageNo: number) {
  if (rendered.has(pageNo) || destroyed || !pdfDoc) return
  rendered.add(pageNo)
  try {
    const page = await pdfDoc.getPage(pageNo)
    if (destroyed) return
    const wrapper = wrappers[pageNo - 1]
    if (!wrapper) return
    const base = page.getViewport({ scale: 1 })
    wrapper.style.aspectRatio = `${base.width} / ${base.height}`
    const dpr = Math.min(window.devicePixelRatio || 1, 2)
    const cssW = wrapper.clientWidth || 700
    let scale = (cssW * dpr) / base.width
    // 限制画布最长边
    const longEdge = Math.max(base.width, base.height) * scale
    if (longEdge > MAX_CANVAS_EDGE) scale *= MAX_CANVAS_EDGE / longEdge
    const viewport = page.getViewport({ scale })
    const canvas = document.createElement('canvas')
    canvas.width = Math.round(viewport.width)
    canvas.height = Math.round(viewport.height)
    canvas.style.width = '100%'
    canvas.style.height = '100%'
    const ctx = canvas.getContext('2d')!
    await page.render({ canvasContext: ctx, viewport }).promise
    if (destroyed) return
    wrapper.replaceChildren(canvas)
  } catch {
    rendered.delete(pageNo) // 允许重试
  }
}

function computeCurrentPage() {
  const el = scrollEl.value
  if (!el) return
  const probe = el.scrollTop + el.clientHeight * 0.3
  let p = 1
  for (let i = 0; i < wrappers.length; i++) {
    if (wrappers[i].offsetTop <= probe) p = i + 1
    else break
  }
  if (p !== currentPage) {
    currentPage = p
    emit('info', { page: currentPage, pages: numPages })
  }
  emit('progress', `page:${currentPage}`)
}

function scrollToPage(p: number, smooth = true) {
  const w = wrappers[Math.max(0, Math.min(numPages - 1, p - 1))]
  if (w) scrollEl.value?.scrollTo({ top: w.offsetTop, behavior: smooth ? 'smooth' : 'auto' })
}

function next() {
  scrollToPage(currentPage + 1)
}
function prev() {
  scrollToPage(currentPage - 1)
}
function getScrollEl() {
  return scrollEl.value
}
defineExpose({ next, prev, getScrollEl, goToPage: (p: number) => scrollToPage(p) })

onMounted(async () => {
  try {
    const lib = await getPdfjs()
    // isEvalSupported:false —— 关闭 pdf.js 字型渲染的 eval 优化路径(P1-23,配合 CSP 删 unsafe-eval)。
    pdfDoc = await lib.getDocument({ url: props.url, isEvalSupported: false }).promise
    if (destroyed) return
    numPages = pdfDoc.numPages
    // 用首页比例预置所有页占位高度（aspect-ratio），减少懒渲染时的滚动跳变。
    const first = await pdfDoc.getPage(1)
    const r = first.getViewport({ scale: 1 })
    const ratio = `${r.width} / ${r.height}`
    const host = pagesEl.value!
    for (let i = 1; i <= numPages; i++) {
      const w = document.createElement('div')
      w.className = 'pdf-page'
      w.style.aspectRatio = ratio
      w.dataset.page = String(i)
      host.appendChild(w)
      wrappers.push(w)
    }
    emit('info', { page: 1, pages: numPages })

    // 懒渲染：进入视口附近才栅格化。
    io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            const p = Number((e.target as HTMLElement).dataset.page)
            renderPage(p)
          }
        }
      },
      { root: scrollEl.value, rootMargin: '600px 0px' },
    )
    wrappers.forEach((w) => io!.observe(w))

    await nextTick()
    // 恢复阅读位置
    const m = props.initial && /^page:(\d+)$/.exec(props.initial)
    if (m) {
      currentPage = parseInt(m[1], 10)
      scrollToPage(currentPage, false)
    } else {
      renderPage(1)
    }
    scrollEl.value?.addEventListener('scroll', onScroll, { passive: true })
    emit('ready')
  } catch (e) {
    error.value = t('doc.pdfOpenFailed', { error: (e as Error)?.message ?? e })
    emit('ready')
  }
})

let scrollRaf = 0
function onScroll() {
  if (scrollRaf) return
  scrollRaf = requestAnimationFrame(() => {
    scrollRaf = 0
    computeCurrentPage()
  })
}

onBeforeUnmount(() => {
  destroyed = true
  scrollEl.value?.removeEventListener('scroll', onScroll)
  if (io) io.disconnect()
  if (pdfDoc) pdfDoc.destroy?.()
})
</script>

<style scoped>
.pdf-reader {
  height: 100%;
  overflow-y: auto;
  /* 原 var(--color-bg-base, #2b2b2b) 引用不存在的幽灵 token,一直走 fallback——
     阅读台衬底改用 inset token,随主题正确着色(S5 修) */
  background: var(--color-bg-inset);
  padding: 24px 0;
  box-sizing: border-box;
}
.pdf-reader__pages {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}
.pdf-reader :deep(.pdf-page) {
  width: min(900px, 92%);
  /* 硬编码豁免:PDF 页面本体=纸,渲染基准永远白(pdfjs 输出按白底合成) */
  background: #fff;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.35);
  display: block;
}
.pdf-reader__error {
  color: var(--color-text-secondary);
  text-align: center;
  padding: 40px;
}
</style>
