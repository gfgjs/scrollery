//! worker 内的 VPL JPEG 路由；官方 packed ABI 仅在 C++ 薄桥内使用。

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::ptr::NonNull;

use crate::thumbnail::route_diagnostics::{record, Attempt, Reason, Stage};
use image::ImageDecoder;

use super::d2d_resize::{acquire_image_slot, GpuAdapter};
use crate::engine::image_rs::apply_exif_orientation;
use crate::engine::native::wic_engine::with_wic_factory;
use crate::engine::traits::DecodedImage;
use crate::error::{AppError, Result};
use crate::scanner::metadata::read_jpeg_orientation_file;

const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PIXELS_BYTES: u64 = 64 * 1024 * 1024;

#[repr(C)]
#[derive(Default, Debug)]
struct BridgeResult {
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    header_status: i32,
    query_status: i32,
    init_status: i32,
    decode_status: i32,
    sync_status: i32,
    init_us: u64,
    decode_us: u64,
    vpp_us: u64,
    sync_us: u64,
}

#[repr(C)]
#[derive(Default)]
struct OpenResult {
    stage: u32,
    status: i32,
}

extern "C" {
    fn scrollery_vpl_create(
        low: u32,
        high: i32,
        vendor: u32,
        device: u32,
        result: *mut OpenResult,
    ) -> *mut c_void;
    fn scrollery_vpl_destroy(session: *mut c_void);
    fn scrollery_vpl_healthy(session: *mut c_void) -> i32;
    fn scrollery_vpl_decode(
        session: *mut c_void,
        jpeg: *mut u8,
        length: u32,
        edge: u32,
        rgba: *mut u8,
        capacity: u32,
        result: *mut BridgeResult,
    ) -> i32;
}

struct Session {
    handle: NonNull<c_void>,
    adapter: GpuAdapter,
    healthy: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: 由薄桥创建的独占句柄，只在所属处理线程释放一次。
        unsafe { scrollery_vpl_destroy(self.handle.as_ptr()) };
    }
}

thread_local! {
    static ABANDONED_WORK: Cell<bool> = const { Cell::new(false) };
    static SESSIONS: RefCell<Option<Vec<Session>>> = const { RefCell::new(None) };
}

/// 同步未完成时，worker 必须退出，由宿主确认退出后释放 GPU 名额。
pub fn has_abandoned_work() -> bool {
    ABANDONED_WORK.get()
}

pub(crate) fn clear_thread_sessions() {
    drop(SESSIONS.take());
}

fn sessions() -> Vec<Session> {
    crate::video::d3d::hardware_adapters()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, desc)| {
            let adapter = GpuAdapter {
                vendor_id: desc.VendorId,
                device_id: desc.DeviceId,
                luid_high: desc.AdapterLuid.HighPart,
                luid_low: desc.AdapterLuid.LowPart,
            };
            let mut result = OpenResult::default();
            // SAFETY: 标量和可写结果的布局与 bridge.h 一致，无借用跨调用留存。
            let handle = unsafe {
                scrollery_vpl_create(
                    adapter.luid_low,
                    adapter.luid_high,
                    adapter.vendor_id,
                    adapter.device_id,
                    &mut result,
                )
            };
            tracing::debug!(target: "scrollery::thumb_perf", vendor_id = adapter.vendor_id,
            device_id = adapter.device_id, stage = result.stage, status = result.status,
            available = !handle.is_null(), "VPL JPEG session probe");
            NonNull::new(handle).map(|handle| Session {
                handle,
                adapter,
                healthy: true,
            })
        })
        .collect()
}

/// 从宿主授权句柄读取 JPEG，完整硬件路径优先于 CPU 解码加 D2D。
/// 不适用、繁忙、部分加速均交由调用方走既有后端；成功输出保留 ICC 并只转正一次。
pub fn decode_open_file(
    mut file: File,
    long_edge: u32,
    max_pixel_bytes: u64,
) -> Result<Option<(DecodedImage, GpuAdapter)>> {
    let input_limit = MAX_INPUT_BYTES.min(max_pixel_bytes);
    let mut attempt = Attempt::new(Stage::Vpl);
    let input_len = file.metadata().map_err(AppError::Io)?.len();
    if !(1..=2048).contains(&long_edge)
        || input_len > input_limit
        || u64::from(long_edge) * u64::from(long_edge) * 4 > max_pixel_bytes
    {
        attempt.reason = Reason::TargetLimit;
        return Ok(None);
    }
    with_wic_factory(|_| {
        SESSIONS.with_borrow_mut(|stored| {
        let sessions = stored.get_or_insert_with(sessions);
        // 能力/健康和槽位均在读源前检查；繁忙不写入负能力缓存。
        if !sessions.iter().any(|session| session.healthy) { attempt.reason = Reason::NoSession; return Ok(None); }
        let mut candidates = sessions.iter_mut().filter(|session| session.healthy)
            .filter_map(|session| acquire_image_slot(session.adapter).map(|slot| (session, slot)));
        let Some(first) = candidates.next() else { attempt.reason = Reason::Busy; return Ok(None); };
        let probe_started = std::time::Instant::now();
        file.rewind().map_err(AppError::Io)?;
        // 头部探测最多读取 256 KiB；超长元数据或不适用源交给常规解码。
        let mut header = Vec::new();
        (&mut file).take(256 * 1024).read_to_end(&mut header).map_err(AppError::Io)?;
        let Ok(mut decoder) = image::codecs::jpeg::JpegDecoder::new(Cursor::new(&header)) else {
            record(Stage::Probe, Reason::Header, probe_started.elapsed());
            attempt.reason = Reason::Header;
            return Ok(None);
        };
        let (width, height) = decoder.dimensions();
        if width.max(height) <= long_edge || width.max(height) > 8192
            || u64::from(width) * u64::from(height) * 4 > MAX_PIXELS_BYTES.min(max_pixel_bytes) {
            record(Stage::Probe, Reason::SourceLimit, probe_started.elapsed());
            attempt.reason = Reason::SourceLimit;
            return Ok(None);
        }
        let icc = decoder.icc_profile().map_err(AppError::Engine)?;
        drop(decoder);
        drop(header);
        record(Stage::Probe, Reason::Success, probe_started.elapsed());
        let orientation = read_jpeg_orientation_file(&mut file);
        file.rewind().map_err(AppError::Io)?;
        let read_started = std::time::Instant::now();
        let mut bytes = vec![0; input_len as usize];
        file.read_exact(&mut bytes).map_err(AppError::Io)?;
        record(Stage::Read, Reason::Success, read_started.elapsed());
        for (session, _slot) in std::iter::once(first).chain(candidates) {
            let mut pixels = vec![0; long_edge as usize * long_edge as usize * 4];
            let mut result = BridgeResult::default();
            // SAFETY: 输入/输出都是独占且长度有界的连续缓冲；薄桥不留存指针。
            let status = unsafe { scrollery_vpl_decode(session.handle.as_ptr(), bytes.as_mut_ptr(),
                bytes.len() as u32, long_edge, pixels.as_mut_ptr(), pixels.len() as u32, &mut result) };
            // SAFETY: 与 decode 同一线程上的存活会话，读取桥的最终健康事实。
            session.healthy = unsafe { scrollery_vpl_healthy(session.handle.as_ptr()) } != 0;
            for (stage, micros) in [(Stage::Init, result.init_us), (Stage::Decode, result.decode_us),
                (Stage::Vpp, result.vpp_us), (Stage::Sync, result.sync_us)] {
                if micros > 0 { record(stage, if status == 0 { Reason::Success } else { Reason::Failed }, std::time::Duration::from_micros(micros)); }
            }
            if result.sync_us > 0 && result.sync_status != 0 {
                ABANDONED_WORK.set(true);
                attempt.reason = Reason::SyncFailed;
                return Ok(None);
            }
            if status != 0 {
                attempt.reason = if result.sync_us > 0 && result.sync_status != 0 { Reason::SyncFailed }
                    else if !session.healthy { Reason::DeviceLost } else { Reason::Unsupported };
                tracing::debug!(target: "scrollery::thumb_perf", status, ?result, "VPL JPEG route declined");
                continue;
            }
            if result.source_width != width || result.source_height != height || result.width == 0 ||
                result.height == 0 || result.width > long_edge || result.height > long_edge {
                session.healthy = false;
                return Err(AppError::Internal("VPL JPEG output dimensions mismatch".into()));
            }
            pixels.truncate(result.width as usize * result.height as usize * 4);
            let image = image::RgbaImage::from_raw(result.width, result.height, pixels)
                .ok_or_else(|| AppError::Internal("VPL JPEG RGBA buffer mismatch".into()))?;
            let image = apply_exif_orientation(image::DynamicImage::ImageRgba8(image), orientation);
            attempt.reason = Reason::Success;
            return Ok(Some((DecodedImage { width: image.width(), height: image.height(),
                pixels: image.into_rgba8().into_raw(), icc }, session.adapter)));
        }
        Ok(None)
    })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_unavailable_skips_source_reads() {
        let mut source = tempfile::tempfile().unwrap();
        source.set_len(1024 * 1024).unwrap();
        let previous = SESSIONS.replace(Some(Vec::new()));
        with_wic_factory(|_| {
            for _ in 0..32 {
                assert!(
                    decode_open_file(source.try_clone().unwrap(), 256, MAX_PIXELS_BYTES)?.is_none()
                );
                assert_eq!(source.stream_position().unwrap(), 0);
            }
            Ok(())
        })
        .unwrap();
        SESSIONS.replace(previous);
    }
}
