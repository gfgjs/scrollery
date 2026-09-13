//! 存储后端(网络盘,§3.8 8B)/ 卷可用性模型(SCHEMA_V10)域模型。

use serde::{Deserialize, Serialize};

// ── 存储后端（网络盘，§3.8 8B） ───────────────────────────────────────────────

/// 来自 `storage_backends` 的已配置存储后端行（§3.8）。密码绝不在此 —— 仅 `cred_ref`（keyring 查找键）。
/// `has_password` 告知 UI 是否已存密码。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageBackendInfo {
    pub id: i64,
    pub kind: String, // 'local' | 'smb' | 'webdav'
    pub name: String,
    pub host: Option<String>,
    pub base_path: Option<String>,
    pub username: Option<String>,
    pub has_password: bool,
    pub created_at: i64,
}

// ── 卷可用性模型（SCHEMA_V10）：移动盘/网络盘插拔感知，「离线≠删除」的稳定身份锚点 ──

/// 卷类型。映射 `volumes.kind` TEXT 列（'local'|'removable'|'network'|'unknown'）。
/// 未知或无法判定的卷必须保留为 `Unknown`，这样物理清理可以安全地 fail closed；读路径不
/// 因未来新增类型 panic，写路径仍使用精确字面量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeKind {
    /// 本机固定盘（只有原生卷探测确认后才可用于物理清理）。
    Local,
    /// 可移动盘（U盘 / 移动硬盘 / SD）——插拔感知的核心对象。
    Removable,
    /// 网络盘（SMB / NFS / UNC）。
    Network,
    /// 无法可靠识别的卷；只允许软删除，不允许物理清理。
    Unknown,
}

impl VolumeKind {
    /// 转 SQL TEXT 值（写严格）。
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeKind::Local => "local",
            VolumeKind::Removable => "removable",
            VolumeKind::Network => "network",
            VolumeKind::Unknown => "unknown",
        }
    }

    /// 从 SQL TEXT 值解析（读宽容）：未知值归 `Unknown`，防止安全能力因解析兜底而被误放行。
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "removable" => VolumeKind::Removable,
            "network" => VolumeKind::Network,
            "unknown" => VolumeKind::Unknown,
            "local" => VolumeKind::Local,
            _ => VolumeKind::Unknown,
        }
    }
}

/// 一条卷登记行（`volumes` 表，SCHEMA_V10）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub id: i64,
    /// 稳定身份锚点：Win 卷GUID / mac 卷UUID / 规范化 UNC；迁移占位期为 `'pending:<scan_root_id>'`，
    /// 由 `probe_volumes`（Part2）首次运行覆写为真实卷 ID。
    pub stable_id: String,
    /// 卷标（展示用，用户可改名）。
    pub label: Option<String>,
    pub kind: VolumeKind,
    /// 最近挂载点 / 盘符（提示 + 运行期路径重组用，**非身份键**——盘符会变，stable_id 不变）。
    pub last_mount_path: Option<String>,
    /// 最近在线 unix 秒。
    pub last_seen: Option<i64>,
    pub is_online: bool,
    pub created_at: i64,
}

/// `upsert_volume` 入参（不含 `id` / `created_at`——由 DB 生成 / 保留）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVolume {
    pub stable_id: String,
    pub label: Option<String>,
    pub kind: VolumeKind,
    pub last_mount_path: Option<String>,
    pub last_seen: Option<i64>,
    pub is_online: bool,
}
