// src-tauri/src/video/mf_attrs.rs
//! Media Foundation 属性/编解码器辅助函数（尾段抽出，纯 GUID/PROPVARIANT 解析，与
//! `Session`/`ReaderCallback` 等异步回调核心无引用关系）。
//!
//! Attribute/codec helper functions extracted from `media_foundation.rs` — pure GUID/PROPVARIANT
//! parsing with no reference into the `Session` async-callback core.

use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PropVariantToInt64;

/// 读取打包的 (宽, 高) 属性（如 `MF_MT_FRAME_SIZE`）。
pub(super) unsafe fn attr_size(mt: &IMFMediaType, key: &GUID) -> Option<(u32, u32)> {
    // 帧尺寸为 u64 属性：高 32 位 = 宽，低 32 位 = 高。
    let packed = mt.GetUINT64(key).ok()?;
    Some(((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32))
}

/// 读取打包的 (分子, 分母) 比率属性（如 `MF_MT_FRAME_RATE`）。
pub(super) unsafe fn attr_ratio(mt: &IMFMediaType, key: &GUID) -> Option<(u32, u32)> {
    let packed = mt.GetUINT64(key).ok()?;
    Some(((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32))
}

/// 通过媒体源的 `MF_PD_DURATION`（100 纳秒 VT_UI8 PROPVARIANT）取总时长（毫秒）。
pub(super) unsafe fn read_duration_ms(reader: &IMFSourceReader) -> u64 {
    match reader.GetPresentationAttribute(super::media_foundation::MEDIASOURCE, &MF_PD_DURATION) {
        Ok(pv) => {
            let v = PropVariantToInt64(&pv).unwrap_or(0);
            if v > 0 {
                (v as u64) / 10_000
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// 尽力将视频子类型 GUID 映射为简短编解码标签。
pub(super) fn codec_label(subtype: GUID) -> Option<String> {
    let name = if subtype == MFVideoFormat_H264 {
        "H264"
    } else if subtype == MFVideoFormat_HEVC || subtype == MFVideoFormat_HEVC_ES {
        "HEVC"
    } else if subtype == MFVideoFormat_MPEG2 {
        "MPEG2"
    } else if subtype == MFVideoFormat_MP4V {
        "MPEG4"
    } else if subtype == MFVideoFormat_WMV3 {
        "WMV3"
    } else if subtype == MFVideoFormat_WVC1 {
        "VC1"
    } else {
        return None;
    };
    Some(name.to_string())
}

/// Clamp the raw `MF_MT_VIDEO_ROTATION` to {0, 90, 180, 270}.
/// 将原始 `MF_MT_VIDEO_ROTATION` 归一到 {0, 90, 180, 270}。
pub(super) fn normalize_rotation(raw: u32) -> i32 {
    match raw {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    }
}
