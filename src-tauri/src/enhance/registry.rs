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
                assert!(a.url.is_empty());
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
    fn unpinned_local_files_do_not_count_as_installed() {
        let p = &enhance_profiles()[0];
        let tmp = tempfile::tempdir().unwrap();
        assert!(!enhance_model_installed(tmp.path(), &p.id));
        for f in [&p.file_fp32, &p.file_fp16] {
            fs::write(tmp.path().join(f), b"x").unwrap();
        }
        assert!(!enhance_model_installed(tmp.path(), &p.id));
    }

    #[test]
    fn pinned_assets_require_regular_files_of_expected_size() {
        let tmp = tempfile::tempdir().unwrap();
        let asset = ModelAsset {
            url: "https://example.invalid/model.onnx".into(),
            mirror_url: None,
            dest: "model.onnx".into(),
            size_bytes: 2,
            sha256: Some("a".repeat(64)),
        };
        let assets = [asset];
        assert!(!assets_installed(tmp.path(), &assets));
        fs::create_dir(tmp.path().join("model.onnx")).unwrap();
        assert!(!assets_installed(tmp.path(), &assets));
        fs::remove_dir(tmp.path().join("model.onnx")).unwrap();
        fs::write(tmp.path().join("model.onnx"), b"x").unwrap();
        assert!(!assets_installed(tmp.path(), &assets));
        fs::write(tmp.path().join("model.onnx"), b"xx").unwrap();
        assert!(assets_installed(tmp.path(), &assets));
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
