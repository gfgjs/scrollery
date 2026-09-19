// src/types/exotic.ts
// Exotic 插件平台类型定义（Part5 T11/T12，消费 Part6 后端）
//
// 前后端职责：这些类型只承载后端**已判定**的授权态；前端**不持任何验签逻辑**，
//    授权真相由后端对签发凭证验签得出。

/**
 * 格式可用态（后端 `exotic::Availability`，serde camelCase）。**只**描述可用性；任务处理态另有其型。
 */
export type Availability =
  | 'availableUninstalled' // 有产品、未安装 → 显示购买占位
  | 'installedUnlicensed' // 已安装、未授权 → 显示激活/购买
  | 'authorized' // 已授权、可运行
  | 'licenseExpired' // License 过期 → 续订
  | 'unsupportedPlatform' // 当前平台无对应包
  | 'incompatibleHost' // Host 版本不满足 min_host_version
  | 'invalidInstallation' // 安装损坏（hash/清单不符）
  | 'disabled' // 子系统/插件被禁用
  | 'noOffering' // 无 offering（无产品可售）

/**
 * 某插件的授权判定（后端 `exotic::PluginEntitlement`，来自 `get_plugin_entitlement` IPC）。
 */
export interface PluginEntitlement {
  pluginId: string
  /** 折叠后的可用态（平台 / 版本 / 安装 / 授权门控结果）。 */
  availability: Availability
  /** 授权来源渠道（'direct' = keyring 直销 / 'free' = 未授权回退桩），取自后端 EntitlementProvider。 */
  sourceTag: string
  /** 付费插件的 sku（免费 / 无 sku 插件为 null）。 */
  sku: string | null
  /** 购买 / 商店链接（未授权时的购买引导用；无则 null）。 */
  storeUrl: string | null
}

/** 媒体大类（后端 `exotic::MediaKind`，serde 小写）。 */
export type MediaKind = 'image' | 'video' | 'audio' | 'document'

/** 能力类型（后端 `exotic::Capability`，serde 小写）。首发只交付 thumbnail。 */
export type ExoticCapability = 'thumbnail' | 'metadata' | 'text' | 'embedding' | 'face_detect_embed' | 'enhance'

/**
 * 结构化格式解析结果（后端 `exotic::FormatResolution`，来自 `get_exotic_item_state` /
 * `list_exotic_format_resolutions`）。比 `PluginEntitlement` 多带格式/能力/安装版本，但**不含** sku/sourceTag。
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
  storeUrl: string | null
  installedVersion: string | null
  /** 内置能力插件(无安装包,直接走 license 门控;T11 商店用它拆分「内置能力插件」区块)。 */
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

// ── 插件商店 DTO（Part5 T11，消费 Part6 registry/install/processing 命令）───────────

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
 * `blockedByAvailability` 单列「未授权 / 平台不支持 / 未安装」而卡住的待处理数，避免进度条永久停 0%。
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
