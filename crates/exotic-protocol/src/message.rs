// crates/exotic-protocol/src/message.rs
//! 协议消息体（v3 Part2 §3.3）。两端共享同一份定义，JSON 只放控制字段；缩略图等二进制走同帧 blob。

use serde::{Deserialize, Serialize};

/// Host→Worker 握手开场（Hello 帧）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloBody {
    /// Host 语义版本（展示/兼容用，不承担回滚防护，R11）。
    pub host_version: String,
    /// Host 期望的协议版本；与 Worker `PROTOCOL_VERSION` 不一致即握手失败。
    pub protocol_version: u16,
    /// Host 能接收的最大 blob；Worker 据此自限输出。
    pub max_blob_len: u32,
}

/// Worker→Host 握手应答（Ready 帧）。Host 校验 worker_id/version/protocol/capabilities。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadyBody {
    pub worker_id: String,
    pub worker_version: String,
    pub protocol_version: u16,
    /// Worker 实测支持的能力（如 `["thumbnail"]`）；只声明 probe 通过范围。
    pub capabilities: Vec<String>,
    pub max_blob_len: u32,
}

/// Host→Worker 处理请求（Request 帧）。按 `op` 分流；首发只有 thumbnail。
///
/// `target_long_edge` 必须是**吸附后档位**（R5）——与指纹里的 `target_tier` 一致，
/// 否则同档不同请求 size 会算出不同指纹、反复重做。
// v2 起含 f32 字段(FaceDetectEmbed.det_score_thresh),不再派生 Eq。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RequestBody {
    Thumbnail {
        item_id: i64,
        source_path: String,
        target_long_edge: u32,
        input_fingerprint: String,
    },
    Metadata {
        item_id: i64,
        source_path: String,
        input_fingerprint: String,
    },
    /// 模型加载 = 显式会话请求(v2;Part4 D3 §2:不进进程握手、不加帧类型;
    /// 其 300s 超时由 host 侧 per-op timeout 表配置)。响应 = Success 帧 +
    /// `SuccessBody.session`(SessionReady 不是新帧)。
    SessionInit {
        /// host 侧单调分配;SessionClose/诊断日志引用。
        session_id: u64,
        /// 多角色模型载荷(Part6 §3.2.1a:合并单 ai-worker 一次声明 CLIP 图/文 +
        /// YuNet + SFace;worker 按 role 取各自 handle)。
        models: Vec<ModelDescriptor>,
        model_profile: ModelProfileSnapshot,
        /// `ModelHandle::Path` 的归属校验根:worker 侧 canonicalize 后须以此为
        /// 前缀,越界回 `ModelLoadFailed`(D1 §3,防宿主被劫持后诱导任意读)。
        models_root: String,
        /// 受限缓存根:worker 只读 `{ai_cache_dir}/{key[..2]}/{key}.webp`,拒越界
        /// (Part6 §3.2.1a ②,补「worker 只收 cache_keys、不知缓存根」缺口)。
        ai_cache_dir: String,
        /// 图像塔 EP:"directml"|"cpu";文本塔由 worker 内硬编码 CPU(Part4 §8.6)。
        image_provider: String,
    },
    /// 显式卸载会话(host 主导生命周期:空闲计时到期/切换模型前;D3 §4)。
    SessionClose { session_id: u64 },
    /// CLIP 批量嵌入,一 Request = 一批(§8.1 决策1/方案B:传 cache_keys 而非
    /// tensor,blob≈0 无 MAX_BLOB 压力)。响应 = Success 帧 + `SuccessBody.embed`,
    /// 嵌入本体在同帧 blob(布局见 [`EmbedBatchSuccess`])。
    EmbedBatch { items: Vec<EmbedItem> },
    /// 人脸检测+嵌入批(与 EmbedBatch 同构逐项化;几何走 JSON、嵌入走 blob)。
    FaceDetectEmbed {
        items: Vec<FaceItem>,
        /// 检测置信度阈值快照(行为参数,进指纹):同图不同阈值产出不同结果。
        /// 取值随 host 侧 face profile(YuNet 0.9 / SCRFD 0.5)。
        det_score_thresh: f32,
    },
    /// CLIP 文本编码(v2 additive,T17):语义搜索查询向量。T16 删主进程 ort+tokenizers
    /// 后,文本塔只存在于 worker(其 EP 恒 CPU,Part4 §8.6),故此 op 是搜索链路的必经
    /// 载体——T10 三源合并时漏列,T15 发现缺口、T17 补齐(additive,不动 PROTOCOL_VERSION)。
    /// 响应 = Success 帧 + `SuccessBody.text_embed`,向量本体在同帧 blob(按 texts 顺序连续,
    /// 每项 `embed_dim × f32(LE)`)。**全批原子**:文本编码无逐项 IO 失败模式(不读盘、
    /// tokenizer 接受任意字符串),任一失败即整批 Failure,不做逐项 Ok/Err。
    /// 不新增 capability:文本塔与 CLIP 图像塔同属 `embedding` 会话,凡会话就绪即可服务。
    EncodeText { texts: Vec<String> },
    /// OCR 会话装载(v2 additive):独立于 CLIP 会话(D-OCR-1),worker 双槽并存。
    /// 响应 = Success + SuccessBody.ocr_session。
    /// v2 additive:不动 PROTOCOL_VERSION;旧 worker 收到新 op 会 JSON 解析失败回 internal_error(main.rs 兜底),两端同仓同步分发,无实际混版窗口。
    OcrSessionInit {
        session_id: u64,
        /// 角色 OcrDet/OcrCls/OcrRec/OcrDict 四件套,逐件 len+sha256(同 D1 §3)。
        models: Vec<ModelDescriptor>,
        /// worker 按 profile_id 从 scrollery-ai-core::ocr_profile 内建注册表取几何/阈值/文件名契约。
        ocr_profile_id: String,
        models_root: String,
    },
    /// OcrSessionClose 兼容性同 OcrSessionInit。
    OcrSessionClose { session_id: u64 },
    /// OCR 批(交互一次一图为主,批口面向未来)。响应 = Success + SuccessBody.ocr,blob 恒空(D-OCR-4)。
    /// v2 additive:不动 PROTOCOL_VERSION;旧 worker 收到新 op 会 JSON 解析失败回 internal_error(main.rs 兜底),两端同仓同步分发,无实际混版窗口。
    OcrBatch { items: Vec<OcrItem> },
    /// 影像增强会话装载(v2 additive,降噪/超分子系统 design.md §G):独立于 CLIP/OCR
    /// 会话(D-OCR-1 同型 worker 独立 worker 进程),models 逐件走 validate_and_resolve
    /// 前缀/字节数/sha256 校验(与 SessionInit 同款)。响应 = Success 帧(无专属就绪体,
    /// 增强会话就绪即可直接派 EnhanceRun)。
    EnhanceSessionInit {
        session_id: u64,
        /// 本会话可用模型集合(ModelRole::Enhance);具体档位由 EnhanceStep.model_id 寻址。
        models: Vec<ModelDescriptor>,
        /// `ModelHandle::Path` 的归属校验根:worker 侧 canonicalize 后须以此为前缀,
        /// 越界回 `ModelLoadFailed`(与 `SessionInit.models_root` 同型语义;D1 §3,
        /// 防宿主被劫持后诱导任意读)。
        models_root: String,
        /// worker 输出白名单前缀:后续 `EnhanceRun.output_tmp_path` 经 canonicalize 后
        /// 必须位于本目录 canonicalize 结果之下,越界即拒(与 OCR `cache_key` 越界拒绝同型,
        /// 防宿主被劫持后诱导 worker 向任意路径写文件)。host 侧 `{work_dir}/{job}.tmp` 落盘根。
        work_dir: String,
    },
    /// EnhanceSessionClose 兼容性同 SessionClose。
    EnhanceSessionClose { session_id: u64 },
    /// 影像增强执行(design.md §D/§E):worker 按 `steps` 顺序逐步跑(host 保证已排好
    /// 降噪→去伪影→超分序,worker 不再重排)。输出组装全图后编码写 host 指定的
    /// `output_tmp_path`(路径前缀白名单校验同 cache_key 越界拒绝先例)。
    /// 响应 = Success + SuccessBody.enhance;per-tile Progress 心跳(帧型复用既有
    /// ProgressBody,不新增)。
    EnhanceRun {
        session_id: u64,
        source_path: String,
        /// worker 写入的输出路径(host 侧 `.tmp` 同卷 rename 前的临时文件)。
        output_tmp_path: String,
        /// 输出编码格式:`"jpeg"` | `"png"`。
        output_format: String,
        /// 执行链,严格按序执行(host 负责排序:降噪→去伪影→超分)。
        steps: Vec<EnhanceStep>,
    },
    /// 视频会话装载(v2 additive,视频格式扩展子系统 design.md §2.3):worker 校验
    /// `ffmpeg_exe_path` 存在 + sha256 相符(防换包),运行 `ffmpeg -version` 读取版本与
    /// configuration 行,发现 `--enable-gpl` 立即回 terminal 失败(许可运行时保险丝,§3.4)。
    VideoSessionInit {
        session_id: u64,
        ffmpeg_exe_path: String,
        ffmpeg_sha256: String,
        /// worker 输出白名单前缀:后续 `output_tmp_path` 经 canonicalize 后必须位于
        /// 本目录 canonicalize 结果之下,越界即拒(与 `EnhanceSessionInit.work_dir`
        /// 完全同型语义;`message.rs` 既有 Enhance 会话字段注)。
        work_dir: String,
    },
    /// VideoSessionClose 兼容性同 EnhanceSessionClose。
    VideoSessionClose { session_id: u64 },
    /// 流事实探测(design.md §2.3):worker 用 ffprobe 输出流事实,不做判定;
    /// 判定表在 host(「host 不信任 worker」+ 策略集中)。响应 = Success +
    /// `SuccessBody.video_probe`。
    VideoProbe {
        session_id: u64,
        source_path: String,
        input_fingerprint: String,
    },
    /// 容器改封(design.md §2.3):`-c:v copy`,`audio_transcode=false` 时 `-c:a copy`、
    /// true 时 `-c:a aac`;输出 `-movflags +faststart` 的 MP4。字幕轨一律 `-sn` 丢弃。
    /// 每 ≤2s 发一帧 Progress(`stage="remux"`)。响应 = Success + `SuccessBody.video_out`。
    VideoRemux {
        session_id: u64,
        source_path: String,
        output_tmp_path: String,
        audio_transcode: bool,
        audio_track_index: Option<u32>,
    },
    /// 一次性全转码(design.md §2.3):`encoder_ladder` 由 host 下发(如
    /// `["h264_nvenc","h264_qsv","h264_amf","h264_mf"]`),worker 逐个试起、首个成功者
    /// 用之;Progress 同 [`RequestBody::VideoRemux`](`stage="transcode"`)。响应同
    /// `video_out`。host 可同时下发 `crf`/`bitrate_kbps` 两者;worker 按 `encoder_ladder`
    /// 胜出的编码器的率控模型取用其一(硬编码率控编码器优先质量模式用 `crf`,
    /// `h264_mf` 等固定码率编码器用 `bitrate_kbps`;两者皆 None 时 worker 用所选
    /// encoder 的默认率控)。
    VideoTranscode {
        session_id: u64,
        source_path: String,
        output_tmp_path: String,
        encoder_ladder: Vec<String>,
        crf: Option<u8>,
        bitrate_kbps: Option<u32>,
        max_long_edge: Option<u32>,
        audio_track_index: Option<u32>,
        hw_decode: bool,
    },
    /// 缩略图后端取帧(design.md §2.3):`mode = Cover` → 响应 Success + blob 单帧 WebP,
    /// `video_frames` 不填;`mode = Keyframes` → 响应 Success + blob 雪碧条 WebP +
    /// `SuccessBody.video_frames`(切格元数据 `{cell_width, cell_height, n}`)。
    VideoFrames {
        session_id: u64,
        source_path: String,
        input_fingerprint: String,
        mode: VideoFramesMode,
    },
}

/// [`RequestBody::VideoFrames`] 的取帧模式(design.md §2.3)。
/// tag 用 `"kind"` 而非 `"mode"`:避免与载体字段 `RequestBody::VideoFrames.mode` 同名
/// 造成双层同名嵌套(`{"mode":{"mode":"cover",...}}`),沿用 [`ModelHandle`] 的
/// `tag = "kind"` 惯例。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VideoFramesMode {
    /// 单帧封面,`max_long_edge` 语义同缩略图档位吸附。
    Cover { max_long_edge: u32 },
    /// n 格关键帧雪碧条,`cell_height` 为单格高度(宽度由源宽高比推得)。
    Keyframes { n: u32, cell_height: u32 },
}

impl RequestBody {
    /// 取请求关联的单一 item_id(响应核对用)。会话/批量 op 无单值语义,返回 None
    /// ——批量的逐项核对走 [`EmbedBatchSuccess`]/[`FaceBatchSuccess`] 的 per-item 字段。
    pub fn item_id(&self) -> Option<i64> {
        match self {
            RequestBody::Thumbnail { item_id, .. } | RequestBody::Metadata { item_id, .. } => {
                Some(*item_id)
            }
            RequestBody::SessionInit { .. }
            | RequestBody::SessionClose { .. }
            | RequestBody::EmbedBatch { .. }
            | RequestBody::FaceDetectEmbed { .. }
            | RequestBody::EncodeText { .. }
            | RequestBody::OcrSessionInit { .. }
            | RequestBody::OcrSessionClose { .. }
            | RequestBody::OcrBatch { .. }
            | RequestBody::EnhanceSessionInit { .. }
            | RequestBody::EnhanceSessionClose { .. }
            | RequestBody::EnhanceRun { .. }
            | RequestBody::VideoSessionInit { .. }
            | RequestBody::VideoSessionClose { .. }
            | RequestBody::VideoProbe { .. }
            | RequestBody::VideoRemux { .. }
            | RequestBody::VideoTranscode { .. }
            | RequestBody::VideoFrames { .. } => None,
        }
    }

    /// 取请求的单一输入指纹(响应核对用);会话/批量 op 返回 None,理由同 [`Self::item_id`]。
    pub fn input_fingerprint(&self) -> Option<&str> {
        match self {
            RequestBody::Thumbnail {
                input_fingerprint, ..
            }
            | RequestBody::Metadata {
                input_fingerprint, ..
            } => Some(input_fingerprint),
            RequestBody::SessionInit { .. }
            | RequestBody::SessionClose { .. }
            | RequestBody::EmbedBatch { .. }
            | RequestBody::FaceDetectEmbed { .. }
            | RequestBody::EncodeText { .. }
            | RequestBody::OcrSessionInit { .. }
            | RequestBody::OcrSessionClose { .. }
            | RequestBody::OcrBatch { .. }
            | RequestBody::EnhanceSessionInit { .. }
            | RequestBody::EnhanceSessionClose { .. }
            | RequestBody::EnhanceRun { .. }
            | RequestBody::VideoSessionInit { .. }
            | RequestBody::VideoSessionClose { .. }
            | RequestBody::VideoProbe { .. }
            | RequestBody::VideoRemux { .. }
            | RequestBody::VideoTranscode { .. }
            | RequestBody::VideoFrames { .. } => None,
        }
    }
}

/// 模型载荷句柄(Part4 D1 两级通道;v2 一次定型,避免二次破坏性升版)。
///
/// - `Path`:明文权重(P2 首期全部模型)——models 目录内文件的**绝对路径**,worker
///   `commit_from_file` 直接加载;须先做 models_root 归属校验(见 SessionInit 字段注)。
/// - `Named`:AES 加密权重(④ 变现后)——主进程解密后写具名共享内存,worker 按名
///   map 后 `commit_from_memory`;名称格式 `pn-{8B hex}-{16B CSPRNG hex}`(D1 §4)。
/// - 刻意不设 `Fd`/`Win32Handle` 变体:匿名句柄跨 exec 进程无效(Part4 §3.7.3),
///   具名方案三平台闭合;除非实测具名开销不可接受才另启(D1 裁决②)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ModelHandle {
    Path(String),
    Named(String),
}

/// 模型角色(Part6 §3.2.1a):一个 session 载多模型,worker 按角色寻址。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    ImageEncoder,
    TextEncoder,
    FaceDetect,
    FaceRecog,
    OcrDet,
    OcrCls,
    OcrRec,
    OcrDict,
    /// 影像增强模型(降噪/超分子系统 design.md §G):单角色,一次会话可装载多个
    /// 同角色档位(降噪/去伪影/超分各选一,或同任务多档),worker 侧模型间的
    /// 二次寻址细节归 P0 批 2(推理模块)实现。
    Enhance,
}

/// 单个模型载荷描述:role→handle 寻址(§3.2.1a)+ 逐模型完整性字段(D1 §3)。
/// worker 加载前校验 len/sha256(均对**明文**;Named 通道 map 后校验),不符回
/// `ModelLoadFailed`(terminal)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub role: ModelRole,
    pub handle: ModelHandle,
    /// 明文字节数(Named 通道即共享内存映射长度)。
    pub len: u64,
    /// 明文 sha256(64 位小写 hex,不带算法前缀)。
    pub sha256: String,
    /// 影像增强会话(`EnhanceSessionInit.models`)必填:worker 靠它把
    /// `EnhanceStep.model_id` 映射到本描述符对应的具体模型——不得靠
    /// `ModelHandle::Path` 的文件名反查(`ModelHandle::Named` 时无文件名可反查,
    /// 会断)。CLIP/Face/OCR 等既有角色不需要多档寻址,恒为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// 模型 profile 快照(Part4 §8.6 / Part6 §8.3):worker 侧预/后处理按 `arch_id` 从
/// 其内建注册表取几何/归一化/tokenizer 等参数,host 不逐字段下发。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProfileSnapshot {
    /// CLIP 架构族 id(worker 内建注册表键)。
    pub arch_id: String,
    pub image_file: String,
    pub text_file: String,
    /// CLIP 图像批容量(静态 batch;EmbedBatch 单批 items 数不得超过)。
    pub batch_size: u32,
    /// 人脸 profile id(= `faces.model_name`);None = 本 session 不载人脸角色。
    /// (T10 补充:合并 session 需同时声明 CLIP 与人脸两套 profile,§3.2.1a 原稿仅 CLIP 形。)
    pub face_profile_id: Option<String>,
}

/// EmbedBatch 单项(Part6 §3.2.1a 逐项化:每项独立 fingerprint,陈旧/错位防护到项)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbedItem {
    pub item_id: i64,
    /// ai_cache 键;worker 拼 `{ai_cache_dir}/{key[..2]}/{key}.webp` 读图自解码。
    pub cache_key: String,
    pub fingerprint: String,
}

/// FaceDetectEmbed 单项:`cache_key`/`source_path` 至少给一,cache 缺失或分辨率不足时
/// host 以 source_path 派活(人脸检测对分辨率敏感;信任语义同 Thumbnail.source_path)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FaceItem {
    pub item_id: i64,
    pub cache_key: Option<String>,
    pub source_path: Option<String>,
    pub fingerprint: String,
}

/// OcrBatch 单项:语义同 [`FaceItem`]——`cache_key`/`source_path` 至少给一。
/// OCR 一期忽略 `cache_key`(ai 缓存 ≤640 级分辨率不足以识字),host 恒传
/// `source_path`;`cache_key` 字段面向未来批量场景保留。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OcrItem {
    pub item_id: i64,
    pub cache_key: Option<String>,
    pub source_path: Option<String>,
    pub fingerprint: String,
}

/// 影像增强任务种类(降噪/超分子系统 design.md §D 三任务;与
/// `scrollery_ai_core::enhance_profile::EnhanceTaskKind` 语义一一对应,两 crate
/// 独立定义、不互相依赖)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnhanceTask {
    Denoise,
    DejpegArtifact,
    Upscale,
}

/// `EnhanceRun.steps` 单步:任务 + 模型档位 + 可选强度。`strength` 为原生量纲
/// (DRUNet σ、FBCNN QF);host 负责把 UI 滑杆值换算到此;`None` = 用模型默认参数。
/// host 保证 `steps` 已排好执行序(降噪→去伪影→超分),worker 严格按序跑,不重排。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhanceStep {
    pub task: EnhanceTask,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f32>,
}

/// 能力名标准字符串:Ready.capabilities / SessionReadyBody.caps / DB
/// `exotic_tasks.capability` 共用,与 host 侧 `catalog::Capability::as_str` 一一对应,
/// 防两端字面量漂移(G5:embedding/face_detect_embed 为 v2 新增)。
pub mod capability {
    pub const THUMBNAIL: &str = "thumbnail";
    pub const METADATA: &str = "metadata";
    pub const EMBEDDING: &str = "embedding";
    pub const FACE_DETECT_EMBED: &str = "face_detect_embed";
    /// 注:worker 能力通告用;OCR builtin 路径不进 exotic 任务化(D-OCR-7),catalog::Capability 无对应变体,豁免本 mod 头注的一一对应承诺。
    pub const OCR_TEXT: &str = "ocr_text";
    /// 影像增强(降噪/超分子系统 design.md §A/§C):同 OCR 一样不进 exotic 任务化调度
    /// (host 侧 EnhanceService 直持 supervisor+worker client),豁免同上。
    pub const ENHANCE: &str = "enhance";
    /// 视频流事实探测(视频格式扩展子系统 design.md §2.2):同 OCR/Enhance 一样不进
    /// exotic 任务化调度(host 侧 VideoWorkerService 直持 supervisor+worker client),
    /// 与 catalog offering capabilities 不一一对应,豁免同上。
    pub const VIDEO_PROBE: &str = "video_probe";
    /// 视频容器改封(fmp4/faststart,含仅音轨转码档),豁免同上。
    pub const VIDEO_REMUX: &str = "video_remux";
    /// 视频一次性全转码(H.264/AAC MP4),豁免同上。
    pub const VIDEO_TRANSCODE: &str = "video_transcode";
    /// 视频封面帧+关键帧雪碧图(缩略图后端),豁免同上。
    pub const VIDEO_FRAMES: &str = "video_frames";
}

/// Worker→Host 成功（Success 帧 + 同帧 blob）。
///
/// v2 起 `item_id`/`input_fingerprint` 为 Option:会话/批量 op 的 Success 无单项语义
/// (批量逐项核对字段在 `embed`/`face` 内)。thumbnail/metadata 的线上形状不变:
/// Some 序列化为原样数值/字符串,新增三字段 None 时不序列化。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SuccessBody {
    pub item_id: Option<i64>,
    pub input_fingerprint: Option<String>,
    /// 缩略图固定 `image/webp`；Host 二次校验。
    pub mime: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// metadata 能力的结构化结果（thumbnail 时为 None）。
    pub metadata: Option<serde_json::Value>,
    /// SessionInit 的就绪应答(D3 §2:SessionReady 不是新帧)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionReadyBody>,
    /// EmbedBatch 的逐项结果(嵌入本体在同帧 blob)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed: Option<EmbedBatchSuccess>,
    /// FaceDetectEmbed 的逐项结果(嵌入本体在同帧 blob)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceBatchSuccess>,
    /// EncodeText 的应答(向量本体在同帧 blob;v2 additive,T17)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_embed: Option<TextEmbedSuccess>,
    /// OcrSessionInit 的就绪应答(v2 additive,D-OCR-1)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_session: Option<OcrSessionReadyBody>,
    /// OcrBatch 的逐项结果(blob 恒空,D-OCR-4)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr: Option<OcrBatchSuccess>,
    /// EnhanceRun 的完成回执(v2 additive,降噪/超分子系统)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhance: Option<EnhanceDone>,
    /// VideoSessionInit 的就绪应答(v2 additive,视频格式扩展子系统 design.md §2.3)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_session: Option<VideoSessionInfo>,
    /// VideoProbe 的流事实结果(v2 additive,同上)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_probe: Option<VideoProbeInfo>,
    /// VideoRemux/VideoTranscode 的产物统计(v2 additive,同上)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_out: Option<VideoOutInfo>,
    /// VideoFrames 的 Keyframes 雪碧条切格元数据(v2 additive,同上);Cover 模式不填。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_frames: Option<VideoFramesInfo>,
}

/// SessionInit 成功应答体(经 `SuccessBody.session` 携带)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionReadyBody {
    /// CLIP 嵌入维度;EmbedBatch 响应 blob 每项长度 = embed_dim × 4 字节。
    pub embed_dim: u32,
    /// 人脸嵌入维度(载了 FaceRecog 角色才有)。
    pub face_embed_dim: Option<u32>,
    /// 本会话实际可服务的能力(如 ["embedding","face_detect_embed"]),host 据此派活。
    pub caps: Vec<String>,
    /// 实际选用的执行提供器回声(如 "directml"/"cpu";T16 additive:host 删进程内引擎后
    /// provider 探测只发生在 worker 侧,host 借此写回 `ai_provider` 配置供状态栏显示)。
    /// serde(default) 容旧 worker 帧;None = 旧帧/未回报,host 保留既有配置值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// GPU 显示名回声(CPU 时为空串;语义同 provider,写回 `ai_gpu_name`)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_name: Option<String>,
}

/// EnhanceRun 完成回执(经 `SuccessBody.enhance` 携带,design.md §E 输出统计)。
/// 输出尺寸/tile 数供 host 落日志与前端展示;像素/字节本体走 `output_tmp_path`
/// 落盘文件,不进协议帧。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnhanceDone {
    pub out_width: u32,
    pub out_height: u32,
    pub tiles_total: u32,
}

/// OcrSessionInit 成功应答体(经 `SuccessBody.ocr_session` 携带)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OcrSessionReadyBody {
    /// 本会话实际可服务的能力,恒含 `capability::OCR_TEXT`。
    pub caps: Vec<String>,
}

/// EncodeText 成功应答体(经 `SuccessBody.text_embed` 携带)。全批原子,无逐项结构;
/// `count` 供 host 与请求 `texts.len()` 双向核对(「不信任 worker」:blob 长度校验之外
/// 再锁一道数量,op 错配/漏项在协议边界即违例)。blob 布局见 `RequestBody::EncodeText`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextEmbedSuccess {
    /// 已编码文本数;必须等于请求 texts 数(host 校验,不符即协议违例)。
    pub count: u32,
}

/// EmbedBatch 逐项结果(§3.2.1a ③)。`results` 与请求 `items` **严格同序同长**:
/// host 收到先断言长度(不符整批判 InternalError 重试),再逐项比对 item_id +
/// fingerprint,不符即丢弃该项(陈旧/错位防护);逐项 Err 不连坐整批。
///
/// 嵌入本体不进 JSON——128 项 × 768d 的 JSON 文本会撞 MAX_JSON_LEN(1MiB),且违反
/// 「JSON 只放控制字段、二进制走同帧 blob」天条(T10 对 §3.2.1a `Ok{embedding}` 的
/// 修正)。blob 布局:按 `results` 中 **Ok 项的顺序**连续排布,每项 `embed_dim ×
/// f32(LE)`;host 校验 `blob.len() == ok_count × embed_dim × 4`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbedBatchSuccess {
    pub results: Vec<EmbedResult>,
}

/// 单项嵌入结果。回带 item_id+fingerprint 供 host 核对(延续单项 input_fingerprint
/// 核对语义到批量)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EmbedResult {
    /// 成功:嵌入在同帧 blob 中(布局见 [`EmbedBatchSuccess`])。
    Ok { item_id: i64, fingerprint: String },
    /// 该项失败(如缓存缺失/解码失败),整批其余项不受影响。
    Err {
        item_id: i64,
        fingerprint: String,
        code: WorkerErrorCode,
    },
}

/// 单张脸的几何输出(原图坐标系)。嵌入不在此(在同帧 blob,见 [`FaceBatchSuccess`]);
/// 质量分由 host 从 bbox+score 派生,不进协议。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaceDet {
    /// x, y, w, h。
    pub bbox: [f32; 4],
    /// 5 关键点(双眼/鼻尖/双嘴角),对齐模板用。
    pub landmarks: [[f32; 2]; 5],
    /// 检测置信度。
    pub score: f32,
}

/// FaceDetectEmbed 逐项结果,同序同长/逐项核对语义同 [`EmbedBatchSuccess`]。
/// blob 布局:按 results 中 Ok 项顺序、项内按 `faces` 顺序,每脸 `face_embed_dim ×
/// f32(LE)`;host 校验总长 = 全部 Ok 项脸数之和 × face_embed_dim × 4。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaceBatchSuccess {
    pub results: Vec<FaceItemResult>,
}

/// 单项人脸结果(0 张脸也是 Ok,faces 为空)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FaceItemResult {
    Ok {
        item_id: i64,
        fingerprint: String,
        faces: Vec<FaceDet>,
        /// 解码图实际宽高(face 波 additive 补,不动 PROTOCOL_VERSION):FaceDet 几何是
        /// 解码图像素坐标,而解码发生在 worker(源可能是缩略图档位)——host 归一化落库与
        /// quality 派生必须用**实际**解码尺寸,预测尺寸有舍入误差。`default` 容旧帧,
        /// host 校验将 0 视作协议违例(两端同仓同步分发,正常不会出现)。
        #[serde(default)]
        width: u32,
        #[serde(default)]
        height: u32,
    },
    Err {
        item_id: i64,
        fingerprint: String,
        code: WorkerErrorCode,
    },
}

/// OcrBatch 逐项结果,同序同长/逐项核对语义同 [`EmbedBatchSuccess`]。blob 恒空(D-OCR-4)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrBatchSuccess {
    pub results: Vec<OcrItemResult>,
}

/// 单项 OCR 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OcrItemResult {
    Ok {
        item_id: i64,
        fingerprint: String,
        lines: Vec<OcrLine>,
        /// 解码图实际宽高(quad 坐标系)。
        width: u32,
        height: u32,
    },
    Err {
        item_id: i64,
        fingerprint: String,
        code: WorkerErrorCode,
    },
}

/// 单行识别结果。quad = 四点框,解码图像素坐标系——尺度即 OcrItemResult::Ok 的 width/height(实际解码尺寸),不是 DB 原生尺寸;二期叠框须按此调和。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub quad: [[f32; 2]; 4],
    pub confidence: f32,
}

/// VideoSessionInit 成功应答体(经 `SuccessBody.video_session` 携带,视频格式扩展
/// 子系统 design.md §2.3)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoSessionInfo {
    /// 本会话实际可服务的能力(如 `["video_probe","video_remux","video_transcode","video_frames"]`)。
    pub caps: Vec<String>,
    /// `ffmpeg -version` 首行版本号回声,供 host 日志/诊断展示。
    pub ffmpeg_version: String,
}

/// VideoProbe 的流事实结果(经 `SuccessBody.video_probe` 携带,design.md §2.3)。
/// worker 只出**流事实**,不做任何可播性判定——判定表在 host(「host 不信任 worker」+
/// 策略集中)。数值型字段用 `Option` 包裹可缺失项(容器解析不出的字段)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoProbeInfo {
    /// 容器格式名(ffprobe `format_name`,如 `"matroska,webm"`)。
    pub container: String,
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 显示旋转角度(0/90/180/270,来自旋转矩阵/`rotate` tag)。
    pub rotation: Option<i32>,
    pub fps: Option<f32>,
    /// 总码率(bit/s)。
    pub bitrate: Option<u64>,
    pub video_codec: String,
    pub video_profile: Option<String>,
    pub bit_depth: Option<u8>,
    pub pixel_format: Option<String>,
    pub audio_tracks: Vec<VideoAudioTrack>,
    pub has_subtitles: bool,
    pub has_hdr_metadata: bool,
}

/// [`VideoProbeInfo::audio_tracks`] 单项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoAudioTrack {
    pub index: u32,
    pub codec: String,
    pub channels: Option<u32>,
    pub language: Option<String>,
    pub is_default: bool,
}

/// VideoRemux/VideoTranscode 的产物统计(经 `SuccessBody.video_out` 携带,
/// design.md §2.3);host 据此做验收(时长±容差、faststart 标志)后再 `*.tmp` 同卷 rename。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoOutInfo {
    pub out_bytes: u64,
    pub out_duration_ms: u64,
    /// 视频轨是否走 `-c:v copy`(remux 恒 true;transcode 恒 false)。
    pub video_copied: bool,
    /// 音轨是否走 `-c:a copy`(remux 视 `audio_transcode` 而定;transcode 恒 false)。
    pub audio_copied: bool,
}

/// Keyframes 雪碧条切格元数据(经 `SuccessBody.video_frames` 携带,design.md §2.3)。
/// Cover 模式不填(单帧 WebP 无需切格信息)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoFramesInfo {
    pub cell_width: u32,
    pub cell_height: u32,
    pub n: u32,
}

/// 稳定错误码（v3 Part2 §3.3）。整数语义跨版本固定，serde 用 snake_case 字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerErrorCode {
    /// 该格式变体不被支持（CMYK/16-bit/PSB/无 merged image 等）→ terminal，等源/版本变化再失效。
    UnsupportedVariant,
    /// 输入畸形（截断/非法字节）→ terminal。
    MalformedInput,
    /// 触及资源上限（尺寸/像素/内存/输出字节）→ terminal。
    ResourceLimit,
    /// 暂时 IO/文件占用 → retryable。
    IoError,
    /// Worker 内部错误（含 panic 兜底）→ retryable。
    InternalError,
    /// GPU EP 不可用/显存不足(v2,G6)→ retryable:让步/降 CPU EP 由 host 决策。
    GpuUnavailable,
    /// 会话未加载或已卸载(v2,G6)→ retryable:host 重发 SessionInit 后重派。
    SessionExpired,
    /// 模型加载失败:文件缺失/校验不符/路径越界/ort 构建失败(v2,G6)→ terminal,
    /// 重试同一 handle 无意义,host 标记该模型待重下载校验。
    ModelLoadFailed,
    /// 嵌入维度与 profile 声明不符(v2,G6)→ terminal:数据完整性红线。
    EmbedDimMismatch,
    /// ORT 动态库不可解析(v3,加固批 A):ORT_DYLIB_PATH 指向不存在文件,或未设 env 且
    /// exe 旁无 onnxruntime 库 → terminal。此码把「死路径无限阻塞」变为秒级明确报错;
    /// 修复靠环境(补 DLL/纠 env),重试同一 worker 无意义。
    OrtDylibUnavailable,
    /// ORT 运行时初始化超时(v3,加固批 A):dylib 存在但装载/环境创建卡死(如 System32
    /// 旧版 1.17 无限阻塞、损坏的 DLL)→ terminal。区别于模型级装载失败,病灶在运行时本体。
    OrtRuntimeInitTimeout,
    /// 单段模型装载超时(v3,加固批 A):ort 运行时已就绪,但某段 Session 构建超出
    /// 单段预算(如 DirectML shader 编译卡死)→ terminal;message 含卡死段名。
    SessionLoadTimeout,
    /// FFmpeg 不可用(视频格式扩展子系统 design.md §2.3/§3.4):`ffmpeg_exe_path` 路径
    /// 缺失/sha256 不符(防换包)、`-version` 起不来、或 configuration 行检出
    /// `--enable-gpl`(许可运行时保险丝)→ terminal,镜像 `OrtDylibUnavailable` 先例;
    /// host 收到即标记工具待重下载。
    FfmpegUnavailable,
}

impl WorkerErrorCode {
    /// 稳定字符串标识（与 DB `exotic_tasks.last_error_code`、序列化形态一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            WorkerErrorCode::UnsupportedVariant => "unsupported_variant",
            WorkerErrorCode::MalformedInput => "malformed_input",
            WorkerErrorCode::ResourceLimit => "resource_limit",
            WorkerErrorCode::IoError => "io_error",
            WorkerErrorCode::InternalError => "internal_error",
            WorkerErrorCode::GpuUnavailable => "gpu_unavailable",
            WorkerErrorCode::SessionExpired => "session_expired",
            WorkerErrorCode::ModelLoadFailed => "model_load_failed",
            WorkerErrorCode::EmbedDimMismatch => "embed_dim_mismatch",
            WorkerErrorCode::OrtDylibUnavailable => "ort_dylib_unavailable",
            WorkerErrorCode::OrtRuntimeInitTimeout => "ort_runtime_init_timeout",
            WorkerErrorCode::SessionLoadTimeout => "session_load_timeout",
            WorkerErrorCode::FfmpegUnavailable => "ffmpeg_unavailable",
        }
    }

    /// 该错误码的**默认** retryable 语义（Worker 也可在 FailureBody 显式覆盖）。
    pub fn default_retryable(self) -> bool {
        matches!(
            self,
            WorkerErrorCode::IoError
                | WorkerErrorCode::InternalError
                | WorkerErrorCode::GpuUnavailable
                | WorkerErrorCode::SessionExpired
        )
    }
}

/// Worker→Host 阶段回执/心跳（Progress 帧,v3 加固批 A）。**非终态**:Host 消费后
/// 重置静默计时并继续等待同 request_id 的 Success/Failure。两种来源:
/// ① 阶段转换(装载进入新段,`stage` 变化);② 周期心跳(段内仍在装载,`stage` 不变)。
/// 语义 = 「worker 活着且仍在干这件事」;宿主 watchdog 据此把「猜总时长」换成「静默限时」。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgressBody {
    /// 当前阶段标识(如 `ort_runtime_init` / `clip_image_load` / `face_embed_load`)。
    pub stage: String,
    /// 可选补充(provider 名/池容量等诊断信息;不得含完整绝对路径)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 自请求开始的耗时(毫秒)——宿主日志可直读「卡在哪段多久」。
    pub elapsed_ms: u64,
}

/// Worker→Host 失败（Failure 帧）。`retryable` 由 Worker 给出，Host 据错误码 + 该位决定重试/终态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureBody {
    /// v2 起 Option:会话/批量 op 的整批失败无单项语义(逐项失败走 Success 侧 per-item Err)。
    pub item_id: Option<i64>,
    pub input_fingerprint: Option<String>,
    pub code: WorkerErrorCode,
    pub retryable: bool,
    /// 用户可见诊断信息；**不得**含完整绝对路径或 License token（v3 Part2 §3.3）。
    pub message: String,
}

#[cfg(test)]
mod tests;
