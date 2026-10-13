//! 将备份包作为不可信输入：校验条目白名单、路径、大小和 SHA-256，解压到同卷暂存目录。
//! 暂存库须通过完整性、外键和当前 schema 校验；跨机恢复时重写 appdata 文档路径。
//! 本模块返回确认摘要，不修改活库；启动交换由 `swap` 处理。

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::manifest::{
    Counts, Manifest, RootEntry, BACKUP_FORMAT_VERSION, ENTRY_DB, ENTRY_DOCUMENTS_PREFIX,
    ENTRY_MANIFEST, MAX_MANIFEST_BYTES,
};
use crate::error::{AppError, Result};
use crate::exotic::package::is_safe_relative_path;

// ── 稳定错误码(方案 §9;透到 IPC `code` 供前端分流)────────────────────────────────
pub const CODE_FORMAT_UNSUPPORTED: &str = "restore_format_unsupported";
/// 备份库结构非当前格式:过旧与过新**一律不接受**,统一此稳定码(无 old/new 分支)。
pub const CODE_SCHEMA_INCOMPATIBLE: &str = "restore_schema_incompatible";
pub const CODE_CORRUPT: &str = "restore_corrupt";
pub const CODE_PATH_INVALID: &str = "restore_path_invalid";
pub const CODE_SIZE_LIMIT: &str = "restore_size_limit";
pub const CODE_DOCUMENT_MISSING: &str = "restore_document_missing";
pub const CODE_IO: &str = "restore_io";
/// 已进入启动恢复流程的暂存不得再清理或覆盖。
pub const CODE_PENDING: &str = "restore_pending";
/// 预检结果已被同一备份的新一轮预检替换。
pub const CODE_STAGE_STALE: &str = "restore_stage_stale";

const STAGING_TOKEN_FILE: &str = "stage-token";

/// 应用可运行多个进程，恢复门闩必须覆盖所有共享 appdata 的实例。
pub(super) struct RestoreLock {
    _file: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}

pub(super) fn acquire_restore_lock(app_data: &Path) -> Result<RestoreLock> {
    let app_data = std::fs::canonicalize(app_data).map_err(io_err)?;
    #[cfg(windows)]
    let mut parents = Vec::new();
    #[cfg(windows)]
    for ancestor in app_data.ancestors().collect::<Vec<_>>().into_iter().rev() {
        parents.push(pin_directory(ancestor)?);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x0020_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(app_data.join("restore.lock"))
        .map_err(io_err)?;
    let metadata = file.metadata().map_err(io_err)?;
    if !metadata.is_file() || metadata_is_link(&metadata) {
        return Err(path_invalid("恢复锁不允许链接或特殊文件"));
    }
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => {
            err(super::core::CODE_JOB_BUSY, "其他进程正在执行恢复任务")
        }
        std::fs::TryLockError::Error(error) => io_err(error),
    })?;
    Ok(RestoreLock {
        _file: file,
        #[cfg(windows)]
        _parents: parents,
    })
}

/// 条目数硬上限(远超真实库;防中央目录膨胀)。
const MAX_ENTRIES: usize = 4_000_000;
/// 解压总尺寸绝对上界(sanity;真实 catalog 远小于此)。
const MAX_TOTAL_UNCOMPRESSED: u64 = 1 << 40; // 1 TiB
/// 解压后须留的磁盘余量(可用空间可判定时)。
const SPACE_MARGIN: u64 = 256 * 1024 * 1024;

fn err(code: &'static str, msg: &str) -> AppError {
    AppError::Restore {
        code,
        message: msg.to_string(),
    }
}
fn corrupt(msg: &str) -> AppError {
    err(CODE_CORRUPT, msg)
}
fn path_invalid(msg: &str) -> AppError {
    err(CODE_PATH_INVALID, msg)
}
fn io_err(_e: std::io::Error) -> AppError {
    err(CODE_IO, "恢复读写失败 | restore io failed")
}
fn db_err(_e: rusqlite::Error) -> AppError {
    err(CODE_IO, "恢复读取暂存库失败 | restore db failed")
}

fn is_symlink_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o120000
}

/// payload 路径白名单形态:恰是 `db/scrollery.db` 或 `documents/` 前缀(拒任意安全相对路径)。
fn is_allowed_payload_path(p: &str) -> bool {
    p == ENTRY_DB || p.starts_with(ENTRY_DOCUMENTS_PREFIX)
}

/// `backup_id` 是否为**安全单路径段**。它被直接用作目录名拼接
/// (`restore-staging/{id}`、`restore-old/{id}`)并进入 `remove_dir_all`,故必须校验:
/// 复用 [`is_safe_relative_path`](拒 `..`/绝对/盘符/反斜杠/保留设备名/首尾点空格/控制字符),
/// 再钉死**单段**(无 `/`)。否则恶意包 `"backupId":"../../x"` 或 `"C:/Users"` 会使清理/解压逃逸
/// appdata(方案 §6.1 把包当不可信输入的红线)。
pub fn is_safe_backup_id(id: &str) -> bool {
    is_safe_relative_path(id) && !id.contains('/')
}

/// 恢复暂存结果摘要(供 UI 双确认,方案 §6.1)。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreStageResult {
    pub backup_id: String,
    /// 本次预检的所有权标识，防止旧向导清理或确认同一备份的新暂存。
    pub staging_token: String,
    /// 暂存目录(供 §6.2 arm/swap 阶段用)。
    pub staging_dir: String,
    /// 暂存库的 schema 版本，须等于运行时 `SCHEMA_VERSION`。
    pub schema_version: u32,
    pub kind: String,
    pub created_at_utc: String,
    pub counts: Counts,
    pub roots: Vec<RootEntry>,
    pub external_document_versions: i64,
    /// rebase 校验通过的 appdata 文档版本数。
    pub appdata_document_count: usize,
}

/// 暂存目录 RAII 清理:失败即回收半装产物,不留残迹。成功后 `disarm` 保留(供交换阶段用)。
struct StagingGuard<'a>(Option<&'a StagingLocation>);
impl StagingGuard<'_> {
    fn disarm(mut self) {
        self.0 = None;
    }
}
impl Drop for StagingGuard<'_> {
    fn drop(&mut self) {
        if let Some(location) = &self.0 {
            let _ = location.remove_existing();
        }
    }
}

/// 校验并暂存一个备份包(方案 §6.1)。`target_app_data_dir` = 本机(目标)应用数据根:
/// 解压落它下 `restore-staging/{backupId}/`,并把 appdata 文档路径 rebase 到它下(§3.2 跨机)。
pub fn restore_stage(
    package_path: &Path,
    target_app_data_dir: &Path,
) -> Result<RestoreStageResult> {
    // 库入口也允许为尚未初始化的目标 appdata 首次预检。
    std::fs::create_dir_all(target_app_data_dir).map_err(io_err)?;
    let _lock = acquire_restore_lock(target_app_data_dir)?;
    // 1. 打开包 + 读 manifest(有界)。
    let file = File::open(package_path).map_err(io_err)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| corrupt("备份包无法打开"))?;
    let manifest = read_manifest(&mut archive)?;
    if manifest.format_version > BACKUP_FORMAT_VERSION {
        return Err(err(CODE_FORMAT_UNSUPPORTED, "备份包格式版本过新"));
    }
    // backup_id 直接用于拼 staging/old 目录名(下方 remove_dir_all + 解压落点),先当不可信输入校验:
    // 拒路径穿越/绝对/盘符/多段,否则清理与解压会逃逸 appdata。
    if !is_safe_backup_id(&manifest.backup_id) {
        return Err(path_invalid("备份包 backupId 非法"));
    }

    // 2. 扫描中央目录:名字安全 / 符号链接拒 / 白名单(manifest.payload)/ checked 尺寸 / 可用空间。
    scan_central_directory(&mut archive, &manifest, target_app_data_dir)?;

    // 3. 解压 + 逐条 sha256 校验到 appdata 同卷 staging。失败 RAII 清理。
    let location = StagingLocation::open(target_app_data_dir, &manifest.backup_id, true)?
        .ok_or_else(|| err(CODE_IO, "暂存父目录缺失"))?;
    location.ensure_not_pending(&manifest.backup_id)?;
    location.remove_existing()?;
    let staging = location.path.clone();
    let cleanup = StagingGuard(Some(&location));
    extract_and_verify(&mut archive, &manifest, &staging)?;

    // 4. 暂存库校验(quick_check / foreign_key_check)+ 格式标识必须等于当前格式。
    let staged_db = staging.join(ENTRY_DB);
    let schema_version = validate_staged_db(&staged_db)?;

    // 5. §3.2 abs_path 跨机 rebase + 每行版本文件在包内交叉核对。
    let appdata_document_count =
        rebase_appdata_documents(&staged_db, target_app_data_dir, &staging)?;

    // 6. 摘要(从暂存库读)。
    let sconn = Connection::open(&staged_db).map_err(db_err)?;
    let counts = read_counts(&sconn)?;
    let roots = read_roots(&sconn)?;
    let external_document_versions = read_external_count(&sconn)?;
    drop(sconn);

    use ring::rand::{SecureRandom, SystemRandom};
    let mut token = [0u8; 16];
    SystemRandom::new()
        .fill(&mut token)
        .map_err(|_| err(CODE_IO, "无法创建暂存标识"))?;
    let staging_token = crate::utils::hash::to_hex_lower(&token);
    crate::thumbnail::generator::write_atomic(
        &staging.join(STAGING_TOKEN_FILE),
        staging_token.as_bytes(),
    )
    .map_err(io_err)?;

    cleanup.disarm(); // 成功:保留 staging
    Ok(RestoreStageResult {
        backup_id: manifest.backup_id,
        staging_token,
        staging_dir: staging.to_string_lossy().to_string(),
        schema_version,
        kind: manifest.kind.file_infix().to_string(),
        created_at_utc: manifest.created_at_utc,
        counts,
        roots,
        external_document_versions,
        appdata_document_count,
    })
}

/// 释放仍属于该预检结果的暂存；目录缺失或所有权已更新时幂等成功。
/// 调用方须持有文件任务门闩，与预检及恢复确认串行执行。
pub fn restore_discard(app_data: &Path, backup_id: &str, staging_token: &str) -> Result<()> {
    let _lock = acquire_restore_lock(app_data)?;
    let Some(location) = StagingLocation::open(app_data, backup_id, false)? else {
        return Ok(());
    };
    location.ensure_not_pending(backup_id)?;
    let Some(target) = location.open_target()? else {
        return Ok(());
    };
    if location.token_matches(&target, staging_token)? {
        location.remove(target)?;
    }
    Ok(())
}

pub(super) struct ValidatedStaging {
    location: StagingLocation,
    _target: File,
}

impl ValidatedStaging {
    pub(super) fn path(&self) -> &Path {
        &self.location.path
    }
}

/// 调用方已持跨进程恢复锁；句柄在 arm 完成前保持目标目录固定。
pub(super) fn validate_staging_owner(
    app_data: &Path,
    backup_id: &str,
    staging_token: &str,
) -> Result<ValidatedStaging> {
    let location = StagingLocation::open(app_data, backup_id, false)?
        .ok_or_else(|| err(CODE_STAGE_STALE, "预检已失效，请重新选择备份"))?;
    location.ensure_not_pending(backup_id)?;
    let target = location
        .open_target()?
        .ok_or_else(|| err(CODE_STAGE_STALE, "预检已失效，请重新选择备份"))?;
    if !location.token_matches(&target, staging_token)? || !location.path.join(ENTRY_DB).is_file() {
        return Err(err(CODE_STAGE_STALE, "预检已失效，请重新选择备份"));
    }
    Ok(ValidatedStaging {
        location,
        _target: target,
    })
}

// 清理只接受从 appdata 派生的单段目录，绝不接受客户端提供的删除路径。
struct StagingLocation {
    #[cfg(windows)]
    app_data: PathBuf,
    path: PathBuf,
    #[cfg(windows)]
    _parents: Vec<File>,
    #[cfg(unix)]
    app_data_handle: File,
    #[cfg(unix)]
    parent_handle: File,
}

fn metadata_is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn checked_directory(path: &Path) -> Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_err(e)),
    };
    if !metadata.is_dir()
        || metadata_is_link(&metadata)
        || std::fs::canonicalize(path).map_err(io_err)? != path
    {
        return Err(path_invalid("恢复暂存路径不是本机受控目录"));
    }
    Ok(true)
}

#[cfg(windows)]
fn pin_directory(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(io_err)?;
    let metadata = file.metadata().map_err(io_err)?;
    if !metadata.is_dir() || metadata_is_link(&metadata) {
        return Err(path_invalid("恢复暂存目录不允许链接"));
    }
    Ok(file)
}

pub(super) fn read_regular_file(path: &Path) -> Result<Option<File>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    match options.open(path) {
        Ok(file) => {
            let metadata = file.metadata().map_err(io_err)?;
            if !metadata.is_file() || metadata_is_link(&metadata) {
                return Err(path_invalid("恢复标记不允许链接或特殊文件"));
            }
            Ok(Some(file))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(e)),
    }
}

#[cfg(unix)]
fn open_directory_at(parent: &File, name: &std::ffi::OsStr) -> Result<Option<File>> {
    use rustix::fs::{openat, Mode, OFlags};
    match openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => Ok(Some(File::from(fd))),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
            Err(path_invalid("恢复暂存目录不允许链接"))
        }
        Err(e) => Err(io_err(e.into())),
    }
}

#[cfg(unix)]
fn read_regular_at(parent: &File, name: &str) -> Result<Option<File>> {
    use rustix::fs::{openat, Mode, OFlags};
    match openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let file = File::from(fd);
            if !file.metadata().map_err(io_err)?.is_file() {
                return Err(path_invalid("恢复标记不允许特殊文件"));
            }
            Ok(Some(file))
        }
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(rustix::io::Errno::LOOP) => Err(path_invalid("恢复标记不允许链接")),
        Err(e) => Err(io_err(e.into())),
    }
}

#[cfg(unix)]
fn remove_directory_contents(directory: &File, preserve_token: bool) -> Result<()> {
    use rustix::fs::{openat, unlinkat, AtFlags, Dir, Mode, OFlags};
    let entries = Dir::read_from(directory).map_err(|e| io_err(e.into()))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(e.into()))?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if preserve_token && name.to_bytes() == STAGING_TOKEN_FILE.as_bytes() {
            continue;
        }
        // 仅从固定目录句柄打开子项；链接只删除目录项，永不沿链接递归。
        let flags = match openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => {
                remove_directory_contents(&File::from(fd), false)?;
                AtFlags::REMOVEDIR
            }
            Err(rustix::io::Errno::NOENT) => continue,
            Err(rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => AtFlags::empty(),
            Err(e) => return Err(io_err(e.into())),
        };
        match unlinkat(directory, name, flags) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => (),
            Err(e) => return Err(io_err(e.into())),
        }
    }
    Ok(())
}

impl StagingLocation {
    fn open(app_data: &Path, backup_id: &str, create_parent: bool) -> Result<Option<Self>> {
        if !is_safe_backup_id(backup_id) {
            return Err(path_invalid("非法 backupId | invalid backup id"));
        }
        let app_data = std::fs::canonicalize(app_data).map_err(io_err)?;
        #[cfg(windows)]
        let mut parents = Vec::new();
        #[cfg(windows)]
        for ancestor in app_data.ancestors().collect::<Vec<_>>().into_iter().rev() {
            // 固定整条祖先链，禁止校验后将父目录替换成 junction 再引导递归删除。
            parents.push(pin_directory(ancestor)?);
        }
        #[cfg(unix)]
        let app_data_handle = {
            use rustix::fs::{open, Mode, OFlags};
            File::from(
                open(
                    &app_data,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|e| io_err(e.into()))?,
            )
        };
        let parent = app_data.join("restore-staging");
        if create_parent {
            #[cfg(windows)]
            match std::fs::create_dir(&parent) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(io_err(e)),
            }
            #[cfg(unix)]
            match rustix::fs::mkdirat(
                &app_data_handle,
                "restore-staging",
                rustix::fs::Mode::from_raw_mode(0o700),
            ) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                Err(e) => return Err(io_err(e.into())),
            }
        }
        if !checked_directory(&parent)? {
            return Ok(None);
        }
        #[cfg(windows)]
        parents.push(pin_directory(&parent)?);
        #[cfg(unix)]
        let Some(parent_handle) =
            open_directory_at(&app_data_handle, std::ffi::OsStr::new("restore-staging"))?
        else {
            return Ok(None);
        };
        let path = parent.join(backup_id);
        checked_directory(&path)?;
        Ok(Some(Self {
            #[cfg(windows)]
            app_data,
            path,
            #[cfg(windows)]
            _parents: parents,
            #[cfg(unix)]
            app_data_handle,
            #[cfg(unix)]
            parent_handle,
        }))
    }

    fn ensure_not_pending(&self, backup_id: &str) -> Result<()> {
        #[cfg(windows)]
        let marker = read_regular_file(&self.app_data.join("pending-restore.json"))?;
        #[cfg(unix)]
        let marker = read_regular_at(&self.app_data_handle, "pending-restore.json")?;
        let Some(marker) = marker else {
            return Ok(());
        };
        let mut text = String::new();
        marker
            .take(16 * 1024)
            .read_to_string(&mut text)
            .map_err(io_err)?;
        let pending: super::swap::PendingRestore = serde_json::from_str(&text).map_err(|_| {
            err(
                super::swap::CODE_MARKER_CORRUPT,
                "恢复标记损坏，暂存不能清理或覆盖",
            )
        })?;
        let pending_path = Path::new(&pending.staging_dir);
        let is_pending = if pending.backup_id == backup_id || pending_path == self.path {
            true
        } else {
            match std::fs::canonicalize(pending_path) {
                Ok(path) => path == self.path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(e) => return Err(io_err(e)),
            }
        };
        if is_pending {
            return Err(err(CODE_PENDING, "恢复已确认，暂存必须保留到重启完成"));
        }
        Ok(())
    }

    fn open_target(&self) -> Result<Option<File>> {
        #[cfg(windows)]
        {
            if !checked_directory(&self.path)? {
                return Ok(None);
            }
            pin_directory(&self.path).map(Some)
        }
        #[cfg(unix)]
        {
            open_directory_at(&self.parent_handle, self.path.file_name().unwrap())
        }
    }

    fn token_matches(&self, _target: &File, staging_token: &str) -> Result<bool> {
        if staging_token.len() != 32 || !staging_token.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Ok(false);
        }
        #[cfg(windows)]
        let token = read_regular_file(&self.path.join(STAGING_TOKEN_FILE))?;
        #[cfg(unix)]
        let token = read_regular_at(_target, STAGING_TOKEN_FILE)?;
        let Some(token) = token else {
            return Ok(false);
        };
        let mut stored = String::new();
        token.take(33).read_to_string(&mut stored).map_err(io_err)?;
        Ok(stored == staging_token)
    }

    fn remove_existing(&self) -> Result<()> {
        if let Some(target) = self.open_target()? {
            self.remove(target)?;
        }
        Ok(())
    }

    fn remove(&self, target: File) -> Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            for entry in std::fs::read_dir(&self.path).map_err(io_err)? {
                let entry = entry.map_err(io_err)?;
                if entry.file_name() == STAGING_TOKEN_FILE {
                    continue;
                }
                let kind = entry.file_type().map_err(io_err)?;
                if kind.is_dir() || kind.is_symlink_dir() {
                    std::fs::remove_dir_all(entry.path()).map_err(io_err)?;
                } else {
                    std::fs::remove_file(entry.path()).map_err(io_err)?;
                }
            }
            // 所有内容删完才移除所有权标记，文件占用失败不会使重试丢失授权。
            match std::fs::remove_file(self.path.join(STAGING_TOKEN_FILE)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(io_err(e)),
            }
            drop(target);
            std::fs::remove_dir(&self.path).map_err(io_err)?;
        }
        #[cfg(unix)]
        {
            remove_directory_contents(&target, true)?;
            match rustix::fs::unlinkat(&target, STAGING_TOKEN_FILE, rustix::fs::AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => (),
                Err(e) => return Err(io_err(e.into())),
            }
            rustix::fs::unlinkat(
                &self.parent_handle,
                self.path.file_name().unwrap(),
                rustix::fs::AtFlags::REMOVEDIR,
            )
            .map_err(|e| io_err(e.into()))?;
        }
        Ok(())
    }
}

fn read_manifest(archive: &mut zip::ZipArchive<File>) -> Result<Manifest> {
    let e = archive
        .by_name(ENTRY_MANIFEST)
        .map_err(|_| corrupt("备份包缺 manifest"))?;
    if e.size() > MAX_MANIFEST_BYTES {
        return Err(err(CODE_SIZE_LIMIT, "manifest 过大"));
    }
    let mut s = String::new();
    e.take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut s)
        .map_err(|_| corrupt("manifest 读取失败"))?;
    serde_json::from_str(&s).map_err(|_| corrupt("manifest 解析失败"))
}

/// 扫描中央目录:每条目名字安全 + 非符号链接 + 属白名单(manifest.json 或 payload 声明);
/// checked 汇总解压尺寸,超绝对上界 / 超可用空间余量即拒(zip bomb 防线之一)。
fn scan_central_directory(
    archive: &mut zip::ZipArchive<File>,
    manifest: &Manifest,
    target_app_data_dir: &Path,
) -> Result<()> {
    let n = archive.len();
    if n > MAX_ENTRIES {
        return Err(err(CODE_SIZE_LIMIT, "备份包条目数超限"));
    }
    // payload 声明即白名单;先校验每个声明路径形态合法。
    let mut payload_paths: HashSet<&str> = HashSet::new();
    for p in &manifest.payload {
        if !is_safe_relative_path(&p.path) || !is_allowed_payload_path(&p.path) {
            return Err(path_invalid("payload 路径非法"));
        }
        payload_paths.insert(p.path.as_str());
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut total: u64 = 0;
    for i in 0..n {
        let f = archive.by_index(i).map_err(|_| corrupt("条目读取失败"))?;
        let name = f.name().to_string();
        if let Some(mode) = f.unix_mode() {
            if is_symlink_mode(mode) {
                return Err(path_invalid("备份包含符号链接条目"));
            }
        }
        if f.is_dir() {
            let t = name.trim_end_matches('/');
            if !t.is_empty() && !is_safe_relative_path(t) {
                return Err(path_invalid("目录条目路径非法"));
            }
            continue;
        }
        if !is_safe_relative_path(&name) {
            return Err(path_invalid("条目路径非法"));
        }
        // 白名单:manifest.json 或 payload 声明内(多余文件拒)。
        if name != ENTRY_MANIFEST && !payload_paths.contains(name.as_str()) {
            return Err(corrupt("备份包含清单外的条目"));
        }
        // 大小写碰撞(Windows 大小写不敏感盘)。
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(path_invalid("重复/大小写碰撞条目"));
        }
        total = total
            .checked_add(f.size())
            .ok_or_else(|| err(CODE_SIZE_LIMIT, "解压尺寸溢出"))?;
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(err(CODE_SIZE_LIMIT, "解压尺寸超绝对上限"));
        }
    }
    // 每个 payload 声明的文件必须在包内(缺文件拒)。
    for p in &manifest.payload {
        if !seen.contains(&p.path.to_ascii_lowercase()) {
            return Err(corrupt("备份包缺 payload 声明的文件"));
        }
    }
    // 可用空间余量(可判定时;不能只信 manifest 声明防 zip bomb,方案 §6.1)。
    if let Some(avail) = available_space(target_app_data_dir) {
        if total.saturating_add(SPACE_MARGIN) > avail {
            return Err(err(CODE_SIZE_LIMIT, "目标可用空间不足"));
        }
    }
    Ok(())
}

/// 解压每个 payload 到 staging(安全 join + create_new 拒跟随既有/链接 + 边写边核 size/sha256 +
/// 落地后再核 symlink 兜底)。任一步失败由调用方 RAII 清理整个 staging。
fn extract_and_verify(
    archive: &mut zip::ZipArchive<File>,
    manifest: &Manifest,
    staging: &Path,
) -> Result<()> {
    std::fs::create_dir_all(staging).map_err(io_err)?;
    for p in &manifest.payload {
        let dest = safe_join(staging, &p.path)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut zf = archive
            .by_name(&p.path)
            .map_err(|_| corrupt("payload 条目缺失"))?;
        // create_new:拒跟随既有文件/符号链接(防解压前被布置的链接逃逸)。
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(io_err)?;
        let (size, sha) = copy_hashing(&mut zf, &mut out, p.bytes)?;
        if size != p.bytes || sha != p.sha256 {
            return Err(corrupt("payload 尺寸或哈希不符"));
        }
        // 纵深兜底:落地后若意外为符号链接(理论不可达)即拒。
        let ft = std::fs::symlink_metadata(&dest)
            .map_err(io_err)?
            .file_type();
        if ft.is_symlink() {
            return Err(path_invalid("payload 落地为符号链接"));
        }
    }
    Ok(())
}

/// 取路径 basename,同时认 `/` 与 `\`。跨机恢复:源机绝对路径的分隔符可能与目标机 OS 不同,
/// **不能**依赖平台 `Path::file_name`(§3.2)。返回最后一个非空段;尾随分隔符 / 空串 → `None`。
fn basename_cross_platform(abs: &str) -> Option<&str> {
    abs.rsplit(['/', '\\']).next().filter(|s| !s.is_empty())
}

fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    if !is_safe_relative_path(rel) {
        return Err(path_invalid("条目路径非法"));
    }
    let mut p = base.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    Ok(p)
}

/// 边复制边算 sha256/size,硬封顶 = 声明字节数+1(声明小、实际大的 bomb 读超即判损坏)。
fn copy_hashing(src: &mut impl Read, dst: &mut impl Write, declared: u64) -> Result<(u64, String)> {
    let cap = declared.saturating_add(1);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    let mut total: u64 = 0;
    loop {
        let n = src.read(&mut buf).map_err(io_err)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| err(CODE_SIZE_LIMIT, "文件大小溢出"))?;
        if total > cap {
            return Err(corrupt("payload 超出声明尺寸"));
        }
        hasher.update(&buf[..n]);
        dst.write_all(&buf[..n]).map_err(io_err)?;
    }
    Ok((total, crate::utils::hash::to_hex_lower(&hasher.finalize())))
}

/// 暂存库须通过 quick_check 和外键检查，且格式标识等于当前格式。
/// 过旧或过新的包均返回 `CODE_SCHEMA_INCOMPATIBLE`，不修改暂存库结构。
fn validate_staged_db(staged_db: &Path) -> Result<u32> {
    let conn = Connection::open(staged_db).map_err(db_err)?;
    quick_check(&conn)?;
    foreign_key_check(&conn)?;
    let pkg_version = crate::db::schema::read_schema_version(&conn);
    if pkg_version != crate::db::schema::SCHEMA_VERSION {
        return Err(err(
            CODE_SCHEMA_INCOMPATIBLE,
            "备份库结构非当前格式,请用当前版本重新导出备份",
        ));
    }
    Ok(pkg_version)
}

fn quick_check(conn: &Connection) -> Result<()> {
    // integrity SQL 单源于 super::dbread;此处仅映射 restore 域错误(quick_check 失败/未通过)。
    if !super::dbread::integrity_ok(conn).map_err(|_| corrupt("quick_check 失败"))? {
        return Err(corrupt("quick_check 未通过"));
    }
    Ok(())
}

fn foreign_key_check(conn: &Connection) -> Result<()> {
    // 有任何返回行 = 存在外键违规。
    let mut stmt = conn.prepare("PRAGMA foreign_key_check").map_err(db_err)?;
    let mut rows = stmt.query([]).map_err(db_err)?;
    if rows.next().map_err(db_err)?.is_some() {
        return Err(corrupt("外键完整性校验未通过"));
    }
    Ok(())
}

/// §3.2 跨机 rebase:暂存库内 `storage='appdata'` 行的 `abs_path` 用**绑定参数**改写为目标机
/// `app_data_dir/documents/{item_id}/{file_name}`;`item_id + file_name` 与包内解压白名单交叉
/// 验证(拒 `..`/绝对/盘符/符号链接逃逸与缺件)。返回 rebase 通过的行数。
fn rebase_appdata_documents(
    staged_db: &Path,
    target_app_data_dir: &Path,
    staging: &Path,
) -> Result<usize> {
    let conn = Connection::open(staged_db).map_err(db_err)?;
    let rows: Vec<(i64, i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, item_id, abs_path FROM document_versions WHERE storage='appdata'")
            .map_err(db_err)?;
        let mapped = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut v = Vec::new();
        for row in mapped {
            v.push(row.map_err(db_err)?);
        }
        v
    };

    let target_docs = target_app_data_dir.join("documents");
    let staging_docs = staging.join("documents");
    let mut count = 0usize;
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    for (vid, item_id, old_abs) in rows {
        // 只取旧路径的**basename**(丢弃机器绝对前缀),不做字符串替换旧前缀(§3.2)。
        let file_name = basename_cross_platform(&old_abs)
            .ok_or_else(|| path_invalid("版本文件名非法"))?
            .to_string();
        // 相对形态须整体安全(item_id 恒数字;file_name 须单段无 / 与 .. )。
        let rel = format!("documents/{item_id}/{file_name}");
        if !is_safe_relative_path(&rel) {
            return Err(path_invalid("版本相对路径非法"));
        }
        // 交叉核对:该文件必须在包内解压出(staging/documents/{item_id}/{file_name}),且非符号链接。
        let extracted = staging_docs.join(item_id.to_string()).join(&file_name);
        let meta = std::fs::symlink_metadata(&extracted)
            .map_err(|_| err(CODE_DOCUMENT_MISSING, "版本文件在包内缺失"))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(path_invalid("版本文件形态非法"));
        }
        // rebase 到目标机路径(绑定参数)。
        let new_abs = target_docs.join(item_id.to_string()).join(&file_name);
        tx.execute(
            "UPDATE document_versions SET abs_path=?2 WHERE id=?1",
            params![vid, new_abs.to_string_lossy()],
        )
        .map_err(db_err)?;
        count += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(count)
}

// counts / roots / external 计数与 quick_check 的 SQL 单源于 super::dbread(审查 #E 去重),
// 此处仅作 restore 域错误映射(restore_io / restore_corrupt)的薄封装,保留原调用点。
fn read_counts(conn: &Connection) -> Result<Counts> {
    super::dbread::read_counts(conn).map_err(db_err)
}

fn read_roots(conn: &Connection) -> Result<Vec<RootEntry>> {
    super::dbread::read_roots(conn).map_err(db_err)
}

fn read_external_count(conn: &Connection) -> Result<i64> {
    super::dbread::read_external_count(conn).map_err(db_err)
}

// ── 可用空间(best-effort;Windows-first 交付,其它平台跳过该项余量检查)──────────────
#[cfg(windows)]
fn available_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut free_avail: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free_avail), None, None).ok()?;
    }
    Some(free_avail)
}
#[cfg(not(windows))]
fn available_space(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::core::{run_backup, BackupParams};
    use crate::backup::manifest::BackupKind;
    use tokio_util::sync::CancellationToken;

    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "scrollery_rstest_{}_{}_{}",
            std::process::id(),
            tag,
            seq
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造源 app_data(scrollery.db 跑迁移 + 合法 FK 链 + 一个 appdata 文档版本真文件)。
    /// 完整 scan_roots → directories → media_items → document_versions 链,使恢复的
    /// foreign_key_check 通过(真库不含孤儿版本行;FK 默认 off,按依赖序插即可)。
    fn seed_source(app_data: &Path) -> PathBuf {
        std::fs::create_dir_all(app_data).unwrap();
        let db_path = app_data.join("scrollery.db");
        let conn = Connection::open(&db_path).unwrap();
        crate::db::schema::initialize_schema(&conn).unwrap();
        let item_id = 42i64;
        conn.execute(
            "INSERT INTO scan_roots (id, path, alias, is_hidden) VALUES (1, ?1, '照片库', 0)",
            params![app_data.join("lib").to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO directories (id, root_id, rel_path, name) VALUES (1, 1, 'sub', 'sub')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_items \
             (id, directory_id, file_name, file_size, file_mtime, file_format, sort_datetime, cache_key) \
             VALUES (?1, 1, 'doc.txt', 1, 1, 'txt', 1, 1)",
            params![item_id],
        )
        .unwrap();
        let docdir = app_data.join("documents").join(item_id.to_string());
        std::fs::create_dir_all(&docdir).unwrap();
        let content = "版本正文内容";
        let vfile = docdir.join("100.txt");
        std::fs::write(&vfile, content).unwrap();
        let hash = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(content.as_bytes()));
        conn.execute(
            "INSERT INTO document_versions (id, item_id, storage, abs_path, source, content_hash) \
             VALUES (100, ?1, 'appdata', ?2, 'user', ?3)",
            params![item_id, vfile.to_string_lossy(), hash],
        )
        .unwrap();
        db_path
    }

    fn make_package(src_app_data: &Path, dest: &Path, backup_id: &str) -> PathBuf {
        let db = seed_source(src_app_data);
        std::fs::create_dir_all(dest).unwrap();
        let params = BackupParams {
            source_db_path: &db,
            app_data_dir: src_app_data,
            dest_dir: dest,
            kind: BackupKind::Manual,
            app_version: "0.1.0".into(),
            backup_id: backup_id.into(),
            created_at_utc: "2026-07-19T00:00:00Z".into(),
            timestamp_label: "20260719-000000".into(),
            auto_retention: None,
        };
        run_backup(&params, &CancellationToken::new()).unwrap().path
    }

    fn arm_preview(target: &Path, staged: &RestoreStageResult) -> Result<()> {
        super::super::arm_restore(
            target,
            &staged.backup_id,
            Path::new(&staged.staging_dir),
            &staged.staging_token,
            "rollback",
            "2026-10-04T00:00:00Z",
            "20261004-000000",
            "0.1.0",
            &CancellationToken::new(),
        )
    }

    #[test]
    fn restore_discard_releases_only_staged_data() {
        let root = tempfile::tempdir().unwrap();
        let package = make_package(
            &root.path().join("source"),
            &root.path().join("backups"),
            "owned",
        );
        let target = root.path().join("target");
        let live_db = seed_source(&target);
        let original_db = std::fs::read(&live_db).unwrap();
        let old = target.join("restore-old/owned/keep.txt");
        let neighbor = target.join("restore-staging/neighbor/keep.txt");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(neighbor.parent().unwrap()).unwrap();
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&neighbor, b"neighbor").unwrap();
        let staged = restore_stage(&package, &target).unwrap();
        let staging = Path::new(&staged.staging_dir);
        assert!(staging.join(ENTRY_DB).is_file());
        assert!(staging.join("documents/42/100.txt").is_file());

        restore_discard(&target, &staged.backup_id, &staged.staging_token).unwrap();
        assert!(!staging.exists());
        restore_discard(&target, &staged.backup_id, &staged.staging_token).unwrap();
        assert_eq!(std::fs::read(&live_db).unwrap(), original_db);
        assert_eq!(
            std::fs::read(target.join("documents/42/100.txt")).unwrap(),
            "版本正文内容".as_bytes()
        );
        assert_eq!(std::fs::read(&old).unwrap(), b"old");
        assert_eq!(std::fs::read(&neighbor).unwrap(), b"neighbor");
        assert!(package.is_file());
    }

    #[test]
    fn restore_staging_ownership_rejects_outdated_preview() {
        let root = tempfile::tempdir().unwrap();
        let package = make_package(
            &root.path().join("source"),
            &root.path().join("backups"),
            "same-id",
        );
        let target = root.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        let first = restore_stage(&package, &target).unwrap();
        let second = restore_stage(&package, &target).unwrap();
        assert_ne!(first.staging_token, second.staging_token);
        assert_eq!(first.staging_dir, second.staging_dir);

        restore_discard(&target, &first.backup_id, &first.staging_token).unwrap();
        assert!(Path::new(&second.staging_dir).join(ENTRY_DB).is_file());
        assert!(matches!(
            arm_preview(&target, &first),
            Err(AppError::Restore {
                code: CODE_STAGE_STALE,
                ..
            })
        ));
        assert_eq!(
            validate_staging_owner(&target, &second.backup_id, &second.staging_token)
                .unwrap()
                .path(),
            Path::new(&second.staging_dir)
        );
        restore_discard(&target, &second.backup_id, &second.staging_token).unwrap();
        assert!(!Path::new(&second.staging_dir).exists());
    }

    #[test]
    fn restore_pending_preserves_staging_until_restart() {
        let root = tempfile::tempdir().unwrap();
        let package = make_package(
            &root.path().join("source"),
            &root.path().join("backups"),
            "armed",
        );
        let target = root.path().join("target");
        seed_source(&target);
        let staged = restore_stage(&package, &target).unwrap();
        let other_package = make_package(
            &root.path().join("other-source"),
            &root.path().join("backups"),
            "other",
        );
        let other_stage = restore_stage(&other_package, &target).unwrap();
        arm_preview(&target, &staged).unwrap();
        let marker = target.join("pending-restore.json");
        let original_marker = std::fs::read(&marker).unwrap();
        assert!(matches!(
            arm_preview(&target, &other_stage),
            Err(AppError::Restore {
                code: CODE_PENDING,
                ..
            })
        ));
        assert_eq!(std::fs::read(&marker).unwrap(), original_marker);
        let original_db = std::fs::read(Path::new(&staged.staging_dir).join(ENTRY_DB)).unwrap();
        assert!(matches!(
            restore_discard(&target, &staged.backup_id, &staged.staging_token),
            Err(AppError::Restore {
                code: CODE_PENDING,
                ..
            })
        ));
        assert!(matches!(
            restore_stage(&package, &target),
            Err(AppError::Restore {
                code: CODE_PENDING,
                ..
            })
        ));
        // 标记不能解析也不能被当成「未确认」，以免破坏崩溃后的恢复现场。
        std::fs::write(target.join("pending-restore.json"), b"{broken").unwrap();
        assert!(matches!(
            restore_discard(&target, &staged.backup_id, &staged.staging_token),
            Err(AppError::Restore {
                code: super::super::swap::CODE_MARKER_CORRUPT,
                ..
            })
        ));
        assert_eq!(
            std::fs::read(Path::new(&staged.staging_dir).join(ENTRY_DB)).unwrap(),
            original_db
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_discard_keeps_ownership_after_cleanup_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let package = make_package(
            &root.path().join("source"),
            &root.path().join("backups"),
            "retry",
        );
        let target = root.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        let staged = restore_stage(&package, &target).unwrap();
        let staging = Path::new(&staged.staging_dir);
        let locked_path = staging.join("zz-locked.txt");
        std::fs::write(&locked_path, b"busy").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_path)
            .unwrap();
        assert!(matches!(
            restore_discard(&target, &staged.backup_id, &staged.staging_token),
            Err(AppError::Restore { code: CODE_IO, .. })
        ));
        assert_eq!(
            std::fs::read_to_string(staging.join(STAGING_TOKEN_FILE)).unwrap(),
            staged.staging_token
        );
        assert!(locked_path.is_file());
        drop(locked);
        restore_discard(&target, &staged.backup_id, &staged.staging_token).unwrap();
        assert!(!staging.exists());
    }

    #[test]
    fn restore_operations_respect_cross_process_lock() {
        const CHILD_DIR: &str = "SCROLLERY_RESTORE_LOCK_TEST_DIR";
        if let Some(target) = std::env::var_os(CHILD_DIR) {
            let target = PathBuf::from(target);
            let busy = |result: Result<()>| {
                assert!(matches!(
                    result,
                    Err(AppError::Restore {
                        code: super::super::core::CODE_JOB_BUSY,
                        ..
                    })
                ))
            };
            busy(restore_discard(
                &target,
                "locked",
                "11111111111111111111111111111111",
            ));
            busy(restore_stage(Path::new("unused"), &target).map(|_| ()));
            busy(super::super::arm_restore(
                &target,
                "locked",
                Path::new("unused"),
                "11111111111111111111111111111111",
                "rb",
                "unused",
                "unused",
                "0.1.0",
                &CancellationToken::new(),
            ));
            busy(super::super::perform_swap_at_boot(&target).map(|_| ()));
            busy(super::super::rollback_restore_at_boot(&target, "locked"));
            super::super::finalize_restore_verified(&target, "locked");
            assert!(target
                .join("restore-staging/locked/db/scrollery.db")
                .is_file());
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let package = make_package(
            &root.path().join("source"),
            &root.path().join("backups"),
            "locked",
        );
        let target = root.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        let staged = restore_stage(&package, &target).unwrap();
        let lock = acquire_restore_lock(&target).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backup::restore::tests::restore_operations_respect_cross_process_lock",
                "--nocapture",
            ])
            .env(CHILD_DIR, &target)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        drop(lock);
        restore_discard(&target, &staged.backup_id, &staged.staging_token).unwrap();
        assert!(!Path::new(&staged.staging_dir).exists());
    }

    #[test]
    fn restore_discard_rejects_path_escape() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(outside.join("owned")).unwrap();
        let protected = outside.join("owned/keep.txt");
        std::fs::write(&protected, b"outside").unwrap();
        assert!(matches!(
            restore_discard(&target, "../../outside", "11111111111111111111111111111111"),
            Err(AppError::Restore {
                code: CODE_PATH_INVALID,
                ..
            })
        ));
        let link = target.join("restore-staging");
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(matches!(
            restore_discard(&target, "owned", "11111111111111111111111111111111"),
            Err(AppError::Restore {
                code: CODE_PATH_INVALID,
                ..
            })
        ));
        assert_eq!(std::fs::read(&protected).unwrap(), b"outside");
        // 只移除测试创建的链接本身，再让 TempDir 回收本用例目录。
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
    }

    /// 篡改:包内 DB 字节被改一位 → SHA-256 不符 → restore_corrupt,且 staging 已清(RAII)。
    #[test]
    fn restore_stage_rejects_tampered_payload() {
        let root = unique_dir("tamper");
        let src_app = root.join("appdata");
        let dest = root.join("backups");
        let pkg = make_package(&src_app, &dest, "bk-tamper-1");

        // 重建一个包:解出 db、改一位、重打包(简单起见直接在 zip 字节层翻转一处非头部字节)。
        let bytes = std::fs::read(&pkg).unwrap();
        let mut tampered = bytes.clone();
        // 翻转靠后的一个字节(避开 zip 头/中央目录起始),大概率落在 deflate 数据区。
        let idx = tampered.len() / 2;
        tampered[idx] ^= 0xFF;
        let tpkg = dest.join("tampered.scrollerybackup");
        std::fs::write(&tpkg, &tampered).unwrap();

        let tgt = root.join("target");
        std::fs::create_dir_all(&tgt).unwrap();
        let e = restore_stage(&tpkg, &tgt);
        // 篡改可能表现为 zip 解压失败或 sha256 不符,二者都归 restore_corrupt。
        match e {
            Err(AppError::Restore { code, .. }) => assert!(
                code == CODE_CORRUPT || code == CODE_IO,
                "篡改应判 corrupt/io,得 {code}"
            ),
            Ok(_) => panic!("篡改的包不应通过校验"),
            Err(other) => panic!("期望 Restore,得 {other:?}"),
        }
        // staging 不得残留(RAII 清理)。
        assert!(!tgt.join("restore-staging").join("bk-tamper-1").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// zip-slip:手工造一个含 `../evil.txt` 条目的假包 → restore_path_invalid(名字校验拦截)。
    #[test]
    fn restore_stage_rejects_zip_slip_entry() {
        let root = unique_dir("zipslip");
        let tgt = root.join("target");
        std::fs::create_dir_all(&tgt).unwrap();

        // 造一个 manifest 声明 db + 一个越界 payload 的假包。
        let evil_path = "../evil.txt";
        let manifest = format!(
            r#"{{"formatVersion":1,"backupId":"evil","kind":"manual","appVersion":"0.1.0",
            "schemaVersion":21,"createdAtUtc":"2026-07-19T00:00:00Z","roots":[],
            "counts":{{"items":0,"albums":0,"tags":0,"namedPersons":0}},"externalDocumentVersions":0,
            "payload":[{{"path":"{evil_path}","bytes":4,"sha256":"00"}}]}}"#
        );
        let pkg = root.join("evil.scrollerybackup");
        {
            let f = File::create(&pkg).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(ENTRY_MANIFEST, opts).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.start_file(evil_path, opts).unwrap();
            zip.write_all(b"evil").unwrap();
            zip.finish().unwrap();
        }

        let e = restore_stage(&pkg, &tgt);
        match e {
            Err(AppError::Restore { code, .. }) => {
                assert_eq!(code, CODE_PATH_INVALID, "zip-slip 应判 path_invalid")
            }
            other => panic!("期望 restore_path_invalid,得 {other:?}"),
        }
        // 越界文件绝不落到 target 之外。
        assert!(!root.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 备份库格式非当前:过旧与过新走**同一条**拒绝路径 → 统一 restore_schema_incompatible,
    /// 且拒绝路径绝不改动暂存库(不「修好」标识、不建结构)。直接单测暂存库校验函数。
    #[test]
    fn staged_db_other_format_is_rejected() {
        for bogus in [
            crate::db::schema::SCHEMA_VERSION + 1,
            crate::db::schema::SCHEMA_VERSION - 1,
        ] {
            let root = unique_dir("otherfmt");
            let db = root.join("scrollery.db");
            let conn = Connection::open(&db).unwrap();
            crate::db::schema::initialize_schema(&conn).unwrap();
            conn.execute(
                "UPDATE app_config SET value=?1 WHERE key='schema_version'",
                params![bogus.to_string()],
            )
            .unwrap();
            drop(conn);

            match validate_staged_db(&db) {
                Err(AppError::Restore { code, .. }) => {
                    assert_eq!(code, CODE_SCHEMA_INCOMPATIBLE, "标识 {bogus}")
                }
                other => panic!("期望 restore_schema_incompatible(标识 {bogus}),得 {other:?}"),
            }
            // 拒绝路径不得「修好」标识,也不得建立任何结构。
            let conn = Connection::open(&db).unwrap();
            assert_eq!(crate::db::schema::read_schema_version(&conn), bogus);
            drop(conn);
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
