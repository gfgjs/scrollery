//! 精确内容去重的 Tauri IPC：任务生命周期 + 动态重复组 keyset 查询。
//!
//! 这里只做输入校验、DTO 映射和读池调度；文件摘要在后台任务中完成，组/成员查询不把
//! 全库 ID 集合搬到前端。所有发送给前端的错误都使用稳定 code，不泄露 SQL、路径或 OS
//! 原文。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::db::models::GroupKeyWire;
use crate::db::queries::{self, DuplicateGroupCursor};
use crate::dedup::folder::{
    generate_cleanup_plan, DedupFolderGroupKey, DedupFolderPlanBaseMode, DedupFolderPlanError,
    DedupFolderPlanGroup, DedupFolderPlanMember, DedupProtectionSummary,
};
use crate::dedup::task::{DedupPhase, DedupProgress, DedupStatus};
use crate::dedup::DEDUP_HASH_VERSION;
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::thumbhash::average_color_hex;

/// 去重状态变更事件名。
///
/// 事件只携带 [`DedupStatusSnapshot`]，前端收到后应以事件快照刷新状态，不把事件当作
/// 全量组/成员数据通道。
pub const DEDUP_STATUS_CHANGED_EVENT: &str = "dedup:progress";

/// 分页大小不在稳定契约允许范围内。
pub const CODE_INVALID_LIMIT: &str = "DEDUP_INVALID_LIMIT";
/// cursor/group key 无法由后端生成的格式解析。
pub const CODE_INVALID_CURSOR: &str = "DEDUP_INVALID_CURSOR";
/// keeper/selection 校验失败；这些码会直接作为前端分流契约。
pub const CODE_KEEPER_REQUIRED: &str = "KEEPER_REQUIRED";
pub const CODE_SOURCE_CHANGED: &str = "SOURCE_CHANGED";
/// 文件夹视图版本戳与当前分析/数据版本不一致。
pub const CODE_VIEW_STALE: &str = "DEDUP_VIEW_STALE";
/// 目标目录不存在、不可见或其范围已不再适合作为单目标计划。
pub const CODE_FOLDER_SCOPE_CHANGED: &str = "FOLDER_SCOPE_CHANGED";
/// 文件夹计划中的显式 override 不符合当前范围。
pub const CODE_SELECTION_INVALID: &str = "SELECTION_INVALID";
const CODE_NO_DELETIONS: &str = "NO_DELETIONS";
const CODE_EMPTY_GROUP_KEY: &str = "GROUP_KEY_REQUIRED";
const CODE_KEEPER_CONFLICT: &str = "KEEPER_CONFLICT";
const CODE_DUPLICATE_ITEM: &str = "DUPLICATE_ITEM";
pub const CODE_CLEANUP_INTERNAL: &str = "DEDUP_CLEANUP_INTERNAL";

/// 单次 keyset 查询允许的最大页长；真正的查询层也必须执行相同上限。
pub const MAX_PAGE_LIMIT: u32 = 500;

/// 文件夹聚合筛选器。首版只开放零字节项开关；offline/missing 始终属于未知项，不能
/// 因为展示筛选而进入默认清理建议。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DuplicateFolderFilters {
    pub include_offline: bool,
    pub include_zero_byte: bool,
}

/// 候选列表请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDuplicateFolderCandidatesRequest {
    pub cursor: Option<String>,
    pub limit: u32,
    pub filters: DuplicateFolderFilters,
}

/// 候选树子节点请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDuplicateFolderChildrenRequest {
    pub parent_id: i64,
    pub cursor: Option<String>,
    pub limit: u32,
    pub filters: DuplicateFolderFilters,
}

/// 文件夹摘要请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDuplicateFolderSummaryRequest {
    pub folder_id: i64,
    pub view_stamp: String,
    pub filters: DuplicateFolderFilters,
}

/// 画廊的四种互斥视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateFolderItemFilter {
    Recommended,
    Review,
    Keep,
    All,
}

/// 文件夹画廊请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDuplicateFolderItemsRequest {
    pub folder_id: i64,
    pub view_stamp: String,
    pub filter: DuplicateFolderItemFilter,
    pub cursor: Option<String>,
    pub limit: u32,
    pub filters: DuplicateFolderFilters,
}

/// 文件夹节点在候选树中的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateFolderNodeState {
    Ready,
    Review,
    None,
}

/// 文件夹聚合统计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderStats {
    pub total_positions: u64,
    pub analyzed_positions: u64,
    pub duplicate_positions: u64,
    pub external_covered_positions: u64,
    pub internal_duplicate_positions: u64,
    pub unreviewed_positions: u64,
    pub protected_positions: u64,
    pub recommended_positions: u64,
    pub recommended_logical_bytes: u64,
}

/// 文件夹候选/树节点最小投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderNode {
    pub node_key: String,
    pub folder_id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub stats: DuplicateFolderStats,
    pub state: DuplicateFolderNodeState,
    pub view_stamp: String,
}

/// 外部覆盖来源的有界摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderCoverSource {
    pub folder_id: i64,
    pub node_key: String,
    pub name: String,
    pub rel_path: String,
    pub covered_positions: u64,
}

/// 文件夹摘要投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderSummary {
    pub folder_id: i64,
    pub node_key: String,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub stats: DuplicateFolderStats,
    pub top_cover_sources: Vec<DuplicateFolderCoverSource>,
    pub view_stamp: String,
}

/// 文件夹候选页。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderNodePage {
    pub items: Vec<DuplicateFolderNode>,
    pub next_cursor: Option<String>,
    pub view_stamp: String,
}

/// 文件夹画廊项的精确关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateFolderItemRelation {
    CoveredOutside,
    InternalDuplicate,
    Protected,
    Unmatched,
    Unknown,
}

/// 文件夹画廊项的默认动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateFolderDefaultAction {
    Clean,
    Keep,
    Review,
}

/// 画廊卡片使用的保护元数据投影。`reasons` 是稳定字段名，不把数据库列名或原始错误
/// 直接暴露给前端；展示层可按当前语言翻译这些原因码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupProtectionDto {
    pub is_protected: bool,
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub album_count: u64,
    pub tag_count: u64,
    pub bookmark_count: u64,
    pub reasons: Vec<String>,
}

/// 文件夹画廊卡片的最小投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderItem {
    pub item_id: i64,
    pub group_key: Option<String>,
    pub directory_id: i64,
    pub file_name: String,
    pub directory_path: String,
    pub file_size: u64,
    pub media_type: String,
    pub availability: String,
    pub is_live_photo: bool,
    pub width: u64,
    pub height: u64,
    pub duration_ms: Option<u64>,
    pub thumb_status: u8,
    pub thumb_path: Option<String>,
    pub placeholder_color: Option<String>,
    pub relation: DuplicateFolderItemRelation,
    pub default_action: DuplicateFolderDefaultAction,
    pub physical_alias: bool,
    pub protection: DedupProtectionDto,
}

/// 文件夹画廊页。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFolderItemPage {
    pub items: Vec<DuplicateFolderItem>,
    pub next_cursor: Option<String>,
    pub view_stamp: String,
}

/// 文件夹计划的期望摘要；不携带大 ID 集合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupFolderCleanupExpected {
    pub selected_position_count: u64,
    pub affected_group_count: u64,
    pub logical_bytes: u64,
}

/// 文件夹清理计划描述符。`recommended` 由后端重算，只需传少量例外。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupFolderCleanupSelection {
    pub view_stamp: String,
    pub target_folder_id: i64,
    pub base_mode: DedupFolderPlanBaseMode,
    pub included_item_ids: Vec<i64>,
    pub excluded_item_ids: Vec<i64>,
    pub expected: DedupFolderCleanupExpected,
}

/// 文件夹清理 preview 的有界返回值；选中 ID 留在后端，后续 apply 也按同一描述符重算。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupFolderCleanupPreview {
    pub view_stamp: String,
    pub target_folder_id: i64,
    pub base_mode: DedupFolderPlanBaseMode,
    pub selected_position_count: u64,
    pub affected_group_count: u64,
    pub logical_bytes: u64,
    pub protected_position_count: u64,
    pub unreviewed_position_count: u64,
    pub kept_position_count: u64,
    pub exception_count: u64,
    pub expected_matches: bool,
}

/// 文件夹清理 apply 的处理摘要；计数口径与 preview 相同，便于提交后刷新局部视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupFolderCleanupResult {
    pub view_stamp: String,
    pub target_folder_id: i64,
    pub base_mode: DedupFolderPlanBaseMode,
    pub selected_position_count: u64,
    pub deleted_item_count: u64,
    pub affected_group_count: u64,
    pub logical_bytes: u64,
    pub protected_position_count: u64,
    pub unreviewed_position_count: u64,
    pub kept_position_count: u64,
    pub exception_count: u64,
}

/// 重复组查询筛选器。
///
/// 字段保持少而稳定；未提供的筛选器使用后端默认安全口径（不主动包含离线和零字节
/// 噪声）。后续新增筛选器应向后兼容地追加可选字段。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DuplicateGroupFilters {
    pub include_offline: bool,
    pub include_zero_byte: bool,
    pub min_member_count: Option<u32>,
}

/// `list_duplicate_groups(cursor, limit, filters)` 的参数 DTO。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDuplicateGroupsRequest {
    /// 不透明 keyset cursor；IPC 层不解码、不重排、不转 offset。
    pub cursor: Option<String>,
    pub limit: u32,
    pub filters: DuplicateGroupFilters,
}

/// `list_duplicate_members(groupKey, cursor, limit, filters)` 的参数 DTO。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDuplicateMembersRequest {
    /// 后端生成的稳定组键；前端只能原样回传。
    pub group_key: String,
    /// 不透明 keyset cursor；不可由前端用数组长度推导。
    pub cursor: Option<String>,
    pub limit: u32,
    pub filters: DuplicateGroupFilters,
}

/// 去重分析运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DedupRunStatus {
    Idle,
    Running,
    Stopping,
    Completed,
    Failed,
    Cancelled,
}

/// 状态中按稳定码聚合的错误计数；不携带路径、SQL 或底层错误串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupErrorSummary {
    pub code: String,
    pub count: u64,
}

/// `dedup_status()` 和状态事件的最小稳定快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupStatusSnapshot {
    pub run_id: Option<String>,
    pub status: DedupRunStatus,
    pub phase: String,
    pub items_done: u64,
    pub items_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub groups_found: u64,
    pub potential_logical_bytes: u64,
    pub errors: Vec<DedupErrorSummary>,
    pub waiting_on: Vec<String>,
}

/// 一个重复组行。
///
/// 这里只返回聚合值和后端生成的 group key；成员要通过
/// `list_duplicate_members` 分页取得，不在组列表中嵌套全量成员 id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub group_key: String,
    pub member_count: u64,
    pub unit_size: u64,
    pub potential_logical_bytes: u64,
    pub metadata_conflict: bool,
    pub suggested_keeper_id: i64,
}

/// 一个重复组成员行。
///
/// `item_id` 是当前页成员的实体身份，不是一次性下发的全库 id 集合；路径字段是成功
/// 查询数据，不会出现在错误载荷中。用户保护标记只读展示，不能替代清理前的重新确认。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateMember {
    pub item_id: i64,
    pub file_name: String,
    pub directory_path: String,
    pub file_size: u64,
    pub file_mtime: i64,
    pub source_revision: u64,
    pub availability: String,
    pub is_live_photo: bool,
    pub is_favorited: bool,
    pub rating: i64,
    pub color_label: i64,
    pub album_count: u64,
    pub tag_count: u64,
    pub bookmark_count: u64,
}

/// keyset 分页统一返回形态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysetPage<T> {
    pub items: Vec<T>,
    /// 后端 cursor 原样透传；无下一页为 null。
    pub next_cursor: Option<String>,
}

pub type DuplicateGroupPage = KeysetPage<DuplicateGroup>;
pub type DuplicateMemberPage = KeysetPage<DuplicateMember>;

/// 一组重复项的清理选择。只提交 keeper 与待处理成员，不接受前端路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupCleanupSelection {
    pub group_key: String,
    pub keeper_id: i64,
    pub selected_item_ids: Vec<i64>,
    pub expected_member_count: u64,
}

/// 清理执行结果。`logical_bytes` 只是选中逻辑单元大小；不承诺实际释放空间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupCleanupResult {
    pub deleted_item_count: u64,
    pub logical_bytes: u64,
}

fn cleanup_error(code: &'static str, message: &'static str) -> AppError {
    AppError::Dedup {
        code,
        message: message.to_string(),
    }
}

fn cleanup_db_error() -> AppError {
    cleanup_error(
        CODE_CLEANUP_INTERNAL,
        "清理数据库操作失败 | cleanup database operation failed",
    )
}

fn validate_cleanup_selection(
    conn: &rusqlite::Connection,
    selection: &DedupCleanupSelection,
) -> Result<(Vec<i64>, i64)> {
    if selection.group_key.trim().is_empty() {
        return Err(cleanup_error(
            CODE_EMPTY_GROUP_KEY,
            "重复组缺少稳定标识 | Duplicate group key is required",
        ));
    }
    if selection.selected_item_ids.is_empty() {
        return Err(cleanup_error(
            CODE_NO_DELETIONS,
            "没有可清理的副本 | No duplicate items selected for cleanup",
        ));
    }

    let (unit_digest, unit_size) = decode_group_key(&selection.group_key)?;
    if unit_size < 0 {
        return Err(invalid_cursor());
    }
    let mut selected_item_ids = selection.selected_item_ids.clone();
    selected_item_ids.sort_unstable();
    let mut unique_ids = HashSet::with_capacity(selected_item_ids.len());
    if selected_item_ids
        .iter()
        .any(|item_id| *item_id <= 0 || !unique_ids.insert(*item_id))
    {
        return Err(cleanup_error(
            CODE_DUPLICATE_ITEM,
            "清理计划包含重复媒体项 | Cleanup plan contains a duplicate item",
        ));
    }

    let members =
        queries::list_duplicate_group_members_all(conn, &unit_digest, unit_size, false, false)
            .map_err(|_| cleanup_db_error())?;
    if members.len() < 2
        || u64::try_from(members.len()).unwrap_or(u64::MAX) != selection.expected_member_count
    {
        return Err(cleanup_error(
            CODE_SOURCE_CHANGED,
            "重复组已变化，请刷新后重新复核 | Duplicate group changed; refresh and review again",
        ));
    }
    if !members
        .iter()
        .any(|member| member.item_id == selection.keeper_id)
    {
        return Err(cleanup_error(
            CODE_KEEPER_REQUIRED,
            "必须保留仍存在的 keeper | A current keeper is required",
        ));
    }
    if selected_item_ids.contains(&selection.keeper_id) {
        return Err(cleanup_error(
            CODE_KEEPER_CONFLICT,
            "keeper 不能位于删除集合中 | Keeper conflicts with deletion set",
        ));
    }
    if selected_item_ids
        .iter()
        .any(|item_id| !members.iter().any(|member| member.item_id == *item_id))
    {
        return Err(cleanup_error(
            CODE_SOURCE_CHANGED,
            "重复组成员已变化，请刷新后重新复核 | Duplicate group member changed; refresh and review again",
        ));
    }

    Ok((selected_item_ids, unit_size))
}

#[tauri::command]
pub async fn apply_dedup_soft_delete(
    selection: DedupCleanupSelection,
    state: State<'_, Arc<AppState>>,
) -> Result<DedupCleanupResult> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| -> Result<DedupCleanupResult> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (selected_ids, unit_size) = validate_cleanup_selection(&conn, &selection)?;
            let deleted_item_count = u64::try_from(selected_ids.len()).unwrap_or(0);
            let logical_bytes = u64::try_from(unit_size)
                .unwrap_or(0)
                .saturating_mul(deleted_item_count);
            queries::soft_delete_items(&conn, &selected_ids).map_err(|_| cleanup_db_error())?;
            state_arc.bump_data_version();
            Ok(DedupCleanupResult {
                deleted_item_count,
                logical_bytes,
            })
        })
    })
    .await
    .map_err(|_| cleanup_db_error())?
}

fn validate_limit(limit: u32) -> Result<()> {
    if (1..=MAX_PAGE_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(AppError::Dedup {
            code: CODE_INVALID_LIMIT,
            message: "去重分页大小无效 | invalid dedup page limit".to_string(),
        })
    }
}

fn invalid_cursor() -> AppError {
    AppError::Dedup {
        code: CODE_INVALID_CURSOR,
        message: "去重游标无效 | invalid dedup cursor".to_string(),
    }
}

pub(crate) fn progress_to_snapshot(progress: DedupProgress) -> DedupStatusSnapshot {
    let status = match progress.status {
        DedupStatus::Idle => DedupRunStatus::Idle,
        DedupStatus::Running => DedupRunStatus::Running,
        DedupStatus::Stopped => DedupRunStatus::Cancelled,
        DedupStatus::Completed => DedupRunStatus::Completed,
        DedupStatus::Failed => DedupRunStatus::Failed,
    };
    let phase = match progress.phase {
        DedupPhase::Idle => "idle",
        DedupPhase::Quick => "quick",
        DedupPhase::Exact => "exact",
        DedupPhase::Unit => "unit",
    };
    DedupStatusSnapshot {
        run_id: (progress.run_id != 0).then(|| progress.run_id.to_string()),
        status,
        phase: phase.to_string(),
        items_done: progress.items_done,
        items_total: progress.items_total,
        bytes_done: progress.bytes_done,
        bytes_total: progress.bytes_total,
        groups_found: progress.groups_found,
        potential_logical_bytes: progress.potential_logical_bytes,
        errors: progress
            .errors
            .into_iter()
            .map(|error| DedupErrorSummary {
                code: error.code,
                count: error.count,
            })
            .collect(),
        waiting_on: progress.waiting_on,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupCursorWire {
    digest: String,
    size: i64,
    first_item_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemberCursorWire {
    item_id: i64,
}

fn encode_digest(digest: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(digest)
}

fn decode_digest(encoded: &str) -> Result<Vec<u8>> {
    let digest = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid_cursor())?;
    if digest.len() != 32 {
        return Err(invalid_cursor());
    }
    Ok(digest)
}

/// 组 key 编码单一事实源（[`crate::db::models::encode_group_key`]）：与主画廊镜头布局的
/// separator groupId 同码，避免两份编码漂移。旧 Result 签名保留（decode 侧可失败）。
fn encode_group_key(digest: &[u8], size: i64) -> Result<String> {
    Ok(crate::db::models::encode_group_key(digest, size))
}

fn decode_group_key(value: &str) -> Result<(Vec<u8>, i64)> {
    let key: GroupKeyWire = serde_json::from_str(value).map_err(|_| invalid_cursor())?;
    Ok((decode_digest(&key.digest)?, key.size))
}

fn decode_group_cursor(value: Option<String>) -> Result<Option<DuplicateGroupCursor>> {
    value
        .map(|value| {
            let cursor: GroupCursorWire =
                serde_json::from_str(&value).map_err(|_| invalid_cursor())?;
            Ok(DuplicateGroupCursor {
                unit_digest: decode_digest(&cursor.digest)?,
                unit_size: cursor.size,
                first_item_id: cursor.first_item_id,
            })
        })
        .transpose()
}

fn encode_group_cursor(cursor: &DuplicateGroupCursor) -> Result<String> {
    serde_json::to_string(&GroupCursorWire {
        digest: encode_digest(&cursor.unit_digest),
        size: cursor.unit_size,
        first_item_id: cursor.first_item_id,
    })
    .map_err(|_| invalid_cursor())
}

fn decode_member_cursor(value: Option<String>) -> Result<Option<i64>> {
    value
        .map(|value| {
            let cursor: MemberCursorWire =
                serde_json::from_str(&value).map_err(|_| invalid_cursor())?;
            Ok(cursor.item_id)
        })
        .transpose()
}

fn encode_member_cursor(item_id: i64) -> Result<String> {
    serde_json::to_string(&MemberCursorWire { item_id }).map_err(|_| invalid_cursor())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderCandidateCursorWire {
    actionable: i64,
    recommended_positions: i64,
    total_positions: i64,
    external_covered_positions: i64,
    recommended_logical_bytes: i64,
    retained_positions: i64,
    depth: i64,
    folder_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderTreeCursorWire {
    name: String,
    folder_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderViewStampPayload {
    analysis_generation: u64,
    data_version: u64,
    hash_version: u32,
    filters: DuplicateFolderFilters,
}

const FOLDER_VIEW_STAMP_PREFIX: &str = "dfv1.";

fn encode_folder_candidate_cursor(
    cursor: &queries::DuplicateFolderCandidateCursor,
) -> Result<String> {
    let wire = FolderCandidateCursorWire {
        actionable: cursor.actionable,
        recommended_positions: cursor.recommended_positions,
        total_positions: cursor.total_positions,
        external_covered_positions: cursor.external_covered_positions,
        recommended_logical_bytes: cursor.recommended_logical_bytes,
        retained_positions: cursor.retained_positions,
        depth: cursor.depth,
        folder_id: cursor.folder_id,
    };
    let bytes = serde_json::to_vec(&wire).map_err(|_| invalid_cursor())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_folder_candidate_cursor(
    value: Option<String>,
) -> Result<Option<queries::DuplicateFolderCandidateCursor>> {
    value
        .map(|value| {
            let bytes = URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| invalid_cursor())?;
            let cursor: FolderCandidateCursorWire =
                serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
            if !(0..=1).contains(&cursor.actionable)
                || cursor.recommended_positions < 0
                || cursor.total_positions <= 0
                || cursor.external_covered_positions < 0
                || cursor.recommended_logical_bytes < 0
                || cursor.retained_positions < 0
                || cursor.depth < 0
                || cursor.folder_id <= 0
            {
                return Err(invalid_cursor());
            }
            Ok(queries::DuplicateFolderCandidateCursor {
                actionable: cursor.actionable,
                recommended_positions: cursor.recommended_positions,
                total_positions: cursor.total_positions,
                external_covered_positions: cursor.external_covered_positions,
                recommended_logical_bytes: cursor.recommended_logical_bytes,
                retained_positions: cursor.retained_positions,
                depth: cursor.depth,
                folder_id: cursor.folder_id,
            })
        })
        .transpose()
}

fn encode_folder_tree_cursor(cursor: &queries::DuplicateFolderTreeCursor) -> Result<String> {
    let bytes = serde_json::to_vec(&FolderTreeCursorWire {
        name: cursor.name.clone(),
        folder_id: cursor.folder_id,
    })
    .map_err(|_| invalid_cursor())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_folder_tree_cursor(
    value: Option<String>,
) -> Result<Option<queries::DuplicateFolderTreeCursor>> {
    value
        .map(|value| {
            let bytes = URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| invalid_cursor())?;
            let cursor: FolderTreeCursorWire =
                serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
            if cursor.folder_id <= 0 {
                return Err(invalid_cursor());
            }
            Ok(queries::DuplicateFolderTreeCursor {
                name: cursor.name,
                folder_id: cursor.folder_id,
            })
        })
        .transpose()
}

fn folder_db_error() -> AppError {
    cleanup_error(
        CODE_CLEANUP_INTERNAL,
        "文件夹去重查询失败 | Duplicate folder query failed",
    )
}

fn folder_view_stale_error() -> AppError {
    cleanup_error(
        CODE_VIEW_STALE,
        "去重视图已更新，请刷新后重新复核 | Dedup view is stale; refresh and review again",
    )
}

fn folder_scope_changed_error() -> AppError {
    cleanup_error(
        CODE_FOLDER_SCOPE_CHANGED,
        "目标文件夹范围已变化，请刷新后重新复核 | Folder scope changed; refresh and review again",
    )
}

fn folder_selection_invalid_error() -> AppError {
    cleanup_error(
        CODE_SELECTION_INVALID,
        "文件夹清理选择无效 | Folder cleanup selection is invalid",
    )
}

fn folder_plan_error(error: DedupFolderPlanError) -> AppError {
    match error {
        DedupFolderPlanError::KeeperConflict => cleanup_error(
            CODE_KEEPER_CONFLICT,
            "清理选择不能移除组内最后保留项 | Cleanup selection conflicts with keeper",
        ),
        DedupFolderPlanError::GroupWouldLoseKeeper => cleanup_error(
            CODE_KEEPER_REQUIRED,
            "每个重复组必须至少保留一个当前有效位置 | Each duplicate group needs a keeper",
        ),
        DedupFolderPlanError::DuplicateOverrideItem | DedupFolderPlanError::ItemNotInTarget => {
            folder_selection_invalid_error()
        }
    }
}

fn make_folder_view_stamp(
    state: &Arc<AppState>,
    conn: &rusqlite::Connection,
    filters: &DuplicateFolderFilters,
) -> Result<String> {
    let analysis_generation = queries::dedup_run_generation(conn)
        .map_err(|_| folder_db_error())?
        .unwrap_or(0);
    let payload = FolderViewStampPayload {
        analysis_generation,
        data_version: state.data_version(),
        hash_version: DEDUP_HASH_VERSION,
        filters: filters.clone(),
    };
    let bytes = serde_json::to_vec(&payload).map_err(|_| folder_db_error())?;
    Ok(format!(
        "{FOLDER_VIEW_STAMP_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

fn decode_folder_view_stamp(value: &str) -> Result<FolderViewStampPayload> {
    let encoded = value
        .strip_prefix(FOLDER_VIEW_STAMP_PREFIX)
        .ok_or_else(folder_view_stale_error)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| folder_view_stale_error())?;
    serde_json::from_slice(&bytes).map_err(|_| folder_view_stale_error())
}

fn validate_folder_view_stamp(
    state: &Arc<AppState>,
    conn: &rusqlite::Connection,
    value: &str,
) -> Result<DuplicateFolderFilters> {
    let payload = decode_folder_view_stamp(value)?;
    let current_generation = queries::dedup_run_generation(conn)
        .map_err(|_| folder_db_error())?
        .unwrap_or(0);
    if payload.analysis_generation != current_generation
        || payload.data_version != state.data_version()
        || payload.hash_version != DEDUP_HASH_VERSION
    {
        return Err(folder_view_stale_error());
    }
    Ok(payload.filters)
}

fn folder_stats_snapshot(
    state: &Arc<AppState>,
    conn: &rusqlite::Connection,
    filters: &DuplicateFolderFilters,
) -> Result<Arc<Vec<queries::DuplicateFolderStatsRow>>> {
    let analysis_generation = queries::dedup_run_generation(conn)
        .map_err(|_| folder_db_error())?
        .unwrap_or(0);
    state
        .dedup_folder_stats_cache
        .get_or_build(
            conn,
            analysis_generation,
            state.data_version(),
            DEDUP_HASH_VERSION,
            filters.include_offline,
            filters.include_zero_byte,
        )
        .map_err(|_| folder_db_error())
}

fn stats_to_dto(row: &queries::DuplicateFolderStatsRow) -> DuplicateFolderStats {
    let to_u64 = |value: i64| u64::try_from(value.max(0)).unwrap_or(0);
    DuplicateFolderStats {
        total_positions: to_u64(row.total_positions),
        analyzed_positions: to_u64(row.analyzed_positions),
        duplicate_positions: to_u64(row.duplicate_positions),
        external_covered_positions: to_u64(row.external_covered_positions),
        internal_duplicate_positions: to_u64(row.internal_duplicate_positions),
        unreviewed_positions: to_u64(row.unreviewed_positions),
        protected_positions: to_u64(row.protected_positions),
        recommended_positions: to_u64(row.recommended_positions),
        recommended_logical_bytes: to_u64(row.recommended_logical_bytes),
    }
}

fn node_state(row: &queries::DuplicateFolderStatsRow) -> DuplicateFolderNodeState {
    if row.recommended_positions > 0 {
        DuplicateFolderNodeState::Ready
    } else if row.duplicate_positions > 0 || row.unreviewed_positions > 0 {
        DuplicateFolderNodeState::Review
    } else {
        DuplicateFolderNodeState::None
    }
}

fn stats_to_node(row: &queries::DuplicateFolderStatsRow, view_stamp: &str) -> DuplicateFolderNode {
    DuplicateFolderNode {
        node_key: crate::tree::node_key(row.root_id, &row.rel_path),
        folder_id: row.folder_id,
        root_id: row.root_id,
        parent_id: row.parent_id,
        name: row.name.clone(),
        rel_path: row.rel_path.clone(),
        depth: row.depth,
        stats: stats_to_dto(row),
        state: node_state(row),
        view_stamp: view_stamp.to_string(),
    }
}

fn candidate_cursor_from_row(
    row: &queries::DuplicateFolderStatsRow,
) -> queries::DuplicateFolderCandidateCursor {
    queries::DuplicateFolderCandidateCursor {
        actionable: i64::from(row.recommended_positions > 0),
        recommended_positions: row.recommended_positions.max(0),
        total_positions: row.total_positions.max(1),
        external_covered_positions: row.external_covered_positions.max(0),
        recommended_logical_bytes: row.recommended_logical_bytes.max(0),
        retained_positions: row.retained_positions.max(0),
        depth: row.depth.max(0),
        folder_id: row.folder_id,
    }
}

fn protection_summary_from_item(row: &queries::DuplicateFolderItemRow) -> DedupProtectionSummary {
    DedupProtectionSummary {
        is_favorited: row.is_favorited,
        rating: row.rating,
        color_label: row.color_label,
        album_count: u64::try_from(row.album_count.max(0)).unwrap_or(0),
        tag_count: u64::try_from(row.tag_count.max(0)).unwrap_or(0),
        bookmark_count: u64::try_from(row.bookmark_count.max(0)).unwrap_or(0),
    }
}

fn protection_from_item(row: &queries::DuplicateFolderItemRow) -> DedupProtectionDto {
    let summary = protection_summary_from_item(row);
    let mut reasons = Vec::new();
    if summary.is_favorited {
        reasons.push("favorite".to_string());
    }
    if summary.rating > 0 {
        reasons.push("rating".to_string());
    }
    if summary.color_label > 0 {
        reasons.push("colorLabel".to_string());
    }
    if summary.album_count > 0 {
        reasons.push("album".to_string());
    }
    if summary.tag_count > 0 {
        reasons.push("tag".to_string());
    }
    if summary.bookmark_count > 0 {
        reasons.push("bookmark".to_string());
    }
    DedupProtectionDto {
        is_protected: summary.is_protected(),
        is_favorited: summary.is_favorited,
        rating: summary.rating,
        color_label: summary.color_label,
        album_count: summary.album_count,
        tag_count: summary.tag_count,
        bookmark_count: summary.bookmark_count,
        reasons,
    }
}

fn folder_item_to_dto(row: queries::DuplicateFolderItemRow) -> Result<DuplicateFolderItem> {
    let protection_summary = protection_summary_from_item(&row);
    let protection = protection_from_item(&row);
    let is_duplicate = row.group_count.is_some_and(|count| count > 1);
    let external = row
        .group_count
        .zip(row.scoped_group_count)
        .is_some_and(|(group_count, scoped_count)| group_count > scoped_count);
    let relation = if !row.analysis_ready {
        DuplicateFolderItemRelation::Unknown
    } else if !is_duplicate {
        DuplicateFolderItemRelation::Unmatched
    } else if protection_summary.is_protected() {
        DuplicateFolderItemRelation::Protected
    } else if external {
        DuplicateFolderItemRelation::CoveredOutside
    } else {
        DuplicateFolderItemRelation::InternalDuplicate
    };
    let default_action =
        if !row.analysis_ready || protection_summary.is_protected() || row.physical_alias {
            DuplicateFolderDefaultAction::Review
        } else if !is_duplicate {
            DuplicateFolderDefaultAction::Keep
        } else if external
            || row
                .suggested_keeper_id
                .is_some_and(|keeper| keeper != row.item_id)
        {
            DuplicateFolderDefaultAction::Clean
        } else {
            DuplicateFolderDefaultAction::Keep
        };
    let group_key = if is_duplicate {
        row.unit_digest
            .as_deref()
            .zip(row.unit_size)
            .map(|(digest, size)| encode_group_key(digest, size))
            .transpose()?
    } else {
        None
    };
    let thumb_status = u8::try_from(row.thumb_status.clamp(0, i64::from(u8::MAX))).unwrap_or(0);
    Ok(DuplicateFolderItem {
        item_id: row.item_id,
        group_key,
        directory_id: row.directory_id,
        file_name: row.file_name,
        directory_path: row.directory_path,
        file_size: u64::try_from(row.file_size.max(0)).unwrap_or(0),
        media_type: row.media_type,
        width: u64::try_from(row.width.max(0)).unwrap_or(0),
        height: u64::try_from(row.height.max(0)).unwrap_or(0),
        duration_ms: row
            .duration_ms
            .and_then(|duration| u64::try_from(duration.max(0)).ok()),
        thumb_status,
        thumb_path: row.thumb_path,
        placeholder_color: row.thumbhash.as_deref().and_then(average_color_hex),
        availability: row.availability,
        is_live_photo: row.is_live_photo,
        relation,
        default_action,
        physical_alias: row.physical_alias,
        protection,
    })
}

fn item_matches_filter(item: &DuplicateFolderItem, filter: DuplicateFolderItemFilter) -> bool {
    match filter {
        DuplicateFolderItemFilter::Recommended => {
            item.default_action == DuplicateFolderDefaultAction::Clean
        }
        DuplicateFolderItemFilter::Review => {
            item.default_action == DuplicateFolderDefaultAction::Review
        }
        DuplicateFolderItemFilter::Keep => {
            item.default_action == DuplicateFolderDefaultAction::Keep
        }
        DuplicateFolderItemFilter::All => true,
    }
}

fn plan_groups_to_domain(
    rows: Vec<queries::DuplicateFolderPlanGroupRow>,
) -> Vec<DedupFolderPlanGroup> {
    rows.into_iter()
        .map(|group| DedupFolderPlanGroup {
            key: DedupFolderGroupKey {
                unit_digest: group.unit_digest,
                unit_size: group.unit_size,
            },
            unit_size: group.unit_size,
            suggested_keeper_id: group.suggested_keeper_id,
            members: group
                .members
                .into_iter()
                .map(|member| DedupFolderPlanMember {
                    item_id: member.item_id,
                    in_target: member.in_target,
                    availability: member.availability,
                    rating: member.rating,
                    color_label: member.color_label,
                    protection: DedupProtectionSummary {
                        is_favorited: member.is_favorited,
                        rating: member.rating,
                        color_label: member.color_label,
                        album_count: u64::try_from(member.album_count.max(0)).unwrap_or(0),
                        tag_count: u64::try_from(member.tag_count.max(0)).unwrap_or(0),
                        bookmark_count: u64::try_from(member.bookmark_count.max(0)).unwrap_or(0),
                    },
                    physical_key: member.physical_key,
                })
                .collect(),
        })
        .collect()
}

fn path_is_ancestor(ancestor: &str, descendant: &str) -> bool {
    ancestor.is_empty()
        || ancestor == descendant
        || descendant
            .strip_prefix(ancestor)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn source_rollup_directory(
    target: &queries::DuplicateFolderStatsRow,
    source: &queries::DuplicateFolderDirectoryRow,
    by_root_path: &BTreeMap<(i64, String), queries::DuplicateFolderDirectoryRow>,
) -> Option<queries::DuplicateFolderDirectoryRow> {
    if source.root_id != target.root_id {
        return by_root_path
            .get(&(source.root_id, String::new()))
            .cloned()
            .or_else(|| Some(source.clone()));
    }
    if path_is_ancestor(&source.rel_path, &target.rel_path)
        || path_is_ancestor(&target.rel_path, &source.rel_path)
    {
        // 祖先/后代范围重叠，不把重叠目录伪装成外部来源。
        return None;
    }
    let target_parts = target.rel_path.split('/').collect::<Vec<_>>();
    let source_parts = source.rel_path.split('/').collect::<Vec<_>>();
    let common = target_parts
        .iter()
        .zip(&source_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let candidate_rel = source_parts[..common.saturating_add(1)].join("/");
    by_root_path.get(&(source.root_id, candidate_rel)).cloned()
}

fn top_cover_sources(
    target: &queries::DuplicateFolderStatsRow,
    groups: &[queries::DuplicateFolderPlanGroupRow],
    directories: &[queries::DuplicateFolderDirectoryRow],
) -> Vec<DuplicateFolderCoverSource> {
    let by_id = directories
        .iter()
        .map(|directory| (directory.id, directory))
        .collect::<BTreeMap<_, _>>();
    let by_root_path = directories
        .iter()
        .cloned()
        .map(|directory| ((directory.root_id, directory.rel_path.clone()), directory))
        .collect::<BTreeMap<_, _>>();
    let mut counts = BTreeMap::<i64, u64>::new();
    for group in groups {
        let target_count = u64::try_from(
            group
                .members
                .iter()
                .filter(|member| member.in_target)
                .count(),
        )
        .unwrap_or(0);
        if target_count == 0 {
            continue;
        }
        let mut sources = BTreeSet::new();
        for member in group.members.iter().filter(|member| !member.in_target) {
            let Some(source) = by_id.get(&member.directory_id) else {
                continue;
            };
            if let Some(rolled_up) = source_rollup_directory(target, source, &by_root_path) {
                sources.insert(rolled_up.id);
            }
        }
        for source_id in sources {
            let count = counts.entry(source_id).or_default();
            *count = count.saturating_add(target_count);
        }
    }
    let mut sources = counts
        .into_iter()
        .filter_map(|(folder_id, covered_positions)| {
            let directory = by_id.get(&folder_id)?;
            Some(DuplicateFolderCoverSource {
                folder_id,
                node_key: crate::tree::node_key(directory.root_id, &directory.rel_path),
                name: directory.name.clone(),
                rel_path: directory.rel_path.clone(),
                covered_positions,
            })
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        right
            .covered_positions
            .cmp(&left.covered_positions)
            .then_with(|| left.rel_path.cmp(&right.rel_path))
            .then_with(|| left.folder_id.cmp(&right.folder_id))
    });
    sources.truncate(5);
    sources
}

/// 按文件夹优先级列出候选目录。结果只包含命中节点及其必要祖先，不下发全库媒体 ID。
#[tauri::command]
pub async fn list_duplicate_folder_candidates(
    request: ListDuplicateFolderCandidatesRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateFolderNodePage> {
    validate_limit(request.limit)?;
    let cursor = decode_folder_candidate_cursor(request.cursor)?;
    let page_limit = usize::try_from(request.limit).unwrap_or(MAX_PAGE_LIMIT as usize);
    let filters = request.filters;
    let state_arc = state.inner().clone();
    let (rows, view_stamp) = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(
            || -> Result<(Vec<queries::DuplicateFolderStatsRow>, String)> {
                let pool = state_arc
                    .db_read_pool
                    .get()
                    .map_err(|_| folder_db_error())?;
                let snapshot = folder_stats_snapshot(&state_arc, &pool, &filters)?;
                let rows = crate::dedup::folder_cache::list_candidate_rows(
                    &snapshot,
                    cursor.as_ref(),
                    page_limit.saturating_add(1),
                );
                let view_stamp = make_folder_view_stamp(&state_arc, &pool, &filters)?;
                Ok((rows, view_stamp))
            },
        )
    })
    .await
    .map_err(|_| folder_db_error())??;
    let has_next = rows.len() > page_limit;
    let rows = rows.into_iter().take(page_limit).collect::<Vec<_>>();
    let next_cursor = if has_next {
        rows.last()
            .map(candidate_cursor_from_row)
            .map(|cursor| encode_folder_candidate_cursor(&cursor))
            .transpose()?
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|row| stats_to_node(row, &view_stamp))
        .collect();
    Ok(DuplicateFolderNodePage {
        items,
        next_cursor,
        view_stamp,
    })
}

/// 列出候选树的扫描根层。
#[tauri::command]
pub async fn list_duplicate_folder_roots(
    filters: DuplicateFolderFilters,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateFolderNodePage> {
    let state_arc = state.inner().clone();
    let (rows, view_stamp) = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(
            || -> Result<(Vec<queries::DuplicateFolderStatsRow>, String)> {
                let pool = state_arc
                    .db_read_pool
                    .get()
                    .map_err(|_| folder_db_error())?;
                let snapshot = folder_stats_snapshot(&state_arc, &pool, &filters)?;
                let rows =
                    crate::dedup::folder_cache::list_tree_rows(&snapshot, None, None, usize::MAX);
                let view_stamp = make_folder_view_stamp(&state_arc, &pool, &filters)?;
                Ok((rows, view_stamp))
            },
        )
    })
    .await
    .map_err(|_| folder_db_error())??;
    let items = rows
        .iter()
        .map(|row| stats_to_node(row, &view_stamp))
        .collect();
    Ok(DuplicateFolderNodePage {
        items,
        next_cursor: None,
        view_stamp,
    })
}

/// 列出候选树的直接子目录；树序与真实目录树一致，不按推荐优先级打散。
#[tauri::command]
pub async fn list_duplicate_folder_children(
    request: ListDuplicateFolderChildrenRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateFolderNodePage> {
    validate_limit(request.limit)?;
    let cursor = decode_folder_tree_cursor(request.cursor)?;
    let page_limit = usize::try_from(request.limit).unwrap_or(MAX_PAGE_LIMIT as usize);
    let filters = request.filters;
    let parent_id = request.parent_id;
    let state_arc = state.inner().clone();
    let (rows, view_stamp) = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(
            || -> Result<(Vec<queries::DuplicateFolderStatsRow>, String)> {
                let pool = state_arc
                    .db_read_pool
                    .get()
                    .map_err(|_| folder_db_error())?;
                let snapshot = folder_stats_snapshot(&state_arc, &pool, &filters)?;
                let rows = crate::dedup::folder_cache::list_tree_rows(
                    &snapshot,
                    Some(parent_id),
                    cursor.as_ref(),
                    page_limit.saturating_add(1),
                );
                let view_stamp = make_folder_view_stamp(&state_arc, &pool, &filters)?;
                Ok((rows, view_stamp))
            },
        )
    })
    .await
    .map_err(|_| folder_db_error())??;
    let has_next = rows.len() > page_limit;
    let rows = rows.into_iter().take(page_limit).collect::<Vec<_>>();
    let next_cursor = if has_next {
        rows.last()
            .map(|row| {
                encode_folder_tree_cursor(&queries::DuplicateFolderTreeCursor {
                    name: row.name.clone(),
                    folder_id: row.folder_id,
                })
            })
            .transpose()?
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|row| stats_to_node(row, &view_stamp))
        .collect();
    Ok(DuplicateFolderNodePage {
        items,
        next_cursor,
        view_stamp,
    })
}

/// 获取一个文件夹（含子文件夹）的聚合摘要与有界外部覆盖来源。
#[tauri::command]
pub async fn get_duplicate_folder_summary(
    request: GetDuplicateFolderSummaryRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateFolderSummary> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| {
            let pool = state_arc
                .db_read_pool
                .get()
                .map_err(|_| folder_db_error())?;
            let stamp_filters = validate_folder_view_stamp(&state_arc, &pool, &request.view_stamp)?;
            if stamp_filters != request.filters {
                return Err(folder_view_stale_error());
            }
            let snapshot = folder_stats_snapshot(&state_arc, &pool, &stamp_filters)?;
            let stats = crate::dedup::folder_cache::find_folder_stats(&snapshot, request.folder_id)
                .ok_or_else(folder_scope_changed_error)?;
            let groups = queries::list_duplicate_folder_plan_groups(
                &pool,
                request.folder_id,
                request.filters.include_zero_byte,
            )
            .map_err(|_| folder_db_error())?;
            let directories =
                queries::list_duplicate_folder_directories(&pool).map_err(|_| folder_db_error())?;
            Ok(DuplicateFolderSummary {
                folder_id: stats.folder_id,
                node_key: crate::tree::node_key(stats.root_id, &stats.rel_path),
                root_id: stats.root_id,
                parent_id: stats.parent_id,
                name: stats.name.clone(),
                rel_path: stats.rel_path.clone(),
                depth: stats.depth,
                stats: stats_to_dto(&stats),
                top_cover_sources: top_cover_sources(&stats, &groups, &directories),
                view_stamp: request.view_stamp,
            })
        })
    })
    .await
    .map_err(|_| folder_db_error())?
}

/// 获取文件夹范围画廊项。数据库按 item_id keyset 取页，状态过滤在后端投影后完成。
#[tauri::command]
pub async fn list_duplicate_folder_items(
    request: ListDuplicateFolderItemsRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateFolderItemPage> {
    validate_limit(request.limit)?;
    let after_item_id = decode_member_cursor(request.cursor)?;
    let page_limit = usize::try_from(request.limit).unwrap_or(MAX_PAGE_LIMIT as usize);
    let ListDuplicateFolderItemsRequest {
        folder_id,
        view_stamp,
        filter,
        filters,
        ..
    } = request;
    let state_arc = state.inner().clone();
    let (items, next_cursor, view_stamp) = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| {
            let pool = state_arc
                .db_read_pool
                .get()
                .map_err(|_| folder_db_error())?;
            let stamp_filters = validate_folder_view_stamp(&state_arc, &pool, &view_stamp)?;
            if stamp_filters != filters {
                return Err(folder_view_stale_error());
            }
            let snapshot = folder_stats_snapshot(&state_arc, &pool, &filters)?;
            if crate::dedup::folder_cache::find_folder_stats(&snapshot, folder_id).is_none() {
                return Err(folder_scope_changed_error());
            }
            let fetch_limit = page_limit.saturating_add(1);
            let mut after = after_item_id;
            let mut items = Vec::with_capacity(page_limit);
            let mut next_cursor = None;

            // 过滤发生在 DTO 投影之后。继续沿 item_id 游标取页，直到收满一页匹配项，
            // 避免「第一页都是保留项」时把后续建议项错误地表现成空列表。
            loop {
                let rows = queries::list_duplicate_folder_items(
                    &pool,
                    folder_id,
                    after,
                    fetch_limit,
                    filters.include_zero_byte,
                )
                .map_err(|_| folder_db_error())?;
                if rows.is_empty() {
                    break;
                }
                let has_raw_more = rows.len() > page_limit;
                let raw_rows = rows.into_iter().take(page_limit).collect::<Vec<_>>();
                let raw_len = raw_rows.len();
                let mut last_consumed = None;
                let mut page_full = false;

                for (index, row) in raw_rows.into_iter().enumerate() {
                    let item_id = row.item_id;
                    last_consumed = Some(item_id);
                    let item = folder_item_to_dto(row)?;
                    if item_matches_filter(&item, filter) {
                        items.push(item);
                    }
                    if items.len() >= page_limit {
                        let has_remaining = index + 1 < raw_len || has_raw_more;
                        next_cursor = if has_remaining {
                            Some(encode_member_cursor(item_id)?)
                        } else {
                            None
                        };
                        page_full = true;
                        break;
                    }
                }

                if page_full {
                    break;
                }
                after = last_consumed;
                if !has_raw_more {
                    break;
                }
            }
            Ok((items, next_cursor, view_stamp))
        })
    })
    .await
    .map_err(|_| folder_db_error())??;
    Ok(DuplicateFolderItemPage {
        items,
        next_cursor,
        view_stamp,
    })
}

/// 文件夹清理 preview。只返回计数和逻辑字节，不把大 ID 选择集合交给前端。
#[tauri::command]
pub async fn preview_dedup_folder_cleanup(
    selection: DedupFolderCleanupSelection,
    state: State<'_, Arc<AppState>>,
) -> Result<DedupFolderCleanupPreview> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| {
            let pool = state_arc
                .db_read_pool
                .get()
                .map_err(|_| folder_db_error())?;
            let filters = validate_folder_view_stamp(&state_arc, &pool, &selection.view_stamp)?;
            let snapshot = folder_stats_snapshot(&state_arc, &pool, &filters)?;
            let stats = crate::dedup::folder_cache::find_folder_stats(
                &snapshot,
                selection.target_folder_id,
            )
            .ok_or_else(folder_scope_changed_error)?;
            let rows = queries::list_duplicate_folder_plan_groups(
                &pool,
                selection.target_folder_id,
                filters.include_zero_byte,
            )
            .map_err(|_| folder_db_error())?;
            let groups = plan_groups_to_domain(rows);
            let plan = generate_cleanup_plan(
                &groups,
                selection.base_mode,
                &selection.included_item_ids,
                &selection.excluded_item_ids,
            )
            .map_err(folder_plan_error)?;
            let expected_matches = selection.expected.selected_position_count
                == plan.selected_position_count
                && selection.expected.affected_group_count == plan.affected_group_count
                && selection.expected.logical_bytes == plan.logical_bytes;
            let to_u64 = |value: i64| u64::try_from(value.max(0)).unwrap_or(0);
            let protected_position_count = to_u64(stats.protected_positions);
            let unreviewed_position_count = to_u64(stats.unreviewed_positions);
            Ok(DedupFolderCleanupPreview {
                view_stamp: selection.view_stamp,
                target_folder_id: selection.target_folder_id,
                base_mode: selection.base_mode,
                selected_position_count: plan.selected_position_count,
                affected_group_count: plan.affected_group_count,
                logical_bytes: plan.logical_bytes,
                protected_position_count,
                unreviewed_position_count,
                kept_position_count: to_u64(stats.total_positions)
                    .saturating_sub(plan.selected_position_count),
                exception_count: protected_position_count.saturating_add(unreviewed_position_count),
                expected_matches,
            })
        })
    })
    .await
    .map_err(|_| folder_db_error())?
}

/// 文件夹清理 apply。执行前在 writer 临界区内重算同一纯计划并校验 expected，成功后一次性软删除。
#[tauri::command]
pub async fn apply_dedup_folder_soft_delete(
    selection: DedupFolderCleanupSelection,
    state: State<'_, Arc<AppState>>,
) -> Result<DedupFolderCleanupResult> {
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| -> Result<DedupFolderCleanupResult> {
            let conn = state_arc
                .db_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let filters = validate_folder_view_stamp(&state_arc, &conn, &selection.view_stamp)?;
            let snapshot = folder_stats_snapshot(&state_arc, &conn, &filters)?;
            let stats = crate::dedup::folder_cache::find_folder_stats(
                &snapshot,
                selection.target_folder_id,
            )
            .ok_or_else(folder_scope_changed_error)?;
            let rows = queries::list_duplicate_folder_plan_groups(
                &conn,
                selection.target_folder_id,
                filters.include_zero_byte,
            )
            .map_err(|_| folder_db_error())?;
            let groups = plan_groups_to_domain(rows);
            let plan = generate_cleanup_plan(
                &groups,
                selection.base_mode,
                &selection.included_item_ids,
                &selection.excluded_item_ids,
            )
            .map_err(folder_plan_error)?;
            let expected_matches = selection.expected.selected_position_count
                == plan.selected_position_count
                && selection.expected.affected_group_count == plan.affected_group_count
                && selection.expected.logical_bytes == plan.logical_bytes;
            if !expected_matches {
                return Err(cleanup_error(
                    CODE_SOURCE_CHANGED,
                    "清理预览已变化，请刷新后重新复核 | Cleanup preview changed; refresh and review again",
                ));
            }
            queries::soft_delete_items(&conn, &plan.selected_item_ids)
                .map_err(|_| cleanup_db_error())?;
            state_arc.bump_data_version();
            Ok(DedupFolderCleanupResult {
                view_stamp: selection.view_stamp,
                target_folder_id: selection.target_folder_id,
                base_mode: selection.base_mode,
                selected_position_count: plan.selected_position_count,
                deleted_item_count: plan.selected_position_count,
                affected_group_count: plan.affected_group_count,
                logical_bytes: plan.logical_bytes,
                protected_position_count: u64::try_from(stats.protected_positions.max(0))
                    .unwrap_or(0),
                unreviewed_position_count: u64::try_from(stats.unreviewed_positions.max(0))
                    .unwrap_or(0),
                kept_position_count: u64::try_from(stats.total_positions.max(0))
                    .unwrap_or(0)
                    .saturating_sub(plan.selected_position_count),
                exception_count: u64::try_from(stats.protected_positions.max(0))
                    .unwrap_or(0)
                    .saturating_add(
                        u64::try_from(stats.unreviewed_positions.max(0)).unwrap_or(0),
                    ),
            })
        })
    })
    .await
    .map_err(|_| folder_db_error())?
}

/// 兼容设计文档中的命令名；旧 `list_duplicate_members` 保持原 IPC 不变。
#[tauri::command]
pub async fn list_duplicate_group_members(
    group_key: String,
    view_stamp: String,
    cursor: Option<String>,
    limit: u32,
    filters: DuplicateFolderFilters,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateMemberPage> {
    let validation_filters = filters.clone();
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| -> Result<()> {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            let stamp_filters = validate_folder_view_stamp(&state_arc, &pool, &view_stamp)?;
            if stamp_filters != validation_filters {
                return Err(folder_view_stale_error());
            }
            Ok(())
        })
    })
    .await
    .map_err(|_| folder_db_error())??;
    list_duplicate_members(
        group_key,
        cursor,
        limit,
        DuplicateGroupFilters {
            include_offline: false,
            include_zero_byte: filters.include_zero_byte,
            min_member_count: None,
        },
        state,
    )
    .await
}

/// 启动（或续跑）精确去重分析。当前默认口径是全部可见、在线资料库；根范围若
/// 非空会明确拒绝，避免 API 看似接受但实际上静默扩大分析范围。
#[tauri::command]
pub async fn start_dedup_analysis(
    reset: bool,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DedupStatusSnapshot> {
    let snapshot = state.inner().with_dedup_lifecycle_read(|| {
        state
            .dedup_task
            .start_with_state_and_app(state.inner().clone(), app, reset)
    })?;
    Ok(progress_to_snapshot(snapshot))
}

/// 停止当前去重分析；停止不删除已经写入的有效 sidecar 结果。
///
/// 接线依赖：调用 `crate::dedup::task::stop`，由任务层 compare-and-clear 当前运行代次。
#[tauri::command]
pub async fn stop_dedup_analysis(state: State<'_, Arc<AppState>>) -> Result<DedupStatusSnapshot> {
    let snapshot = state
        .inner()
        .with_dedup_lifecycle_read(|| state.dedup_task.stop())?;
    Ok(progress_to_snapshot(snapshot))
}

/// 返回可恢复的去重分析状态快照。
///
/// 接线依赖：调用 `crate::dedup::task::status`；不要在 IPC 层从内存数组重建全库进度。
#[tauri::command]
pub fn dedup_status(state: State<'_, Arc<AppState>>) -> Result<DedupStatusSnapshot> {
    Ok(progress_to_snapshot(state.dedup_task.status()))
}

/// 按 group key 与不透明 cursor 分页列出重复组。
///
/// 接线依赖：调用 `crate::db::queries::dedup::list_duplicate_groups`。查询层负责绑定
/// cursor/limit/filter 参数并使用稳定 keyset 顺序；本层不拼 SQL。
#[tauri::command]
pub async fn list_duplicate_groups(
    cursor: Option<String>,
    limit: u32,
    filters: DuplicateGroupFilters,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateGroupPage> {
    validate_limit(limit)?;
    let decoded_cursor = decode_group_cursor(cursor)?;
    let page_limit = usize::try_from(limit).unwrap_or(MAX_PAGE_LIMIT as usize);
    let fetch_limit = page_limit.saturating_add(1);
    let state_arc = state.inner().clone();
    let rows = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            queries::list_duplicate_groups(
                &pool,
                decoded_cursor.as_ref(),
                fetch_limit,
                filters.include_offline,
                filters.include_zero_byte,
                filters.min_member_count,
            )
        })
    })
    .await
    .map_err(|_| cleanup_db_error())??;
    let has_next = rows.len() > page_limit;
    let rows = rows.into_iter().take(page_limit).collect::<Vec<_>>();
    let next_cursor = if has_next {
        rows.last()
            .map(|row| {
                encode_group_cursor(&DuplicateGroupCursor {
                    unit_digest: row.unit_digest.clone(),
                    unit_size: row.unit_size,
                    first_item_id: row.first_item_id,
                })
            })
            .transpose()?
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(DuplicateGroup {
                group_key: encode_group_key(&row.unit_digest, row.unit_size)?,
                member_count: u64::try_from(row.member_count).unwrap_or(0),
                unit_size: u64::try_from(row.unit_size).unwrap_or(0),
                potential_logical_bytes: u64::try_from(row.potential_logical_bytes).unwrap_or(0),
                metadata_conflict: row.metadata_conflict,
                suggested_keeper_id: row.suggested_keeper_id,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(DuplicateGroupPage { items, next_cursor })
}

/// 按 group key 与不透明 cursor 分页列出成员。
///
/// 接线依赖：调用 `crate::db::queries::dedup::list_duplicate_members`，并将 group key
/// 与 cursor 原样交给参数绑定查询；不接受前端传入路径或全库 id 数组。
#[tauri::command]
pub async fn list_duplicate_members(
    group_key: String,
    cursor: Option<String>,
    limit: u32,
    filters: DuplicateGroupFilters,
    state: State<'_, Arc<AppState>>,
) -> Result<DuplicateMemberPage> {
    validate_limit(limit)?;
    let (digest, size) = decode_group_key(&group_key)?;
    let after_item_id = decode_member_cursor(cursor)?;
    let page_limit = usize::try_from(limit).unwrap_or(MAX_PAGE_LIMIT as usize);
    let state_arc = state.inner().clone();
    let rows = tokio::task::spawn_blocking(move || {
        state_arc.with_dedup_lifecycle_read(|| {
            let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
            queries::list_duplicate_members(
                &pool,
                &digest,
                size,
                filters.include_offline,
                filters.include_zero_byte,
                after_item_id,
                page_limit.saturating_add(1),
            )
        })
    })
    .await
    .map_err(|_| cleanup_db_error())??;
    let has_next = rows.len() > page_limit;
    let rows = rows.into_iter().take(page_limit).collect::<Vec<_>>();
    let next_cursor = if has_next {
        rows.last()
            .map(|row| encode_member_cursor(row.item_id))
            .transpose()?
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(|row| DuplicateMember {
            item_id: row.item_id,
            file_name: row.file_name,
            directory_path: row.path,
            file_size: u64::try_from(row.file_size).unwrap_or(0),
            file_mtime: row.file_mtime,
            source_revision: u64::try_from(row.source_revision).unwrap_or(0),
            availability: row.availability,
            is_live_photo: row.is_live_photo,
            is_favorited: row.is_favorited,
            rating: row.rating,
            color_label: row.color_label,
            album_count: u64::try_from(row.album_count).unwrap_or(0),
            tag_count: u64::try_from(row.tag_count).unwrap_or(0),
            bookmark_count: u64::try_from(row.bookmark_count).unwrap_or(0),
        })
        .collect();
    Ok(DuplicateMemberPage { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn cleanup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        crate::db::migration::run_migrations(&conn).expect("migrate");
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             INSERT INTO scan_roots (id,path,alias) VALUES (1,'/root','root');
             INSERT INTO directories (id,root_id,rel_path,name) VALUES (10,1,'','root');
             INSERT INTO media_items
                 (id,directory_id,file_name,file_size,file_mtime,file_mtime_ns,file_format,
                  media_type,width,height,sort_datetime,cache_key)
             VALUES (1,10,'keep.jpg',10,1,1,'jpg','image',1,1,1,1),
                    (2,10,'delete.jpg',10,1,1,'jpg','image',1,1,1,2);
             INSERT INTO dedup_index
                 (item_id,source_revision,hash_version,quick_digest,exact_digest,unit_digest,
                  unit_size,status,checked_at)
             VALUES (1,1,1,X'01',X'02',zeroblob(32),10,'ready',1),
                    (2,1,1,X'01',X'02',zeroblob(32),10,'ready',1);",
        )
        .expect("seed exact-ready group");
        conn
    }

    #[test]
    fn cleanup_selection_is_checked_against_current_exact_ready_group() {
        let conn = cleanup_db();
        let group_key = encode_group_key(&[0; 32], 10).expect("group key");
        let selection = DedupCleanupSelection {
            group_key,
            keeper_id: 1,
            selected_item_ids: vec![2],
            expected_member_count: 2,
        };

        assert_eq!(
            validate_cleanup_selection(&conn, &selection).expect("current group is valid"),
            (vec![2], 10)
        );

        conn.execute(
            "UPDATE media_items SET source_revision=source_revision+1 WHERE id=?1",
            params![2],
        )
        .expect("drift source revision");
        let error =
            validate_cleanup_selection(&conn, &selection).expect_err("stale group rejected");
        assert!(matches!(
            error,
            AppError::Dedup {
                code: CODE_SOURCE_CHANGED,
                ..
            }
        ));
    }

    #[test]
    fn dto_uses_camel_case_and_keeps_cursor_opaque() {
        let request = ListDuplicateMembersRequest {
            group_key: "unit:v1:opaque".into(),
            cursor: Some("cursor%2Fwith%3Dpadding".into()),
            limit: 50,
            filters: DuplicateGroupFilters::default(),
        };
        let json = serde_json::to_value(request).expect("request serializes");
        assert_eq!(json["groupKey"], "unit:v1:opaque");
        assert_eq!(json["cursor"], "cursor%2Fwith%3Dpadding");
        assert_eq!(json["limit"], 50);
        assert_eq!(json["filters"]["includeOffline"], false);
        assert_eq!(json["filters"]["includeZeroByte"], false);
        assert!(json.get("group_key").is_none());
    }

    #[test]
    fn status_and_page_payloads_use_stable_wire_names() {
        let status = DedupStatusSnapshot {
            run_id: Some("run-1".into()),
            status: DedupRunStatus::Running,
            phase: "hashing".into(),
            items_done: 2,
            items_total: 3,
            bytes_done: 10,
            bytes_total: 20,
            groups_found: 1,
            potential_logical_bytes: 100,
            errors: vec![DedupErrorSummary {
                code: "SOURCE_STALE".into(),
                count: 1,
            }],
            waiting_on: vec!["scan".into()],
        };
        let page = DuplicateGroupPage {
            items: vec![DuplicateGroup {
                group_key: "g".into(),
                member_count: 2,
                unit_size: 50,
                potential_logical_bytes: 50,
                metadata_conflict: false,
                suggested_keeper_id: 1,
            }],
            next_cursor: Some("opaque-next".into()),
        };

        let status_json = serde_json::to_value(status).expect("status serializes");
        assert_eq!(status_json["runId"], "run-1");
        assert_eq!(status_json["itemsDone"], 2);
        assert_eq!(status_json["potentialLogicalBytes"], 100);
        assert_eq!(status_json["waitingOn"][0], "scan");

        let page_json = serde_json::to_value(page).expect("page serializes");
        assert_eq!(page_json["items"][0]["groupKey"], "g");
        assert_eq!(page_json["nextCursor"], "opaque-next");
    }

    #[test]
    fn errors_are_dedup_codes_without_internal_details() {
        let error = invalid_cursor();
        let json = serde_json::to_value(error).expect("error serializes");
        assert_eq!(json["code"], CODE_INVALID_CURSOR);
        assert_eq!(json["message"], "去重游标无效 | invalid dedup cursor");
        assert!(!json.to_string().contains("SELECT"));
        assert!(!json.to_string().contains("\\\\"));
    }

    #[test]
    fn page_limit_is_bounded_with_a_stable_error_code() {
        assert!(validate_limit(1).is_ok());
        assert!(validate_limit(MAX_PAGE_LIMIT).is_ok());
        let error = validate_limit(0).expect_err("zero limit must fail");
        let json = serde_json::to_value(error).expect("error serializes");
        assert_eq!(json["code"], CODE_INVALID_LIMIT);
        assert!(validate_limit(MAX_PAGE_LIMIT + 1).is_err());
    }
}
