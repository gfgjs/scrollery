// 打包预算门禁(S7,替代 Rollup 的 chunkSizeWarningLimit 告警)。
//
// 为什么自建而不用 Rollup 内建告警(以下均为 2026-07-15 实测,非推理):
//   ① 它是**告警不是门**。把 pdfjs(365 kB)静态 import 进 main.ts 造回归,`npm run build`
//      照喊照过 —— **exit=0**,CI 全绿,没人会发现首屏翻倍。本门同一回归 exit=1。
//   ② 它**不区分入口块与懒加载块**,当下喊的两个块全是误报:
//      · cpp-*.js   637.55 kB = shiki 的 C++ 语法(@shikijs/langs),**懒加载**、gzip 仅 47.22 kB,
//                   只在阅读器高亮 C++ 时才取,不进首屏;
//      · index-*.js 549.75 kB = 16 路由全懒加载后的应用外壳,已是共享底座。
//      两条都不对应任何可行动的问题。更糟的是①+②叠加:常年两条误报会训练所有人无视这类
//      告警,于是真回归来了(它确实会多喊一句)也淹没在噪音里——狼来了效应本身就是伤害。
//   ③ 阈值无差别:若某天入口只有 300 kB,塞进一个 150 kB 的重依赖,它连喊都不喊。
//   无差别阈值的下场是被调高或被无视(调高即永久失守),故换成两条**可行动**的不变量。
//
// 两条硬门(任一破 → build 失败):
//   ① 入口块体积 ≤ entryMaxKB。入口是我们自己的外壳,涨了就是我们干的,红了值得查。
//   ② 重依赖不得出现在入口块。这是设计契约(它们只该经 dynamic import 进懒块),
//      红了必是真回归。
//
// 有意**不设**懒块体积上限:最大的懒块(shiki cpp 语法)体积由上游决定,我们既不控制也
// 无从优化;为它设阈值,唯一可能的响应就是「把数字调大」——那正是本仓 S7 认定的幽灵门禁
// 反模式(见 docs/experience.md,门禁可信度是独立于覆盖率的属性)。改为只报告不拦截,保住可见性。
//
// 纯判定逻辑 evaluateBundle 与 Vite 解耦,由 vite-plugin-bundle-budget.spec.mjs 双向验证
// (能绿也能红)。插件本体只是薄适配层。

/**
 * 默认预算。数字须标注实测出处,否则下一个人无从判断该不该调。
 * 实测基线 2026-07-15(`npm run build`,minify=esbuild):入口 549.75 kB。
 * 2026-07-22 重定基线:CI 因主机硬件故障(见 docs 外部 memory)冻结约一周,期间落地的
 * cropperjs 图片编辑(07-19)/导出整理成果对话框(07-19)/S4 独立日志窗口等多条 feature
 * 从未被这道门实际跑过。硬件修复后首次全量 CI 验出两处真回归:①日志窗口
 * (@tanstack/vue-virtual 虚拟列表)经 main.ts 顶层静态 import,被塞进主窗口共享的同一
 * entry chunk——已改动态 import 修复(见 src/main.ts);②修复后实测入口仍为 626.46 kB
 * (`npm run build`),差额是前述其余 feature 的合法弥散式增长,非单一坏提交,遂重定基线。
 */
export const DEFAULT_POLICY = {
  // 626.46 实测(2026-07-22,日志窗口动态 import 修复后)+ ~13% 余量(与上一次基线同一比例),
  // 余量够正常加功能,又紧到 pdfjs(365 kB)之类挤不进来。
  // 红了不是「调大它」,是先跑 scripts 里的组成分析看多出来的是什么。
  // 单位与 Vite 报告一致 = kB(SI,bytes/1000),**不是** KiB/1024;两者在此量级差 ~2.7%,
  // 混用会让预算悄悄比字面值松。
  // 2026-09-02 重定基线:主画廊重复项浏览(2026-09-02 方案 P0–P4 六切片)落地——镜头
  // chip 按 §4.1 固定在筛选 chip 首位、激活反馈须 100ms 内出现,故镜头入口 UI/store
  // 属首屏契约内容而非可懒化载荷;实测入口 712.29 kB(`npm run build`,esbuild),按
  // 既有惯例 +13% 余量 → 805。
  entryMaxKB: 805,
  // 只该经 dynamic import 抵达的重依赖。名字均为 2026-07-15 实测在产物中真实出现的包名
  // (禁一个不存在的包 = 又造一个永不触发的幽灵规则)。`@shikijs/` 以 `/` 结尾表整个 scope。
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
