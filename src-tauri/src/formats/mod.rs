//! 已注册格式的运行时并集：内置表与 exotic Catalog。
//!
//! `exotic::catalog` 依赖 `utils::format` 判定内置格式冲突；合并层单独放置，避免反向依赖。
//!
//! 两层数据用途不同：
//! - `utils::format::RegisteredFormatDef` = **内部**定义，带处理能力字段（`phase1_image` /
//!   `document_subtype`）。
//! - [`FormatDescriptor`] = **UI 投影**，只有 `ext/media_type/group/source`。处理能力字段
//!   不下发，避免把派生能力误当成筛选条件。

use serde::Serialize;

use crate::exotic::catalog::CatalogSnapshot;
use crate::utils::format::{builtin_formats, MediaType};

/// 一个已注册格式的来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormatSource {
    /// 内置 common 表。
    Builtin,
    /// exotic Catalog 登记（插件可能尚未安装/授权 —— 那是 availability，不影响「已注册」）。
    Exotic { plugin_id: String },
}

/// 下发给 UI 的只读格式描述，供格式弹层与 facet 分组使用。
///
/// `group` 是 **UI 显示分组**：一个 UI 概念 → 多个物理扩展名（JPEG={jpg,jpeg}、RAW={cr2,…}）。
/// DB / IPC 状态 / URL **始终存 `ext`**，`group` 不落库。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatDescriptor {
    pub ext: String,
    pub media_type: MediaType,
    pub group: Option<String>,
    pub source: FormatSource,
}

/// 内置 ∪ Catalog 的全部已注册格式，按 `ext` 升序（稳定输出：UI 与测试都不必再排）。
///
/// 两侧**不会**撞名：Catalog 装载期即拒绝与 common 撞名的整表
/// （`CatalogError::CommonFormatConflict`），故此处无需再做冲突消解。
///
/// exotic 格式的 `group` 恒为 `None`：Catalog schema 目前没有 group 元数据，而一个 offering 的
/// 多个 `formats` 只表示同一插件能处理它们，不能据此推断为同一 UI 分组。
pub fn merged_formats(catalog: &CatalogSnapshot) -> Vec<FormatDescriptor> {
    let mut out: Vec<FormatDescriptor> = builtin_formats()
        .iter()
        .map(|d| FormatDescriptor {
            ext: d.ext.to_string(),
            media_type: d.media_type,
            group: d.group.map(str::to_string),
            source: FormatSource::Builtin,
        })
        // builtin offering(D-OCR-5)跳过:它是能力插件而非文件格式,不进扩展名集/扫描器面
        // ——"ocr" 等标记式 format 字符串不是真实文件扩展名,不该出现在格式弹层/facet 分组里。
        .chain(
            catalog
                .iter_formats()
                .filter(|(_, off)| !off.builtin)
                .map(|(ext, off)| FormatDescriptor {
                    ext: ext.clone(),
                    media_type: off.media_kind.into(),
                    group: None,
                    source: FormatSource::Exotic {
                        plugin_id: off.plugin_id.clone(),
                    },
                }),
        )
        .collect();
    out.sort_by(|a, b| a.ext.cmp(&b.ext));
    out
}
