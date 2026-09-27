// src-tauri/src/enhance/registry.rs
//! 影像增强模型下载清单 + 安装态判定（照 `ai::ocr_registry` 式样，2026-07-24）。
//!
//! 文件名/落地路径**单源**取自 `scrollery_ai_core::enhance_profile::EnhanceProfile`
//! （每档 fp32 + fp16 两件，`{id}-fp32.onnx` / `{id}-fp16.onnx`）——本文件只负责
//! 「往哪下 + 校验多少」。
//!
//! 发行资产尚未提供（2026-09-22）：没有获准分发的 ONNX、导出记录、LICENSE/NOTICE
//! 和固定版本下载地址。空 URL/大小/哈希只表达缺失；下载、执行与安装态均关闭。

use std::path::Path;

use crate::ai::profile::ModelAsset;
use scrollery_ai_core::enhance_profile::{find_enhance_profile, EnhanceProfile};

/// 某增强模型档位的完整下载清单（fp32 + fp16 两件）。
/// 文件名单源取自 `profile`；资产尚未交付，URL/sha256/bytes 留空。
/// profile_id 不在注册表 → `None`（fail-closed，下载命令据此回 `enhance_model_missing`）。
pub fn enhance_assets(profile_id: &str) -> Option<Vec<ModelAsset>> {
    find_enhance_profile(profile_id).map(|p| profile_assets(&p))
}

/// 只有 URL、大小与 sha256 都已钉定时，清单才可暴露为可下载。
/// 占位清单仍保留文件名单源用途，但必须 fail-closed，避免向无效地址发真实网络请求。
pub fn enhance_manifest_ready(profile_id: &str) -> bool {
    enhance_assets(profile_id).is_some_and(|assets| manifest_is_ready(&assets))
}

fn manifest_is_ready(assets: &[ModelAsset]) -> bool {
    !assets.is_empty()
        && assets.iter().all(|asset| {
            asset.url.starts_with("https://")
                && !asset.url.contains("PENDING_")
                && asset.size_bytes > 0
                && asset.sha256.as_ref().is_some_and(|sha| {
                    sha.len() == 64
                        && sha
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                })
        })
}

fn profile_assets(p: &EnhanceProfile) -> Vec<ModelAsset> {
    [&p.file_fp32, &p.file_fp16]
        .into_iter()
        .map(|file| ModelAsset {
            // 只有核实导出契约、分发许可和实际字节后，才可填写固定版本清单。
            url: String::new(),
            mirror_url: None,
            dest: file.clone(),
            size_bytes: 0,
            sha256: None,
        })
        .collect()
}

/// 档位安装判定：发行清单就绪，两个普通文件均位于模型根内且大小匹配。
/// SHA-256 深校验由 worker 对照发行清单执行，状态查询不反复读取全部模型。
pub fn enhance_model_installed(models_dir: &Path, profile_id: &str) -> bool {
    enhance_assets(profile_id).is_some_and(|assets| assets_installed(models_dir, &assets))
}

fn assets_installed(models_dir: &Path, assets: &[ModelAsset]) -> bool {
    if !manifest_is_ready(assets) {
        return false;
    }
    let Ok(root) = models_dir.canonicalize() else {
        return false;
    };
    assets.iter().all(|asset| {
        let Ok(path) = models_dir.join(&asset.dest).canonicalize() else {
            return false;
        };
        path.starts_with(&root)
            && path
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() == asset.size_bytes)
    })
}
