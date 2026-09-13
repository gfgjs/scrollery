// src/types/view.ts
// R1-2（S4/T4c）· 后端 ViewDescriptor 的前端镜像。
//
// wire 形状由后端锁测试钉死（queries.rs::selection_descriptor_wire_format_locks_camel_case）：
// tag 变体名小驼峰（'directory' / 'selectAll'），struct 变体字段小驼峰（directoryId / excludedIds）。
// 修改任何一侧都必须同步另一侧 + 锁测试。
//
// 2026-09-02 重复镜头方案（docs/designs/2026-09-02-主画廊重复项浏览方案.md §10.2）新增
// DuplicateLensDescriptorDto：Rust 侧由并行任务同步扩展，wire 形状以本契约为准。
//
// 注意：SemanticSearch scope 刻意不镜像 —— 后端 view_to_sql 拒绝解析它（v1 语义搜索非纯 SQL），
// 语义模式下的批量操作由调用方回退 Explicit（物化 id）。

/** 视图集合来源（scope 定来源，filter 在来源上再筛 —— 对齐后端 ViewScope，D1）。 */
export type ViewScopeDto =
  | { kind: 'all' }
  | { kind: 'directory'; directoryId: number }
  | { kind: 'collection'; albumId: number }
  | { kind: 'person'; personId: number }
  | { kind: 'trash' }

/** 附加筛选（对齐后端 GalleryFilter；全部可选，undefined 字段序列化时被丢弃）。 */
export interface GalleryFilterDto {
  mediaTypes?: string[]
  /**
   * 细分格式（S 线 D-011）：规范化小写扩展名，与 `mediaTypes` 取 AND、维度内 OR。
   *
   * 🔴 **必须随描述符走**：SelectAll 经后端 `to_media_filter()` 解析全集，这里漏掉不会报错 ——
   * 批量收藏/评分/删除照跑，只是作用到**比画面更大的集合**上。
   */
  fileFormats?: string[]
  livePhotoOnly?: boolean
  favoritedOnly?: boolean
  minRating?: number
  colorLabel?: number
  dateRange?: { from: number; to: number }
  searchQuery?: string
  searchScope?: string
  /** 「最近导入」智能相册（R1-2 后端补的对应字段）。 */
  recentOnly?: boolean
}

/** 排序规格（对齐后端 SortSpec；与 uiStore.groupBy / sortWithinGroup / sortOrder 同源）。 */
export interface SortSpecDto {
  groupBy: string
  sortWithinGroup: string
  sortOrder: string
}

/** 重复镜头模式（2026-09-02 方案 §10.2）：groups=按重复组，folders=按关联文件夹。 */
export type DuplicateLensModeDto = 'groups' | 'folders'

/**
 * 主画廊重复镜头描述符（对齐后端 DuplicateLensDescriptor；修改任一侧须同步另一侧）。
 * 附加在 ViewDescriptorDto 上：缺失 = 普通画廊（wire 形状不变）。
 * MVP 校验（后端执行，前端装配时同样必须满足）：scope=all、filter 为空、
 * groups 模式 showUniqueItems 必须 false、orderingVersion 固定 1。
 */
export interface DuplicateLensDescriptorDto {
  mode: DuplicateLensModeDto
  showUniqueItems: boolean
  /** 排序契约版本：镜头排序语义调整时递增，防旧描述符被新语义错误执行。 */
  orderingVersion: number
}

/** 不可变视图描述符：唯一确定「当前画廊视图全集 + 序」，SelectAll 解析的依据。 */
export interface ViewDescriptorDto {
  scope: ViewScopeDto
  filter: GalleryFilterDto
  sort: SortSpecDto
  /** 重复镜头（方案 §10.2）：undefined = 普通画廊。 */
  duplicateLens?: DuplicateLensDescriptorDto
  /** 与后端 LayoutCache.layout_version 对齐；不一致时后端拒绝（ViewStale）。 */
  layoutVersion: number
}
