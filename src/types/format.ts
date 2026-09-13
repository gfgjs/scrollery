// src/types/format.ts
// 已注册格式的 UI 投影（对应 Rust `formats::FormatDescriptor`，S 线 §6.1 / D-007）。

import type { MediaType } from './media'

/**
 * 一个已注册格式的来源（对应 Rust `FormatSource`，`#[serde(tag = "kind")]`）。
 *
 * exotic = Catalog 登记；**插件可能尚未安装/授权** —— 那是 availability，与「已注册」正交（§2）。
 * 即：未装插件的 PSD 照样能被筛出来，只是可能预览不了（弹层给 badge，不禁用选项）。
 */
export type FormatSource = { kind: 'builtin' } | { kind: 'exotic'; pluginId: string }

/**
 * 一个已注册格式（`list_registered_formats` 下发）。
 *
 * **只有 UI 需要的四个字段**：后端的处理能力字段（`phase1_image` / `document_subtype`）有意
 * 不下发 —— 下发了迟早有人拿它当「能不能筛」用，而那是 availability 维度（§6.1）。
 */
export interface FormatDescriptor {
  /** 规范化小写扩展名。DB / API / URL 存的都是它。 */
  ext: string
  mediaType: MediaType
  /**
   * UI 显示分组：一个 UI 概念 → 多个物理扩展名（JPEG={jpg,jpeg} / TIFF={tif,tiff} /
   * RAW={cr2,nef,…}）。**不落库、不进 API、不进 URL**。
   *
   * 🔴 由后端下发，前端**不得**硬编码 —— 那是把 `utils::format` 的表抄第二遍（D-003）。
   * exotic 格式恒为 null：Catalog schema 没有 group 元数据，而一个 offering 的多个 formats
   * 只表示同一插件能处理它们，不等于一个 UI alias group。
   */
  group: string | null
  source: FormatSource
}
