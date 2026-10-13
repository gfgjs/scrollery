import { IPC } from '../constants/ipc'
import {
  uiHarnessAvail,
  uiHarnessAvailRatio,
  uiHarnessAppearance,
  uiHarnessHoverCards,
  uiHarnessItems,
  uiHarnessRenderMode,
  uiHarnessRowHeight,
  uiHarnessSeed,
  uiHarnessThumbStatus,
} from './runtime'
import { serializeThemeSeed } from '../themes/config'
import { DEFAULT_THEME_DEFINITION } from '../themes/presets'
import type { ThemeSeed } from '../themes/types'
import type { IpcCommand } from '../utils/ipc'
import type { LayoutRow, LayoutRowItem, LayoutSummary, MediaMeta } from '../types/layout'
import type {
  AppStats,
  DirNode,
  MediaDetail,
  MediaType,
  ScanRoot,
  ThumbResult,
  ThumbStatus,
} from '../types/media'
import type { DedupStatusSnapshot } from '../types/ipc'
import type { SettingsChange, SettingsSnapshot, StartupPayload } from '../types/config'
import type { PersonSummary } from '../types/person'

const SCENE_VERSION = 1
const activatedHarnessFeatures = new Set<string>()
const freeActivationIds = ['feature-editing', 'exotic-ocr', 'exotic-enhance', 'exotic-image-psd']

// 阅读编辑验收使用独立深链样例，不改变图库数量，也不会读写真实文件。
// /doc/900001?engine=markdown-editor；本页内保存到内存，刷新后恢复样例。
const markdownFixtures = new Map<number, string>([
  [
    900001,
    '# 阅读与编辑样例\n\n这是一段 **粗体** 与 *斜体* 的中文 Markdown。\n\n## 清单\n\n- 第一项\n- 第二项\n\n## 代码\n\n```ts\nconst answer = 42\n```\n\n## 表格\n\n| 名称 | 状态 |\n| --- | --- |\n| 样例 | 就绪 |\n',
  ],
  [900002, '# 第二篇文档\n\n用于验证切换文档后的保存归属。\n'],
  // 长段落用于验证查找滚动到命中文字，而非只滚到段落边界。
  [
    900003,
    `# 长段落查找样例\n\n${'这是一段用于检查长文定位的连续正文。'.repeat(240)}定位终点\n\n## 文档结尾\n\n长段落样例结束。\n`,
  ],
  [
    900004,
    '# 相对图片样例\n\n![目录内的图片](<images/示例 image.svg>)\n\n![上级目录的图片](../shared/reference.svg)\n\n相对路径在编辑保存后应继续可移植。\n',
  ],
  [
    900005,
    '纯文本阅读与编辑\n\n进入编辑后仍采用阅读字号，并可保存后继续写作。\n\n取消前应保留尚未保存的草稿。\n',
  ],
])
let markdownVersionId = 1
const markdownVersions = new Map<number, { itemId: number; content: string }>()
const markdownCurrentVersions = new Map<number, number>()
const readerBookPrefs = new Map<number, string>()

function effectiveDocumentText(itemId: unknown): string {
  return (
    markdownVersions.get(markdownCurrentVersions.get(Number(itemId)) ?? -1)?.content ??
    markdownFixture(itemId)
  )
}

function markdownFixture(itemId: unknown): string {
  const content = markdownFixtures.get(Number(itemId))
  if (content === undefined) throw new Error('[UI harness] 未定义该文档样例')
  return content
}

// 人物墙编辑验证只用合成人物，不加载真实人脸或磁盘内容。
const personFixtures: PersonSummary[] = [
  {
    id: 1,
    name: '示例人物',
    isNamed: true,
    isHidden: false,
    faceCount: 3,
    coverItemId: null,
    coverThumbPath: null,
    coverThumbStatus: null,
    coverBbox: null,
  },
  {
    id: 2,
    name: null,
    isNamed: false,
    isHidden: false,
    faceCount: 1,
    coverItemId: null,
    coverThumbPath: null,
    coverThumbStatus: null,
    coverBbox: null,
  },
]
const ignoredPersonFixtures: PersonSummary[] = [
  {
    id: 3,
    name: '示例误检',
    isNamed: true,
    isHidden: false,
    faceCount: 1,
    coverItemId: null,
    coverThumbPath: null,
    coverThumbStatus: null,
    coverBbox: null,
  },
]

const samples = [
  ['山湖晨雾', '#6f8f9d', '#d9e7e8', 1.45],
  ['城市光影', '#5d617c', '#edc58a', 1.1],
  ['海岸公路', '#507b92', '#d8b56c', 1.65],
  ['室内静物', '#7e665f', '#d8c7b2', 0.82],
  ['玻璃花房', '#547a68', '#c9dec1', 1.32],
  ['夜色街角', '#30384f', '#e8a85c', 1.5],
  ['雪原小屋', '#8a9da8', '#f0eee8', 1.2],
  ['森林步道', '#496655', '#c7ad7f', 0.76],
  ['晚霞云层', '#9d6574', '#efbd8d', 1.7],
  ['咖啡与书', '#73594a', '#d8c29f', 1.05],
  ['港湾蓝调', '#41647d', '#acc9d6', 1.5],
  ['春日花园', '#66885e', '#efc8cf', 1.26],
  ['建筑几何', '#626b75', '#d9d6cc', 0.9],
  ['岛屿晴空', '#347b9a', '#e4d59a', 1.6],
  ['雨后车窗', '#465867', '#bdccd1', 1.15],
  ['老城巷口', '#866c55', '#d6b98c', 0.78],
  ['草原远山', '#6f8656', '#b9d2df', 1.75],
  ['月下海面', '#263f61', '#9eb4d1', 1.38],
] as const

const fixtureItemCount = uiHarnessItems ?? samples.length

function baseSampleIndex(index: number): number {
  return ((index % samples.length) + samples.length) % samples.length
}

function sampleAt(index: number): (typeof samples)[number] {
  return samples[baseSampleIndex(index)]
}

function sampleSvg(index: number): string {
  const [label, dark, light] = sampleAt(index)
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="960" height="720" viewBox="0 0 960 720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop stop-color="${dark}"/><stop offset="1" stop-color="${light}"/></linearGradient></defs><rect width="960" height="720" fill="url(#g)"/><circle cx="760" cy="140" r="88" fill="#fff" opacity=".3"/><path d="M0 520 210 310l170 150 150-210 260 270 170-120v320H0z" fill="#111827" opacity=".28"/><path d="M0 580 260 420l170 100 230-170 300 190v180H0z" fill="#fff" opacity=".2"/><text x="54" y="650" fill="#fff" font-size="34" font-family="system-ui,sans-serif" opacity=".9">${label}</text></svg>`
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}

/**
 * 少数样本改为非 image,好让 harness/主题矩阵覆盖 AUDIO(绿)/DOC(橙)类型角标 —— 否则整套
 * fixture 全是 image,S7 新建的这两个徽章 token 在任何场景里都渲染不出来、等于不在覆盖内。
 *
 * 取值须满足 `typeBadgeOf(mediaType, fileFormat, thumbStatus)`:audio 恒出角标;document 仅在
 * 「走真实缩略图」时出(pdf/svg/有封面 epub),故给 pdf + thumbStatus=3;txt/md 那类纸张卡另有
 * 扩展名角标、有意不出 DOC。索引避开 5/13(LivePhoto)与 0/6/12(星级)、0/7/14(色标),免叠加干扰。
 */
const NON_IMAGE_SAMPLES: Readonly<Record<number, { mediaType: MediaType; fileFormat: string }>> = {
  4: { mediaType: 'audio', fileFormat: 'mp3' },
  11: { mediaType: 'document', fileFormat: 'pdf' },
}

function kindOf(index: number): { mediaType: MediaType; fileFormat: string } {
  return NON_IMAGE_SAMPLES[baseSampleIndex(index)] ?? { mediaType: 'image', fileFormat: 'jpg' }
}

/**
 * `&hoverCards=1` 的固定悬停样本(仅该显式参数开启时生效,默认时所有样本与既有主题截图/性能
 * 场景逐字节一致):覆盖「在线无缩略图 MD 文档卡 / missing 无缩略图 MD / 正常图片 / 视频」四条
 * 悬停路径,并给两篇 MD 配上评分与收藏,配合长文件名核对悬停卡信息行的碰撞。
 *
 * mediaType=document + fileFormat=md + thumbStatus=2(失败) + thumbPath=null 即真实的「文档缩略图
 * 未生成」纸张卡路径;md 按 typeBadgeOf 不出 DOC 角标,与生产一致。视频只覆盖播放角标与 <video>
 * 挂载:harness 没有真实视频字节,不保证浏览器能解码 HOVER_VIDEO_ABS_PATH 的内容。
 */
type HoverFixturePatch = {
  mediaType: MediaType
  fileFormat?: string
  thumbStatus?: ThumbStatus
  thumbPath?: string | null
  isFavorited?: boolean
  rating?: number
  availability?: string
  durationMs?: number | null
}

const HOVER_VIDEO_ABS_PATH = `${typeof window === 'undefined' ? '' : (window.location?.origin ?? '')}/harness/documents/hover-card-sample-video.mp4`

const HOVER_FIXTURES: Readonly<Record<number, HoverFixturePatch>> = {
  // 0=在线无缩略图 MD 文档卡(已评分/收藏)
  0: {
    mediaType: 'document',
    fileFormat: 'md',
    thumbStatus: 2,
    thumbPath: null,
    isFavorited: true,
    rating: 5,
  },
  // 1=missing 无缩略图 MD(已评分/收藏),配合超长元数据文件名验证信息行碰撞
  1: {
    mediaType: 'document',
    fileFormat: 'md',
    thumbStatus: 2,
    thumbPath: null,
    isFavorited: true,
    rating: 4,
    availability: 'missing',
  },
  // 2 保持正常图片(不覆盖)
  // 3=视频:直接源无路径，核对无位图占位与播放角标。
  3: { mediaType: 'video', fileFormat: 'mp4', thumbStatus: 3, thumbPath: null, durationMs: 12_000 },
}

/** 仅 GET_META_FOR_VIEWPORT 用:长文件名样本用于悬停卡信息行碰撞验证。 */
const HOVER_FIXTURE_FILE_NAMES: Readonly<Record<number, string>> = {
  0: '悬停卡样例-在线MD文档-缩略图未生成.md',
  1: '悬停卡样例-季度结算与预算复核说明-编号2026-Q4-终版-请勿修改-超长文件名用于悬停卡信息行碰撞.md',
  3: '悬停卡样例-城市延时短片.mp4',
}

function hoverFixtureOverride(index: number): HoverFixturePatch | null {
  if (!uiHarnessHoverCards) return null
  return HOVER_FIXTURES[baseSampleIndex(index)] ?? null
}

function hoverFixtureFileName(index: number): string | null {
  if (!uiHarnessHoverCards) return null
  return HOVER_FIXTURE_FILE_NAMES[baseSampleIndex(index)] ?? null
}

/** 高占比可用性场景(&avail=&availRatio=):前 ratio 比例条目按序标 offline/missing,其余 online。 */
function availabilityOf(index: number): LayoutRowItem['availability'] {
  if (uiHarnessAvail && index < Math.floor(fixtureItemCount * uiHarnessAvailRatio)) {
    return uiHarnessAvail
  }
  return 'online'
}

function createItem(index: number, x: number, y: number, w: number, h: number): LayoutRowItem {
  const baseIndex = baseSampleIndex(index)
  const [, dark, , ratio] = sampleAt(index)
  const kind = kindOf(index)
  const item: LayoutRowItem = {
    id: index + 1,
    x,
    w,
    h,
    fileSize: 1_200_000 + index * 83_000,
    fileFormat: kind.fileFormat,
    mediaType: kind.mediaType,
    isLivePhoto: baseIndex === 5 || baseIndex === 13,
    durationMs: null,
    thumbStatus: uiHarnessThumbStatus ?? 3,
    // 默认 18 张视觉场景保留富 SVG；极密性能场景改用小型 raster。SVG 会让
    // createImageBitmap 裁剪拒绝后走整图 Image 回退，既不代表真实 WebP 管线，也会把
    // 960×720 解码体积错误放大到每格约 2.6MB，令 512MB 缓存只剩约 194 格。
    thumbPath: uiHarnessItems === null ? sampleSvg(index) : '/favicon.png',
    placeholderColor: dark,
    isFavorited: baseIndex === 2 || baseIndex === 9 || baseIndex === 14,
    rating: baseIndex % 6 === 0 ? 4 : 0,
    colorLabel: baseIndex % 7 === 0 ? 4 : 0,
    availability: availabilityOf(index),
    originalWidth: Math.round(1800 * ratio),
    originalHeight: 1800,
    sortDatetime: 1_767_225_600 - index * 18_000,
  }
  // &hoverCards=1:前几条固定样本换成悬停卡验证组(见 HOVER_FIXTURES);默认关时原样返回。
  const hoverPatch = hoverFixtureOverride(index)
  return hoverPatch ? { ...item, ...hoverPatch } : item
}

let layoutRows: LayoutRow[] = []
let layoutSummary: LayoutSummary = {
  totalRows: 0,
  totalHeight: 0,
  layoutVersion: SCENE_VERSION,
  orderVersion: SCENE_VERSION,
  totalItems: fixtureItemCount,
  separators: [],
  monthBuckets: [],
}

function computeFixtureLayout(
  containerWidth: number,
  targetHeight: number,
  gap: number,
): LayoutSummary {
  const width = Math.max(640, containerWidth)
  const rows: LayoutRow[] = []
  const separators: LayoutSummary['separators'] = []
  let y = 0
  let itemIndex = 0
  const firstGroupCount = Math.ceil((fixtureItemCount * 4) / 9)
  const secondGroupCount = Math.ceil((fixtureItemCount - firstGroupCount) * 0.6)
  const groups = [
    { label: '2026年 1月 22日', count: firstGroupCount },
    { label: '2026年 1月 18日', count: secondGroupCount },
    { label: '2026年 1月 12日', count: fixtureItemCount - firstGroupCount - secondGroupCount },
  ]

  for (let groupIndex = 0; groupIndex < groups.length; groupIndex++) {
    const group = groups[groupIndex]
    const groupId = `2026-01-${22 - groupIndex * 5}`
    rows.push({ rowType: 'separator', y, height: 38, separatorLabel: group.label, groupId })
    separators.push({
      label: group.label,
      y,
      groupId,
      count: group.count,
      epochDay: 20_475 - groupIndex * 5,
    })
    y += 38

    let remaining = group.count
    while (remaining > 0) {
      const estimatedPerRow = Math.max(1, Math.floor(width / (targetHeight * 1.25 + gap)))
      const count = Math.min(estimatedPerRow, remaining)
      const rowSamples = Array.from({ length: count }, (_, offset) => sampleAt(itemIndex + offset))
      const ratioSum = rowSamples.reduce((sum, sample) => sum + sample[3], 0)
      const height = Math.min(targetHeight, (width - gap * (count - 1)) / ratioSum)
      const items: LayoutRowItem[] = []
      let x = 0
      for (let i = 0; i < count; i++) {
        const itemWidth =
          i === count - 1 ? width - x : Math.round(height * sampleAt(itemIndex + i)[3])
        items.push(createItem(itemIndex + i, x, y, itemWidth, height))
        x += itemWidth + gap
      }
      rows.push({ rowType: 'normal', y, height, items })
      y += height + gap
      itemIndex += count
      remaining -= count
    }
  }

  layoutRows = rows
  layoutSummary = {
    totalRows: rows.length,
    totalHeight: y,
    layoutVersion: SCENE_VERSION,
    orderVersion: SCENE_VERSION,
    totalItems: fixtureItemCount,
    separators,
    monthBuckets: [{ year: 2026, month: 1, count: fixtureItemCount, y: 0, groupId: '2026-01' }],
  }
  return layoutSummary
}

const scanRoot: ScanRoot = {
  id: 1,
  path: 'C:/Scrollery 示例图库',
  alias: '示例图库',
  scanStatus: 'completed',
  scanProgress: 1,
  totalFiles: fixtureItemCount,
  lastScanAt: 1_767_225_600,
  isActive: true,
  createdAt: 1_767_225_600,
  updatedAt: 1_767_225_600,
  isHidden: false,
}

// 与 NON_IMAGE_SAMPLES 保持一致:侧栏计数若和网格里实际渲染的类型对不上,harness 自己就成了
// 一个会误导人的假页面。
const fixtureKinds = Array.from(
  { length: fixtureItemCount },
  (_, index) => hoverFixtureOverride(index)?.mediaType ?? kindOf(index).mediaType,
)
const audioCount = fixtureKinds.filter((kind) => kind === 'audio').length
const documentCount = fixtureKinds.filter((kind) => kind === 'document').length
const videoCount = fixtureKinds.filter((kind) => kind === 'video').length
const nonImageCount = audioCount + documentCount + videoCount

const stats: AppStats = {
  totalItems: fixtureItemCount,
  totalImages: fixtureItemCount - nonImageCount,
  totalVideos: videoCount,
  totalAudios: audioCount,
  totalDocuments: documentCount,
  totalFavorited: Array.from({ length: fixtureItemCount }, (_, index) =>
    hoverFixtureOverride(index)?.isFavorited ?? [2, 9, 14].includes(baseSampleIndex(index)),
  ).filter(Boolean).length,
  totalDeleted: 0,
  totalLivePhotos: Array.from({ length: fixtureItemCount }, (_, index) =>
    [5, 13].includes(baseSampleIndex(index)),
  ).filter(Boolean).length,
}

// ── 设置快照 fixture(设置集中保存,2026-09-16)──────────────────────────────────
//
// 真后端以 config.toml 为全部用户设置的唯一真源,快照里每个已注册键都带生效值;harness 只
// **覆盖视觉基线需要的键**——本表没有的键读取即 undefined,消费方回落到各自的产品默认
// (与旧 fixture 里留 null 的语义一致:harness 是视觉场景,不是「新装默认」模拟器)。
const SETTINGS_FIXTURE: Record<string, string> = {
  language: 'zh-CN',
  timeline_axis_width: '44',
  scroll_thumb_min_height: '28',
  ui_font_size: '15',
  enable_thumb_hover_scale: 'true',
  // 行高:显式 &rowHeight= 优先;否则视觉场景 220 / perf 场景 60(既有行为)。
  grid_row_height:
    uiHarnessRowHeight !== null
      ? String(uiHarnessRowHeight)
      : uiHarnessItems === null
        ? '220'
        : '60',
  group_by: 'date',
  sort_within_group: 'datetime',
  layout_mode: 'justified',
  close_behavior: 'ask',
  // 置顶设置/常驻工具:用 schema 默认值(两项常驻工具),使 harness 重置后的结果与真机默认一致。
  pinned_settings: '["aiFullAnalysis","faceFullAnalysis"]',
  // harness 是**视觉场景**不是「新装默认」模拟器(整套 fixture 本就是 18 张假图):这里刻意把
  // 缩略图信息开满,好让主题矩阵覆盖到徽章类 token(size 半透明黑底 / type 绿橙底 —— 正是 S7
  // 修的那批)。生产默认另有其值(showThumbInfo 默认开、elements 默认空 → 观感无徽章)。
  show_thumb_info: 'true',
  thumb_info_elements: uiHarnessHoverCards
    ? '["filename","type","size","status","favorite"]'
    : '["type","size","status"]',
  hover_autoplay: 'false',
  appearance: 'light',
  // 主题参数按新模型落两份扁平种子(结构转换由 themes/config.ts 负责,harness 不另拼形状);
  // 截图矩阵的 &appearance= / &seed=custom 覆盖在 resolveSettingsSnapshot 里,不写进本默认表。
  theme_light_palette: serializeThemeSeed(DEFAULT_THEME_DEFINITION.light),
  theme_dark_palette: serializeThemeSeed(DEFAULT_THEME_DEFINITION.dark),
  // 主题矩阵拍的是 DB 目录树的观感,与显示范围无关;留默认态,勿让 harness 走 FS 枚举路径
  // (那条路径在 harness 里没有真实磁盘可枚举)。
  tree_display_mode: 'registeredOnly',
  // 拖拽手柄(#5):留生产默认开,选中态截图覆盖手柄观感。
  show_drag_handle: 'true',
  // 无缝分组(#1):留生产默认关,视觉基线保分隔符观感。
  seamless_groups: 'false',
  // 无缝 minimap 轴:留生产默认开(无缝关时不渲染,基线截图不受影响)。
  seamless_minimap: 'true',
  // 渲染模式留生产默认缩略图;当前 fixture 不启用 minimap,不改变视觉基线。
  minimap_render_mode: 'thumbnails',
  // harness 不跑真日志管线,留生产默认 info 即可(off 会让 logger.ts 在 harness 里也不入队,
  // 无实际影响,但 'info' 更贴合「模拟正常运行态」的 harness 定位)。
  log_level: 'info',
  // 窗口材质(毛玻璃,2026-08-24):harness 非 Windows 桌面场景,不启用玻璃层。
  // 浏览器没有 DWM 背板，使用实色材质才能真实核对主题底色与文字对比度。
  window_material: 'none',
  window_opacity: '90',
  // 画廊渲染引擎:留生产默认 canvas;截图矩阵的 &render=dom 覆盖它,好让两条绘制路径都出图。
  gallery_render_mode: 'canvas',
  gallery_blend_enabled: 'false',
  gallery_blend_edge_width: '2',
  gallery_blend_blur_radius: '3',
  gallery_blend_strength: '100',
}

/**
 * harness 的设置快照状态(进程内):提交与重置只改这一份,并镜像后端的 revision / generation
 * 语义——revision 每次提交递增(前端据此丢弃过期回执),generation 只在重置时递增(旧代次
 * 提交被拒,防重置后其他窗口的迟到回写)。
 */
const settingsState = {
  values: { ...SETTINGS_FIXTURE },
  revision: 1,
  generation: 0,
}

/**
 * 内部状态(留 DB 的业务标记):启动批只读一次,逐键读写通道也只认这些键。
 * `first_launch` / `guide_seen` 决定引导是否重放,重置设置不动它们。
 */
const appState: Record<string, string> = {
  first_launch: 'false',
  guide_seen: 'true',
  // 备份最近成功时间(后台工作时间戳,非用户设置)。
  backup_last_success_at: '1784422800',
}
const APP_STATE_KEYS = Object.keys(appState)

/**
 * 截图矩阵的「明显自定义配色」种子(方案 §9 视觉矩阵要求的一组自定义色)。
 *
 * 刻意偏离出厂配色且两档极性相反,好让人眼在同一张矩阵里同时核对:自定义浅色、自定义深色、
 * 以及 accent 与状态色在非默认种子上的可读性。仍是**合法种子**,故走真实的
 * generateTheme → applyPalette 链路,视觉产物与用户自由选色时完全同源。
 */
const CUSTOM_SEEDS: { light: ThemeSeed; dark: ThemeSeed } = {
  light: {
    background: '#fff6ec',
    foreground: '#33241a',
    accent: '#c2410c',
    contrast: 70,
    gallery: 'auto',
  },
  dark: {
    background: '#141026',
    foreground: '#efeaff',
    accent: '#f472b6',
    contrast: 70,
    gallery: 'auto',
  },
}

/**
 * 截图矩阵用:`&appearance=light|dark` 指定呈现档,`&seed=custom` 换成自定义配色,
 * `&render=dom|canvas` 指定画廊绘制路径。
 *
 * 有意走设置快照而**不是**直接写 data-* / CSS 变量——后者会让截图为一条产品里不存在的路径
 * 背书。经此处则完整跑真实链:快照 → themeStore 的结构转换与校验 → generateTheme →
 * applyPalette。故非法值的归位行为亦与生产一致。
 */
function resolveSettingsSnapshot(): SettingsSnapshot {
  const values = { ...settingsState.values }
  if (uiHarnessAppearance !== null) values.appearance = uiHarnessAppearance
  if (uiHarnessSeed === 'custom') {
    values.theme_light_palette = serializeThemeSeed(CUSTOM_SEEDS.light)
    values.theme_dark_palette = serializeThemeSeed(CUSTOM_SEEDS.dark)
  }
  if (uiHarnessRenderMode !== null) values.gallery_render_mode = uiHarnessRenderMode
  return { values, revision: settingsState.revision, generation: settingsState.generation }
}

/**
 * 提交一批设置(镜像 set_app_settings):落值 → revision 递增 → 回最新快照与本次真正变化的键。
 * 与后端一致,回执里 keys 只列真变了的键(同值的键不算变化)。
 */
function applySettingsPatch(patch: Record<string, string>): SettingsChange {
  const keys: string[] = []
  for (const [key, value] of Object.entries(patch)) {
    if (settingsState.values[key] === value) continue
    settingsState.values[key] = value
    keys.push(key)
  }
  settingsState.revision += 1
  return { snapshot: resolveSettingsSnapshot(), keys, restart_required: [], apply_failed: [] }
}

/**
 * 恢复默认设置(镜像 clear_settings):整份回到 fixture 默认 → generation 与 revision 递增。
 * harness 里「默认」即 SETTINGS_FIXTURE(真后端是 schema 默认模板);不触碰内部状态,故引导
 * 不会重放。
 */
function resetSettingsFixture(): SettingsChange {
  const before = settingsState.values
  const after = { ...SETTINGS_FIXTURE }
  // 键集取两侧并集:被提交写进来、默认表里没有的键同样是「这次重置改变了的键」。
  const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(
    (key) => before[key] !== after[key],
  )
  settingsState.values = after
  settingsState.generation += 1
  settingsState.revision += 1
  return { snapshot: resolveSettingsSnapshot(), keys, restart_required: [], apply_failed: [] }
}

/** 内部状态逐键读写:设置类键与外键一律拒绝(与后端 ensure_state_key 同口径)。 */
function requireAppStateKey(key: unknown): string {
  const name = String(key ?? '')
  if (!APP_STATE_KEYS.includes(name)) {
    throw new Error(
      `[UI harness] ${name} 不是内部状态键;用户设置请走 get_settings_snapshot / set_app_settings`,
    )
  }
  return name
}

function mediaDetail(id: number): MediaDetail {
  const index = Math.max(0, Math.min(fixtureItemCount - 1, id - 1))
  const [label, , , ratio] = sampleAt(index)
  // 与网格同源取类型:两路不一致会让「网格显示 AUDIO、点进去却是 image」这种 harness 自造的
  // 假象被当成产品 bug 追。
  const kind = kindOf(index)
  const hoverPatch = hoverFixtureOverride(index)
  // 与网格同源:悬停卡组里被覆盖的条目,查看器/预览解析也必须返回同一 mediaType/缩略图态,
  // 否则「格上无图、点进去却有图」又是 harness 自造的假象。
  const overlay: Pick<HoverFixturePatch, 'mediaType' | 'fileFormat' | 'thumbStatus'> | null =
    hoverPatch
      ? {
          mediaType: hoverPatch.mediaType,
          fileFormat: hoverPatch.fileFormat,
          thumbStatus: hoverPatch.thumbStatus,
        }
      : null
  const mediaType = overlay ? overlay.mediaType : kind.mediaType
  const fileFormat = overlay?.fileFormat ?? kind.fileFormat
  const thumbStatus = overlay?.thumbStatus ?? 3
  return {
    id: index + 1,
    directoryId: 101,
    fileName: hoverPatch ? `${label}.${fileFormat}` : `${label}.${kind.fileFormat}`,
    fileSize: 1_200_000 + index * 83_000,
    fileMtime: 1_767_225_600 - index * 18_000,
    fileFormat,
    mediaType,
    width: Math.round(1800 * ratio),
    height: 1800,
    durationMs: hoverPatch?.durationMs ?? null,
    sortDatetime: 1_767_225_600 - index * 18_000,
    cacheKey: index + 1,
    thumbStatus,
    thumbPath: hoverPatch ? (hoverPatch.thumbPath ?? null) : sampleSvg(index),
    thumbhash: null,
    isFavorited: hoverPatch?.isFavorited ?? (index === 2 || index === 9 || index === 14),
    isDeleted: false,
    deletedAt: null,
    rating: hoverPatch?.rating ?? (index % 6 === 0 ? 4 : 0),
    colorLabel: index % 7 === 0 ? 4 : 0,
    viewRotation: 0,
    playbackPositionMs: 0,
    isLivePhoto: false,
    hasEmbeddedVideo: false,
    companionOf: null,
    contentHash: null,
    createdAt: 1_767_225_600,
    updatedAt: 1_767_225_600,
    absPath: mediaType === 'video' ? HOVER_VIDEO_ABS_PATH : sampleSvg(index),
    imageMeta: null,
    availability: hoverPatch?.availability ?? 'online',
    videoMeta: null,
  }
}

// nodeKey/parentKey 照抄后端 `crate::tree::node_key` 的真实产出格式(`{rootId}:{relPath}`,
// 扫描根 relPath 为空 → 键 '1:'、无父)。harness 是后端的替身,键必须与真后端同形——否则
// 用 harness 截的主题快照会与真机跑出不同的树身份,而这类分歧不会报错。
function directoryTree(): DirNode[] {
  return [
    {
      nodeKey: '1:',
      parentKey: null,
      id: 101,
      rootId: 1,
      parentId: null,
      name: '示例图库',
      relPath: '',
      depth: 0,
      mediaCount: samples.length,
      hasChildren: true,
    },
    {
      nodeKey: '1:旅行',
      parentKey: '1:',
      id: 102,
      rootId: 1,
      parentId: 101,
      name: '旅行',
      relPath: '旅行',
      depth: 1,
      mediaCount: 10,
      hasChildren: false,
    },
    {
      nodeKey: '1:日常',
      parentKey: '1:',
      id: 103,
      rootId: 1,
      parentId: 101,
      name: '日常',
      relPath: '日常',
      depth: 1,
      mediaCount: 8,
      hasChildren: false,
    },
  ]
}

function editPreviewPacket(): ArrayBuffer {
  const pngBase64 =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='
  const binary = atob(pngBase64)
  const body = Uint8Array.from(binary, (char) => char.charCodeAt(0))
  const packet = new Uint8Array(28 + body.length)
  packet.set([0x53, 0x45, 0x50, 0x32, 1, 2, 0, 0])
  const view = new DataView(packet.buffer)
  view.setUint32(8, 1600, true)
  view.setUint32(12, 1200, true)
  view.setUint32(16, 1, true)
  view.setUint32(20, 1, true)
  view.setUint32(24, body.length, true)
  packet.set(body, 28)
  return packet.buffer
}

// 旧组浏览/清理 IPC（list_duplicate_groups/members、apply_dedup_soft_delete）前端已无消费方
// （P4 退场），不再提供 fixture；后端命令暂保留，harness 未覆盖时会走 default 记录。
const dedupFixtureStatus: DedupStatusSnapshot = {
  runId: 'harness-dedup-20260830',
  status: 'completed',
  phase: 'unit',
  itemsDone: 18,
  itemsTotal: 18,
  bytesDone: 32_400_000,
  bytesTotal: 32_400_000,
  groupsFound: 2,
  potentialLogicalBytes: 6_600_000,
  errors: [],
  waitingOn: [],
}

/** 真实页面使用的 DEV-only IPC fixture；未覆盖命令会记录出来，便于补齐场景。 */
// 与真实 IPC 一样按批登记，定时结果可逐项取消。
const viewportRequests = new Map<string, Map<number, ReturnType<typeof setTimeout>>>()

export async function invokeHarness<T>(
  cmd: IpcCommand,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (cmd) {
    case IPC.GET_STARTUP_CONFIG: {
      // 新契约:设置快照 + 启动内部状态一次往返(不再逐字段列举设置)。
      const payload: StartupPayload = {
        settings: resolveSettingsSnapshot(),
        state: {
          firstLaunch: appState.first_launch ?? null,
          guideSeen: appState.guide_seen ?? null,
        },
      }
      return payload as T
    }
    case IPC.GET_SETTINGS_SNAPSHOT:
      return resolveSettingsSnapshot() as T
    case IPC.SET_APP_SETTINGS: {
      // patch 的键名与取值保持原样交给 fixture:harness 不代替后端做键校验(那是后端 schema 的活)。
      const payload = args as { patch?: Record<string, string>; generation?: number } | undefined
      // 代次契约(与后端同口径):提交必须带上当前代次,缺失或过期一律拒绝。
      // 少了这道校验,harness 会掩盖「重置后旧窗口的迟到回写被拒」这条核心行为——
      // 开发页看着一切正常,真机上旧代次却会被后端拒绝。
      const generation = payload?.generation
      if (typeof generation !== 'number' || generation !== settingsState.generation) {
        throw new Error(
          `[UI harness] set_app_settings 代次不匹配:提交 ${String(generation)}(当前 ${settingsState.generation}),已按后端语义拒绝`,
        )
      }
      const patch = payload?.patch ?? {}
      const change: SettingsChange = applySettingsPatch(patch)
      return change as T
    }
    case IPC.CLEAR_SETTINGS: {
      const change: SettingsChange = resetSettingsFixture()
      return change as T
    }
    case IPC.GET_CONFIG_STATUS:
      return { path: 'C:/Scrollery UI Harness/config.toml', exists: true, last_error: null } as T
    case IPC.LIST_SCAN_ROOTS:
      return [scanRoot] as T
    case IPC.SET_SCAN_ROOT_HIDDEN:
      // harness 无真实持久层，显隐切换记到 fixture 上即可（store 本地也会同步）。
      scanRoot.isHidden = Boolean(args?.hidden)
      return undefined as T
    case IPC.GET_STATS:
      return stats as T
    case IPC.DEDUP_STATUS:
      return dedupFixtureStatus as T
    case IPC.START_DEDUP_ANALYSIS:
      return dedupFixtureStatus as T
    case IPC.STOP_DEDUP_ANALYSIS:
      return { ...dedupFixtureStatus, status: 'cancelled' } as T
    case IPC.GET_DIRECTORY_TREE:
      return directoryTree() as T
    case IPC.GET_DIRECTORY_CHILDREN:
    case IPC.LIST_DIRECTORY_FILES:
      return [] as T
    // 「所有文件」两态(S 线 §4,R-19):此前无 fixture → dev harness 里切 allFiles 在
    // `page.entries` 上抛 TypeError。给一页有代表性的条目:FS-only 目录(无 directoryId)、
    // 未注册文件、隐藏项(淡化样式可视)、已入库文件(带实体身份)。relPath 按被列目录拼,
    // 键与真后端同形(见 directoryTree 头注)。
    case IPC.LIST_TREE_ENTRIES: {
      const a = args as { rootId?: number; relPath?: string; kind?: string; mode?: string }
      const rootId = a.rootId ?? 1
      const rel = a.relPath ?? ''
      const child = (name: string) => (rel === '' ? name : `${rel}/${name}`)
      const mk = (name: string, kind: 'dir' | 'file', over: Record<string, unknown> = {}) => ({
        nodeKey: `${rootId}:${child(name)}`,
        parentKey: `${rootId}:${rel}`,
        rootId,
        relPath: child(name),
        name,
        kind,
        hidden: false,
        registered: false,
        isSymlink: false,
        ...over,
      })
      const dirs = [
        mk('FS-only 目录', 'dir'),
        ...(a.mode === 'allFilesWithHidden' ? [mk('.隐藏目录', 'dir', { hidden: true })] : []),
      ]
      const files = [
        mk('未注册.xyz', 'file'),
        mk('已入库.png', 'file', { registered: true, mediaId: 1, mediaType: 'image' }),
      ]
      const entries = a.kind === 'dirs' ? dirs : a.kind === 'files' ? files : [...dirs, ...files]
      return { entries, total: entries.length } as T
    }
    case IPC.INVALIDATE_TREE_CACHE:
    case IPC.REVEAL_TREE_ENTRY:
      return undefined as T
    // 树内纯文本预览(问题②方案 B v1):给足撑起弹层的样例——多行 + markdown 语法原样
    // (预览**不渲染** markdown,fixture 里的 `#`/`**` 应原样可见),truncated 恒 false;
    // 截断提示条形态由组件测试/真机看,不靠 fixture 分叉。
    case IPC.GET_TREE_TEXT_PREVIEW:
      return {
        content: '# 预览样例\n\n这是 **纯文本** 预览——markdown 语法应原样显示。\n\n第三行。',
        truncated: false,
      } as T
    // 格式弹层（S 线 P3）：harness 是**视觉场景**，给一份能撑起弹层各形态的最小 registry ——
    // 别名组（JPEG={jpg,jpeg}）、单扩展名（PNG）、exotic（PSD，带 badge）、跨大类（MP4/TXT）。
    case IPC.LIST_REGISTERED_FORMATS:
      return [
        { ext: 'jpg', mediaType: 'image', group: 'JPEG', source: { kind: 'builtin' } },
        { ext: 'jpeg', mediaType: 'image', group: 'JPEG', source: { kind: 'builtin' } },
        { ext: 'png', mediaType: 'image', group: null, source: { kind: 'builtin' } },
        {
          ext: 'psd',
          mediaType: 'image',
          group: null,
          source: { kind: 'exotic', pluginId: 'exotic-image-psd' },
        },
        { ext: 'mp4', mediaType: 'video', group: null, source: { kind: 'builtin' } },
        { ext: 'txt', mediaType: 'document', group: null, source: { kind: 'builtin' } },
      ] as T
    // facet 与上面的 registry 取交集后即为弹层实际呈现集；有意不含 psd 之外的 exotic。
    case IPC.LIST_LIBRARY_FORMATS:
      return ['jpg', 'jpeg', 'mp4', 'png', 'psd', 'txt'] as T
    case IPC.GET_THUMB_CACHE_DIR:
      return '' as T
    case IPC.COMPUTE_LAYOUT: {
      const params = (args?.params ?? {}) as Record<string, unknown>
      return computeFixtureLayout(
        Number(params.containerWidth ?? 1200),
        Number(params.rowHeight ?? 220),
        Number(params.gap ?? 4),
      ) as T
    }
    case IPC.GET_LAYOUT_ROWS_BY_Y: {
      const top = Number(args?.topY ?? 0)
      const bottom = Number(args?.bottomY ?? Number.MAX_SAFE_INTEGER)
      return layoutRows.filter((row) => row.y < bottom && row.y + row.height > top) as T
    }
    case IPC.GET_BUCKET_ROWS: {
      const start = Number(args?.startY ?? 0)
      const end = Number(args?.endY ?? Number.MAX_SAFE_INTEGER)
      return layoutRows.filter((row) => row.y >= start && row.y < end) as T
    }
    case IPC.GET_VIEW_IDS:
      return Array.from({ length: fixtureItemCount }, (_, index) => index + 1) as T
    case IPC.GET_META_FOR_VIEWPORT: {
      const ids = Array.isArray(args?.ids) ? (args.ids as number[]) : []
      return ids.map<MediaMeta>((id) => ({
        id,
        // 与 createItem 同源取类型与文件名:否则悬停卡信息行的「文件名/类型」来自另一份 fixture,
        // 长文件名碰撞验证会失准(默认关闭 &hoverCards=1 时与原式逐字节一致)。
        fileName:
          hoverFixtureFileName(id - 1) ??
          `${sampleAt(id - 1)[0]}.${hoverFixtureOverride(id - 1)?.fileFormat ?? kindOf(id - 1).fileFormat}`,
        dirPath: 'C:/Scrollery 示例图库',
        gpsLat: null,
        gpsLng: null,
        exifMake: 'Scrollery',
        exifModel: 'UI Harness',
        exifLens: null,
        exifFocalLength: null,
        exifAperture: null,
        exifShutter: null,
        exifIso: null,
      })) as T
    }
    case IPC.GET_KEYFRAME_SPRITE:
      // 固定视频样本的关键帧由浏览器验收拦截提供；默认夹具仍无 sprite。
      return (
        uiHarnessHoverCards && args?.itemId === 4
          ? `${window.location.origin}/harness/documents/hover-card-sprite.svg`
          : null
      ) as T
    case IPC.GET_MEDIA_DETAIL:
      if (markdownFixtures.has(Number(args?.id))) {
        const id = Number(args?.id)
        const fileFormat = id === 900005 ? 'txt' : 'md'
        return {
          ...mediaDetail(1),
          id,
          fileName: `阅读编辑样例-${id}.${fileFormat}`,
          mediaType: 'document',
          fileFormat,
          absPath: `${window.location.origin}/harness/documents/${id}.${fileFormat}`,
          fileSize: new TextEncoder().encode(markdownFixture(id)).length,
          thumbPath: null,
          width: null,
          height: null,
        } as T
      }
      return mediaDetail(Number(args?.id ?? 1)) as T
    case IPC.GET_DOCUMENT_TEXT:
      return effectiveDocumentText(args?.itemId) as T
    case IPC.GET_TEXT_BOOK_INDEX:
      return {
        encoding: 'UTF-8',
        confidence: 'detected',
        chapters: [{ title: '', charLen: effectiveDocumentText(args?.itemId).length }],
      } as T
    case IPC.GET_TEXT_CHAPTER:
      return {
        title: '',
        paragraphs: effectiveDocumentText(args?.itemId).trim().split(/\n\n/),
      } as T
    case IPC.SAVE_VERSION: {
      markdownFixture(args?.itemId)
      if (typeof args?.content !== 'string') {
        throw new Error('[UI harness] 文档保存缺少文本')
      }
      const versionId = markdownVersionId++
      const itemId = Number(args.itemId)
      if (args.target === 'overwrite') {
        markdownFixtures.set(itemId, args.content)
        markdownCurrentVersions.delete(itemId)
      } else {
        markdownVersions.set(versionId, { itemId, content: args.content })
      }
      return versionId as T
    }
    case IPC.SET_CURRENT_VERSION: {
      const itemId = Number(args?.itemId)
      const versionId = Number(args?.versionId)
      if (markdownVersions.get(versionId)?.itemId !== itemId) {
        throw new Error('[UI harness] 版本不属于当前文档')
      }
      markdownCurrentVersions.set(itemId, versionId)
      return undefined as T
    }
    case IPC.GET_CURRENT_VERSION:
      return (
        markdownCurrentVersions.has(Number(args?.itemId))
          ? { id: markdownCurrentVersions.get(Number(args?.itemId)) }
          : null
      ) as T
    case IPC.GET_READER_BOOK_PREFS:
      return (readerBookPrefs.get(Number(args?.itemId)) ?? null) as T
    case IPC.SET_READER_BOOK_PREFS:
      markdownFixture(args?.itemId)
      if (typeof args?.prefs !== 'string') throw new Error('[UI harness] 阅读偏好缺少JSON')
      readerBookPrefs.set(Number(args.itemId), args.prefs)
      return undefined as T
    case IPC.GET_READING_PROGRESS:
      return null as T
    case IPC.GET_EFFECTIVE_REPLACEMENTS:
    case IPC.LIST_READER_BOOKMARKS:
      return [] as T
    case IPC.SET_READING_PROGRESS:
      return undefined as T
    case IPC.LIST_FEATURE_OFFERINGS:
      return {
        features: [
          {
            id: 'feature-editing',
            name: 'Image editing',
            activationRequired: true,
            activated: false,
            builtin: true,
            availability: 'inactive',
            resources: 'ready',
          },
          {
            id: 'exotic-ocr',
            name: 'OCR',
            activationRequired: true,
            activated: false,
            builtin: true,
            availability: 'availableUninstalled',
            resources: 'modelMissing',
          },
          {
            id: 'exotic-enhance',
            name: 'Enhancement',
            activationRequired: true,
            activated: false,
            builtin: true,
            availability: 'availableUninstalled',
            resources: 'manifestUnready',
          },
          {
            id: 'exotic-image-psd',
            name: 'PSD',
            activationRequired: true,
            activated: false,
            builtin: false,
            availability: 'availableUninstalled',
            resources: 'needsInstall',
          },
          {
            id: 'exotic-image-raw',
            name: 'RAW',
            activationRequired: false,
            activated: false,
            builtin: true,
            availability: 'ready',
            resources: 'included',
          },
          {
            id: 'video-extended',
            name: 'Video formats',
            activationRequired: false,
            activated: false,
            builtin: true,
            availability: 'ready',
            resources: 'included',
          },
        ].map((feature) => ({
          ...feature,
          activated: activatedHarnessFeatures.has(feature.id),
          availability:
            feature.activationRequired && feature.builtin
              ? activatedHarnessFeatures.has(feature.id)
                ? 'ready'
                : 'inactive'
              : feature.availability,
        })),
      } as T
    case IPC.FETCH_EXOTIC_REGISTRY:
      return { pluginCount: 0 } as T
    case IPC.LIST_EXOTIC_REGISTRY:
    case IPC.LIST_INSTALLED_EXOTIC_PLUGINS:
      return [] as T
    case IPC.GET_EXOTIC_PROCESSING_STATUS:
      return {
        running: false,
        pending: 0,
        processing: 0,
        done: 0,
        error: 0,
        blockedByAvailability: 0,
      } as T
    case IPC.ACTIVATE_FEATURE:
    case IPC.GET_FEATURE_ACTIVATION: {
      const featureId = String(args?.featureId ?? '')
      if (!freeActivationIds.includes(featureId))
        throw { code: 'unknown_feature', message: 'Unknown feature' }
      if (cmd === IPC.ACTIVATE_FEATURE) activatedHarnessFeatures.add(featureId)
      return { featureId, activated: activatedHarnessFeatures.has(featureId) } as T
    }
    case IPC.GET_EDIT_PREVIEW:
      return editPreviewPacket() as T
    case IPC.SAVE_EDITED_IMAGE:
      return { status: 'saved', newItemId: Number(args?.itemId ?? 1) } as T
    case IPC.GET_ADJACENT_MEDIA: {
      const nextId = Number(args?.currentId ?? 1) + Number(args?.offset ?? 0)
      return mediaDetail(((nextId - 1 + fixtureItemCount) % fixtureItemCount) + 1) as T
    }
    case IPC.GET_LENS_ADJACENT_MEDIA: {
      const currentId = Number(args?.currentId ?? 1)
      const offset = Number(args?.offset ?? 0)
      const targetIndex = currentId - 1 + offset
      if (targetIndex < 0 || targetIndex >= fixtureItemCount) return null as T
      return {
        detail: mediaDetail(targetIndex + 1),
        index: targetIndex,
        totalCount: fixtureItemCount,
      } as T
    }
    case IPC.GET_AI_STATUS:
      return {
        provider: '',
        gpuName: '',
        vramGb: 0,
        batchSize: 0,
        activeFixedBatch: null,
        clipLoaded: false,
        totalItems: fixtureItemCount,
        analyzedItems: 0,
        pendingItems: 0,
        errorItems: 0,
        isAnalyzing: false,
        analysisActive: false,
        waitingOn: [],
      } as T
    case IPC.LIST_FACE_PERSONS:
      return personFixtures.map((person) => ({ ...person })) as T
    case IPC.LIST_IGNORED_FACE_PERSONS:
      return ignoredPersonFixtures.map((person) => ({ ...person })) as T
    case IPC.LIST_LIKELY_FACE_MATCHES:
      return [] as T
    case IPC.RENAME_FACE_PERSON: {
      const person = [...personFixtures, ...ignoredPersonFixtures].find(
        (p) => p.id === args?.personId,
      )
      if (!person) throw new Error('harness person not found')
      const name = String(args?.name ?? '').trim()
      person.name = name || null
      person.isNamed = name.length > 0
      return undefined as T
    }
    case IPC.GET_FACE_STATUS:
      return {
        provider: '',
        gpuName: '',
        faceLoaded: false,
        totalItems: fixtureItemCount,
        processedItems: 0,
        pendingItems: 0,
        personCount: personFixtures.length,
        faceCount: personFixtures.reduce((count, person) => count + person.faceCount, 0),
        errorItems: 0,
        isAnalyzing: false,
        analysisActive: false,
        waitingOn: [],
      } as T
    case IPC.DERIVATION_STATUS:
      return { pending: 0, processing: 0, done: 0, error: 0, isRunning: true, active: false } as T
    case IPC.FULL_THUMB_GEN_STATUS:
      return { generated: 0, total: 0, status: 'idle' } as T
    case IPC.BACKUP_STATUS:
      return { status: 'idle' } as T
    case IPC.PREFLIGHT_BACKUP:
      return {
        destDir: String(args?.dest ?? ''),
        writable: true,
        sameVolumeWarning: false,
        documentsConsistent: true,
        estimatedBytes: 28 * 1024 * 1024,
      } as T
    case IPC.LIST_BACKUPS:
      return [
        {
          path: 'D:/Scrollery Backups/scrollery-auto-20260719-090000.scrollerybackup',
          fileName: 'scrollery-auto-20260719-090000.scrollerybackup',
          kind: 'auto',
          createdAtUtc: '2026-07-19T09:00:00Z',
          backupId: '00112233445566778899aabbccddeeff',
          bytes: 19 * 1024 * 1024,
        },
        {
          path: 'D:/Scrollery Backups/scrollery-backup-20260718-210000.scrollerybackup',
          fileName: 'scrollery-backup-20260718-210000.scrollerybackup',
          kind: 'backup',
          createdAtUtc: '2026-07-18T21:00:00Z',
          backupId: 'ffeeddccbbaa99887766554433221100',
          bytes: 18 * 1024 * 1024,
        },
      ] as T
    case IPC.RESTORE_STAGE:
      return {
        backupId: '00112233445566778899aabbccddeeff',
        stagingToken: 'harness-staging-generation',
        stagingDir: 'C:/Scrollery UI Harness/restore-staging/00112233445566778899aabbccddeeff',
        schemaVersion: 34,
        kind: 'auto',
        createdAtUtc: '2026-07-19T09:00:00Z',
        counts: { items: 18, albums: 3, tags: 12, namedPersons: 4 },
        roots: [{ id: 1, alias: '示例图库', hidden: false }],
        externalDocumentVersions: 1,
        appdataDocumentCount: 6,
      } as T
    // 目录移动恢复清单：后端契约是 Result<Vec<MoveRecoveryReport>>，未覆盖命令时不能再落回
    // default 的兜底 null——那会把「模拟器缺实现」变成生产端必须承担的兼容分支。显式给空数组。
    case IPC.LIST_PENDING_DIRECTORY_MOVES:
    case IPC.LIST_PENDING_DOC_THUMBS:
    case IPC.LIST_ICC_PROFILES:
    case IPC.LIST_EXOTIC_FORMAT_RESOLUTIONS:
    case IPC.LIST_COLLECTIONS:
    case IPC.LIST_BACKENDS:
    case IPC.LIST_VOLUMES:
    case IPC.LIST_FACE_MODEL_REGISTRY:
    case IPC.GET_ITEM_FACES:
      return [] as T
    case IPC.LIST_MODEL_REGISTRY:
      return {
        archs: [],
        activeArchId: '',
        activeImageFile: '',
        online: false,
      } as T
    // 内部状态逐键读:设置类键在此一律拒绝(用户设置的唯一入口是快照与批量提交)。
    case IPC.GET_APP_CONFIG:
      return (appState[requireAppStateKey(args?.key)] ?? null) as T
    case IPC.SET_APP_CONFIG:
      appState[requireAppStateKey(args?.key)] = String(args?.value ?? '')
      return undefined as T
    case IPC.GET_LOG_DIR:
      return 'C:/Scrollery UI Harness/logs' as T
    case IPC.LIST_LOG_FILES:
      return [] as T
    case IPC.READ_LOG_FILE_PAGE:
      return { lines: [], totalLines: 0, hasMoreOlder: false } as T
    case IPC.GET_LOG_DIAGNOSTICS:
      return { droppedLines: 0, dedupActive: [] } as T
    case IPC.COMPUTE_LOG_HISTOGRAM:
      return { buckets: [], totalLines: 0, parsedLines: 0 } as T
    case IPC.EXPORT_DIAGNOSTICS_PACKAGE:
      return {
        dir: 'C:/Scrollery UI Harness/logs/diagnostics',
        zipPath: 'C:/Scrollery UI Harness/logs/diagnostics/scrollery-diagnostics-harness.zip',
        sizeBytes: 0,
        redactedMatches: 0,
      } as T
    case IPC.FRONTEND_HEARTBEAT:
    case IPC.LOG_FRONTEND_EVENTS:
    case IPC.ENSURE_DOC_THUMB_QUEUE:
    case IPC.REGENERATE_MISSING_THUMB:
    case IPC.START_BACKUP:
    case IPC.STOP_BACKUP:
    case IPC.RESTORE_ARM:
    case IPC.RESTORE_DISCARD:
    case IPC.RELAUNCH_APP:
    case IPC.OPEN_LOG_WINDOW:
      return undefined as T
    case IPC.CANCEL_VIEWPORT_THUMBNAIL_REQUEST: {
      const a = args as { requestId: string; itemIds: number[] }
      const pending = viewportRequests.get(a.requestId)
      for (const id of a.itemIds) {
        clearTimeout(pending?.get(id))
        pending?.delete(id)
      }
      if (pending?.size === 0) viewportRequests.delete(a.requestId)
      return undefined as T
    }
    case IPC.BATCH_REQUEST_THUMBNAILS: {
      const a = args as {
        requestId: string
        itemIds: number[]
        onResult: { onmessage: (r: ThumbResult & { outcome: 'complete' }) => void }
      }
      const pending = new Map<number, ReturnType<typeof setTimeout>>()
      if (a.itemIds.length) viewportRequests.set(a.requestId, pending)
      a.itemIds.forEach((id, i) => {
        pending.set(
          id,
          setTimeout(
            () => {
              pending.delete(id)
              if (pending.size === 0) viewportRequests.delete(a.requestId)
              a.onResult.onmessage({
                itemId: id,
                thumbStatus: 3,
                thumbPath: '/favicon.png',
                thumbhash: null,
                outcome: 'complete',
              })
            },
            30 * (i + 1),
          ),
        )
      })
      return undefined as T
    }
    default:
      console.warn(`[UI harness] 尚未覆盖 IPC: ${cmd}`, args)
      return null as T
  }
}
