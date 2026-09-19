// crates/scrollery-ai-core/src/clip.rs
//! Chinese-CLIP 推理：图像预处理与文本分词。
//!
//! 模型：eisneim/cn-clip_vit-b-16（FP16 外部数据格式，需要 ORT 1.26+）
//!
//! 图像编码器管线:RGB → Resize(shortest_edge=224, Bicubic) → CenterCrop(224)
//!   → Normalise(CLIP mean/std) → CHW f32 张量 → "image" 输入
//!   → "unnorm_image_features" [1,512] → L2 归一化 → 512 维单位向量
//!
//! 文本编码器管线:BERT 分词(vocab.txt,21128 tokens)→ token_ids i64[1,52]
//!   → "text" 输入 → "unnorm_text_features" [1,512] → L2 归一化 → 512 维单位向量
//!
//! # 踩坑记录 — 接入 CLIP 类多模态模型的经验总结
//!
//! 以下经验适用于所有 CLIP 变体（OpenAI CLIP / Chinese-CLIP / SigLIP / EVA-CLIP 等）
//! 和任何使用 ONNX Runtime 进行跨模态（图像-文本）语义搜索的场景。
//!
//! ## 坑1（致命）：vocab.txt 必须与模型的文本编码器严格匹配
//!
//! 这是我们遇到的**准确率低至 30% 的根因**。
//!
//! `eisneim/cn-clip_vit-b-16` 仓库附带的 `vocab.txt` 是 **英文 OpenAI CLIP**
//! 的 BPE 词表（~5594 tokens），而 Chinese-CLIP 的文本编码器是
//! `RoBERTa-wwm-ext-base-chinese`（= BERT），需要 `bert-base-chinese` 的
//! WordPiece 词表（**21128 tokens**）。
//!
//! 用错误的词表时，所有中文字符都被编码为 `[UNK]`：
//! ```text
//! 错误："小猫" → [CLS=2, UNK=1, UNK=1, SEP=3]
//!       "花朵" → [CLS=2, UNK=1, UNK=1, SEP=3]   // 完全相同！
//!       cosine_similarity = 1.0000                 // 无法区分任何查询
//!
//! 正确："小猫" → [CLS=101, 小=2207, 猫=4344, SEP=102]
//!       "花朵" → [CLS=101, 花=5709, 朵=3321, SEP=102]
//!       cosine_similarity = 0.8499                 // 合理的语义相似度
//! ```
//!
//! **检查方法**：加载 vocab 后检查 `vocab_size`，BERT 系列应 ≥ 20000。
//! **正确来源**：`OFA-Sys/chinese-clip-vit-base-patch16` 的 `vocab.txt`。
//! **通用规则**：永远从模型原始作者（OFA-Sys）获取 vocab，而不是第三方 ONNX 导出者。
//!
//! ## 坑2：图像预处理顺序不能搞反 — 先 Resize 后 CenterCrop
//!
//! CLIP 系列的标准预处理是：
//! ```text
//! 正确：Resize(shortest_edge=224, BICUBIC) → CenterCrop(224) → Normalize
//! 错误：CenterCrop(最大正方形)              → Resize(224)     → Normalize
//! ```
//!
//! 区别示例（1920×1080 照片）：
//! - 错误做法：先裁成 1080×1080（丢失左右各 420px），再缩到 224
//! - 正确做法：先缩到 398×224（保持比例），再裁成 224×224（只丢各 87px）
//!
//! 错误做法丢失大量语义信息，图像特征与训练时期望的特征偏离。
//!
//! **检查方法**：找一张宽幅全景照（如 16:9），分别用两种方式预处理后做推理，
//! 比较与 Python `cn_clip.clip.image_transform` 的输出差异。
//!
//! **通用规则**：查看模型仓库的 `preprocessor_config.json` 或 Python 推理代码，
//! 严格按照 `Resize → CenterCrop` 的顺序实现。
//!
//! ## 坑3：插值方法必须一致 — CatmullRom ≈ PIL.Image.BICUBIC
//!
//! CLIP 训练时使用 PIL 的 BICUBIC 插值。Rust `image` crate 中：
//! - `CatmullRom` ≈ Bicubic（推荐，与 PIL 最接近）
//! - `Lanczos3` = sinc 插值（高频响应不同，会产生微妙的特征偏移）
//!
//! **通用规则**：查看 `preprocessor_config.json` 的 `"resample"` 字段，
//! PIL resample 枚举：0=NEAREST, 1=LANCZOS, 2=BILINEAR, 3=BICUBIC。
//!
//! ## 坑4：tokenizers crate 不会自动添加 [CLS]/[SEP]
//!
//! 与 Python `transformers.BertTokenizer` 不同，Rust `tokenizers` crate 的
//! `encode(text, true)` 中 `true` 只是个提示，如果没有配置 PostProcessor，
//! 它不知道要添加什么特殊 token。
//!
//! **必须手动配置** `TemplateProcessing` 或 `BertProcessing`：
//! （示意伪代码，故标 `ignore` 不编译——省略了 import / builder 收尾，仅展示 API 形状）
//! ```ignore
//! TemplateProcessing::builder()
//!     .try_single("[CLS]:0 $A:0 [SEP]:0")
//!     .special_tokens(vec![("[CLS]", 101), ("[SEP]", 102)])
//! ```
//!
//! **检查方法**：编码 "小猫" 后打印 token IDs，确认首位是 101、末位是 102。
//!
//! ## 坑5：手动截断会丢失 [SEP] — 使用 TruncationParams
//!
//! 如果在 `encode()` 返回后手动 `ids[..MAX_SEQ_LEN]` 截断，
//! 超长文本的最后一个 token（[SEP]=102）会被截掉，破坏 BERT 的句边界语义。
//!
//! **正确做法**：配置 `tokenizer.with_truncation(Some(TruncationParams { ... }))`，
//! tokenizers crate 会智能截断内容 token 并保留 [CLS] 和 [SEP]。
//! 同时配置 `with_padding(Some(PaddingParams { ... }))` 让 crate 自动填充到固定长度。
//!
//! ## 坑6：ONNX 模型输出可能未归一化
//!
//! 部分导出的 ONNX 模型输出的是**未归一化**的原始特征向量（如 `unnorm_*`），
//! 必须在推理后手动 L2 归一化才能用于余弦相似度搜索。
//! 也有些模型（如旧版 cn-clip-vit-b16-*.onnx）输出已归一化的向量。
//!
//! **检查方法**：看输出 tensor 名称是否有 `unnorm` 前缀，
//! 或检查输出向量的 L2 范数是否 ≈ 1.0。
//!
//! # 踩坑记录 — 导出篇（PyTorch → ONNX，2026-06 导 ViT-L/14@336 时踩齐）
//!
//! 导出脚本见 `tools/export_clip_l14_336_onnx.py`（含逐条详注）、
//! 验证脚本 `tools/validate_clip_l14_336_onnx.py`。以下按出现顺序：
//!
//! ## 坑7（关键·决定精度）：cn-clip 的 BERT 文本编码器在 fp16 下数值塌缩
//!
//! fp16 文本编码器**能加载但算错**：注意力分数 / softmax / LayerNorm 在 fp16 下溢出，
//! 所有文本嵌入退化为几乎相同的向量。实测（ORT CPU）：fp16 输出 vs PyTorch(fp32) 余弦
//! 仅 ~0.2，且任意两条不同文本互相余弦 0.999+（完全无法区分）。这与近期 commit
//! 「文本编码器强制走 CPU（DirectML 静默算错文本模型）」是同一现象。
//! **对策**：ViT-L 走精确 **fp32**（profile `cn-clip-vit-l14-336` 即 `.fp32.onnx`）；
//! 轻量化用 B/16 fp16（eisneim 仓库直供、已验证可用）。
//! **通用规则**：fp16 验证**绝不能只看「能加载」**，必须与原始 PyTorch 输出逐条比对余弦 ≥0.99。
//!
//! ## 坑8：opset 必须 ≥17，否则 fp16 触发 ORT 的 LayerNorm 融合崩溃
//!
//! opset 14 把 LayerNorm 拆成 ReduceMean/Sqrt/Sub/Div/... 一堆原语；转 fp16 后
//! ORT 的 `SimplifiedLayerNormFusion` 想把它们重新融合，却被中间插入的 Cast 节点卡住，
//! 加载即报 `GetIndexFromName ... InsertedPrecisionFreeCast ... 不存在`。
//! opset 17 把 LayerNorm 导成单个 `LayerNormalization` 算子，无可重融合的原语 → 规避。
//! （SDPA 算子另需 opset≥14；torch 2.12 把注意力融合成 `scaled_dot_product_attention`。）
//!
//! ## 坑9：官方 cn_clip.deploy 的 fp16 入口已失效
//!
//! 官方脚本 `from onnxmltools.utils import convert_float_to_float16` 在新版 onnxmltools
//! 中已退化为「抛 NotImplementedError」的桩函数（`tools/convert_large_onnx.bat` 注释记录了
//! 这个「依赖地狱」）。**对策**：改用 `onnxruntime.transformers.float16.convert_float_to_float16`
//! （它能正确改写视觉模型里的 Cast 节点，`onnxconverter_common` 的同名函数则不能，会产生
//! 「conv1 输入 float / 权重 float16」类型冲突）。
//!
//! ## 坑10：torch 2.12 默认 dynamo 导出器 + Windows 大文件写盘
//!
//! - torch 2.12 默认 dynamo（torch.export）路径不能稳妥处理 `(None, text)` / `(image, None)`
//!   这种 None 占位入参，也可能不严格沿用 io 名 → 显式 `dynamo=False` 走旧 TorchScript 导出器。
//! - 旧导出器把 ~1.2GB 的 fp32 图**一次性写盘**，在 Windows 触发 `[Errno 22] Invalid argument`
//!   → 改为导出到内存 `io.BytesIO`，再用 onnx 的外部数据格式保存（按块写盘）。
//!
//! ## 坑11（隐蔽）：onnx 写外部数据是 append 模式，重导前必须删旧 `.extra_file`
//!
//! `onnx.save_model(save_as_external_data=True)` 写权重时以**追加模式 `'ab'`** 打开
//! `.extra_file`。若上次导出的 `.extra_file` 还在，新权重会被**追加**而非覆盖 →
//! 文件体积**正好翻倍**；更坑的是 header 偏移指向新追加段，模型**仍能正常加载、推理结果正确**，
//! 只是白白多占一倍磁盘/下载。仅删 `.onnx` 头不够，**必须连 `.extra_file` 一起删**。
//!
//! ## 关于动态 batch 轴
//!
//! 图像塔导出默认把 batch 轴设为动态（`dynamic_axes={"image": {0: "batch"}}`），
//! 区别于 eisneim B/16 把 batch 钉死为 1。动态轴下 `encode_image_batch` 会整批送入（强 GPU 提速）；
//! 钉死为 1 时则回退逐张推理（`image_input_fixed_batch` 探测）。两种导出本函数都能正确处理。
//!
//! ## 诊断技巧：写 Python 对比脚本
//!
//! 当搜索准确率不符合预期时，最有效的诊断方法是写一个 Python 脚本：
//! 1. 用 `tokenizers` Python 库（与 Rust crate 同源）加载 vocab 并编码
//! 2. 用 `onnxruntime` 加载同一个 ONNX 模型并推理
//! 3. 比较文本间的余弦相似度是否合理
//! 4. 关键判据：语义不同的文本（如"小猫"vs"花朵"）相似度不应该 > 0.95
//!
//! 如果 Python 也产出错误结果，说明是数据问题（vocab/模型文件）；
//! 如果 Python 正确但 Rust 不对，说明是代码问题（预处理/tokenizer 配置）。

use std::path::Path;

use image::DynamicImage;
use ndarray::Array4;

use ort::value::Tensor;

use tracing::debug;

use crate::decoded::DecodedImage;
use crate::error::{AiError, Result};
use crate::profile::{ModelProfile, TokenizerKind};

enum TextInputMode {
    Single(String),
    Bert {
        input_ids: String,
        attention_mask: String,
        token_type_ids: String,
    },
}

fn session_input_names(session: &ort::session::Session) -> Vec<String> {
    session
        .inputs()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

fn resolve_single_input_name(available: &[String], preferred: &str, role: &str) -> Result<String> {
    if available.iter().any(|name| name == preferred) {
        return Ok(preferred.to_string());
    }

    if available.len() == 1 {
        debug!(
            "AI {} input name fallback: profile='{}', session='{}' | AI {} 输入名回退",
            role, preferred, available[0], role
        );
        return Ok(available[0].clone());
    }

    Err(AiError::Internal(format!(
        "AI {role} input name mismatch: profile='{preferred}', session inputs={available:?}"
    )))
}

fn has_input(available: &[String], name: &str) -> bool {
    available.iter().any(|n| n == name)
}

fn resolve_text_input_mode(available: &[String], profile: &ModelProfile) -> Result<TextInputMode> {
    if profile.text_inputs.len() == 1 && has_input(available, &profile.text_inputs[0]) {
        return Ok(TextInputMode::Single(profile.text_inputs[0].clone()));
    }

    if profile
        .text_inputs
        .iter()
        .all(|name| has_input(available, name))
        && has_input(&profile.text_inputs, "input_ids")
        && has_input(&profile.text_inputs, "attention_mask")
        && has_input(&profile.text_inputs, "token_type_ids")
    {
        return Ok(TextInputMode::Bert {
            input_ids: "input_ids".to_string(),
            attention_mask: "attention_mask".to_string(),
            token_type_ids: "token_type_ids".to_string(),
        });
    }

    if available.len() == 1 {
        debug!(
            "AI text input name fallback: profile={:?}, session='{}' | AI 文本输入名回退",
            profile.text_inputs, available[0]
        );
        return Ok(TextInputMode::Single(available[0].clone()));
    }

    if has_input(available, "input_ids")
        && has_input(available, "attention_mask")
        && has_input(available, "token_type_ids")
    {
        return Ok(TextInputMode::Bert {
            input_ids: "input_ids".to_string(),
            attention_mask: "attention_mask".to_string(),
            token_type_ids: "token_type_ids".to_string(),
        });
    }

    Err(AiError::Internal(format!(
        "AI text input names mismatch: profile={:?}, session inputs={available:?}",
        profile.text_inputs
    )))
}

// ── Model contract ─────────────────────────────────────────────────────────────
// ── 模型契约 ─────────────────────────────────────────────────────────────────
// 历史上这里写死了 Chinese-CLIP ViT-B/16 的常量（IMG_SIZE=224 / EMBED_DIM=512 /
// MAX_SEQ_LEN=52 / MODEL_NAME / MEAN / STD）。现已全部迁移到 `ModelProfile`，由调用方
// 传入，使同族不同尺寸（B/16↔L/14↔L/14@336）乃至将来异构模型可切换。
// 默认 profile 保持历史几何/维度/归一化契约，张量 I/O 名则按当前 ONNX session 动态校验。

// ── Image encoding ────────────────────────────────────────────────────────────
// ── 图像编码 ────────────────────────────────────────────────────────────────

/// 将 JPEG/PNG 缩略图字节切片编码为 512-d 单位向量。
pub fn encode_image_bytes(
    session_pool: &crate::engine::SessionPool,
    image_bytes: &[u8],
    profile: &ModelProfile,
) -> Result<Vec<f32>> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|e| AiError::Internal(format!("Image decode failed | 图像解码失败: {e}")))?;

    encode_image(session_pool, &img, profile)
}

/// 将 `DynamicImage` 编码为 `embed_dim` 维单位向量。
pub fn encode_image(
    session_pool: &crate::engine::SessionPool,
    img: &DynamicImage,
    profile: &ModelProfile,
) -> Result<Vec<f32>> {
    let array = preprocess_image(img, profile);
    run_image_inference(session_pool, array, profile)
}

/// 将预解码的图像（RGBA u8 像素）编码为 512-d 单位向量。
///
/// The image should already be resized to `short_edge=224` by the `ImageEngine`.
/// This function only performs lightweight CenterCrop + Normalize + CHW conversion
/// on the small (~336×224) image before running CLIP inference.
///
/// 图像应已由 `ImageEngine` 缩放至 `short_edge=224`。
/// 本函数仅在小图（约 336×224）上执行轻量的 CenterCrop + 归一化 + CHW 转换，
/// 然后运行 CLIP 推理。
pub fn encode_image_from_decoded(
    session_pool: &crate::engine::SessionPool,
    decoded: &DecodedImage,
    profile: &ModelProfile,
) -> Result<Vec<f32>> {
    let array = preprocess_decoded(decoded, profile);
    run_image_inference(session_pool, array, profile)
}

/// 在单张预处理后的 [1,3,S,S] f32 张量上运行图像编码推理；委托给 `encode_image_batch`，使固定
/// batch 模型的补齐逻辑同样覆盖单图路径（否则固定 batch>1 的导出会拒绝 `[1,…]`）。
fn run_image_inference(
    session_pool: &crate::engine::SessionPool,
    array: Array4<f32>,
    profile: &ModelProfile,
) -> Result<Vec<f32>> {
    encode_image_batch(session_pool, array, profile)?
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Internal("Empty image inference output | 图像推理输出为空".into()))
}

/// 探测图像编码器 ONNX `image` 输入声明的 batch 维度。
/// batch 轴为**固定**大小 `k` 时返回 `Some(k)`；动态轴（`-1`/符号，接受任意批大小）返回 `None`。
fn image_input_fixed_batch(session: &ort::session::Session, image_input: &str) -> Option<usize> {
    let shape = session
        .inputs()
        .iter()
        .find(|i| i.name() == image_input)?
        .dtype()
        .tensor_shape()?;
    // ONNX 用 -1 表示动态维；非正值一律视为动态（不约束批大小）。
    match shape.first().copied() {
        Some(d) if d > 0 => Some(d as usize),
        _ => None,
    }
}

/// 装载期契约自检(2026-07-10 审查 K1 后半):用 ONNX 元数据在**激活时**即拒错配模型,
/// 不等运行期错切——输入须为 [N,3,S,S] 且静态 S==image_size,输出末维静态时须==embed_dim。
/// 动态轴(≤0/符号维)不约束;运行期形状断言(encode_image_batch 内)仍是最后防线。
pub(crate) fn verify_image_tower_contract(
    session: &ort::session::Session,
    profile: &ModelProfile,
) -> Result<()> {
    let input_names = session_input_names(session);
    let image_input = resolve_single_input_name(&input_names, &profile.image_input, "image")?;
    if let Some(shape) = session
        .inputs()
        .iter()
        .find(|i| i.name() == image_input)
        .and_then(|i| i.dtype().tensor_shape())
    {
        let dims: Vec<i64> = shape.iter().copied().collect();
        let side_ok = dims.len() == 4
            && dims[2..4]
                .iter()
                .all(|&d| d <= 0 || d == profile.image_size as i64);
        if !side_ok {
            return Err(AiError::Internal(format!(
                "图像塔输入形状 {dims:?} 与 profile 不符(期望 [N,3,{0},{0}]):模型文件与 \
                 profile 错配 | image tower input shape mismatch",
                profile.image_size
            )));
        }
    }
    if let Some(shape) = session
        .outputs()
        .first()
        .and_then(|o| o.dtype().tensor_shape())
    {
        if let Some(&last) = shape.last() {
            if last > 0 && last != profile.embed_dim as i64 {
                return Err(AiError::Internal(format!(
                    "图像塔输出末维 {last} 与 profile.embed_dim {} 不符:模型文件与 profile \
                     错配 | image tower output dim mismatch",
                    profile.embed_dim
                )));
            }
        }
    }
    Ok(())
}

/// 图像批编码分段计时(µs,只累加)。**不分配、不打印**:输出与开关都在调用方
/// (ai-worker `batch.rs` 的 opt-in 汇总行),本核保持纯推理层,不读环境变量。
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageBatchTiming {
    /// 从会话池取 session 的等待(池内串行时反映上一次占用未释放)。
    pub pool_wait_us: u64,
    /// 子批物化:切片 to_owned + 固定 batch 补齐 + Tensor::from_array。
    pub prep_us: u64,
    /// session.run 本体(含 DirectML 提交与同步等待)。
    pub run_us: u64,
    /// 输出取用:形状断言 + 逐项拷贝到 Vec + 归一化。
    pub extract_us: u64,
    /// session.run 调用次数;固定 batch=1 导出时 = 批内项数(逐张推理的直接证据)。
    pub runs: u32,
}

/// 可选分段计时器(单一实现路径):`None` 时**零采集**——测量点既不取时钟也不写值;
/// `Some` 时每个测量点取一次时钟并按选择器累加。带/不带计时共用同一段业务代码,只差
/// 一个 `Option` 分支,不为计时复制流水线(ai-worker 侧自定义计数结构亦复用本类型)。
pub struct StageClock<'a, T> {
    out: Option<&'a mut T>,
    t0: Option<std::time::Instant>,
}

impl<'a, T> StageClock<'a, T> {
    pub fn new(out: Option<&'a mut T>) -> Self {
        Self { out, t0: None }
    }

    /// 开始一段测量;关闭时为空操作。
    pub fn begin(&mut self) {
        self.t0 = self.out.is_some().then(std::time::Instant::now);
    }

    /// 结束当前段并按 `pick` 落账 µs;关闭时为空操作。
    pub fn end(&mut self, pick: fn(&mut T) -> &mut u64) {
        if let (Some(t0), Some(out)) = (self.t0.take(), self.out.as_deref_mut()) {
            *pick(out) += t0.elapsed().as_micros() as u64;
        }
    }

    /// 计数一次(如 session.run 次数、子批数);关闭时为空操作。
    pub fn count(&mut self, pick: fn(&mut T) -> &mut u32) {
        if let Some(out) = self.out.as_deref_mut() {
            *pick(out) += 1;
        }
    }
}

/// 在一批预处理后的张量上运行 CLIP 图像编码器推理(既有入口:不采集计时)。
pub fn encode_image_batch(
    session_pool: &crate::engine::SessionPool,
    batch_tensor: Array4<f32>,
    profile: &ModelProfile,
) -> Result<Vec<Vec<f32>>> {
    encode_image_batch_timed(session_pool, batch_tensor, profile, None)
}

/// 同 [`encode_image_batch`];`timing` 为 `Some` 时才采集分段耗时。
pub fn encode_image_batch_timed(
    session_pool: &crate::engine::SessionPool,
    batch_tensor: Array4<f32>,
    profile: &ModelProfile,
    timing: Option<&mut ImageBatchTiming>,
) -> Result<Vec<Vec<f32>>> {
    let s = profile.image_size as i64;
    let side = profile.image_size as usize;
    let dim = profile.embed_dim;
    let n_total = batch_tensor.shape()[0];

    let mut clock = StageClock::new(timing);
    clock.begin();
    let mut guard = session_pool
        .get()
        .ok_or_else(|| AiError::Internal("Session pool disconnected".into()))?;
    clock.end(|t| &mut t.pool_wait_us);
    let input_names = session_input_names(&guard);
    let image_input = resolve_single_input_name(&input_names, &profile.image_input, "image")?;

    // 探测模型声明的 batch 轴：
    // - 动态轴(-1/符号) → `None`：整批 [N,3,S,S] 一次送入（强 GPU 提速）。
    // - 固定为 k（eisneim B/16 即 1；新仓库 bN 导出为 N） → `Some(k)`：按 k 分块。
    //   **关键修复**：固定 batch 的模型要求每次喂入恰好 k 行——直接喂 cur≠k（尾批不足、或
    //   k≥N 时整批仍 <k）都会被 ORT 拒绝「Got invalid dimensions for input: image …」。
    //   故对不足 k 的块**补齐到 k**（复制末样本，避免全零的潜在数值问题），推理后只取前 cur 个
    //   输出、丢弃填充行。这样无论 dyn / 固定 batch 导出，推理都正确。
    let fixed = image_input_fixed_batch(&guard, &image_input);
    let chunk = fixed.unwrap_or(n_total).max(1);

    let mut results: Vec<Vec<f32>> = Vec::with_capacity(n_total);

    for start in (0..n_total).step_by(chunk) {
        let end = (start + chunk).min(n_total);
        let cur = end - start;
        clock.begin();

        // 物化连续的 [cur,3,S,S] 子批 —— 切片视图非拥有所有权的 Vec，而 `Tensor::from_array` 需要拥有的扁平数据。
        let sub = batch_tensor
            .slice(ndarray::s![start..end, .., .., ..])
            .to_owned();

        // 固定 batch 模型且本块不足 k → 补齐到 k（用最后一帧填充）；否则直接用 cur 行。
        let run_rows = match fixed {
            Some(k) if cur < k => k,
            _ => cur,
        };
        let tensor = if run_rows > cur {
            let mut padded = Array4::<f32>::zeros((run_rows, 3, side, side));
            padded
                .slice_mut(ndarray::s![0..cur, .., .., ..])
                .assign(&sub);
            let last = sub.slice(ndarray::s![cur - 1..cur, .., .., ..]).to_owned();
            for r in cur..run_rows {
                padded
                    .slice_mut(ndarray::s![r..r + 1, .., .., ..])
                    .assign(&last);
            }
            let shape: [i64; 4] = [run_rows as i64, 3, s, s];
            let (flat_data, _offset) = padded.into_raw_vec_and_offset();
            Tensor::from_array((shape, flat_data)).map_err(AiError::Ort)?
        } else {
            let shape: [i64; 4] = [cur as i64, 3, s, s];
            let (flat_data, _offset) = sub.into_raw_vec_and_offset();
            Tensor::from_array((shape, flat_data)).map_err(AiError::Ort)?
        };
        clock.end(|t| &mut t.prep_us);

        clock.begin();
        let outputs = guard
            .run(vec![(image_input.as_str(), tensor)])
            .map_err(AiError::Ort)?;
        clock.end(|t| &mut t.run_us);
        clock.count(|t| &mut t.runs);

        // Output: "unnorm_image_features" [run_rows, embed_dim]；只取前 cur 个（丢弃填充行输出）。
        clock.begin();
        let raw = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(AiError::Ort)?;
        let (out_shape, raw_slice) = raw;

        // 输出形状断言(2026-07-10 审查 K1,全链唯一有效防线):错配模型(如 768 维模型
        // 配 512 维 profile)时,按 profile.embed_dim 手工切行会错位串行,但**每个向量长度
        // 恰好合法**——worker 维度红线、host blob 对账、搜索缓存长度过滤全部骗过,垃圾向量
        // 批量入库零告警。总元素数也可能恰好整除(batch 是 2 的幂时 rows×768 必能按 512 切),
        // 故必须断言 ORT 回报的真实形状,而非任何长度推断。
        let dims: Vec<i64> = out_shape.iter().copied().collect();
        if dims.len() != 2 || dims[0] != run_rows as i64 || dims[1] != dim as i64 {
            return Err(AiError::Internal(format!(
                "图像塔输出形状 {dims:?} 与 profile 不符(期望 [{run_rows}, {dim}]):\
                 模型文件与激活 profile 错配?请核对导入/改名的 onnx 是否属于该架构 \
                 | image tower output shape mismatch, model file vs profile"
            )));
        }

        for i in 0..cur {
            let off = i * dim;
            let embedding: Vec<f32> = raw_slice
                .get(off..off + dim)
                .ok_or_else(|| AiError::Internal("Batch output tensor out of bounds".to_string()))?
                .to_vec();
            results.push(maybe_normalize(embedding, profile));
        }
        clock.end(|t| &mut t.extract_us);
    }

    Ok(results)
}

/// 将图像预处理为 [1, 3, 224, 224] f32 数组。
///
/// 流水线与官方 Chinese-CLIP (cn_clip) 一致：
///   RGB → 按短边缩放到224(Bicubic) → 中心裁剪224 → 归一化
///
/// 【关键修复】旧代码先 CenterCrop(最大正方形) 再 Resize(224, Lanczos3)，
/// 导致宽幅图像丢失大量语义内容（如 1920×1080 图片会先裁掉左右各 420px）。
/// 正确做法是先按短边等比缩放，再裁剪，最大限度保留图像内容。
pub fn preprocess_image(img: &DynamicImage, profile: &ModelProfile) -> Array4<f32> {
    let img_size = profile.image_size;
    let (mean, std) = (profile.mean, profile.std);

    // 1. 转换为 RGB
    let rgb = img.to_rgb8();

    // 2. 按短边缩放到 image_size，保持长宽比。
    //    使用 CatmullRom（= Bicubic 插值，匹配 PIL.Image.BICUBIC）。
    let (w, h) = (rgb.width(), rgb.height());
    let short_edge = w.min(h) as f32;
    let scale = img_size as f32 / short_edge;
    let new_w = (w as f32 * scale).round() as u32;
    let new_h = (h as f32 * scale).round() as u32;
    let resized =
        image::imageops::resize(&rgb, new_w, new_h, image::imageops::FilterType::CatmullRom);

    // 3. 中心裁剪到 image_size×image_size
    let cx = (resized.width() - img_size) / 2;
    let cy = (resized.height() - img_size) / 2;
    let cropped = image::imageops::crop_imm(&resized, cx, cy, img_size, img_size).to_image();

    // 4. HWC → CHW，使用 CLIP 均值/标准差归一化
    //    平坦 slice 写(T18.5):ndarray 逐元素索引在 dev 构建(opt-0)是热点;
    //    算术保持逐位一致(黄金向量/双后端对拍依赖确定性),仅改寻址方式。
    let side = img_size as usize;
    let mut tensor = Array4::<f32>::zeros((1, 3, side, side));
    {
        let dst = tensor.as_slice_mut().expect("零初始化张量必为标准布局");
        let raw = cropped.as_raw(); // RGB8 行主序,3 字节/像素,行宽恰为 side
        let plane = side * side;
        for y in 0..side {
            for x in 0..side {
                let src = (y * side + x) * 3;
                for c in 0..3usize {
                    let val = raw[src + c] as f32 / 255.0;
                    dst[c * plane + y * side + x] = (val - mean[c]) / std[c];
                }
            }
        }
    }

    tensor
}

/// 对预缩放的 `DecodedImage`（RGBA u8 像素）进行轻量预处理。
///
/// 图像预期 `短边 = 224`（如 336×224 或 224×224）。
/// 执行：CenterCrop(224×224) → RGBA→RGB → /255 → CLIP 归一化 → HWC→CHW。
pub fn preprocess_decoded(decoded: &DecodedImage, profile: &ModelProfile) -> Array4<f32> {
    let (mean, std) = (profile.mean, profile.std);
    let (w, h) = (decoded.width as usize, decoded.height as usize);
    let crop_size = profile.image_size as usize;

    // CenterCrop：计算偏移量（对于恰好 224 的图像饱和到 0）
    let cx = w.saturating_sub(crop_size) / 2;
    let cy = h.saturating_sub(crop_size) / 2;

    let mut tensor = Array4::<f32>::zeros((1, 3, crop_size, crop_size));
    {
        // 平坦 slice 写(T18.5,同 preprocess_image):仅改寻址,算术逐位不变;
        // 越界像素保持 0.0(与原实现的防御语义一致)。
        let dst = tensor.as_slice_mut().expect("零初始化张量必为标准布局");
        let px = &decoded.pixels;
        let plane = crop_size * crop_size;
        for y in 0..crop_size {
            for x in 0..crop_size {
                let src_x = cx + x;
                let src_y = cy + y;
                // RGBA 步幅:每像素 4 字节
                let idx = (src_y * w + src_x) * 4;
                if idx + 2 < px.len() {
                    for c in 0..3usize {
                        let val = px[idx + c] as f32 / 255.0;
                        dst[c * plane + y * crop_size + x] = (val - mean[c]) / std[c];
                    }
                }
            }
        }
    }

    tensor
}

// ── 文本编码 ─────────────────────────────────────────────────────────────────

/// Chinese-CLIP 的简易 BERT WordPiece 分词器。
pub struct ClipTokenizer {
    inner: tokenizers::Tokenizer,
}

impl ClipTokenizer {
    /// 按 `ModelProfile` 描述从模型目录加载分词器。
    ///
    /// 目前仅支持 BERT WordPiece（cn-clip 系列）；异构分词器（BPE/SentencePiece）留第二阶段
    /// —— 届时新增的 `TokenizerKind` 变体会让下方 `match` 非穷尽编译报错，强制补全实现。
    pub fn from_profile(models_dir: &Path, profile: &ModelProfile) -> Result<Self> {
        use tokenizers::models::wordpiece::WordPiece;
        use tokenizers::normalizers::bert::BertNormalizer;
        use tokenizers::pre_tokenizers::bert::BertPreTokenizer;
        use tokenizers::processors::template::TemplateProcessing;

        let (vocab_file, cls_id, sep_id, min_vocab) = match &profile.tokenizer {
            TokenizerKind::BertWordPiece {
                vocab_file,
                cls_id,
                sep_id,
                min_vocab,
            } => (vocab_file.as_str(), *cls_id, *sep_id, *min_vocab),
        };
        let vocab_path = models_dir.join(vocab_file);

        let wp = WordPiece::from_file(vocab_path.to_str().unwrap_or("vocab.txt"))
            .unk_token("[UNK]".to_string())
            .max_input_chars_per_word(100)
            .build()
            .map_err(|e| AiError::Tokenizer(format!("Tokenizer error: {e}")))?;

        let mut tokenizer = tokenizers::Tokenizer::new(wp);

        // ── Normalizer & PreTokenizer ───────────────────────────────────────
        // ── 归一化器与预分词器 ──────────────────────────────────────────────
        // Chinese-CLIP 使用 BERT 分词器,须做小写化与 CJK 字符切分处理。
        tokenizer.with_normalizer(Some(BertNormalizer::new(true, true, Some(true), true)));

        tokenizer.with_pre_tokenizer(Some(BertPreTokenizer));

        // ── 词表完整性检查 ──────────────────────────────────────────────────
        //
        // 【关键防护】Chinese-CLIP 使用 bert-base-chinese 词表（21128 个 token）。
        //   eisneim/cn-clip_vit-b-16 仓库附带的 vocab.txt 是英文 CLIP 的
        //   （约 5594 个 token），如果误用，所有中文字符都会变成 [UNK]，
        //   导致每个查询产生完全相同的嵌入向量，搜索准确率降为零。
        //   正确的 vocab.txt 应从 OFA-Sys/chinese-clip-vit-base-patch16 获取。
        let vocab_size = tokenizer.get_vocab_size(true);
        if vocab_size < min_vocab {
            tracing::error!(
                "vocab.txt has only {} tokens — expected ~21128 (bert-base-chinese). \
                 This is likely the wrong vocab file (English CLIP). \
                 Download the correct one from OFA-Sys/chinese-clip-vit-base-patch16. \
                 | vocab.txt 仅有 {} 个 token，预期约 21128。\
                 可能使用了错误的词表文件（英文 CLIP）。\
                 请从 OFA-Sys/chinese-clip-vit-base-patch16 下载正确的词表。",
                vocab_size,
                vocab_size
            );
            return Err(AiError::Tokenizer("Wrong vocab.txt".to_string()));
        }
        debug!(
            "Loaded vocab with {} tokens | 加载了 {} 个 token 的词表",
            vocab_size, vocab_size
        );

        // ── 后处理器：在序列前插入 [CLS]，末尾插入 [SEP] ──────────────────────
        //
        // 【关键修复·上轮】没有 TemplateProcessing，tokenizers crate 即使传
        //   add_special_tokens=true 也不知道应该添加什么 token。
        //   Chinese-CLIP 的 BERT 文本编码器依赖 [CLS]（id=101）的输出
        //   作为整句语义表示。缺少它会导致文本嵌入语义错乱，搜索准确率骤降。
        // [CLS]/[SEP] ids 来自 profile（cn-clip = 101/102）。
        let post_processor = TemplateProcessing::builder()
            .try_single("[CLS]:0 $A:0 [SEP]:0")
            .map_err(|e| AiError::Tokenizer(e.to_string()))?
            .try_pair("[CLS]:0 $A:0 [SEP]:0 $B:1 [SEP]:1")
            .map_err(|e| AiError::Tokenizer(e.to_string()))?
            .special_tokens(vec![
                (String::from("[CLS]"), cls_id),
                (String::from("[SEP]"), sep_id),
            ])
            .build()
            .map_err(|e| AiError::Tokenizer(e.to_string()))?;
        tokenizer.with_post_processor(Some(post_processor));

        // ── 截断配置：确保长文本也能保留 [SEP] ──────────────────────────────
        //
        // 【关键修复】旧代码在 encode() 中手动截断 raw_ids[..MAX_SEQ_LEN]，
        //   这会在超长文本时丢弃末尾的 [SEP]（id=102），破坏 BERT 的句边界语义。
        //   配置 Truncation 后，tokenizers crate 会自动在截断后保留 [SEP]。
        let truncation = tokenizers::TruncationParams {
            max_length: profile.max_seq_len,
            strategy: tokenizers::TruncationStrategy::LongestFirst,
            ..Default::default()
        };
        tokenizer
            .with_truncation(Some(truncation))
            .map_err(|e| AiError::Tokenizer(e.to_string()))?;

        // ── 填充：用 0（= [PAD]）填充到 MAX_SEQ_LEN ──────────────────────────
        tokenizer.with_padding(Some(tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::Fixed(profile.max_seq_len),
            pad_id: 0,
            pad_token: String::from("[PAD]"),
            ..Default::default()
        }));

        Ok(Self { inner: tokenizer })
    }

    /// 将文本分词，返回 (input_ids, attention_mask, token_type_ids) 的 i64 向量。
    ///
    /// 截断和填充由 tokenizers crate 通过 `from_vocab()` 中配置的
    /// TruncationParams 和 PaddingParams 自动处理。
    /// 这保证 [CLS] 和 [SEP] 始终存在，输出长度恰好为 MAX_SEQ_LEN。
    pub fn encode(&self, text: &str) -> Result<(Vec<i64>, Vec<i64>, Vec<i64>)> {
        let encoding = self
            .inner
            .encode(text, true)
            .map_err(|e| AiError::Tokenizer(format!("Tokenize failed: {e}")))?;

        // tokenizers crate 已截断到 MAX_SEQ_LEN 并用 [PAD](0) 填充。
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let types: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();

        Ok((ids, mask, types))
    }
}

/// 使用 CLIP 文本编码器将文本查询编码为 512-d 单位向量。
pub fn encode_text(
    session_pool: &crate::engine::SessionPool,
    tokenizer: &ClipTokenizer,
    text: &str,
    profile: &ModelProfile,
) -> Result<Vec<f32>> {
    debug!("Encoding text query | 正在编码文本查询: {:?}", text);

    let (ids, mask, types) = tokenizer.encode(text)?;

    // ── 诊断：打印 token IDs 用于调试 ───────────────────────────────────
    let non_pad: Vec<i64> = ids.iter().copied().filter(|&x| x != 0).collect();
    debug!(
        "Token IDs for {:?}: {:?} (total={}, non-pad={})",
        text,
        non_pad,
        ids.len(),
        non_pad.len()
    );

    let mut guard = session_pool
        .get()
        .ok_or_else(|| AiError::Internal("Session pool disconnected".into()))?;
    let input_names = session_input_names(&guard);
    let text_input_mode = resolve_text_input_mode(&input_names, profile)?;

    let shape = [1i64, profile.max_seq_len as i64];
    let outputs = match text_input_mode {
        TextInputMode::Single(text_input) => {
            let text = Tensor::from_array((shape, ids)).map_err(AiError::Ort)?;
            guard.run(vec![(text_input, text)]).map_err(AiError::Ort)?
        }
        TextInputMode::Bert {
            input_ids,
            attention_mask,
            token_type_ids,
        } => {
            let input_ids_tensor = Tensor::from_array((shape, ids)).map_err(AiError::Ort)?;
            let attention_mask_tensor = Tensor::from_array((shape, mask)).map_err(AiError::Ort)?;
            let token_type_ids_tensor = Tensor::from_array((shape, types)).map_err(AiError::Ort)?;
            guard
                .run(vec![
                    (input_ids, input_ids_tensor),
                    (attention_mask, attention_mask_tensor),
                    (token_type_ids, token_type_ids_tensor),
                ])
                .map_err(AiError::Ort)?
        }
    };

    // 输出:"text_features" [1, 512]
    let raw = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(AiError::Ort)?;

    let (_shape, raw_slice) = raw;
    let embedding: Vec<f32> = raw_slice.to_vec();
    Ok(maybe_normalize(embedding, profile))
}

// ── 向量工具函数 ──────────────────────────────────────────────────────────────

/// 仅当模型输出非单位向量时才做 L2（由 profile 决定）。余弦搜索要求单位向量；
/// cn-clip 输出 `unnorm_*` → 需归一化。
fn maybe_normalize(v: Vec<f32>, profile: &ModelProfile) -> Vec<f32> {
    if profile.output_normalized {
        v
    } else {
        l2_normalize(v)
    }
}

// 纯字节序/归一化工具已外移至 crate::embedding(T16 准备:不在 inference 门内,
// host 删 ort 后仍可用);此处原位再导出保持既有 `clip::*` 引用路径不变。
pub use crate::embedding::{bytes_to_embedding, embedding_to_bytes, l2_normalize};
