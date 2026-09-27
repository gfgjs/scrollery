// src/utils/contentRendering.core.spec.ts
// 不可信内容 → 渲染产物的安全契约。2026-09-16 由 markdown.spec.ts / epubScriptGate.spec.ts /
// srtToVtt.spec.ts 三份同域纯工具测试集中而来(同目录、无冲突 mock),原零散文件删去;每条场景仍是独立 it。
// 三者共同点:输入都是用户/外部文件内容,产物会进入 DOM 或 foliate 的 blob: iframe
// (该 iframe 与父页同源,父页持有 Tauri IPC 桥)。
//
// markdown:注入面(2026-07-16 安全审查)——escapeHtml 此前只转义 & < >,而本模块有两处属性上下文
// (代码块 class="language-…" 与链接 href="…"),不转引号就能提前闭合属性再补事件处理器。
// epubScriptGate:「脚本化 EPUB 不支持」(设计 §4.8)此前只是文档里的一句话,上游 seam 无人订阅、
// allow 恒真;本块用真 EventTarget + 真 CustomEvent 复现上游 epub.js loadItem 的调用形状,钉住它真的被执行。
// srtToVtt:字幕标签白名单与其余尖括号转义。
import { describe, it, expect, beforeEach, vi } from 'vitest'

const { loggerWarn } = vi.hoisted(() => ({ loggerWarn: vi.fn() }))
vi.mock('./logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: loggerWarn, error: vi.fn() },
}))

import { renderMarkdown, renderMarkdownBlocks } from './markdown'
import { denyScriptResources } from './epubScriptGate'
import { srtToVtt } from './srtToVtt'

/**
 * 产物里是否出现了**真标签**(非转义后的文本)。
 * 有意不写成「找 onXxx= 就报警」那种黑名单探针:&lt;img src=x onerror=alert(1)&gt; 是纯文本、完全无害,
 * 黑名单却会对它误报 —— 施工时正是这样红了一次。黑名单两个方向都会误判,故改为只问一个精确问题:
 * **有没有未经转义的 <tag**。
 */
function hasRawTag(html: string, tag: string): boolean {
  return html.includes('<' + tag)
}

describe('renderMarkdown:属性上下文注入(2026-07-16 实证缺口)', () => {
  it('代码围栏语言不能闭合 class 属性', () => {
    // 实证过的原始 payload:此前产出
    //   <pre><code class="language-a"onmouseover="alert(1)">hello</code></pre>
    const html = renderMarkdown('```a"onmouseover="alert(1)\nhello\n```')
    // 「引号后面紧跟另一个属性」= 属性已被提前闭合。这是本注入的唯一判据,精确且不误报。
    expect(html).not.toContain('"onmouseover=')
    expect(html).toContain('&quot;')
  })

  it('链接 URL 不能闭合 href 属性 —— 协议白名单挡不住它(注入在合法协议之后)', () => {
    const html = renderMarkdown('[x](https://a"onmouseover="location=name)')
    expect(html).not.toContain('"onmouseover=')
  })

  it('非法协议仍回退 #(既有白名单不回归)', () => {
    const html = renderMarkdown('[x](javascript:alert(1))')
    expect(html).not.toMatch(/href="javascript:/i)
  })
})

describe('renderMarkdown:标记注入(既有转义不回归)', () => {

  it('img onerror 被转义成文本,不成标签', () => {
    // 产出 &lt;img src=x onerror=alert(1)&gt; —— 文本里带 onerror= 字样无害,关键是它不是标签。
    const html = renderMarkdown('<img src=x onerror=alert(1)>')
    expect(hasRawTag(html, 'img')).toBe(false)
    expect(html).toContain('&lt;img')
  })
})

describe('renderMarkdown:块数组契约(2026-07-17 内存爆炸修复:分片依赖)', () => {

  it('断长跑(字符上限):长行日志在 ~6K 字符处切块,不等满 200 行', () => {
    // 100 行 × 240 字符:按字符上限 6000 → 每块 25 行,共 4 块。
    const line = 'x'.repeat(240)
    const blocks = renderMarkdownBlocks(Array.from({ length: 100 }, () => line).join('\n'))
    expect(blocks).toHaveLength(4)
    for (const b of blocks) expect(b.split('<br>')).toHaveLength(25)
  })
})

/** 复现上游 epub.js loadItem 的那几行 —— 判据必须对着真实调用形状,而非我臆想的形状。 */
async function simulateLoadItem(
  target: EventTarget,
  type: string,
  isScript: boolean,
): Promise<boolean> {
  const detail = { type, isScript, allow: true }
  target.dispatchEvent(new CustomEvent('load', { detail }))
  return await detail.allow
}

beforeEach(() => {
  loggerWarn.mockClear()
})

describe('epubScriptGate:脚本资源被拒', () => {
  it('isScript 的项 → allow 置 false(上游据此 return null,该资源不落 blob URL)', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(false)
  })
})

describe('srtToVtt', () => {

  it('白名单标签含属性时剥除属性', () => {
    const vtt = srtToVtt('1\n00:00:01,000 --> 00:00:02,000\n<b class="x">text</b>')
    expect(vtt).toContain('<b>text</b>')
    expect(vtt).not.toContain('class')
  })

  it('其余尖括号转义防注入', () => {
    const vtt = srtToVtt('1\n00:00:01,000 --> 00:00:02,000\n<script>alert(1)</script>')
    expect(vtt).not.toContain('<script>')
    expect(vtt).toContain('&lt;script&gt;alert(1)&lt;/script&gt;')
  })
})
