//! 针对 Justified Layout（两端对齐布局）的 Tauri IPC 命令（§ 6.1 — 布局）。

use std::sync::Arc;

use tauri::State;

use crate::db::models::{
    DuplicateLensDescriptor, DuplicateLensMode, GalleryFilter, LayoutItem, MediaFilter, ViewScope,
};
use crate::db::queries::{
    list_duplicate_folder_lens_rows, list_duplicate_lens_members, query_dir_labels,
    query_item_ids_filename_order, query_layout_items, query_layout_items_canonical,
};
use crate::dedup::hash::DEDUP_HASH_VERSION;
use crate::error::{AppError, Result};
use crate::layout::cache::{
    dedup_summary, get_rows, get_summary, get_view_ids as cache_view_ids, store_layout_with_lens,
    LayoutCache, LayoutSummary,
};
use crate::layout::geometry::{median_measured_aspect, HydratedRow, LayoutParams, LayoutRow};
use crate::layout::grid_pack::compute_grid_layout;
use crate::layout::items_cache::{self, CachedOrder, ItemsCacheData};
use crate::layout::justified::compute_justified_layout;
use crate::layout::lens::{
    assemble_lens_groups, build_lens_projection, compute_lens_layout_grid,
    compute_lens_layout_justified, LensProjection,
};
use crate::layout::lens_folder::{
    assemble_lens_folder_clusters, compute_lens_folder_layout, LensFolderAssembly,
};
use crate::state::AppState;

/// 布局计算参数。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeLayoutParams {
    pub directory_id: Option<i64>,
    pub filters: Option<MediaFilter>,
    pub container_width: f64,
    pub row_height: f64,
    pub gap: f64,
    pub group_by: Option<String>,
    pub sort_within_group: Option<String>,
    pub sort_order: Option<String>,
    pub include_meta: Option<bool>,
    /// 布局模式：None / "justified" = 等高行（默认），"grid" = 均匀宫格（T20）。
    pub layout_mode: Option<String>,
    /// 无缝分组(#1):true 时排序仍按 group_by 聚合,打包按 none 语义(无分隔符、行跨组)。
    pub seamless: Option<bool>,
    /// 设备像素比(multi-tier serving,2026-08-16 阶段 2):出口拼装按「格尺寸×DPR」选最小
    /// 满足档。几何不依赖它,故不入幂等指纹;缺省 1.0。
    pub dpr: Option<f64>,
    /// 重复镜头（2026-09-02 主画廊重复项浏览方案 §10.2）：Some = 镜头布局（groups/folders）。
    /// 镜头下 group_by/sort_within/sort_order/seamless 被忽略（排列由镜头自带，方案 §6.3）；
    /// directory_id/filters 必须为空（全库范围）。
    pub duplicate_lens: Option<DuplicateLensDescriptor>,
}

/// 普通画廊布局键（S3.1 幂等指纹）。**wire 形状表征锁**（`gen_key_gallery_is_unchanged`
/// 测试）：重复镜头改造不得触碰普通路径键格式——污染会摧毁普通画廊缓存命中。
#[allow(clippy::too_many_arguments)]
fn build_gallery_gen_key(
    filter_key: &str,
    group_by: &str,
    sort_within: &str,
    sort_order: &str,
    container_width: f64,
    target_row_height: f64,
    gap: f64,
    layout_mode: Option<&str>,
    include_meta: bool,
    seamless: bool,
    dv: u64,
) -> String {
    format!(
        "{}|{}|{}|{}|w{:.2}|h{:.2}|g{:.2}|m{}|meta{}|sl{}|dv{}",
        filter_key,
        group_by,
        sort_within,
        sort_order,
        container_width,
        target_row_height,
        gap,
        layout_mode.unwrap_or("justified"),
        include_meta,
        seamless,
        dv
    )
}

/// 镜头布局键（2026-09-02 方案 §12.3）：排列由镜头自带（无 group_by/sort/seamless 维度），
/// 失效维度 = dedup_view_epoch（§12.2 已发布代次）+ data_version + hash_version + 契约
/// 版本 + 独有项开关（P3 folders 用）+ 几何。普通路径键不含 lens 前缀，互不命中。
#[allow(clippy::too_many_arguments)]
fn build_lens_gen_key(
    mode: &str,
    show_unique_items: bool,
    ordering_version: u32,
    dedup_view_epoch: u64,
    dv: u64,
    layout_mode: Option<&str>,
    container_width: f64,
    target_row_height: f64,
    gap: f64,
    include_meta: bool,
) -> String {
    format!(
        "lens:{mode}|u{show_unique_items}|ov{ordering_version}|ep{dedup_view_epoch}|dv{dv}|hv{}|m{}|w{container_width:.2}|h{target_row_height:.2}|g{gap:.2}|meta{include_meta}",
        DEDUP_HASH_VERSION,
        layout_mode.unwrap_or("justified"),
    )
}

/// 为给定 items 构建与之平行的 `filename_rank`（**B-file-iii MISS 取数路径**）：**优先全局
/// filter-invariant rank**（[`AppState::try_global_filename_ranks`]，免 DB、O(N) 映射）；全局未就绪 /
/// 非默认基集 → per-filter id-only NATURAL_CMP 查询兜底（B-file-i），并触发后台全局构建使下次即全局
/// 命中。孤儿 id（缓存有而 filename 查询无——FK 级联下不应出现）取 `u32::MAX` 排末尾，不 panic。
/// `conn` 仅兜底路径使用（全局命中时不碰 DB）。
fn resolve_filename_ranks(
    state: &Arc<AppState>,
    items: &[LayoutItem],
    filter: &MediaFilter,
    data_version: u64,
    conn: &rusqlite::Connection,
) -> Result<Vec<u32>> {
    if let Some(ranks) = state.try_global_filename_ranks(items, data_version) {
        return Ok(ranks);
    }
    // 全局未就绪 → 触发后台构建（幂等），本次用 per-filter id-only 查询兜底。
    state.spawn_global_filename_rank_build();
    let fname_ids = query_item_ids_filename_order(conn, filter)?;
    let mut rank_of: std::collections::HashMap<i64, u32> =
        std::collections::HashMap::with_capacity(fname_ids.len());
    for (r, id) in fname_ids.iter().enumerate() {
        rank_of.insert(*id, r as u32);
    }
    Ok(items
        .iter()
        .map(|it| rank_of.get(&it.id).copied().unwrap_or(u32::MAX))
        .collect())
}

/// **双键统一缓存**的惰性 `filename_rank` 填充：驻留命中源是 datetime 基准（[`CachedOrder::Canonical`]）
/// 而用户请求 filename 序时，为缓存 items 补 [`ItemsCacheData::filename_rank`]，此后 datetime↔filename
/// 互切全命中（免整行重查）。**B-file-iii**：优先全局 filter-invariant rank（读锁内直接映射、免 DB）；
/// 全局未就绪才退化 per-filter id-only 查询（B-file-i）+ 触发后台全局构建。
///
/// 锁纪律：全局命中路径在 items 读锁内一次映射填充（`try_global_filename_ranks` 取的是**另一把**
/// global rank 读锁，叶子锁、不反向嵌套）；兜底 DB 查询在 items 读锁**之外**做（避免长时间持读锁阻塞
/// 换代）。算得的 rank 依 (filter, data_version) 定，即便查询期间缓存换代，重取锁校验 order/dv/filter_key
/// 不符则丢弃白算、绝不误填旧数据。竞态双填由 `OnceLock::set` 天然去重（后者返 Err，忽略）。
fn ensure_filename_rank_for_hit(
    state: &Arc<AppState>,
    filter: &MediaFilter,
    filter_key: &str,
    data_version: u64,
) -> Result<()> {
    // 快速判定：当前命中源是否恰是「datetime 基准 + 键匹配 + 缺 rank」——是才付查询。
    let need = {
        let guard = state
            .layout_items_cache
            .read()
            .unwrap_or_else(|e| e.into_inner());
        matches!(guard.as_ref(), Some(d) if d.reusable
            && matches!(d.order, CachedOrder::Canonical)
            && d.data_version == data_version
            && d.filter_key == filter_key
            && d.filename_rank.get().is_none())
    };
    if !need {
        return Ok(());
    }
    // ── 优先全局 rank：读锁内直接映射填充（免 DB）。全局覆盖当前 items（默认基集子集）即成。──
    {
        let guard = state
            .layout_items_cache
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let Some(d) = guard.as_ref() else {
            return Ok(());
        };
        let still = d.reusable
            && matches!(d.order, CachedOrder::Canonical)
            && d.data_version == data_version
            && d.filter_key == filter_key
            && d.filename_rank.get().is_none();
        if !still {
            return Ok(()); // 期间已换代/已填，无需处理
        }
        if let Some(ranks) = state.try_global_filename_ranks(&d.items, data_version) {
            let _ = d.filename_rank.set(ranks);
            return Ok(());
        }
    }
    // ── 全局未就绪 / 非默认基集 → 触发后台构建 + per-filter id-only 查询兜底（B-file-i）。──
    state.spawn_global_filename_rank_build();
    // id-only filename 序查询（读池连接仅在此持有，不与 items 锁重叠）。
    let fname_ids = {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        query_item_ids_filename_order(&pool, filter)?
    };
    let mut rank_of: std::collections::HashMap<i64, u32> =
        std::collections::HashMap::with_capacity(fname_ids.len());
    for (r, id) in fname_ids.iter().enumerate() {
        rank_of.insert(*id, r as u32);
    }
    // 重取读锁，确认仍是同一 datetime 缓存，构建与 items 平行的 rank 数组并经 &self set。
    let guard = state
        .layout_items_cache
        .read()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(d) = guard.as_ref() {
        if matches!(d.order, CachedOrder::Canonical)
            && d.data_version == data_version
            && d.filter_key == filter_key
            && d.filename_rank.get().is_none()
        {
            // 孤儿 id（缓存有而 filename 查询无——FK 级联下不应出现）取 u32::MAX 排末尾，不 panic。
            let ranks: Vec<u32> = d
                .items
                .iter()
                .map(|it| rank_of.get(&it.id).copied().unwrap_or(u32::MAX))
                .collect();
            let _ = d.filename_rank.set(ranks);
        }
    }
    Ok(())
}

/// 计算给定过滤器的 Justified Layout（两端对齐布局）。
/// 返回布局摘要（行数、总高度、版本）。
/// 完整的行数据存储在内存缓存中。
#[tauri::command]
pub async fn compute_layout(
    params: ComputeLayoutParams,
    state: State<'_, Arc<AppState>>,
) -> Result<LayoutSummary> {
    // span 埋点(W1,D-312 debug 档:视口滚动热路径查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:compute_layout");
    // 标记为主动交互，使后台视频派生/AI 节流，不饿死这次 CPU 密集的重排（布局被视频派生阻塞）。
    state.note_interaction();

    let filter = {
        let mut f = params.filters.unwrap_or_default();
        if let Some(dir_id) = params.directory_id {
            f.directory_id = Some(dir_id);
        }
        f
    };

    // ── 重复镜头（2026-09-02 方案 §10.2/§12）fail-closed 校验：镜头固定全库 + 空成员
    // 筛选（任何非默认字段显式拒绝，绝不静默忽略——否则 SelectAll/画面集合漂移）；
    // 合法镜头由下方独立管线处理。
    if let Some(lens) = &params.duplicate_lens {
        if params.directory_id.is_some() || filter != MediaFilter::default() {
            return Err(AppError::DuplicateLensInvalid(
                "重复镜头只支持全库范围且不带成员级筛选 | Duplicate lens supports the whole library without member filters"
                    .into(),
            ));
        }
        // folders 布局管线见 compute_lens_folder_blocking；orderingVersion
        // 等其余规则仍由 validate 把关。注意 view_to_sql 对 folders 仍显式拒绝（排列含
        // 内存二部图遍历，无 SQL 等价 lowering——browse-only 下 SelectAll 无消费）。
        lens.validate(&ViewScope::All, &GalleryFilter::default())?;
    }

    // 把（可能百万行的）查询与受限于 CPU 的布局算法放进同一个阻塞任务，
    // 二者均不阻塞 tokio 工作线程。
    let state_arc = state.inner().clone();
    let group_by = params
        .group_by
        .clone()
        .unwrap_or_else(|| "date".to_string());
    let sort_within = params
        .sort_within_group
        .clone()
        .unwrap_or_else(|| "datetime".to_string());
    let sort_order = params
        .sort_order
        .clone()
        .unwrap_or_else(|| "desc".to_string());
    let container_width = params.container_width.max(100.0);
    let target_row_height = params.row_height.max(50.0);
    let gap = params.gap.max(0.0);
    let layout_mode = params.layout_mode.clone();
    let seamless = params.seamless.unwrap_or(false);
    // DuplicateLensDescriptor 是 Copy：镜头分支在闭包内据此走独立管线。
    let duplicate_lens = params.duplicate_lens;

    // 多档源服务 DPR 上报(阶段 2):原子写即可,取行出口按最新值选档;同参数幂等命中
    // (提前 return)不受影响——DPR 不入指纹,收敛靠下一次 compute。
    if let Some(dpr) = params.dpr {
        state.thumb_serve_dpr.store(
            (dpr.clamp(0.25, 4.0) * 1000.0) as u32,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    // ── S3.1 幂等去重（前端重复触发治理）────────────────────────────────────────
    // 挂载/统计返回/尺寸观察等前端触发源可能以完全相同的输入连发 compute（mediaStore
    // 的在飞合并只排队、不比对参数）。布局是「有序快照 × 参数」的纯函数：快照可命中
    // （同 HIT 守卫判据）且现行布局代的构建指纹一致 ⇒ 输出必然逐项相同——直接复用现行
    // 摘要，免全量重排；**版本不换代**，前端 bucket 段表亦免于虚假重建。
    // 锁纪律：items 读锁与 layout 读锁先后独立取放，绝不重叠（S1）。
    let filter_key_probe = serde_json::to_string(&filter)
        .map_err(|e| AppError::internal("数据序列化失败 | serialization failed", e))?;
    let dv_probe = state.data_version();
    // 布局键分流（§12.3）：镜头键含 dedup_view_epoch/契约版本；普通键格式不变（表征锁测试）。
    let gen_key = if let Some(lens) = &params.duplicate_lens {
        build_lens_gen_key(
            if lens.mode == DuplicateLensMode::Groups {
                "groups"
            } else {
                "folders"
            },
            lens.show_unique_items,
            lens.ordering_version,
            state.dedup_view_epoch(),
            dv_probe,
            layout_mode.as_deref(),
            container_width,
            target_row_height,
            gap,
            params.include_meta.unwrap_or(false),
        )
    } else {
        build_gallery_gen_key(
            &filter_key_probe,
            &group_by,
            &sort_within,
            &sort_order,
            container_width,
            target_row_height,
            gap,
            layout_mode.as_deref(),
            params.include_meta.unwrap_or(false),
            seamless,
            dv_probe,
        )
    };
    // 镜头路径在 spawn_blocking 内独立管线（取 dedup 成员 + 全库 canonical 快照）；
    // 普通路径保持既有 HIT/MISS 管线不动。镜头的幂等复用只看布局键（同键 = 同
    // mode/epoch/dv/几何 → 布局必然逐项相同）。
    if params.duplicate_lens.is_some() {
        if let Some(summary) = dedup_summary(&state.layout_cache, &gen_key) {
            tracing::info!(
                "compute_layout LENS DEDUP: v{} unchanged | 同镜头键——复用现行布局(免重排免换代)",
                summary.layout_version
            );
            return Ok(summary);
        }
    } else if filter.ai_search != Some(true)
        && items_cache::is_hit_valid(
            &state.layout_items_cache,
            &filter_key_probe,
            dv_probe,
            &group_by,
            &sort_within,
            &sort_order,
        )
    {
        if let Some(summary) = dedup_summary(&state.layout_cache, &gen_key) {
            tracing::info!(
                "compute_layout DEDUP: v{} unchanged, {} rows | 同参数同数据代——复用现行布局(免重排免换代)",
                summary.layout_version,
                summary.total_rows
            );
            return Ok(summary);
        }
    }

    let (rows, total_height, lens_projection): (
        Vec<LayoutRow>,
        f64,
        Option<Arc<LensProjection>>,
    ) = tokio::task::spawn_blocking(
        move || -> Result<(Vec<LayoutRow>, f64, Option<Arc<LensProjection>>)> {
            let t0 = std::time::Instant::now();
            let layout_params = LayoutParams {
                container_width,
                target_row_height,
                gap,
                group_by: group_by.clone(),
                sort_within_group: sort_within.clone(),
                seamless,
            };
            // ── 重复镜头独立管线（2026-09-02 方案 §12.1）：dedup 证据 + 全库
            //    canonical 快照 → groups 组切片 / folders 关联簇 → 镜头打包。
            //    普通 HIT/MISS 分支完全不参与。
            if let Some(lens) = duplicate_lens {
                let dv = state_arc.data_version();
                let (rows, total_height, projection) = match lens.mode {
                    DuplicateLensMode::Groups => {
                        compute_lens_blocking(&state_arc, &layout_params, layout_mode.as_deref(), dv)?
                    }
                    DuplicateLensMode::Folders => compute_lens_folder_blocking(
                        &state_arc,
                        &layout_params,
                        layout_mode.as_deref(),
                        dv,
                        lens.show_unique_items,
                    )?,
                };
                return Ok((rows, total_height, Some(projection)));
            }
            // ai 搜索视图不可复用（S1/S3）：ai_search_results 随每次搜索整表重写，命中
            // 徒增失效面——每次全量重查；但快照仍驻留（reusable=false）作出口拼装的载荷源。
            let cacheable = filter.ai_search != Some(true);
            let is_datetime = sort_within == "datetime";
            // B-file-i/iii:filename 序 + none/folder/date 轴走「filename 基准 + 内存派生」(与 datetime
            // 家族对称,把轴/方向切换从每次 SQL NATURAL_CMP filesort 降为亚秒内存派生)。**date+filename
            // 现已纳入**(B-file-iii/D-018 A′):UTC 日桶 `sort_datetime.div_euclid(86400)` 内存分桶,与
            // SQL(已去 'localtime' 改 UTC 日界)逐值等价——单槽缓存服务任意 group×sort,根治 folder↔date
            // 换轴 thrash。can_derive_axis 是可派生性单一事实源,本判据与之同步(filename→none/folder/date)。
            let is_filename_derivable = sort_within == "filename"
                && (group_by == "none" || group_by == "folder" || group_by == "date");
            // 去重预检已序列化过一次，闭包直接接管该键（同一 filter，键必同一）。
            let filter_key = filter_key_probe;
            let dv = state_arc.data_version();

            // ── ①⁻ 双键统一缓存的惰性 filename_rank 预填：驻留命中源是 datetime 基准而请求 filename
            //    序时，先在 items 读锁外补一次 id-only NATURAL_CMP 查询得全局位次，映射回缓存建
            //    filename_rank，使随后 HIT 分支可跨键派 filename（datetime↔filename 互切免全量重查）。
            //    best-effort：查询失败仅记日志、不中断——filename_rank 仍缺 → order_ok 退化 MISS 一次。
            if cacheable && is_filename_derivable {
                if let Err(e) =
                    ensure_filename_rank_for_hit(&state_arc, &filter, &filter_key, dv)
                {
                    tracing::warn!(
                        "filename_rank 惰性预填失败(退化本次 MISS 全量重查): {}",
                        e
                    );
                }
            }

            // ── ① 命中：免 SQL —— items 读锁内派生序 + 布局（S1 锁纪律：items 读锁与
            //    layout_cache 写锁绝不重叠，store_layout 在本闭包返回后才执行）────────────
            if cacheable {
                let guard = state_arc.layout_items_cache.read().unwrap_or_else(|e| e.into_inner());
                if let Some(data) = guard.as_ref() {
                    // 命中判据须与 items_cache::is_hit_valid 逐字同判(S3.1 去重预检镜像本处)：
                    // 轴可派生性共用 can_derive_axis;唯一实现差异是 datetime 基准派 filename 需
                    // filename_rank 已就绪(惰性预填,见 ①⁻),未就绪则退化 MISS 一次而非以坏序命中。
                    let axis_ok = items_cache::can_derive_axis(&group_by, &sort_within);
                    let rank_ready = match &data.order {
                        CachedOrder::Canonical => {
                            !is_filename_derivable || data.filename_rank.get().is_some()
                        }
                        _ => true,
                    };
                    let order_ok = match &data.order {
                        CachedOrder::Canonical | CachedOrder::CanonicalFilename => {
                            axis_ok && rank_ready
                        }
                        CachedOrder::Sql {
                            group_by: g,
                            sort_within: s,
                            sort_order: o,
                        } => *g == group_by && *s == sort_within && *o == sort_order,
                    };
                    if data.reusable
                        && order_ok
                        && data.data_version == dv
                        && data.filter_key == filter_key
                    {
                        let ordered: Vec<&LayoutItem> = match data.order {
                            // 双键统一:任一基准都内存派生请求轴/方向;派生内部按请求轴取次键。
                            CachedOrder::Canonical | CachedOrder::CanonicalFilename => {
                                items_cache::derive_order(data, &group_by, &sort_within, &sort_order)
                            }
                            CachedOrder::Sql { .. } => data.items.iter().collect(),
                        };
                        let t_derive = t0.elapsed();
                        // S3.5：中位数缓存于快照（OnceLock,首次现算后驻留）。
                        let aspect = *data
                            .median_aspect
                            .get_or_init(|| median_measured_aspect(&data.items));
                        let (rows, total_height) = run_layout(
                            &ordered,
                            &layout_params,
                            layout_mode.as_deref(),
                            &data.dir_labels,
                            Some(aspect),
                        );
                        tracing::info!(
                            "compute_layout HIT: {} items, {} rows; derive {:.0}ms + layout {:.0}ms (axis={}/{}) | 取数缓存命中(免 SQL)",
                            data.items.len(),
                            rows.len(),
                            t_derive.as_secs_f64() * 1000.0,
                            (t0.elapsed() - t_derive).as_secs_f64() * 1000.0,
                            group_by,
                            sort_order
                        );
                        return Ok((rows, total_height, None));
                    }
                }
            }

            // ── ② miss：查询（datetime 家族走基准序免 JOIN 查询）→ 布局 → 回填缓存 ──────
            // 读连接仅在查询期间持有，CPU 密集的布局计算前即释放 —— 否则会把 4 个池连接之一
            // 钉住整个计算，拖慢滚动时并发的可视区读取。
            let (items, dir_labels, sql_ms, filename_ranks) = {
                let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
                // 布局排序 SQL 本体的独立计时点位：与随后的 query_dir_labels(10^3 级小查询)分离，
                // 单独量化「取数 + ORDER BY 排序」耗时。B-file-iii 后 filename 亦走 canonical 取数
                // （datetime 基准、无 collation FFI），仅 folder+similarity(ai) 仍下发 SQL 目录序排序。
                let t_sql = std::time::Instant::now();
                // **取数一律 canonical**（datetime 基准，无 collation FFI，走 idx_media_sort）——filename
                // 序由内存 filename_rank 派生（B-file-iii）：把 filename MISS 从全表 NATURAL_CMP filesort
                // （891ms release / 5279ms dev）降为 canonical 取数(158-319ms) + rank 映射。仅 similarity/ai
                // 走通用 SQL（reusable=false 不复用）。单槽缓存恒 Canonical → 服务任意 group×sort。
                let items = if cacheable && (is_datetime || is_filename_derivable) {
                    query_layout_items_canonical(&pool, &filter)?
                } else {
                    query_layout_items(
                        &pool,
                        &filter,
                        Some(&group_by),
                        Some(&sort_within),
                        Some(&sort_order),
                        false,
                    )?
                };
                let sql_ms = t_sql.elapsed();
                // filename 请求 → 预建与 items 平行的 filename_rank（优先全局免 DB；兜底 per-filter
                // id-only 查询 + 触发后台全局构建）。datetime 请求留空，切 filename 时经 ①⁻ ensure 补。
                let filename_ranks = if cacheable && is_filename_derivable {
                    Some(resolve_filename_ranks(&state_arc, &items, &filter, dv, &pool)?)
                } else {
                    None
                };
                // 目录标签映射恒取（量级 10^3 的小查询）：缓存驻留后可直接服务后续
                // folder 轴的内存派生，无需再碰 DB。
                let dir_labels = query_dir_labels(&pool)?;
                (items, dir_labels, sql_ms, filename_ranks)
            };
            let t_query = t0.elapsed();

            // 取数恒 canonical（B-file-iii）：cacheable 非 ai/similarity 视图统一 Canonical 基准，单槽
            // 缓存服务任意 group×sort（含 date+filename），根治换轴 thrash。CanonicalFilename 变体保留
            // （derive 逻辑 + 对拍测试仍在），但 MISS 路径不再产出——filename 一律经 datetime 基准派生。
            let order = if cacheable && (is_datetime || is_filename_derivable) {
                CachedOrder::Canonical
            } else {
                CachedOrder::Sql {
                    group_by: group_by.clone(),
                    sort_within: sort_within.clone(),
                    sort_order: sort_order.clone(),
                }
            };
            // filename 请求时 filename_rank 已在取数阶段预建；datetime 请求留空（首次切 filename 经
            // ①⁻ ensure 补）。OnceLock 一次性写。
            let filename_rank = std::sync::OnceLock::new();
            if let Some(ranks) = filename_ranks {
                let _ = filename_rank.set(ranks);
            }
            let data = ItemsCacheData {
                filter_key,
                order,
                data_version: dv,
                id_to_idx: items_cache::build_id_index(&items),
                dir_rank: items_cache::build_dir_rank(&dir_labels),
                items,
                dir_labels,
                filter: filter.clone(),
                reusable: cacheable,
                median_aspect: std::sync::OnceLock::new(),
                perm_memo: std::sync::Mutex::new(None),
                filename_rank,
            };
            let ordered: Vec<&LayoutItem> = match &data.order {
                CachedOrder::Canonical | CachedOrder::CanonicalFilename => {
                    items_cache::derive_order(&data, &group_by, &sort_within, &sort_order)
                }
                CachedOrder::Sql { .. } => data.items.iter().collect(),
            };
            let aspect = *data
                .median_aspect
                .get_or_init(|| median_measured_aspect(&data.items));
            let (rows, total_height) = run_layout(
                &ordered,
                &layout_params,
                layout_mode.as_deref(),
                &data.dir_labels,
                Some(aspect),
            );
            drop(ordered);
            let item_count = data.items.len();
            // S3：无条件驻留（含 ai_search）——快照同时是布局行的载荷源（出口拼装），
            // 不可复用视图（reusable=false）只是不参与命中，仍服务 get_*_rows 取载荷。
            items_cache::store_items(&state_arc.layout_items_cache, data);
            tracing::info!(
                "compute_layout MISS: {} items, {} rows; sql {:.0}ms + dirlabels {:.0}ms = query {:.0}ms, total {:.0}ms (axis={}/{}) | 取数缓存未命中(重查)",
                item_count,
                rows.len(),
                sql_ms.as_secs_f64() * 1000.0,
                // 余量 = query_dir_labels + 读池连接获取(t_sql 之前);folder-sort SQL 本体已单列。
                (t_query - sql_ms).as_secs_f64() * 1000.0,
                t_query.as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0,
                group_by,
                sort_within
            );
            Ok((rows, total_height, None))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    // S3 布局换代计时（S3.2 后=索引物化+指针交换；旧代 drop 已卸后台线程）。1M 级索引
    // 物化是 CPU 工作,同样不占 tokio worker——挪进 spawn_blocking(锁纪律不变:此处
    // 不持任何 items 锁,layout 写锁在 store_layout 内短窗取放)。
    let t_store = std::time::Instant::now();
    let state_store = state.inner().clone();
    let version = tokio::task::spawn_blocking(move || {
        store_layout_with_lens(
            &state_store.layout_cache,
            rows,
            total_height,
            gen_key,
            lens_projection,
        )
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    tracing::info!(
        "store_layout: v{} in {:.0}ms | 布局换代(索引物化;旧代已后台释放)",
        version,
        t_store.elapsed().as_secs_f64() * 1000.0
    );

    // 单次读锁取摘要（此前是三次独立的 get_summary 调用）。
    Ok(get_summary(&state.layout_cache).unwrap_or(LayoutSummary {
        total_rows: 0,
        total_height,
        layout_version: version,
        total_items: 0,
        separators: vec![],
        month_buckets: vec![],
    }))
}

/// 镜头布局阻塞管线（2026-09-02 方案 §12.1，spawn_blocking 内调用）：
/// dedup 有效成员 → 全库 canonical items 快照 → 组切片组装 + 镜头打包。
///
/// 锁纪律与单槽竞态：items 读锁内组装/布局（与普通 HIT 分支同型——布局可在读锁内，
/// `store_layout_with_lens` 的 layout 写锁在锁释放后才取）；ensure 后的「命中判据 +
/// 组装」必须在**同一次读锁**内完成，否则并发视图的 `store_items` 可在窗口内覆盖单槽，
/// 组装会基于错误快照。故采用两轮结构：读锁 MISS → 全库重查填充 → 重读（刚写入几乎
/// 必然命中）；两轮后仍不命中（极端并发持续覆盖）→ `LayoutNotReady`，前端重算自愈。
fn compute_lens_blocking(
    state: &Arc<AppState>,
    params: &LayoutParams,
    layout_mode: Option<&str>,
    dv: u64,
) -> Result<(Vec<LayoutRow>, f64, Arc<LensProjection>)> {
    let t0 = std::time::Instant::now();
    // ① dedup 有效成员（读池连接仅在此持有；成员集 = 重复位置数，通常远小于全库）。
    let members = {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        list_duplicate_lens_members(&pool)?
    };
    let t_members = t0.elapsed();
    let empty_key = serde_json::to_string(&MediaFilter::default())
        .map_err(|e| AppError::internal("数据序列化失败 | serialization failed", e))?;

    // ② 命中判据 + 组装 + 布局，同一次读锁内完成（见函数注释）。
    let mut filled = false;
    loop {
        {
            let guard = state
                .layout_items_cache
                .read()
                .unwrap_or_else(|e| e.into_inner());
            let hit = guard.as_ref().is_some_and(|d| {
                d.reusable
                    && matches!(d.order, CachedOrder::Canonical)
                    && d.data_version == dv
                    && d.filter_key == empty_key
            });
            if hit {
                let data = guard.as_ref().expect("hit 判定后必有快照");
                let slices = assemble_lens_groups(&members, &data.items, &data.id_to_idx);
                let projection = Arc::new(build_lens_projection(&slices));
                let aspect = *data
                    .median_aspect
                    .get_or_init(|| median_measured_aspect(&data.items));
                let (rows, total_height) = if layout_mode == Some("grid") {
                    compute_lens_layout_grid(&slices, params)
                } else {
                    compute_lens_layout_justified(&slices, params, Some(aspect))
                };
                tracing::info!(
                    "compute_layout LENS: {} groups, {} members; members {:.0}ms, total {:.0}ms",
                    slices.len(),
                    projection.len(),
                    t_members.as_secs_f64() * 1000.0,
                    t0.elapsed().as_secs_f64() * 1000.0
                );
                return Ok((rows, total_height, projection));
            }
        }
        if filled {
            // 填充后仍被并发覆盖（持续 thrash）→ 让前端重算自愈，不空转。
            return Err(AppError::LayoutNotReady);
        }
        ensure_canonical_items(state, dv, &empty_key)?;
        filled = true;
    }
}

/// folders 镜头阻塞管线（2026-09-02 方案 §7/§12.1，P3）：三桶行 → 关联簇组装 →
/// 文件夹头 + 目录内打包。锁纪律/单槽竞态处理与 [`compute_lens_blocking`] 完全同型
///（两轮：读锁命中 → 全库重查填充 → 重读）。
fn compute_lens_folder_blocking(
    state: &Arc<AppState>,
    params: &LayoutParams,
    layout_mode: Option<&str>,
    dv: u64,
    show_unique_items: bool,
) -> Result<(Vec<LayoutRow>, f64, Arc<LensProjection>)> {
    let t0 = std::time::Instant::now();
    // ① 三桶行（读池连接仅在此持有；行域 = 纳入文件夹内的全部可见项）。
    let folder_rows = {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        list_duplicate_folder_lens_rows(&pool)?
    };
    let t_members = t0.elapsed();
    let empty_key = serde_json::to_string(&MediaFilter::default())
        .map_err(|e| AppError::internal("数据序列化失败 | serialization failed", e))?;

    let mut filled = false;
    loop {
        {
            let guard = state
                .layout_items_cache
                .read()
                .unwrap_or_else(|e| e.into_inner());
            let hit = guard.as_ref().is_some_and(|d| {
                d.reusable
                    && matches!(d.order, CachedOrder::Canonical)
                    && d.data_version == dv
                    && d.filter_key == empty_key
            });
            if hit {
                let data = guard.as_ref().expect("hit 判定后必有快照");
                // 文件夹头显示路径：DirLabel.display（与 folder 轴组头同一来源）。
                let dir_display: std::collections::HashMap<i64, String> = data
                    .dir_labels
                    .iter()
                    .map(|(id, dl)| (*id, dl.display.clone()))
                    .collect();
                let assembly: LensFolderAssembly = assemble_lens_folder_clusters(
                    &folder_rows,
                    &data.items,
                    &data.id_to_idx,
                    &dir_display,
                );
                let aspect = *data
                    .median_aspect
                    .get_or_init(|| median_measured_aspect(&data.items));
                let (rows, total_height, projection) = compute_lens_folder_layout(
                    &assembly,
                    show_unique_items,
                    params,
                    Some(aspect),
                    layout_mode == Some("grid"),
                );
                let folder_count: usize = assembly.clusters.iter().map(|c| c.folders.len()).sum();
                tracing::info!(
                    "compute_layout LENS FOLDERS: {} clusters, {} folders, {} rows projected; rows {:.0}ms, total {:.0}ms",
                    assembly.clusters.len(),
                    folder_count,
                    projection.len(),
                    t_members.as_secs_f64() * 1000.0,
                    t0.elapsed().as_secs_f64() * 1000.0
                );
                return Ok((rows, total_height, Arc::new(projection)));
            }
        }
        if filled {
            return Err(AppError::LayoutNotReady);
        }
        ensure_canonical_items(state, dv, &empty_key)?;
        filled = true;
    }
}

/// 确保 items 单槽持有**全库 canonical** 快照（方案 §12.1：镜头成员 ⊆ 全库，出口
/// hydrate 亦从该快照取载荷；与普通全库视图共用同一单槽）。MISS 时重查填充。
/// 只在 `compute_lens_blocking` 的重试循环里调用；调用方随后在同一次读锁内复检。
fn ensure_canonical_items(state: &Arc<AppState>, dv: u64, empty_key: &str) -> Result<()> {
    // 双重检查：进入查询前再读一次（尽量免 SQL）。
    {
        let guard = state
            .layout_items_cache
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_some_and(|d| {
            d.reusable
                && matches!(d.order, CachedOrder::Canonical)
                && d.data_version == dv
                && d.filter_key == empty_key
        }) {
            return Ok(());
        }
    }
    let (items, dir_labels) = {
        let pool = state.db_read_pool.get().map_err(AppError::from)?;
        let t = std::time::Instant::now();
        let items = query_layout_items_canonical(&pool, &MediaFilter::default())?;
        let dir_labels = query_dir_labels(&pool)?;
        tracing::info!(
            "lens canonical items MISS: {} items in {:.0}ms",
            items.len(),
            t.elapsed().as_secs_f64() * 1000.0
        );
        (items, dir_labels)
    };
    let data = ItemsCacheData {
        filter_key: empty_key.to_string(),
        order: CachedOrder::Canonical,
        data_version: dv,
        id_to_idx: items_cache::build_id_index(&items),
        dir_rank: items_cache::build_dir_rank(&dir_labels),
        items,
        dir_labels,
        filter: MediaFilter::default(),
        reusable: true,
        median_aspect: std::sync::OnceLock::new(),
        perm_memo: std::sync::Mutex::new(None),
        filename_rank: std::sync::OnceLock::new(),
    };
    items_cache::store_items(&state.layout_items_cache, data);
    Ok(())
}

/// 布局段（纯 CPU）：模式分支 + 总高。入参为引用序 —— S1 命中路径直接引用缓存内 items，
/// 零拷贝；布局模式分支见 T20（grid 均匀宫格 / 其余等高行，产出同一 LayoutRow 枚举，
/// 缓存/取行/月桶/虚拟滚动通路完全复用）。
fn run_layout(
    ordered: &[&LayoutItem],
    params: &LayoutParams,
    layout_mode: Option<&str>,
    dir_labels: &std::collections::HashMap<i64, crate::db::models::DirLabel>,
    placeholder_aspect: Option<f64>,
) -> (Vec<LayoutRow>, f64) {
    let rows = if layout_mode == Some("grid") {
        compute_grid_layout(ordered, params, dir_labels)
    } else {
        compute_justified_layout(ordered, params, dir_labels, placeholder_aspect)
    };
    let total_height = rows.last().map(|r| r.y() + r.height()).unwrap_or(0.0);
    (rows, total_height)
}

/// 返回当前视图**按布局序的全集 id**（T14.5 / T18 选择契约的前端前置）。
///
/// 解锁 Part5 T4「选区脱离 DOM」：Shift-range 跨视口、框选命中判定基于 flat_ids 序号而非可视 DOM；
/// Ctrl+A 全选亦据此（前端只持「全选标记 + 排除集」，批量写再走 `SelectionDescriptor::SelectAll`）。
///
/// 直接返回缓存内已物化的 `flat_ids`（O(1)，无 DB 往返）。`layout_version` 与当前布局不一致 →
/// `ViewStale`（前端重算 layout 重取）；压根无布局 → `LayoutNotReady`。
#[tauri::command]
pub async fn get_view_ids(
    layout_version: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<i64>> {
    match cache_view_ids(&state.layout_cache, layout_version) {
        Some(ids) => Ok(ids),
        // None 二义：无布局 vs 版本不符。无版本约束再取一次以区分，给前端可分流的错误码。
        None => {
            if cache_view_ids(&state.layout_cache, None).is_some() {
                Err(AppError::ViewStale)
            } else {
                Err(AppError::LayoutNotReady)
            }
        }
    }
}

/// 三滚动出口共用的出口拼装(P1-5):载荷读锁内只做纯内存工作,磁盘 IO 全走阻塞线程。
/// ① 收集选档请求(items 读锁内,零 IO);② `thumbnail::serve::prepare_offloaded` 在阻塞线程内
/// probe + 逐项 stat(不占 async 执行器、不持任何锁);③ 重验布局版本 + 应用(短读锁,零 IO 查表)。
///
/// ③ 的版本重验是新增 await 的配套:② 把「取几何/镜头 → 拼装」窗口从同步微秒拉到慢盘可达
/// 数百 ms,期间布局可能换代或被清空——旧几何必须连同旧载荷一起作废(拒绝新载荷配旧几何),
/// 故 IO 后按 ① 前的版本基线复验,不符即返回既有 [`AppError::LayoutNotReady`] 由前端重算重取。
async fn hydrate_visible(
    state: &Arc<AppState>,
    rows: Vec<LayoutRow>,
    lens: Option<Arc<LensProjection>>,
    layout_version: Option<u64>,
) -> Result<Vec<HydratedRow>> {
    // 版本基线:调用方给了就用它(取行已按它校验过);未给(旧调用方)则快照当前版本,尽力校。
    let expected_layout_version =
        layout_version.or_else(|| current_layout_version(&state.layout_cache));
    let dpr = f64::from(
        state
            .thumb_serve_dpr
            .load(std::sync::atomic::Ordering::Relaxed),
    ) / 1000.0;
    let requests = items_cache::collect_serve_requests(&state.layout_items_cache, &rows, dpr);
    let serve = if requests.is_empty() {
        None
    } else {
        let cache_dir = {
            let cfg = state.thumb_config.read().unwrap_or_else(|e| e.into_inner());
            cfg.cache_dir.clone()
        };
        Some(
            crate::thumbnail::serve::prepare_offloaded(cache_dir, dpr, requests)
                .await
                .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?,
        )
    };
    apply_visible_rows(
        &state.layout_cache,
        &state.layout_items_cache,
        rows,
        serve.as_ref(),
        lens.as_deref(),
        expected_layout_version,
    )
}

/// 三段式③(应用,生产唯一拼装出口):IO 结束后先复验布局版本(P1-5),再在载荷读锁内零 IO
/// 拼装线上行。只有真发生了 IO(窗口被拉长)才复验;零请求批的窗口仍是同步微秒级,保持既有行为。
fn apply_visible_rows(
    layout_cache: &LayoutCache,
    items_cache: &items_cache::ItemsCache,
    rows: Vec<LayoutRow>,
    serve: Option<&crate::thumbnail::serve::ThumbServe>,
    lens: Option<&LensProjection>,
    expected_layout_version: Option<u64>,
) -> Result<Vec<HydratedRow>> {
    if serve.is_some() {
        ensure_layout_unchanged(layout_cache, expected_layout_version)?;
    }
    Ok(items_cache::hydrate_rows(items_cache, rows, serve, lens))
}

/// 当前布局版本(None = 无布局)。取行后的版本基线与 IO 结束后的复验共用。
fn current_layout_version(layout_cache: &LayoutCache) -> Option<u64> {
    layout_cache
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|d| d.layout_version)
}

/// IO 结束后的布局版本复验(P1-5):取行时的布局已换代/被清空 → 返回既有 `LayoutNotReady`,
/// 前端重算重取。拒绝「旧几何 + 新载荷」——布局行只存几何,载荷来自 items 快照,两者代次错配
/// 会拼出与当前布局不符的线上行。
fn ensure_layout_unchanged(layout_cache: &LayoutCache, expected: Option<u64>) -> Result<()> {
    if current_layout_version(layout_cache) != expected {
        return Err(AppError::LayoutNotReady);
    }
    Ok(())
}

/// 从内存缓存中获取布局行的切片。
#[tauri::command]
pub async fn get_layout_rows(
    start_row: usize,
    end_row: usize,
    layout_version: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HydratedRow>> {
    // Scrolling = active interaction → throttle background decode (布局被视频派生阻塞).
    // 滚动 = 主动交互 → 节流后台解码。
    state.note_interaction();
    // S3 出口拼装：瘦行几何 + items 取数缓存载荷 → 线上行（两把锁先后独立取放，不重叠）。
    // 镜头投影先短读锁取 Arc（普通画廊 = None），与 items 读锁不重叠。
    let rows = get_rows(&state.layout_cache, start_row, end_row, layout_version)
        .ok_or(AppError::LayoutNotReady)?;
    let lens = crate::layout::cache::get_lens_projection(&state.layout_cache);
    hydrate_visible(state.inner(), rows, lens, layout_version).await
}

/// 从内存缓存中获取与 [top_y, bottom_y] 相交的布局行的切片。
#[tauri::command]
pub async fn get_layout_rows_by_y(
    top_y: f64,
    bottom_y: f64,
    layout_version: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HydratedRow>> {
    // Scrolling = active interaction → throttle background decode (布局被视频派生阻塞).
    // 滚动 = 主动交互 → 节流后台解码。
    state.note_interaction();
    let rows =
        crate::layout::cache::get_rows_by_y(&state.layout_cache, top_y, bottom_y, layout_version)
            .ok_or(AppError::LayoutNotReady)?;
    let lens = crate::layout::cache::get_lens_projection(&state.layout_cache);
    hydrate_visible(state.inner(), rows, lens, layout_version).await
}

/// 取单个 bucket 段的行:y 落在 [start_y, end_y) 的行——精确归属(半开区间),区别于
/// `get_layout_rows_by_y` 的视口相交语义。边界来自 summary 的 month_buckets 相邻 y
/// (末桶用 total_height)。T16 方案 B(bucket 分段虚拟滚动)B0。
#[tauri::command]
pub async fn get_bucket_rows(
    start_y: f64,
    end_y: f64,
    layout_version: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HydratedRow>> {
    // 滚动 = 主动交互 → 节流后台解码。
    state.note_interaction();
    let rows =
        crate::layout::cache::get_bucket_rows(&state.layout_cache, start_y, end_y, layout_version)
            .ok_or(AppError::LayoutNotReady)?;
    let lens = crate::layout::cache::get_lens_projection(&state.layout_cache);
    hydrate_visible(state.inner(), rows, lens, layout_version).await
}

/// 通过分组 id（唯一目录 id）查找分隔符行的 Y 坐标。
#[tauri::command]
pub async fn get_separator_y_by_group_id(
    group_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<f64>> {
    Ok(crate::layout::cache::get_separator_y_by_group_id(
        &state.layout_cache,
        &group_id,
    ))
}

/// 查找包含给定项 id 的行的 Y 坐标（用于行高重排后把视口重新锚定到之前浏览的项 — 问题1）。
#[tauri::command]
pub async fn get_item_y_by_id(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<f64>> {
    Ok(crate::layout::cache::get_item_y_by_id(
        &state.layout_cache,
        item_id,
    ))
}

/// 点击文件夹（按文件夹分组）时的滚动目标：若该文件夹有直接媒体则用它自己的分隔符，否则用
/// 布局顺序中其首个「有媒体」的后代子文件夹——这样点击「空」父文件夹会跳到首个含媒体的子项，
/// 而非毫无反应。返回命中的目录 id + y；若整棵子树在当前视图无媒体则返回 null。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtreeScrollTarget {
    pub dir_id: i64,
    pub y: f64,
}

#[tauri::command]
pub async fn get_subtree_scroll_target(
    dir_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<SubtreeScrollTarget>> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Result<Option<SubtreeScrollTarget>> {
        let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
        let ids = crate::db::queries::get_directory_descendant_ids(&pool, dir_id)?;
        let set: std::collections::HashSet<String> =
            ids.into_iter().map(|i| i.to_string()).collect();
        Ok(
            crate::layout::cache::get_first_separator_y_in_set(&state_arc.layout_cache, &set).map(
                |(gid, y)| SubtreeScrollTarget {
                    dir_id: gid.parse().unwrap_or(dir_id),
                    y,
                },
            ),
        )
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表征锁（2026-09-02 方案 §12.3）：普通画廊布局键格式逐字符不变——镜头改造
    /// 不得污染普通键，否则既有全量用户缓存一次性全失效。
    #[test]
    fn gen_key_gallery_is_unchanged() {
        let key = build_gallery_gen_key(
            "k",
            "date",
            "datetime",
            "desc",
            1200.0,
            100.0,
            4.0,
            Some("justified"),
            false,
            false,
            7,
        );
        assert_eq!(
            key,
            "k|date|datetime|desc|w1200.00|h100.00|g4.00|mjustified|metafalse|slfalse|dv7"
        );
        // grid 模式与 None layout_mode 的缺省写法。
        assert_eq!(
            build_gallery_gen_key(
                "k", "none", "filename", "asc", 800.0, 60.5, 0.0, None, true, true, 1
            ),
            "k|none|filename|asc|w800.00|h60.50|g0.00|mjustified|metatrue|sltrue|dv1"
        );
    }

    /// 镜头键格式锁：lens 前缀 + mode/独有开关/契约版本/镜头代次/数据代/hash 版本/几何。
    #[test]
    fn gen_key_lens_locks_shape() {
        let key = build_lens_gen_key(
            "groups",
            false,
            1,
            3,
            7,
            Some("justified"),
            1200.0,
            100.0,
            4.0,
            false,
        );
        assert_eq!(
            key,
            format!(
                "lens:groups|ufalse|ov1|ep3|dv7|hv{}|mjustified|w1200.00|h100.00|g4.00|metafalse",
                DEDUP_HASH_VERSION
            )
        );
        // folders + 独有项维度进键。
        let folders = build_lens_gen_key(
            "folders",
            true,
            1,
            3,
            7,
            Some("grid"),
            1200.0,
            100.0,
            4.0,
            true,
        );
        assert!(folders.starts_with("lens:folders|utrue|"));
        assert!(folders.ends_with("|mgrid|w1200.00|h100.00|g4.00|metatrue"));
    }

    /// **P1-5 边界回归**:受控 IO(门闸)**在途**时换布局 / 清布局——IO 结束后的版本复验必须拒绝,
    /// 否则「新载荷 + 旧几何」会拼成与当前布局不符的线上行(新增的 await 把该窗口从同步微秒
    /// 拉到慢盘数百 ms,旧实现无窗口故无需此守卫)。
    ///
    /// 三段与 [`hydrate_visible`] 逐段同源:① `collect_serve_requests` → ② [`prepare_offloaded_with`]
    /// (同一个 spawn_blocking,只把 IO 换成受控实现)→ ③ [`apply_visible_rows`](生产唯一拼装出口)。
    /// 复验若从 ③ 里被摘掉,本测试即失败——锁的是接线而不只是判据。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn layout_swap_during_offloaded_io_is_rejected() {
        use crate::db::models::LayoutItem;
        use crate::layout::cache::{new_layout_cache, store_layout_with_lens};
        use crate::layout::geometry::SlimRowItem;
        use crate::layout::items_cache::{
            build_dir_rank, build_id_index, collect_serve_requests, new_items_cache, store_items,
            CachedOrder, ItemsCacheData,
        };
        use crate::thumbnail::cache::thumb_db_path;
        use crate::thumbnail::serve::test_io::{CountingIo, Gate};
        use crate::thumbnail::serve::{prepare_offloaded_with, ThumbProbeCache, THUMB_PROBE_TTL};

        // 可视区 1 行 1 项;载荷项 status=1 且 DB 路径记 512 档(可被重写到 64 档)。
        let key = 0x0042_4242i64;
        let slot = SlimRowItem {
            id: 1,
            x: 0.0,
            w: 60.0,
            h: 45.0,
        };
        let rows = vec![LayoutRow::Normal {
            y: 0.0,
            height: 45.0,
            items: vec![slot],
        }];
        let items_cache = new_items_cache();
        let items = vec![LayoutItem {
            id: 1,
            width: 4000,
            height: 3000,
            file_size: 1024,
            sort_datetime: 1_700_000_000,
            file_format: "jpg".into(),
            media_type: "image".into(),
            is_live_photo: false,
            duration_ms: None,
            thumb_status: 1,
            thumb_path: Some(thumb_db_path(512, key)),
            thumbhash: None,
            is_favorited: false,
            rating: 0,
            color_label: 0,
            availability: "online".into(),
            dir_id: Some(1),
            similarity: None,
            cache_key: key,
        }];
        let id_to_idx = build_id_index(&items);
        let dir_labels = std::collections::HashMap::new();
        let dir_rank = build_dir_rank(&dir_labels);
        store_items(
            &items_cache,
            ItemsCacheData {
                filter_key: "{}".into(),
                order: CachedOrder::Canonical,
                data_version: 1,
                items,
                id_to_idx,
                dir_labels,
                dir_rank,
                filter: MediaFilter::default(),
                reusable: true,
                median_aspect: std::sync::OnceLock::new(),
                perm_memo: std::sync::Mutex::new(None),
                filename_rank: std::sync::OnceLock::new(),
            },
        );
        let requests = collect_serve_requests(&items_cache, &rows, 1.0);
        assert_eq!(requests.len(), 1, "夹具应有 1 个选档请求");

        // 取行版本基线:布局代次 v1。
        let layout_cache = new_layout_cache();
        let v1 = store_layout_with_lens(&layout_cache, rows.clone(), 45.0, String::new(), None);
        assert_eq!(current_layout_version(&layout_cache), Some(v1));

        // ② 受控 IO:进入后停在门闸内,测试据此把「换布局」精确放到 IO 在途。
        let gate = Arc::new(Gate::default());
        let io = Arc::new(CountingIo::default());
        io.set_gate(gate.clone());
        io.set_results(true, true); // 档目录非空 + 目标档存在 → 正常可重写
        let probes = Arc::new(ThumbProbeCache::new(THUMB_PROBE_TTL));
        let fetch = tokio::spawn(prepare_offloaded_with(
            io.clone(),
            probes,
            std::path::PathBuf::from("C:/fake-cache"),
            1.0,
            requests,
        ));
        assert!(
            gate.wait_entered_timeout(std::time::Duration::from_secs(10)),
            "IO 未进入(夹具失效)"
        );

        // IO 在途:布局换代(v2)。旧几何此刻已作废。
        let v2 =
            store_layout_with_lens(&layout_cache, rows.clone(), 45.0, "gen-key-2".into(), None);
        assert_ne!(v1, v2, "换代必须递增版本");
        gate.release();
        let serve = fetch
            .await
            .expect("runtime join")
            .expect("spawn_blocking join");
        assert_eq!(io.under_lock(), 0, "解析阶段不得在载荷锁内做 IO");

        // ③ 复验:IO 期间换代 → 拒绝(旧几何不得配新载荷)。
        assert!(
            matches!(
                apply_visible_rows(
                    &layout_cache,
                    &items_cache,
                    rows.clone(),
                    Some(&serve),
                    None,
                    Some(v1)
                ),
                Err(AppError::LayoutNotReady)
            ),
            "IO 期间换代后必须返回 LayoutNotReady"
        );
        // 对照组:版本一致(当前 v2)→ 放行,且选档照常生效。
        let wire = apply_visible_rows(
            &layout_cache,
            &items_cache,
            rows.clone(),
            Some(&serve),
            None,
            Some(v2),
        )
        .expect("版本一致应放行");
        match &wire[0] {
            HydratedRow::Normal { items, .. } => assert_eq!(
                items[0].thumb_path.as_deref(),
                Some(thumb_db_path(64, key).as_str()),
                "复验放行后应用阶段应重写到 64 档"
            ),
            _ => panic!("expected normal row"),
        }

        // 清空布局(清库/失效路径)→ 同样拒绝。
        *layout_cache.write().unwrap() = None;
        assert!(
            matches!(
                apply_visible_rows(
                    &layout_cache,
                    &items_cache,
                    rows.clone(),
                    Some(&serve),
                    None,
                    Some(v2)
                ),
                Err(AppError::LayoutNotReady)
            ),
            "布局被清空后必须返回 LayoutNotReady"
        );
        // 未传版本(旧调用方)的基线:hydrate_visible 快照当前版本后再复验,此处直接验证同一基线的
        // 自洽性——相同基线放行。
        let _ = store_layout_with_lens(&layout_cache, rows.clone(), 45.0, "gen-key-3".into(), None);
        let baseline = current_layout_version(&layout_cache);
        assert!(baseline.is_some());
        apply_visible_rows(
            &layout_cache,
            &items_cache,
            rows,
            Some(&serve),
            None,
            baseline,
        )
        .expect("同基线应放行");
    }
}
