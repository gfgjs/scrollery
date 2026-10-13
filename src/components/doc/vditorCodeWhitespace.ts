/**
 * 保留源码模式代码块的空白；沿用 Lute 的语法树和其它节点渲染。
 * @see https://github.com/88250/lute/blob/v1.7.6/render/vditor_sv_renderer.go#L505
 * @see https://github.com/88250/lute/blob/v1.7.6/vditor_sv.go#L22
 */
export function installVditorCodeWhitespace(lute: Pick<Lute, 'SetJSRenderers' | 'Md2VditorSVDOM' | 'SpinVditorSVDOM'>): void {
  const newline = '<span data-type="newline"><br /><span style="display: none">\n</span></span>'
  lute.SetJSRenderers({ renderers: { Md2VditorSVDOM: {
    renderCodeBlockCode: (node, entering) => {
      if (!entering) return ['', Lute.WalkContinue]
      // 1.7.6 默认对整个代码块 TrimSpace；最后一个换行单独放到正文 span 后，保留上游 DOM 结构。
      const content = node.TokensStr().replace(/\n$/, '')
        .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
        .replace(/\n/g, `<span data-type="padding"></span>${newline}`)
      // 扩展输出不更新 SV 的 LastOut；显式补上代码正文至闭合围栏之间的换行。
      return [`<span data-type="text">${content}</span>${newline}`, Lute.WalkContinue]
    },
  } } })
  const spin = lute.SpinVditorSVDOM.bind(lute)
  // 同版本 Spin 未接扩展渲染，其余解析流程与 Md2 相同；保留仅有光标时的上游特殊输出。
  lute.SpinVditorSVDOM = markdown => markdown.trim() === Lute.Caret ? spin(markdown) : lute.Md2VditorSVDOM(markdown)
}
