//! 基于 `lofty` 读取音频元数据、内嵌封面和歌词。
//!
//! 富化、封面派生与播放器详情共用此处的读取和提取函数。

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

/// 读取音频文件的标签 + 属性。缺失标签映射为 `None`（文件可能完全无标签块 —— 仍从音频属性返回
/// 编解码/时长）。
pub fn read_tags(path: &Path) -> Result<AudioTags> {
    Ok(tags_from(&open(path)?))
}

/// 内嵌封面图的原始字节（`jpeg`/`png`/…），优先取正面封面。返回 `(字节, 文件扩展名)`；
/// 文件无内嵌图片则为 `None`。
pub fn read_cover(path: &Path) -> Result<Option<(Vec<u8>, &'static str)>> {
    Ok(cover_from(&open(path)?))
}

/// 一次解析同时取标签与封面，供 `get_audio_detail` 复用。
// 返回（标签，可选封面（字节，文件扩展名）），一次解析得到两种结果。
#[allow(clippy::type_complexity)]
pub fn read_all(path: &Path) -> Result<(AudioTags, Option<(Vec<u8>, &'static str)>)> {
    let tagged = open(path)?;
    Ok((tags_from(&tagged), cover_from(&tagged)))
}

/// 一次性打开并解析文件标签。
fn open(path: &Path) -> Result<lofty::file::TaggedFile> {
    read_from_path(path)
        .map_err(|e| AppError::AudioMetadata(format!("lofty read failed | 音频读取失败: {e}")))
}

/// 从已解析的文件提取 `AudioTags`。
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

/// 从已解析的文件提取封面图片。
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
        // 未识别的图片类型沿用 jpg 默认扩展名。
        _ => "jpg",
    };
    Some((pic.data().to_vec(), ext))
}

/// 查找同目录同名的歌词文件，如 `song.mp3` → `song.lrc`；依次尝试 `.lrc` 与 `.LRC`。
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

/// 从**已读取**的标签 + 文件路径为播放器解析歌词：先内嵌，再同名 `.lrc`。返回 `(文本, 是否带时间轴)`
/// —— 文本含 LRC `[mm:ss]` 时间标签则为 true。接收预读标签，使调用方（如 `get_audio_detail`）无需重复解析文件。
pub fn lyrics_from_tags(tags: &AudioTags, path: &Path) -> (Option<String>, bool) {
    // 内嵌歌词优先（已在标签内，常见情形无需额外读文件）。
    if let Some(text) = tags.lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
        return (Some(text.to_string()), is_synced_lrc(text));
    }
    if let Some(lrc) = find_lrc(path) {
        if let Ok(bytes) = std::fs::read(&lrc) {
            // 复用文本解码以读取非 UTF-8 歌词，并剥离会遮住首行时间标签的 BOM。
            let text = crate::reader::encoding::decode_bytes(&bytes, None).text;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_bom_sidecar_lyrics_are_read_without_modifying_source() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("song.mp3");
        let lrc = audio.with_extension("lrc");
        let text = "[00:01.50]中文歌词🎵\r\n[00:03.00]第二行";
        let bytes: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(&audio, b"audio fixture").unwrap();
        std::fs::write(&lrc, &bytes).unwrap();

        let (lyrics, synced) = lyrics_from_tags(&AudioTags::default(), &audio);

        assert_eq!(std::fs::read(&lrc).unwrap(), bytes);
        assert_eq!(std::fs::read(&audio).unwrap(), b"audio fixture");
        assert_eq!(lyrics.as_deref(), Some(text));
        assert!(synced);
    }

    #[test]
    fn utf8_bom_sidecar_first_timestamp_remains_synced() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("song.mp3");
        let text = "[00:01.50]唯一一行歌词";
        let bytes = format!("\u{feff}{text}").into_bytes();
        let lrc = audio.with_extension("lrc");
        std::fs::write(&lrc, &bytes).unwrap();

        let (lyrics, synced) = lyrics_from_tags(&AudioTags::default(), &audio);

        assert_eq!(std::fs::read(&lrc).unwrap(), bytes);
        assert!(synced, "BOM 不应遮住首行的时间标签");
        assert_eq!(lyrics.as_deref(), Some(text));
    }
}
