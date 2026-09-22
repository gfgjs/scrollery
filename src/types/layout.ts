// src/types/layout.ts
// 两端对齐布局行类型（对应 Rust 的 LayoutRow 枚举）

/** 重复镜头逐项分类桶（方案 §3.4）。 */
export type DuplicateBucket = 'duplicate' | 'unconfirmed' | 'unique'

/** 分隔符类别（方案 §11.2）：date/folder 为普通画廊既有分组；duplicateGroup=重复组头，duplicateFolder=镜头文件夹头。 */
export type GallerySeparatorKind = 'date' | 'folder' | 'duplicateGroup' | 'duplicateFolder'

// 常驻逐项行数据（为百万项内存保持精简）。重型元数据（fileName/dirPath/EXIF/GPS）
// 不在此处 —— 经 MediaMeta 按需拉取。
export interface LayoutRowItem {
  id: number
  x: number
  w: number
  h: number
  fileSize: number
  fileFormat: string
  mediaType: string
  isLivePhoto: boolean
  durationMs: number | null
  thumbStatus: number
  thumbPath: string | null
  /** 占位平均色 CSS `#rrggbb`(null → 回退 CSS 变量)。后端 hydrate 时由 thumbhash 算好过桥
   *  (原 thumbhash 数组过桥 + 前端逐格逐帧现算均色已废,见 Rust average_color_hex)。 */
  placeholderColor: string | null
  similarity?: number
  isFavorited: boolean
  /** 用户评分 0-5（0 = 未评分）。与 isFavorited 同类的逐项小标量，供网格星级显示 + hover 快捷评分 + 「≥N 星」筛选。 */
  rating: number
  /** 用户颜色标签 0-7（0 = 未标）。与 rating 同类的逐项小标量，供网格 swatch 显示 + 按色筛选（T16）。 */
  colorLabel: number
  /** 系统可用态 'online'|'offline'|'missing'（缺失检测 Part2 §3.2）：前端置灰+角标。 */
  availability: string
  originalWidth: number
  originalHeight: number
  sortDatetime: number
  /** 重复镜头投影（方案 §11.1，全部可选：普通画廊行不含这些键）：
   * bucket=分类桶；groupOrdinal=组短编号；memberOrdinal/memberCount=组内第 M/共 N 项。 */
  duplicateBucket?: DuplicateBucket
  duplicateGroupOrdinal?: number
  duplicateMemberOrdinal?: number
  duplicateMemberCount?: number
}

// 仅为可视区按需拉取的逐项重型元数据。
export interface MediaMeta {
  id: number
  fileName: string
  dirPath: string | null
  gpsLat: number | null
  gpsLng: number | null
  exifMake: string | null
  exifModel: string | null
  exifLens: string | null
  exifFocalLength: number | null
  exifAperture: number | null
  exifShutter: string | null
  exifIso: number | null
}

export interface LayoutRowNormal {
  rowType: 'normal'
  y: number
  height: number
  items: LayoutRowItem[]
}

export interface LayoutRowSeparator {
  rowType: 'separator'
  y: number
  height: number
  separatorLabel: string
  groupId?: string
  /** 重复镜头投影（方案 §11.2，全部可选）：separatorKind=分隔符类别；
   * parentGroup* 描述 folders 模式的关联簇头信息；duplicate/unconfirmed/unique 计数
   * 与 uniqueHidden 描述文件夹头统计（独有项隐藏时数量仍准确显示）。 */
  separatorKind?: GallerySeparatorKind
  parentGroupId?: string
  parentGroupStart?: boolean
  /** groups 镜头组头结构化数值；组头文本由前端 i18n 生成。 */
  duplicateGroupOrdinal?: number
  duplicateMemberCount?: number
  duplicateFolderCount?: number
  duplicateUnitSize?: number
  /** folders 镜头关联簇结构化数值；separatorLabel 仍是原始路径。 */
  parentGroupOrdinal?: number
  parentGroupFolderCount?: number
  parentGroupGroupCount?: number
  duplicateCount?: number
  unconfirmedCount?: number
  uniqueCount?: number
  uniqueHidden?: boolean
}

export type LayoutRow = LayoutRowNormal | LayoutRowSeparator

/** LayoutSummary 中的分隔符投影；与 LayoutRowSeparator 共享镜头数值字段。 */
export interface LayoutSeparatorInfo {
  label: string
  y: number
  groupId?: string
  count: number
  epochDay: number | null
  separatorKind?: GallerySeparatorKind
  duplicateGroupOrdinal?: number
  duplicateMemberCount?: number
  duplicateFolderCount?: number
  duplicateUnitSize?: number
  parentGroupOrdinal?: number
  parentGroupFolderCount?: number
  parentGroupGroupCount?: number
}

/// 月密度桶（T14 §3.8.3）：date 分组下同月日分隔符合并而成，供时间轴 scrubber 按时间均布 +
/// 密度条渲染。仅 date 分组非空（folder/none 为空数组）。`groupId="YYYY-MM"` 可按月→y 定向滚动。
export interface MonthBucket {
  year: number
  month: number // 1-12
  count: number // 该月媒体项数（密度条高度依据）
  y: number // 该月首个分隔符逻辑 y（scrubber 跳转定位）
  groupId: string // "YYYY-MM"
}

export interface LayoutSummary {
  totalRows: number
  totalHeight: number
  layoutVersion: number
  /** 成员及顺序身份；纯几何重排沿用，供全集 ID 与邻接缓存复用。 */
  orderVersion: number
  totalItems: number
  /// date 分组：每日一项（label=日期串，count=该日项数）；folder 分组：每文件夹一项（count=文件数）。
  /// epochDay = 该日 UTC 天数（date 分组），folder/none 分组为 null；P3 时间比例坐标据此线性
  /// 铺行 + 相邻间隔 >1 检测空日间隙。serde `Option<i64>` → `number | null`（None 序列化为 null）。
  separators: LayoutSeparatorInfo[]
  /// date 分组才非空（见 MonthBucket）。
  monthBuckets: MonthBucket[]
}
