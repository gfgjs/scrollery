// 插件与专业功能的可用状态；免费激活与资源准备分别表达。

export type Availability =
  | 'availableUninstalled'
  | 'inactive'
  | 'ready'
  | 'unsupportedPlatform'
  | 'incompatibleHost'
  | 'invalidInstallation'
  | 'disabled'
  | 'noOffering'

/** 四项功能各自保存的免费激活记录。 */
export interface FeatureActivation {
  featureId: string
  activated: boolean
}

/** 插件可用状态与独立激活记录。 */
export interface PluginAvailability {
  pluginId: string
  availability: Availability
  activationRequired: boolean
  activated: boolean
}

/** 功能卡所需的激活、安装和模型信息。 */
export interface FeatureOffering {
  id: string
  name: string
  activationRequired: boolean
  activated: boolean
  builtin: boolean
  availability: Availability
  resources: 'ready' | 'included' | 'modelMissing' | 'manifestUnready' | 'workerMissing' | 'needsInstall'
}

export interface FeatureOfferings { features: FeatureOffering[] }

/** 媒体大类（后端 `exotic::MediaKind`，serde 小写）。 */
export type MediaKind = 'image' | 'video' | 'audio' | 'document'

/** 能力类型（后端 `exotic::Capability`，serde 小写）。首发只交付 thumbnail。 */
export type ExoticCapability = 'thumbnail' | 'metadata' | 'text' | 'embedding' | 'face_detect_embed' | 'enhance'

/**
 * 结构化格式解析结果（后端 `exotic::FormatResolution`，来自 `get_exotic_item_state` /
 * `list_exotic_format_resolutions`）。比 `PluginAvailability` 多带格式/能力/安装版本。
 */
export interface FormatResolution {
  format: string
  mediaKind: MediaKind
  /** 提供该格式的插件 id；非 catalog 格式为 null。 */
  pluginId: string | null
  /** 展示名（来自 Catalog；旧后端/测试夹具可能没有，前端回退 pluginId）。 */
  displayName?: string
  capabilities: ExoticCapability[]
  availability: Availability
  activationRequired: boolean
  activated: boolean
  installedVersion: string | null
  /** 内置能力插件(无安装包，激活状态由本地记录决定)。 */
  builtin: boolean
}

/**
 * 单个媒体项的 exotic 状态（后端 `ExoticItemState`，来自 `get_exotic_item_state`）。
 * `resolution` 为 null 表示该格式不在 exotic catalog（即普通格式，无需 gate）。
 */
export interface ExoticItemState {
  itemId: number
  format: string
  resolution: FormatResolution | null
  /** thumbnail 任务处理态：none/pending/processing/done/retryableError/terminalError。 */
  taskState: string
}

// ── 插件商店 DTO ───────────

/** 安装状态（后端 `exotic_plugins.install_state`）。 */
export type ExoticInstallState = 'installed' | 'disabled' | 'broken'

/**
 * 签名 Registry 的可安装条目（后端 `ExoticRegistryEntry`，来自 `list_exotic_registry`）。
 * 不暴露内部下载坐标（hash/size/url），仅市场展示所需。
 */
export interface ExoticRegistryEntry {
  pluginId: string
  /** 展示名（来自 Catalog；旧后端可能没有，前端回退 pluginId）。 */
  name?: string
  version: string
  formats: string[]
  capabilities: string[]
  sku: string
  target: string
  packageSequence: number
  storeUrl: string | null
  /** 该 Registry 是否已过期（过期仍展示，但不允许新装）。 */
  registryExpired: boolean
}

/** 已安装插件（后端 `InstalledExoticPlugin`，来自 `list_installed_exotic_plugins`）。 */
export interface InstalledExoticPlugin {
  pluginId: string
  version: string
  packageSequence: number
  installState: ExoticInstallState
  installedAt: number
  updatedAt: number
}

/** `fetch_exotic_registry` 拉取结果摘要（刷新后：装得了几个 / 序号 / 是否过期）。 */
export interface RegistrySummary {
  pluginCount: number
  sequence: number
  expired: boolean
}

/**
 * exotic 处理状态摘要（后端 `ExoticProcessingStatus`，来自 `get_exotic_processing_status`）。
 * `blockedByAvailability` 单列「未激活 / 平台不支持 / 未安装」而卡住的待处理数，避免进度条永久停 0%。
 */
export interface ExoticProcessingStatus {
  pending: number
  processing: number
  done: number
  error: number
  blockedByAvailability: number
  running: boolean
  paused: boolean
}

/** 详情筛选桶（与摘要四桶对齐；后端 `list_exotic_task_details` 的 bucket 参数）。 */
export type ExoticTaskBucket = 'pending' | 'processing' | 'done' | 'error'

/** 详情行状态（桶内细分：待重试 retrying 归 pending 桶但单独标示）。 */
export type ExoticTaskDetailStatus = 'pending' | 'retrying' | 'processing' | 'done' | 'error'

/**
 * 处理详情行（后端 `ExoticTaskDetail`，来自 `list_exotic_task_details`）——
 * 进度区「展开详情」的文件级投影。
 */
export interface ExoticTaskDetail {
  itemId: number
  fileName: string
  /** 所在目录相对扫描根的路径（根目录为空串）。 */
  dirPath: string
  format: string
  status: ExoticTaskDetailStatus
  attempts: number
  lastErrorCode: string | null
  lastErrorMessage: string | null
}
