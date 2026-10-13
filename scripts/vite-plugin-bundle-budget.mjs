// 打包预算门禁：构建失败条件分别约束入口块体积与入口块依赖。
// Rollup 的 chunkSizeWarningLimit 只报告体积，且不区分入口块与懒块。
//
// 两条硬门(任一破 → build 失败):
//   ① 入口块体积 ≤ entryMaxKB。
//   ② 重依赖不得出现在入口块。这是设计契约(它们只该经 dynamic import 进懒块),
//      命中时检查静态导入路径。
//
// 懒块体积只报告，用于定位依赖变化，不阻断构建。
//
// 纯判定逻辑 evaluateBundle 与 Vite 解耦,由 vite-plugin-bundle-budget.spec.mjs 双向验证
// (能绿也能红)。插件本体只是薄适配层。

/**
 * 默认预算；入口上限以生产构建的实际字节数为基线，单位见下方说明。
 */
export const DEFAULT_POLICY = {
  // 超限时先检查入口块组成，再决定是否调整预算。
  // 单位与 Vite 报告一致 = kB(SI,bytes/1000),**不是** KiB/1024;两者在此量级差 ~2.7%,
  // 混用会让预算悄悄比字面值松。
  // 入口实测 712.29 kB(`npm run build`,esbuild)，加约 13% 余量为 805 kB。
  entryMaxKB: 805,
  // 只经 dynamic import 抵达的重依赖；`@shikijs/` 末尾斜线表示整个 scope。
  entryForbiddenDeps: ['pdfjs-dist', 'shiki', '@shikijs/'],
  // 超过此值的懒块只报告,不拦截。
  lazyReportKB: 500,
}

/** 从模块 id 取 npm 包名;非 node_modules 模块返回 null。 */
export function pkgOf(moduleId) {
  const norm = moduleId.split('\\').join('/')
  // 取**最后一个** node_modules 之后的段:pnpm/嵌套依赖会出现多重 node_modules 路径
  const i = norm.lastIndexOf('node_modules/')
  if (i === -1) return null
  const rest = norm.slice(i + 'node_modules/'.length)
  const parts = rest.split('/')
  if (parts.length === 0 || !parts[0]) return null
  return parts[0].startsWith('@') && parts.length > 1 ? `${parts[0]}/${parts[1]}` : parts[0]
}

/** 包名是否命中禁用表(表项以 `/` 结尾表 scope 前缀,否则全等)。 */
export function isForbiddenPkg(pkg, forbidden) {
  return forbidden.some((f) => (f.endsWith('/') ? pkg.startsWith(f) : pkg === f))
}

/**
 * 纯判定:输入归一化后的 chunk 列表,输出违规与报告。不碰 fs、不碰 Vite。
 * @param {Array<{name:string,sizeBytes:number,isEntry:boolean,isDynamicEntry:boolean,moduleIds:string[]}>} chunks
 * @param {typeof DEFAULT_POLICY} policy
 * @returns {{failures:string[],report:string[]}}
 */
export function evaluateBundle(chunks, policy = DEFAULT_POLICY) {
  const failures = []
  const report = []
  const kb = (b) => (b / 1000).toFixed(2)

  const entries = chunks.filter((c) => c.isEntry)
  if (entries.length === 0) {
    // 没有入口块 = 判定前提不成立。此时静默通过等于门禁失效,故显式报错。
    failures.push('产物中没有入口 chunk —— 预算门无从判定(构建配置可能已改变)')
    return { failures, report }
  }

  for (const entry of entries) {
    const overBy = entry.sizeBytes - policy.entryMaxKB * 1000
    if (overBy > 0) {
      failures.push(
        `入口块 ${entry.name} = ${kb(entry.sizeBytes)} kB,超预算 ${policy.entryMaxKB} kB(超出 ${kb(overBy)} kB)`,
      )
    } else {
      report.push(
        `✓ 入口 ${entry.name}: ${kb(entry.sizeBytes)} kB / ${policy.entryMaxKB} kB(余 ${kb(-overBy)} kB)`,
      )
    }

    const hits = new Map()
    for (const id of entry.moduleIds) {
      const pkg = pkgOf(id)
      if (pkg && isForbiddenPkg(pkg, policy.entryForbiddenDeps)) {
        if (!hits.has(pkg)) hits.set(pkg, [])
        hits.get(pkg).push(id)
      }
    }
    for (const [pkg, ids] of hits) {
      failures.push(
        `重依赖 ${pkg} 出现在入口块 ${entry.name}(${ids.length} 个模块)——它只应经 dynamic import 进懒块。` +
          `首个模块: ${ids[0]}`,
      )
    }
    if (hits.size === 0) {
      report.push(`✓ 入口无重依赖(禁用表: ${policy.entryForbiddenDeps.join(', ')})`)
    }
  }

  const bigLazy = chunks
    .filter((c) => !c.isEntry && c.sizeBytes > policy.lazyReportKB * 1000)
    .sort((a, b) => b.sizeBytes - a.sizeBytes)
  for (const c of bigLazy) {
    report.push(`· 懒块 ${c.name}: ${kb(c.sizeBytes)} kB(不进首屏,仅报告)`)
  }

  return { failures, report }
}

/**
 * Vite 插件:薄适配层——把 bundle 归一化后交给 evaluateBundle,失败则中断构建。
 *
 * 用 writeBundle 而非 generateBundle,量**盘上真实字节数**:generateBundle 阶段 `chunk.code`
 * 尚未定稿,后续仍被改写(2026-07-15 实测入口 548,106 → 落盘 549,751,差 1,645 字节;加
 * `enforce:'post'` 也不消失,即改写发生在 Vite 内建的 post 插件之后)。与其逐个追查改写者,
 * 不如直接量发货物——对任何未知后处理免疫。代价:红灯时 dist/ 已写出,但 CI 判的是退出码。
 */
export default function bundleBudget(policy = DEFAULT_POLICY) {
  return {
    name: 'scrollery:bundle-budget',
    apply: 'build',
    async writeBundle(options, bundle) {
      const { statSync } = await import('node:fs')
      const { join } = await import('node:path')
      const outDir = options.dir ?? 'dist'

      const chunks = Object.values(bundle)
        .filter((c) => c.type === 'chunk')
        .map((c) => ({
          name: c.fileName,
          sizeBytes: statSync(join(outDir, c.fileName)).size,
          isEntry: c.isEntry,
          isDynamicEntry: c.isDynamicEntry,
          moduleIds: Object.keys(c.modules),
        }))

      const { failures, report } = evaluateBundle(chunks, policy)
      for (const line of report) this.info(line)
      if (failures.length > 0) {
        this.error(
          `打包预算门禁未通过:\n  ${failures.join('\n  ')}\n` +
            `策略见 scripts/vite-plugin-bundle-budget.mjs 顶部注释。`,
        )
      }
    },
  }
}
