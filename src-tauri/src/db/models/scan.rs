//! 扫描根目录 / 目录树域模型。

use serde::{Deserialize, Serialize};

// ── 扫描根目录 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRoot {
    pub id: i64,
    pub path: String,
    pub alias: Option<String>,
    pub scan_status: String,
    pub scan_progress: i64,
    pub total_files: i64,
    pub last_scan_at: Option<i64>,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// 用户在设置页是否隐藏该根（V21，库级排除）：为真时该根媒体从画廊「全部」/时间轴/搜索/统计/
    /// 侧栏文件树/全选全部排除，取消即恢复。查询侧走条件子查询排除（见 `db::queries::layout`）。
    pub is_hidden: bool,
}

// ── 目录 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    pub id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub rel_path: String,
    pub name: String,
    pub depth: i64,
    pub media_count: i64,
    pub mtime: Option<i64>,
    pub created_at: i64,
}

/// 侧边栏文件夹树中使用的轻量级节点。
///
/// **两种身份**（S 线 D-013）：`node_key`/`parent_key` 是**路径身份**，恒有，与
/// 「所有文件」模式的 `TreeEntry` 同一命名空间（同一目录在两种模式下是同一个节点）；
/// `id`/`parent_id` 是**实体身份**，只在 DB 确有该目录行时才有意义。本结构是 DB 模式的
/// 产物故 `id` 必有；FS-only 目录走 `TreeEntry`，那里 `directory_id` 是 `Option`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirNode {
    /// 路径身份 `{root_id}:{rel_path}`，由 [`crate::tree::node_key`] 产出（唯一实现）。
    pub node_key: String,
    /// 父节点的路径身份；扫描根无父，为 `None`。
    pub parent_key: Option<String>,
    pub id: i64,
    pub root_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub media_count: i64,
    pub has_children: bool,
}

/// 侧边栏文件夹树中作为目录叶子显示的轻量媒体文件行。仅含文件列表所需的少量字段
///（名称 + 类型 + 收藏标志）；点击打开时再按需拉取完整项。
///
/// 同 [`DirNode`]：`node_key` 是路径身份（恒有），`id` 是媒体库实体身份。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirFile {
    /// 路径身份 `{root_id}:{rel_path}`，`rel_path` 含文件名本身。
    pub node_key: String,
    /// 所属目录的路径身份。
    pub parent_key: String,
    /// 相对扫描根的路径（含文件名本身）。
    ///
    /// 与 `node_key` 冗余（后者由 `root_id` + 本字段拼成），但**有意**单发一份：前端 reveal
    /// 要的正是它，而从 `node_key` 拆回来就是在前端重造一遍键格式的解析，从 `parent_key + 名字`
    /// 拼出来则是重造一遍 [`child_rel_path`](crate::tree::child_rel_path)。两者都是「已经算好了
    /// 却不给，让调用方自己再算一次」——而这里的产出侧本来就现成有它。
    pub rel_path: String,
    pub id: i64,
    pub file_name: String,
    pub media_type: String,
    pub is_favorited: bool,
}
