// 侧栏对齐网格契约(2026-07-10 排版重构 7b17c4d):全部区块的内容左缘对齐
// --sidebar-indent(30px)统一轨,但等式的各项散在多个文件里——部分是 CSS token、
// 部分是 JS 常量(TREE_INDENT)、部分是手调宽度(工具手柄 18px),此前只靠注释粘合。
// 本 spec 从源码提取各常量,机器化钉住三条对齐等式与 var() fallback 一致性,
// 防「改 token / 改常量后五处静默散架」。提取正则若失配即测试红——说明契约点被移动,
// 须同步更新本文件(而非放松正则)。
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const read = (rel: string) => readFileSync(join(here, rel), 'utf-8')

const variablesCss = read('../../assets/styles/variables.css')
const appSidebar = read('AppSidebar.vue')
const accordion = read('AccordionSection.vue')
const folders = read('sections/FoldersSection.vue')
const folderStyles = read('sections/FoldersSection.styles.css')
const folderTreeVirtualization = read('../../composables/useFolderTreeVirtualization.ts')
const tools = read('sections/ToolsSection.vue')
const library = read('sections/LibrarySection.vue')
const management = read('sections/ManagementSection.vue')

/** 提取 `--name: <N>px` 声明值;失配抛错让测试红得明白。 */
function pxVar(css: string, name: string, from: string): number {
  const m = css.match(new RegExp(`${name}:\\s*(\\d+)px`))
  if (!m) throw new Error(`${from} 中找不到 ${name} 的 px 声明`)
  return Number(m[1])
}

const spacingXs = pxVar(variablesCss, '--spacing-xs', 'variables.css')
const spacingSm = pxVar(variablesCss, '--spacing-sm', 'variables.css')
const rail = pxVar(appSidebar, '--sidebar-rail', 'AppSidebar.vue')
const indent = pxVar(appSidebar, '--sidebar-indent', 'AppSidebar.vue')

describe('侧栏对齐网格契约(--sidebar-indent 统一内容轨)', () => {
  it('等式①标题行:rail + 箭头 + gap(--spacing-sm) = indent(标题文字起点)', () => {
    // 箭头宽 = ChevronRight 的 :size(方形图标)
    const chevron = accordion.match(/<ChevronRight[^>]*:size="(\d+)"/s)
    expect(chevron, 'AccordionSection.vue 找不到 ChevronRight :size').not.toBeNull()
    // 标题行左内边距必须消费 --sidebar-rail(而非硬编码)
    expect(accordion).toContain('var(--sidebar-rail')
    // 箭头与标题文字的间隔必须是 --spacing-sm(toggle 按钮上的 gap)
    expect(accordion).toContain('gap: var(--spacing-sm)')
    expect(rail + Number(chevron![1]) + spacingSm).toBe(indent)
  })

  it('等式②文件树:.tree 横向 padding(--spacing-xs) + TREE_INDENT = indent', () => {
    const treeIndent = folderTreeVirtualization.match(/export const TREE_INDENT = (\d+)/)
    expect(treeIndent, 'useFolderTreeVirtualization.ts 找不到 TREE_INDENT 常量').not.toBeNull()
    expect(folders, 'FoldersSection.vue 未消费 TREE_INDENT').toContain('TREE_INDENT')
    // .tree 的横向 padding 必须仍是 --spacing-xs(等式左项的来源)
    const treePad = folderStyles.match(/\.tree \{[^}]*padding:\s*0 var\(--spacing-xs\)/s)
    expect(treePad, 'FoldersSection.styles.css 中 .tree 的横向 padding 不再是 --spacing-xs').not.toBeNull()
    expect(spacingXs + Number(treeIndent![1])).toBe(indent)
  })

  it('等式③工具区:列表 padding(--spacing-sm) + 手柄宽 + 行 gap = indent(卡片左缘)', () => {
    const listPad = tools.match(/\.tool-list \{[^}]*padding:\s*\d+px var\(--spacing-sm\)/s)
    expect(listPad, '.tool-list 横向 padding 不再是 --spacing-sm').not.toBeNull()
    const rowGap = tools.match(/\n\.tool \{[^}]*gap:\s*(\d+)px/s)
    expect(rowGap, '.tool 行 gap 声明被移动').not.toBeNull()
    const handleW = tools.match(/\.tool__handle \{[^}]*width:\s*(\d+)px/s)
    expect(handleW, '.tool__handle 宽度声明被移动').not.toBeNull()
    expect(spacingSm + Number(handleW![1]) + Number(rowGap![1])).toBe(indent)
  })

  it('图库/管理区:左缘以 var(--sidebar-indent) 表达(calc 自适应,无需等式)', () => {
    // LibrarySection:行内左缩进 = indent − 列表横向 padding(二者必须引用同一对 token)
    expect(library).toContain('calc(var(--sidebar-indent, 30px) - var(--spacing-sm))')
    // ManagementSection:直接对齐内容轨
    expect(management).toContain('var(--sidebar-indent, 30px)')
  })

  it('var() fallback 与声明值一致(fallback 漂移 = 组件脱离主容器时静默错位)', () => {
    for (const [src, name, declared] of [
      [accordion, '--sidebar-rail', rail],
      [library, '--sidebar-indent', indent],
      [management, '--sidebar-indent', indent],
    ] as const) {
      for (const m of src.matchAll(new RegExp(`var\\(${name},\\s*(\\d+)px\\)`, 'g'))) {
        expect(Number(m[1]), `${name} 的 fallback 与 AppSidebar 声明值不一致`).toBe(declared)
      }
    }
  })
})
