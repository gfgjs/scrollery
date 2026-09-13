//! 恢复暂存与校验(方案 B §6.1)。把备份包当**不可信输入**:白名单 + zip-slip/符号链接拒绝 +
//! checked 尺寸(防 zip bomb)+ 逐条 SHA-256 → 解到 appdata 同卷 `restore-staging/{backupId}/` →
//! 暂存库 quick_check/foreign_key_check/schema 门 + 老版迁移 → §3.2 abs_path 跨机 rebase →
//! 返回摘要供双确认。**本模块只暂存校验,不动活库**(启动交换在 §6.2,阶段 3b)。

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
pub const CODE_SCHEMA_TOO_NEW: &str = "restore_schema_too_new";
pub const CODE_CORRUPT: &str = "restore_corrupt";
pub const CODE_PATH_INVALID: &str = "restore_path_invalid";
pub const CODE_SIZE_LIMIT: &str = "restore_size_limit";
pub const CODE_DOCUMENT_MISSING: &str = "restore_document_missing";
pub const CODE_IO: &str = "restore_io";

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
    /// 暂存目录(供 §6.2 arm/swap 阶段用)。
    pub staging_dir: String,
    /// 暂存库(迁移后)的 schema 版本(== runtime CURRENT_VERSION)。
    pub schema_version: u32,
    /// 包内 schema 老于 runtime,已在暂存副本上迁移。
    pub needs_migration: bool,
    pub kind: String,
    pub created_at_utc: String,
    pub counts: Counts,
    pub roots: Vec<RootEntry>,
    pub external_document_versions: i64,
    /// rebase 校验通过的 appdata 文档版本数。
    pub appdata_document_count: usize,
}

/// 暂存目录 RAII 清理:失败即回收半装产物,不留残迹。成功后 `disarm` 保留(供交换阶段用)。
struct StagingGuard(Option<PathBuf>);
impl StagingGuard {
    fn disarm(mut self) {
        self.0 = None;
    }
}
impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// 校验并暂存一个备份包(方案 §6.1)。`target_app_data_dir` = 本机(目标)应用数据根:
/// 解压落它下 `restore-staging/{backupId}/`,并把 appdata 文档路径 rebase 到它下(§3.2 跨机)。
pub fn restore_stage(
    package_path: &Path,
    target_app_data_dir: &Path,
) -> Result<RestoreStageResult> {
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
    let staging = target_app_data_dir
        .join("restore-staging")
        .join(&manifest.backup_id);
    let _ = std::fs::remove_dir_all(&staging); // 清前次残留
    let cleanup = StagingGuard(Some(staging.clone()));
    extract_and_verify(&mut archive, &manifest, &staging)?;

    // 4. 暂存库校验(quick_check/foreign_key_check/schema 门)+ 老版迁移。
    let staged_db = staging.join(ENTRY_DB);
    let (schema_version, needs_migration) = validate_and_migrate_staged_db(&staged_db)?;

    // 5. §3.2 abs_path 跨机 rebase + 每行版本文件在包内交叉核对。
    let appdata_document_count =
        rebase_appdata_documents(&staged_db, target_app_data_dir, &staging)?;

    // 6. 摘要(从暂存库读)。
    let sconn = Connection::open(&staged_db).map_err(db_err)?;
    let counts = read_counts(&sconn)?;
    let roots = read_roots(&sconn)?;
    let external_document_versions = read_external_count(&sconn)?;
    drop(sconn);

    cleanup.disarm(); // 成功:保留 staging
    Ok(RestoreStageResult {
        backup_id: manifest.backup_id,
        staging_dir: staging.to_string_lossy().to_string(),
        schema_version,
        needs_migration,
        kind: manifest.kind.file_infix().to_string(),
        created_at_utc: manifest.created_at_utc,
        counts,
        roots,
        external_document_versions,
        appdata_document_count,
    })
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

/// 暂存库校验 + 老版迁移(方案 §6.1)。返回 (迁移后版本, 是否迁移过)。
/// 包内版本新于本二进制 → `restore_schema_too_new`;老于 → 在暂存副本迁移后再 quick/fk check。
///
/// TODO(审查 #10,加固,待独立处理):此处对**不可信**暂存库直接跑 `run_migrations`。恶意包可携
/// 触发器/视图,在迁移 DML 期间执行攻击者 SQL(quick_check/fk_check 不覆盖此面)。稳健加固(受限
/// 连接 DEFENSIVE/TRUSTED_SCHEMA、迁移前剥离非规范触发器/视图、或 schema 指纹比对)落在迁移关键
/// 路径,可能破坏自家迁移或 FTS 影子表,须专门跑全迁移套件验证——故本轮不硬塞,留此显式记号。
/// 利用门槛较高(用户须主动选攻击者的包)。
fn validate_and_migrate_staged_db(staged_db: &Path) -> Result<(u32, bool)> {
    let conn = Connection::open(staged_db).map_err(db_err)?;
    quick_check(&conn)?;
    foreign_key_check(&conn)?;
    let pkg_version = crate::db::migration::read_schema_version(&conn);
    let runtime = crate::db::migration::current_schema_version();
    if pkg_version > runtime {
        return Err(err(CODE_SCHEMA_TOO_NEW, "备份 schema 新于本程序"));
    }
    let needs_migration = pkg_version < runtime;
    if needs_migration {
        crate::db::migration::run_migrations(&conn).map_err(|_| corrupt("暂存库迁移失败"))?;
        quick_check(&conn)?;
        foreign_key_check(&conn)?;
    }
    let final_version = crate::db::migration::read_schema_version(&conn);
    Ok((final_version, needs_migration))
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
        crate::db::migration::run_migrations(&conn).unwrap();
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

    /// 跨机 rebase(§3.2 fixture 硬要求:源/目标 appdata 路径不同):恢复暂存后,暂存库内
    /// appdata 版本的 abs_path 指向**目标机** documents,不是源机路径;文件已解压到 staging。
    #[test]
    fn restore_stage_rebases_appdata_paths_cross_machine() {
        let root = unique_dir("rebase");
        let src_app = root.join("machineA/appdata");
        let dest = root.join("backups");
        let pkg = make_package(&src_app, &dest, "bk-rebase-1");

        // 目标机 appdata 路径与源不同。
        let tgt_app = root.join("machineB/appdata");
        std::fs::create_dir_all(&tgt_app).unwrap();

        let res = restore_stage(&pkg, &tgt_app).expect("restore_stage 应成功");
        assert_eq!(res.backup_id, "bk-rebase-1");
        assert_eq!(
            res.schema_version,
            crate::db::migration::current_schema_version()
        );
        assert!(!res.needs_migration, "同版本包无需迁移");
        assert_eq!(res.appdata_document_count, 1);
        assert_eq!(res.roots.len(), 1);

        // 暂存库内 abs_path 已 rebase 到目标机路径。
        let staged_db = PathBuf::from(&res.staging_dir).join("db/scrollery.db");
        let conn = Connection::open(&staged_db).unwrap();
        let abs: String = conn
            .query_row(
                "SELECT abs_path FROM document_versions WHERE id=100",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let expected = tgt_app.join("documents").join("42").join("100.txt");
        assert_eq!(
            PathBuf::from(&abs),
            expected,
            "abs_path 须 rebase 到目标机 documents,不留源机前缀"
        );
        // 源机路径绝不残留。
        assert!(!abs.contains("machineA"), "rebase 后不得残留源机路径:{abs}");
        // 版本文件已解压到 staging。
        assert!(PathBuf::from(&res.staging_dir)
            .join("documents/42/100.txt")
            .is_file());
        let _ = std::fs::remove_dir_all(&root);
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

    /// #7 跨平台 basename:Windows 反斜杠路径、Unix 正斜杠路径、纯文件名都取到末段;
    /// 尾随分隔符/空串 → None。锁死跨机(Windows↔macOS)恢复不依赖平台 Path 语义。
    #[test]
    fn basename_cross_platform_handles_both_separators() {
        assert_eq!(
            basename_cross_platform(r"C:\Users\x\documents\42\100.txt"),
            Some("100.txt")
        );
        assert_eq!(
            basename_cross_platform("/home/x/documents/42/100.txt"),
            Some("100.txt")
        );
        assert_eq!(basename_cross_platform("100.txt"), Some("100.txt"));
        // 混合分隔符(源机路径经某些序列化后可能混用)也取最后一段。
        assert_eq!(basename_cross_platform(r"a/b\c/d.txt"), Some("d.txt"));
        assert_eq!(basename_cross_platform("a/b/"), None);
        assert_eq!(basename_cross_platform(""), None);
    }

    /// #1 backup_id 单段安全校验:接受随机 hex,拒穿越/绝对/盘符/多段/反斜杠。
    #[test]
    fn safe_backup_id_rejects_traversal_and_absolute() {
        assert!(is_safe_backup_id("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"));
        assert!(is_safe_backup_id("bk-2026"));
        assert!(!is_safe_backup_id("../evil"));
        assert!(!is_safe_backup_id("a/b"));
        assert!(!is_safe_backup_id("/abs"));
        assert!(!is_safe_backup_id(r"C:\Users\x"));
        assert!(!is_safe_backup_id("C:/Users/x"));
        assert!(!is_safe_backup_id(".."));
        assert!(!is_safe_backup_id(""));
    }

    /// #1 端到端:manifest.backupId 含路径穿越 → restore_stage 判 restore_path_invalid,
    /// 且不在 appdata 外制造/删除任何目录(remove_dir_all/解压落点均被拦在校验前)。
    #[test]
    fn restore_stage_rejects_malicious_backup_id() {
        let root = unique_dir("badid");
        let tgt = root.join("target");
        std::fs::create_dir_all(&tgt).unwrap();

        // 造一个 backupId 越界、payload 仅 db 的假包(校验应在解压/清理前拦下)。
        let manifest = r#"{"formatVersion":1,"backupId":"../../escape","kind":"manual",
            "appVersion":"0.1.0","schemaVersion":21,"createdAtUtc":"2026-07-19T00:00:00Z","roots":[],
            "counts":{"items":0,"albums":0,"tags":0,"namedPersons":0},"externalDocumentVersions":0,
            "payload":[{"path":"db/scrollery.db","bytes":4,"sha256":"00"}]}"#;
        let pkg = root.join("badid.scrollerybackup");
        {
            let f = File::create(&pkg).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file(ENTRY_MANIFEST, opts).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        match restore_stage(&pkg, &tgt) {
            Err(AppError::Restore { code, .. }) => {
                assert_eq!(code, CODE_PATH_INVALID, "越界 backupId 应判 path_invalid")
            }
            other => panic!("期望 restore_path_invalid,得 {other:?}"),
        }
        // 越界目录绝不被创建(escape 应落在 target 之外的 root 下)。
        assert!(!root.join("escape").exists());
        assert!(!root.join("restore-staging").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// schema 新于本二进制 → restore_schema_too_new(直接单测暂存库校验函数)。
    #[test]
    fn staged_db_newer_schema_is_rejected() {
        let root = unique_dir("toonew");
        let db = root.join("scrollery.db");
        let conn = Connection::open(&db).unwrap();
        crate::db::migration::run_migrations(&conn).unwrap();
        // 把版本改成 runtime+1(伪装成更新的库)。
        let too_new = crate::db::migration::current_schema_version() + 1;
        conn.execute(
            "UPDATE app_config SET value=?1 WHERE key='schema_version'",
            params![too_new.to_string()],
        )
        .unwrap();
        drop(conn);

        let e = validate_and_migrate_staged_db(&db);
        match e {
            Err(AppError::Restore { code, .. }) => assert_eq!(code, CODE_SCHEMA_TOO_NEW),
            other => panic!("期望 restore_schema_too_new,得 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
