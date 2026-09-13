//! 备份引擎(方案 B §5)。**纯函数**:时间戳 / backup_id / retention 由调用方注入,
//! 便于单测确定性。**须在持 `AppState.document_storage_guard` write guard 的 blocking 上下文
//! 调用**(方案 §3.1:VACUUM 快照与 documents 文件须来自同一逻辑时点)。
//!
//! 流程:preflight(可写)→ VACUUM INTO 一致快照 → quick_check + 计数/根/schema 版本读 →
//! 文档一致性逐行校验(§3.1)→ zip 流式打包(逐条未压缩 SHA-256)→ manifest 最后写 →
//! `*.tmp` 同卷 rename 为正式包 → 仅成功后对**自动包**做 retention(B-6)。

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use xxhash_rust::xxh3::Xxh3;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::manifest::{
    BackupKind, Counts, Manifest, PayloadEntry, RootEntry, BACKUP_FILE_EXT, BACKUP_FORMAT_VERSION,
    ENTRY_DB, ENTRY_DOCUMENTS_PREFIX, ENTRY_MANIFEST, MAX_MANIFEST_BYTES,
};
use crate::error::{AppError, Result};

// ── 稳定错误码(方案 §9;透到 IPC `code` 字段供前端分流)────────────────────────────
pub const CODE_DIR_UNSET: &str = "backup_dir_unset";
pub const CODE_DIR_NOT_WRITABLE: &str = "backup_dir_not_writable";
pub const CODE_DOC_INCONSISTENT: &str = "backup_document_inconsistent";
pub const CODE_CANCELLED: &str = "backup_cancelled";
pub const CODE_IO: &str = "backup_io";
pub const CODE_JOB_BUSY: &str = "file_job_busy";

fn err(code: &'static str, msg: &str) -> AppError {
    // message 一律固定文案,**不携带绝对路径 / SQL / 内部错误串**(泄漏面,方案 §9)。
    AppError::Backup {
        code,
        message: msg.to_string(),
    }
}
fn cancelled() -> AppError {
    err(CODE_CANCELLED, "备份已取消 | backup cancelled")
}
fn inconsistent(msg: &str) -> AppError {
    err(CODE_DOC_INCONSISTENT, msg)
}
/// DB 错误统一收敛为 backup_io(不泄漏 SQL / rusqlite 内部串)。
fn db_io(_e: rusqlite::Error) -> AppError {
    err(CODE_IO, "备份读取数据库快照失败 | backup db read failed")
}

/// 工作目录 RAII 清理:任何早退(含 `?`)都回收 VACUUM 暂存库,不留残迹。
struct DirCleanup(PathBuf);
impl Drop for DirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 一个通过校验、可入包的 appdata 文档版本。
struct ValidatedDoc {
    item_id: i64,
    /// 规范化后的绝对路径(本机源盘,用于读取入包)。
    abs_path: PathBuf,
    /// 文件名(入包相对路径 `documents/{item_id}/{file_name}` 用,§3.2 去机器绝对前缀)。
    file_name: String,
    /// 期望的内容 xxh3(与 save_version 写入一致)。打包时**单遍读**同算 sha256+xxh3,以此比对
    /// (审查 #14:避免 validate 再整读一遍算 xxh3 —— 双读 + 无界内存)。`None`=行无 content_hash,跳过。
    content_hash: Option<String>,
}

/// manifest 除 payload 外的全部字段(payload 在打包时逐条累积)。
pub struct ManifestMeta {
    pub backup_id: String,
    pub kind: BackupKind,
    pub app_version: String,
    pub schema_version: u32,
    pub created_at_utc: String,
    pub roots: Vec<RootEntry>,
    pub counts: Counts,
    pub external_document_versions: i64,
}

impl ManifestMeta {
    fn into_manifest(self, payload: Vec<PayloadEntry>) -> Manifest {
        Manifest {
            format_version: BACKUP_FORMAT_VERSION,
            backup_id: self.backup_id,
            kind: self.kind,
            app_version: self.app_version,
            schema_version: self.schema_version,
            created_at_utc: self.created_at_utc,
            roots: self.roots,
            counts: self.counts,
            external_document_versions: self.external_document_versions,
            payload,
        }
    }
}

/// 备份入参(调用方注入,便于确定性单测)。
pub struct BackupParams<'a> {
    /// 源(活)DB 路径,VACUUM INTO 从此读一致快照。
    pub source_db_path: &'a Path,
    /// 应用数据根(`documents/` 子目录据此定位、受控目录边界校验用)。
    pub app_data_dir: &'a Path,
    /// 备份目的目录(正式包与 `*.tmp` 同卷,rename 落名)。
    pub dest_dir: &'a Path,
    pub kind: BackupKind,
    /// 应用版本(CARGO_PKG_VERSION)。
    pub app_version: String,
    /// 稳定备份标识(调用方生成;暂存目录命名用)。
    pub backup_id: String,
    /// 产包 UTC 时刻 RFC3339(调用方 `chrono::Utc::now()`)。
    pub created_at_utc: String,
    /// 文件名时间戳 `YYYYMMDD-HHmmss`(调用方生成)。
    pub timestamp_label: String,
    /// 自动包保留份数;`Some(n)` 且 `kind==Auto` 时成功产包后轮转(B-6:仅碰 auto 包)。
    pub auto_retention: Option<usize>,
}

/// 备份产出。
#[derive(Debug)]
pub struct BackupOutcome {
    /// 正式包绝对路径。
    pub path: PathBuf,
    /// 包体字节数。
    pub bytes: u64,
    pub backup_id: String,
}

/// 执行一次完整备份(方案 §5)。见模块文档的前置不变量。
pub fn run_backup(p: &BackupParams, cancel: &CancellationToken) -> Result<BackupOutcome> {
    // 0. preflight:目的地存在且可写(尝试写一个探针文件)。
    ensure_dest_writable(p.dest_dir)?;

    // 1. 工作目录 + VACUUM INTO 暂存库(独立连接 + 同 PRAGMA,参数绑定路径)。
    // 放 **app_data_dir**(本地源卷)而非 dest_dir:dest 可能是慢速 USB/NAS,VACUUM 出的完整 catalog
    // 若写到那儿,还要再读回 hash+deflate、再写 tmp——三次全量过慢链,且全程持文档一致性 write guard
    // 阻塞文档保存。只有最终 tmp→正式包的原子 rename 须与正式包同卷(见第 5 步),VACUUM 暂存不必
    // (审查 #15)。
    let work_dir = p
        .app_data_dir
        .join(format!(".scrollery-backup-work-{}", p.backup_id));
    let _ = std::fs::remove_dir_all(&work_dir); // 清前次崩溃残留同名工作目录
    std::fs::create_dir_all(&work_dir)
        .map_err(|_| err(CODE_DIR_NOT_WRITABLE, "无法创建备份工作目录"))?;
    let _work_cleanup = DirCleanup(work_dir.clone());
    let staging_db = work_dir.join("scrollery.db");
    vacuum_into(p.source_db_path, &staging_db)?;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }

    // 2. 打开暂存快照:quick_check + 读 schema 版本 / 计数 / 根 / external 数(查询共用 dbread)。
    let sconn = Connection::open(&staging_db).map_err(db_io)?;
    if !super::dbread::integrity_ok(&sconn).map_err(db_io)? {
        return Err(err(CODE_IO, "备份快照完整性校验未通过"));
    }
    let schema_version = crate::db::migration::read_schema_version(&sconn);
    let counts = super::dbread::read_counts(&sconn).map_err(db_io)?;
    let roots = super::dbread::read_roots(&sconn).map_err(db_io)?;
    let external_document_versions = super::dbread::read_external_count(&sconn).map_err(db_io)?;

    // 3. 文档一致性逐行校验(§3.1):路径非空 / 文件存在 / 位于受控 documents/ 下。content_hash 比对
    //    下沉到第 4 步打包的**单遍读**(add_file_streaming 同算 sha256+xxh3),此处 verify_hash=false
    //    只做路径/存在/受控目录门,避免再整读一遍文件(审查 #14:双读 + 无界内存)。
    let docs = validate_appdata_documents(&sconn, p.app_data_dir, false)?;
    drop(sconn);
    if cancel.is_cancelled() {
        return Err(cancelled());
    }

    // 4. zip 流式打包到 `*.tmp`(db → documents → manifest 最后写)。
    let final_name = format!(
        "Scrollery-{}-{}.{}",
        p.kind.file_infix(),
        p.timestamp_label,
        BACKUP_FILE_EXT
    );
    let final_path = p.dest_dir.join(&final_name);
    let tmp_path = p.dest_dir.join(format!("{final_name}.tmp"));
    let _ = std::fs::remove_file(&tmp_path); // 清残留 tmp

    let meta = ManifestMeta {
        backup_id: p.backup_id.clone(),
        kind: p.kind,
        app_version: p.app_version.clone(),
        schema_version,
        created_at_utc: p.created_at_utc.clone(),
        roots,
        counts,
        external_document_versions,
    };
    if let Err(e) = write_package(&tmp_path, &staging_db, &docs, meta, cancel) {
        let _ = std::fs::remove_file(&tmp_path); // 失败/取消:清 tmp,不留半截包
        return Err(e);
    }
    if cancel.is_cancelled() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(cancelled());
    }

    // 5. 同卷 rename → 正式包(原子落名;此后视为已提交,不再取消)。
    std::fs::rename(&tmp_path, &final_path).map_err(|_| err(CODE_IO, "备份改名落盘失败"))?;
    let bytes = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

    // 6. retention:仅对自动包、仅成功产包后(B-6)。删失败不阻断。
    if p.kind == BackupKind::Auto {
        if let Some(keep) = p.auto_retention {
            apply_auto_retention(p.dest_dir, keep);
        }
    }

    Ok(BackupOutcome {
        path: final_path,
        bytes,
        backup_id: p.backup_id.clone(),
    })
}

/// preflight:目的地可写(写-删探针文件)。不可写返回稳定码。
fn ensure_dest_writable(dest_dir: &Path) -> Result<()> {
    if !dest_dir.is_dir() {
        return Err(err(CODE_DIR_NOT_WRITABLE, "备份目的地不存在或不是目录"));
    }
    let probe = dest_dir.join(".scrollery-backup-writable-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(err(CODE_DIR_NOT_WRITABLE, "备份目的地不可写")),
    }
}

/// VACUUM INTO 一致快照(方案 §5.3):独立连接 + 同生产 PRAGMA,**参数绑定**目标路径。
/// synchronous=NORMAL 下 SQLite 在命令成功返回前同步输出;此后再入 zip。
fn vacuum_into(source_db_path: &Path, staging_db: &Path) -> Result<()> {
    let conn = Connection::open(source_db_path).map_err(db_io)?;
    crate::db::connection::apply_pragmas(&conn)?; // 同生产 PRAGMA(单一事实源)
                                                  // 自定义 collation 须与生产建连三处(写连接/读池/迁移)一致注册:VACUUM INTO 会重建全部索引,
                                                  // 若日后有 `COLLATE NATURAL_CMP` 索引,缺注册的连接会 "no such collation sequence" 致备份全断
                                                  // (且被稳定码 backup_io 遮蔽)。现无此类索引故为潜伏防线,但与其余三处保持单一姿态。
    crate::db::register_custom_collations(&conn).map_err(AppError::from)?;
    let staging_str = staging_db
        .to_str()
        .ok_or_else(|| err(CODE_IO, "备份暂存路径非法"))?;
    conn.execute("VACUUM INTO ?1", params![staging_str])
        .map_err(db_io)?;
    Ok(())
}

/// 流式(64K)算文件 xxh3,不整读进内存(审查 #14:大版本文件避免无界分配)。
fn hash_file_xxh3(path: &Path) -> Result<String> {
    let mut f = File::open(path).map_err(|_| inconsistent("版本文件读取失败"))?;
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|_| inconsistent("版本文件读取失败"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}

/// 逐行校验 `storage='appdata'` 文档版本(方案 §3.1),返回可入包的清单。任一失败返回
/// `backup_document_inconsistent`,不产「成功」包。
///
/// `verify_hash`:是否在此就地校验 content_hash。preflight(`documents_consistent`)传 `true`
/// (报告用,须独立于打包);`run_backup` 传 `false`——内容 xxh3 校验下沉到打包的单遍读,避免
/// 二次整读(审查 #14)。无论真假,期望 content_hash 都随 `ValidatedDoc` 带出供打包比对。
fn validate_appdata_documents(
    conn: &Connection,
    app_data_dir: &Path,
    verify_hash: bool,
) -> Result<Vec<ValidatedDoc>> {
    let docs_root = app_data_dir.join("documents");
    let mut stmt = conn
        .prepare(
            "SELECT item_id, abs_path, content_hash FROM document_versions \
             WHERE storage='appdata' ORDER BY item_id, id",
        )
        .map_err(db_io)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db_io)?;

    let mut out = Vec::new();
    for row in rows {
        let (item_id, abs_path, content_hash) = row.map_err(db_io)?;
        if abs_path.is_empty() {
            return Err(inconsistent("存在版本行的文件路径为空"));
        }
        let p = PathBuf::from(&abs_path);
        let meta = std::fs::metadata(&p).map_err(|_| inconsistent("版本文件缺失"))?;
        if !meta.is_file() {
            return Err(inconsistent("版本路径不是文件"));
        }
        // 受控目录边界(拒符号链接/`..`逃逸):canonicalize 后须位于 documents/ 之下。
        let canon = dunce::canonicalize(&p).map_err(|_| inconsistent("版本文件无法规范化"))?;
        let root_canon =
            dunce::canonicalize(&docs_root).map_err(|_| inconsistent("documents 目录缺失"))?;
        if !canon.starts_with(&root_canon) {
            return Err(inconsistent("版本文件越出受控 documents 目录"));
        }
        // content_hash 非空时:preflight 就地流式校验(报告用);备份路径(verify_hash=false)
        // 把校验交给打包单遍读。期望值随 ValidatedDoc 带出。
        let expected_hash = content_hash.filter(|s| !s.is_empty());
        if verify_hash {
            if let Some(h) = expected_hash.as_deref() {
                let actual = hash_file_xxh3(&canon)?;
                if actual != h {
                    return Err(inconsistent("版本文件内容校验不符"));
                }
            }
        }
        let file_name = canon
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| inconsistent("版本文件名非法"))?
            .to_string();
        out.push(ValidatedDoc {
            item_id,
            abs_path: canon,
            file_name,
            content_hash: expected_hash,
        });
    }
    Ok(out)
}

/// preflight 用:检查 `storage='appdata'` 文档版本是否全部一致(方案 §5.1)。全一致返回 `true`,
/// 任一行不一致返回 `false`(不 Err——preflight 是报告而非硬失败;真正的硬门在 `run_backup`)。
/// 传入活库只读连接。
pub fn documents_consistent(conn: &Connection, app_data_dir: &Path) -> bool {
    // preflight 是独立报告(不打包),故 verify_hash=true 就地流式校验 content_hash。
    validate_appdata_documents(conn, app_data_dir, true).is_ok()
}

/// preflight 用:目的地是否可写(写-删探针)。暴露 `run_backup` 的同款检查供预检复用。
pub fn dest_writable(dest_dir: &Path) -> bool {
    ensure_dest_writable(dest_dir).is_ok()
}

/// zip 打包到 `tmp_path`:db → 各 document → manifest 最后写。逐条流式计算未压缩 SHA-256
/// (读一遍源、边喂 hasher 边喂 deflate,大库不进内存驻留)。收尾 fsync 确保落盘。
fn write_package(
    tmp_path: &Path,
    staging_db: &Path,
    docs: &[ValidatedDoc],
    meta: ManifestMeta,
    cancel: &CancellationToken,
) -> Result<()> {
    let file =
        File::create(tmp_path).map_err(|_| err(CODE_DIR_NOT_WRITABLE, "无法创建备份临时文件"))?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    // large_file(true):大库 DB 可能超 4GB,强制 ZIP64 局部头(自有 restore 读取,无兼容负担)。
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);

    let mut payload = Vec::with_capacity(docs.len() + 1);
    // DB 条目无 content_hash → 不比对(None)。
    payload.push(add_file_streaming(
        &mut zip, ENTRY_DB, staging_db, opts, None,
    )?);
    for d in docs {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let entry_name = format!("{}{}/{}", ENTRY_DOCUMENTS_PREFIX, d.item_id, d.file_name);
        // 单遍读同算 sha256(入 manifest)+ xxh3(比对期望 content_hash,§#14 融合校验)。
        payload.push(add_file_streaming(
            &mut zip,
            &entry_name,
            &d.abs_path,
            opts,
            d.content_hash.as_deref(),
        )?);
    }

    // manifest 最后写(含全部 payload 的 sha256)。
    let manifest = meta.into_manifest(payload);
    let mbytes =
        serde_json::to_vec_pretty(&manifest).map_err(|_| err(CODE_IO, "manifest 序列化失败"))?;
    zip.start_file(ENTRY_MANIFEST, opts)
        .map_err(|_| err(CODE_IO, "写 manifest 失败"))?;
    zip.write_all(&mbytes)
        .map_err(|_| err(CODE_IO, "写 manifest 失败"))?;

    let inner = zip.finish().map_err(|_| err(CODE_IO, "zip 收尾失败"))?;
    let f = inner
        .into_inner()
        .map_err(|_| err(CODE_IO, "备份缓冲刷新失败"))?;
    f.sync_all()
        .map_err(|_| err(CODE_IO, "备份落盘 fsync 失败"))?;
    Ok(())
}

/// 把一个文件**单遍**流式写入 zip 条目,同时计算未压缩内容的 SHA-256(入 manifest;zip CRC 不能
/// 替代,§4)。`expected_xxh3` 非空时,顺便同算 xxh3 并与之比对(§#14:content_hash 校验融进这一遍
/// 读,避免 validate 再整读一次)——不符即 `backup_document_inconsistent`。
fn add_file_streaming(
    zip: &mut ZipWriter<BufWriter<File>>,
    entry_name: &str,
    src_path: &Path,
    opts: SimpleFileOptions,
    expected_xxh3: Option<&str>,
) -> Result<PayloadEntry> {
    zip.start_file(entry_name, opts)
        .map_err(|_| err(CODE_IO, "写备份条目失败"))?;
    let mut f = File::open(src_path).map_err(|_| err(CODE_IO, "读取待备份文件失败"))?;
    let mut hasher = Sha256::new();
    let verify = expected_xxh3.filter(|s| !s.is_empty());
    let mut xh = verify.map(|_| Xxh3::new());
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|_| err(CODE_IO, "读取待备份文件失败"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(x) = xh.as_mut() {
            x.update(&buf[..n]);
        }
        zip.write_all(&buf[..n])
            .map_err(|_| err(CODE_IO, "写备份条目失败"))?;
        total += n as u64;
    }
    // content_hash 比对(§3.1 一致性硬门,§#14 融合):不符不产成功包(调用方清 tmp)。
    if let (Some(expected), Some(x)) = (verify, xh) {
        if format!("{:016x}", x.digest()) != expected {
            return Err(inconsistent("版本文件内容校验不符"));
        }
    }
    Ok(PayloadEntry {
        path: entry_name.to_string(),
        bytes: total,
        sha256: crate::utils::hash::to_hex_lower(&hasher.finalize()),
    })
}

/// 自动包 retention(B-6):仅删「文件名前缀 `Scrollery-auto-` + 扩展名 + manifest.kind==auto
/// 验证通过」的包;按文件名(内嵌时间戳=时间序)保留最新 `keep` 份。永不碰手动包 / 陌生文件 /
/// 无法解析的包。删失败不阻断(尽力而为)。
fn apply_auto_retention(dest_dir: &Path, keep: usize) {
    let Ok(read) = std::fs::read_dir(dest_dir) else {
        return;
    };
    let mut autos: Vec<PathBuf> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if is_auto_backup_filename(&path) && verify_is_auto_package(&path) {
            autos.push(path);
        }
    }
    autos.sort(); // 文件名前缀同 → 时间戳字典序 == 时间序(升序)
    autos.reverse(); // 最新在前
    for old in autos.into_iter().skip(keep) {
        let _ = std::fs::remove_file(&old);
    }
}

fn is_auto_backup_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("Scrollery-auto-") && name.ends_with(&format!(".{BACKUP_FILE_EXT}"))
}

/// 打开候选包读 `manifest.json`,确认 `kind==auto`。任何失败=不确认=不删(保守)。
fn verify_is_auto_package(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let Ok(entry) = archive.by_name(ENTRY_MANIFEST) else {
        return false;
    };
    if entry.size() > MAX_MANIFEST_BYTES {
        return false; // 敌意/损坏包声明超大 manifest → 不确认(保守不删),不 OOM
    }
    let mut s = String::new();
    if entry
        .take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut s)
        .is_err()
    {
        return false;
    }
    matches!(
        serde_json::from_str::<Manifest>(&s),
        Ok(m) if m.kind == BackupKind::Auto
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::manifest::Manifest;
    use xxhash_rust::xxh3::xxh3_64;

    /// 造一套源:app_data_dir 内的 scrollery.db(跑迁移)+ 一个扫描根 + 一个 appdata 文档版本
    /// (真文件 + content_hash)+ 一个 external 版本(不入包只计数)。返回 (db_path, app_data_dir)。
    fn seed_source(app_data: &Path) -> PathBuf {
        std::fs::create_dir_all(app_data).unwrap();
        let db_path = app_data.join("scrollery.db");
        let conn = Connection::open(&db_path).unwrap();
        crate::db::migration::run_migrations(&conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap(); // 免造 media_items 满列行

        conn.execute(
            "INSERT INTO scan_roots (id, path, alias, is_hidden) VALUES (1, ?1, '照片库', 0)",
            params![app_data.join("lib").to_string_lossy()],
        )
        .unwrap();

        let item_id = 42i64;
        let docdir = app_data.join("documents").join(item_id.to_string());
        std::fs::create_dir_all(&docdir).unwrap();
        let content = "版本正文内容";
        let vfile = docdir.join("100.txt");
        std::fs::write(&vfile, content).unwrap();
        let hash = format!("{:016x}", xxh3_64(content.as_bytes()));
        conn.execute(
            "INSERT INTO document_versions (id, item_id, storage, abs_path, source, content_hash) \
             VALUES (100, ?1, 'appdata', ?2, 'user', ?3)",
            params![item_id, vfile.to_string_lossy(), hash],
        )
        .unwrap();
        // external 版本:不入包,manifest 计数 +1。
        conn.execute(
            "INSERT INTO document_versions (id, item_id, storage, abs_path, source) \
             VALUES (200, ?1, 'external', 'D:/外部/无关.txt', 'user')",
            params![item_id],
        )
        .unwrap();
        db_path
    }

    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "scrollery_bktest_{}_{}_{}",
            std::process::id(),
            tag,
            seq
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn params<'a>(
        db: &'a Path,
        app_data: &'a Path,
        dest: &'a Path,
        kind: BackupKind,
        label: &str,
        retention: Option<usize>,
    ) -> BackupParams<'a> {
        BackupParams {
            source_db_path: db,
            app_data_dir: app_data,
            dest_dir: dest,
            kind,
            app_version: "0.1.0".into(),
            backup_id: format!("id-{label}"),
            created_at_utc: "2026-07-19T00:00:00Z".into(),
            timestamp_label: label.into(),
            auto_retention: retention,
        }
    }

    /// 端到端:产包成功 → 包内 db/documents/manifest 齐全 → 每条 payload 的 SHA-256 与解压内容
    /// 逐字节吻合(完整性凭据)→ external 计数正确 → 工作目录已清。
    #[test]
    fn run_backup_produces_valid_package_with_matching_sha256() {
        let root = unique_dir("valid");
        let app_data = root.join("appdata");
        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let db = seed_source(&app_data);

        let p = params(
            &db,
            &app_data,
            &dest,
            BackupKind::Manual,
            "20260719-000000",
            None,
        );
        let outcome = run_backup(&p, &CancellationToken::new()).expect("backup 应成功");

        assert!(outcome.path.exists(), "正式包应存在");
        assert!(outcome
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with(".scrollerybackup"));
        // 工作目录已回收(无 .scrollery-backup-work-* 残留)。
        let leftovers: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with(".scrollery-backup-work-"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(leftovers.is_empty(), "工作目录应已清");

        // 开包核对。
        let file = File::open(&outcome.path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        // manifest 校验。
        let manifest: Manifest = {
            let mut s = String::new();
            archive
                .by_name(ENTRY_MANIFEST)
                .unwrap()
                .read_to_string(&mut s)
                .unwrap();
            serde_json::from_str(&s).unwrap()
        };
        assert_eq!(manifest.format_version, 1);
        assert_eq!(manifest.kind, BackupKind::Manual);
        assert_eq!(
            manifest.schema_version,
            crate::db::migration::current_schema_version(),
            "schema 版本须运行时读,等于当前"
        );
        assert_eq!(manifest.external_document_versions, 1, "external 版本计数");
        assert_eq!(manifest.roots.len(), 1);
        assert_eq!(manifest.roots[0].alias.as_deref(), Some("照片库"));
        assert!(manifest.payload.iter().any(|e| e.path == ENTRY_DB));
        assert!(manifest
            .payload
            .iter()
            .any(|e| e.path == "documents/42/100.txt"));

        // 每条 payload:解压内容的 SHA-256 与 manifest 声明吻合、字节数吻合。
        for entry in &manifest.payload {
            let mut zf = archive.by_name(&entry.path).unwrap();
            let mut bytes = Vec::new();
            zf.read_to_end(&mut bytes).unwrap();
            assert_eq!(
                bytes.len() as u64,
                entry.bytes,
                "{} 字节数应吻合",
                entry.path
            );
            let mut h = Sha256::new();
            h.update(&bytes);
            assert_eq!(
                crate::utils::hash::to_hex_lower(&h.finalize()),
                entry.sha256,
                "{} 的 SHA-256 应吻合",
                entry.path
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 文档不一致(行引用的文件被删)→ 返回 backup_document_inconsistent,且**不产任何包**。
    #[test]
    fn backup_rejects_inconsistent_documents_and_produces_no_package() {
        let root = unique_dir("inconsistent");
        let app_data = root.join("appdata");
        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let db = seed_source(&app_data);
        // 删掉 appdata 版本文件 → 行引用缺失文件。
        std::fs::remove_file(app_data.join("documents").join("42").join("100.txt")).unwrap();

        let p = params(
            &db,
            &app_data,
            &dest,
            BackupKind::Manual,
            "20260719-000001",
            None,
        );
        let e = run_backup(&p, &CancellationToken::new()).expect_err("应因文档不一致失败");
        match e {
            AppError::Backup { code, .. } => assert_eq!(code, CODE_DOC_INCONSISTENT),
            other => panic!("期望 Backup{{backup_document_inconsistent}},得 {other:?}"),
        }
        // 目的地不得留下任何正式包或 tmp。
        let packages: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.contains(".scrollerybackup"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(packages.is_empty(), "不一致时不得产包(含 tmp)");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// §#14 融合校验:文档内容被篡改(行 content_hash 不变、文件字节改动)→ 打包单遍读时 xxh3
    /// 比对失败 → backup_document_inconsistent,且不产任何包(§3.1 硬门在下沉后仍守住)。
    #[test]
    fn backup_rejects_tampered_document_content_during_packing() {
        let root = unique_dir("tamper_doc");
        let app_data = root.join("appdata");
        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let db = seed_source(&app_data);
        // 篡改 appdata 版本文件内容(document_versions.content_hash 行不变)。
        std::fs::write(
            app_data.join("documents").join("42").join("100.txt"),
            "被篡改的不同内容",
        )
        .unwrap();

        let p = params(
            &db,
            &app_data,
            &dest,
            BackupKind::Manual,
            "20260719-000002",
            None,
        );
        let e = run_backup(&p, &CancellationToken::new()).expect_err("篡改内容应失败");
        match e {
            AppError::Backup { code, .. } => assert_eq!(code, CODE_DOC_INCONSISTENT),
            other => panic!("期望 backup_document_inconsistent,得 {other:?}"),
        }
        let packages: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.contains(".scrollerybackup"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(packages.is_empty(), "篡改时不得产包(含 tmp)");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// retention:连产 3 个自动包(keep=2)后仅存最新 2 个 auto;手动包永不被轮转。
    #[test]
    fn auto_retention_keeps_newest_and_spares_manual() {
        let root = unique_dir("retention");
        let app_data = root.join("appdata");
        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let db = seed_source(&app_data);

        // 先产一个手动包(retention 永不碰)。
        run_backup(
            &params(
                &db,
                &app_data,
                &dest,
                BackupKind::Manual,
                "20260719-000000",
                None,
            ),
            &CancellationToken::new(),
        )
        .unwrap();

        // 连产 3 个自动包,每次 keep=2。时间戳递增 → 保留最新两个。
        for label in ["20260719-010000", "20260719-020000", "20260719-030000"] {
            run_backup(
                &params(&db, &app_data, &dest, BackupKind::Auto, label, Some(2)),
                &CancellationToken::new(),
            )
            .unwrap();
        }

        let names: Vec<String> = std::fs::read_dir(&dest)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.ends_with(".scrollerybackup"))
            .collect();

        let autos: Vec<&String> = names.iter().filter(|n| n.contains("-auto-")).collect();
        let manuals: Vec<&String> = names.iter().filter(|n| n.contains("-backup-")).collect();
        assert_eq!(autos.len(), 2, "应仅保留最新 2 个自动包,得 {autos:?}");
        assert!(
            autos.iter().any(|n| n.contains("020000"))
                && autos.iter().any(|n| n.contains("030000")),
            "保留的应是最新两个(02/03),得 {autos:?}"
        );
        assert_eq!(manuals.len(), 1, "手动包不得被轮转");
        let _ = std::fs::remove_dir_all(&root);
    }
}
