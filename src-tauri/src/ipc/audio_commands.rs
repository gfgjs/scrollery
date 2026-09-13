//! Audio IPC commands (P3, §3.6). 音频相关 IPC 命令（P3，§3.6）。
//!
//! `get_audio_detail`：音频播放器（路由 `/audio/:id`）打开一首曲目时调用，返回核心项 + 绝对路径 +
//! 标签元数据 + 全分辨率封面（按需抽取至缓存）+ 歌词（内嵌或 `.lrc`，带同步标记）。
//! 标签/歌词从文件**懒加载**，故既有库无需重扫即可工作；`audio_meta` DB 行（由 enricher 回填）
//! 仅用于列表/索引。

use std::path::Path;
use std::sync::Arc;

use tauri::State;

use crate::audio;
use crate::db::models::{AudioDetail, AudioMeta};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::state::AppState;

/// Full detail for the audio player (§3.6). Reads tags + lyrics lazily from the file (so it is
/// correct even before/without enrichment) and extracts the full-resolution embedded cover to the
/// cache on demand (`convertFileSrc`-able path).
/// 音频播放器的完整详情（§3.6）。从文件懒加载标签 + 歌词（即便未补全也正确），并按需把全分辨率
/// 内嵌封面抽取至缓存（返回可 `convertFileSrc` 的路径）。
#[tauri::command]
pub async fn get_audio_detail(id: i64, state: State<'_, Arc<AppState>>) -> Result<AudioDetail> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<AudioDetail> {
        let detail = {
            let pool = state.db_read_pool.get()?;
            q::get_media_detail(&pool, id)?
        };
        let abs = detail.abs_path.clone();
        let path = Path::new(&abs);

        // Tags + cover in a SINGLE lofty parse (always fresh; defaults if unreadable). Reusing the
        // tags for lyrics/cover avoids re-parsing the same file 3× per detail open.
        // 标签 + 封面一次 lofty 解析（始终最新；不可读则回落默认）。复用标签做歌词/封面，避免每次打开
        // 详情对同一文件解析 3 次。
        let (tags, cover) = audio::read_all(path).unwrap_or_default();
        let lrc = audio::find_lrc(path);
        let lyrics_src = audio::lyrics_source(&tags, &lrc);
        let meta = AudioMeta {
            item_id: id,
            audio_codec: tags.codec.clone(),
            artist: tags.artist.clone(),
            album_title: tags.album.clone(),
            track_title: tags.title.clone(),
            track_no: tags.track_no,
            year: tags.year,
            genre: tags.genre.clone(),
            lyrics_source: Some(lyrics_src.to_string()),
            lyrics_path: lrc.as_ref().map(|p| p.to_string_lossy().replace('\\', "/")),
        };

        // 歌词（内嵌或 .lrc）+ 是否带时间轴，复用上面已读标签（不重复解析）。
        let (lyrics, lyrics_synced) = audio::lyrics_from_tags(&tags, path);

        // 全分辨率封面 → 把上面已抽出的字节写入缓存一次（以 cache_key 为键），已存在则复用。
        let cover_path = write_cover_to_cache(&state, cover, detail.item.cache_key)?;

        Ok(AudioDetail {
            item: detail.item,
            abs_path: abs,
            meta,
            cover_path,
            lyrics,
            lyrics_synced,
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// Write the (already-extracted) full-resolution embedded cover to
/// `<cache>/audio_covers/<cache_key>.<ext>` and return its forward-slash absolute path (within the
/// asset scope). `None` if there was no embedded art. No re-encode → cheap + lossless.
/// 把（已抽出的）全分辨率内嵌封面写入 `<cache>/audio_covers/<cache_key>.<ext>`，返回其正斜杠绝对
/// 路径（位于 asset 授权范围内）。无内嵌封面则返回 `None`。不重新编码 → 廉价且无损。
fn write_cover_to_cache(
    state: &AppState,
    cover: Option<(Vec<u8>, &'static str)>,
    cache_key: i64,
) -> Result<Option<String>> {
    let Some((bytes, ext)) = cover else {
        return Ok(None);
    };
    let cache_dir = {
        state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone()
    };
    // 路径经 cache.rs 单一事实源构造(R5):枚举清理/GC/统计按同一方案对账。
    let file = crate::thumbnail::cache::audio_cover_cache_path(&cache_dir, cache_key, ext);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(AppError::from)?;
    }
    // 仅当不存在时写入（给定 cache_key = path|mtime 的封面是不可变的）。
    // 原子落盘(2026-07-06 审查 R5):命中判定只查存在性,直写会让半截封面永久有效。
    if !file.exists() {
        crate::thumbnail::generator::write_atomic(&file, &bytes).map_err(AppError::from)?;
    }
    Ok(Some(file.to_string_lossy().replace('\\', "/")))
}
