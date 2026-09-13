import { describe, it, expect } from 'vitest'
import {
  buildFormatGroups,
  filterGroupsByQuery,
  pruneFormatsForTypes,
  selectGroup,
  toggleOption,
  type FormatGroup,
} from './formatFilter.helpers'
import type { FormatDescriptor } from '../../types/format'
import type { MediaType } from '../../types/media'

function def(
  ext: string,
  mediaType: MediaType,
  group: string | null = null,
  pluginId?: string,
): FormatDescriptor {
  return {
    ext,
    mediaType,
    group,
    source: pluginId ? { kind: 'exotic', pluginId } : { kind: 'builtin' },
  }
}

// 后端 registry 的缩样。**区分度是刻意的**：
// - jpg/jpeg 同组 JPEG —— 实测库内 jpg 515,728 与 jpeg 2,530 并存，不合组则 JPEG chip 是个
//   2,530 项的心智陷阱（D-003）；
// - cr2/nef 同组 RAW 且**默认不在库内** —— 用来钉「零数据不显示，有数据自动显示」（D-004）；
// - png 无 group —— 单扩展名项；
// - psd 是 exotic —— 钉 badge 的来源判定；
// - mp4/txt/mp3 各占一个大类 —— 钉大类兼容与分组顺序。
const REGISTRY: FormatDescriptor[] = [
  def('jpg', 'image', 'JPEG'),
  def('jpeg', 'image', 'JPEG'),
  def('png', 'image'),
  def('cr2', 'image', 'RAW'),
  def('nef', 'image', 'RAW'),
  def('psd', 'image', null, 'exotic-image-psd'),
  def('mp4', 'video'),
  def('txt', 'document'),
  def('mp3', 'audio'),
]

const set = (...xs: string[]) => new Set(xs)
const labels = (gs: FormatGroup[], mt: MediaType) =>
  gs.find((g) => g.mediaType === mt)?.options.map((o) => o.label) ?? []

describe('buildFormatGroups', () => {
  /** §7.1：只列**库内实际存在**的格式。registry 67 vs 库内 29 → 砍掉 38 个必然空结果的选项。 */
  it('只列库内实际存在的扩展名(registry 有而库里没有的不出现)', () => {
    const gs = buildFormatGroups(REGISTRY, set('png'), set(), [])
    expect(labels(gs, 'image')).toEqual(['PNG'])
    // mp4 在 registry 里但库内没有 → 整个 video 组不出现（而非出现一个空组）。
    expect(gs.map((g) => g.mediaType)).toEqual(['image'])
  })

  /**
   * 🔴 别名合组（D-003）：jpg 与 jpeg 塌成一个 JPEG 选项。
   *
   * 可证伪性:样本 jpg/jpeg 同组而 png 无组 —— 任何「不合组、逐扩展名列」的实现会得到
   * ['JPG','JPEG','PNG'] 而非 ['JPEG','PNG']。
   */
  it('同 group 的扩展名合成一项,label 取组名并大写', () => {
    const gs = buildFormatGroups(REGISTRY, set('jpg', 'jpeg', 'png'), set(), [])
    expect(labels(gs, 'image')).toEqual(['JPEG', 'PNG'])
    const jpeg = gs[0].options[0]
    expect(jpeg.exts).toEqual(['jpg', 'jpeg'])
  })

  /** 别名组只把**库内存在**的成员算进去:库里只有 jpg 时 JPEG 项不该声称自己代表 jpeg。 */
  it('别名组与库内存在取交集', () => {
    const gs = buildFormatGroups(REGISTRY, set('jpg'), set(), [])
    expect(gs[0].options[0].exts).toEqual(['jpg'])
  })

  /**
   * 🔴 D-004:RAW **零数据时不显示,有数据自动显示** —— 这是数据驱动而非条件代码。
   *
   * 可证伪性:同一 registry、同一调用,只换 present 集合,RAW 组的出没随之翻转。任何把 RAW
   * 写成条件分支(或反过来硬编码常显)的实现都过不了其中一半。
   */
  it('RAW 零数据不显示;库里出现任一 RAW 扩展名即自动显示', () => {
    expect(labels(buildFormatGroups(REGISTRY, set('png'), set(), []), 'image')).not.toContain('RAW')
    const withRaw = buildFormatGroups(REGISTRY, set('png', 'cr2'), set(), [])
    expect(labels(withRaw, 'image')).toContain('RAW')
    expect(withRaw[0].options.find((o) => o.label === 'RAW')?.exts).toEqual(['cr2'])
  })

  it('分组按 图片→视频→文档→音频 呈现(与顶栏 chip 同序)', () => {
    const gs = buildFormatGroups(REGISTRY, set('png', 'mp4', 'txt', 'mp3'), set(), [])
    expect(gs.map((g) => g.mediaType)).toEqual(['image', 'video', 'document', 'audio'])
  })

  /**
   * 🔴 §7.5:选了媒体大类时只展示兼容组。
   *
   * 不做的后果是用户能选出「图片 AND (MP4)」这种恒空组合,而且冲突**不可见**(弹层一收
   * 只剩「格式 1」)。
   */
  it('选了媒体大类只展示兼容组;未选大类展示全部', () => {
    const only = buildFormatGroups(REGISTRY, set('png', 'mp4', 'txt'), set(), ['image'])
    expect(only.map((g) => g.mediaType)).toEqual(['image'])
    const all = buildFormatGroups(REGISTRY, set('png', 'mp4', 'txt'), set(), [])
    expect(all.map((g) => g.mediaType)).toEqual(['image', 'video', 'document'])
  })

  it('多选大类展示多组', () => {
    const gs = buildFormatGroups(REGISTRY, set('png', 'mp4', 'txt'), set(), ['image', 'document'])
    expect(gs.map((g) => g.mediaType)).toEqual(['image', 'document'])
  })

  /** 别名组的三态:全选 / 部分选(URL 深链只带 jpg)/ 未选。 */
  it('别名组的选中态:全选 selected、半选 partial、未选皆假', () => {
    const present = set('jpg', 'jpeg')
    const full = buildFormatGroups(REGISTRY, present, set('jpg', 'jpeg'), [])[0].options[0]
    expect([full.selected, full.partial]).toEqual([true, false])
    const half = buildFormatGroups(REGISTRY, present, set('jpg'), [])[0].options[0]
    expect([half.selected, half.partial]).toEqual([false, true])
    const none = buildFormatGroups(REGISTRY, present, set(), [])[0].options[0]
    expect([none.selected, none.partial]).toEqual([false, false])
  })

  /** availability badge 的来源判定:exotic 项带 pluginId,内置项为 null。 */
  it('exotic 项带 pluginId,内置项为 null', () => {
    const gs = buildFormatGroups(REGISTRY, set('png', 'psd'), set(), [])
    const by = (l: string) => gs[0].options.find((o) => o.label === l)
    expect(by('PSD')?.pluginId).toBe('exotic-image-psd')
    expect(by('PNG')?.pluginId).toBeNull()
  })

  it('空库 → 无分组(而非四个空组)', () => {
    expect(buildFormatGroups(REGISTRY, set(), set(), [])).toEqual([])
  })
})

describe('toggleOption', () => {
  const present = set('jpg', 'jpeg', 'png')
  const jpegOpt = (selected: Set<string>) =>
    buildFormatGroups(REGISTRY, present, selected, [])[0].options[0]

  it('未选 → 补齐该项全部扩展名(别名组一次进两个)', () => {
    expect(toggleOption(set(), jpegOpt(set())).sort()).toEqual(['jpeg', 'jpg'])
  })

  it('全选 → 移除该项全部扩展名', () => {
    expect(toggleOption(set('jpg', 'jpeg'), jpegOpt(set('jpg', 'jpeg')))).toEqual([])
  })

  /** 半选点击走**补齐**而非清空:用户看到半勾的 JPEG,点它的意图是「我要 JPEG」。 */
  it('半选 → 补齐(不是清空)', () => {
    expect(toggleOption(set('jpg'), jpegOpt(set('jpg'))).sort()).toEqual(['jpeg', 'jpg'])
  })

  it('不动其他项的选中态', () => {
    const next = toggleOption(set('png'), jpegOpt(set('png')))
    expect(next).toContain('png')
  })
})

describe('selectGroup', () => {
  it('并入该组全部扩展名(含别名组展开),幂等', () => {
    const gs = buildFormatGroups(REGISTRY, set('jpg', 'jpeg', 'png'), set(), [])
    const once = selectGroup(set(), gs[0]).sort()
    expect(once).toEqual(['jpeg', 'jpg', 'png'])
    expect(selectGroup(new Set(once), gs[0]).sort()).toEqual(once)
  })
})

// ── 跨维度剪枝（§7.5）──────────────────────────────────────────────────────────
describe('pruneFormatsForTypes', () => {
  const extToType = new Map<string, MediaType>(REGISTRY.map((d) => [d.ext, d.mediaType]))

  /**
   * 🔴 取消大类时同步移除该类已选格式,否则留下**不可见的冲突筛选**:
   * 选了「图片 + PNG」再取消「图片」→ PNG 还在筛,而顶栏只显示「格式 1」,用户看不出。
   */
  it('取消某大类 → 移除该类下已选格式,保留其他类的', () => {
    expect(pruneFormatsForTypes(['png', 'mp4'], ['video'], extToType)).toEqual(['mp4'])
  })

  it('大类为空(不限)→ 一个不剪', () => {
    expect(pruneFormatsForTypes(['png', 'mp4'], [], extToType)).toEqual(['png', 'mp4'])
  })

  /**
   * registry 不认识的扩展名予以**保留**:可能来自 URL 深链或刚被卸载的插件。悄悄丢掉用户
   * 明确表达过的筛选,比留着更坏 —— 留着最多筛出零条(诚实的空结果)。
   */
  it('registry 不认识的扩展名保留(不替用户做主)', () => {
    expect(pruneFormatsForTypes(['png', 'xyz'], ['image'], extToType)).toEqual(['png', 'xyz'])
  })

  it('保序(选中集顺序是用户的点击序)', () => {
    expect(pruneFormatsForTypes(['mp4', 'png', 'txt'], ['image', 'video'], extToType)).toEqual([
      'mp4',
      'png',
    ])
  })
})

describe('filterGroupsByQuery', () => {
  const gs = buildFormatGroups(REGISTRY, set('jpg', 'jpeg', 'png', 'mp4'), set(), [])

  it('空查询不过滤', () => {
    expect(filterGroupsByQuery(gs, '  ')).toEqual(gs)
  })

  it('按标签匹配(大小写不敏感)', () => {
    expect(labels(filterGroupsByQuery(gs, 'jpe'), 'image')).toEqual(['JPEG'])
  })

  /** 按**扩展名**匹配:用户搜 "jpg" 该能命中标签为 JPEG 的那一项(标签里没有 "jpg" 三个字)。 */
  it('按扩展名匹配(标签与扩展名不同形时仍可搜到)', () => {
    expect(labels(filterGroupsByQuery(gs, 'jpg'), 'image')).toEqual(['JPEG'])
  })

  it('整组无命中则该组不出现(不留空组)', () => {
    const r = filterGroupsByQuery(gs, 'mp4')
    expect(r.map((g) => g.mediaType)).toEqual(['video'])
  })
})
