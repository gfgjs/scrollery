// crates/scrollery-ai-core/src/enhance_profile.rs
//! 影像增强模型契约（`EnhanceProfile`）+ 内置注册表（纯数据，零 ort）。
//!
//! 仿 [`crate::ocr_profile`] 之法：把「换降噪/超分模型档位」变成 DATA 而非 CODE——
//! 文件名、任务归属、tile 几何全部来自 profile。
//!
//! # 单源约定
//! 5 模型短名单出自 `docs/planning/2026-07-24-降噪超分子系统/design.md` §D/§H
//! 定案；模型资产托管/sha256 钉定由 host 侧资产清单负责，本文件只管纯数据契约。
//!
//! # tile 静态 shape 联动契约（D-439）
//! `tile`/`tile_pad` 与导出 ONNX 时锁定的静态输入 shape（1×3×512×512，见 design.md
//! §E）是**联动契约**：改 `tile` 必须重导模型，否则 worker 侧 tiling 拼图与模型实际
//! 输入 shape 不符，会直接推理失败（非静默错误，但改值前务必确认已重导）。
//!
//! # 与 exotic-protocol 的关系
//! [`EnhanceTaskKind`] 与 `exotic_protocol::EnhanceTask` 语义一一对应，但本 crate
//! 不依赖 exotic-protocol（ai-core 定位是纯推理/契约层，不引协议 crate），故独立
//! 定义；两端的转换由各自调用方（worker/host）负责。

use serde::{Deserialize, Serialize};

/// 增强任务种类：降噪 / 去 JPEG 伪影 / 超分（design.md §D 三任务）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnhanceTaskKind {
    Denoise,
    DejpegArtifact,
    Upscale,
}

/// 模型辅助输入通道属性(除主 RGB 输入外,部分模型还需一路辅助输入)：
/// `SigmaMap` = DRUNet σ noise-level map(第 4 通道)、`QualityFactor` = FBCNN 第二输入(QF 标量)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnhanceAuxInput {
    None,
    SigmaMap,
    QualityFactor,
}

/// 单个增强模型的完整契约：任务归属 + 文件名 + tiling 几何。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhanceProfile {
    /// 稳定 id，如 `"realesrgan-x4plus"`。
    pub id: String,
    /// 该模型服务的任务种类。
    pub task: EnhanceTaskKind,
    /// fp32 权重文件名（相对 models 目录；CPU 用）。
    pub file_fp32: String,
    /// fp16 权重文件名（相对 models 目录；GPU 默认档，design.md §E）。
    pub file_fp16: String,
    /// 输出/输入尺度比：超分模型为 4，降噪/去伪影模型原样输出为 1。
    pub scale: u32,
    /// 静态 tile 边长（像素）；与导出 ONNX 的静态输入 shape 联动，见模块头 D-439。
    pub tile: u32,
    /// tile 重叠 padding（像素）；融合时每 tile 只取中心 `tile − 2×tile_pad` 区域拼接。
    pub tile_pad: u32,
    /// 模型辅助输入通道属性（SigmaMap = DRUNet σ 第 4 通道、QualityFactor = FBCNN 第二输入）。
    pub aux_input: EnhanceAuxInput,
    /// DirectML fp16 数值达标性（spike-D 实测）：`false` 表示该档 GPU fp16 会掉出 PSNR
    /// 门（SCUNet DML fp16 实测 37.48<40），host 侧即便走 GPU 也须加载 fp32 权重；
    /// `true` 表示 GPU 可安全用 fp16 档（design.md §E「GPU 默认 fp16」的例外收敛点）。
    pub fp16_safe: bool,
}

/// 五个候选增强模型档位；实际导出资产及分发许可须在发行清单落地时核验。
pub fn enhance_profiles() -> Vec<EnhanceProfile> {
    // tile/tile_pad 全部 512/16(design.md §E 定案),文件名 `{id}-fp32.onnx` / `{id}-fp16.onnx`。
    // fp16_safe:仅 SCUNet 为 false(spike-D 实测 DML fp16 PSNR 37.48<40 门,GPU 亦须 fp32);
    // 其余 4 档 fp16 达标(design.md §E)。
    [
        (
            "realesrgan-x4plus",
            EnhanceTaskKind::Upscale,
            4u32,
            EnhanceAuxInput::None,
            true,
        ),
        (
            "realesrgan-x4plus-anime-6b",
            EnhanceTaskKind::Upscale,
            4,
            EnhanceAuxInput::None,
            true,
        ),
        (
            "scunet",
            EnhanceTaskKind::Denoise,
            1,
            EnhanceAuxInput::None,
            false,
        ),
        (
            "drunet",
            EnhanceTaskKind::Denoise,
            1,
            EnhanceAuxInput::SigmaMap,
            true,
        ),
        (
            "fbcnn",
            EnhanceTaskKind::DejpegArtifact,
            1,
            EnhanceAuxInput::QualityFactor,
            true,
        ),
    ]
    .into_iter()
    .map(|(id, task, scale, aux_input, fp16_safe)| EnhanceProfile {
        id: id.to_string(),
        task,
        file_fp32: format!("{id}-fp32.onnx"),
        file_fp16: format!("{id}-fp16.onnx"),
        scale,
        tile: 512,
        tile_pad: 16,
        aux_input,
        fp16_safe,
    })
    .collect()
}

/// 按稳定 id 查增强模型 profile。
pub fn find_enhance_profile(id: &str) -> Option<EnhanceProfile> {
    enhance_profiles().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn five_profiles_present_with_expected_tasks() {
        let ps = enhance_profiles();
        assert_eq!(ps.len(), 5);
        assert_eq!(
            find_enhance_profile("realesrgan-x4plus").unwrap().task,
            EnhanceTaskKind::Upscale
        );
        assert_eq!(
            find_enhance_profile("scunet").unwrap().task,
            EnhanceTaskKind::Denoise
        );
        assert_eq!(
            find_enhance_profile("drunet").unwrap().task,
            EnhanceTaskKind::Denoise
        );
        assert_eq!(
            find_enhance_profile("fbcnn").unwrap().task,
            EnhanceTaskKind::DejpegArtifact
        );
        assert!(find_enhance_profile("nope").is_none());
    }

    #[test]
    fn ids_are_unique() {
        let ps = enhance_profiles();
        let ids: HashSet<_> = ps.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), ps.len(), "id 必须唯一,注册表不得重名");
    }

    #[test]
    fn scale_is_one_or_four() {
        for p in enhance_profiles() {
            assert!(
                p.scale == 1 || p.scale == 4,
                "scale 只应为 1(降噪/去伪影原样输出)或 4(超分),got {} for {}",
                p.scale,
                p.id
            );
        }
    }

    #[test]
    fn file_names_follow_id_suffix_invariant() {
        for p in enhance_profiles() {
            assert_eq!(p.file_fp32, format!("{}-fp32.onnx", p.id));
            assert_eq!(p.file_fp16, format!("{}-fp16.onnx", p.id));
            assert_eq!(p.tile, 512);
            assert_eq!(p.tile_pad, 16);
        }
    }

    #[test]
    fn fp16_safe_only_scunet_is_false() {
        // spike-D 裁决:SCUNet DML fp16 掉门 → fp16_safe=false(GPU 亦走 fp32);其余 4 档 true。
        assert!(!find_enhance_profile("scunet").unwrap().fp16_safe);
        for id in [
            "realesrgan-x4plus",
            "realesrgan-x4plus-anime-6b",
            "drunet",
            "fbcnn",
        ] {
            assert!(
                find_enhance_profile(id).unwrap().fp16_safe,
                "{id} 应为 fp16_safe=true"
            );
        }
    }

    #[test]
    fn aux_input_matches_model_contract() {
        // 三档:drunet=SigmaMap、fbcnn=QualityFactor、其余(scunet/两个 realesrgan)=None。
        assert_eq!(
            find_enhance_profile("drunet").unwrap().aux_input,
            EnhanceAuxInput::SigmaMap
        );
        assert_eq!(
            find_enhance_profile("fbcnn").unwrap().aux_input,
            EnhanceAuxInput::QualityFactor
        );
        for id in ["realesrgan-x4plus", "realesrgan-x4plus-anime-6b", "scunet"] {
            assert_eq!(
                find_enhance_profile(id).unwrap().aux_input,
                EnhanceAuxInput::None,
                "{id} 无辅助输入通道"
            );
        }
    }
}
