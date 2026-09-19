// src/types/media.ts
// 核心媒体数据类型，对应 Rust 模型

export type MediaType = 'image' | 'video' | 'audio' | 'document'
/**
 * 文件树专用分类。`other` 只表示树能看到但媒体分类器无法归入四类的文件，
 * 不进入图库 MediaType、URL 或 gallery filter。
 */
export type TreeCategory = MediaType | 'other'
export type ThumbStatus = 0 | 1 | 2 | 3 // 等待中 | 完成 | 失败 | 直接源

export interface ScanRoot {
  id: number
  path: string
  alias: string | null
  scanStatus: string
  scanProgress: number
  totalFiles: number
  lastScanAt: number | null
  isActive: boolean
  createdAt: number
  updatedAt: number
  /** 用户在设置页是否隐藏该根（V21，库级排除）：为真时其媒体从画廊/时间轴/搜索/统计/侧栏树全部排除。 */
  isHidden: boolean
}

/**
 * 侧边栏文件树的目录节点。
 *
 * **两种身份严格分开**（S 线 D-013）：
 * - `nodeKey`/`parentKey` = **路径身份**，恒有，是树的结构轴 —— DOM key、展开态、折叠、
 *   键盘 active 行全用它。「所有文件」模式下 FS-only 目录没有 DB 行，只有路径。
 * - `id`/`parentId` = **实体身份**，是媒体库的目录行 —— 路由 `/folder/:id`、滚动锚点、
 *   拖拽移动/复制用它。这些能力对 FS-only 目录本就不开放（§4.1），故它们留在 id 轴上。
 *
 * `nodeKey` 由**后端** `crate::tree::node_key` 产出（格式 `{rootId}:{relPath}`），前端
 * **不推导** —— 否则 DB 模式与 FS 模式就是两份 key 实现，分歧后果是同一目录被当成两个
 * 节点，且不报任何错。
 */
export interface DirNode {
  nodeKey: string
  /** 扫描根无父，为 null。 */
  parentKey: string | null
  /**
   * 媒体库目录行 id。**FS-only 目录为 null**（磁盘上有、库里没有 —— 扫描器整棵剪掉了
   * 隐藏目录，或格式全部未注册故从未入库）。禁止伪造：`/folder/:id` 会拿它去查库。
   */
  id: number | null
  rootId: number
  /** 父目录的库行 id；FS-only 目录（或其父是 FS-only）为 null。结构关系请用 `parentKey`。 */
  parentId: number | null
  name: string
  relPath: string
  depth: number
  /**
   * 子树递归媒体数（角标）。**「所有文件」模式为 null** —— 那是媒体数不是文件数，把它显示在
   * 一棵正在列全部文件的树上就是「媒体数冒充文件数」（§4.2 明禁）。
   *
   * null 意为「不适用/未知」，**不能用 0 冒充**：0 会渲染出一个「0」角标，读起来像「这里没东西」。
   */
  mediaCount: number | null
  /**
   * 是否有子目录。**「所有文件」模式为 null（未知）** —— 不 `read_dir` 每个子目录就无从知道，
   * 而那对一个上万项的目录意味着上万次枚举。资源管理器/VSCode 同样是「先给箭头，展开才知道」。
   *
   * 同样不用 `true` 冒充：那是断言「确有子目录」，而我们并不知道。可展开性请用
   * `isExpandable()` 判，别在各处自己拼判据。
   */
  hasChildren: boolean | null
  // UI 状态（非来自数据库）
  expanded?: boolean
  loading?: boolean
  children?: DirNode[]
  absPath?: string
  // 本目录的直接文件，展开时懒加载（侧边栏树文件列表）。
  files?: DirFile[]
  filesLoaded?: boolean
  // 分页(T2 百万级):本目录还有未加载的直接文件(已加载 files.length 条,后端探测到更多)。
  // 侧边栏树按页拉取,避免含数万散文件的目录一次性灌满 files 与拍平行数组。
  filesHasMore?: boolean
  /**
   * 「所有文件」模式下取下一页文件的游标（`TreePage.nextCursor` 原样存下）。
   *
   * 由**后端**给，前端不拿 `files.length` 反推：今天两者恰好相等（files 视图不丢项），但那是
   * 巧合不是契约 —— 同 `nodeKey`，能照抄的就别推导。DB 模式不用它（那边用 offset = 已加载数）。
   */
  filesCursor?: number
  /**
   * 隐藏项（点前缀 / Windows hidden 属性 / macOS UF_HIDDEN）。仅「所有文件+隐藏项」模式下
   * 可能为 true —— DB 模式的树按构造不含隐藏项（扫描器整棵剪掉），缺省即视作 false。
   * 前端据此淡化标签（R-06：与「背景填充=选中」视觉正交，只动文字/图标透明度）。
   */
  hidden?: boolean
}

// ── 文件树显示范围（S 线 §3 / D-009）─────────────────────────────────────────

/**
 * 文件树的三种互斥显示状态，按本机持久化。
 *
 * 没有第四态「已注册格式 + 隐藏项」：扫描器 `walker.rs` 整棵剪掉隐藏目录，`ensure_dir_chain`
 * 又只由已分类文件触发 —— 只含未知/隐藏文件的目录**连目录行都不存在**。第四态在当前扫描
 * 契约下无法兑现，不是没做而是做不出（§3）。
 *
 * 值与 Rust `TreeDisplayMode`（`#[serde(rename_all = "camelCase")]`）逐字节对应。
 */
export type TreeDisplayMode = 'registeredOnly' | 'allFiles' | 'allFilesWithHidden'

/**
 * 「所有文件」模式下的一条 FS 条目（对应 Rust `TreeEntry`）。
 *
 * 两种身份分离（D-013）：`nodeKey`/`parentKey` 恒有；`directoryId`/`mediaId`/`mediaType`
 * 只在媒体库确有对应行时出现（Rust 侧 `skip_serializing_if = "Option::is_none"`，故这里
 * 用可选属性而非 `| null`）。
 */
export interface TreeEntry {
  nodeKey: string
  parentKey: string
  rootId: number
  relPath: string
  name: string
  kind: 'dir' | 'file'
  hidden: boolean
  /** 格式是否已注册（内置表 ∪ exotic Catalog）。**与是否入库无关**：隐藏目录里的 PNG
   *  `registered = true` 但没有 `mediaId`（扫描器整棵剪掉了它，从没见过）。 */
  registered: boolean
  isSymlink: boolean
  directoryId?: number
  /**
   * **父**目录（即被列目录本身）的库行 id；父目录是 FS-only 时缺席。仅目录条目下发。
   * 供 FS 模式恢复库内目录的拖拽移动/复制（R-07：拖拽链全走 DB id，缺它则 FS 模式
   * 连库内目录也被禁拖，比设计 §4.1「只禁 FS-only」更紧且无处声明）。
   */
  parentDirectoryId?: number
  mediaId?: number
  mediaType?: MediaType
}

/** 一页树条目（对应 Rust `TreePage`）。 */
export interface TreePage {
  entries: TreeEntry[]
  /** 无更多页时后端不发该字段。 */
  nextCursor?: number
  /** 本目录直接子项总数。**不是**递归媒体数。 */
  total: number
}

/** 侧边栏树中目录下显示的轻量媒体文件行。同 {@link DirNode}：`nodeKey` 是路径身份（恒有），
 *  `id` 是媒体库实体身份。 */
export interface DirFile {
  /** 路径身份 `{rootId}:{relPath}`，`relPath` 含文件名本身。后端产出。 */
  nodeKey: string
  /** 所属目录的路径身份。 */
  parentKey: string
  /**
   * 相对扫描根的路径（含文件名本身）。**后端产出，前端不拼**。
   *
   * 与 `nodeKey` 冗余，但拿它才是对的：`reveal_tree_entry` 要 `rootId + relPath`，而从 `nodeKey`
   * 拆是在前端重造键格式的解析、从 `parentKey + fileName` 拼是在重造 Rust 的 `child_rel_path`
   * —— 两条产出侧本来都现成有这个值（DB 侧 queries.rs 算完塞进 node_key 就扔了，FS 侧 TreeEntry
   * 本就带着）。同 `nodeKey`：能照抄的就别推导。
   */
  relPath: string
  /**
   * 媒体库项 id。**未入库的文件为 null**（格式未注册、或在被扫描器剪掉的隐藏目录里）。
   *
   * 🔴 禁止伪造：`mediaRoute.ts` 对非 doc/audio 兜底到 `/view/{id}`，塞个假 id 不会报错，
   * 会把一个 `.exe` 静默送进图片查看器。null 即「此文件不可在应用内打开」。
   */
  id: number | null
  fileName: string
  /** 未入库的文件没有媒体类型（我们从没解析过它）。 */
  mediaType: MediaType | null
  isFavorited: boolean
  /** 同 {@link DirNode.hidden}：仅「所有文件+隐藏项」模式可能为 true，缺省视作 false。 */
  hidden?: boolean
}

export interface MediaItem {
  id: number
  directoryId: number
  fileName: string
  fileSize: number
  fileMtime: number
  fileFormat: string
  mediaType: MediaType
  width: number
  height: number
  durationMs: number | null
  sortDatetime: number
  cacheKey: number
  thumbStatus: ThumbStatus
  thumbPath: string | null
  thumbhash: number[] | null // 来自 Rust BLOB 的 Uint8Array
  isFavorited: boolean
  isDeleted: boolean
  deletedAt: number | null
  rating: number
  /** 用户颜色标签 0-7（0=未标）。与 rating 同类的逐项小标量，供详情页单项设色（T16）。 */
  colorLabel: number
  /**
   * 看图台用户展示旋转（归一化 0/90/180/270，顺时针；V20）。与拍摄内在方向（EXIF orientation /
   * 视频 rotation 元数据）正交——此为用户偏好、可改回。打开大图时据此复原上次旋转朝向。
   */
  viewRotation: number
  /**
   * 播放器上次退出时的播放进度(毫秒;V23)。重开同一视频时据此续播,与 viewRotation 同姿态——
   * 用户会话偏好、随时可覆写。
   */
  playbackPositionMs: number
  isLivePhoto: boolean
  hasEmbeddedVideo: boolean
  companionOf: number | null
  contentHash: string | null
  createdAt: number
  updatedAt: number
}

export interface ImageMeta {
  itemId: number
  orientation: number
  exifDatetime: number | null
  exifMake: string | null
  exifModel: string | null
  exifLens: string | null
  exifFocalLength: number | null
  exifAperture: number | null
  exifShutter: string | null
  exifIso: number | null
  exifGpsLat: number | null
  exifGpsLng: number | null
  dominantHue: number | null
  dominantSat: number | null
  dominantLum: number | null
  dominantHex: string | null
  isMonochrome: boolean
}

export interface MediaDetail extends MediaItem {
  absPath: string
  imageMeta: ImageMeta | null
  /** 系统可用态（缺失检测 Part2 §3.2）：'online' | 'offline' | 'missing'。
   *  查看器据此对「卷离线/文件缺失」明确提示，替代 broken 图标。 */
  availability: string
  /** 视频元数据(播放器线):`null`=非视频或尚未 enrichment 探测出 video_meta 行。 */
  videoMeta: VideoMeta | null
}

/** 镜头查看器的一步邻接结果；顺序位置由后端 layoutVersion 缓存返回。 */
export interface LensAdjacentMedia {
  detail: MediaDetail
  index: number
  totalCount: number
}

/** 视频元数据投影(播放器线):字段对齐后端 `VideoMeta` serde 输出。 */
export interface VideoMeta {
  videoCodec: string | null
  fps: number | null
  bitrate: number | null
  rotation: number
  hasAudio: boolean
}

export interface AppStats {
  totalItems: number
  totalImages: number
  totalVideos: number
  totalAudios: number
  totalDocuments: number
  totalFavorited: number
  totalDeleted: number
  totalLivePhotos: number
}

export interface ThumbResult {
  itemId: number
  thumbStatus: ThumbStatus
  thumbPath: string | null
  thumbhash: number[] | null
}

export interface DateRange {
  from: number // unix 时间戳
  to: number
}

export interface MediaFilter {
  directoryId?: number | null
  albumId?: number | null // 用户收藏夹成员过滤（系统夹改用 mediaTypes + favoritedOnly）
  mediaTypes?: MediaType[] | null
  favoritedOnly?: boolean | null
  minRating?: number | null
  dateRange?: DateRange | null
  livePhotoOnly?: boolean | null
  searchQuery?: string | null
  searchScope?: string | null
  aiSearch?: boolean | null
  aiThreshold?: number | null
}

// ── 收藏夹（需求7） ────────────────────────────────────────────────────────────

/** 由 albums 表承载的收藏夹。 */
export interface Collection {
  id: number
  name: string
  kind: 'system' | 'user'
  mediaTypeFilter?: string | null // 系统夹的类型：image/video/audio/document
  icon?: string | null // lucide 图标名
  coverItemId?: number | null
  itemCount: number
  sortOrder: number
}
