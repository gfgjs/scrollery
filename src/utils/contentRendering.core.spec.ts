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
import { srtToVtt, looksLikeVtt } from './srtToVtt'

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

  it('单引号同样不能闭合属性(防未来把属性写成单引号形式)', () => {
    const html = renderMarkdown("```a'onmouseover='alert(1)\nhi\n```")
    expect(html).not.toContain("'onmouseover=")
    expect(html).toContain('&#39;')
  })

  it('非法协议仍回退 #(既有白名单不回归)', () => {
    const html = renderMarkdown('[x](javascript:alert(1))')
    expect(html).not.toMatch(/href="javascript:/i)
  })
})

describe('renderMarkdown:标记注入(既有转义不回归)', () => {
  it('裸 script 标签被转义成文本,不成标签', () => {
    const html = renderMarkdown('<script>alert(1)</script>')
    expect(hasRawTag(html, 'script')).toBe(false)
    expect(html).toContain('&lt;script&gt;')
  })

  it('img onerror 被转义成文本,不成标签', () => {
    // 产出 &lt;img src=x onerror=alert(1)&gt; —— 文本里带 onerror= 字样无害,关键是它不是标签。
    const html = renderMarkdown('<img src=x onerror=alert(1)>')
    expect(hasRawTag(html, 'img')).toBe(false)
    expect(html).toContain('&lt;img')
  })
})

describe('renderMarkdown:转义不破坏正常渲染', () => {
  it('引号在文本里照常呈现(转成实体,浏览器渲染回 " 与 \')', () => {
    const html = renderMarkdown('He said "hi" and it\'s fine')
    // 实体形式在 HTML 源里,渲染时仍是原字符 —— 不能退化成把引号吃掉。
    expect(html).toContain('&quot;hi&quot;')
    expect(html).toContain('it&#39;s')
  })

  it('正常链接不受影响', () => {
    const html = renderMarkdown('[t](https://example.com/a?b=1)')
    expect(html).toContain('href="https://example.com/a?b=1"')
    expect(html).toContain('rel="noopener noreferrer"')
  })
})

describe('renderMarkdown:块数组契约(2026-07-17 内存爆炸修复:分片依赖)', () => {
  it('renderMarkdown = blocks.join(换行),字节一致', () => {
    const src = [
      '# 标题',
      '',
      '段落一行',
      '第二行',
      '',
      '- a',
      '- b',
      '',
      '1. x',
      '2. y',
      '',
      '> 引用',
      '',
      '```js',
      'const a = 1',
      '```',
      '',
      '---',
    ].join('\n')
    expect(renderMarkdownBlocks(src).join('\n')).toBe(renderMarkdown(src))
  })

  it('断长跑(行数上限):连续 450 短行切成 200/200/50 三个 <p>', () => {
    const src = Array.from({ length: 450 }, (_, i) => 'line' + i).join('\n')
    const blocks = renderMarkdownBlocks(src)
    expect(blocks).toHaveLength(3)
    expect(blocks[0].split('<br>')).toHaveLength(200)
    expect(blocks[1].split('<br>')).toHaveLength(200)
    expect(blocks[2].split('<br>')).toHaveLength(50)
    // 内容无损:行序连续跨块。
    expect(blocks[1]).toContain('line200')
    expect(blocks[2]).toContain('line449')
  })

  it('断长跑(字符上限):长行日志在 ~6K 字符处切块,不等满 200 行', () => {
    // 100 行 × 240 字符:按字符上限 6000 → 每块 25 行,共 4 块。
    const line = 'x'.repeat(240)
    const blocks = renderMarkdownBlocks(Array.from({ length: 100 }, () => line).join('\n'))
    expect(blocks).toHaveLength(4)
    for (const b of blocks) expect(b.split('<br>')).toHaveLength(25)
  })

  it('断长跑:单超长行自成一块,不丢内容', () => {
    const giant = 'y'.repeat(20_000)
    const blocks = renderMarkdownBlocks('short\n' + giant + '\nshort2')
    expect(blocks).toHaveLength(3)
    expect(blocks[1]).toContain(giant)
    expect(blocks[0]).toContain('short')
    expect(blocks[2]).toContain('short2')
  })

  it('断长跑不碰代码围栏:围栏内容再长也是单 <pre> 块', () => {
    const body = Array.from({ length: 300 }, (_, i) => 'code' + i).join('\n')
    const blocks = renderMarkdownBlocks('```\n' + body + '\n```')
    expect(blocks).toHaveLength(1)
    expect(blocks[0].startsWith('<pre><code>')).toBe(true)
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

  it('拒绝时留 warn 诊断 —— 静默拒绝会让「这本书某功能没了」变成玄学', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    await simulateLoadItem(target, 'application/javascript', true)
    expect(loggerWarn).toHaveBeenCalled()
  })
})

describe('epubScriptGate:非脚本资源不受影响(不能把书弄坏)', () => {
  it('XHTML 章节、CSS、字体、图片一律放行', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    for (const type of [
      'application/xhtml+xml',
      'text/css',
      'font/woff2',
      'image/jpeg',
      'image/svg+xml',
    ]) {
      expect(await simulateLoadItem(target, type, false)).toBe(true)
    }
  })
})

describe('epubScriptGate:装配', () => {
  it('无 transformTarget 的 book(SyntheticBook/comic/fb2)静默跳过,不抛', () => {
    expect(() => denyScriptResources({})).not.toThrow()
    expect(() => denyScriptResources(undefined)).not.toThrow()
  })

  it('返回的摘除函数生效后不再拦(卸载后不留悬挂订阅)', async () => {
    const target = new EventTarget()
    const dispose = denyScriptResources({ transformTarget: target })
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(false)
    dispose()
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(true)
  })
})

describe('srtToVtt', () => {
  it('正常转换:编号剥离、时间戳逗号转点、多条 cue', () => {
    const srt = [
      '1',
      '00:00:01,000 --> 00:00:04,000',
      'Hello world',
      '',
      '2',
      '00:00:05,500 --> 00:00:07,250',
      'Second line',
      'with wrap',
    ].join('\n')

    const vtt = srtToVtt(srt)
    expect(vtt.startsWith('WEBVTT\n\n')).toBe(true)
    expect(vtt).toContain('00:00:01.000 --> 00:00:04.000')
    expect(vtt).toContain('Hello world')
    expect(vtt).toContain('00:00:05.500 --> 00:00:07.250')
    expect(vtt).toContain('Second line\nwith wrap')
  })

  it('坏 cue 跳过不抛:缺时间行、时间戳非法、无文本', () => {
    const srt = [
      '1',
      'not a time line',
      'orphan text',
      '',
      '2',
      'aa:bb:cc,ddd --> 00:00:07,250',
      'bad timestamp',
      '',
      '3',
      '00:00:08,000 --> 00:00:09,000',
      '',
      '4',
      '00:00:10,000 --> 00:00:11,000',
      'valid cue',
    ].join('\n')

    expect(() => srtToVtt(srt)).not.toThrow()
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('valid cue')
    expect(vtt).not.toContain('orphan text')
    expect(vtt).not.toContain('bad timestamp')
  })

  it('CRLF 与 LF 均可处理', () => {
    const vtt = srtToVtt('1\r\n00:00:01,000 --> 00:00:02,000\r\nCRLF text\r\n')
    expect(vtt).toContain('00:00:01.000 --> 00:00:02.000')
    expect(vtt).toContain('CRLF text')
  })

  it('标签保留:<b> <i> <u> 不转义', () => {
    const vtt = srtToVtt('1\n00:00:01,000 --> 00:00:02,000\n<b>bold</b> <i>italic</i> <u>underline</u>')
    expect(vtt).toContain('<b>bold</b> <i>italic</i> <u>underline</u>')
  })

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

describe('looksLikeVtt', () => {
  it('首个非空行以 WEBVTT 开头 → true', () => {
    expect(looksLikeVtt('WEBVTT\n\n00:00:01.000 --> 00:00:02.000\ntext')).toBe(true)
    expect(looksLikeVtt('\n\n  WEBVTT\nfoo')).toBe(true)
  })

  it('非 WEBVTT 开头 → false', () => {
    expect(looksLikeVtt('1\n00:00:01,000 --> 00:00:02,000\ntext')).toBe(false)
    expect(looksLikeVtt('')).toBe(false)
  })
})
