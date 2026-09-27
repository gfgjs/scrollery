//! 主缩略图路由（v3 §6.2 / Part1 §2.2 / 勘误 R3）。
//!
//! 纯判定函数：决定一个缩略图请求走「现有缓存 / 主 generator / 冷门让路」哪条路。
//! **不持 `AppState`、不查 DB、不调 Coordinator**——与 generator 一样是纯函数（附录 A 已核验
//! generator 不持 AppState）。数据访问层负责批量预取 `FormatResolution` 与 task 状态后传入，
//! 避免 Router 内 N+1 查询（R7）。
//!
//! 真实入口（R3）：`thumbnail_commands.rs` 只有 `batch_request_thumbnails` 与
//! `start_full_thumbnail_generation` 两个命令，各自「查缓存 → needs_gen → 生成」。Router 接在
//! **这两处的 needs_gen 过滤点**：命中未完成 `Exotic` 即不入 needs_gen——既不调
//! `decode_media_step`/`process_deferred_cpu`，也不写 `thumb_status=2`。

use crate::exotic::{Capability, ExoticTaskStatus, FormatResolution};

/// 路由判定结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ThumbnailRoute {
    /// 已有有效缩略图（thumb_status 1/3，或 exotic 已完成且指纹有效）→ 用现有，不生成。
    Existing,
    /// 常见格式 → 交主 generator。
    Common,
    /// 冷门格式且未完成 → 不调 generator；前端按 `resolution.availability` 显示
    /// 处理中 / 购买占位 / 平台不支持等；数据层合并发一次 Coordinator wake。
    Exotic(FormatResolution),
}

/// Router 输入（数据层批量预取后构造；纯函数只读不查库）。
pub struct ThumbnailRouteInput<'a> {
    pub item_id: i64,
    pub file_format: &'a str,
    pub thumb_status: i64,
    /// 该格式的能力解析（None = 非 catalog 格式 / catalog 不认领该格式）。
    pub resolution: Option<&'a FormatResolution>,
    /// 该 item 的 thumbnail exotic 任务状态（None = 尚无任务，如 backfill 前窗口）。
    pub task_status: Option<ExoticTaskStatus>,
    /// 任务 done 时其输出指纹是否仍有效。数据层用全局档位重算期望指纹与存储指纹比对后传入（问题4）；
    /// 失效（如用户改档位）→ Router 判 `Exotic`，调用方须先 invalidate 为 pending 再让路重做。
    pub fingerprint_valid: bool,
}

/// 纯判定（规则顺序对齐 Part1 §2.2）：
///   1. 已有有效缩略图（status 1/3）→ Existing；
///   2. 非 exotic（无 offering 或不认领 thumbnail）→ Common；
///   3. exotic 且 task done 且指纹有效 → Existing（优先用产物，不再尝试解原图）；
///   4. exotic 未完成 → Exotic(resolution)，不调 generator；
///   5. 无插件/未授权/平台不支持 → 仍 Exotic（availability 在 resolution 内，供前端准确占位）。
pub fn route_thumbnail(input: &ThumbnailRouteInput<'_>) -> ThumbnailRoute {
    // 1. 已有有效缩略图（1=已生成 / 3=小文件直显）。
    if input.thumb_status == 1 || input.thumb_status == 3 {
        return ThumbnailRoute::Existing;
    }

    // 2. 非 exotic（或 catalog 不认领 thumbnail）→ 常见格式走主 generator。
    let res = match input.resolution {
        Some(r) if r.capabilities.contains(&Capability::Thumbnail) => r,
        _ => return ThumbnailRoute::Common,
    };

    // 3. exotic 已完成且指纹有效 → 用产物（理论上 thumb_status 应已为 1，此为一致性兜底）。
    if input.task_status == Some(ExoticTaskStatus::Done) && input.fingerprint_valid {
        return ThumbnailRoute::Existing;
    }

    // 4./5. exotic 未完成（含未安装/未授权/平台不支持）→ 让路，不调 generator。
    ThumbnailRoute::Exotic(res.clone())
}
