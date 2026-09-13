// src/utils/markdown.spec.ts
// renderMarkdown 的**注入面**契约(2026-07-16 安全审查)。
//
// 本模块的产物被渲染进 foliate 的 blob: iframe —— 而该 iframe 与父页**同源**(blob URL 继承创建者的
// 源),父页持有 Tauri IPC 桥。故「一个 .md 文件能否注入可执行标记」不是理论问题:它是 md → 181 个
// IPC 命令的第一跳。
//
// 被修的实证缺口:escapeHtml 此前只转义 & < >,而本模块有**两处属性上下文**
// (代码块 class="language-…" 与链接 href="…") —— 不转引号就能提前闭合属性再补事件处理器。
// 文件头注当时写着「先做 HTML 转义再套用变换,避免 XSS」/「raw input can't inject markup」,
// 对属性上下文而言那句话是假的。

import { describe, it, expect } from 'vitest'
import { renderMarkdown, renderMarkdownBlocks } from './markdown'

/**
 * 产物里是否出现了**真标签**(非转义后的文本)。
 * 有意不写成「找 on\w+= 就报警」那种黑名单探针:`&lt;img src=x onerror=alert(1)&gt;` 是纯文本、
 * 完全无害,黑名单却会对它误报 —— 施工时正是这样红了一次。黑名单两个方向都会误判,故改为
 * 只问一个精确问题:**有没有未经转义的 `<tag`**。
 */
function hasRawTag(html: string, tag: string): boolean {
  return new RegExp(`<${tag}\\b`, 'i').test(html)
}

describe('属性上下文注入(2026-07-16 实证缺口)', () => {
  it('代码围栏语言不能闭合 class 属性', () => {
    // 实证过的原始 payload:此前产出
    //   <pre><code class="language-a"onmouseover="alert(1)">hello</code></pre>
    const html = renderMarkdown('```a"onmouseover="alert(1)\nhello\n```')
    // 「引号后面紧跟另一个属性」= 属性已被提前闭合。这是本注入的**唯一**判据,精确且不误报。
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

describe('标记注入(既有转义不回归)', () => {
  it('裸 script 标签被转义成文本,不成标签', () => {
    const html = renderMarkdown('<script>alert(1)</script>')
    expect(hasRawTag(html, 'script')).toBe(false)
    expect(html).toContain('&lt;script&gt;')
  })

  it('img onerror 被转义成文本,不成标签', () => {
    // 产出 `&lt;img src=x onerror=alert(1)&gt;` —— 文本里带 "onerror=" 字样无害,
    // 关键是它不是标签(见 hasRawTag 注)。
    const html = renderMarkdown('<img src=x onerror=alert(1)>')
    expect(hasRawTag(html, 'img')).toBe(false)
    expect(html).toContain('&lt;img')
  })
})

describe('转义不破坏正常渲染', () => {
  it('引号在文本里照常呈现(转成实体,浏览器渲染回 " 与 \')', () => {
    const html = renderMarkdown('He said "hi" and it\'s fine')
    // 实体形式在 HTML 源里,渲染时仍是原字符 —— 不能退化成把引号吃掉。
    expect(html).toContain('&quot;hi&quot;')
    expect(html).toContain('it&#39;s')
  })

  it('代码块内的引号保留为实体,不丢字符', () => {
    const html = renderMarkdown('```js\nconst a = "x"\n```')
    expect(html).toContain('&quot;x&quot;')
    expect(html).toContain('class="language-js"')
  })

  it('正常语言围栏仍产出干净的 class', () => {
    const html = renderMarkdown('```rust\nfn main() {}\n```')
    expect(html).toContain('<code class="language-rust">')
  })

  it('正常链接不受影响', () => {
    const html = renderMarkdown('[t](https://example.com/a?b=1)')
    expect(html).toContain('href="https://example.com/a?b=1"')
    expect(html).toContain('rel="noopener noreferrer"')
  })
})

describe('块数组契约(2026-07-17 内存爆炸修复:分片依赖)', () => {
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

  it('每个块自包含:列表整体是一个块,<ul> 不会与 </ul> 分离', () => {
    const blocks = renderMarkdownBlocks('- a\n- b\n\n段落')
    expect(blocks).toHaveLength(2)
    expect(blocks[0]).toBe('<ul>\n<li>a</li>\n<li>b</li>\n</ul>')
    expect(blocks[1]).toBe('<p>段落</p>')
  })

  it('列表产物与旧「逐项 push」格式字节一致(join 后不回归)', () => {
    // 旧实现 joined 产物:<ul>\n<li>…</li>\n</ul> —— 新实现必须逐字节保持。
    expect(renderMarkdown('1. x\n2. y')).toBe('<ol>\n<li>x</li>\n<li>y</li>\n</ol>')
  })

  it('断长跑(行数上限):连续 450 短行切成 200/200/50 三个 <p>', () => {
    const src = Array.from({ length: 450 }, (_, i) => `line${i}`).join('\n')
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
    const src = Array.from({ length: 100 }, () => line).join('\n')
    const blocks = renderMarkdownBlocks(src)
    expect(blocks).toHaveLength(4)
    for (const b of blocks) expect(b.split('<br>')).toHaveLength(25)
  })

  it('断长跑:单超长行自成一块,不丢内容', () => {
    const giant = 'y'.repeat(20_000)
    const blocks = renderMarkdownBlocks(`short\n${giant}\nshort2`)
    expect(blocks).toHaveLength(3)
    expect(blocks[1]).toContain(giant)
    expect(blocks[0]).toContain('short')
    expect(blocks[2]).toContain('short2')
  })

  it('断长跑不碰代码围栏:围栏内容再长也是单 <pre> 块', () => {
    const body = Array.from({ length: 300 }, (_, i) => `code${i}`).join('\n')
    const blocks = renderMarkdownBlocks('```\n' + body + '\n```')
    expect(blocks).toHaveLength(1)
    expect(blocks[0].startsWith('<pre><code>')).toBe(true)
  })
})
