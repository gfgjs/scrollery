import { expect, it } from 'vitest'
import { renderMarkdown, renderMarkdownBlocks } from './markdown'
import { buildMarkdownSyntheticBook } from './syntheticBook'



it('图片与链接仅保留安全资源协议，原始HTML和属性字符仍转义', () => {
  const output = renderMarkdown('![x](javascript:alert%281%29) [危险](data:text/html,evil) <img src=x onerror=alert(1)> ![引号"<&](ok.png "标题\\\"内容")')
  expect(output).not.toContain('src="javascript:')
  expect(output).not.toContain('href="data:')
  expect(output).toContain('&lt;img src=x onerror=alert(1)&gt;')
  expect(output).toContain('<img src="ok.png" alt="引号&quot;&lt;&amp;" title="标题&quot;内容">')
  expect(renderMarkdown('![粘贴图](data:image/png;base64,aGVsbG8=)')).toContain('src="data:image/png;base64,aGVsbG8="')
  expect(renderMarkdown('![脚本图](data:image/svg+xml;base64,aGVsbG8=)')).not.toContain('<img')
})






it('Markdown 相对文档链接保留原始路径和片段且禁用非文档绝对协议', () => {
  expect(renderMarkdown('[说明](<../中文 100%25.md#标题>) [站点](https://example.org) [标题](#这里)')).toBe('<p><a href="../中文 100%25.md#标题" data-document-link="">说明</a> <a href="https://example.org" target="_blank" rel="noopener noreferrer">站点</a> <a href="#这里" target="_blank" rel="noopener noreferrer">标题</a></p>')
  expect(renderMarkdown('[脚本](javascript:alert%281%29) [绝对](/root.md) [磁盘](file:///c:/book.md) [查询](?q=1)')).toBe('<p><a aria-disabled="true">脚本</a> <a aria-disabled="true">绝对</a> <a aria-disabled="true">磁盘</a> <a aria-disabled="true">查询</a></p>')
})


it('Markdown 正文实体仅解码一次且代码与反斜线转义保持字面文本', () => {
  const source = '# &#x4e2d; &copy;\n\nA &amp; B &#65; &nbsp; &amp;lt; &unknown; &copy no-semicolon\n\n`&amp; &#65;` \\&amp;\n\n&lt;img src=x onerror=evil&gt;\n\n```text\n&amp; &#65;\n```'
  const blocks = renderMarkdownBlocks(source)
  expect(blocks).toEqual([
    '<h1>中 ©</h1>',
    '<p>A &amp; B A \u00a0 &amp;lt; &amp;unknown; &amp;copy no-semicolon</p>',
    '<p><code>&amp;amp; &amp;#65;</code> &amp;amp;</p>',
    '<p>&lt;img src=x onerror=evil&gt;</p>',
    '<pre><code class="language-text">&amp;amp; &amp;#65;</code></pre>',
  ])
  const book = buildMarkdownSyntheticBook({ blocks, createObjectURL: html => html })
  expect(book.toc).toEqual([{ label: '中 ©', href: '#中-' }])
})
