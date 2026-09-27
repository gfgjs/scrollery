//! 已注册格式的**运行时并集** = 内置表（`utils::format::BUILTIN_FORMATS`）∪ exotic Catalog。
//!
//! 为什么单独成模块（S 线 D-007）：`utils::format` **不能**引用 `exotic::catalog` —— 反向依赖
//! 已存在（`exotic/catalog.rs` 装载期要调 `classify_media_type` 判 common 冲突），放一起即成环。
//! 而「已注册格式全集」在语义上也不属于 exotic —— exotic 只是它的一个 **source**。故合并层
//! 独立在此，向下依赖 `utils::format` 与 `exotic::catalog` 两侧。
//!
//! **分层**（S 线 §6.1）：
//! - `utils::format::RegisteredFormatDef` = **内部**定义，带处理能力字段（`phase1_image` /
//!   `document_subtype`）。
//! - [`FormatDescriptor`] = **UI 投影**，只有 `ext/media_type/group/source`。处理能力字段
//!   **不下发** —— 避免把「能不能派生」误当成「能不能筛」（availability ⊥ registered）。
//!
//! **不写死总数**：当前 66 内置 + 1 PSD = 67 只是**数据快照**，不是协议常量。新增普通冷门格式
//! 只需给 Catalog 加 offering，本模块、facet、筛选链路与契约测试全部自动覆盖。

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

/// 下发给 UI 的只读格式描述（P3 的格式弹层与 facet 分组据此渲染）。
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
/// 多个 `formats` **只表示同一插件能处理它们**，不等于一个 UI alias group（D-007）。将来若需
/// 别名合组，走 Catalog schema 的显式可选 group 字段，禁止从 offering 边界猜测。
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
