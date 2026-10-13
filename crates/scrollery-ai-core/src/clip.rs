// crates/scrollery-ai-core/src/clip.rs
//! Chinese-CLIP 推理：图像预处理与文本分词。
//!
//! 图像编码器按 profile 的 `image_size` 缩放、中心裁剪并归一化，再输出单位向量。
//! 文本编码器按 profile 的词表、序列长度和输入名分词与推理。
//!
//! 词表必须与文本塔匹配：Chinese-CLIP 使用 BERT WordPiece，错误词表会使中文查询退化为 [UNK]。
//! 加载时按 profile 的最小词表规模校验；TemplateProcessing 添加 [CLS]/[SEP]，
//! TruncationParams 保留句末 [SEP]，PaddingParams 补齐到 profile 的序列长度。
//! 图像预处理按短边缩放后中心裁剪，CatmullRom 对齐训练所用的 bicubic。
//! 未归一化的模型输出按 profile 再做 L2；固定与动态 batch 均由输入形状决定派发方式。

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

/// 将预缩放的 RGBA 图像编码为 profile 指定维度的单位向量。
/// 本函数按 `profile.image_size` 中心裁剪、归一化并转换为 CHW 张量。
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

/// 读取已装载图像编码器的真实 batch 轴；动态轴返回 `None`。
///
/// 调度方在装载时保存此值，避免固定大批模型被拆成小批后反复填充。
pub fn fixed_image_batch(
    pool: &crate::engine::SessionPool,
    profile: &ModelProfile,
) -> Result<Option<usize>> {
    let guard = pool
        .get()
        .ok_or_else(|| AiError::Internal("Session pool disconnected".into()))?;
    let names = session_input_names(&guard);
    let input = resolve_single_input_name(&names, &profile.image_input, "image")?;
    Ok(image_input_fixed_batch(&guard, &input))
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

/// 生成连续推理输入；固定尾批沿用末样本填充，不改输入数值。
fn image_row_data(
    batch: &mut Option<Array4<f32>>,
    start: usize,
    end: usize,
    run_rows: usize,
) -> Vec<f32> {
    let original = batch.as_ref().expect("输入张量在分块完成前保留");
    if start == 0 && end == original.shape()[0] && run_rows == end && original.is_standard_layout()
    {
        // 完整批直接交出已有缓冲区；带偏移的拥有切片仍只保留它的有效元素。
        let len = original.len();
        let (mut data, offset) = batch
            .take()
            .expect("完整批只消费一次")
            .into_raw_vec_and_offset();
        if let Some(offset) = offset.filter(|offset| *offset != 0) {
            data.drain(..offset);
        }
        data.truncate(len);
        return data;
    }
    let sub = batch
        .as_ref()
        .expect("输入张量在分块完成前保留")
        .slice(ndarray::s![start..end, .., .., ..])
        .to_owned();
    let cur = end - start;
    if run_rows > cur {
        let (_, channels, height, width) = sub.dim();
        let mut padded = Array4::<f32>::zeros((run_rows, channels, height, width));
        padded
            .slice_mut(ndarray::s![0..cur, .., .., ..])
            .assign(&sub);
        let last = sub.slice(ndarray::s![cur - 1..cur, .., .., ..]);
        for r in cur..run_rows {
            padded
                .slice_mut(ndarray::s![r..r + 1, .., .., ..])
                .assign(&last);
        }
        padded.into_raw_vec_and_offset().0
    } else {
        sub.into_raw_vec_and_offset().0
    }
}

/// 同 [`encode_image_batch`];`timing` 为 `Some` 时才采集分段耗时。
pub fn encode_image_batch_timed(
    session_pool: &crate::engine::SessionPool,
    batch_tensor: Array4<f32>,
    profile: &ModelProfile,
    timing: Option<&mut ImageBatchTiming>,
) -> Result<Vec<Vec<f32>>> {
    let s = profile.image_size as i64;
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
    let mut batch_tensor = Some(batch_tensor);

    let mut results: Vec<Vec<f32>> = Vec::with_capacity(n_total);

    for start in (0..n_total).step_by(chunk) {
        let end = (start + chunk).min(n_total);
        let cur = end - start;
        clock.begin();

        // 固定 batch 模型且本块不足 k → 补齐到 k（用最后一帧填充）；否则直接用 cur 行。
        let run_rows = match fixed {
            Some(k) if cur < k => k,
            _ => cur,
        };
        let shape: [i64; 4] = [run_rows as i64, 3, s, s];
        let flat_data = image_row_data(&mut batch_tensor, start, end, run_rows);
        let tensor = Tensor::from_array((shape, flat_data)).map_err(AiError::Ort)?;
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

/// 将图像预处理为 `[1, 3, image_size, image_size]` f32 数组。
///
/// RGB → 按短边等比缩放至 `profile.image_size` → 中心裁剪 → 归一化。
/// 先缩放再裁剪，避免宽幅图像在缩放前丢失过多内容。
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
    //    平坦 slice 写：ndarray 逐元素索引在 dev 构建(opt-0)是热点；
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
/// 图像预期已按 profile 缩放；执行中心裁剪至 `profile.image_size`、归一化与 CHW 转换。
pub fn preprocess_decoded(decoded: &DecodedImage, profile: &ModelProfile) -> Array4<f32> {
    let (mean, std) = (profile.mean, profile.std);
    let (w, h) = (decoded.width as usize, decoded.height as usize);
    let crop_size = profile.image_size as usize;

    // CenterCrop：计算偏移量（对于恰好 224 的图像饱和到 0）
    let cx = w.saturating_sub(crop_size) / 2;
    let cy = h.saturating_sub(crop_size) / 2;

    let mut tensor = Array4::<f32>::zeros((1, 3, crop_size, crop_size));
    {
        // 平坦 slice 写：与 preprocess_image 使用相同寻址，算术逐位不变；
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
    /// 支持 profile 声明的 BERT WordPiece 分词器。
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

// 嵌入向量工具来自无需 inference feature 的 crate::embedding；在此再导出供调用方使用。
pub use crate::embedding::{bytes_to_embedding, embedding_to_bytes, l2_normalize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_rows_preserve_chunk_and_tail_values() {
        let mut batch =
            Some(Array4::from_shape_vec((3, 1, 1, 2), vec![1., 2., 3., 4., 5., 6.]).unwrap());
        assert_eq!(image_row_data(&mut batch, 0, 2, 2), vec![1., 2., 3., 4.]);
        assert_eq!(image_row_data(&mut batch, 2, 3, 2), vec![5., 6., 5., 6.]);
    }
}
