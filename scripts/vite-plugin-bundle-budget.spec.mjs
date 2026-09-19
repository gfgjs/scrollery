// 打包预算门禁的纯判定层测试。
//
// 本仓 S7 教训:门禁必须**双向验证**——能绿也能红。只断言「当前产物过门」等于把一个
// 永不触发的规则钉进 CI(见 docs/experience.md「门禁可信度是独立于覆盖率的属性」)。
// 故每条不变量都配一组「合规样本过 / 违规样本红」对照。
//
// 2026-09-16 精简:保留每条不变量的双向对照与判定前提;DEFAULT_POLICY 自身的取值自证
// (禁用表非空、预算落在某区间)属配置值断言而非门禁行为,整删。每条场景仍是独立 it。
import { describe, it, expect } from 'vitest'
import { evaluateBundle, pkgOf, isForbiddenPkg, DEFAULT_POLICY } from './vite-plugin-bundle-budget.mjs'

// kB = SI 千字节(bytes/1000),与 Vite 报告口径一致 —— 不是 1024。
const KB = 1000
/** 造一个合规的基准产物:入口 500 kB 无重依赖 + 一个 637 kB 懒块(仿 shiki cpp 语法)。 */
function baseline(overrides = {}) {
  return [
    {
      name: 'assets/index-abc.js',
      sizeBytes: 500 * KB,
      isEntry: true,
      isDynamicEntry: false,
      moduleIds: ['src/main.ts', 'node_modules/vue/dist/vue.runtime.esm-bundler.js'],
      ...overrides,
    },
    {
      name: 'assets/cpp-xyz.js',
      sizeBytes: 637 * KB,
      isEntry: false,
      isDynamicEntry: true,
      moduleIds: ['node_modules/@shikijs/langs/dist/cpp.mjs'],
    },
  ]
}

describe('pkgOf', () => {
  it('取 scope 包名(含 scope 段)', () => {
    expect(pkgOf('node_modules/@shikijs/langs/dist/cpp.mjs')).toBe('@shikijs/langs')
  })

  it('Windows 反斜杠路径同样可解', () => {
    expect(pkgOf('D:\\workspace\\scrollery\\node_modules\\pdfjs-dist\\build\\pdf.mjs')).toBe(
      'pdfjs-dist',
    )
  })

  it('嵌套 node_modules 取最内层(取最后一个 node_modules 段)', () => {
    expect(pkgOf('node_modules/a/node_modules/pdfjs-dist/build/pdf.mjs')).toBe('pdfjs-dist')
  })

  it('应用自身模块返回 null', () => {
    expect(pkgOf('src/components/media/MediaGrid.vue')).toBeNull()
  })
})

describe('isForbiddenPkg', () => {

  it('以 / 结尾表 scope 前缀(全等项的精确匹配不误伤同前缀包名)', () => {
    expect(isForbiddenPkg('@shikijs/langs', ['@shikijs/'])).toBe(true)
    expect(isForbiddenPkg('@shikijs/core', ['@shikijs/'])).toBe(true)
  })

  it('不误伤同前缀的无关包', () => {
    // 'shiki' 全等项不得吃掉 'shikimori' 这类名字
    expect(isForbiddenPkg('shikimori', ['shiki'])).toBe(false)
  })
})

describe('evaluateBundle:入口体积门', () => {
  it('合规产物全绿', () => {
    const { failures } = evaluateBundle(baseline(), DEFAULT_POLICY)
    expect(failures).toEqual([])
  })

  it('入口超预算 → 红(变异测试:证明这条门能触发)', () => {
    const chunks = baseline({ sizeBytes: (DEFAULT_POLICY.entryMaxKB + 1) * KB })
    const { failures } = evaluateBundle(chunks, DEFAULT_POLICY)
    expect(failures).toHaveLength(1)
    expect(failures[0]).toContain('超预算')
  })

  it('按 SI kB(bytes/1000)报告,与 Vite 口径一致', () => {
    // 回归钉:首版用 /1024,把落盘 549,751 字节报成 536.87,而 Vite 报 549.75 —— 同一个块两个数字,
    // 预算比字面值悄悄松 2.7%。此处用实测字节数锁死口径。
    const { report } = evaluateBundle(baseline({ sizeBytes: 549751 }), DEFAULT_POLICY)
    expect(report[0]).toContain('549.75')
  })

  it('懒块再大也不拦截,只报告(有意:上游体积不可控)', () => {
    const chunks = baseline()
    chunks[1].sizeBytes = 5000 * KB
    const { failures, report } = evaluateBundle(chunks, DEFAULT_POLICY)
    expect(failures).toEqual([])
    expect(report.some((l) => l.includes('cpp-xyz.js') && l.includes('仅报告'))).toBe(true)
  })
})

describe('evaluateBundle:重依赖不进入口门', () => {
  it('pdfjs 混进入口 → 红', () => {
    const chunks = baseline()
    chunks[0].moduleIds.push('node_modules/pdfjs-dist/build/pdf.mjs')
    const { failures } = evaluateBundle(chunks, DEFAULT_POLICY)
    expect(failures).toHaveLength(1)
    expect(failures[0]).toContain('pdfjs-dist')
    expect(failures[0]).toContain('入口块')
  })

  it('shiki 语法混进入口 → 红(scope 前缀生效)', () => {
    const chunks = baseline()
    chunks[0].moduleIds.push('node_modules/@shikijs/langs/dist/cpp.mjs')
    expect(evaluateBundle(chunks, DEFAULT_POLICY).failures[0]).toContain('@shikijs/langs')
  })

  it('同一包多模块只报一条,并给出首个模块路径', () => {
    const chunks = baseline()
    chunks[0].moduleIds.push(
      'node_modules/pdfjs-dist/build/pdf.mjs',
      'node_modules/pdfjs-dist/build/pdf.worker.mjs',
    )
    const { failures } = evaluateBundle(chunks, DEFAULT_POLICY)
    expect(failures).toHaveLength(1)
    expect(failures[0]).toContain('2 个模块')
    expect(failures[0]).toContain('pdfjs-dist/build/pdf.mjs')
  })
})

describe('evaluateBundle:判定前提', () => {
  it('产物无入口块 → 红,不静默放行', () => {
    const chunks = baseline().filter((c) => !c.isEntry)
    const { failures } = evaluateBundle(chunks, DEFAULT_POLICY)
    expect(failures).toHaveLength(1)
    expect(failures[0]).toContain('没有入口 chunk')
  })
})
