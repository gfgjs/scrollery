//! 原生缩略图子进程的有界 stdio 协议；请求不携带可自行选取的输出路径。

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use super::generator::{EncodedThumbPayload, MAX_ENCODED_ARTIFACT_BYTES};

pub const WORKER_HELLO: [u8; 4] = *b"NTHD";
/// 两份最终产物及小型任务数据的预留；剩余工作集供解码/变换的同时存活缓冲共享。
pub const RESULT_RESERVATION_BYTES: u64 = 36 * 1024 * 1024;

/// 从宿主已持有的工作集折算单像素缓冲上限，留出四份同时存活的缓冲空间。
pub fn pixel_limit_for_reservation(bytes: u64) -> u64 {
    (bytes.saturating_sub(RESULT_RESERVATION_BYTES) / 4).min(128 * 1024 * 1024)
}
const MAX_REQUEST_BYTES: usize = 4096;
const MAX_HASH_BYTES: usize = 1024;
const MAX_STAGE_US: u64 = 60_000_000;

#[derive(Serialize, Deserialize)]
pub enum NativeKind {
    Image,
    VideoCover,
}

#[derive(Serialize, Deserialize)]
pub struct NativeRequest {
    pub id: u64,
    /// `DuplicateHandle` 复制到子进程后的句柄值，子进程独占并关闭。
    pub file_handle: u64,
    pub kind: NativeKind,
    pub format: String,
    pub codec_hint: Option<String>,
    /// 图片允许 GPU 变换；视频允许硬件会话。false 时视频不枚举或绑定 D3D 设备。
    pub prefer_gpu: bool,
    pub prefer_system_codec: bool,
    pub decode_long_edge: u32,
    /// 宿主根据已取得的共享工作集发放；worker 按真实尺寸检查，不信任数据库尺寸。
    pub max_pixel_bytes: u64,
    pub output_size: u32,
    pub webp_quality: u8,
    pub ai_cache_short_edge: u32,
    pub emit_ai_cache: bool,
    pub qos_foreground: bool,
    pub qos_revision: u64,
}

/// 子进程确认的实际处理后端；VideoMfHardwareMft 表示确认硬件 MFT，VideoMf 的加速程度未知。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum NativeBackend {
    EmbeddedJpeg = 1,
    ImageD2d = 2,
    ImageWic = 3,
    ImageRs = 4,
    VideoMf = 5,
    VideoMfHardwareMft = 6,
    /// VPL 的 JPEG decode 与同设备 VPP 全部无警告成功。
    ImageVpl = 7,
}

impl NativeBackend {
    fn from_byte(value: u8) -> io::Result<Self> {
        match value {
            1 => Ok(Self::EmbeddedJpeg),
            2 => Ok(Self::ImageD2d),
            3 => Ok(Self::ImageWic),
            4 => Ok(Self::ImageRs),
            5 => Ok(Self::VideoMf),
            6 => Ok(Self::VideoMfHardwareMft),
            7 => Ok(Self::ImageVpl),
            _ => Err(invalid_data("invalid native thumbnail backend")),
        }
    }
}

/// 随有界结果回传的设备标识；D2D 是实际变换设备，MF 是候选设备，后端枚举区分确认程度。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeExecution {
    pub backend: NativeBackend,
    pub vendor_id: u32,
    pub device_id: u32,
    pub luid_high: i32,
    pub luid_low: u32,
    pub adapter_kind: NativeAdapterKind,
}

/// 操作系统报告的实际图像设备类型；查询不可用或仅有候选设备时保持未知。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum NativeAdapterKind {
    #[default]
    Unknown = 0,
    Integrated = 1,
    Discrete = 2,
}

impl NativeExecution {
    /// 构造没有已确认图像 GPU 变换的处理结果。
    pub fn cpu(backend: NativeBackend) -> Self {
        Self {
            backend,
            vendor_id: 0,
            device_id: 0,
            luid_high: 0,
            luid_low: 0,
            adapter_kind: NativeAdapterKind::Unknown,
        }
    }

    /// 在隔离 worker 内补全实际图像设备类型；MF 候选不提升为已确认设备。
    #[cfg(windows)]
    pub fn resolve_adapter_kind(&mut self) {
        if matches!(
            self.backend,
            NativeBackend::ImageD2d | NativeBackend::ImageVpl
        ) {
            self.adapter_kind = super::native_adapter::classify(self.luid_high, self.luid_low);
        }
    }
}

/// 子进程内阶段耗时（微秒）。0 表示该阶段不适用；解码接口内部不可拆分时合并计时。
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTimings {
    pub decode_transform_us: u64,
    pub encode_hash_us: u64,
    pub embedded_jpeg_combined_us: u64,
}

/// 原生处理线程对本次 QoS revision 的应答；仅 attempted 表示本次实际调用了系统接口。
#[derive(Clone, Copy, Debug)]
pub struct NativeQosAck {
    pub worker_slot: u8,
    pub sequence: u64,
    pub revision: u64,
    pub applied: bool,
    pub attempted: bool,
}

pub enum NativeResponse {
    Ok {
        id: u64,
        payload: EncodedThumbPayload,
        execution: NativeExecution,
        timings: NativeTimings,
        qos: NativeQosAck,
    },
    Failed {
        id: u64,
        code: u8,
        qos: NativeQosAck,
    },
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn write_request(mut writer: impl Write, request: &NativeRequest) -> io::Result<()> {
    let json = serde_json::to_vec(request).map_err(io::Error::other)?;
    if json.len() > MAX_REQUEST_BYTES {
        return Err(invalid_data("native thumbnail request exceeds limit"));
    }
    writer.write_all(&(json.len() as u32).to_le_bytes())?;
    writer.write_all(&json)?;
    writer.flush()
}

pub fn read_request(mut reader: impl Read) -> io::Result<Option<NativeRequest>> {
    let mut length = [0u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_REQUEST_BYTES {
        return Err(invalid_data("invalid native thumbnail request length"));
    }
    let mut json = vec![0; length];
    reader.read_exact(&mut json)?;
    serde_json::from_slice(&json)
        .map(Some)
        .map_err(|_| invalid_data("invalid native thumbnail request"))
}

pub fn write_response(mut writer: impl Write, response: &NativeResponse) -> io::Result<()> {
    match response {
        NativeResponse::Ok {
            id,
            payload,
            execution,
            timings,
            qos,
        } => {
            let hash = payload.thumbhash.as_deref().unwrap_or_default();
            let ai = payload.ai_cache.as_deref().unwrap_or_default();
            if payload.webp.len() > MAX_ENCODED_ARTIFACT_BYTES
                || ai.len() > MAX_ENCODED_ARTIFACT_BYTES
                || hash.len() > MAX_HASH_BYTES
            {
                return Err(invalid_data("native thumbnail response exceeds limit"));
            }
            writer.write_all(&[0])?;
            writer.write_all(&id.to_le_bytes())?;
            writer.write_all(&qos.revision.to_le_bytes())?;
            writer.write_all(&[u8::from(qos.applied) | (u8::from(qos.attempted) << 1)])?;
            writer.write_all(&[qos.worker_slot])?;
            writer.write_all(&qos.sequence.to_le_bytes())?;
            writer.write_all(&[execution.backend as u8])?;
            writer.write_all(&execution.vendor_id.to_le_bytes())?;
            writer.write_all(&execution.device_id.to_le_bytes())?;
            writer.write_all(&execution.luid_high.to_le_bytes())?;
            writer.write_all(&execution.luid_low.to_le_bytes())?;
            writer.write_all(&[execution.adapter_kind as u8])?;
            writer.write_all(&timings.decode_transform_us.to_le_bytes())?;
            writer.write_all(&timings.encode_hash_us.to_le_bytes())?;
            writer.write_all(&timings.embedded_jpeg_combined_us.to_le_bytes())?;
            writer.write_all(&(payload.webp.len() as u32).to_le_bytes())?;
            writer.write_all(&(hash.len() as u16).to_le_bytes())?;
            writer.write_all(&(ai.len() as u32).to_le_bytes())?;
            writer.write_all(&payload.webp)?;
            writer.write_all(hash)?;
            writer.write_all(ai)?;
        }
        NativeResponse::Failed { id, code, qos } => {
            writer.write_all(&[*code])?;
            writer.write_all(&id.to_le_bytes())?;
            writer.write_all(&qos.revision.to_le_bytes())?;
            writer.write_all(&[u8::from(qos.applied) | (u8::from(qos.attempted) << 1)])?;
            writer.write_all(&[qos.worker_slot])?;
            writer.write_all(&qos.sequence.to_le_bytes())?;
        }
    }
    writer.flush()
}

pub fn read_response(mut reader: impl Read) -> io::Result<NativeResponse> {
    let mut status = [0u8; 1];
    reader.read_exact(&mut status)?;
    let mut id = [0u8; 8];
    reader.read_exact(&mut id)?;
    let id = u64::from_le_bytes(id);
    let mut qos_bytes = [0u8; 18];
    reader.read_exact(&mut qos_bytes)?;
    if qos_bytes[8] > 3 || !(1..=4).contains(&qos_bytes[9]) {
        return Err(invalid_data("invalid native thumbnail QoS acknowledgement"));
    }
    let qos = NativeQosAck {
        worker_slot: qos_bytes[9],
        sequence: u64::from_le_bytes(qos_bytes[10..18].try_into().unwrap()),
        revision: u64::from_le_bytes(qos_bytes[0..8].try_into().unwrap()),
        applied: qos_bytes[8] & 1 != 0,
        attempted: qos_bytes[8] & 2 != 0,
    };
    if qos.sequence == 0 {
        return Err(invalid_data("invalid native thumbnail QoS sequence"));
    }
    if status[0] != 0 {
        return Ok(NativeResponse::Failed {
            id,
            code: status[0],
            qos,
        });
    }
    let mut execution_bytes = [0u8; 18];
    reader.read_exact(&mut execution_bytes)?;
    let execution = NativeExecution {
        backend: NativeBackend::from_byte(execution_bytes[0])?,
        vendor_id: u32::from_le_bytes(execution_bytes[1..5].try_into().unwrap()),
        device_id: u32::from_le_bytes(execution_bytes[5..9].try_into().unwrap()),
        luid_high: i32::from_le_bytes(execution_bytes[9..13].try_into().unwrap()),
        luid_low: u32::from_le_bytes(execution_bytes[13..17].try_into().unwrap()),
        adapter_kind: match execution_bytes[17] {
            0 => NativeAdapterKind::Unknown,
            1 => NativeAdapterKind::Integrated,
            2 => NativeAdapterKind::Discrete,
            _ => return Err(invalid_data("invalid native thumbnail adapter kind")),
        },
    };
    let mut timing_bytes = [0u8; 24];
    reader.read_exact(&mut timing_bytes)?;
    let timings = NativeTimings {
        decode_transform_us: u64::from_le_bytes(timing_bytes[0..8].try_into().unwrap()),
        encode_hash_us: u64::from_le_bytes(timing_bytes[8..16].try_into().unwrap()),
        embedded_jpeg_combined_us: u64::from_le_bytes(timing_bytes[16..24].try_into().unwrap()),
    };
    if timings.decode_transform_us > MAX_STAGE_US
        || timings.encode_hash_us > MAX_STAGE_US
        || timings.embedded_jpeg_combined_us > MAX_STAGE_US
        || (timings.embedded_jpeg_combined_us > 0
            && (timings.decode_transform_us > 0 || timings.encode_hash_us > 0))
    {
        return Err(invalid_data("invalid native thumbnail stage timing"));
    }
    let mut webp_len = [0u8; 4];
    let mut hash_len = [0u8; 2];
    let mut ai_len = [0u8; 4];
    reader.read_exact(&mut webp_len)?;
    reader.read_exact(&mut hash_len)?;
    reader.read_exact(&mut ai_len)?;
    let webp_len = u32::from_le_bytes(webp_len) as usize;
    let hash_len = u16::from_le_bytes(hash_len) as usize;
    let ai_len = u32::from_le_bytes(ai_len) as usize;
    if webp_len == 0
        || webp_len > MAX_ENCODED_ARTIFACT_BYTES
        || hash_len > MAX_HASH_BYTES
        || ai_len > MAX_ENCODED_ARTIFACT_BYTES
    {
        return Err(invalid_data("invalid native thumbnail response length"));
    }
    let mut webp = vec![0; webp_len];
    let mut hash = vec![0; hash_len];
    let mut ai = vec![0; ai_len];
    reader.read_exact(&mut webp)?;
    reader.read_exact(&mut hash)?;
    reader.read_exact(&mut ai)?;
    Ok(NativeResponse::Ok {
        id,
        execution,
        timings,
        qos,
        payload: EncodedThumbPayload {
            webp,
            thumbhash: (hash_len > 0).then_some(hash),
            ai_cache: (ai_len > 0).then_some(ai),
        },
    })
}
