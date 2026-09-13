import { describe, it, expect } from 'vitest'
import zhCN from './locales/zh-CN'
import enUS from './locales/en-US'

// R1-7 i18n 完整性锁测试(防回潮,上 CI):
// 1. 键树对齐 —— zh-CN 与 en-US 必须拥有完全相同的键路径集合,防止「只补一边」;
// 2. 键存在性 —— 全库源码中字面量 t('...') / $t('...') 引用的键必须在两份字典中都存在
//    且解析为字符串叶子。vue-i18n 缺键时返回键路径本身(truthy),UI 会直接渲染出
//    "contextMenu.copyImage" 这类原始键路径,且 `t('x') || '回退'` 永远短路不到回退,
//    所以缺键只能靠静态检查兜住,运行时无感。
// 局限:动态键(t(`a.${b}`) 模板串、t('prefix' + x) 拼接)无法静态提取,不在本测试覆盖面内
// ——正则要求字面量是完整实参(后随 , 或 )),拼接前缀不会被误提为完整键。

// 叶子可为字符串,或字符串数组(如 guide.chapters.*.points:多条要点)。数组按索引展成
// path.0/path.1… 独立叶子,使「zh 5 条、en 4 条」这类只补一边的数组也被键树对拍抓住。
type LocaleLeaf = string | string[]
type LocaleTree = { [key: string]: LocaleLeaf | LocaleTree }

/** 深度展开键路径:{a:{b:'x'}} → ['a.b'];数组 {p:['x','y']} → ['p.0','p.1']。 */
function flattenKeys(tree: LocaleTree, prefix = ''): string[] {
  const keys: string[] = []
  for (const [k, v] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (typeof v === 'string') keys.push(path)
    else if (Array.isArray(v)) v.forEach((_, i) => keys.push(`${path}.${i}`))
    else keys.push(...flattenKeys(v, path))
  }
  return keys
}

/** 按点路径取值;任一段缺失返回 undefined。数组/字符串视为终点,再下探即缺失。 */
function resolvePath(tree: LocaleTree, path: string): LocaleLeaf | LocaleTree | undefined {
  let node: LocaleLeaf | LocaleTree | undefined = tree
  for (const seg of path.split('.')) {
    if (node === undefined || typeof node === 'string' || Array.isArray(node)) return undefined
    node = node[seg]
  }
  return node
}

describe('locale 完整性', () => {
  it('zh-CN 与 en-US 键树完全对齐', () => {
    const zhKeys = new Set(flattenKeys(zhCN as LocaleTree))
    const enKeys = new Set(flattenKeys(enUS as LocaleTree))
    const missingInEn = [...zhKeys].filter((k) => !enKeys.has(k)).sort()
    const missingInZh = [...enKeys].filter((k) => !zhKeys.has(k)).sort()
    expect(missingInEn, 'zh-CN 有而 en-US 缺的键').toEqual([])
    expect(missingInZh, 'en-US 有而 zh-CN 缺的键').toEqual([])
  })

  it('源码中所有字面量 t() 键都存在于两份字典且为字符串叶子', () => {
    // ?raw 静态读入全库源码文本(vitest 走 Vite 管线,.vue 也可读);
    // 排除 i18n 目录自身与 spec 文件(测试代码里的 t() 不是运行时 UI 引用)。
    const sources = import.meta.glob('../**/*.{vue,ts}', {
      query: '?raw',
      import: 'default',
      eager: true,
    }) as Record<string, string>

    // 匹配裸 t(' / $t(' / xxx.t('(即 i18n.global.t);负向后行排除 it(/split(/wait( 等
    // 以字母结尾的普通函数名。仅提取单引号字面量键,且字面量须为完整实参(后随 , 或 ))。
    const keyPattern = /(?<![\w$])(?:\$t|t)\(\s*'([^']+)'\s*[,)]/g

    const used = new Map<string, string[]>() // key → 引用文件列表
    for (const [path, text] of Object.entries(sources)) {
      if (path.includes('/i18n/') || path.endsWith('.spec.ts')) continue
      for (const m of text.matchAll(keyPattern)) {
        const key = m[1]
        if (!used.has(key)) used.set(key, [])
        used.get(key)!.push(path)
      }
    }
    expect(used.size, '至少应提取到一个 t() 字面量键(提取器自检)').toBeGreaterThan(0)

    const problems: string[] = []
    for (const [key, files] of used) {
      for (const [name, dict] of [
        ['zh-CN', zhCN],
        ['en-US', enUS],
      ] as const) {
        const v = resolvePath(dict as LocaleTree, key)
        if (v === undefined) problems.push(`缺键 ${key}(${name})← ${files[0]}`)
        else if (typeof v !== 'string') problems.push(`非叶子 ${key}(${name})← ${files[0]}`)
      }
    }
    expect(problems, '所有 t() 字面量键须在两份字典中解析为字符串').toEqual([])
  })

  it('settingsMap.ts 与 cacheStats 行的 label/descKey/labelKey 属性值都存在于两份字典且为字符串叶子', () => {
    // settingsMap.ts 的 SettingSpec.label/descKey 与选项 labelKey、SettingsView.vue 的
    // cacheStats 行 labelKey,均是运行时经 t(spec.label) 之类动态解析的属性值,不落在上面
    // t() 字面量提取器覆盖面内(实参不是字符串字面量,是变量/属性访问)。只在这两个已知落点
    // 扫描 `(label|descKey|labelKey): '...'` 形态的属性;用「含点号」过滤掉 SettingOptionSpec.label
    // 的原文展示值(如 '简体中文'/'English',不含点、非 i18n key 形态,故意不当 key 处理)。
    const sources = import.meta.glob('../**/*.{vue,ts}', {
      query: '?raw',
      import: 'default',
      eager: true,
    }) as Record<string, string>

    const propPattern = /\b(?:label|descKey|labelKey):\s*'([^']+)'/g

    const used = new Map<string, string[]>()
    for (const [path, text] of Object.entries(sources)) {
      if (!path.endsWith('constants/settingsMap.ts') && !path.endsWith('views/SettingsView.vue')) {
        continue
      }
      for (const m of text.matchAll(propPattern)) {
        const key = m[1]
        if (!key.includes('.')) continue
        if (!used.has(key)) used.set(key, [])
        used.get(key)!.push(path)
      }
    }
    expect(
      used.size,
      '至少应提取到一个 settingsMap/cacheStats 属性键(提取器自检)'
    ).toBeGreaterThan(0)

    const problems: string[] = []
    for (const [key, files] of used) {
      for (const [name, dict] of [
        ['zh-CN', zhCN],
        ['en-US', enUS],
      ] as const) {
        const v = resolvePath(dict as LocaleTree, key)
        if (v === undefined) problems.push(`缺键 ${key}(${name})← ${files[0]}`)
        else if (typeof v !== 'string') problems.push(`非叶子 ${key}(${name})← ${files[0]}`)
      }
    }
    expect(problems, '所有 settingsMap/cacheStats 属性键须在两份字典中解析为字符串').toEqual([])
  })
})
