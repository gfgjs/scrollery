// src-tauri/src/enhance/registry.rs
//! 影像增强模型下载清单 + 安装态判定（照 `ai::ocr_registry` 式样，2026-07-24）。
//!
//! 文件名/落地路径**单源**取自 `scrollery_ai_core::enhance_profile::EnhanceProfile`
//! （每档 fp32 + fp16 两件，`{id}-fp32.onnx` / `{id}-fp16.onnx`）——本文件只负责
//! 「往哪下 + 校验多少」。
//!
//! # 资产源未定案（P0）
//! design.md §C/J-2:模型托管走用户自建公开仓（GitHub Releases / HuggingFace），托管
//! 自导出 ONNX + LICENSE/NOTICE。仓名定案前 URL 为占位、sha256/字节数留空——下载引擎
//! 契约「size_bytes=0 / sha256=None → 暂不校验」（`model_download`，plugin-store-map §2）。

use std::path::Path;

use crate::ai::profile::ModelAsset;
use scrollery_ai_core::enhance_profile::{find_enhance_profile, EnhanceProfile};

/// 占位资产源前缀（design.md §C）。
// TODO(批6): 用户 HuggingFace/GitHub 仓名定案后回填 URL/sha256/bytes（含 mirror_url）。
const PENDING_REPO_BASE: &str = "https://huggingface.co/PENDING_USER_REPO/resolve/main";

/// 某增强模型档位的完整下载清单（fp32 + fp16 两件）。
/// 文件名单源取自 `profile`；URL 暂用占位、sha256/bytes 暂空（清单待批 6 回填）。
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
            // TODO(批6): 占位 URL——仓名定案后回填真实直链 + mirror_url + sha256 + size_bytes。
            url: format!("{PENDING_REPO_BASE}/{file}"),
            mirror_url: None,
            dest: file.clone(),
            size_bytes: 0,
            sha256: None,
        })
        .collect()
}

/// 档位安装判定：`models_dir` 下 fp32 + fp16 两文件均存在且非空。
/// sha 深校验留给 worker `EnhanceSessionInit`（此处仅粗判「看起来已下载」，供设置页/门控展示）。
pub fn enhance_model_installed(models_dir: &Path, profile_id: &str) -> bool {
    find_enhance_profile(profile_id).is_some_and(|p| {
        [&p.file_fp32, &p.file_fp16].into_iter().all(|filename| {
            models_dir
                .join(filename)
                .metadata()
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrollery_ai_core::enhance_profile::enhance_profiles;
    use std::fs;

    #[test]
    fn assets_present_for_every_profile_two_files_each() {
        for p in enhance_profiles() {
            let assets = enhance_assets(&p.id).expect("已知 profile 必有清单");
            assert_eq!(assets.len(), 2, "每档恰 fp32 + fp16 两件");
            for a in &assets {
                assert!(a.url.starts_with("https://"));
                // 清单未定案:字节数 0 + sha256 None（下载引擎据此暂不校验）。
                assert_eq!(a.size_bytes, 0);
                assert!(a.sha256.is_none());
            }
        }
    }

    /// registry 文件名与 enhance_profile 单源一致（dest 恒等于 profile 的 fp32/fp16 名）。
    #[test]
    fn dest_matches_profile_filenames_single_source() {
        for p in enhance_profiles() {
            let assets = enhance_assets(&p.id).unwrap();
            let dests: Vec<&str> = assets.iter().map(|a| a.dest.as_str()).collect();
            assert_eq!(dests, vec![p.file_fp32.as_str(), p.file_fp16.as_str()]);
        }
    }

    #[test]
    fn unknown_profile_has_no_assets() {
        assert!(enhance_assets("nope").is_none());
    }

    #[test]
    fn placeholder_manifests_are_not_ready() {
        for p in enhance_profiles() {
            assert!(
                !enhance_manifest_ready(&p.id),
                "占位 URL / 零字节 / 空 sha256 不得开放下载:{}",
                p.id
            );
        }
    }

    #[test]
    fn readiness_requires_pinned_metadata() {
        let ready = ModelAsset {
            url: "https://example.invalid/model.onnx".to_string(),
            mirror_url: None,
            dest: "model.onnx".to_string(),
            size_bytes: 1,
            sha256: Some("a".repeat(64)),
        };
        assert!(manifest_is_ready(std::slice::from_ref(&ready)));

        let mut missing_hash = ready.clone();
        missing_hash.sha256 = None;
        assert!(!manifest_is_ready(&[missing_hash]));

        let mut placeholder = ready;
        placeholder.url = "https://example.invalid/PENDING_REPO/model.onnx".to_string();
        assert!(!manifest_is_ready(&[placeholder]));
    }

    #[test]
    fn model_installed_false_when_missing_true_when_present() {
        let p = &enhance_profiles()[0];
        let tmp = tempfile::tempdir().unwrap();
        assert!(!enhance_model_installed(tmp.path(), &p.id));
        for f in [&p.file_fp32, &p.file_fp16] {
            fs::write(tmp.path().join(f), b"x").unwrap();
        }
        assert!(enhance_model_installed(tmp.path(), &p.id));
    }

    #[test]
    fn model_installed_false_when_file_empty() {
        let p = &enhance_profiles()[0];
        let tmp = tempfile::tempdir().unwrap();
        for f in [&p.file_fp32, &p.file_fp16] {
            fs::write(tmp.path().join(f), b"").unwrap();
        }
        assert!(!enhance_model_installed(tmp.path(), &p.id));
    }
}
