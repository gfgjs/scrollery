//! 用于媒体项操作的 Tauri IPC 命令（§ 6.1 — 媒体查询）。

use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use tauri::State;

use crate::db::models::{AppStats, DirFile, DirNode, MediaDetail, MediaItem, SelectionDescriptor};
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::scanner::metadata::read_image_dimensions;
use crate::state::AppState;
use crate::tree::TreeMediaCategory;
use crate::utils::path::resolve_media_path;

/// 按需为给定的、仍是 0×0 占位的项（通常是刚滚动到的可视窗口）提取真实像素尺寸。
/// 仅读文件头（+ JPEG 方向），并行执行并更新数据库；返回成功测量的数量。前端随后
/// 重算布局，使方块贴回正确比例 —— 抢在自上而下的后台 enrichment 之前。
#[tauri::command]
pub async fn prioritize_dimensions(
    item_ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<usize> {
    // span 埋点(W1,D-312 info 档:并行文件头读取+批量写,真实 IO/CPU 工作;非候选文件明显重负载
    // 遗漏补入)。
    let _span = crate::logging::SpanTimer::info("ipc:prioritize_dimensions");
    if item_ids.is_empty() {
        return Ok(0);
    }

    // R1-3：DB 读 + rayon 并行文件头读取（par_iter 会阻塞当前线程）+ 批量写，整段离开 tokio worker。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<usize> {
        // 仅解析占位项的路径（跳过已测量的项）。
        let targets: Vec<(i64, String, String)> = {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            item_ids
                .iter()
                .filter_map(|&id| {
                    q::get_placeholder_item_path(&pool, id)
                        .ok()
                        .flatten()
                        .map(|(path, ext)| (id, path, ext))
                })
                .collect()
        };
        if targets.is_empty() {
            return Ok(0);
        }

        // 并行读取真实尺寸（文件头 + JPEG 方向）。
        let results: Vec<(i64, i64, i64)> = targets
            .par_iter()
            .filter_map(|(id, path, ext)| {
                let (w, h) = read_image_dimensions(Path::new(path), ext);
                if w > 0 && h > 0 {
                    Some((*id, w, h))
                } else {
                    None
                }
            })
            .collect();

        let n = results.len();
        if n > 0 {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tx = conn.unchecked_transaction()?;
            for (id, w, h) in &results {
                q::update_media_dimensions(&tx, *id, *w, *h)?;
            }
            tx.commit()?;
            // S1：尺寸是布局几何输入 → 就地 patch items 取数缓存（0×0 守卫同 SQL），不失效
            // 不 bump——补尺寸阶段高频写走失效会让缓存长冷；前端随后重算布局即得新比例。
            state_arc.set_dimensions_cached(&results);
        }
        Ok(n)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// R1-3 助手已提升为全 ipc/ 共享模块（本文件首建，随全量清扫上移）。
use super::blocking::{read_blocking, write_blocking};

/// 获取单个媒体项的完整详细信息。
#[tauri::command]
pub async fn get_media_detail(id: i64, state: State<'_, Arc<AppState>>) -> Result<MediaDetail> {
    read_blocking(&state, move |c| q::get_media_detail(c, id)).await
}

/// 仅为可视区批量获取重型逐项元数据（文件名、目录路径、EXIF、GPS）——
/// 这些字段已从常驻布局缓存剥离，以支撑百万项内存目标。
#[tauri::command]
pub async fn get_meta_for_viewport(
    ids: Vec<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::db::models::MediaMeta>> {
    // span 埋点(W1,D-312 debug 档:视口滚动热路径查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:get_meta_for_viewport");
    read_blocking(&state, move |c| q::get_media_meta_batch(c, &ids)).await
}

/// 获取相邻的媒体项详细信息。
#[tauri::command]
pub async fn get_adjacent_media(
    current_id: i64,
    offset: isize,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<MediaDetail>> {
    let adj_id = crate::layout::cache::get_adjacent_item(&state.layout_cache, current_id, offset);
    if let Some(id) = adj_id {
        let detail = get_media_detail(id, state).await?;
        Ok(Some(detail))
    } else {
        Ok(None)
    }
}

/// 按指定布局版本获取重复镜头中的相邻媒体项。
///
/// 与普通 `get_adjacent_media` 的区别是：镜头导航必须带上布局版本，后端直接从
/// `LayoutCache` 的扁平序与 O(1) id 索引查找，过期时返回 `ViewStale`，绝不退回普通
/// 数据库邻接序，也不把整个 `flat_ids` 物化到前端。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensAdjacentMedia {
    pub detail: MediaDetail,
    pub index: usize,
    pub total_count: usize,
}

#[tauri::command]
pub async fn get_lens_adjacent_media(
    current_id: i64,
    offset: isize,
    layout_version: u64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<LensAdjacentMedia>> {
    let adjacent = crate::layout::cache::get_lens_adjacent_item(
        &state.layout_cache,
        current_id,
        offset,
        layout_version,
    );

    let (id, index, total_count) = match adjacent {
        crate::layout::cache::LensAdjacentLookup::LayoutNotReady => {
            return Err(AppError::LayoutNotReady)
        }
        crate::layout::cache::LensAdjacentLookup::ViewStale
        | crate::layout::cache::LensAdjacentLookup::CurrentMissing => {
            return Err(AppError::ViewStale)
        }
        crate::layout::cache::LensAdjacentLookup::OutOfBounds => return Ok(None),
        crate::layout::cache::LensAdjacentLookup::Found {
            id,
            index,
            total_count,
        } => (id, index, total_count),
    };

    let detail = get_media_detail(id, state).await?;
    Ok(Some(LensAdjacentMedia {
        detail,
        index,
        total_count,
    }))
}

/// 获取实况照片（Live Photo）关联文件的可播放视频 URL。
/// 返回绝对文件路径（调用者使用 convertFileSrc 进行包装）。
#[tauri::command]
pub async fn get_companion_video_url(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    // span 埋点(W1,D-312 info 档:全文件读取提取嵌入 MP4,可达数十 MB,真实 IO 工作;
    // 非候选文件明显重负载遗漏补入)。
    let _span = crate::logging::SpanTimer::info("ipc:get_companion_video_url");
    // R1-3：DB 读 + 全文件读取提取嵌入 MP4（可达数十 MB）整段离开 tokio worker。
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;

        // 卷离线守门（T13 §3.7）：所在卷离线 → 直接返 VolumeOffline（携卷标签），
        // 前端弹「请插入设备 <label>」而非任由后续路径解析失败给破图/泛化错误。
        if let Some(label) = q::get_item_volume_offline_label(&pool, item_id)? {
            return Err(AppError::VolumeOffline(label));
        }

        // 检查项目是否有配套的 MOV 文件（Apple 实况照片）
        let companion_id = q::get_companion_item_id(&pool, item_id);

        if let Ok(Some(comp_id)) = companion_id {
            let (root, rel, name) = q::get_item_path_info(&pool, comp_id)?;
            return Ok(resolve_media_path(&root, &rel, &name));
        }

        // 检查是否有嵌入式视频（Google/Samsung 动态照片）
        // 纯读查询走读池,不与写路径抢 db_writer 锁(2026-07-06 审查 P0-1 顺带项)。
        let (has_embedded, cache_key): (bool, i64) = pool.query_row(
            "SELECT has_embedded_video, cache_key FROM media_items WHERE id=?1",
            rusqlite::params![item_id],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get(1)?)),
        )?;

        if has_embedded {
            let (root, rel, name) = q::get_item_path_info(&pool, item_id)?;
            let abs_path = resolve_media_path(&root, &rel, &name);

            // 检查动态视频缓存
            let cache_path = {
                let config = state_arc
                    .thumb_config
                    .read()
                    .unwrap_or_else(|e| e.into_inner());
                crate::thumbnail::cache::motion_video_cache_path(&config.cache_dir, cache_key)
            };

            if cache_path.exists() {
                return Ok(cache_path.to_string_lossy().replace('\\', "/"));
            }

            // 从 JPEG 中提取（读取尾部字节）
            let video_bytes = extract_embedded_mp4(&abs_path)?;
            if let Some(parent) = cache_path.parent() {
                std::fs::create_dir_all(parent).map_err(AppError::from)?;
            }
            // 原子落盘(2026-07-06 审查 P0-1):内嵌 MP4 可达数十 MB,命中判定只查 exists(),
            // 直写最终路径会让崩溃/断电留下的半截文件永久被当作有效缓存。
            crate::thumbnail::generator::write_atomic(&cache_path, &video_bytes)
                .map_err(AppError::from)?;
            return Ok(cache_path.to_string_lossy().replace('\\', "/"));
        }

        Err(AppError::MediaNotFound(item_id))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 获取视频的关键帧雪碧图 URL（若已生成，§3.3）。返回绝对路径（调用者用 `convertFileSrc` 包装），
/// 尚无雪碧图则返回 `None`。支撑超大 / 网络盘视频的悬停 scrub 降级（§3.1）。
#[tauri::command]
pub async fn get_keyframe_sprite(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    let Some(rel) =
        read_blocking(&state, move |c| q::get_keyframe_sprite_payload(c, item_id)).await?
    else {
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
    let abs = cache_dir.join(&rel);
    // 冷路径存在性自愈(2026-07-06 审查 P1-4):sprite 文件被 LRU/清理删掉而 derivation 行仍
    // status=2 时,退回 pending 让 Coordinator 重建,本次返 None(前端 scrub 降级)。本命令是
    // 悬停 scrub 降级(冷路径),一次 stat 无性能顾虑;缩略图热路径的同类重建另议(设计决策)。
    if !abs.exists() {
        let _ = super::blocking::write_blocking(&state, move |c| {
            q::reset_derivation_for_item(c, item_id, "video_keyframes")
        })
        .await;
        state.wake_exotic(crate::exotic::coordinator::WakeReason::ConfigChanged);
        return Ok(None);
    }
    Ok(Some(abs.to_string_lossy().replace('\\', "/")))
}

/// 切换媒体项的收藏状态。
#[tauri::command]
pub async fn toggle_favorite(item_id: i64, state: State<'_, Arc<AppState>>) -> Result<bool> {
    // R1-3：写锁等待与执行都在 blocking 线程；布局缓存同步回到 async 侧（锁已释放）。
    let new_val = write_blocking(&state, move |c| q::toggle_favorite(c, item_id)).await?;
    // Keep the resident caches consistent so the star doesn't revert on
    // scroll-out/scroll-in (D3; S1 起 layout + items 双缓存成对 patch).
    // 同步常驻双缓存，避免滚出再滚回时收藏标记回退（D3）。
    state.set_favorite_cached(&[item_id], new_val);
    Ok(new_val)
}

/// R1-2（S4 收尾）：解析 `SelectionDescriptor`（读池）→ 在同一 blocking 线程上执行写闭包。
///
/// - 批量命令的入参从 `Vec<i64>` 迁为描述符：全选百万项时 IPC payload 只含视图描述 + 排除集，
///   与选区大小无关（T18 D4）；id 物化收敛到后端 SQL 层。
/// - resolve 与写全程在 `spawn_blocking` 内（CLAUDE.md 硬化：async command 内 rusqlite 调用
///   一律离开 tokio worker；模式同 `compute_layout`）。
/// - 空选区由写闭包侧自然短路（queries 批量助手对空集返回 0 / Ok）。
async fn resolve_then_write<T, F>(
    state: &State<'_, Arc<AppState>>,
    selection: SelectionDescriptor,
    write: F,
) -> Result<(Vec<i64>, T)>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection, &[i64]) -> Result<T> + Send + 'static,
{
    let version = current_layout_version(state);
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<(Vec<i64>, T)> {
        // 读连接仅在解析期间持有，写前释放（不占读池名额跨越写锁等待）。
        let ids = {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            q::resolve_selection(&pool, &selection, version)?
        };
        let conn = state_arc
            .db_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let out = write(&conn, &ids)?;
        Ok((ids, out))
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 批量设置选区收藏状态（R1-2/S4：入参迁 SelectionDescriptor，全选不整包传 id）。
#[tauri::command]
pub async fn batch_toggle_favorite(
    state: State<'_, Arc<AppState>>,
    selection: SelectionDescriptor,
    value: bool,
) -> Result<u64> {
    // span 埋点(W1,D-312 info 档:批量写类,单条 UPDATE+IN,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:batch_toggle_favorite");
    let (ids, affected) = resolve_then_write(&state, selection, move |conn, ids| {
        q::batch_set_favorite(conn, ids, value)
    })
    .await?;

    // Sync the resident caches so favorites survive scroll-out/scroll-in (D3; S1 双缓存).
    // 同步常驻双缓存，使收藏在滚出再滚回后仍保持（D3）。
    state.set_favorite_cached(&ids, value);

    // S4 验收证据：payload 与选区大小无关——解析出的实际规模只在此后端日志可见。
    tracing::info!(
        "Batch favorite(S4): resolved {} ids, affected {}, value {} | 批量收藏：解析 {} 项，影响 {} 行，值 {}",
        ids.len(),
        affected,
        value,
        ids.len(),
        affected,
        value
    );

    Ok(affected)
}

/// 设置媒体项的评分（0-5）。
#[tauri::command]
pub async fn set_rating(item_id: i64, rating: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    // R1-3：写走 write_blocking（断言测试补抓的漏网点）。
    write_blocking(&state, move |conn| {
        q::set_rating(conn, item_id, rating.clamp(0, 5))
    })
    .await?;
    // S1：评分双缓存 patch（minRating 过滤视图由 items_cache 内部整体失效）。
    state.set_rating_cached(&[item_id], rating.clamp(0, 5));
    Ok(())
}

/// 批量设置评分（0-5），单条 UPDATE + IN 完成。镜像 `batch_toggle_favorite`,支撑画廊键盘 1-5 对
/// 当前选区快捷评分（避免逐项 loop 在大选区上的 N 次 IPC 往返）。返回受影响行数。
/// 注：LayoutRowItem 携带 rating（网格星级显示），S1 起与 favorite 同样走双缓存 patch
/// （顺手修复：此前不 patch，行滚出滚回星级会回退）。
#[tauri::command]
pub async fn batch_set_rating(
    state: State<'_, Arc<AppState>>,
    selection: SelectionDescriptor,
    rating: i64,
) -> Result<u64> {
    // span 埋点(W1,D-312 info 档:批量写类,单条 UPDATE+IN,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:batch_set_rating");
    let (ids, affected) = resolve_then_write(&state, selection, move |conn, ids| {
        q::batch_set_rating(conn, ids, rating) // 0-5 钳制在 db 层
    })
    .await?;

    // S1：评分双缓存 patch（minRating 过滤视图整体失效）。
    state.set_rating_cached(&ids, rating.clamp(0, 5));

    tracing::info!(
        "Batch rating(S4): resolved {} ids, affected {}, rating {} | 批量评分：解析 {} 项，影响 {} 行，评分 {}",
        ids.len(),
        affected,
        rating.clamp(0, 5),
        ids.len(),
        affected,
        rating.clamp(0, 5)
    );

    Ok(affected)
}

/// 设置媒体项的颜色标签（0=无，1-7 色档）。
#[tauri::command]
pub async fn set_color_label(
    item_id: i64,
    color_label: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // R1-3：写走 write_blocking（断言测试补抓的漏网点）。
    write_blocking(&state, move |conn| {
        q::set_color_label(conn, item_id, color_label.clamp(0, 7))
    })
    .await?;
    // S1：色标双缓存 patch（colorLabel 过滤视图由 items_cache 内部整体失效）。
    state.set_color_label_cached(&[item_id], color_label.clamp(0, 7));
    Ok(())
}

/// 持久化媒体项的看图台展示旋转（归一化 0/90/180/270，顺时针；V20）。
/// 与评分/色标不同：`view_rotation` 不随 LayoutRowItem 进网格（旋转仅作用于大图查看），
/// 故无双缓存 patch——只落库即可。入参越界的角度在此归一到最近的 90° 步进。
#[tauri::command]
pub async fn set_view_rotation(
    item_id: i64,
    rotation: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 归一化到 0/90/180/270：先取模再取正（Rust % 对负数保留符号），再对齐到 90° 步进。
    let norm = ((rotation % 360) + 360) % 360 / 90 * 90;
    // R1-3：写走 write_blocking（rusqlite 不得在 async worker 上直接跑）。
    write_blocking(&state, move |conn| {
        q::set_view_rotation(conn, item_id, norm)
    })
    .await?;
    Ok(())
}

/// 持久化播放器播放位置记忆(V23)。与 `set_view_rotation` 同构:入参负值钳制为 0,
/// 只落库(播放进度不进网格,无双缓存 patch)。
#[tauri::command]
pub async fn set_playback_position(
    item_id: i64,
    ms: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let clamped = clamp_playback_position_ms(ms);
    // R1-3：写走 write_blocking（rusqlite 不得在 async worker 上直接跑）。
    write_blocking(&state, move |conn| {
        q::set_playback_position(conn, item_id, clamped)
    })
    .await?;
    Ok(())
}

/// `set_playback_position` 的 clamp 规则:入参负值钳制为 0,非负值原样透传。抽成纯函数
/// 便于单测(GA-fix:回归钉「负 ms 落库 0、正 ms 原样」的 IPC 层职责边界)。
fn clamp_playback_position_ms(ms: i64) -> i64 {
    ms.max(0)
}

#[cfg(test)]
mod playback_position_clamp_tests {
    use super::*;

    #[test]
    fn negative_ms_clamped_to_zero() {
        assert_eq!(clamp_playback_position_ms(-1), 0);
        assert_eq!(clamp_playback_position_ms(-999_999), 0);
    }

    #[test]
    fn nonnegative_ms_passed_through_unchanged() {
        assert_eq!(clamp_playback_position_ms(0), 0);
        assert_eq!(clamp_playback_position_ms(4200), 4200);
    }
}

/// 批量设置颜色标签（0-7），单条 UPDATE + IN 完成。镜像 `batch_set_rating`，支撑画廊对当前选区批量
/// 打色签（避免逐项 loop 在大选区上的 N 次 IPC）。返回受影响行数。
#[tauri::command]
pub async fn batch_set_color_label(
    state: State<'_, Arc<AppState>>,
    selection: SelectionDescriptor,
    color_label: i64,
) -> Result<u64> {
    // span 埋点(W1,D-312 info 档:批量写类,单条 UPDATE+IN,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:batch_set_color_label");
    let (ids, affected) = resolve_then_write(&state, selection, move |conn, ids| {
        q::batch_set_color_label(conn, ids, color_label) // 0-7 钳制在 db 层
    })
    .await?;

    // S1：色标双缓存 patch（colorLabel 过滤视图整体失效）。
    state.set_color_label_cached(&ids, color_label.clamp(0, 7));

    tracing::info!(
        "Batch color label(S4): resolved {} ids, affected {} | 批量色签：解析 {} 项，影响 {} 行",
        ids.len(),
        affected,
        ids.len(),
        affected
    );

    Ok(affected)
}

/// Soft-delete the resolved selection (mark is_deleted=1; Live Photo companion 连带在 db 层)。
/// 软删除选区（R1-2/S4：入参迁 SelectionDescriptor）。
#[tauri::command]
pub async fn soft_delete_items(
    selection: SelectionDescriptor,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:批量写类,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:soft_delete_items");
    let (ids, ()) = resolve_then_write(&state, selection, |conn, ids| {
        q::soft_delete_items(conn, ids)
    })
    .await?;
    // S1：软删改变一切视图成员 → bump 数据版本（下次重排强制重查）。
    state.bump_data_version();
    tracing::info!(
        "Soft delete(S4): resolved {} ids | 软删除：解析 {} 项",
        ids.len(),
        ids.len()
    );
    Ok(())
}

/// 恢复软删除的项目（R1-2/S4：入参迁 SelectionDescriptor；撤销路径传 Explicit）。
#[tauri::command]
pub async fn restore_items(
    selection: SelectionDescriptor,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // span 埋点(W1,D-312 info 档:批量写类,真实 DB 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:restore_items");
    let (ids, ()) = resolve_then_write(&state, selection, q::restore_items).await?;
    // S1：恢复改变视图成员（回收站 ↔ 常规视图）→ bump。
    state.bump_data_version();
    tracing::info!(
        "Restore(S4): resolved {} ids | 恢复：解析 {} 项",
        ids.len(),
        ids.len()
    );
    Ok(())
}

/// 读当前布局版本（无布局 → 0）。SelectAll 解析以此对 `view.layout_version` 守门;
/// Explicit 解析忽略此值，故无布局时传 0 安全（不会误判 Explicit）。
/// `pub(crate)`：导出(方案 A)的选区解析复用同一版本源，不重复实现（`ipc::export_commands`）。
pub(crate) fn current_layout_version(state: &AppState) -> u64 {
    crate::layout::cache::get_summary(&state.layout_cache)
        .map(|s| s.layout_version)
        .unwrap_or(0)
}

/// 把 `SelectionDescriptor` 解析为实际 id 列表（按视图布局序）。Part5 S4：暴露为 IPC（纯新增）。
///
/// - `Explicit{ids}`：上限校验后原样返回。
/// - `SelectAll{view, excludedIds}`：`view.layoutVersion` 与当前布局不一致 → `ViewStale`；
///   否则经 `view_to_sql` 取全集 − 排除集。百万级全选在后端 SQL 解析，不经前端整包传 id。
///
/// R1-2（T4c）已落地：批量命令（favorite/rating/color/soft_delete/restore）直接收
/// SelectionDescriptor 在后端解析；本命令保留为通用「描述符 → id 列表」入口（前端仅在
/// 确需 id 的操作——移动/复制/加收藏夹等——使用物化路径）。
#[tauri::command]
pub async fn resolve_selection(
    selection: SelectionDescriptor,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<i64>> {
    let version = current_layout_version(&state);
    read_blocking(&state, move |c| {
        q::resolve_selection(c, &selection, version)
    })
    .await
}

/// 仅计数 `SelectionDescriptor`（UI「将操作 N 项」）。Part5 S4：暴露为 IPC（纯新增）。
/// SelectAll 走 `COUNT(*) − (excluded ∩ view)`，不取全 id（精确计数，T18 D3）。
#[tauri::command]
pub async fn count_selection(
    selection: SelectionDescriptor,
    state: State<'_, Arc<AppState>>,
) -> Result<u64> {
    let version = current_layout_version(&state);
    read_blocking(&state, move |c| q::count_selection(c, &selection, version)).await
}

/// 获取垃圾桶中的项目（分页）。
#[tauri::command]
pub async fn get_trash(
    offset: i64,
    limit: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MediaItem>> {
    read_blocking(&state, move |c| q::get_trash(c, offset, limit.min(200))).await
}

/// 获取整体应用统计信息。
#[tauri::command]
pub async fn get_stats(state: State<'_, Arc<AppState>>) -> Result<AppStats> {
    // 扫描期统计会触发画廊刷新；记录含排队的耗时，便于与布局开销分开核验。
    let _span = crate::logging::SpanTimer::debug("ipc:get_stats");
    read_blocking(&state, q::get_app_stats).await
}

/// 全部**已注册格式**的 UI 投影 = 内置表 ∪ exotic Catalog（S 线 §6 / D-007）。
///
/// 「已注册」= Host 能分类并允许入库；**与 availability 正交** —— Catalog 登记但插件未安装的 PSD
/// 仍算已注册（能筛出来，只是可能预览不了）。故 descriptor 只带 `ext/mediaType/group/source`，
/// 处理能力字段（`phase1_image`/`document_subtype`）**有意不下发**：下发了就迟早有人拿它当
/// 「能不能筛」用。
///
/// 前端据此把 facet（库内实际存在的扩展名，见 [`list_library_formats`](q::list_library_formats)）
/// 分到四大类、并把 `group` 合成 UI 别名（JPEG={jpg,jpeg} / RAW={cr2,…}）。
/// 🔴 RAW 列表必须**由此下发**，前端硬编码就是把 `utils::format` 的表抄了第二遍（D-003）。
///
/// 不查 DB，纯内存快照，无需 `spawn_blocking`。
#[tauri::command]
pub async fn list_registered_formats(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::formats::FormatDescriptor>> {
    Ok(crate::formats::merged_formats(
        &state.exotic_catalog.snapshot(),
    ))
}

/// 库内**实际存在**的格式（S 线 §7.2）。首版弹层只列这些 —— registry 67 项里有 38 项在本机为 0，
/// 全铺出来是 38 个点了必然空结果的选项。
///
/// 实测 0.0ms（走 `idx_media_format` 覆盖索引）→ 不缓存、不做失效，每次开弹层现查。
#[tauri::command]
pub async fn list_library_formats(state: State<'_, Arc<AppState>>) -> Result<Vec<String>> {
    read_blocking(&state, q::list_library_formats).await
}

/// 获取扫描根目录的完整目录树。
///
/// `categories` 为 `None` 或五类全选时不限；显式空集合表示仅目录（保留目录骨架）；
/// `Other` 不对应 DB `media_type`，仅选它时结果为空。注册格式模式只会命中四类 DB 类型。
#[tauri::command]
pub async fn get_directory_tree(
    root_id: i64,
    categories: Option<Vec<TreeMediaCategory>>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DirNode>> {
    read_blocking(&state, move |c| {
        q::get_directory_tree(c, root_id, categories.as_deref())
    })
    .await
}

/// 获取目录节点的直接子节点（延迟加载）。分类协议同 [`get_directory_tree`]。
#[tauri::command]
pub async fn get_directory_children(
    parent_id: i64,
    categories: Option<Vec<TreeMediaCategory>>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DirNode>> {
    read_blocking(&state, move |c| {
        q::get_directory_children(c, parent_id, categories.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn get_directory_ancestors(id: i64, state: State<'_, Arc<AppState>>) -> Result<Vec<i64>> {
    read_blocking(&state, move |c| q::get_directory_ancestors(c, id)).await
}

/// List the direct media files of a directory for the sidebar tree's file list
/// (lazy-loaded when a folder is expanded).
/// 列出某目录的直接媒体文件，供侧边栏树的文件列表使用（展开文件夹时懒加载）。
#[tauri::command]
pub async fn list_directory_files(
    directory_id: i64,
    limit: Option<i64>,
    offset: Option<i64>,
    categories: Option<Vec<TreeMediaCategory>>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DirFile>> {
    read_blocking(&state, move |c| {
        q::list_directory_files(c, directory_id, limit, offset, categories.as_deref())
    })
    .await
}
// ── Helpers ───────────────────────────────────────────────────────────────────
// ── 助手函数 ───────────────────────────────────────────────────────────────────

/// 从 Google/Samsung 动态照片 JPEG 中提取嵌入的 MP4。
fn extract_embedded_mp4(abs_path: &str) -> Result<Vec<u8>> {
    let data = std::fs::read(abs_path).map_err(AppError::from)?;
    let ftyp_marker = b"ftyp";
    for i in (4..data.len().saturating_sub(4)).rev() {
        if &data[i..i + 4] == ftyp_marker {
            let mp4_start = i - 4;
            if mp4_start + 8 < data.len() {
                return Ok(data[mp4_start..].to_vec());
            }
        }
    }
    Err(AppError::Internal(
        "No embedded MP4 found in Motion Photo".into(),
    ))
}
