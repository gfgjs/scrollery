import { IPC } from '../constants/ipc'
import { getTheme } from '../themes/registry'
import {
  uiHarnessAvail,
  uiHarnessAvailRatio,
  uiHarnessBucket,
  uiHarnessItems,
  uiHarnessRowHeight,
  uiHarnessTheme,
  uiHarnessText,
  uiHarnessTint,
  uiHarnessThumbStatus,
} from './runtime'
import type { IpcCommand } from '../utils/ipc'
import type { LayoutRow, LayoutRowItem, LayoutSummary, MediaMeta } from '../types/layout'
import type { AppStats, DirNode, MediaDetail, MediaType, ScanRoot, ThumbResult } from '../types/media'
import type { DedupStatusSnapshot } from '../types/ipc'
import type { StartupConfig } from '../stores/uiStore'

const SCENE_VERSION = 1

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
  return {
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
}

let layoutRows: LayoutRow[] = []
let layoutSummary: LayoutSummary = {
  totalRows: 0,
  totalHeight: 0,
  layoutVersion: SCENE_VERSION,
  totalItems: fixtureItemCount,
  separators: [],
  monthBuckets: [],
}

function computeFixtureLayout(containerWidth: number, targetHeight: number, gap: number): LayoutSummary {
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
    totalItems: fixtureItemCount,
    separators,
    monthBuckets: [
      { year: 2026, month: 1, count: fixtureItemCount, y: 0, groupId: '2026-01' },
    ],
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
const fixtureKinds = Array.from({ length: fixtureItemCount }, (_, index) => kindOf(index).mediaType)
const audioCount = fixtureKinds.filter((kind) => kind === 'audio').length
const documentCount = fixtureKinds.filter((kind) => kind === 'document').length
const nonImageCount = audioCount + documentCount

const stats: AppStats = {
  totalItems: fixtureItemCount,
  totalImages: fixtureItemCount - nonImageCount,
  totalVideos: 0,
  totalAudios: audioCount,
  totalDocuments: documentCount,
  totalFavorited: Array.from({ length: fixtureItemCount }, (_, index) =>
    [2, 9, 14].includes(baseSampleIndex(index)),
  ).filter(Boolean).length,
  totalDeleted: 0,
  totalLivePhotos: Array.from({ length: fixtureItemCount }, (_, index) =>
    [5, 13].includes(baseSampleIndex(index)),
  ).filter(Boolean).length,
}

const startupConfig: StartupConfig = {
  language: 'zh-CN',
  timelineScrollWidth: null,
  timelineAxisWidth: '44',
  scrollThumbMinHeight: '28',
  uiFontSize: '15',
  enableThumbHoverScale: 'true',
  // 行高:显式 &rowHeight= 优先;否则视觉场景 220 / perf 场景 60(既有行为)。
  gridRowHeight:
    uiHarnessRowHeight !== null ? String(uiHarnessRowHeight) : uiHarnessItems === null ? '220' : '60',
  groupBy: 'date',
  sortWithinGroup: 'datetime',
  layoutMode: 'justified',
  closeBehavior: 'ask',
  pinnedSettings: '[]',
  guideSeen: 'true',
  // harness 是**视觉场景**不是「新装默认」模拟器(整套 fixture 本就是 18 张假图):这里刻意把
  // 缩略图信息开满,好让主题矩阵覆盖到徽章类 token(size 半透明黑底 / type 绿橙底 —— 正是 S7
  // 修的那批)。生产默认另有其值(showThumbInfo 默认开、elements 默认空 → 观感无徽章)。
  showThumbInfo: 'true',
  thumbInfoElements: '["type","size","status"]',
  hoverAutoplay: 'false',
  // &bucket=1 时切生产默认的 bucket 分段引擎(性能基准代表性);缺省保持方案 A 视觉基线。
  bucketSegmentedScroll: uiHarnessBucket === null ? 'false' : String(uiHarnessBucket),
  theme: null,
  appearance: 'light',
  themeLight: 'fresh-light',
  themeDark: 'fresh-dark',
  firstLaunch: 'false',
  // 主题矩阵拍的是 DB 目录树的观感,与显示范围无关;留默认态,勿让 harness 走 FS 枚举路径
  // (那条路径在 harness 里没有真实磁盘可枚举)。
  treeDisplayMode: 'registeredOnly',
  // 拖拽手柄(#5):留生产默认开,选中态截图覆盖手柄观感。
  showDragHandle: 'true',
  // 无缝分组(#1):留生产默认关,视觉基线保分隔符观感。
  seamlessGroups: 'false',
  // 无缝 minimap 轴:留生产默认开(无缝关时不渲染,基线截图不受影响)。
  seamlessMinimap: 'true',
  // 渲染模式留生产默认缩略图;当前 fixture 不启用 minimap,不改变视觉基线。
  minimapRenderMode: 'thumbnails',
  // harness 不跑真日志管线,留生产默认 info 即可(off 会让 logger.ts 在 harness 里也不入队,
  // 无实际影响,但 'info' 更贴合「模拟正常运行态」的 harness 定位)。
  logLevel: 'info',
  // 批次C:5 个前端阈值键(advanced)——harness 是静态视觉场景,留 null 回退生产默认值即可
  // (hydrateFromStartupConfig 对 null 不覆盖 uiStore ref 的初始默认)。
  heavyVideoMaxPixels: null,
  heavyVideoMaxBytes: null,
  hoverDelayMs: null,
  searchDebounceMs: null,
  resizeDebounceMs: null,
  // 小批 C2:同上——留 null 回退生产默认值(uiStore videoKeyframeCount ref 初始值 10)。
  videoKeyframeCount: null,
  // 窗口化沉浸模式(2026-07-23):留 null 回退生产默认关闭(harness 是静态视觉场景,不需要
  // 边缘唤出交互)。
  autoHideChromeWindowed: null,
  // 轴视窗不透明度缩放(2026-07-24):留 null 回退生产默认 100%(CSS 变量缺省 1,基线观感不变)。
  axisViewportOpacity: null,
  // 轴形态偏好(2026-07-24):留 null 回退生产默认 'timeline',不改变视觉基线。
  axisMode: null,
  // 窗口材质(毛玻璃,2026-08-24):harness 非 Windows 桌面场景,留 null 不启用玻璃层。
  // 浏览器没有 DWM 背板，使用实色材质才能真实核对主题底色与文字对比度。
  windowMaterial: 'none',
  // 毛玻璃分层缩放(2026-08-25):视觉 harness 保持生产默认 100,由 uiStore 初始值回退。
  glassChromeOpacity: null,
  glassStickyOpacity: null,
  glassSurfaceOpacity: null,
  glassControlOpacity: null,
  // 内容底面缩放(2026-09-06):同上,留 null 回退生产默认 100。
  glassContentOpacity: null,
  glassGalleryOpacity: null,
  // 主题色浓度(2026-09-06):留 null 回退生产默认 60;主题矩阵截图可用 &tint= 覆盖。
  themeTintStrength: null,
  // 文字浓度(2026-09-06):留 null 回退生产默认 75;主题矩阵截图可用 &text= 覆盖。
  themeTextStrength: null,
}

/**
 * 6 主题视觉矩阵截图(S7)用:`&theme=<id>` 覆盖 fixture 的外观三键。
 *
 * 有意经 startupConfig 而**不是**直接写 `data-theme` —— 后者会让截图为一条产品里不存在的
 * 路径背书。经此处则完整跑真实链:normalizeThemeId 归一化 → resolvedThemeId(外观模式→槽位)
 * → applyAppearance 单点写 documentElement。故非法 id 的行为亦与生产一致(落回槽位默认)。
 *
 * 亮槽恒 light kind、暗槽恒 dark kind 是 uiStore 的模型不变量,故此处按 kind 落槽并把
 * appearance 定为同一 kind,才能让 resolvedThemeId 取到目标主题。
 */
function resolveStartupConfig(): StartupConfig {
  const theme = uiHarnessTheme ? getTheme(uiHarnessTheme) : undefined
  // &tint=<pct>:20–100 合法值覆写主题色浓度(经真实 uiStore 水合链应用 CSS 变量),其余落默认。
  const tintRaw = Number(uiHarnessTint)
  const tint = Number.isFinite(tintRaw) && tintRaw >= 20 && tintRaw <= 100
    ? String(Math.round(tintRaw))
    : null
  // &text=<pct>:40–100 合法值覆写文字浓度,其余落默认。
  const textRaw = Number(uiHarnessText)
  const text = Number.isFinite(textRaw) && textRaw >= 40 && textRaw <= 100
    ? String(Math.round(textRaw))
    : null
  if (!theme && tint === null && text === null) return startupConfig
  return {
    ...startupConfig,
    appearance: theme ? theme.kind : startupConfig.appearance,
    themeLight: theme ? (theme.kind === 'light' ? theme.id : startupConfig.themeLight) : startupConfig.themeLight,
    themeDark: theme ? (theme.kind === 'dark' ? theme.id : startupConfig.themeDark) : startupConfig.themeDark,
    themeTintStrength: tint,
    themeTextStrength: text,
  }
}

function mediaDetail(id: number): MediaDetail {
  const index = Math.max(0, Math.min(fixtureItemCount - 1, id - 1))
  const [label, , , ratio] = sampleAt(index)
  // 与网格同源取类型:两路不一致会让「网格显示 AUDIO、点进去却是 image」这种 harness 自造的
  // 假象被当成产品 bug 追。
  const kind = kindOf(index)
  return {
    id: index + 1,
    directoryId: 101,
    fileName: `${label}.${kind.fileFormat}`,
    fileSize: 1_200_000 + index * 83_000,
    fileMtime: 1_767_225_600 - index * 18_000,
    fileFormat: kind.fileFormat,
    mediaType: kind.mediaType,
    width: Math.round(1800 * ratio),
    height: 1800,
    durationMs: null,
    sortDatetime: 1_767_225_600 - index * 18_000,
    cacheKey: index + 1,
    thumbStatus: 3,
    thumbPath: sampleSvg(index),
    thumbhash: null,
    isFavorited: index === 2 || index === 9 || index === 14,
    isDeleted: false,
    deletedAt: null,
    rating: index % 6 === 0 ? 4 : 0,
    colorLabel: index % 7 === 0 ? 4 : 0,
    viewRotation: 0,
    playbackPositionMs: 0,
    isLivePhoto: false,
    hasEmbeddedVideo: false,
    companionOf: null,
    contentHash: null,
    createdAt: 1_767_225_600,
    updatedAt: 1_767_225_600,
    absPath: sampleSvg(index),
    imageMeta: null,
    availability: 'online',
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
export async function invokeHarness<T>(
  cmd: IpcCommand,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (cmd) {
    case IPC.GET_STARTUP_CONFIG:
      return resolveStartupConfig() as T
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
        fileName: `${sampleAt(id - 1)[0]}.${kindOf(id - 1).fileFormat}`,
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
    case IPC.GET_MEDIA_DETAIL:
      return mediaDetail(Number(args?.id ?? 1)) as T
    case IPC.GET_EDITING_ENTITLEMENT:
      return {
        pluginId: 'feature-editing',
        availability: 'authorized',
        sourceTag: 'harness',
        sku: 'editing-tools-2026',
        storeUrl: null,
      } as T
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
    case IPC.GET_FACE_STATUS:
      return {
        provider: '',
        gpuName: '',
        faceLoaded: false,
        totalItems: fixtureItemCount,
        processedItems: 0,
        pendingItems: 0,
        personCount: 0,
        faceCount: 0,
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
        stagingDir:
          'C:/Scrollery UI Harness/restore-staging/00112233445566778899aabbccddeeff',
        schemaVersion: 21,
        needsMigration: false,
        kind: 'auto',
        createdAtUtc: '2026-07-19T09:00:00Z',
        counts: { items: 18, albums: 3, tags: 12, namedPersons: 4 },
        roots: [{ id: 1, alias: '示例图库', hidden: false }],
        externalDocumentVersions: 1,
        appdataDocumentCount: 6,
      } as T
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
    case IPC.GET_APP_CONFIG:
      switch (String(args?.key ?? '')) {
        case 'backup_dir':
          return 'D:/Scrollery Backups' as T
        case 'backup_auto_enabled':
          return 'true' as T
        case 'backup_retention':
          return '5' as T
        case 'backup_last_success_at':
          return '1784422800' as T
        default:
          return null as T
      }
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
    case IPC.SET_APP_CONFIG:
    case IPC.ENSURE_DOC_THUMB_QUEUE:
    case IPC.REGENERATE_MISSING_THUMB:
    case IPC.START_BACKUP:
    case IPC.STOP_BACKUP:
    case IPC.RESTORE_ARM:
    case IPC.RELAUNCH_APP:
    case IPC.ACTIVATE_EDITING_FEATURE:
    case IPC.DEACTIVATE_EDITING_FEATURE:
    case IPC.OPEN_LOG_WINDOW:
      return undefined as T
    case IPC.BATCH_REQUEST_THUMBNAILS: {
      // 「未生成冷库首览」基准场景的模拟生成器(&thumbStatus=0):逐项延迟经 Channel 送达
      // 结果(状态 3 + favicon 走原图管线,与极密性能场景同源),全部送达后再 resolve invoke——
      // 若先 resolve,useRequestQueue 的 finally 兜底会按「批内缺结果」拒绝并触发有界重试,
      // 造成同批重复生成。harness 无真实 worker,这里以 30ms/项 的节流近似生成吞吐。
      const a = args as
        | { itemIds?: number[]; onResult?: { onmessage: (r: ThumbResult) => void } }
        | undefined
      const ids = Array.isArray(a?.itemIds) ? (a!.itemIds as number[]) : []
      const ch = a?.onResult
      if (!ch || ids.length === 0) return undefined as T
      ids.forEach((id, i) => {
        setTimeout(
          () => ch.onmessage({ itemId: id, thumbStatus: 3, thumbPath: '/favicon.png', thumbhash: null }),
          30 * (i + 1),
        )
      })
      return new Promise<void>((resolve) => setTimeout(resolve, 30 * ids.length + 50)) as T
    }
    default:
      console.warn(`[UI harness] 尚未覆盖 IPC: ${cmd}`, args)
      return null as T
  }
}
