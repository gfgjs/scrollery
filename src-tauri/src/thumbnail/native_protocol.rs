//! 图片隔离域协议：仅授权句柄进入，编码字节返回；帧和产物长度均有上限。

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub(crate) const HELLO: [u8; 8] = *b"SCIM0004";
pub(crate) const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 4096;

/// 旧配置值保留；协议明确表达解码器顺序，避免 worker 忽略用户策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NativeDecodePolicy {
    NativeFirst,
    ImageRsFirst,
}

impl NativeDecodePolicy {
    pub(crate) fn from_strategy(strategy: &str) -> Self {
        if strategy == "cpu" {
            Self::ImageRsFirst
        } else {
            Self::NativeFirst
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NativeRequest {
    pub id: u64,
    pub file_handle: u64,
    pub format: String,
    pub decode_policy: NativeDecodePolicy,
    pub output_size: u32,
    pub decode_long_edge: u32,
    pub webp_quality: u8,
    pub ai_cache_short_edge: u32,
    pub emit_ai: bool,
    pub emit_display: bool,
    /// 调用方按本次产物尺寸估算的编码载荷上界；宿主读取载荷前核验。
    pub max_encoded_bytes: u64,
    pub max_pixel_bytes: u64,
    pub foreground: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NativeFailure {
    Content,
    Unsupported,
    Resource,
    Unavailable,
    Cancelled,
}

// 完成事件沿现有队列按值移交，避免每张图片因可选度量再增加一次堆分配。
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum NativeEvent {
    /// CopyPixels/软件解码完成，worker 已关闭源句柄并移交小图。
    Decoded { id: u64 },
    Finished {
        id: u64,
        failure: Option<NativeFailure>,
        backend: String,
        decode_us: u64,
        encode_us: u64,
        display_len: usize,
        ai_len: usize,
        thumbhash: Option<Vec<u8>>,
        #[serde(default)]
        stats: NativeStats,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gpu: Option<NativeGpuMetrics>,
    },
}

/// GPU 实验的解码子阶段及一次性设备初始化；硬件时间戳缺失时保留 None。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NativeGpuMetrics {
    pub cpu_planar_us: u64,
    pub queue_us: u64,
    pub upload_us: u64,
    pub gpu_us: Option<u64>,
    pub readback_us: u64,
    pub completion_wait_us: u64,
    pub init_us: u64,
    pub adapter: Option<String>,
    pub fallback: Option<String>,
    pub reserved_peak_bytes: u64,
    pub usage_peak_bytes: u64,
    pub budget_bytes: u64,
}

/// 当前执行域的累计峰值；宿主跨域取最大值，不能对不同阶段峰值求和冒充同时占用。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NativeStats {
    pub decode_active_peak: u64,
    pub encode_active_peak: u64,
    pub decoded_queue_peak: u64,
    pub working_bytes_peak: u64,
    pub encoded_bytes_peak: u64,
}

impl NativeStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.decode_active_peak = self.decode_active_peak.max(other.decode_active_peak);
        self.encode_active_peak = self.encode_active_peak.max(other.encode_active_peak);
        self.decoded_queue_peak = self.decoded_queue_peak.max(other.decoded_queue_peak);
        self.working_bytes_peak = self.working_bytes_peak.max(other.working_bytes_peak);
        self.encoded_bytes_peak = self.encoded_bytes_peak.max(other.encoded_bytes_peak);
    }
}

pub(crate) struct NativeReply {
    pub event: NativeEvent,
    pub display: Vec<u8>,
    pub ai: Vec<u8>,
}

pub(crate) fn write_request(out: &mut impl Write, request: &NativeRequest) -> io::Result<()> {
    write_header(out, request)
}

pub(crate) fn read_request(input: &mut impl Read) -> io::Result<NativeRequest> {
    let request: NativeRequest = read_header(input)?;
    if request.file_handle == 0
        || request.format.len() > 32
        || request.output_size == 0
        || request.output_size > 1024
        || request.decode_long_edge < request.output_size
        || request.ai_cache_short_edge == 0
        || !(1..=100).contains(&request.webp_quality)
        || request.max_pixel_bytes == 0
        || request.max_encoded_bytes > MAX_ARTIFACT_BYTES as u64
        || ((request.emit_display || request.emit_ai) && request.max_encoded_bytes == 0)
    {
        return Err(invalid("invalid image request"));
    }
    Ok(request)
}

pub(crate) fn write_reply(out: &mut impl Write, reply: &NativeReply) -> io::Result<()> {
    validate(&reply.event, reply.display.len(), reply.ai.len())?;
    write_header(out, &reply.event)?;
    out.write_all(&reply.display)?;
    out.write_all(&reply.ai)?;
    out.flush()
}

#[cfg(test)]
pub(crate) fn read_reply(input: &mut impl Read) -> io::Result<NativeReply> {
    read_reply_with_limit(input, |_| Ok(MAX_ARTIFACT_BYTES))
}

/// 先解析有界小头，再按可信 pending 解析本请求上界；禁止先分配载荷再查额度。
pub(crate) fn read_reply_with_limit(
    input: &mut impl Read,
    mut resolve_limit: impl FnMut(&NativeEvent) -> io::Result<usize>,
) -> io::Result<NativeReply> {
    let event: NativeEvent = read_header(input)?;
    let (display_len, ai_len) = match &event {
        NativeEvent::Decoded { .. } => (0, 0),
        NativeEvent::Finished {
            display_len,
            ai_len,
            ..
        } => (*display_len, *ai_len),
    };
    validate(&event, display_len, ai_len)?;
    let limit = resolve_limit(&event)?;
    if display_len.saturating_add(ai_len) > limit {
        return Err(invalid("image result exceeds request byte limit"));
    }
    let mut display = vec![0; display_len];
    let mut ai = vec![0; ai_len];
    input.read_exact(&mut display)?;
    input.read_exact(&mut ai)?;
    Ok(NativeReply { event, display, ai })
}

fn validate(event: &NativeEvent, display_len: usize, ai_len: usize) -> io::Result<()> {
    if display_len.saturating_add(ai_len) > MAX_ARTIFACT_BYTES {
        return Err(invalid("image result exceeds byte limit"));
    }
    match event {
        NativeEvent::Decoded { .. } if display_len != 0 || ai_len != 0 => {
            Err(invalid("invalid stage payload"))
        }
        NativeEvent::Finished {
            display_len: expected_display,
            ai_len: expected_ai,
            thumbhash,
            failure,
            ..
        } if *expected_display != display_len
            || *expected_ai != ai_len
            || thumbhash.as_ref().is_some_and(|hash| hash.len() > 128)
            || (failure.is_some() && (display_len != 0 || ai_len != 0)) =>
        {
            Err(invalid("invalid image result"))
        }
        _ => Ok(()),
    }
}

fn write_header(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_HEADER_BYTES {
        return Err(invalid("image header too large"));
    }
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}

fn read_header<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_HEADER_BYTES {
        return Err(invalid("invalid image header length"));
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_lengths_are_rejected_before_pixel_allocation() {
        assert!(read_reply(&mut u32::MAX.to_le_bytes().as_slice()).is_err());
        let mut wire = Vec::new();
        write_header(
            &mut wire,
            &NativeEvent::Finished {
                id: 1,
                failure: None,
                backend: "wic".into(),
                decode_us: 0,
                encode_us: 0,
                display_len: MAX_ARTIFACT_BYTES + 1,
                ai_len: 0,
                thumbhash: None,
                stats: Default::default(),
                gpu: None,
            },
        )
        .unwrap();
        assert!(read_reply(&mut wire.as_slice()).is_err());
    }
    #[test]
    fn trusted_request_limit_and_unknown_id_reject_before_reading_payload() {
        let mut wire = Vec::new();
        write_reply(
            &mut wire,
            &NativeReply {
                event: NativeEvent::Finished {
                    id: 7,
                    failure: None,
                    backend: "wic".into(),
                    decode_us: 0,
                    encode_us: 0,
                    display_len: 3,
                    ai_len: 2,
                    thumbhash: None,
                    stats: Default::default(),
                    gpu: Some(NativeGpuMetrics {
                        cpu_planar_us: 3,
                        gpu_us: Some(5),
                        adapter: Some("adapter".into()),
                        ..Default::default()
                    }),
                },
                display: vec![1, 2, 3],
                ai: vec![4, 5],
            },
        )
        .unwrap();
        let header_end = 4 + u32::from_le_bytes(wire[..4].try_into().unwrap()) as u64;
        let mut input = std::io::Cursor::new(&wire);
        let result = read_reply_with_limit(&mut input, |event| {
            assert!(matches!(event, NativeEvent::Finished { id: 7, .. }));
            Ok(4)
        });
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidData);
        assert_eq!(input.position(), header_end);
        input.set_position(0);
        assert!(read_reply_with_limit(&mut input, |_| Err(invalid("unknown request"))).is_err());
        assert_eq!(input.position(), header_end);
        input.set_position(0);
        let reply = read_reply_with_limit(&mut input, |_| Ok(5)).unwrap();
        assert_eq!(reply.display, [1, 2, 3]);
        assert_eq!(reply.ai, [4, 5]);
        match reply.event {
            NativeEvent::Finished { gpu: Some(gpu), .. } => {
                assert_eq!(gpu.cpu_planar_us, 3);
                assert_eq!(gpu.gpu_us, Some(5));
                assert_eq!(gpu.adapter.as_deref(), Some("adapter"));
                assert_eq!(gpu.completion_wait_us, 0);
            }
            _ => panic!("GPU metrics missing from finished reply"),
        }
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&wire[4..header_end as usize]).unwrap();
        legacy["Finished"].as_object_mut().unwrap().remove("gpu");
        let mut legacy_wire = Vec::new();
        write_header(&mut legacy_wire, &legacy).unwrap();
        legacy_wire.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert!(matches!(
            read_reply(&mut legacy_wire.as_slice()).unwrap().event,
            NativeEvent::Finished { gpu: None, .. }
        ));
    }
}
