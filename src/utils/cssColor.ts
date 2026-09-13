// src/utils/cssColor.ts
// 把「可能是表达式而非具体色的主题色 token」解成具体颜色字符串。
//
// 背景(2026-09-06 主题色浓度):底色 wash token 声明为
//   color-mix(in oklch, <锚点> calc(var(--theme-tint-scale, 1) * 100%), <中性>)
// getComputedStyle().getPropertyValue() 对未注册自定义属性只做 var() **代换**(不是求值):
// 回读串里 var() 已展开,但 color-mix()/calc()/相对色等函数原样保留——canvas fillStyle 与
// parseColorToRgb 都吃不下这类表达式,须经真实渲染树求值:探针元素设 color: var(--token),
// 回读 computed color 即得具体色(oklch()/rgb() 形态,解析见 utils/color.ts)。
// 判定按**结果形态**而非 var() 有无:简单具体色走快路径原样返回,其余交探针。

/** 仅由数字常量构成的简单具体色:十六进制、rgb()/rgba()、oklch()、color(srgb …)。
 *  函数体内出现括号(calc/color-mix 嵌套)或首参非数字(相对色 oklch(from …))即不匹配,
 *  交 DOM 探针求值。 */
const SIMPLE_COLOR_RE =
  /^(?:#[0-9a-f]{3,8}|rgba?\(\s*[\d.]+[^()]*\)|oklch\(\s*[\d.]+[^()]*\)|color\(\s*srgb\s+[\d.]+[^()]*\))$/i

export function resolveTokenColor(scope: HTMLElement, token: string, fallback: string): string {
  const raw = getComputedStyle(scope).getPropertyValue(token).trim()
  if (!raw) return fallback
  if (SIMPLE_COLOR_RE.test(raw)) return raw
  const probe = document.createElement('span')
  // 不用 display:none:display:none 子树的 computed 取值不可靠;绝对定位 + hidden 不参与布局不闪现。
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.style.pointerEvents = 'none'
  scope.appendChild(probe)
  try {
    probe.style.color = `var(${token})`
    const resolved = getComputedStyle(probe).color.trim()
    return resolved || fallback
  } finally {
    probe.remove()
  }
}
