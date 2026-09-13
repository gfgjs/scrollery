// src-tauri/src/audio/mod.rs
//! 音频元数据 / 封面图 / 歌词提取（P3，§3.6），基于 `lofty`（纯 Rust，符合轻量原则）。
//!
//! 本模块是音频能力的纯函数层，供三处复用：
//!  1. enricher（补全阶段）—— 回填 `audio_meta`（artist/album/track/year/genre + 时长 + 歌词来源）。
//!  2. 派生 `audio_cover` —— 提取内嵌封面 → 复用缩略图编码/缓存（见 derive/audio.rs）。
//!  3. `get_audio_detail` IPC —— 播放器按需读取完整标签 + 全分辨率封面 + 歌词文本（含 .lrc）。
//!
//! This is the pure-function layer for audio, reused by the enricher, the `audio_cover`
//! derivation, and the `get_audio_detail` IPC.

use std::path::{Path, PathBuf};

use lofty::prelude::*; // Accessor / AudioFile / TaggedFileExt / ItemKey
use lofty::read_from_path;

use crate::error::{AppError, Result};

/// 从音频文件读取的标签 + 属性（`audio_meta` 持久化的子集 + 时长）。
#[derive(Debug, Clone, Default)]
pub struct AudioTags {
    pub codec: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub title: Option<String>,
    pub track_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_ms: Option<i64>,
    /// 内嵌歌词文本（ID3 `USLT`、Vorbis `LYRICS`、MP4 `©lyr`…），若有。
    pub lyrics: Option<String>,
}

/// Lyrics provenance for `audio_meta.lyrics_source`. `embedded` = in the tag; `lrc` = sibling
/// `.lrc` file; `none` = neither. The detail view reads the actual text lazily by source.
/// `audio_meta.lyrics_source` 的歌词来源。`embedded` = 标签内嵌；`lrc` = 同名 `.lrc`；`none` = 无。
/// 详情视图按来源按需读取实际文本。
pub fn lyrics_source(tags: &AudioTags, lrc: &Option<PathBuf>) -> &'static str {
    if tags
        .lyrics
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        "embedded"
    } else if lrc.is_some() {
        "lrc"
    } else {
        "none"
    }
}

/// 由 lofty `FileType` 得到的简短稳定编解码标签（存入 `audio_meta.audio_codec`）。
fn codec_label(ft: lofty::file::FileType) -> &'static str {
    use lofty::file::FileType as F;
    match ft {
        F::Aac => "AAC",
        F::Aiff => "AIFF",
        F::Ape => "APE",
        F::Flac => "FLAC",
        F::Mpeg => "MP3",
        F::Mp4 => "AAC/ALAC",
        F::Mpc => "Musepack",
        F::Opus => "Opus",
        F::Vorbis => "Vorbis",
        F::Speex => "Speex",
        F::Wav => "WAV",
        F::WavPack => "WavPack",
        _ => "audio",
    }
}

/// Read tags + properties for an audio file. Missing tags map to `None` (a file may carry no
/// tag block at all — still returns codec/duration from the audio properties).
/// 读取音频文件的标签 + 属性。缺失标签映射为 `None`（文件可能完全无标签块 —— 仍从音频属性返回
/// 编解码/时长）。
pub fn read_tags(path: &Path) -> Result<AudioTags> {
    Ok(tags_from(&open(path)?))
}

/// Embedded cover art as raw image bytes (`jpeg`/`png`/…), preferring the front-cover picture.
/// Returns `(bytes, file_extension)`; `None` if the file carries no embedded picture.
/// 内嵌封面图的原始字节（`jpeg`/`png`/…），优先取正面封面。返回 `(字节, 文件扩展名)`；
/// 文件无内嵌图片则为 `None`。
pub fn read_cover(path: &Path) -> Result<Option<(Vec<u8>, &'static str)>> {
    Ok(cover_from(&open(path)?))
}

/// Read tags AND cover art in a SINGLE file parse — used by `get_audio_detail`, which otherwise
/// would parse the same file up to 3× (tags + lyrics + cover). 一次解析同时取标签与封面 ——
/// 供 `get_audio_detail` 用，避免对同一文件解析多达 3 次（标签 + 歌词 + 封面）。
// 返回 (标签, 可选封面(字节, MIME))，单次解析双产物的固有形状，抽别名收益有限。
#[allow(clippy::type_complexity)]
pub fn read_all(path: &Path) -> Result<(AudioTags, Option<(Vec<u8>, &'static str)>)> {
    let tagged = open(path)?;
    Ok((tags_from(&tagged), cover_from(&tagged)))
}

/// Open + decode a file's tags once. 一次性打开并解码文件标签。
fn open(path: &Path) -> Result<lofty::file::TaggedFile> {
    read_from_path(path)
        .map_err(|e| AppError::AudioMetadata(format!("lofty read failed | 音频读取失败: {e}")))
}

/// Extract `AudioTags` from an already-parsed file. 从已解析的文件提取 `AudioTags`。
fn tags_from(tagged: &lofty::file::TaggedFile) -> AudioTags {
    let props = tagged.properties();
    let duration_ms = {
        let ms = props.duration().as_millis();
        if ms > 0 {
            Some(ms as i64)
        } else {
            None
        }
    };
    let codec = Some(codec_label(tagged.file_type()).to_string());

    // primary_tag（该格式的规范标签）→ first_tag 兜底（如仅 ID3v1）。
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let mut out = AudioTags {
        codec,
        duration_ms,
        ..Default::default()
    };
    if let Some(tag) = tag {
        out.artist = tag
            .artist()
            .map(|c| c.to_string())
            .filter(|s| !s.trim().is_empty());
        out.album = tag
            .album()
            .map(|c| c.to_string())
            .filter(|s| !s.trim().is_empty());
        out.title = tag
            .title()
            .map(|c| c.to_string())
            .filter(|s| !s.trim().is_empty());
        out.track_no = tag.track().map(|n| n as i64);
        out.year = tag.year().map(|n| n as i64);
        out.genre = tag
            .genre()
            .map(|c| c.to_string())
            .filter(|s| !s.trim().is_empty());
        out.lyrics = tag
            .get_string(&ItemKey::Lyrics)
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
    }
    out
}

/// Extract the cover picture from an already-parsed file. 从已解析的文件提取封面图片。
fn cover_from(tagged: &lofty::file::TaggedFile) -> Option<(Vec<u8>, &'static str)> {
    use lofty::picture::{MimeType, PictureType};

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pics = tag.pictures();
    if pics.is_empty() {
        return None;
    }
    // 优先正面封面；否则退回第一张图片。
    let pic = pics
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .unwrap_or(&pics[0]);

    let ext = match pic.mime_type() {
        Some(MimeType::Png) => "png",
        Some(MimeType::Gif) => "gif",
        Some(MimeType::Bmp) => "bmp",
        Some(MimeType::Tiff) => "tiff",
        // Jpeg / Unknown / None → default to jpg (the most common embedded format).
        _ => "jpg",
    };
    Some((pic.data().to_vec(), ext))
}

/// Locate a sibling `.lrc` lyrics file (same base name, same directory) — e.g. `song.mp3` →
/// `song.lrc`. Tries the exact case and a lowercase `.lrc` extension.
/// 查找同目录同名的 `.lrc` 歌词文件 —— 如 `song.mp3` → `song.lrc`。尝试原扩展名与小写 `.lrc`。
pub fn find_lrc(path: &Path) -> Option<PathBuf> {
    let lrc = path.with_extension("lrc");
    if lrc.exists() {
        return Some(lrc);
    }
    let lrc_upper = path.with_extension("LRC");
    if lrc_upper.exists() {
        return Some(lrc_upper);
    }
    None
}

/// Resolve displayable lyrics for the player from ALREADY-READ tags + the file path: embedded
/// lyrics first, then a sibling `.lrc`. Returns `(text, is_synced)` where `is_synced` is true if
/// the text carries LRC `[mm:ss]` timestamps (the frontend then highlights/scrolls in sync). Takes
/// pre-read tags so callers (e.g. `get_audio_detail`) don't parse the file again.
/// 从**已读取**的标签 + 文件路径为播放器解析歌词：先内嵌，再同名 `.lrc`。返回 `(文本, 是否带时间轴)`
/// —— 文本含 LRC `[mm:ss]` 时间标签则为 true。接收预读标签，使调用方（如 `get_audio_detail`）无需重复解析文件。
pub fn lyrics_from_tags(tags: &AudioTags, path: &Path) -> (Option<String>, bool) {
    // 内嵌歌词优先（已在标签内，常见情形无需额外读文件）。
    if let Some(text) = tags.lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
        return (Some(text.to_string()), is_synced_lrc(text));
    }
    if let Some(lrc) = find_lrc(path) {
        if let Ok(text) = std::fs::read_to_string(&lrc) {
            let synced = is_synced_lrc(&text);
            return (Some(text), synced);
        }
    }
    (None, false)
}

/// 启发式判断：歌词文本是否至少含一个 `[mm:ss(.xx)]` LRC 时间标签？
fn is_synced_lrc(text: &str) -> bool {
    text.lines().any(|line| {
        let l = line.trim_start();
        // 轻量形状检查：`[` 后接数字，且 `]` 前含 `:`。
        if let Some(rest) = l.strip_prefix('[') {
            if let Some(close) = rest.find(']') {
                let inside = &rest[..close];
                return inside.contains(':')
                    && inside.chars().next().is_some_and(|c| c.is_ascii_digit());
            }
        }
        false
    })
}
