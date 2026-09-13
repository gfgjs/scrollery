// crates/scrollery-ai-core/src/enhance/chain.rs
//! 增强执行链:解码 → 逐 step(tiling → 采样 → ort → 融合)→ 编码(无元数据)。
//!
//! # 中间态
//! 全图 **f32 planar RGB(CHW)** buffer,值域 0–1;通道优先布局
//! `idx(c,y,x) = c·(W·H) + y·W + x`。每 step 全图过一遍 tiling 循环,产出新的
//! `W·scale × H·scale` 画布作为下一 step 输入。单点内存,无 per-tile blob 过管道。
//!
//! # fp32 IO 假设(集中标注,唯一一处)
//! P0 模型以 `keep_io_types` 导出 fp16 权重:**内部 fp16、IO fp32**(design.md §E)。
//! 故本链一律以 f32 构造输入张量、以 f32 抽取输出——GPU fp16 档与 CPU fp32 档共用同一
//! IO 契约,链侧无需分档。CPU 档加载 `-fp32.onnx`、GPU 档加载 `-fp16.onnx`(profile 决定),
//! 但两者对外皆 fp32 IO。
//!
//! # 输入张量按 aux_input 分支(集中在 [`build_and_run_tile`],便于 spike REPORT 定案后调整)
//! - `Upscale`(Real-ESRGAN)/ `Denoise`-SCUNet:`[1,3,512,512]`,0–1。
//! - `Denoise`-DRUNet:`[1,4,512,512]` = RGB + 第 4 通道 σ noise-level map(σ = strength/255)。
//! - `DejpegArtifact`(FBCNN):主输入 `[1,3,512,512]` + 第二输入 QF 标量 `[1,1]`
//!   (strength 0–100 → /100);取主图输出 `outputs[0]`。
//!
//! DRUNet/SCUNet 的分派靠 `profile.aux_input`(两者同为 `Denoise` task,不能靠 task 区分)。

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;

use image::{ExtendedColorType, ImageEncoder};
use ort::value::Tensor;

use crate::engine::SessionPool;
use crate::enhance_profile::{find_enhance_profile, EnhanceAuxInput, EnhanceProfile};

use super::tiling::{plan_tiles, Rect, TileSpec};
use super::{
    EnhanceChainRequest, EnhanceError, EnhanceOutputFormat, EnhanceReport, EnhanceStepSpec,
};

/// 强度缺省(host 通常显式给值;缺省仅为健壮性兜底)。
const DEFAULT_DRUNET_SIGMA_STRENGTH: f32 = 15.0; // 0–50 量纲
const DEFAULT_FBCNN_QF_STRENGTH: f32 = 50.0; // 0–100 量纲

/// profile_id → Session 池容器(对齐 engine.rs 的 SessionPool 封装)。
/// host/worker 侧按 step 需要的 profile 预装载 session 后填入本容器,再驱动 [`run_enhance_chain`]。
#[derive(Default)]
pub struct EnhanceSessions {
    sessions: HashMap<String, SessionPool>,
}

impl EnhanceSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册某模型档的 session 池。
    pub fn insert(&mut self, profile_id: impl Into<String>, pool: SessionPool) {
        self.sessions.insert(profile_id.into(), pool);
    }

    /// 取某模型档的 session 池。
    pub fn get(&self, profile_id: &str) -> Option<&SessionPool> {
        self.sessions.get(profile_id)
    }
}

/// 全图 planar RGB f32 画布(值域 0–1)。
struct Canvas {
    /// `3 · w · h`,布局 `c·(w·h) + y·w + x`。下标一律以 usize 展开(防 100MP×通道溢出)。
    data: Vec<f32>,
    w: u32,
    h: u32,
}

impl Canvas {
    fn plane(&self) -> usize {
        self.w as usize * self.h as usize
    }
}

/// reflect-101 边缘映射(不重复边界像素;`n==1` 恒 0)。
fn reflect(c: i64, n: i64) -> i64 {
    if n <= 1 {
        return 0;
    }
    let period = 2 * (n - 1);
    let mut m = c.rem_euclid(period);
    if m >= n {
        m = period - m;
    }
    m
}

/// 从画布按 `src_rect`(可越界,reflect 补齐)采样出 `[3, tile, tile]` planar f32。
fn sample_rgb_tile(canvas: &Canvas, src: &Rect, tile: u32) -> Vec<f32> {
    let t = tile as usize;
    let w = canvas.w as i64;
    let h = canvas.h as i64;
    let wu = canvas.w as usize;
    let plane_src = canvas.plane();
    let plane_dst = t * t;
    let mut out = vec![0.0f32; 3 * plane_dst];
    for ty in 0..t {
        let sy = reflect(src.y + ty as i64, h) as usize;
        let row_src = sy * wu;
        let row_dst = ty * t;
        for tx in 0..t {
            let sx = reflect(src.x + tx as i64, w) as usize;
            let s = row_src + sx;
            let d = row_dst + tx;
            for c in 0..3 {
                out[c * plane_dst + d] = canvas.data[c * plane_src + s];
            }
        }
    }
    out
}

/// 把某 step 的模型输出瓦片(planar `[3, toh, tow]`)的 core 中心区写回输出画布。
/// core 在输出瓦片内的局部起点恒 = `pad·scale`(src 起点 = core 起点 − pad,见 tiling 不变量)。
fn write_core(
    out: &mut Canvas,
    ts: &TileSpec,
    tile_out: &[f32],
    tow: usize,
    toh: usize,
    pad: u32,
    scale: u32,
) {
    let local = (pad * scale) as usize; // 两轴同偏移
    let plane_out = out.plane();
    let plane_tile = tow * toh;
    let ow = out.w as usize;
    let dw = ts.dst_rect.w as usize;
    let dh = ts.dst_rect.h as usize;
    let dx = ts.dst_rect.x as usize;
    let dy = ts.dst_rect.y as usize;
    for c in 0..3 {
        let cp_out = c * plane_out;
        let cp_tile = c * plane_tile;
        for ry in 0..dh {
            let src_base = cp_tile + (local + ry) * tow + local;
            let dst_base = cp_out + (dy + ry) * ow + dx;
            out.data[dst_base..dst_base + dw].copy_from_slice(&tile_out[src_base..src_base + dw]);
        }
    }
}

/// 构造张量、跑 session、抽主图输出为 planar `[3, toh, tow]` f32,并返回 `(vec, tow, toh)`。
fn build_and_run_tile(
    pool: &SessionPool,
    aux_input: EnhanceAuxInput,
    profile_id: &str,
    rgb_tile: Vec<f32>,
    tile: u32,
    scale: u32,
    strength: Option<f32>,
) -> Result<(Vec<f32>, usize, usize), EnhanceError> {
    let mut guard = pool
        .get()
        .ok_or_else(|| EnhanceError::Inference(format!("session pool 断开: {profile_id}")))?;
    let input_names: Vec<String> = guard
        .inputs()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    if input_names.is_empty() {
        return Err(EnhanceError::Inference(format!("{profile_id} 无输入")));
    }
    let t = tile as i64;

    // ── 按 profile.aux_input 构造输入并 run(唯一分支点)──────────────────────────
    let outputs = match aux_input {
        EnhanceAuxInput::SigmaMap => {
            // [1,4,512,512]:RGB + σ noise-level map(第 4 通道常量填充,σ = strength/255)。
            let sigma = strength.unwrap_or(DEFAULT_DRUNET_SIGMA_STRENGTH) / 255.0;
            let plane = (tile * tile) as usize;
            let mut buf = Vec::with_capacity(4 * plane);
            buf.extend_from_slice(&rgb_tile);
            buf.extend(std::iter::repeat_n(sigma, plane));
            let tensor = Tensor::from_array(([1i64, 4, t, t], buf))
                .map_err(|e| EnhanceError::Inference(e.to_string()))?;
            guard
                .run(vec![(input_names[0].as_str(), tensor)])
                .map_err(|e| EnhanceError::Inference(e.to_string()))?
        }
        EnhanceAuxInput::QualityFactor => {
            // 主输入 [1,3,512,512] + 第二输入 QF 标量 [1,1](strength 0–100 → /100)。
            let qf = strength.unwrap_or(DEFAULT_FBCNN_QF_STRENGTH) / 100.0;
            let main = Tensor::from_array(([1i64, 3, t, t], rgb_tile))
                .map_err(|e| EnhanceError::Inference(e.to_string()))?;
            if input_names.len() < 2 {
                return Err(EnhanceError::Inference(format!(
                    "{profile_id} 需 2 输入(image+qf),实得 {}",
                    input_names.len()
                )));
            }
            let qf_tensor = Tensor::from_array(([1i64, 1], vec![qf]))
                .map_err(|e| EnhanceError::Inference(e.to_string()))?;
            guard
                .run(vec![
                    (input_names[0].as_str(), main),
                    (input_names[1].as_str(), qf_tensor),
                ])
                .map_err(|e| EnhanceError::Inference(e.to_string()))?
        }
        // Upscale(Real-ESRGAN)/ Denoise-SCUNet:单输入 [1,3,512,512] 0–1。
        EnhanceAuxInput::None => {
            let tensor = Tensor::from_array(([1i64, 3, t, t], rgb_tile))
                .map_err(|e| EnhanceError::Inference(e.to_string()))?;
            guard
                .run(vec![(input_names[0].as_str(), tensor)])
                .map_err(|e| EnhanceError::Inference(e.to_string()))?
        }
    };

    // ── 抽主图输出(fp32 IO 假设)→ planar [3, toh, tow]───────────────────────────
    if outputs.len() == 0 {
        return Err(EnhanceError::Inference(format!("{profile_id} 无输出")));
    }
    let (shape, slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| EnhanceError::Inference(e.to_string()))?;
    let dims = shape.len();
    if dims < 2 {
        return Err(EnhanceError::Inference(format!(
            "{profile_id} 输出 rank {dims} < 2"
        )));
    }
    let toh = shape[dims - 2] as usize;
    let tow = shape[dims - 1] as usize;
    let expect = (tile * scale) as usize;
    if toh != expect || tow != expect {
        return Err(EnhanceError::Inference(format!(
            "{profile_id} 输出尺寸 {tow}×{toh} 与预期 {expect}²(tile·scale)不符——tile/scale 与导出 shape 联动契约(D-439)被破坏?"
        )));
    }
    if slice.len() < 3 * tow * toh {
        return Err(EnhanceError::Inference(format!(
            "{profile_id} 输出通道不足(need ≥3, got flat {})",
            slice.len()
        )));
    }
    let vec = slice[..3 * tow * toh].to_vec();
    Ok((vec, tow, toh))
}

/// 执行整条增强链。`progress(done, total)` 每完成一个瓦片回调一次;`total` = 全链瓦片数之和。
pub fn run_enhance_chain(
    sessions: &EnhanceSessions,
    req: &EnhanceChainRequest,
    progress: Option<&dyn Fn(u32, u32)>,
) -> Result<EnhanceReport, EnhanceError> {
    if req.steps.is_empty() {
        return Err(EnhanceError::UnsupportedInput("空增强链(无 step)".into()));
    }

    // ── 解码 → 初始画布(RGB8 → f32/255 planar)────────────────────────────────
    let dynimg = image::ImageReader::open(&req.source_path)
        .map_err(|e| EnhanceError::Io(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| EnhanceError::Io(e.to_string()))?
        .decode()
        .map_err(|e| EnhanceError::Decode(e.to_string()))?;
    let rgb = dynimg.to_rgb8();
    let (w0, h0) = rgb.dimensions();
    if w0 == 0 || h0 == 0 {
        return Err(EnhanceError::UnsupportedInput("源图零尺寸".into()));
    }
    let mut canvas = decode_to_canvas(&rgb);

    // ── 预解析 profile + 预算总瓦片数(几何模拟,不推理)──────────────────────────
    let mut resolved: Vec<(EnhanceProfile, &EnhanceStepSpec)> = Vec::with_capacity(req.steps.len());
    for step in &req.steps {
        let p = find_enhance_profile(&step.profile_id)
            .ok_or_else(|| EnhanceError::ProfileMissing(step.profile_id.clone()))?;
        resolved.push((p, step));
    }
    let mut tiles_total: u32 = 0;
    {
        let (mut sw, mut sh) = (w0, h0);
        for (p, _) in &resolved {
            let n = plan_tiles(sw, sh, p.tile, p.tile_pad, p.scale).len() as u32;
            tiles_total = tiles_total.saturating_add(n);
            sw = sw.saturating_mul(p.scale);
            sh = sh.saturating_mul(p.scale);
        }
    }

    // ── 逐 step 执行(严格按 req.steps 顺序,不重排)────────────────────────────
    let mut done: u32 = 0;
    for (profile, step) in &resolved {
        let pool = sessions
            .get(&profile.id)
            .ok_or_else(|| EnhanceError::Inference(format!("session 未装载: {}", profile.id)))?;
        let tiles = plan_tiles(
            canvas.w,
            canvas.h,
            profile.tile,
            profile.tile_pad,
            profile.scale,
        );
        let ow = canvas.w.saturating_mul(profile.scale);
        let oh = canvas.h.saturating_mul(profile.scale);
        let mut out = Canvas {
            data: vec![0.0f32; 3 * ow as usize * oh as usize],
            w: ow,
            h: oh,
        };
        for ts in &tiles {
            let rgb_tile = sample_rgb_tile(&canvas, &ts.src_rect, profile.tile);
            let (tile_out, tow, toh) = build_and_run_tile(
                pool,
                profile.aux_input,
                &profile.id,
                rgb_tile,
                profile.tile,
                profile.scale,
                step.strength,
            )?;
            write_core(
                &mut out,
                ts,
                &tile_out,
                tow,
                toh,
                profile.tile_pad,
                profile.scale,
            );
            done += 1;
            if let Some(cb) = progress {
                cb(done, tiles_total);
            }
        }
        canvas = out;
    }

    // ── 编码(无元数据)写 output_tmp_path ──────────────────────────────────────
    encode_canvas(&canvas, req.output_format, req)?;

    Ok(EnhanceReport {
        out_width: canvas.w,
        out_height: canvas.h,
        tiles_total,
    })
}

/// RGB8 → planar f32/255 画布。
fn decode_to_canvas(rgb: &image::RgbImage) -> Canvas {
    let (w, h) = rgb.dimensions();
    let plane = w as usize * h as usize;
    let mut data = vec![0.0f32; 3 * plane];
    for (i, px) in rgb.pixels().enumerate() {
        data[i] = px.0[0] as f32 / 255.0;
        data[plane + i] = px.0[1] as f32 / 255.0;
        data[2 * plane + i] = px.0[2] as f32 / 255.0;
    }
    Canvas { data, w, h }
}

/// planar f32 画布 → 交织 RGB8(u8 量化 clamp)→ 无元数据编码写文件。
fn encode_canvas(
    canvas: &Canvas,
    fmt: EnhanceOutputFormat,
    req: &EnhanceChainRequest,
) -> Result<(), EnhanceError> {
    let plane = canvas.plane();
    let n = plane;
    let mut interleaved = vec![0u8; 3 * n];
    for i in 0..n {
        for c in 0..3 {
            let v = canvas.data[c * plane + i];
            interleaved[i * 3 + c] = (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
    let file = File::create(&req.output_tmp_path).map_err(|e| EnhanceError::Io(e.to_string()))?;
    let mut w = BufWriter::new(file);
    match fmt {
        EnhanceOutputFormat::Jpeg => {
            // quality 95;JpegEncoder 不写 EXIF/元数据。
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 95)
                .write_image(&interleaved, canvas.w, canvas.h, ExtendedColorType::Rgb8)
                .map_err(|e| EnhanceError::Encode(e.to_string()))?;
        }
        EnhanceOutputFormat::Png => {
            image::codecs::png::PngEncoder::new(&mut w)
                .write_image(&interleaved, canvas.w, canvas.h, ExtendedColorType::Rgb8)
                .map_err(|e| EnhanceError::Encode(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_101_edges() {
        // n=5:序列在边界不重复(reflect-101):…1 0 |0 1 2 3 4| 3 2…
        assert_eq!(reflect(-1, 5), 1);
        assert_eq!(reflect(-2, 5), 2);
        assert_eq!(reflect(0, 5), 0);
        assert_eq!(reflect(4, 5), 4);
        assert_eq!(reflect(5, 5), 3);
        assert_eq!(reflect(6, 5), 2);
        // n=1:恒 0(单像素轴)。
        assert_eq!(reflect(-3, 1), 0);
        assert_eq!(reflect(7, 1), 0);
    }

    #[test]
    fn sample_reflects_out_of_bounds() {
        // 2×1 画布,通道 0 值 [0.2, 0.8]。src 从 x=-1 起采 4 宽:reflect → x: -1,0,1,2 → 0,0,1,0。
        let canvas = Canvas {
            data: vec![0.2, 0.8, /*g*/ 0.0, 0.0, /*b*/ 0.0, 0.0],
            w: 2,
            h: 1,
        };
        let src = Rect {
            x: -1,
            y: 0,
            w: 4,
            h: 4,
        };
        let t = sample_rgb_tile(&canvas, &src, 4);
        // planar [3,4,4];通道0 第 0 行 = x(-1,0,1,2)→ reflect-101(w=2)→ idx 1,0,1,0 → 值 0.8,0.2,0.8,0.2。
        assert_eq!(&t[0..4], &[0.8, 0.2, 0.8, 0.2]);
    }
}
