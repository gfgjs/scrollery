// src-tauri/src/ai/ocr_registry.rs
//! OCR 模型元数据 + 下载清单（T4；资产源已钉定，2026-07-23）。
//!
//! 主源 = RapidAI/RapidOCR ModelScope v3.9.2 resolve 直链（`docs/planning/2026-07-23-OCR文字提取/
//! model-assets.md` 定案）。[`PINNED_ASSETS`] 的 sha256/字节数均为本机对 ModelScope 服务器直接
//! 实测（HTTP Range 请求取 `Content-Range` 精确总字节数、`X-Linked-Etag` 响应头核对 sha256），
//! 非网页转述；六个 onnx 文件的 sha256 与 `default_models.yaml`（RapidOCR 官方清单）逐一核对一致。
//! `ppocrv5_dict.txt` 官方清单无 sha256 字段，本机下载后自算，并核实 mobile/server 两档字节级
//! 完全一致（74012 字节，18383 行）。镜像 = GreatV/oar-ocr GitHub Release v0.3.0（det/rec/dict
//! 三类有对应文件；cls 无镜像，两档 cls 下载失败仅能重试主源）。
//!
//! # 临时性（用户 2026-07-23 明示）
//! 本源为过渡方案，用户后续会切至自有仓库托管；届时只需替换 [`PINNED_ASSETS`] 表内的
//! `url`/`mirror_url` 字段（sha256/size_bytes 若仓库内容不变则无需重算），拼装/校验逻辑
//! 不必再动。
//!
//! 文件名/落地路径单源仍取自 `scrollery_ai_core::ocr_profile::OcrProfile`——本文件只负责
//! 「往哪下 + 校验多少」。

use std::path::Path;

use crate::ai::profile::ModelAsset;
use scrollery_ai_core::ocr_profile::OcrProfile;

/// 单个已钉定资产的元数据（主源 + 镜像 + 校验值，均本机实测，非转述）。
struct PinnedAsset {
    /// 落地文件名，对齐 `OcrProfile::{det,cls,rec,dict}_file`（单源见模块头）。
    filename: &'static str,
    url: &'static str,
    /// GitHub oar-ocr 镜像；cls 两档均无对应发布文件，留 `None`。
    mirror_url: Option<&'static str>,
    size_bytes: u64,
    sha256: &'static str,
}

/// 七件互异物理文件：mobile det/cls/rec + server det/cls/rec + 两档共用 dict。
const PINNED_ASSETS: &[PinnedAsset] = &[
    PinnedAsset {
        filename: "ch_PP-OCRv5_det_mobile.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx",
        mirror_url: Some("https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_mobile_det.onnx"),
        size_bytes: 4_819_576,
        sha256: "4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae",
    },
    PinnedAsset {
        filename: "ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/cls/ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx",
        mirror_url: None,
        size_bytes: 1_018_508,
        sha256: "54379ae5174d026780215fc748a7f31910dee36818e63d49e17dc598ecc82df7",
    },
    PinnedAsset {
        filename: "ch_PP-OCRv5_rec_mobile.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile.onnx",
        mirror_url: Some("https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_mobile_rec.onnx"),
        size_bytes: 16_631_306,
        sha256: "5825fc7ebf84ae7a412be049820b4d86d77620f204a041697b0494669b1742c5",
    },
    PinnedAsset {
        filename: "ch_PP-OCRv5_det_server.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/det/ch_PP-OCRv5_det_server.onnx",
        mirror_url: Some("https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_server_det.onnx"),
        size_bytes: 88_118_768,
        sha256: "0f8846b1d4bba223a2a2f9d9b44022fbc22cc019051a602b41a7fda9667e4cad",
    },
    PinnedAsset {
        filename: "ch_PP-LCNet_x1_0_textline_ori_cls_server.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/cls/ch_PP-LCNet_x1_0_textline_ori_cls_server.onnx",
        mirror_url: None,
        size_bytes: 6_776_876,
        sha256: "7d3c02ef6c7da8ae08b4347cc7695b2081aae68c325d64375724ecf39c99e743",
    },
    PinnedAsset {
        filename: "ch_PP-OCRv5_rec_server.onnx",
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5/rec/ch_PP-OCRv5_rec_server.onnx",
        mirror_url: Some("https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-ocrv5_server_rec.onnx"),
        size_bytes: 84_577_022,
        sha256: "e09385400eaaaef34ceff54aeb7c4f0f1fe014c27fa8b9905d4709b65746562a",
    },
    PinnedAsset {
        filename: "ppocrv5_dict.txt",
        // mobile/server 两档各自有一份同名文件，本机比对字节级完全一致（74012 字节），取 mobile 路径。
        url: "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/paddle/PP-OCRv5/rec/ch_PP-OCRv5_rec_mobile/ppocrv5_dict.txt",
        mirror_url: Some("https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/ppocrv5_dict.txt"),
        size_bytes: 74_012,
        sha256: "d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b",
    },
];

fn pinned(filename: &str) -> Option<&'static PinnedAsset> {
    PINNED_ASSETS.iter().find(|a| a.filename == filename)
}

/// 某个 OCR 档位的完整下载清单（det+cls+rec+dict 四件）。
/// 文件名/落地路径单源取自 `profile`；URL/大小/sha256 单源取自 [`PINNED_ASSETS`]。
/// 若某文件名在 `PINNED_ASSETS` 中查不到（profile 与本表定义drift，理论上不该发生），
/// 整体回 `None`——沿用既有 fail-closed 契约，下载命令据此回 `ocr_manifest_unready`。
pub fn ocr_assets(profile: &OcrProfile) -> Option<Vec<ModelAsset>> {
    let files = [
        &profile.det_file,
        &profile.cls_file,
        &profile.rec_file,
        &profile.dict_file,
    ];
    files
        .into_iter()
        .map(|filename| {
            let p = pinned(filename)?;
            Some(ModelAsset {
                url: p.url.to_string(),
                mirror_url: p.mirror_url.map(|m| m.to_string()),
                dest: filename.clone(),
                size_bytes: p.size_bytes,
                sha256: Some(p.sha256.to_string()),
            })
        })
        .collect()
}

/// 档位安装判定：`models_dir` 下四文件（det/cls/rec/dict）均存在且非空。
/// sha 深校验留给 worker `OcrSessionInit`（此处仅粗判「看起来已下载」，供设置页/门控展示）。
pub fn ocr_tier_installed(models_dir: &Path, profile: &OcrProfile) -> bool {
    [
        &profile.det_file,
        &profile.cls_file,
        &profile.rec_file,
        &profile.dict_file,
    ]
    .into_iter()
    .all(|filename| {
        models_dir
            .join(filename)
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrollery_ai_core::ocr_profile::{find_ocr_profile, ocr_profiles};
    use std::fs;

    #[test]
    fn ocr_assets_ready_for_both_tiers() {
        for profile in ocr_profiles() {
            let assets = ocr_assets(&profile).expect("pinned assets present for known profile");
            assert_eq!(assets.len(), 4);
            for a in &assets {
                assert!(a.url.starts_with("https://www.modelscope.cn/"));
                assert!(a.size_bytes > 0, "{} size must be pinned", a.dest);
                assert!(a.sha256.is_some(), "{} sha256 must be pinned", a.dest);
            }
        }
    }

    #[test]
    fn dest_matches_profile_filenames_and_tiers_differ() {
        let ps = ocr_profiles();
        let (mobile, server) = (&ps[0], &ps[1]);
        let m_assets = ocr_assets(mobile).expect("mobile pinned");
        let s_assets = ocr_assets(server).expect("server pinned");
        let m_dests: Vec<&str> = m_assets.iter().map(|a| a.dest.as_str()).collect();
        assert_eq!(
            m_dests,
            vec![
                mobile.det_file.as_str(),
                mobile.cls_file.as_str(),
                mobile.rec_file.as_str(),
                mobile.dict_file.as_str(),
            ]
        );

        // 两档仅 dict dest 相同(主线修正节),det/cls/rec 互异。
        assert_ne!(m_assets[0].dest, s_assets[0].dest, "det 两档互异");
        assert_ne!(m_assets[1].dest, s_assets[1].dest, "cls 两档互异(主线修正)");
        assert_ne!(m_assets[2].dest, s_assets[2].dest, "rec 两档互异");
        assert_eq!(m_assets[3].dest, s_assets[3].dest, "dict 两档共用");

        // cls 两档均无镜像(oar-ocr release 未打包 cls);det/rec/dict 均有镜像。
        assert!(m_assets[1].mirror_url.is_none(), "mobile cls 无镜像");
        assert!(s_assets[1].mirror_url.is_none(), "server cls 无镜像");
        assert!(m_assets[0].mirror_url.is_some(), "mobile det 有镜像");
        assert!(m_assets[2].mirror_url.is_some(), "mobile rec 有镜像");
        assert!(m_assets[3].mirror_url.is_some(), "dict 有镜像");
    }

    #[test]
    fn ocr_tier_installed_false_when_missing_and_true_when_present() {
        let profile = find_ocr_profile("pp-ocrv5-mobile").expect("profile exists");
        let tmp = tempfile::tempdir().expect("tmp dir");
        assert!(!ocr_tier_installed(tmp.path(), &profile));

        for filename in [
            &profile.det_file,
            &profile.cls_file,
            &profile.rec_file,
            &profile.dict_file,
        ] {
            fs::write(tmp.path().join(filename), b"x").expect("write stub");
        }
        assert!(ocr_tier_installed(tmp.path(), &profile));
    }

    #[test]
    fn ocr_tier_installed_false_when_file_empty() {
        let profile = find_ocr_profile("pp-ocrv5-mobile").expect("profile exists");
        let tmp = tempfile::tempdir().expect("tmp dir");
        for filename in [
            &profile.det_file,
            &profile.cls_file,
            &profile.rec_file,
            &profile.dict_file,
        ] {
            fs::write(tmp.path().join(filename), b"").expect("write empty stub");
        }
        assert!(!ocr_tier_installed(tmp.path(), &profile));
    }
}
