// src-tauri/src/scanner/metadata.rs
//! EXIF 和 XMP 元数据解析。
//!
//! 使用 `kamadak-exif` 解析 EXIF，使用 `quick-xml` 解析 XMP（动态照片检测）。

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::hdd_io::{DiskBudget, ReadPermit};
use crate::db::models::ImageMeta;
use crate::error::{AppError, Result};
use tokio_util::sync::CancellationToken;

/// 单项诊断与读取生命周期；补读计时包含解析，不能当作纯磁盘等待。
#[derive(Default)]
pub(crate) struct ImageReadContext {
    budget: Option<Arc<DiskBudget>>,
    cancel: CancellationToken,
    permit: Option<Arc<ReadPermit>>,
    disk_read_timed_out: bool,
    pub file_access_wait: Duration,
    pub exif_memory: Duration,
    pub exif_probe: Duration,
    pub exif_fallback: Duration,
    pub dimensions_file: bool,
}

impl ImageReadContext {
    pub(crate) fn with_disk_budget(
        budget: Option<Arc<DiskBudget>>,
        cancel: &CancellationToken,
    ) -> Self {
        Self {
            budget,
            cancel: cancel.clone(),
            ..Default::default()
        }
    }

    /// 纯缓冲解析不申请名额；首次补读才独占，直到本项及其超时后台读取均结束。
    pub(crate) fn ensure_file_access(&mut self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        self.check_io_error()?;
        if self.permit.is_none() {
            if let Some(budget) = &self.budget {
                let started = Instant::now();
                let acquired = budget.acquire(&self.cancel);
                self.file_access_wait += started.elapsed();
                self.disk_read_timed_out = matches!(acquired, Err(AppError::ImageReadTimeout));
                self.permit = Some(acquired?);
            }
        }
        Ok(())
    }

    /// 尺寸接口用零值表示普通失败；磁盘超时必须向批次传播，不能写入最小元数据行。
    pub(crate) fn check_io_error(&self) -> Result<()> {
        if self.disk_read_timed_out
            || self
                .permit
                .as_ref()
                .is_some_and(|permit| permit.timed_out())
        {
            return Err(AppError::ImageReadTimeout);
        }
        Ok(())
    }
}

/// TIFF 维度解析的硬超时上限。读文件头本应亚秒级完成；给 5s 余量以容忍慢盘 /
/// 合法大文件，同时对畸形 TIFF 的无限阻塞兜底。
const TIFF_DIMENSION_TIMEOUT: Duration = Duration::from_secs(5);

/// 在 detached 线程上运行 `f`，最多等待 `timeout`；超时 / 线程 panic 返回 `None`。
///
/// 用于给可能无限阻塞的第三方解析（如畸形 TIFF 的 `image::image_dimensions`）设硬上限：
/// 超时即放弃等待、立即返回，让出当前工作线程。落单线程在后台自行跑完退出——
/// HDD 超时会唤醒同盘等待者报错；后台线程仍持有名额，直到实际读取结束才恢复。
///
/// 注意：`std::thread::scope` 无法实现真超时——其 drop 必须 join 完所有子线程才返回，
/// 与「超时即放弃」语义互斥；故此处用 detached spawn + `recv_timeout`。
fn run_with_timeout<T, F>(timeout: Duration, permit: Option<Arc<ReadPermit>>, f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let worker_permit = permit.clone();
    std::thread::spawn(move || {
        // 等待端超时不会中止文件访问，名额必须由真正工作的线程持有。
        let _permit = worker_permit;
        // 接收端可能已超时丢弃 rx → send 失败属预期，忽略。
        let _ = tx.send(f());
    });
    match rx.recv_timeout(timeout) {
        Ok(value) => Some(value),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // 等待端也保留一份守卫，避免后台恰好结束时把超时标记写到下一个读取上。
            if let Some(permit) = permit {
                permit.mark_timed_out();
            }
            None
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    }
}

// ── EXIF orientation (fast path — for quick scan) ────────────────────────────
// ── EXIF 方向（快速路径 — 用于快速扫描） ────────────────────────────

/// 仅读取 JPEG 的 EXIF 方向标签。
/// 返回方向值 (1-8)，如果不存在 / 出错则返回 `1`。
/// 这是轻量级的：kamadak-exif 仅读取足够的字节来寻找标签。
pub fn read_jpeg_orientation(path: &Path) -> u32 {
    read_orientation_inner(path).unwrap_or(1)
}

fn read_orientation_inner(path: &Path) -> Option<u32> {
    let mut file = std::fs::File::open(path).ok()?;
    Some(read_jpeg_orientation_file(&mut file))
}

/// 从已授权打开的句柄读取 JPEG 方向；返回前恢复文件游标，供同句柄继续解码。
pub fn read_jpeg_orientation_file(file: &mut std::fs::File) -> u32 {
    use std::io::{Seek, SeekFrom};
    // EXIF 快捷探测和后端回退共用句柄，调用前的游标不一定在文件起点。
    if file.rewind().is_err() {
        return 1;
    }
    let orientation = {
        let mut reader = BufReader::new(&mut *file);
        exif::Reader::new()
            .read_from_container(&mut reader)
            .ok()
            .and_then(|exif| {
                exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                    .and_then(|field| match field.value {
                        exif::Value::Short(ref values) => values.first().copied(),
                        _ => None,
                    })
            })
            .map(u32::from)
            .unwrap_or(1)
    };
    let _ = file.seek(SeekFrom::Start(0));
    orientation
}

/// 头缓冲版 JPEG EXIF 方向读取(阶段5):优先从已读缓冲解析;失败且缓冲截断
/// (EXIF 可能落在 128KB 窗口之外)才回退整文件路径,保留旧语义。
pub fn read_jpeg_orientation_buf(path: &Path, hb: &HeaderBuf) -> u32 {
    let mut cursor = std::io::Cursor::new(&hb.bytes[..]);
    match exif::Reader::new().read_from_container(&mut cursor) {
        Ok(exif) => orientation_from_exif(&exif),
        // 头部已经看到了 SOS/EOI 且没有 EXIF 时，后面的扫描数据不可能再出现
        // 合法的 EXIF APP1；不要为了确认方向再把整张 JPEG 读一遍。
        Err(_) if hb.truncated() && jpeg_exif_probe(&hb.bytes) == TruncatedExifProbe::NeedMore => {
            read_jpeg_orientation(path)
        }
        Err(_) => 1,
    }
}

fn orientation_from_exif(exif: &exif::Exif) -> u32 {
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| match field.value {
            exif::Value::Short(ref v) => v.first().copied().map(|n| n as u32),
            _ => None,
        })
        .unwrap_or(1)
}

/// 如果方向值需要 90° / 270° 旋转，则返回 `true`
/// （即宽度和高度应该互换）。
pub fn orientation_needs_swap(orientation: u32) -> bool {
    matches!(orientation, 5..=8)
}

/// Header-only pixel dimensions, WITHOUT orientation correction and WITHOUT any
/// EXIF read (TIFF gets a scoped-thread timeout guard). Returns `(0, 0)` on failure.
/// 仅读文件头的像素尺寸：不做方向校正、不读 EXIF（TIFF 用作用域线程加超时保护）。失败返回 `(0, 0)`。
pub fn read_raw_dimensions(abs_path: &Path, ext: &str) -> (i64, i64) {
    read_raw_dimensions_with_context(abs_path, ext, &mut ImageReadContext::default())
}

fn read_raw_dimensions_with_context(
    abs_path: &Path,
    ext: &str,
    context: &mut ImageReadContext,
) -> (i64, i64) {
    if context.ensure_file_access().is_err() {
        return (0, 0);
    }
    context.dimensions_file = true;
    // TIFF: 解析可能读取大量字节、且畸形文件可能无限阻塞 — 用真超时守卫兜底。
    if ext == "tif" || ext == "tiff" {
        let path = abs_path.to_path_buf();
        let permit = context.permit.clone();
        return run_with_timeout(TIFF_DIMENSION_TIMEOUT, permit, move || {
            image::image_dimensions(&path).ok()
        })
        .flatten()
        .map(|(w, h)| (w as i64, h as i64))
        .unwrap_or((0, 0));
    }

    image::image_dimensions(abs_path)
        .map(|(w, h)| (w as i64, h as i64))
        .unwrap_or((0, 0))
}

/// 头缓冲版尺寸读取(#10):非 TIFF 从缓冲解头(免再开文件);TIFF 保持原路径(IFD 偏移可指向
/// 文件任意处,截断缓冲不可靠,且需保留超时守卫)。缓冲解不出且缓冲截断 → 原路径回退。
pub fn read_raw_dimensions_buf(abs_path: &Path, ext: &str, hb: &HeaderBuf) -> (i64, i64) {
    read_dimensions_with_context(abs_path, ext, Some(hb), &mut ImageReadContext::default())
}

pub(crate) fn read_dimensions_with_context(
    abs_path: &Path,
    ext: &str,
    hb: Option<&HeaderBuf>,
    context: &mut ImageReadContext,
) -> (i64, i64) {
    let Some(hb) = hb else {
        return read_raw_dimensions_with_context(abs_path, ext, context);
    };
    if ext == "tif" || ext == "tiff" {
        return read_raw_dimensions_with_context(abs_path, ext, context);
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(&hb.bytes[..]));
    if let Ok(guessed) = reader.with_guessed_format() {
        if let Ok((w, h)) = guessed.into_dimensions() {
            return (w as i64, h as i64);
        }
    }
    if hb.truncated() {
        read_raw_dimensions_with_context(abs_path, ext, context)
    } else {
        (0, 0)
    }
}

/// 当 EXIF 方向表示 90°/270° 旋转时交换 `(w, h)`。
pub fn apply_orientation_swap(dims: (i64, i64), orientation: u32) -> (i64, i64) {
    if orientation_needs_swap(orientation) {
        (dims.1, dims.0)
    } else {
        dims
    }
}

/// Orientation-corrected dimensions. For JPEG this reads the EXIF Orientation tag
/// (one extra file open); callers that have ALREADY parsed EXIF should instead use
/// `read_raw_dimensions` + `apply_orientation_swap` with the known orientation to
/// avoid re-opening the file.
/// 经方向校正的尺寸。JPEG 会读取 EXIF 方向标签（多开一次文件）；已解析过 EXIF 的调用方
/// 应改用 `read_raw_dimensions` + `apply_orientation_swap` 传入已知方向，避免重复打开文件。
///
/// Single-sourced so the fast-scan eager path and the viewport-priority path stay
/// consistent (same orientation handling → no double-flip).
/// 在此单一实现，使快速扫描即时路径与可视窗口优先路径一致（相同方向处理 → 不会双重翻转）。
pub fn read_image_dimensions(abs_path: &Path, ext: &str) -> (i64, i64) {
    let dims = read_raw_dimensions(abs_path, ext);
    if dims == (0, 0) {
        return (0, 0);
    }
    if ext == "jpg" || ext == "jpeg" {
        apply_orientation_swap(dims, read_jpeg_orientation(abs_path))
    } else {
        dims
    }
}

/// 头缓冲版方向校正尺寸(阶段5 fast_scan eager 路径):一次 open 出的 [`HeaderBuf`]
/// 同时供尺寸与 JPEG orientation 消费,取代「尺寸 open 一次 + orientation 再 open 一次」。
/// TIFF 仍走原路径(IFD 任意偏移 + 超时守卫);其余格式与 [`read_image_dimensions`] 语义一致。
pub fn read_image_dimensions_buf(abs_path: &Path, ext: &str, hb: &HeaderBuf) -> (i64, i64) {
    let dims = read_raw_dimensions_buf(abs_path, ext, hb);
    if dims == (0, 0) {
        return (0, 0);
    }
    if ext == "jpg" || ext == "jpeg" {
        apply_orientation_swap(dims, read_jpeg_orientation_buf(abs_path, hb))
    } else {
        dims
    }
}

// ── 富化单次头读缓冲(#10,2026-07-17)───────────────────────────────────────────
//
// 原富化 pass 对同一文件最多 open 3 次(EXIF / XMP 128KB / 尺寸),Windows 下每次 open
// 伴随属性检查与 Defender 扫描,是「500 张/1-2s」的首要结构性成本。改为一次 open +
// 顺序读前 256KB 进内存,EXIF/XMP/尺寸三个消费者共用同一缓冲;缓冲不足以解出结果且
// 文件确实更大时,逐消费者回退原整文件路径(罕见),正确性不折损。

/// 头缓冲上限(256 KB):JPEG 的 EXIF APP1 按规范 ≤64KB 且位于 SOS 之前;XMP Motion Photo
/// 标记扫描窗口为前 128KB(原语义不变);常见格式(PNG/WebP/HEIC)头元数据亦远小于此。
pub const ENRICH_HEADER_BUF: usize = 262_144;

/// 一次 open 读出的文件头缓冲 + 文件总长(用于判断「缓冲是否截断了文件」以决定回退)。
pub struct HeaderBuf {
    pub bytes: Vec<u8>,
    pub file_len: u64,
}

/// 保留头读阶段的文件句柄，供同一项的格式探测/完整回退复用。
///
/// 只在富化两级流水线内部使用；`HeaderBuf` 继续保持轻量值类型，避免影响快扫调用方。
pub struct HeaderRead {
    pub header: HeaderBuf,
    pub file: std::fs::File,
}

/// 单次 open 读前 [`ENRICH_HEADER_BUF`] 字节。失败(不存在/无权限)时调用方按原样处理
/// (与原 per-fn open 失败同语义)。
pub fn read_header_buf(path: &Path) -> std::io::Result<HeaderBuf> {
    read_header_buf_limited(path, ENRICH_HEADER_BUF)
}

/// 按格式选择头缓冲上限(阶段5):
/// - JPEG:131072B——XMP Motion Photo 窗口原语义就是前 128KB,EXIF APP1 ≤64KB;
/// - TIFF/HEIC/HEIF/AVIF:保持 256KB——IFD/moov 可能不在文件首部,减少整文件回退;
/// - 其它常见图像:64KB——尺寸头与元数据通常远小于此,截断时既有回退兜底正确性。
pub fn header_buf_cap_for_ext(ext: &str) -> usize {
    match ext {
        "jpg" | "jpeg" => 131_072,
        "tif" | "tiff" | "heic" | "heif" | "avif" => ENRICH_HEADER_BUF,
        _ => 65_536,
    }
}

/// 按扩展名读头缓冲,语义同 [`read_header_buf`],仅上限不同。
pub fn read_header_buf_for_ext(path: &Path, ext: &str) -> std::io::Result<HeaderBuf> {
    read_header_buf_limited(path, header_buf_cap_for_ext(ext))
}

fn read_header_buf_limited(path: &Path, cap: usize) -> std::io::Result<HeaderBuf> {
    let mut read = read_header_buf_limited_with_file_size(path, cap, None)?;
    if read.header.truncated() {
        // 兼容旧调用方：公开的 HeaderBuf 继续提供精确 file_len；富化新路径使用带已知
        // 文件大小的接口，从而省掉这一趟 metadata()。
        read.header.file_len = std::fs::metadata(path)?.len();
    }
    Ok(read.header)
}

/// 读头并保留句柄；`known_file_size` 来自快扫已入库的文件大小，避免富化阶段再次 stat。
///
/// 多读一个字节用于区分「确实读到 EOF」和「头缓冲刚好读满」。命中上限时保守标记为截断，
/// 即使文件在扫描后被替换/增长，也不会因为过时的尺寸快照而跳过完整回退。
pub fn read_header_buf_for_ext_with_file_size(
    path: &Path,
    ext: &str,
    known_file_size: Option<u64>,
) -> std::io::Result<HeaderRead> {
    read_header_buf_limited_with_file_size(path, header_buf_cap_for_ext(ext), known_file_size)
}

fn read_header_buf_limited_with_file_size(
    path: &Path,
    cap: usize,
    known_file_size: Option<u64>,
) -> std::io::Result<HeaderRead> {
    let mut file = std::fs::File::open(path)?;
    let read_limit = cap.saturating_add(1) as u64;
    let mut bytes = Vec::with_capacity(cap);
    file.by_ref().take(read_limit).read_to_end(&mut bytes)?;
    let hit_limit = bytes.len() > cap;
    if hit_limit {
        bytes.truncate(cap);
    }

    let file_len = if hit_limit {
        // 只要触及 cap+1，就保守认为文件仍有未读内容；已知尺寸只用于给出更大的下界。
        known_file_size
            .unwrap_or(0)
            .max((cap as u64).saturating_add(2))
    } else {
        // read_to_end 已经读到 EOF，此值比额外 metadata() 更准确地反映本次读取到的文件。
        bytes.len() as u64
    };

    Ok(HeaderRead {
        header: HeaderBuf { bytes, file_len },
        file,
    })
}

/// 缓冲是否未覆盖整个文件(EXIF/尺寸解析失败时据此决定是否值得整文件回退)。
impl HeaderBuf {
    fn truncated(&self) -> bool {
        self.file_len > self.bytes.len() as u64
    }
}

/// EXIF 解析采用的路径，用于在一次富化批次结束时输出低频性能摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifParsePath {
    /// 头缓冲直接解析成功。
    HeaderBuffer,
    /// 定位到容器内的 EXIF 块，直接读取载荷并解析，未从头重扫文件。
    ContainerChunk,
    /// 头缓冲不足，从原文件开头解析（有保留句柄时复用）。
    FullFileFallback,
    /// 已确认容器结束且没有 EXIF。
    NoMetadata,
    /// 当前格式不在 kamadak-exif 支持范围内。
    Unsupported,
    /// 头缓冲读取失败，直接走原文件路径。
    DirectFile,
    /// 解析失败，调用方会写入最小元数据行。
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TruncatedExifProbe {
    /// PNG 后置 eXIf 的 TIFF 载荷，可直接交给既有解析器。
    Exif(Vec<u8>),
    /// 已确认没有 EXIF，可以安全跳过整文件回退。
    NoMetadata,
    /// 仍不能判断，必须保留原文件回退以保证元数据不丢失。
    NeedMore,
    /// 已知当前容器不由 EXIF 解析器支持。
    Unsupported,
}

// ── Full EXIF parse (enrichment phase) ───────────────────────────────────────
// ── 完整 EXIF 解析（丰富信息阶段） ───────────────────────────────────────

/// 从图像文件解析完整的 EXIF 元数据。
/// 返回部分填充的 `ImageMeta`（item_id 将由调用者设置）。
pub fn parse_exif_meta(path: &Path) -> Result<ImageMeta> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    parse_exif_meta_reader(&mut reader)
}

fn parse_exif_meta_from_file(file: &mut std::fs::File) -> Result<ImageMeta> {
    file.seek(SeekFrom::Start(0))?;
    let mut reader = BufReader::new(file);
    parse_exif_meta_reader(&mut reader)
}

fn parse_exif_meta_reader<R: BufRead + Seek>(reader: &mut R) -> Result<ImageMeta> {
    let exif = exif::Reader::new()
        .read_from_container(reader)
        .map_err(AppError::from)?;
    Ok(extract_image_meta(&exif))
}

/// 头缓冲版 EXIF 解析。
///
/// 头缓冲失败时不能把所有错误都当成“EXIF 在缓冲外”：JPEG 的 SOS、PNG 的 IEND
/// 和 WebP 的 RIFF 尾部都能证明后续没有 EXIF。只有格式扫描仍不能判断时才回退整文件，
/// 以避免无 EXIF 的大图重复打开并扫描图像数据。
pub fn parse_exif_meta_buf(path: &Path, hb: &HeaderBuf) -> Result<ImageMeta> {
    parse_exif_meta_buf_detailed(path, hb).0
}

/// 与 [`parse_exif_meta_buf`] 相同，但返回本次解析实际采用的路径，供批次级诊断使用。
pub fn parse_exif_meta_buf_detailed(
    path: &Path,
    hb: &HeaderBuf,
) -> (Result<ImageMeta>, ExifParsePath) {
    parse_exif_meta_buf_detailed_inner(path, hb, None, &mut ImageReadContext::default())
}

/// 与 [`parse_exif_meta_buf_detailed`] 相同，但复用头读阶段保留的文件句柄。
pub fn parse_exif_meta_buf_detailed_with_file(
    path: &Path,
    hb: &HeaderBuf,
    file: &mut std::fs::File,
) -> (Result<ImageMeta>, ExifParsePath) {
    parse_exif_meta_buf_detailed_inner(path, hb, Some(file), &mut ImageReadContext::default())
}

pub(crate) fn parse_exif_with_context(
    path: &Path,
    read: Option<&mut HeaderRead>,
    context: &mut ImageReadContext,
) -> (Result<ImageMeta>, ExifParsePath) {
    match read {
        Some(read) => {
            parse_exif_meta_buf_detailed_inner(path, &read.header, Some(&mut read.file), context)
        }
        None => {
            let started = Instant::now();
            let result = context
                .ensure_file_access()
                .and_then(|()| parse_exif_meta(path));
            context.exif_fallback += started.elapsed();
            (result, ExifParsePath::DirectFile)
        }
    }
}

fn parse_exif_meta_buf_detailed_inner(
    path: &Path,
    hb: &HeaderBuf,
    mut source: Option<&mut std::fs::File>,
    context: &mut ImageReadContext,
) -> (Result<ImageMeta>, ExifParsePath) {
    if is_known_exif_unsupported(&hb.bytes) {
        return (Ok(empty_image_meta()), ExifParsePath::Unsupported);
    }

    let mut cursor = std::io::Cursor::new(&hb.bytes[..]);
    let started = Instant::now();
    let parsed = exif::Reader::new().read_from_container(&mut cursor);
    context.exif_memory += started.elapsed();
    match parsed {
        Ok(exif) => (Ok(extract_image_meta(&exif)), ExifParsePath::HeaderBuffer),
        Err(exif::Error::NotFound(_)) if !hb.truncated() => {
            (Ok(empty_image_meta()), ExifParsePath::NoMetadata)
        }
        Err(_) if hb.truncated() => {
            let started = Instant::now();
            let probe = probe_truncated_exif(path, hb, source.as_deref_mut(), context);
            context.exif_probe += started.elapsed();
            match probe {
                Err(error) => (Err(error), ExifParsePath::Failed),
                Ok(TruncatedExifProbe::Exif(data)) => {
                    let started = Instant::now();
                    let meta = exif::Reader::new()
                        .read_raw(data)
                        .map(|exif| extract_image_meta(&exif))
                        .map_err(AppError::from);
                    context.exif_memory += started.elapsed();
                    (meta, ExifParsePath::ContainerChunk)
                }
                Ok(TruncatedExifProbe::NoMetadata) => {
                    (Ok(empty_image_meta()), ExifParsePath::NoMetadata)
                }
                Ok(TruncatedExifProbe::Unsupported) => {
                    (Ok(empty_image_meta()), ExifParsePath::Unsupported)
                }
                Ok(TruncatedExifProbe::NeedMore) => {
                    let started = Instant::now();
                    let result = context.ensure_file_access().and_then(|()| match source {
                        Some(file) => parse_exif_meta_from_file(file),
                        None => parse_exif_meta(path),
                    });
                    context.exif_fallback += started.elapsed();
                    (result, ExifParsePath::FullFileFallback)
                }
            }
        }
        Err(e) => (Err(AppError::from(e)), ExifParsePath::Failed),
    }
}

/// 与原错误路径保持一致：没有 EXIF 或格式不支持时，方向仍写为 1。
fn empty_image_meta() -> ImageMeta {
    ImageMeta {
        orientation: 1,
        ..Default::default()
    }
}

fn probe_truncated_exif(
    path: &Path,
    hb: &HeaderBuf,
    source: Option<&mut std::fs::File>,
    context: &mut ImageReadContext,
) -> Result<TruncatedExifProbe> {
    if is_known_exif_unsupported(&hb.bytes) {
        return Ok(TruncatedExifProbe::Unsupported);
    }
    if hb.bytes.starts_with(&[0xff, 0xd8]) {
        return Ok(jpeg_exif_probe(&hb.bytes));
    }
    if hb.bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        context.ensure_file_access()?;
        return Ok(scan_png_exif(path, hb, source).unwrap_or(TruncatedExifProbe::NeedMore));
    }
    if hb.bytes.len() >= 12 && &hb.bytes[..4] == b"RIFF" && &hb.bytes[8..12] == b"WEBP" {
        context.ensure_file_access()?;
        return Ok(match scan_webp_exif_presence(path, source) {
            Ok(Some(false)) => TruncatedExifProbe::NoMetadata,
            Ok(Some(true)) | Ok(None) | Err(_) => TruncatedExifProbe::NeedMore,
        });
    }
    // TIFF、HEIF/AVIF 和 RAW 保留原文件回退，实际回退前再申请补读名额。
    Ok(TruncatedExifProbe::NeedMore)
}

/// JPEG 在 SOS 之后进入压缩扫描数据，合法 EXIF APP1 只能出现在 SOS 之前。
/// 只扫描头部 marker，不读取压缩图像数据；返回 NeedMore 时才允许整文件回退。
fn jpeg_exif_probe(bytes: &[u8]) -> TruncatedExifProbe {
    if bytes.len() < 2 || !bytes.starts_with(&[0xff, 0xd8]) {
        return TruncatedExifProbe::NeedMore;
    }

    let mut pos = 2usize;
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos] != 0xff {
            pos += 1;
        }
        if pos >= bytes.len() {
            return TruncatedExifProbe::NeedMore;
        }
        while pos < bytes.len() && bytes[pos] == 0xff {
            pos += 1;
        }
        if pos >= bytes.len() {
            return TruncatedExifProbe::NeedMore;
        }

        let marker = bytes[pos];
        pos += 1;
        match marker {
            0x00 => continue,
            0xd9 | 0xda => return TruncatedExifProbe::NoMetadata,
            0xd0..=0xd7 | 0x01 => continue,
            _ => {
                if pos + 2 > bytes.len() {
                    return TruncatedExifProbe::NeedMore;
                }
                let segment_len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                if segment_len < 2 {
                    return TruncatedExifProbe::NeedMore;
                }
                let segment_start = pos + 2;
                let segment_end = match segment_start.checked_add(segment_len - 2) {
                    Some(end) if end <= bytes.len() => end,
                    _ => return TruncatedExifProbe::NeedMore,
                };
                if marker == 0xe1 && bytes[segment_start..segment_end].starts_with(b"Exif\0\0") {
                    // EXIF 已存在但头部解析失败，交给原解析器处理损坏/超长数据。
                    return TruncatedExifProbe::NeedMore;
                }
                pos = segment_end;
            }
        }
    }

    TruncatedExifProbe::NeedMore
}

/// 复用已读头部；头外每块只定位读取 8 字节，不读取 IDAT，不为后置 EXIF 重扫文件。
fn scan_png_exif(
    path: &Path,
    hb: &HeaderBuf,
    source: Option<&mut std::fs::File>,
) -> std::io::Result<TruncatedExifProbe> {
    match source {
        Some(file) => scan_png_exif_chunks(hb, |offset, bytes| read_exact_at(file, offset, bytes)),
        None => {
            let mut file = std::fs::File::open(path)?;
            scan_png_exif_chunks(hb, |offset, bytes| read_exact_at(&mut file, offset, bytes))
        }
    }
}

/// Windows 的 seek_read / Unix 的 read_at 将偏移和读取合成一次请求，避免逐块 seek。
fn read_exact_at(
    file: &mut std::fs::File,
    mut offset: u64,
    mut bytes: &mut [u8],
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(file, bytes, offset);
        #[cfg(not(windows))]
        let read = std::os::unix::fs::FileExt::read_at(file, bytes, offset);
        match read {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => {
                offset += count as u64;
                bytes = &mut bytes[count..];
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn scan_png_exif_chunks(
    hb: &HeaderBuf,
    mut read_at: impl FnMut(u64, &mut [u8]) -> std::io::Result<()>,
) -> std::io::Result<TruncatedExifProbe> {
    let mut read = |offset: u64, bytes: &mut [u8]| {
        let cached = if offset < hb.bytes.len() as u64 {
            let offset = offset as usize;
            let len = bytes.len().min(hb.bytes.len() - offset);
            bytes[..len].copy_from_slice(&hb.bytes[offset..offset + len]);
            len
        } else {
            0
        };
        if cached == bytes.len() {
            return Ok(());
        }
        read_at(offset + cached as u64, &mut bytes[cached..])
    };
    let mut offset = 8u64;
    loop {
        let mut chunk = [0u8; 8];
        read(offset, &mut chunk)?;
        let len = u32::from_be_bytes(chunk[..4].try_into().expect("four-byte length")) as u64;
        match &chunk[4..] {
            b"eXIf" => {
                // 保持单项额外缓冲有界；超大载荷仍交既有文件解析路径，不忽略元数据。
                if len > ENRICH_HEADER_BUF as u64 {
                    return Ok(TruncatedExifProbe::NeedMore);
                }
                let mut data = vec![0; len as usize];
                read(offset + 8, &mut data)?;
                return Ok(TruncatedExifProbe::Exif(data));
            }
            b"IEND" => return Ok(TruncatedExifProbe::NoMetadata),
            _ => {
                offset = offset
                    .checked_add(12 + len)
                    .ok_or(std::io::ErrorKind::InvalidData)?
            }
        }
    }
}

/// 只读取 WebP RIFF chunk 头并 seek 跳过 payload，避免为确认没有 EXIF 而读过图像数据。
fn scan_webp_exif_presence(
    path: &Path,
    source: Option<&mut std::fs::File>,
) -> std::io::Result<Option<bool>> {
    match source {
        Some(file) => scan_webp_exif_presence_file(file),
        None => {
            let mut file = std::fs::File::open(path)?;
            scan_webp_exif_presence_file(&mut file)
        }
    }
}

fn scan_webp_exif_presence_file(file: &mut std::fs::File) -> std::io::Result<Option<bool>> {
    file.seek(SeekFrom::Start(0))?;
    let mut signature = [0u8; 12];
    file.read_exact(&mut signature)?;
    if &signature[..4] != b"RIFF" || &signature[8..12] != b"WEBP" {
        return Ok(None);
    }

    let declared_size =
        u32::from_le_bytes([signature[4], signature[5], signature[6], signature[7]]) as u64;
    let Some(mut remaining) = declared_size.checked_sub(4) else {
        return Ok(None);
    };

    while remaining > 0 {
        if remaining < 8 {
            return Ok(None);
        }
        let mut chunk_header = [0u8; 8];
        file.read_exact(&mut chunk_header)?;
        remaining -= 8;

        let chunk_len = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]) as u64;
        if chunk_len > remaining {
            return Ok(None);
        }
        if &chunk_header[..4] == b"EXIF" {
            return Ok(Some(true));
        }

        file.seek(SeekFrom::Current(chunk_len as i64))?;
        remaining -= chunk_len;
        if chunk_len % 2 == 1 {
            if remaining == 0 {
                return Ok(None);
            }
            file.seek(SeekFrom::Current(1))?;
            remaining -= 1;
        }
    }

    Ok(Some(false))
}

/// 当前 EXIF 库不解析这些容器；继续回退只会得到同一个 Unknown image format。
fn is_known_exif_unsupported(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BM")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"8BPS")
}

/// EXIF 字段提取(parse_exif_meta 与 parse_exif_meta_buf 共享,单一事实源)。
fn extract_image_meta(exif: &exif::Exif) -> ImageMeta {
    let mut meta = ImageMeta::default();

    // Orientation
    // 方向
    if let Some(f) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
        if let exif::Value::Short(ref v) = f.value {
            meta.orientation = v.first().copied().unwrap_or(1) as i64;
        }
    }

    // 日期时间 (原始 → 数字化 → 修改)
    for tag in [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ] {
        if let Some(f) = exif.get_field(tag, exif::In::PRIMARY) {
            if let exif::Value::Ascii(ref v) = f.value {
                if let Some(dt_str) = v.first().and_then(|b| std::str::from_utf8(b).ok()) {
                    if let Some(ts) = parse_exif_datetime(dt_str) {
                        meta.exif_datetime = Some(ts);
                        break;
                    }
                }
            }
        }
    }

    // 相机制造商 / 型号 / 镜头
    meta.exif_make = get_ascii_field(exif, exif::Tag::Make);
    meta.exif_model = get_ascii_field(exif, exif::Tag::Model);
    meta.exif_lens = get_ascii_field(exif, exif::Tag::LensModel);

    // 焦距 (mm)
    if let Some(f) = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY) {
        meta.exif_focal_length = rational_to_f64(&f.value);
    }

    // 光圈 (F 值)
    if let Some(f) = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY) {
        meta.exif_aperture = rational_to_f64(&f.value);
    }

    // 快门速度 (ExposureTime 作为 "1/200" 字符串)
    if let Some(f) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
        if let exif::Value::Rational(ref v) = f.value {
            if let Some(r) = v.first() {
                meta.exif_shutter = Some(format!("{}/{}", r.num, r.denom));
            }
        }
    }

    // ISO
    // ISO
    if let Some(f) = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY) {
        if let exif::Value::Short(ref v) = f.value {
            meta.exif_iso = v.first().copied().map(|n| n as i64);
        }
    }

    // GPS
    // GPS
    if let (Some(lat), Some(lat_ref), Some(lng), Some(lng_ref)) = (
        exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY),
    ) {
        if let (Some(lat_dd), Some(lng_dd)) =
            (dms_to_decimal(&lat.value), dms_to_decimal(&lng.value))
        {
            let lat_sign =
                if get_ascii_field(exif, exif::Tag::GPSLatitudeRef).as_deref() == Some("S") {
                    -1.0
                } else {
                    1.0
                };
            let lng_sign =
                if get_ascii_field(exif, exif::Tag::GPSLongitudeRef).as_deref() == Some("W") {
                    -1.0
                } else {
                    1.0
                };
            meta.exif_gps_lat = Some(lat_dd * lat_sign);
            meta.exif_gps_lng = Some(lng_dd * lng_sign);
        }
        let _ = (lat_ref, lng_ref); // suppress unused warning
                                    // 抑制未使用警告
    }

    meta
}

// ── XMP Motion Photo detection ────────────────────────────────────────────────
// ── XMP 动态照片检测 ────────────────────────────────────────────────

/// 扫描 JPEG 的前 128 KB 以寻找 XMP 动态照片标记。
/// 返回 `(is_live_photo, has_embedded_video)`。
pub fn detect_motion_photo_xmp(path: &Path) -> (bool, bool) {
    let Ok(file) = std::fs::File::open(path) else {
        return (false, false);
    };
    use std::io::Read;
    // 读满 128 KB(2026-07-06 审查 R10):`Read::read` 不保证一次读满(网络盘/特殊文件系统上
    // 短读合法),XMP 标记落在未读部分即漏检 → Motion Photo 不识别、mp4 伴随不配对。用 take
    // + read_to_end 循环读满(或读到 EOF)。
    let mut buf = Vec::with_capacity(131_072);
    if file.take(131_072).read_to_end(&mut buf).is_err() {
        return (false, false);
    }
    motion_photo_markers(&buf)
}

/// 头缓冲版 Motion Photo 检测(#10):扫描窗口仍取前 128KB(原语义逐字保留,缓冲 256KB ⊇ 窗口)。
pub fn detect_motion_photo_xmp_buf(hb: &HeaderBuf) -> (bool, bool) {
    let window = &hb.bytes[..hb.bytes.len().min(131_072)];
    motion_photo_markers(window)
}

/// 标记匹配单一事实源(文件版/缓冲版共享)。
///
/// 阶段5:改为纯字节搜索,不再 `String::from_utf8_lossy`(原实现每个 JPEG 分配最多
/// 128KB 临时字符串)。标记全部为 ASCII,UTF-8 合法片段在字节层与字符串层等价。
/// 2026-08-23(吸收对照线):滑窗改「首字节定位 + 整串比对」——128KB 窗口上远比
/// `windows()` 全程逐位滑窗廉价,语义不变(由既有 lossy 等价性测试锁死)。
fn motion_photo_markers(buf: &[u8]) -> (bool, bool) {
    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() {
            return false;
        }
        let mut from = 0;
        while let Some(pos) = haystack[from..].iter().position(|&b| b == needle[0]) {
            let start = from + pos;
            if haystack[start..].starts_with(needle) {
                return true;
            }
            from = start + 1;
        }
        false
    }

    // Google 动态照片标记
    let google = contains_bytes(buf, b"GCamera:MotionPhoto=\"1\"")
        || contains_bytes(buf, b"Camera:MotionPhoto=\"1\"")
        || contains_bytes(buf, b"MotionPhoto=\"1\"");

    // 三星动态照片标记
    let samsung = contains_bytes(buf, b"MotionPhoto_Capture_Type")
        || contains_bytes(buf, b"com.samsung.android.photo");

    (google || samsung, google || samsung)
}

// ── Helpers ───────────────────────────────────────────────────────────────────
// ── 辅助函数 ───────────────────────────────────────────────────────────────────

fn get_ascii_field(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    exif.get_field(tag, exif::In::PRIMARY).and_then(|f| {
        if let exif::Value::Ascii(ref v) = f.value {
            v.first()
                .and_then(|b| std::str::from_utf8(b).ok())
                .map(|s| s.trim_end_matches('\0').trim().to_string())
        } else {
            None
        }
    })
}

fn rational_to_f64(value: &exif::Value) -> Option<f64> {
    if let exif::Value::Rational(ref v) = value {
        v.first().map(|r| r.num as f64 / r.denom as f64)
    } else {
        None
    }
}

fn dms_to_decimal(value: &exif::Value) -> Option<f64> {
    if let exif::Value::Rational(ref v) = value {
        if v.len() >= 3 {
            let deg = v[0].num as f64 / v[0].denom as f64;
            let min = v[1].num as f64 / v[1].denom as f64;
            let sec = v[2].num as f64 / v[2].denom as f64;
            return Some(deg + min / 60.0 + sec / 3600.0);
        }
    }
    None
}

/// 将 EXIF 日期时间字符串 (`"2024:03:15 10:30:00"`) 解析为 Unix 时间戳。
fn parse_exif_datetime(s: &str) -> Option<i64> {
    // Format: "YYYY:MM:DD HH:MM:SS"
    // 格式: "YYYY:MM:DD HH:MM:SS"
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    // 用 `str::get` 按字节区间取子串(2026-07-06 审查 R3):畸形相机可能写入多字节字符,
    // 直接 `s[0..4]` 索引落在非 char 边界会 panic;该函数跑在 enrichment 的 par_iter 里,
    // 单个坏 EXIF 会经 rayon 传播放倒整根补全。`get` 越界/非边界返回 None,安全降级。
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;

    // 简单的 UTC 时间戳（忽略时区）
    use chrono::{TimeZone, Utc};
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .map(|dt| dt.timestamp())
}
