// src-tauri/src/video/playback_policy.rs
//! 视频可播性判定表(视频格式扩展子系统 design.md §5.1)。
//!
//! **纯函数、无 IO**:host 侧集中策略,worker 只出流事实(「host 不信任 worker」)。输入为
//! 归一化后的容器扩展名 + 视频 codec/profile/位深 + 音轨列表,输出五态
//! [`PlaybackVerdict`]。判定表(§5.1)逐行硬编 + 单测钉死;HEVC 一项由前端
//! `MediaCapabilities.decodingInfo` 实测覆盖修正(D-444 ④)。

/// Chromium/WebView2 原生可解容器(文件扩展名,小写)。§5.1 表首行。
const DIRECT_CONTAINERS: &[&str] = &["mp4", "m4v", "webm", "ogv"];
/// mp4/mov 系容器:HEVC 走 `NeedsHevcExt` 引导(容器本可直播,仅缺 HEVC 系统解码)。
const HEVC_EXT_CONTAINERS: &[&str] = &["mp4", "m4v", "mov"];
/// Chromium/WebView2 可解视频 codec(已归一)。
const DIRECT_VCODECS: &[&str] = &["h264", "vp8", "vp9", "av1"];
/// Chromium/WebView2 可解音频 codec(已归一)。
const DIRECT_ACODECS: &[&str] = &["aac", "mp3", "opus", "vorbis", "flac"];

/// 五态判定结果(design.md §5.1)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackVerdict {
    /// 现状路径,零改动——容器+视频+音频均 webview 可解。
    DirectPlay,
    /// mp4/mov 装 HEVC:前端实测系统硬解,可解→直播,不可解→引导装扩展或转码(§10 开放问题 4)。
    NeedsHevcExt,
    /// 容器改封(`-c copy`,秒级):视频+音频 webview 可解,仅容器不认。
    Remux,
    /// 半转码:视频 webview 可解、仅音轨不支持(AC-3/DTS/E-AC-3/TrueHD…)→ `-c:v copy -c:a aac`。
    RemuxAudioTranscode {
        /// 选中转码的音轨索引(disposition=default 优先,§9.2)。
        audio_track_index: u32,
    },
    /// 一次性全转码 H.264/AAC:视频 codec 本身 webview 不可解(vc1/mpeg2/rv40/msmpeg4…或
    /// 无扩展的 HEVC 落非直播容器)。
    Transcode,
}

/// 单条音轨事实(判定所需最小投影;来自 `VideoProbeInfo.audio_tracks`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrackFacts {
    pub index: u32,
    /// codec 名(判定前经 [`normalize_codec`] 归一)。
    pub codec: String,
    /// disposition=default 标记(多音轨选轨依据,§9.2)。
    pub is_default: bool,
}

/// 判定输入(归一化流事实)。`container_ext` 为**文件扩展名**(非 ffprobe `format_name`),
/// 由 host 侧从 `media_items.file_format` 或探测容器名映射而来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyInput {
    /// 容器扩展名(小写,如 `"mkv"`)。
    pub container_ext: String,
    /// 视频 codec(判定前经 [`normalize_codec`] 归一)。
    pub video_codec: String,
    /// 视频 profile(保留:v1 判定表不据此分流,HDR/10bit 由 UI 提示,§9.4)。
    pub video_profile: Option<String>,
    /// 位深(保留:同上)。
    pub bit_depth: Option<u8>,
    /// 音轨列表(空 = 无音轨源,§9.9)。
    pub audio_tracks: Vec<AudioTrackFacts>,
}

/// 归一化 codec 名:小写 + 常见别名折叠(容器/探测器命名差异)。未知名原样返回小写。
pub fn normalize_codec(codec: &str) -> String {
    let c = codec.trim().to_ascii_lowercase();
    match c.as_str() {
        // 视频别名。
        "avc" | "avc1" | "h.264" | "x264" => "h264".to_string(),
        "hevc" | "h265" | "h.265" | "hvc1" | "hev1" | "x265" => "hevc".to_string(),
        "av01" => "av1".to_string(),
        "vp09" => "vp9".to_string(),
        "vp08" => "vp8".to_string(),
        "vc-1" => "vc1".to_string(),
        "mpeg2video" => "mpeg2".to_string(),
        // 音频别名。
        "mp4a" | "aac_latm" => "aac".to_string(),
        "mp3float" | "mp3adu" => "mp3".to_string(),
        "eac3" | "ac-3" => c, // 保留精确名(非直放行,仅用于「不可解」判定)
        _ => c,
    }
}

/// 归一后 codec 是否为 HEVC。
fn is_hevc(vcodec: &str) -> bool {
    vcodec == "hevc"
}

/// 容器扩展名是否 Chromium/WebView2 原生可解(供 DB 兜底路径在无探测时乐观直播判断)。
pub fn is_direct_container(ext: &str) -> bool {
    DIRECT_CONTAINERS.contains(&ext.trim().to_ascii_lowercase().as_str())
}

/// codec(**须已归一**,调用方先过 [`normalize_codec`])是否 webview 可解视频 codec。
pub fn is_direct_video_codec(normalized: &str) -> bool {
    DIRECT_VCODECS.contains(&normalized)
}

/// 选中音轨索引(§9.2:default 优先),供 transcode 下发 `audio_track_index`。无音轨 → `None`。
pub fn selected_audio_index(input: &PolicyInput) -> Option<u32> {
    select_audio_track(&input.audio_tracks).map(|t| t.index)
}

/// 选轨(§9.2):disposition=default 优先;否则首条 webview 可解轨;再否则首条。返回 `None` = 无音轨。
fn select_audio_track(tracks: &[AudioTrackFacts]) -> Option<&AudioTrackFacts> {
    if tracks.is_empty() {
        return None;
    }
    tracks
        .iter()
        .find(|t| t.is_default)
        .or_else(|| {
            tracks
                .iter()
                .find(|t| DIRECT_ACODECS.contains(&normalize_codec(&t.codec).as_str()))
        })
        .or_else(|| tracks.first())
}

/// 判定表(design.md §5.1)。纯函数。`input.video_codec` / 各音轨 codec 调用前**无需**预归一——
/// 本函数内部统一走 [`normalize_codec`]。
pub fn decide(input: &PolicyInput) -> PlaybackVerdict {
    let container = input.container_ext.trim().to_ascii_lowercase();
    let vcodec = normalize_codec(&input.video_codec);

    // 行②/⑤:HEVC。非 mp4/mov 容器无扩展直解不成立 → 全转码。mp4/mov 则**先查选中音轨可解性**:
    // NeedsHevcExt 是把**原文件裸交**前端实测系统硬解,若选中音轨本身 webview 不可解(ac3/dts…),
    // 即便装了 HEVC 扩展也只有画面没声音——此时不能裸交,须走全转码(视频+音轨一起转,§V6-3)。
    // 音轨可解(或无音轨)才 NeedsHevcExt。
    if is_hevc(&vcodec) {
        if !HEVC_EXT_CONTAINERS.contains(&container.as_str()) {
            return PlaybackVerdict::Transcode;
        }
        return match select_audio_track(&input.audio_tracks) {
            Some(track) if !DIRECT_ACODECS.contains(&normalize_codec(&track.codec).as_str()) => {
                PlaybackVerdict::Transcode
            }
            _ => PlaybackVerdict::NeedsHevcExt,
        };
    }

    // 行⑤:视频 codec 本身 webview 不可解(vc1/mpeg2/rv40/msmpeg4/wmv3/未知…)→ 全转码。
    if !DIRECT_VCODECS.contains(&vcodec.as_str()) {
        return PlaybackVerdict::Transcode;
    }

    // 至此视频 codec webview 可解。据容器直播能力 × 选中音轨可解性分流。
    let container_direct = DIRECT_CONTAINERS.contains(&container.as_str());
    match select_audio_track(&input.audio_tracks) {
        // 无音轨(§9.9):直播容器 → DirectPlay;非直播容器 → Remux(仅改封,音轨缺失分支须容错)。
        None => {
            if container_direct {
                PlaybackVerdict::DirectPlay
            } else {
                PlaybackVerdict::Remux
            }
        }
        Some(track) => {
            let audio_ok = DIRECT_ACODECS.contains(&normalize_codec(&track.codec).as_str());
            match (container_direct, audio_ok) {
                // 行①:容器+视频+音频全可解。
                (true, true) => PlaybackVerdict::DirectPlay,
                // 直播容器但音轨不可解(如 mp4 装 ac3):仅转音轨。
                (true, false) => PlaybackVerdict::RemuxAudioTranscode {
                    audio_track_index: track.index,
                },
                // 行③:非直播容器 + 视频音频均可解 → 秒级改封。
                (false, true) => PlaybackVerdict::Remux,
                // 行④:非直播容器 + 音轨不可解 → 半转码。
                (false, false) => PlaybackVerdict::RemuxAudioTranscode {
                    audio_track_index: track.index,
                },
            }
        }
    }
}
