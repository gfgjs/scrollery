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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::walker::classify_scanned_file;
    use crate::utils::format::{classify_media_type, GROUP_JPEG, GROUP_RAW};
    use std::collections::HashSet;

    /// 双 offering fixture：证明**新增 exotic 只改数据**，无需碰中央白名单或期望总数。
    /// 第二个 offering 有意用多格式 + 非 image 大类，覆盖「offering 多格式 ≠ 一个 alias group」。
    const TWO_OFFERING_CATALOG: &str = r#"{
      "schema": 1,
      "sequence": 2,
      "offerings": [
        {
          "plugin_id": "exotic-image-psd",
          "name": "PSD 图像引擎",
          "media_kind": "image",
          "formats": ["psd"],
          "capabilities": ["thumbnail"],
          "license_tier": "paid",
          "sku": "psd-engine-2026",
          "platforms": ["x86_64-pc-windows-msvc"],
          "min_host_version": "0.1.0"
        },
        {
          "plugin_id": "exotic-doc-cad",
          "name": "CAD 文档引擎",
          "media_kind": "document",
          "formats": ["dwg", "dxf"],
          "capabilities": ["thumbnail"],
          "license_tier": "paid",
          "sku": "cad-engine-2026",
          "platforms": ["x86_64-pc-windows-msvc"],
          "min_host_version": "0.1.0"
        }
      ]
    }"#;

    fn two_offering_catalog() -> CatalogSnapshot {
        CatalogSnapshot::parse(TWO_OFFERING_CATALOG).expect("fixture catalog 应可解析")
    }

    #[test]
    fn merged_covers_builtin_and_catalog_without_central_list() {
        let cat = two_offering_catalog();
        let merged = merged_formats(&cat);
        let exts: HashSet<&str> = merged.iter().map(|d| d.ext.as_str()).collect();

        // 内置侧全覆盖 —— 基数由**实际数据**推导，不硬编码 66/67。
        for d in builtin_formats() {
            assert!(exts.contains(d.ext), "合并集缺内置格式 {}", d.ext);
        }
        // exotic 侧全覆盖：第二个 offering 的两个格式**自动**进入，无需改本测试的任何常量。
        for ext in ["psd", "dwg", "dxf"] {
            assert!(exts.contains(ext), "合并集缺 exotic 格式 {ext}");
        }
        // 基数 = 两侧之和（装载期 CommonFormatConflict 保证不相交）。
        assert_eq!(
            merged.len(),
            builtin_formats().len() + cat.iter_formats().count(),
            "合并集基数不等于两侧之和（出现重复或丢失）"
        );
    }

    #[test]
    fn builtin_offering_excluded_from_merged_but_still_resolvable() {
        // builtin(D-OCR-5,如 exotic-ocr)不是文件格式,不该混进扩展名并集/扫描器面;
        // 但 resolve_format 直查 catalog,不经 merged_formats——availability 通路不受影响。
        let json = r#"{"schema":1,"sequence":1,"offerings":[
          {"plugin_id":"exotic-ocr","name":"OCR","media_kind":"image","formats":["ocr"],
           "capabilities":["text"],"license_tier":"paid","sku":"ocr-engine-2026",
           "platforms":[],"min_host_version":"0.1.0","distribution":"builtin"}
        ]}"#;
        let cat = CatalogSnapshot::parse(json).expect("fixture catalog 应可解析");
        let merged = merged_formats(&cat);
        assert!(
            merged.iter().all(|d| d.ext != "ocr"),
            "builtin offering 的标记式 format 不该出现在合并扩展名集"
        );
        assert!(
            cat.resolve_format("ocr").is_some(),
            "builtin offering 仍应可经 catalog 直接解析(availability 通路不断)"
        );
    }

    #[test]
    fn merged_matches_combined_classifier_both_ways() {
        // 合并集必须与扫描期真正用的 common-first 分类器逐值一致 ——
        // 否则 UI 显示「能筛 X」而扫描器根本不收 X（或反之）。
        let cat = two_offering_catalog();
        let merged = merged_formats(&cat);

        // 正向：合并集每一项，扫描器都认，且大类一致。
        for d in &merged {
            assert_eq!(
                classify_scanned_file(&d.ext, &cat),
                Some(d.media_type),
                "合并集 {} 的大类与扫描期分类器不一致",
                d.ext
            );
        }
        // 反向：扫描器认的，合并集都得有（抽样已知集合 + 未知格式必须两侧都不认）。
        let exts: HashSet<&str> = merged.iter().map(|d| d.ext.as_str()).collect();
        for ext in ["jpg", "mp4", "txt", "psd", "dwg", "dxf"] {
            assert!(classify_scanned_file(ext, &cat).is_some() && exts.contains(ext));
        }
        for ext in ["xyz", "exe", ""] {
            assert!(
                classify_scanned_file(ext, &cat).is_none(),
                "{ext} 不该被认作媒体"
            );
            assert!(!exts.contains(ext));
        }
    }

    #[test]
    fn exotic_source_carries_plugin_id_and_no_group() {
        let cat = two_offering_catalog();
        let merged = merged_formats(&cat);
        let by = |e: &str| merged.iter().find(|d| d.ext == e).unwrap().clone();

        let psd = by("psd");
        assert_eq!(psd.media_type, MediaType::Image);
        assert_eq!(
            psd.source,
            FormatSource::Exotic {
                plugin_id: "exotic-image-psd".into()
            }
        );
        // psd 是 exotic：common 侧必须仍返回 None（common-first 的前提）。
        assert_eq!(classify_media_type("psd"), None);

        // 同一 offering 的 dwg/dxf 大类相同，但**不共享 group** ——
        // offering 边界 ≠ UI alias group（D-007）。
        for ext in ["dwg", "dxf"] {
            let d = by(ext);
            assert_eq!(d.media_type, MediaType::Document);
            assert_eq!(d.group, None, "{ext} 不该从 offering 边界猜出 group");
            assert_eq!(
                d.source,
                FormatSource::Exotic {
                    plugin_id: "exotic-doc-cad".into()
                }
            );
        }
    }

    #[test]
    fn builtin_groups_survive_projection() {
        let merged = merged_formats(&CatalogSnapshot::empty());
        let by = |e: &str| merged.iter().find(|d| d.ext == e).unwrap().clone();
        for ext in ["jpg", "jpeg"] {
            assert_eq!(by(ext).group.as_deref(), Some(GROUP_JPEG));
            assert_eq!(by(ext).source, FormatSource::Builtin);
        }
        assert_eq!(by("cr2").group.as_deref(), Some(GROUP_RAW));
        assert_eq!(by("png").group, None);
    }

    #[test]
    fn merged_is_sorted_and_unique() {
        let cat = two_offering_catalog();
        let merged = merged_formats(&cat);
        let exts: Vec<&str> = merged.iter().map(|d| d.ext.as_str()).collect();
        let mut sorted = exts.clone();
        sorted.sort_unstable();
        assert_eq!(exts, sorted, "合并集未按 ext 升序");
        let uniq: HashSet<&str> = exts.iter().copied().collect();
        assert_eq!(uniq.len(), exts.len(), "合并集有重复 ext");
    }

    #[test]
    fn real_builtin_catalog_merges_clean() {
        // 生产 Catalog（当前 PSD + builtin OCR）也必须干净合并 —— 防 exotic-catalog.json 改坏。
        // builtin offering(如 exotic-ocr)不是文件格式,合并集基数按非 builtin 的 catalog 条目算。
        let cat = CatalogSnapshot::builtin().expect("内置 Catalog 应可解析");
        let merged = merged_formats(&cat);
        let non_builtin_catalog_count = cat.iter_formats().filter(|(_, off)| !off.builtin).count();
        assert_eq!(
            merged.len(),
            builtin_formats().len() + non_builtin_catalog_count
        );
        for d in &merged {
            assert_eq!(classify_scanned_file(&d.ext, &cat), Some(d.media_type));
        }
    }
}
