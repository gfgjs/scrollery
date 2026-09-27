// 打包预算门禁的纯判定层测试。
//
// 本仓 S7 教训:门禁必须**双向验证**——能绿也能红。只断言「当前产物过门」等于把一个
// 永不触发的规则钉进 CI(见 docs/experience.md「门禁可信度是独立于覆盖率的属性」)。
// 故每条不变量都配一组「合规样本过 / 违规样本红」对照。
//
// 2026-09-16 精简:保留每条不变量的双向对照与判定前提;DEFAULT_POLICY 自身的取值自证
// (禁用表非空、预算落在某区间)属配置值断言而非门禁行为,整删。每条场景仍是独立 it。
import { describe, it, expect } from 'vitest'
import { evaluateBundle, DEFAULT_POLICY } from './vite-plugin-bundle-budget.mjs'

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
})
