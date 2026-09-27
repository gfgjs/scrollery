//! 备份包 manifest 类型(方案 B §4)。`manifest.json` 是备份包的自描述元数据:格式版本、
//! 应用/schema 版本、扫描根摘要、计数、外部版本数,以及每个 payload 条目的字节数与 SHA-256。
//!
//! **数值一律运行时读取**(schema_version 从暂存库读、counts 从库查),严禁把 V21 之类写死进实现。
//! manifest **不写凭据**;扫描根路径属恢复必要信息但含隐私,UI 须提示备份含个人路径/人名/标签。

use serde::{Deserialize, Serialize};

/// 备份包格式版本。恢复时校验 `formatVersion` 是否被本二进制支持(方案 B §6.1)。
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// 备份包扩展名(无点)。手动/自动包同扩展名,靠 `kind` 与文件名前缀区分。
pub const BACKUP_FILE_EXT: &str = "scrollerybackup";

/// `manifest.json` 单条目读取上限(防超大/敌意 manifest OOM,方案 B §6.1)。**所有**读 manifest 的
/// 路径(恢复扫描、备份列表、retention 校验)都须以此封顶——真实 manifest 远小于此(几十 KB 级)。
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

/// 包内固定条目名(恢复白名单据此,方案 B §6.1)。
pub const ENTRY_DB: &str = "db/scrollery.db";
pub const ENTRY_MANIFEST: &str = "manifest.json";
/// documents 条目前缀(包内相对路径 `documents/{item_id}/{file_name}`,方案 B §3.2 去机器绝对前缀)。
pub const ENTRY_DOCUMENTS_PREFIX: &str = "documents/";

/// 备份类型:手动触发 / 自动定时。retention 只轮转 `Auto`(B-6),永不碰 `Manual`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupKind {
    Manual,
    Auto,
}

impl BackupKind {
    /// 文件名中缀:手动 `backup`、自动 `auto`(方案 B §4 文件名规范)。
    pub fn file_infix(self) -> &'static str {
        match self {
            BackupKind::Manual => "backup",
            BackupKind::Auto => "auto",
        }
    }
}

/// 单个 payload 条目:包内相对路径 + 未压缩字节数 + 未压缩内容的 hex SHA-256。
/// zip CRC 不能替代——SHA-256 是恢复完整性校验的唯一凭据(方案 B §4)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadEntry {
    /// 包内相对路径,如 `db/scrollery.db` / `documents/12/34.txt`。
    pub path: String,
    /// 未压缩字节数。
    pub bytes: u64,
    /// 未压缩内容的 hex SHA-256(小写)。
    pub sha256: String,
}

/// 扫描根摘要(恢复摘要 + 隐私提示用)。v1 不含 volumeStableId(卷身份归恢复后 relink)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootEntry {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub hidden: bool,
}

/// 计数摘要(恢复摘要展示)。运行时从暂存库查,非承诺容量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub items: i64,
    pub albums: i64,
    pub tags: i64,
    pub named_persons: i64,
}

/// 备份包 manifest(方案 B §4)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    /// 稳定备份标识(恢复暂存目录命名用)。
    pub backup_id: String,
    pub kind: BackupKind,
    /// 应用版本(CARGO_PKG_VERSION,运行时读)。
    pub app_version: String,
    /// 产包时的运行时 schema 版本(从暂存库读,非硬编码)。
    pub schema_version: u32,
    /// 产包 UTC 时刻(RFC3339)。
    pub created_at_utc: String,
    pub roots: Vec<RootEntry>,
    pub counts: Counts,
    /// `storage='external'` 的文档版本数(不入包,恢复摘要提示,方案 B §2.2)。
    pub external_document_versions: i64,
    pub payload: Vec<PayloadEntry>,
}
