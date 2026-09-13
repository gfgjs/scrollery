// 细分格式弹层的**纯逻辑**（S 线 §7 / D-003、D-004、D-005、D-011、D-012）。
//
// 无 Vue / 无 IPC：输入是「registry 描述符 + 库内实际存在的扩展名 + 已选状态」，输出是要渲染的
// 分组树。抽出来是因为这层的正确性全是**集合运算**（别名合组、大类兼容、跨维度剪枝），
// 而集合运算不该需要挂载一个组件才能验。

import type { MediaType } from '../../types/media'
import type { FormatDescriptor } from '../../types/format'

/** 弹层里的一个可选项：要么是单个扩展名，要么是一个别名组（JPEG={jpg,jpeg}）。 */
export interface FormatOption {
  /** 展示标签，统一大写（§7.1）。别名组用 group 名，否则用扩展名。 */
  label: string
  /**
   * 该项代表的**具体扩展名**（已与库内实际存在取交集）。
   *
   * 别名组展开成多个 —— 核心状态永远是具体扩展名，`group` 只是 UI 概念，不落库不进 URL（D-003）。
   */
  exts: string[]
  /** 全部 `exts` 都被选中。 */
  selected: boolean
  /** 选了一部分（URL 深链可能只带 `formats=jpg` 而 JPEG 组还有 jpeg）。 */
  partial: boolean
  /** 该项是否来自 exotic 插件；`null` = 内置。用于 availability badge（§7.1）。 */
  pluginId: string | null
}

/** 一个媒体大类分组。 */
export interface FormatGroup {
  mediaType: MediaType
  options: FormatOption[]
}

/** 四大类的**呈现顺序**，与顶栏 chip 顺序一致（图片 视频 文档 音频）。 */
const GROUP_ORDER: readonly MediaType[] = ['image', 'video', 'document', 'audio']

/**
 * 组装弹层分组树。
 *
 * @param registry 全部已注册格式（`list_registered_formats` 下发，内置 ∪ Catalog）。
 * @param present 库内**实际存在**的扩展名（`list_library_formats` 的 facet）。
 * @param selected 当前已选的具体扩展名。
 * @param mediaTypes 顶栏已选的媒体大类；空 = 不限。
 *
 * ## 只列库内实际存在的（§7.1）
 *
 * registry 当前 67 项而本机库内只有 29 → 全铺出来是 38 个点了必然空结果的选项。RAW 与
 * heic/heif/avif 在本机全为 0，故**不显示** —— 但这是**数据驱动**而非条件代码：库里出现任一
 * RAW 扩展名，RAW 组自动出现（D-004）。
 *
 * ## 大类兼容（§7.5 / D-011）
 *
 * 选了媒体大类时只展示兼容组 —— 否则用户能选出「图片 AND (MP4)」这种恒空的组合，而且那个
 * 冲突还是**不可见**的（格式弹层收起后只剩一个「格式 1」）。
 */
export function buildFormatGroups(
  registry: readonly FormatDescriptor[],
  present: ReadonlySet<string>,
  selected: ReadonlySet<string>,
  mediaTypes: readonly string[],
): FormatGroup[] {
  const groups: FormatGroup[] = []
  for (const mt of GROUP_ORDER) {
    // 大类兼容：选了大类就只展示兼容组；没选大类 = 不限 = 全展示。
    if (mediaTypes.length > 0 && !mediaTypes.includes(mt)) continue

    // 库内存在 + 属于本大类的 registry 项。
    const defs = registry.filter((d) => d.mediaType === mt && present.has(d.ext))
    if (defs.length === 0) continue

    // 别名合组：同 group 的多个扩展名塌成一项；无 group 的各自成项。
    // 用 Map 保 registry 的既有序（后端已按 ext 升序），不再自行排序 —— 序的事实源在后端。
    //
    // 键的构造要保证「组名空间」与「扩展名空间」不撞车：扩展名形态是 `[a-z0-9]{1,16}`
    // （后端 is_valid_format 保证），故 `#` 前缀足以隔开，无需靠不可见字符当分隔符。
    const byKey = new Map<string, FormatDescriptor[]>()
    for (const d of defs) {
      const key = d.group ?? `#ext:${d.ext}`
      const bucket = byKey.get(key)
      if (bucket) bucket.push(d)
      else byKey.set(key, [d])
    }

    const options: FormatOption[] = []
    for (const members of byKey.values()) {
      const exts = members.map((d) => d.ext)
      const hit = exts.filter((e) => selected.has(e)).length
      options.push({
        label: (members[0].group ?? members[0].ext).toUpperCase(),
        exts,
        selected: hit === exts.length,
        partial: hit > 0 && hit < exts.length,
        // 别名组理论上可跨 source，取首个成员的来源即可：内置组（JPEG/RAW）恒为内置，
        // 而 exotic 侧的 group 恒为 null（Catalog schema 无 group 元数据，D-007）→ 组内恒同源。
        pluginId: members[0].source.kind === 'exotic' ? members[0].source.pluginId : null,
      })
    }
    groups.push({ mediaType: mt, options })
  }
  return groups
}

/**
 * 点击某项后的新选中集合。
 *
 * 已全选 → 取消该项全部扩展名；未选或**部分选** → 补齐该项全部扩展名。
 * 部分选走「补齐」而非「清空」：用户看到一个半勾的 JPEG，点它的意图是「我要 JPEG」。
 */
export function toggleOption(selected: ReadonlySet<string>, opt: FormatOption): string[] {
  const next = new Set(selected)
  if (opt.selected) for (const e of opt.exts) next.delete(e)
  else for (const e of opt.exts) next.add(e)
  return [...next]
}

/** 「全选本组」：把该组全部选项的扩展名并入。 */
export function selectGroup(selected: ReadonlySet<string>, group: FormatGroup): string[] {
  const next = new Set(selected)
  for (const o of group.options) for (const e of o.exts) next.add(e)
  return [...next]
}

/**
 * 🔴 跨维度剪枝（§7.5）：取消某媒体大类时，同步移除该类下已选的细分格式。
 *
 * 不剪的后果是**不可见的冲突筛选**：用户选了「图片 + PNG」，再取消「图片」chip —— PNG 还留在
 * 选中集里，但弹层此时展示全部大类、而顶栏只显示「格式 1」。用户看不出自己还在筛 PNG，只看到
 * 结果莫名其妙。
 *
 * 保留 registry 里**不认识**的扩展名（不在 `extToType` 中）：那可能来自 URL 深链或一个刚被卸载
 * 的插件。悄悄丢掉用户明确表达过的筛选，比留着更坏 —— 留着最多是筛出零条（诚实的空结果）。
 *
 * @param extToType registry 的 `ext → mediaType` 映射。
 */
export function pruneFormatsForTypes(
  selected: readonly string[],
  mediaTypes: readonly string[],
  extToType: ReadonlyMap<string, MediaType>,
): string[] {
  if (mediaTypes.length === 0) return [...selected] // 不限大类 → 无从冲突
  return selected.filter((ext) => {
    const mt = extToType.get(ext)
    if (mt === undefined) return true // registry 不认识 → 不替用户做主
    return mediaTypes.includes(mt)
  })
}

/** 搜索过滤：按标签或任一扩展名做子串匹配（大小写不敏感）。空串 = 不过滤。 */
export function filterGroupsByQuery(groups: readonly FormatGroup[], query: string): FormatGroup[] {
  const q = query.trim().toLowerCase()
  if (!q) return [...groups]
  const out: FormatGroup[] = []
  for (const g of groups) {
    const options = g.options.filter(
      (o) => o.label.toLowerCase().includes(q) || o.exts.some((e) => e.includes(q)),
    )
    if (options.length) out.push({ ...g, options })
  }
  return out
}
