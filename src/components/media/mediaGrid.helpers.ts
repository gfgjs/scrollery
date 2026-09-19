// src/components/media/mediaGrid.helpers.ts
// MediaGrid 纯逻辑辅助(抽出便于单测,不碰组件状态)。

import type { MediaMeta } from '../../types/layout'
import type { DemoInfoText } from '../../utils/demoAlias'

// ── 查看器返回时的侧栏几何守卫 ────────────────────────────────────────────────

/**
 * 路由返回期间锁住画廊 wrapper 的实际主轴宽度。
 *
 * 单写 width 会被父级 flex 自动收缩覆盖；同时钉住 flex-basis、min/max-width，才不会把
 * 侧栏 `margin-left` 动画的中间几何传给画廊布局。
 */
export function buildViewportLockStyle(
  width: number,
  height = 0,
): Record<string, string> | undefined {
  if (!(width > 0) || !Number.isFinite(width)) return undefined
  const cssWidth = `${width}px`
  const style: Record<string, string> = {
    width: cssWidth,
    minWidth: cssWidth,
    maxWidth: cssWidth,
    flex: `0 0 ${cssWidth}`,
  }
  if (height > 0 && Number.isFinite(height)) {
    const cssHeight = `${height}px`
    style.height = cssHeight
    style.minHeight = cssHeight
    style.maxHeight = cssHeight
  }
  return style
}

/** 路由返回优先取当前可测宽度；KeepAlive 暂时摘离时退回最后一次稳定宽度。 */
export function pickRouteReturnViewportLockWidth(
  currentWrapperWidth: number,
  stableWidth: number,
): number | null {
  if (currentWrapperWidth > 0 && Number.isFinite(currentWrapperWidth)) return currentWrapperWidth
  if (stableWidth > 0 && Number.isFinite(stableWidth)) return stableWidth
  return null
}

/** 仅忽略浏览器布局噪声（≤ 1px）；最终宽度有效且实质变化才需要重新排版。 */
export function hasMaterialWidthChange(previousWidth: number, nextWidth: number): boolean {
  return nextWidth > 0 && Number.isFinite(nextWidth) && Math.abs(nextWidth - previousWidth) > 1
}

// ── 缩略图信息浮窗(DOM MediaThumb 与 Canvas 网格共享单源,防两份组装逻辑漂移)──────

/** buildThumbInfoLines 需要的布局行项字段子集(轻量常驻字段,重型字段走 meta)。 */
export interface ThumbInfoItemFields {
  sortDatetime: number
  originalWidth: number
  originalHeight: number
}

/**
 * 组装缩略图信息浮窗的文本行(按用户勾选的 elements 过滤):文件名/日期/分辨率/路径/GPS/
 * 相机/拍摄参数。轻量字段读 item(常驻布局行),重型字段读 meta(可视区懒加载,未到达时
 * 对应行缺席,数据到达后由调用方重渲染/重绘补上)。
 * demo(2026-09-16 演示打码):非空时把敏感字段的**文本**换成示例值,而每行的**存在性**判定
 * 仍走真实数据(不新增原来没有的行)。DOM 画廊不传此参,行为逐字不变。
 */
export function buildThumbInfoLines(
  item: ThumbInfoItemFields,
  meta: MediaMeta | undefined,
  elements: readonly string[],
  demo?: DemoInfoText | null,
): string[] {
  const lines: string[] = []
  if (elements.includes('filename') && meta?.fileName) {
    lines.push(demo ? demo.fileName : meta.fileName)
  }
  if (elements.includes('date') && item.sortDatetime) {
    lines.push(demo ? demo.date : new Date(item.sortDatetime * 1000).toLocaleString())
  }
  if (elements.includes('resolution') && item.originalWidth && item.originalHeight) {
    lines.push(demo ? demo.resolution : `${item.originalWidth} × ${item.originalHeight}`)
  }
  if (elements.includes('path') && meta?.dirPath) {
    lines.push(demo ? demo.dirPath : meta.dirPath)
  }
  if (elements.includes('geo') && meta?.gpsLat != null && meta?.gpsLng != null) {
    lines.push(demo ? demo.geo : `${meta.gpsLat.toFixed(4)}, ${meta.gpsLng.toFixed(4)}`)
  }
  if (elements.includes('camera') && (meta?.exifMake || meta?.exifModel)) {
    const make = meta?.exifMake || ''
    const model = meta?.exifModel || ''
    lines.push(demo ? demo.camera : `${make} ${model}`.trim())
  }
  if (elements.includes('params') && meta) {
    const params = []
    if (meta.exifFocalLength) params.push(`${meta.exifFocalLength}mm`)
    if (meta.exifAperture) params.push(`f/${meta.exifAperture}`)
    if (meta.exifShutter) params.push(`${meta.exifShutter}s`)
    if (meta.exifIso) params.push(`ISO${meta.exifIso}`)
    if (params.length > 0) lines.push(demo ? demo.params : params.join(' '))
  }
  return lines
}

// ── 文本文档卡(§3.4,DOM 与 Canvas 共享)────────────────────────────────────────

/** 不栅格化的文本文档格式(pdf/svg/epub 有真实缩略图):渲染为 CSS/canvas「文本卡」。 */
export const TEXT_CARD_FORMATS = [
  'txt',
  'md',
  'rtf',
  'doc',
  'docx',
  'xls',
  'xlsx',
  'ppt',
  'pptx',
  'odt',
  'ods',
  'odp',
] as const

/** 该项是否用「文本卡」呈现(document 类型且属于非栅格化格式)。 */
export function isTextCardFormat(mediaType: string, fileFormat: string | undefined): boolean {
  return (
    mediaType === 'document' &&
    (TEXT_CARD_FORMATS as readonly string[]).includes((fileFormat || '').toLowerCase())
  )
}

/**
 * 有真实缩略图的文档格式在「封面确定缺失」时的文本卡降级(深审 defer ⑤)。
 * 目前仅 epub:后端 OPF 三级启发式找不到封面图(自制/转制书常见)→ 派生 status=3 →
 * thumb_status=2,原先落通用灰底占位,观感反不如 txt。仅 status===2(盖棺无封面)才降级;
 * pending(0)保持占位色等待封面,有封面(1)走正常缩略图,不抢。
 * pdf/svg 不入此表:其封面由前端渲染器兜底,失败即真不可渲染,文本卡无从谈起体验一致性。
 */
export function isTextCardFallback(
  mediaType: string,
  fileFormat: string | undefined,
  thumbStatus: number,
): boolean {
  return (
    mediaType === 'document' && thumbStatus === 2 && (fileFormat || '').toLowerCase() === 'epub'
  )
}

/** 文本卡扩展名徽章的配色档(对应 --color-badge-doc-* token;DOM 走 CSS 类,canvas 查调色板)。 */
export function docBadgeKind(fileFormat: string | undefined): 'md' | 'word' | 'excel' | 'ppt' | 'generic' {
  switch ((fileFormat || '').toLowerCase()) {
    case 'md':
      return 'md'
    case 'doc':
    case 'docx':
    case 'odt':
      return 'word'
    case 'xls':
    case 'xlsx':
    case 'ods':
      return 'excel'
    case 'ppt':
    case 'pptx':
    case 'odp':
      return 'ppt'
    default:
      return 'generic'
  }
}

/**
 * 真正消费 viewport 元数据(get_meta_for_viewport)的信息元素。
 * 其余元素的字段直接来自 layout 行:date/resolution 取 item 自带值,
 * status/size/favorite/type 只看 item 标志位——勾这些不该触发元数据拉取。
 * 与 buildThumbInfoLines 里的 `meta?.x` 取用面一一对应,改那里须同步改这里。
 */
export const META_DRIVEN_INFO_ELEMENTS = ['filename', 'path', 'geo', 'camera', 'params'] as const

/** 当前勾选的信息元素里是否有人要 viewport 元数据(无人要则不必按视口拉 EXIF/GPS/路径)。 */
export function needsViewportMeta(elements: readonly string[]): boolean {
  return elements.some((el) => (META_DRIVEN_INFO_ELEMENTS as readonly string[]).includes(el))
}

/**
 * RAW 相机原始格式扩展名(镜像后端 `src-tauri/src/utils/format.rs::GROUP_RAW` 收录表)。
 *
 * 未从 registry(`list_registered_formats`)取此判定:registry 是异步 IPC 缓存(见
 * useFormatFilter.ts),而本判定要在逐格同步渲染路径(typeBadgeOf/占位符/查看器文案)求值,
 * 无法等一次网络往返。与 TEXT_CARD_FORMATS 同一先例——静态镜像一份扩展名表而非引入运行时
 * 依赖。改动后端 RAW 收录表须同步改这里。
 */
export const RAW_FORMATS = [
  'cr2',
  'cr3',
  'nef',
  'arw',
  'dng',
  'raf',
  'orf',
  'rw2',
  'pef',
  'srw',
] as const

/** 该扩展名是否属于 RAW 相机格式组(大小写不敏感)。 */
export function isRawFormat(fileFormat: string | undefined): boolean {
  return (RAW_FORMATS as readonly string[]).includes((fileFormat || '').toLowerCase())
}

/**
 * RAW 格式「已注册未解码」态:thumb_status===2(盖棺无缩略图,与损坏文件同底层状态码,
 * 但成因是「尚未接入解码」而非「文件损坏」)。用于网格占位符 + 类型角标的专属分支,
 * 避免与真损坏文件的通用空白占位混同(RAW 半成品过渡体验修复线)。
 */
export function isRawNoThumb(
  mediaType: string,
  fileFormat: string | undefined,
  thumbStatus: number,
): boolean {
  return mediaType === 'image' && isRawFormat(fileFormat) && thumbStatus === 2
}

/**
 * 类型角标(AUDIO/DOC/RAW)判定——DOM(MediaThumb)与 canvas(MediaGridCanvas)共享单源。
 * 两路此前靠「条件逐字对齐 DOM 模板 v-if」的注释维持一致(纯人肉守约),新增条件走
 * 单源可从构造上消除漂移(照 TEXT_CARD_FORMATS/docBadgeKind 既有范式)。
 *
 * 只标「本来没有类型标识」的项——不重复标记(用户 2026-07-15 裁决):
 * - audio:网格内无任何专属视觉(无播放键、无时长),角标是唯一标识。
 * - document:仅限走真实缩略图者(pdf/svg/有封面 epub)。文本卡格式自带扩展名角标
 *   (.media-thumb__textcard-ext),再叠 DOC 属重复。
 * - video:已有居中播放键 + 右下时长两重标识,不出 VIDEO 角标。
 * - photo:无须标,除非是 RAW 且尚无缩略图(isRawNoThumb)——那种情况网格永久空白,
 *   角标是唯一告知「非损坏,只是待解码」的信号。
 */
export function typeBadgeOf(
  mediaType: string,
  fileFormat: string | undefined,
  thumbStatus: number,
): 'audio' | 'document' | 'raw' | null {
  if (mediaType === 'audio') return 'audio'
  if (isRawNoThumb(mediaType, fileFormat, thumbStatus)) return 'raw'
  if (
    mediaType === 'document' &&
    !isTextCardFormat(mediaType, fileFormat) &&
    !isTextCardFallback(mediaType, fileFormat, thumbStatus)
  ) {
    return 'document'
  }
  return null
}

// ── 重排锚点(行高滑块/分组/排序/容器宽度/布局模式共用)────────────────────────

/**
 * pickReflowAnchor 需要的布局行字段子集(结构化约束,不引入完整 LayoutRow 依赖)。
 * items 可选:LayoutRow 联合的 separator 变体没有 items 字段,靠 rowType 判别——
 * 此处非判别联合,以可选字段承接两个变体。
 */
export interface ReflowAnchorRowLike {
  rowType: string
  y: number
  height: number
  items?: ReadonlyArray<{ id: number }>
}

/**
 * 重排锚点拾取:返回视口顶部(逻辑坐标 vTop)处第一个仍可见的 normal 行的首项 id,
 * 及该行相对视口顶的偏移(row.y - vTop,行顶已滚出时为负)。重排完成后按新布局里
 * 该项的 y 减去同一偏移回滚,即可把用户正在看的内容钉在屏上原位。
 *
 * 顶部特判(vTop ≤ 0 返回 null):列表顶端不押锚——排序翻转后「重排前的首个可视项」
 * 会跑到列表另一端,钉住它等于把顶部浏览者甩到底部;顶部用户的「原浏览位置」就是顶部,
 * 走 scrollCache 兜底(值为 0)原地不动。
 */
export function pickReflowAnchor<T extends ReflowAnchorRowLike>(
  rows: readonly T[],
  vTop: number,
): { id: number; screenOffset: number } | null {
  if (vTop <= 0) return null
  for (const row of rows) {
    if (row.rowType !== 'normal') continue
    if (row.items && row.items.length && row.y + row.height > vTop) {
      return { id: row.items[0].id, screenOffset: row.y - vTop }
    }
  }
  return null
}

/**
 * 二分查找:按 y 升序的 separators 中,**最后一个 `y ≤ targetY`** 的项(即 targetY 所在区段
 * 的分隔符)。用于「画廊→侧栏文件夹高亮」联动,替代 onGridScroll 里逐 scroll-event 的
 * O(滚动深度) 线性扫描——深处大库快滚时那是一处叠加放大 jank 的逐帧开销,二分降到 O(log n)。
 *
 * 泛型约束到 `{ y: number }`:传入完整 separator、拿回完整对象(调用方再读 groupId),类型安全。
 * @returns 命中项;若 targetY 在首个 separator 之前(或数组空)则 null。
 */
export function activeSeparatorAtY<T extends { y: number }>(
  separators: readonly T[],
  targetY: number,
): T | null {
  let lo = 0
  let hi = separators.length - 1
  let ans: T | null = null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (separators[mid].y <= targetY) {
      ans = separators[mid] // 候选,继续往右找更大的仍 ≤ targetY 的
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return ans
}

/// 快滚抑制缩略图加载的速度阈值(px/ms):高于此判为「飞掠」,关闸。
/// 经验标定:一帧(~16ms)慢速浏览位移 ~5-15px → ~0.3-1 px/ms;快速飞掠一帧位移
/// 常达 150-400px → ~9-25 px/ms。取 3 px/ms 落在二者之间的空档——低于此的慢滚照常
/// 出图(体感不变),高于此时段落地的挂载/解码来不及塞进惯性帧(症状①爆发区),
/// 推迟到降速/停稳再载(业界甩滚低保真做法)。
export const THUMB_LOAD_DEFER_VELOCITY = 3

/// 放行阈值(px/ms,滞回下沿):已关闸后须降到此速度**以下**才放行,与关闸阈值构成
/// 滞回带 [1.5, 3](带内维持现状)。瞬时速度有采样噪声,单阈值在减速穿越时会反复翻转
/// 闸门(审查 F3)——DOM compact 模式数千卡各持一个 watch(loadGate),每次翻转都是一轮
/// 全量 watcher 齐发。1.5 px/ms ≈ 90px/帧仍明显快于慢滚浏览,不伤「慢滚恒出图」。
export const THUMB_LOAD_RELEASE_VELOCITY = 1.5

/**
 * 由相邻两次 scroll 采样的位移与间隔算瞬时滚动速度(px/ms,取绝对值,纯函数单测锁定)。
 *
 * `dtMs <= 0`(同一帧内多次事件、或时钟未推进)返回 0——视为「不可判定」而非「无穷快」,
 * 避免把首帧/抖动误判成飞掠而错误抑制加载(宁可漏抑制,不可误伤慢滚出图)。
 * @param dyPx 两次采样间 scrollTop 位移(可正可负,内部取绝对值)。
 * @param dtMs 两次采样间隔(毫秒)。
 */
export function scrollVelocity(dyPx: number, dtMs: number): number {
  if (!(dtMs > 0)) return 0
  return Math.abs(dyPx) / dtMs
}

/**
 * 按行高缩放闸门双阈值(2026-07-10 真机回归修正):原 3/1.5 按「慢滚 5-15px/帧」标定,
 * 那是触摸板画像——鼠标滚轮是离散跳变,Chromium 平滑动画下连续拨轮(用户体感「慢速浏览」)
 * 峰值帧速度 2-4 px/ms 即越过关闸线,且动画帧持续处于滞回带内、事件间隔 <64ms 令释放
 * 定时器永被重排 → 闸门在整段慢滚中钉死,两模式同现占位墙。
 *
 * 闸门保护的真实成本是「单位时间进入视口的格数」:速度阈值应随行高等比抬升——
 * 60px 极密网格维持原 3/1.5(每屏数百格,妥协仍必要);行高翻倍则同速度下入视口格数
 * 减半(行数减半×每行更少),阈值翻倍;封顶 6 倍(≥360px 行高,18/9 仍能拦住真·飞掠)。
 * 放行阈值恒为关闸的一半,滞回带比例与原标定一致。
 */
export function thumbGateThresholds(rowHeight: number): { engage: number; release: number } {
  const scale = Math.min(Math.max(rowHeight / 60, 1), 6)
  const engage = THUMB_LOAD_DEFER_VELOCITY * scale
  return { engage, release: engage / 2 }
}

/**
 * 当前速度下是否应推迟缩略图加载启动(纯函数单测锁定,带滞回)。
 * 开闸态:严格大于关闸阈值才抑制——等于/低于(含 velocity=0 不可判定态)一律放行,
 * 保证慢滚与停稳恒出图。关闸态:须降到放行阈值以下才放行,带内维持关闸(防抖动翻转)。
 * @param wasDeferred 当前闸门态(上一采样的判定结果),滞回的记忆项。
 */
export function shouldDeferThumbLoad(
  velocity: number,
  wasDeferred: boolean,
  engageAbove: number = THUMB_LOAD_DEFER_VELOCITY,
  releaseBelow: number = THUMB_LOAD_RELEASE_VELOCITY,
): boolean {
  return wasDeferred ? velocity >= releaseBelow : velocity > engageAbove
}
