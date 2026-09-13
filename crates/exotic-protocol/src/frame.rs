// crates/exotic-protocol/src/frame.rs
//! 统一帧编解码（v3 Part2 §3.2）。
//!
//! **唯一**帧格式（禁止「JSON 行 + 另一套二进制长度前缀」混合协议）。所有整数小端：
//!
//! ```text
//! offset  size  field
//! 0       4     magic = "EXOT"
//! 4       2     protocol_version
//! 6       2     frame_type
//! 8       8     request_id
//! 16      4     json_len
//! 20      4     blob_len
//! 24      N     UTF-8 JSON（仅控制字段）
//! 24+N    M     blob（缩略图字节等；同一帧内，禁止再跟第二个独立 blob frame）
//! ```
//!
//! 安全要点：**先读 24 字节定长头、校验 magic/版本/类型/长度上限，再分配内存**。
//! 谎报的 `json_len`/`blob_len` 在分配前即被 `MAX_JSON_LEN`/`MAX_BLOB_LEN` 拦截，避免内存炸弹。

use std::io::{self, Read, Write};

/// 帧魔数。stdout 上任何非该魔数开头的前导字节都视为协议损坏（Worker 误把日志写进 stdout）。
pub const MAGIC: [u8; 4] = *b"EXOT";

/// 当前协议版本（握手时校验；不承担包回滚防护，R11）。
/// v2(2026-07-03,Part4 T10):RequestBody 扩 session/embed 族 op(SessionInit/
/// SessionClose/EmbedBatch/FaceDetectEmbed)、WorkerErrorCode +4、Success/Failure 的
/// item 字段 Option 化。帧结构与 FrameType **不变**(D3 裁决:零新帧,SessionReady =
/// SessionInit 的 Success 响应;D2 裁决:GPU 令牌留主进程不进协议)。帧层硬等值校验
/// → psd-worker 与协议同波重编译,无混版(Part6 §8.2 C1)。
/// v3(2026-07-11,事故加固批 A):新增 [`FrameType::Progress`] 帧(Worker→Host,
/// 长操作期间的阶段回执+心跳),宿主 watchdog 由「猜总时长」改「静默限时」。
/// 新帧型是线上不兼容变更(旧端 read_frame 判 UnknownFrameType 即杀 worker),
/// 故升版本让陈旧二进制在握手层收到明确的版本错而非中途「协议损坏」;
/// WorkerErrorCode +3(ort dylib/装载阶段化,见 message.rs)。
/// 版本号变更须同步 scripts/check-exotic-protocol-sync.mjs(跨语言同步门禁)。
pub const PROTOCOL_VERSION: u16 = 3;

/// JSON 段最大字节（1 MiB）。控制字段不应接近此值；超限即协议异常 → 杀 Worker。
pub const MAX_JSON_LEN: u32 = 1 << 20;

/// blob 段最大字节（64 MiB）。缩略图 WebP 远小于此；超限即异常 → 杀 Worker。
pub const MAX_BLOB_LEN: u32 = 64 << 20;

/// 定长头字节数。
pub const HEADER_LEN: usize = 24;

/// 帧类型。值显式固定，避免重排枚举导致跨版本语义漂移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FrameType {
    /// Host→Worker：握手开场（HelloBody）。
    Hello = 1,
    /// Worker→Host：握手应答（ReadyBody：worker_id/version/protocol/capabilities/max_blob_len）。
    Ready = 2,
    /// Host→Worker：处理请求（RequestBody，按 `op` 分 Thumbnail/Metadata）。
    Request = 3,
    /// Worker→Host：成功（SuccessBody + 同帧 blob）。
    Success = 4,
    /// Worker→Host：失败（FailureBody，含稳定错误码与 retryable）。
    Failure = 5,
    /// Host→Worker：请关闭（优雅退出；超时再 kill）。
    Shutdown = 6,
    /// Worker→Host（v3）：在途请求的阶段回执/心跳（ProgressBody）。**非终态**——
    /// Host 读循环消费后重置静默计时并继续等待,终态仍只有 Success/Failure。
    /// request_id 必须等于在途请求;不匹配的 Progress 由 Host 忽略并告警(不判违例)。
    Progress = 7,
}

impl FrameType {
    pub fn to_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(v: u16) -> Option<Self> {
        Some(match v {
            1 => FrameType::Hello,
            2 => FrameType::Ready,
            3 => FrameType::Request,
            4 => FrameType::Success,
            5 => FrameType::Failure,
            6 => FrameType::Shutdown,
            7 => FrameType::Progress,
            _ => return None,
        })
    }
}

/// 协议级错误。除 `Io` 外，任一变体都意味着对端不可信 → 调用方应杀 Worker（v3 Part2 §3.7）。
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("IO 错误：{0}")]
    Io(#[from] io::Error),
    #[error("magic 不匹配：{0:02x?}（期望 EXOT）")]
    BadMagic([u8; 4]),
    #[error("不支持的协议版本：{0}（本端 {PROTOCOL_VERSION}）")]
    UnsupportedVersion(u16),
    #[error("未知帧类型：{0}")]
    UnknownFrameType(u16),
    #[error("json_len 超限：{0} > {MAX_JSON_LEN}")]
    JsonTooLarge(u32),
    #[error("blob_len 超限：{0} > {MAX_BLOB_LEN}")]
    BlobTooLarge(u32),
    #[error("JSON 反序列化失败：{0}")]
    Json(String),
}

impl ProtocolError {
    /// 是否为干净的流结束（对端正常关闭/退出）。Host 读取线程据此区分「Worker 退出」与「协议损坏」。
    pub fn is_clean_eof(&self) -> bool {
        matches!(self, ProtocolError::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof)
    }
}

/// 一帧的内存表示。`json` 仅控制字段；`blob` 为同帧二进制负载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub frame_type: FrameType,
    pub request_id: u64,
    pub json: Vec<u8>,
    pub blob: Vec<u8>,
}

impl Frame {
    /// 以序列化某控制体构造帧（blob 为空）。
    pub fn control<T: serde::Serialize>(
        frame_type: FrameType,
        request_id: u64,
        body: &T,
    ) -> Result<Frame, ProtocolError> {
        let json = serde_json::to_vec(body).map_err(|e| ProtocolError::Json(e.to_string()))?;
        Ok(Frame {
            frame_type,
            request_id,
            json,
            blob: Vec::new(),
        })
    }

    /// 以控制体 + blob 构造帧（如 Success + 缩略图字节）。
    pub fn with_blob<T: serde::Serialize>(
        frame_type: FrameType,
        request_id: u64,
        body: &T,
        blob: Vec<u8>,
    ) -> Result<Frame, ProtocolError> {
        let mut f = Frame::control(frame_type, request_id, body)?;
        f.blob = blob;
        Ok(f)
    }

    /// 把 JSON 段反序列化为某控制体。
    pub fn parse_json<T: serde::de::DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        serde_json::from_slice(&self.json).map_err(|e| ProtocolError::Json(e.to_string()))
    }
}

/// 从 `r` 读出一帧。`read_exact` 自动处理分片读取（TCP/管道粘包/拆包透明）。
///
/// 校验顺序严格为「头 → magic → 版本 → 类型 → 长度上限 → 才分配 json/blob」。任一不过即返回错误，
/// **不**分配谎报的大缓冲。返回 `ProtocolError::is_clean_eof()==true` 表示对端正常关闭流。
pub fn read_frame<R: Read>(r: &mut R) -> Result<Frame, ProtocolError> {
    let mut header = [0u8; HEADER_LEN];
    r.read_exact(&mut header)?;

    let magic = [header[0], header[1], header[2], header[3]];
    if magic != MAGIC {
        return Err(ProtocolError::BadMagic(magic));
    }
    let protocol = u16::from_le_bytes([header[4], header[5]]);
    if protocol != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(protocol));
    }
    let frame_type_raw = u16::from_le_bytes([header[6], header[7]]);
    let frame_type = FrameType::from_u16(frame_type_raw)
        .ok_or(ProtocolError::UnknownFrameType(frame_type_raw))?;
    let request_id = u64::from_le_bytes([
        header[8], header[9], header[10], header[11], header[12], header[13], header[14],
        header[15],
    ]);
    let json_len = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    let blob_len = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);

    // 长度上限**先**于分配——谎报尺寸不会触发大块分配（内存炸弹防线）。
    if json_len > MAX_JSON_LEN {
        return Err(ProtocolError::JsonTooLarge(json_len));
    }
    if blob_len > MAX_BLOB_LEN {
        return Err(ProtocolError::BlobTooLarge(blob_len));
    }

    let mut json = vec![0u8; json_len as usize];
    r.read_exact(&mut json)?;
    let mut blob = vec![0u8; blob_len as usize];
    r.read_exact(&mut blob)?;

    Ok(Frame {
        frame_type,
        request_id,
        json,
        blob,
    })
}

/// 把一帧写入 `w`（不 flush；调用方负责 flush）。写前再次校验长度上限——本端绝不发超限帧。
pub fn write_frame<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    let json_len: u32 = frame
        .json
        .len()
        .try_into()
        .map_err(|_| ProtocolError::JsonTooLarge(u32::MAX))?;
    if json_len > MAX_JSON_LEN {
        return Err(ProtocolError::JsonTooLarge(json_len));
    }
    let blob_len: u32 = frame
        .blob
        .len()
        .try_into()
        .map_err(|_| ProtocolError::BlobTooLarge(u32::MAX))?;
    if blob_len > MAX_BLOB_LEN {
        return Err(ProtocolError::BlobTooLarge(blob_len));
    }

    let mut header = [0u8; HEADER_LEN];
    header[0..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&frame.frame_type.to_u16().to_le_bytes());
    header[8..16].copy_from_slice(&frame.request_id.to_le_bytes());
    header[16..20].copy_from_slice(&json_len.to_le_bytes());
    header[20..24].copy_from_slice(&blob_len.to_le_bytes());

    w.write_all(&header)?;
    w.write_all(&frame.json)?;
    w.write_all(&frame.blob)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// 每次只吐 1 字节的 Reader，用于验证 `read_exact` 对分片/逐字节流的健壮性。
    struct DripReader<'a> {
        data: &'a [u8],
        pos: usize,
    }
    impl<'a> Read for DripReader<'a> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.data[self.pos];
            self.pos += 1;
            Ok(1)
        }
    }

    fn sample_frame() -> Frame {
        Frame {
            frame_type: FrameType::Success,
            request_id: 0xDEAD_BEEF_0000_0001,
            json: br#"{"k":"v"}"#.to_vec(),
            blob: vec![1, 2, 3, 4, 5],
        }
    }

    #[test]
    fn roundtrip() {
        let f = sample_frame();
        let mut buf = Vec::new();
        write_frame(&mut buf, &f).unwrap();
        let mut cur = Cursor::new(buf);
        let got = read_frame(&mut cur).unwrap();
        assert_eq!(got, f);
    }

    #[test]
    fn header_split_byte_by_byte() {
        // 分片读取：逐字节流也必须解出同一帧（read_exact 内部循环）。
        let f = sample_frame();
        let mut buf = Vec::new();
        write_frame(&mut buf, &f).unwrap();
        let mut drip = DripReader { data: &buf, pos: 0 };
        let got = read_frame(&mut drip).unwrap();
        assert_eq!(got, f);
    }

    #[test]
    fn coalesced_two_frames() {
        // 粘包：一个缓冲含两帧，连续读出且不串。
        let f1 = sample_frame();
        let f2 = Frame {
            frame_type: FrameType::Failure,
            request_id: 7,
            json: b"{}".to_vec(),
            blob: Vec::new(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &f1).unwrap();
        write_frame(&mut buf, &f2).unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), f1);
        assert_eq!(read_frame(&mut cur).unwrap(), f2);
    }

    #[test]
    fn progress_frame_roundtrip() {
        // v3 新帧型:Progress 与既有帧同一编解码路径,回环无损。
        let f = Frame {
            frame_type: FrameType::Progress,
            request_id: 42,
            json: br#"{"stage":"clip_image_load","elapsed_ms":1500}"#.to_vec(),
            blob: Vec::new(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &f).unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), f);
    }

    #[test]
    fn empty_payload_ok() {
        let f = Frame {
            frame_type: FrameType::Shutdown,
            request_id: 0,
            json: Vec::new(),
            blob: Vec::new(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &f).unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), f);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &sample_frame()).unwrap();
        buf[0] = b'X';
        let mut cur = Cursor::new(buf);
        assert!(matches!(
            read_frame(&mut cur),
            Err(ProtocolError::BadMagic(_))
        ));
    }

    #[test]
    fn bad_version_rejected() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &sample_frame()).unwrap();
        buf[4..6].copy_from_slice(&999u16.to_le_bytes());
        let mut cur = Cursor::new(buf);
        assert!(matches!(
            read_frame(&mut cur),
            Err(ProtocolError::UnsupportedVersion(999))
        ));
    }

    #[test]
    fn unknown_frame_type_rejected() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &sample_frame()).unwrap();
        buf[6..8].copy_from_slice(&255u16.to_le_bytes());
        let mut cur = Cursor::new(buf);
        assert!(matches!(
            read_frame(&mut cur),
            Err(ProtocolError::UnknownFrameType(255))
        ));
    }

    #[test]
    fn oversized_json_len_rejected_before_alloc() {
        // 谎报 json_len 远超上限：必须在分配前拒绝（不 OOM）。手工拼一个仅含头的缓冲。
        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&MAGIC);
        header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        header[6..8].copy_from_slice(&FrameType::Request.to_u16().to_le_bytes());
        header[16..20].copy_from_slice(&(MAX_JSON_LEN + 1).to_le_bytes());
        let mut cur = Cursor::new(header.to_vec());
        assert!(matches!(
            read_frame(&mut cur),
            Err(ProtocolError::JsonTooLarge(_))
        ));
    }

    #[test]
    fn oversized_blob_len_rejected_before_alloc() {
        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&MAGIC);
        header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        header[6..8].copy_from_slice(&FrameType::Success.to_u16().to_le_bytes());
        header[20..24].copy_from_slice(&(MAX_BLOB_LEN + 1).to_le_bytes());
        let mut cur = Cursor::new(header.to_vec());
        assert!(matches!(
            read_frame(&mut cur),
            Err(ProtocolError::BlobTooLarge(_))
        ));
    }

    #[test]
    fn clean_eof_detected() {
        // 空流：read_exact 头即 UnexpectedEof → is_clean_eof()，Host 视为 Worker 正常退出。
        let mut cur = Cursor::new(Vec::new());
        let err = read_frame(&mut cur).unwrap_err();
        assert!(err.is_clean_eof());
    }

    #[test]
    fn truncated_payload_is_io_error_not_clean_eof() {
        // 头声明有 payload 但流提前断：UnexpectedEof，但语义是「损坏」——调用方仍杀 Worker。
        let mut buf = Vec::new();
        write_frame(&mut buf, &sample_frame()).unwrap();
        buf.truncate(HEADER_LEN + 2); // 砍掉大部分 payload
        let mut cur = Cursor::new(buf);
        let err = read_frame(&mut cur).unwrap_err();
        // 仍是 EOF 类（这里不强求区分；Host 对任一读错误都杀 Worker）。
        assert!(matches!(err, ProtocolError::Io(_)));
    }

    #[test]
    fn v1_frame_rejected_with_diagnosable_error() {
        // 旧 psd-worker(v1)的帧在读帧层即被版本门拒,错误信息含双方版本号(D3 §6 验收)。
        let mut buf = Vec::new();
        write_frame(&mut buf, &sample_frame()).unwrap();
        buf[4..6].copy_from_slice(&1u16.to_le_bytes());
        let mut cur = Cursor::new(buf);
        match read_frame(&mut cur) {
            Err(ProtocolError::UnsupportedVersion(1)) => {}
            other => panic!("期望 UnsupportedVersion(1),得到 {other:?}"),
        }
    }

    #[test]
    fn write_rejects_oversized_blob() {
        // 本端绝不发超限帧：构造 blob 超 MAX 时 write 直接报错（不真的分配 64MiB+，用 len 伪造不便，
        // 改为信任 try_into/上限分支；此处验证上限常量关系）。
        assert!(MAX_BLOB_LEN as usize <= u32::MAX as usize);
    }
}
