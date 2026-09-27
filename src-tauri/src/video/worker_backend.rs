// src-tauri/src/video/worker_backend.rs
//! 视频格式扩展子系统 · 缩略图后端桥(design.md §1 修正2 / §8 V5)。
//!
//! `WorkerVideoBackend` 实现既有 `VideoBackend` trait([`crate::video::VideoBackend`]),
//! 把 probe/cover/keyframes 三方法委托给 host 侧 [`crate::video::worker_service::VideoWorkerService`]
//! (video-worker 子进程,同一 worker 兼供缩略图后端,§1 修正2)。MF 后端(`media_foundation.rs`)
//! 不认的容器(mkv/webm/flv/ogv 等)经 `backend_for()` 接入本桥,派生流水线(`derive/video.rs`)
//! 零改造即自动获得覆盖 —— `kind.rs` 的 `is_implemented` 只是编译期粗粒度门,真实按扩展名的
//! 可用性判定内聚在 `backend_for` 返回 `None`/`Some` 上,本文件即该判定新增的第二分支。
//!
//! ## 同步 trait ↔ 异步 Service 桥接
//! `VideoBackend` 三方法是**同步**签名(供 rayon 派生池直调,§1.4.1);`VideoWorkerService` 的
//! 公开方法是 **async**(内部经 std 线程 driver + `tokio::sync::oneshot` 交割结果,§2.1)。
//! 派生任务运行在 `spawn_blocking` 内嵌的 `rayon::scope` 工作线程上 —— rayon 自有线程池、
//! 非 tokio 托管,不能假设外层 `Handle::current()` 可用。桥接方案:本文件持一个**独立、
//! 惰性初始化**的单线程 `tokio::runtime::Runtime`,只用于 `block_on` 这三个 async 调用 ——
//! 不依赖调用方是否处于 tokio 上下文,也不额外起工作线程(`current_thread`)。
//!
//! ## host 不信任 worker(§6 / `exotic::worker.rs:11-12` 先例)
//! Cover/Keyframes 回传的 WebP blob 经 [`VideoWorkerService`] 传输到本层后,**仍需独立解码
//! 复核**:雪碧条声明的切格元数据 `{cell_width,cell_height,n}` 必须与独立解码得到的实际尺寸
//! 完全吻合,否则整批拒收(见 [`decode_sprite_cells`])。

use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use exotic_protocol::{VideoFramesInfo, VideoFramesMode, VideoProbeInfo};

use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::video::worker_service::{VideoPriority, VideoServiceError, VideoWorkerService};
use crate::video::{VideoBackend, VideoInfo};

/// video-worker 桥接 runtime:仅供 `block_on` 驱动三个 async Service 调用,不额外起工作线程
/// (`current_thread`;`enable_all` 供 oneshot 等 tokio 原语正常工作)。
fn bridge_rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("构建 video worker_backend 桥接 runtime 失败 | build bridge runtime failed")
    })
}

/// `VideoBackend` 实现:probe/cover/keyframes 委托给 [`VideoWorkerService`](§2.1)。
pub struct WorkerVideoBackend {
    service: Arc<VideoWorkerService>,
}

impl WorkerVideoBackend {
    pub fn new(service: Arc<VideoWorkerService>) -> Self {
        WorkerVideoBackend { service }
    }
}

impl VideoBackend for WorkerVideoBackend {
    fn name(&self) -> &'static str {
        "video-worker"
    }

    /// `backend_for()` 已按扩展名 × 授权 × ffmpeg 就绪三重门控挑选本后端(§8 V5),
    /// 故本方法恒真 —— 真正的「能不能处理」判定收敛在 `backend_for` 一处,避免两处判据漂移。
    fn can_handle(&self, _ext: &str) -> bool {
        true
    }

    fn probe(&self, path: &Path) -> Result<VideoInfo> {
        let item_id = pseudo_item_id(path);
        let fingerprint = fingerprint_of(path)?;
        // 派生流水线批量导入走 Background 优先级(V5 深审批4):批量 probe 不该抢占播放交互
        // 队列在途 kill,也不该堵播放侧 probe(播放侧走 `VideoWorkerService::probe` 恒 Interactive)。
        let info: VideoProbeInfo = bridge_rt()
            .block_on(self.service.probe_with_priority(
                item_id,
                path_string(path),
                fingerprint,
                VideoPriority::Background,
            ))
            .map_err(|e| map_service_err(e, path))?;
        Ok(map_probe_info(info))
    }

    fn cover_bounded(
        &self,
        path: &Path,
        max_long_edge: u32,
        max_pixel_bytes: u64,
    ) -> Result<DecodedImage> {
        let item_id = pseudo_item_id(path);
        let fingerprint = fingerprint_of(path)?;
        let (blob, _info) = bridge_rt()
            .block_on(self.service.frames(
                item_id,
                path_string(path),
                fingerprint,
                VideoFramesMode::Cover { max_long_edge },
            ))
            .map_err(|e| map_service_err(e, path))?;
        decode_cover_webp(&blob, max_long_edge, max_pixel_bytes)
    }

    fn keyframes_bounded(
        &self,
        path: &Path,
        n: usize,
        cell_height: u32,
        max_pixel_bytes: u64,
    ) -> Result<Vec<DecodedImage>> {
        let n_u32 = n.max(1) as u32;
        let item_id = pseudo_item_id(path);
        let fingerprint = fingerprint_of(path)?;
        let (blob, info) = bridge_rt()
            .block_on(self.service.frames(
                item_id,
                path_string(path),
                fingerprint,
                VideoFramesMode::Keyframes {
                    n: n_u32,
                    cell_height,
                },
            ))
            .map_err(|e| map_service_err(e, path))?;
        let info = info.ok_or_else(|| {
            AppError::Internal(
                "video-worker 未回传雪碧条切格元数据 | missing video_frames info".into(),
            )
        })?;
        decode_sprite_cells(&blob, info, n_u32, cell_height, max_pixel_bytes)
    }
}

/// 路径 → 服务侧 `source_path`(字符串,worker 端按此定位源文件)。
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 去重维度用的合成 `item_id`(§9.12 `(item,kind)` 去重键)。`VideoBackend` trait 只传路径、
/// 不携带真实 DB item id(它服务于派生流水线的纯函数式 `run(ctx)`,`ctx.item_id` 未下传到这层
/// trait 接口)—— 用路径的稳定哈希充当去重键:同路径的重入请求天然合并。注意这是正确性面
/// 而非纯性能优化——碰撞会把别的文件的结果错交给等待方,靠 63 位哈希空间把概率压到可忽略,
/// 勿换弱哈希。
fn pseudo_item_id(path: &Path) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    // 与真实 DB id 空间隔离:强制置符号位,恒落负数命名空间(真实 item_id 恒为正)。
    (hasher.finish() as i64) | i64::MIN
}

/// 输入指纹 = mtime(秒)+ 文件大小(design.md §9.1「源文件在 remux 中被改/删」核对语义同款)。
/// 只发不验——协议前向字段,worker 侧核对记 V8 遗留;源变更错配由下轮 mtime 重派自愈
/// (与 `exotic::fingerprint` 面向 cache_key 的版本化指纹是两回事)。
fn fingerprint_of(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(format!("{mtime}:{}", meta.len()))
}

/// `VideoServiceError` → `AppError`。授权/组件未就绪/worker 不可用等价「后端不支持」——
/// `backend_for` 已做过同等门控,这里出现多半是门控与实际派活之间的竞态(如 FFmpeg 被用户
/// 中途卸载),按「不支持该格式」降级而非报泛化内部错误(设计要求的防御性兜底)。
fn map_service_err(e: VideoServiceError, path: &Path) -> AppError {
    match e {
        VideoServiceError::NotAuthorized
        | VideoServiceError::NeedsComponent
        | VideoServiceError::WorkerUnavailable
        | VideoServiceError::FfmpegUnavailable => AppError::UnsupportedFormat(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        other => other.into(),
    }
}

/// `VideoProbeInfo`(worker 流事实,字段多为 `Option`)→ 既有 `VideoInfo`(trait 返回类型,
/// 非 `Option` 字段)。缺失项按 0/空 补齐 —— 与 MF 后端探测失败时的既有容错姿态一致
/// (调用方不因个别字段缺失整体失败)。
fn map_probe_info(info: VideoProbeInfo) -> VideoInfo {
    let rotation = info.rotation.unwrap_or(0);
    let (nw, nh) = (info.width.unwrap_or(0), info.height.unwrap_or(0));
    // 90/270 旋转交换显示宽高(VideoBackend 契约=显示尺寸,media_foundation.rs:89 先例:
    // worker 协议层 width/height 是转换前的原生几何,同 MF NATIVE 类型同理需要交换)。
    let (width, height) = if rotation == 90 || rotation == 270 {
        (nh, nw)
    } else {
        (nw, nh)
    };
    VideoInfo {
        width,
        height,
        duration_ms: info.duration_ms.unwrap_or(0),
        rotation,
        fps: info.fps.unwrap_or(0.0),
        // 协议层 bitrate 是 u64(容大码率),trait 历史字段是 u32 —— 钳到上限,截断而非 panic
        // (仅展示用途,截断无功能性影响)。
        bitrate: info.bitrate.unwrap_or(0).min(u32::MAX as u64) as u32,
        has_audio: !info.audio_tracks.is_empty(),
        codec: if info.video_codec.is_empty() {
            None
        } else {
            // 归一大写,与 MF 后端(media_foundation.rs codec_label)落库标签一致。
            Some(info.video_codec.to_ascii_uppercase())
        },
    }
}

/// Cover 独立解码防御上限(host 不信任 worker,§6):长边容差同 `exotic/validate.rs:75-96`
/// 先例(64px);总像素上限防御体积异常放大(4K UHD=8,294,400px,留有余量覆盖非 16:9 源)。
const COVER_LONG_EDGE_TOLERANCE: u32 = 64;
const COVER_MAX_OUTPUT_PIXELS: u64 = 16_000_000;

/// 独立解码 Cover 单帧 WebP(host 不信任 worker,§6 / `exotic::worker.rs:11-12` 先例)。
/// `max_long_edge`(0 = 原生尺寸,不做长边校验)是本次请求档位 —— 解码后须核对实际尺寸
/// 未越出「请求档位 + 容差」,且总像素不超上限,否则整批拒收(同 `exotic/validate.rs` 先例)。
fn decode_cover_webp(
    blob: &[u8],
    max_long_edge: u32,
    max_pixel_bytes: u64,
) -> Result<DecodedImage> {
    let reader =
        image::ImageReader::with_format(std::io::Cursor::new(blob), image::ImageFormat::WebP);
    let img = crate::engine::image_rs::decode_image_reader_bounded(
        reader,
        max_pixel_bytes.min(COVER_MAX_OUTPUT_PIXELS * 4),
    )
    .map_err(|e| {
        AppError::Internal(format!(
            "封面 WebP 独立解码失败 | cover WebP decode failed: {e}"
        ))
    })?
    .into_rgba8();
    let (width, height) = (img.width(), img.height());
    if max_long_edge > 0 {
        let long = width.max(height);
        let cap = max_long_edge.saturating_add(COVER_LONG_EDGE_TOLERANCE);
        if long > cap {
            return Err(AppError::Internal(format!(
                "封面长边 {long} 超过请求档位 {max_long_edge}+容差 {COVER_LONG_EDGE_TOLERANCE} \
                 | cover long edge exceeds requested cap"
            )));
        }
    }
    let pixels = (width as u64).saturating_mul(height as u64);
    if pixels > COVER_MAX_OUTPUT_PIXELS {
        return Err(AppError::Internal(format!(
            "封面像素 {pixels} 超上限 {COVER_MAX_OUTPUT_PIXELS} | cover pixel count exceeds cap"
        )));
    }
    Ok(DecodedImage {
        pixels: img.into_raw(),
        width,
        height,
        icc: None, // 视频帧无 ICC 来源,按 sRGB 假定(同 media_foundation.rs 惯例)。
    })
}

/// 独立解码雪碧条 WebP 并按声明的 `{cell_width,cell_height,n}` 切格(host 不信任 worker:
/// 声明几何须与实际解码尺寸完全吻合,否则整批拒收,§6)。切法与 `derive/video.rs` 的横向
/// 拼接严格对称(左→右第 i 格 = x ∈ [i*cell_w, (i+1)*cell_w))。`requested_n`/`requested_cell_height`
/// 是本次请求参数 —— worker 声明的 `info.n`/`info.cell_height` 必须与请求完全一致,否则拒收
/// (钉几何请求,防止 worker 静默改判/错序应答被当作合法结果消费)。
fn decode_sprite_cells(
    blob: &[u8],
    info: VideoFramesInfo,
    requested_n: u32,
    requested_cell_height: u32,
    max_pixel_bytes: u64,
) -> Result<Vec<DecodedImage>> {
    if info.n != requested_n || info.cell_height != requested_cell_height {
        return Err(AppError::Internal(format!(
            "雪碧条声明几何与请求不符:声明 n={} cell_height={},请求 n={requested_n} \
             cell_height={requested_cell_height} | sprite metadata mismatch with request",
            info.n, info.cell_height
        )));
    }
    super::check_frame_collection_budget(
        info.cell_width,
        info.cell_height,
        info.n as usize,
        max_pixel_bytes,
    )?;
    let reader =
        image::ImageReader::with_format(std::io::Cursor::new(blob), image::ImageFormat::WebP);
    let img = crate::engine::image_rs::decode_image_reader_bounded(reader, max_pixel_bytes)
        .map_err(|e| {
            AppError::Internal(format!(
                "雪碧条 WebP 独立解码失败 | sprite WebP decode failed: {e}"
            ))
        })?
        .into_rgba8();
    let (w, h) = (img.width(), img.height());
    let (cell_w, cell_h, n) = (info.cell_width, info.cell_height, info.n);
    if cell_w == 0 || cell_h == 0 || n == 0 {
        return Err(AppError::Internal(
            "雪碧条切格元数据非法(0) | invalid sprite cell metadata".into(),
        ));
    }
    if h != cell_h || w != cell_w.saturating_mul(n) {
        return Err(AppError::Internal(format!(
            "雪碧条几何与声明不符:实际 {w}x{h},声明 {cell_w}x{cell_h}×{n} | sprite geometry mismatch"
        )));
    }
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let x0 = i * cell_w;
        let cell = image::imageops::crop_imm(&img, x0, 0, cell_w, cell_h).to_image();
        out.push(DecodedImage {
            pixels: cell.into_raw(),
            width: cell_w,
            height: cell_h,
            icc: None,
        });
    }
    Ok(out)
}
