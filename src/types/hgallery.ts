// src/types/hgallery.ts
// H-Lab 横向画廊实验类型(镜像 Rust layout/horizontal.rs 与 ipc/hgallery_commands.rs 的
// serde camelCase 序列化;docs/designs/2026-07-02-horizontal-gallery-lab.md §4)。

/** 实验项:仅缩略图渲染所需字段,x/y/w/h 为全局绝对坐标(渲染层不感知布局模式)。 */
export interface HItem {
  id: number
  x: number
  y: number
  w: number
  h: number
  mediaType: string
  fileFormat: string
  fileSize: number
  isLivePhoto: boolean
  durationMs: number | null
  thumbStatus: number
  thumbPath: string | null
  thumbhash: number[] | null
}

/** 虚拟化单元:bbox 覆盖其全部子项(lanes 模式相邻块 bbox 可轻微重叠)。 */
export interface HBlock {
  x: number
  width: number
  items: HItem[]
}

export interface HLayoutSummary {
  totalWidth: number
  blockCount: number
  totalItems: number
  layoutVersion: number
  /** 后端「查询 + 布局」耗时 ms(实验控制条展示)。 */
  computeMs: number
}

/** 布局模式判别联合(internally-tagged,与 Rust HLayoutMode 一致)。 */
export type HLayoutMode =
  | { mode: 'paged'; pageFactor: number; targetRowHeight: number }
  | { mode: 'lanes'; laneCount: number; balance: boolean }
  | { mode: 'columns'; targetColWidth: number }

export type HLayoutModeKind = HLayoutMode['mode']
