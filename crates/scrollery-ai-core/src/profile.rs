// crates/scrollery-ai-core/src/profile.rs
//! AI 模型契约（`ModelProfile`）+ 内置模型注册表。
//! 把推理路径与具体模型解耦：图像尺寸、嵌入维度、归一化、张量 I/O 名、分词器类型、
//! 下载资产全部来自 profile 而非写死常量 —— 使「换模型」变成换数据而非改代码。
//!
//! # 不变量（务必保持）
//! - `id` **同时是 `ai_embeddings.model_name` 主键**：换 id = 换一套向量空间，不同模型
//!   的向量不可互比，切换后需对缺该模型向量的项重新分析（见 `sync_ai_status_for_model`）。
//! - 默认 profile（`cn-clip-vit-b16`）必须保持嵌入维度、预处理和归一化契约稳定；
//!   张量 I/O 名跟随当前 ONNX 导出，并由运行时按实际 session 输入兜底。

use serde::{Deserialize, Serialize};

/// 文本编码器所需的分词器后端。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenizerKind {
    /// BERT WordPiece（Chinese-CLIP / bert-base-chinese）。需 vocab.txt，自动插 [CLS]/[SEP]。
    BertWordPiece {
        vocab_file: String,
        cls_id: u32,
        sep_id: u32,
        /// 词表完整性下限：低于此值判定为错误词表（如英文 BPE）→ 立即报错。
        min_vocab: usize,
    },
    // 第二阶段（异构模型）预留：Bpe { merges_file, vocab_file }、SentencePiece { model_file } …
}

/// 一个可下载资产（模型权重主文件 / 外部权重 / 词表）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelAsset {
    /// 主下载地址（HuggingFace 直链等）。
    pub url: String,
    /// 国内镜像（可空；hf-mirror.com / ModelScope）。
    #[serde(default)]
    pub mirror_url: Option<String>,
    /// 落地文件名（相对 models 目录）。
    pub dest: String,
    /// 预期字节数（进度/粗校验；0=未知）。
    #[serde(default)]
    pub size_bytes: u64,
    /// 预期 sha256（小写 hex；None=暂不校验，待清单完善）。
    #[serde(default)]
    pub sha256: Option<String>,
}

/// 完整模型契约：推理路径所需的一切 + 目录元数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    /// 稳定 id；**也是 `ai_embeddings.model_name` 主键**。
    pub id: String,
    pub display_name: String,
    pub description: String,

    // ── 文件（相对 models 目录）─────────────────────────────────────────────
    pub image_file: String,
    pub text_file: String,

    // ── 几何 / 预处理 ───────────────────────────────────────────────────────
    pub image_size: u32,
    pub embed_dim: usize,
    pub max_seq_len: usize,
    pub mean: [f32; 3],
    pub std: [f32; 3],

    // ── 张量 I/O ────────────────────────────────────────────────────────────
    // NOTE: 当前内置下载清单使用 eisneim/cn-clip 新 ONNX 导出，张量名为 image/text。
    // 推理路径仍会按实际 session 输入名兜底，以兼容旧导出（pixel_values + BERT 三输入）。
    pub image_input: String,
    pub text_inputs: Vec<String>,
    /// 模型输出是否已 L2 归一化；false → 推理后手动归一化（cn-clip 的 `unnorm_*` 输出）。
    pub output_normalized: bool,

    // ── 分词器 ──────────────────────────────────────────────────────────────
    pub tokenizer: TokenizerKind,

    // ── 目录 / 许可 / 下载 ──────────────────────────────────────────────────
    pub languages: Vec<String>,
    pub license: String,
    /// 是否允许商业使用；商业发行版按构建渠道过滤掉 `false` 的条目。
    pub commercial_ok: bool,
    /// 体积提示（MB，UI 展示；0=未知）。
    pub size_mb: u32,
    /// 下载资产；**空 = 仅支持手动导入**（尚无现成 ONNX 托管，待 Layer B 填充已校验直链）。
    pub assets: Vec<ModelAsset>,
}

/// 默认（且历史）profile id —— 其嵌入向量须永久有效。
pub const DEFAULT_PROFILE_ID: &str = "cn-clip-vit-b16";

const VOCAB_FILE: &str = "vocab.txt";

/// CLIP/Chinese-CLIP 标准 ImageNet 归一化（全族共用）。
const CLIP_MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
const CLIP_STD: [f32; 3] = [0.26862954, 0.261_302_6, 0.275_777_1];

/// 所有 Chinese-CLIP 尺寸共用的 bert-base-chinese WordPiece 分词器规格。
fn cn_bert() -> TokenizerKind {
    TokenizerKind::BertWordPiece {
        vocab_file: VOCAB_FILE.to_string(),
        cls_id: 101,
        sep_id: 102,
        min_vocab: 10_000,
    }
}

/// 构造托管在 eisneim/cn-clip_vit-b-16 HF 仓库的下载资产（主源 + hf-mirror 镜像）。
fn cn_clip_b16_asset(file: &str, size_bytes: u64, sha256: &str) -> ModelAsset {
    const REPO: &str = "eisneim/cn-clip_vit-b-16";
    ModelAsset {
        url: format!("https://huggingface.co/{REPO}/resolve/main/{file}"),
        mirror_url: Some(format!("https://hf-mirror.com/{REPO}/resolve/main/{file}")),
        dest: file.to_string(),
        size_bytes,
        sha256: Some(sha256.to_string()),
    }
}

/// 全族共用的 vocab.txt（OFA-Sys bert-base-chinese，21128 token）。各 onnx 仓库都不含 vocab，
/// 且绝不可用 eisneim 仓库的英文 BPE 词表（见 clip.rs 坑1/坑8）。体积小、非 LFS → 仅按大小校验，
/// 运行时再校验 vocab_size ≥ min_vocab 双保险。
pub fn vocab_asset() -> ModelAsset {
    ModelAsset {
        url: "https://huggingface.co/OFA-Sys/chinese-clip-vit-base-patch16/resolve/main/vocab.txt"
            .to_string(),
        mirror_url: Some(
            "https://hf-mirror.com/OFA-Sys/chinese-clip-vit-base-patch16/resolve/main/vocab.txt"
                .to_string(),
        ),
        dest: VOCAB_FILE.to_string(),
        size_bytes: 109_540,
        sha256: None,
    }
}

/// 每个架构的静态元数据（文件清单里没有：几何尺寸、嵌入维度、分词器/词表、归一化），以稳定的
/// 架构 id（= `ai_embeddings.model_name`）为键。可下载的图像编码器 batch 变体及其大小/sha256
/// 由运行时动态发现（`remote_registry`）；这里只放固定不变的部分。
#[derive(Clone, Debug)]
pub struct ArchMeta {
    /// 稳定架构 id = 向量空间主键。
    pub id: &'static str,
    /// 新仓库（gficcg/clip_cn_vit-onnx）中的文件夹名；`None` = 静态条目（fp16 B/16，托管在 eisneim）。
    pub folder: Option<&'static str>,
    pub display_name: &'static str,
    pub description: &'static str,
    pub image_size: u32,
    pub embed_dim: usize,
    /// 文本塔落地文件名（同架构所有 batch 变体共用一份文本塔）。
    pub text_file: &'static str,
    /// 缺省图像 onnx（未显式选择变体时加载的文件；动态架构取 dyn 变体，静态取 fp16）。
    pub default_image_file: &'static str,
    /// UI 体积提示（MB）。
    pub size_mb: u32,
    /// 是否 fp16（仅用于展示与区分两个 B/16）。
    pub fp16: bool,
}

/// 所有已知架构（第一条 = 默认）。h-14 预留登记；仅当仓库真有其 onnx 时才会出现在模型库
/// （发现不到变体即隐藏）。
pub fn arch_metas() -> Vec<ArchMeta> {
    vec![
        ArchMeta {
            id: DEFAULT_PROFILE_ID, // "cn-clip-vit-b16"
            folder: None,
            display_name: "Chinese-CLIP ViT-B/16 (fp16)",
            description: "中英双语 · 轻量 · 推荐默认。512 维 / 224px，约 370MB。",
            image_size: 224,
            embed_dim: 512,
            text_file: "vit-b-16.txt.fp16.onnx",
            default_image_file: "vit-b-16.img.fp16.onnx",
            size_mb: 370,
            fp16: true,
        },
        ArchMeta {
            id: "cn-clip-vit-b16-fp32",
            folder: Some("clip_cn_vit-b-16"),
            display_name: "Chinese-CLIP ViT-B/16 (fp32)",
            description: "中英双语 · 轻量 · fp32 更精确（体积更大）。512 维 / 224px。",
            image_size: 224,
            embed_dim: 512,
            text_file: "vit-b-16.txt.fp32.onnx",
            default_image_file: "vit-b-16.img.dyn.fp32.onnx",
            size_mb: 720,
            fp16: false,
        },
        ArchMeta {
            id: "cn-clip-vit-l14",
            folder: Some("clip_cn_vit-l-14"),
            description: "中英双语 · 高精度。768 维 / 224px（fp32）。需更多显存/内存。",
            display_name: "Chinese-CLIP ViT-L/14",
            image_size: 224,
            embed_dim: 768,
            text_file: "vit-l-14.txt.fp32.onnx",
            default_image_file: "vit-l-14.img.dyn.fp32.onnx",
            size_mb: 1550,
            fp16: false,
        },
        ArchMeta {
            id: "cn-clip-vit-l14-336",
            folder: Some("clip_cn_vit-l-14-336"),
            display_name: "Chinese-CLIP ViT-L/14@336",
            description: "中英双语 · 最高精度 · 最慢。768 维 / 336px。仅推荐强独显。",
            image_size: 336,
            embed_dim: 768,
            text_file: "vit-l-14-336.txt.fp32.onnx",
            default_image_file: "vit-l-14-336.img.dyn.fp32.onnx",
            size_mb: 1550,
            fp16: false,
        },
        ArchMeta {
            id: "cn-clip-vit-h14",
            folder: Some("clip_cn_vit-h-14"),
            display_name: "Chinese-CLIP ViT-H/14",
            description: "中英双语 · 超高精度。1024 维 / 224px（fp32）。需大显存。",
            image_size: 224,
            embed_dim: 1024,
            text_file: "vit-h-14.txt.fp32.onnx",
            default_image_file: "vit-h-14.img.dyn.fp32.onnx",
            size_mb: 3000,
            fp16: false,
        },
    ]
}

/// 按稳定 id 查架构元数据。
pub fn arch_by_id(id: &str) -> Option<ArchMeta> {
    arch_metas().into_iter().find(|a| a.id == id)
}

/// 按仓库文件夹名查架构元数据。
pub fn arch_by_folder(folder: &str) -> Option<ArchMeta> {
    arch_metas().into_iter().find(|a| a.folder == Some(folder))
}

/// 为某架构合成完整 `ModelProfile`，`image_file` 指向选定的 batch 变体（`None` 用架构缺省）。
/// cn-clip 各尺寸共用分词器/张量名/归一化，仅按架构变化的字段不同。`assets` 留空 —— 下载在
/// `ai_commands` 里按变体单独构造清单。
pub fn resolve_profile(arch_id: &str, image_file: Option<&str>) -> Option<ModelProfile> {
    let m = arch_by_id(arch_id)?;
    let image_file = image_file.unwrap_or(m.default_image_file).to_string();
    Some(ModelProfile {
        id: m.id.to_string(),
        display_name: m.display_name.to_string(),
        description: m.description.to_string(),
        image_file,
        text_file: m.text_file.to_string(),
        image_size: m.image_size,
        embed_dim: m.embed_dim,
        max_seq_len: 52,
        mean: CLIP_MEAN,
        std: CLIP_STD,
        image_input: "image".to_string(),
        text_inputs: vec!["text".to_string()],
        output_normalized: false,
        tokenizer: cn_bert(),
        languages: vec!["zh".to_string(), "en".to_string()],
        license: "MIT".to_string(),
        commercial_ok: true,
        size_mb: m.size_mb,
        assets: Vec::new(),
    })
}

/// 按 id 查 profile（架构缺省变体）。供只需架构级契约的调用方；激活模型经 `resolve_profile` 解析所选变体。
pub fn find(id: &str) -> Option<ModelProfile> {
    resolve_profile(id, None)
}

/// 默认 profile（始终存在）。
pub fn default_profile() -> ModelProfile {
    resolve_profile(DEFAULT_PROFILE_ID, None)
        .expect("default profile must exist | 默认 profile 必须存在")
}

/// 静态 fp16 B/16 的已校验下载清单（托管在 eisneim，非新仓库）。
///
/// 已校验（2026-06-16 核对 HF 仓库实际文件/大小/LFS sha256）：FP16 **外部数据格式**——小 `.onnx`
/// 头 + 同名 `.extra_file` 权重，须同目录共存；vocab.txt 取自 OFA-Sys（绝不可用 eisneim 的英文词表，
/// 见 clip.rs 坑1/坑8）。
pub fn static_fp16_b16_assets() -> Vec<ModelAsset> {
    vec![
        cn_clip_b16_asset(
            "vit-b-16.img.fp16.onnx",
            3_770_126,
            "2a26f4fa948071b8fac2f8e63c87d8f798238e5e618eeee55621fdaf166541c5",
        ),
        cn_clip_b16_asset(
            "vit-b-16.img.fp16.onnx.extra_file",
            172_386_816,
            "41ca7f726a18f3dcd678178c5fe4effbb6036fafd6743742112d9ee797ea54cf",
        ),
        cn_clip_b16_asset(
            "vit-b-16.txt.fp16.onnx",
            2_284_232,
            "31cde86ac026826e46b65a2cac931ccc9d387f53a881c776b3bb15e5b2460cd5",
        ),
        cn_clip_b16_asset(
            "vit-b-16.txt.fp16.onnx.extra_file",
            204_140_544,
            "42cafb2217d7cba53b23f51ff4deacad4a322c120c4e32b5a4630dd5c83767a2",
        ),
        vocab_asset(),
    ]
}
