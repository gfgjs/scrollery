//! 导出引擎：检查目标、命名、暂存复制和 manifest 写入。
//! 不直接访问 `AppState` 或 DB；时间戳、job_id 和取消令牌由调用方注入。
//! IPC、门闩和进度事件由 `ipc::export_commands` 编排。

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::db::queries::ExportItemMeta;
use crate::error::{AppError, Result};

use super::manifest::{ExportSource, Manifest, ManifestItem, MANIFEST_FILE_NAME};
use super::naming::{build_file_name, resolve_conflict, NamingScheme};

pub const CODE_TARGET_INVALID: &str = "export_target_invalid";
pub const CODE_TARGET_NOT_WRITABLE: &str = "export_target_not_writable";
pub const CODE_CANCELLED: &str = "export_cancelled";
pub const CODE_IO: &str = "export_io";
pub const CODE_CLEANUP_FAILED: &str = "export_cleanup_failed";

/// 单项失败的稳定码(§3.3「单文件结果」)。与 `AppError` 的任务级码是两套独立集合——单项
/// 失败不中断整批,只作为结果数据,不经 `AppError` 传播。
pub const ITEM_CODE_SOURCE_MISSING: &str = "source_missing";
pub const ITEM_CODE_NAME_INVALID: &str = "name_invalid";
pub const ITEM_CODE_PATH_TOO_LONG: &str = "path_too_long";
pub const ITEM_CODE_IO: &str = "item_io";
pub const ITEM_CODE_SKIPPED_CONFLICT: &str = "skipped_conflict";

/// 单项结果明细上限(方案 §3.3:「上限封顶,避免百万失败项撑爆 IPC」)。
pub const ITEM_RESULT_CAP: usize = 100;

/// Windows `MAX_PATH`(260)的保守护栏。不做逐平台精确判定(macOS/Linux 限制更宽松但导出
/// 目标平台在运行时已知、跨平台契约仍按最严约束统一,方案 §2.3「路径过长」)。
const MAX_STAGING_PATH_LEN: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportConflict {
    Rename,
    Skip,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportItemResult {
    pub file_name: String,
    pub code: &'static str,
}

#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub final_dir: PathBuf,
    pub succeeded: usize,
    /// 详情条数已封顶(`ITEM_RESULT_CAP`);`skipped_or_failed_total` 是未封顶总数。
    pub skipped_or_failed: Vec<ExportItemResult>,
    pub skipped_or_failed_total: usize,
}

pub struct ExportParams<'a> {
    pub target_parent: &'a Path,
    pub job_id: &'a str,
    pub naming: NamingScheme,
    pub conflict: ExportConflict,
    pub include_manifest: bool,
    pub source: ExportSource,
}

fn err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::Export {
        code,
        message: message.into(),
    }
}

fn cancelled() -> AppError {
    err(CODE_CANCELLED, "导出已取消 | export cancelled")
}

/// 目的父目录合法性 + 可写性；匿名探针由句柄生命周期清理，不写删用户文件名。
/// 返回 canonicalize 后的路径，供后续库内判定复用。
pub fn ensure_target_writable(target_parent: &Path) -> Result<PathBuf> {
    let canon = dunce::canonicalize(target_parent).map_err(|_| {
        err(
            CODE_TARGET_INVALID,
            "目的父目录不存在或不是目录 | target parent invalid",
        )
    })?;
    if !canon.is_dir() {
        return Err(err(
            CODE_TARGET_INVALID,
            "目的父目录不是目录 | target parent is not a directory",
        ));
    }
    match tempfile::tempfile_in(&canon).and_then(|mut probe| probe.write_all(b"ok")) {
        Ok(()) => Ok(canon),
        Err(_) => Err(err(
            CODE_TARGET_NOT_WRITABLE,
            "目的父目录不可写 | target parent not writable",
        )),
    }
}

/// 目标是否落在任一扫描根内部(方案 §3.2.2:「与所有 `scan_roots.path` 做规范化的路径组件
/// 比较,不能用字符串 `starts_with`」)。`Path::starts_with` 本身即按组件比较(不同于字符串
/// 前缀拼接),两侧均先 canonicalize 以消除大小写/短路径/尾部分隔符差异。
pub fn is_inside_library(canon_target: &Path, scan_root_paths: &[String]) -> bool {
    scan_root_paths.iter().any(|root| {
        let Ok(canon_root) = dunce::canonicalize(root) else {
            return false; // 根路径本身已离线/不可达,不参与库内判定(不可达=不会被物理污染)
        };
        canon_target.starts_with(&canon_root)
    })
}

fn final_dir_name(timestamp_label: &str) -> String {
    format!("Scrollery-export-{timestamp_label}")
}

fn staging_dir_name(job_id: &str) -> String {
    format!(".scrollery-export-{job_id}.tmp")
}

fn cleanup_staging(staging_dir: &Path) -> Result<()> {
    // 清理未完成优先于原取消/失败原因，防止遗留暂存却报告干净取消；保留残余供手动处理。
    std::fs::remove_dir_all(staging_dir).map_err(|_| {
        err(
            CODE_CLEANUP_FAILED,
            "导出暂存清理失败，请关闭占用后手动清理目标目录内本任务的暂存目录 | export staging cleanup failed; release file locks and manually remove this job's staging directory",
        )
    })
}

fn publish_staging(staging_dir: &Path, final_dir: &Path) -> Result<()> {
    // 名称探测之后仍可能出现第三方目录，拒绝覆盖必须由同一个原子操作保证。
    if crate::utils::fs::rename_no_replace(staging_dir, final_dir).is_err() {
        cleanup_staging(staging_dir)?;
        return Err(err(
            CODE_IO,
            "导出目录改名落盘失败 | export dir rename failed",
        ));
    }
    Ok(())
}

/// 根别名缺省时回退根路径的最后一段(不落绝对路径全串,只取目录名作可读标签)。
fn root_alias_or_basename(meta: &ExportItemMeta) -> String {
    meta.root_alias.clone().unwrap_or_else(|| {
        Path::new(&meta.root_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    })
}

fn record_item_issue(
    name: String,
    code: &'static str,
    results: &mut Vec<ExportItemResult>,
    total_ct: &mut usize,
) {
    *total_ct += 1;
    if results.len() < ITEM_RESULT_CAP {
        results.push(ExportItemResult {
            file_name: name,
            code,
        });
    }
}

// 固定内存分块，使大视频复制期间也能响应取消；不能中断正在进行的单次磁盘系统调用。
fn copy_cancellable(
    source: &mut impl std::io::Read,
    target: &mut impl std::io::Write,
    cancel: &CancellationToken,
) -> std::io::Result<()> {
    let mut buffer = vec![0; 256 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let count = match source.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        if count == 0 {
            return Ok(());
        }
        target.write_all(&buffer[..count])?;
    }
}

/// 运行导出(方案 §3.2):逐项 staging 内 `.tmp` → rename,复制分块及最终发布前检查取消,成功后
/// 写 manifest、最后整目录 rename 为正式目录。`items` 已由调用方按目标顺序解析并取好元数据
/// (本函数不碰 DB)。`on_progress(processed, total)` 内部按项目数动态节流,避免万级导出逐项
/// 发事件淹没 IPC。
pub fn run_export(
    params: &ExportParams,
    items: &[ExportItemMeta],
    cancel: &CancellationToken,
    timestamp_label: &str,
    exported_at_utc: String,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<ExportOutcome> {
    let staging_dir = params.target_parent.join(staging_dir_name(params.job_id));

    // 只有独占创建成功才拥有清理权，不能接管同名旧目录。
    std::fs::create_dir(&staging_dir)
        .map_err(|_| err(CODE_IO, "创建导出暂存目录失败 | create staging dir failed"))?;

    let total = items.len();
    let progress_stride = (total / 200).max(1); // 至多约 200 次事件,大批量不淹没 IPC
    let mut used_names: HashSet<String> = HashSet::with_capacity(total);
    let mut next_suffixes = HashMap::new();
    if params.include_manifest {
        // 预占 manifest 文件名，避免同名源文件被 manifest 发布覆盖。
        used_names.insert(MANIFEST_FILE_NAME.to_lowercase());
    }
    let mut manifest_items = Vec::with_capacity(total);
    let mut item_results: Vec<ExportItemResult> = Vec::new();
    let mut skipped_or_failed_total = 0usize;
    let mut succeeded = 0usize;

    for (i, meta) in items.iter().enumerate() {
        if cancel.is_cancelled() {
            cleanup_staging(&staging_dir)?;
            return Err(cancelled());
        }

        let index = i + 1;
        // 进度回调置于所有 `continue` 之前，跳过项也计入处理进度。
        if index % progress_stride == 0 || index == total {
            on_progress(index, total);
        }
        if meta.availability != "online" {
            record_item_issue(
                meta.file_name.clone(),
                ITEM_CODE_SOURCE_MISSING,
                &mut item_results,
                &mut skipped_or_failed_total,
            );
            continue;
        }

        let candidate = build_file_name(
            params.naming,
            index,
            total,
            &meta.file_name,
            meta.sort_datetime,
        );
        let Some(final_name) = resolve_conflict(
            &candidate,
            &used_names,
            params.conflict == ExportConflict::Rename,
            &mut next_suffixes,
        ) else {
            record_item_issue(
                meta.file_name.clone(),
                ITEM_CODE_SKIPPED_CONFLICT,
                &mut item_results,
                &mut skipped_or_failed_total,
            );
            continue;
        };

        #[cfg(windows)]
        let full_len = {
            use std::os::windows::ffi::OsStrExt as _;
            // Windows 按 UTF-16 单元限制路径；字节数会把中文等合法路径误判为过长。
            staging_dir.as_os_str().encode_wide().count() + 1 + final_name.encode_utf16().count()
        };
        #[cfg(not(windows))]
        let full_len = staging_dir.as_os_str().len() + 1 + final_name.len();
        if full_len > MAX_STAGING_PATH_LEN {
            record_item_issue(
                meta.file_name.clone(),
                ITEM_CODE_PATH_TOO_LONG,
                &mut item_results,
                &mut skipped_or_failed_total,
            );
            continue;
        }

        // 临时名也可能是合法原名；避开已发布项和本项正式名，不能仅换一个固定前缀。
        let tmp_prefix = format!(".export-tmp-{index}");
        let mut tmp_name = tmp_prefix.clone();
        let mut tmp_suffix = 2usize;
        while used_names.contains(&tmp_name.to_lowercase())
            || tmp_name.eq_ignore_ascii_case(&final_name)
        {
            tmp_name = format!("{tmp_prefix}-{tmp_suffix}");
            tmp_suffix += 1;
        }
        let tmp_target = staging_dir.join(tmp_name);
        let final_target = staging_dir.join(&final_name);

        let copy_result: std::io::Result<()> = (|| {
            let mut source = super::source::open(&meta.root_path, &meta.rel_path, &meta.file_name)?;
            let mut target = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_target)?;
            let result = (|| {
                copy_cancellable(&mut source, &mut target, cancel)?;
                // 先用已打开的句柄写时间，再复制只读属性；否则 Windows 重新写开会被拒绝。
                let mtime = filetime::FileTime::from_unix_time(meta.file_mtime, 0);
                filetime::set_file_handle_times(&target, None, Some(mtime))?;
                target.set_permissions(source.metadata()?.permissions())
            })();
            drop(target);
            let result = result.and_then(|()| std::fs::rename(&tmp_target, &final_target));
            // 仅清理由本项 create_new 成功创建的临时文件，不碰创建失败时已存在的条目。
            if result.is_err() {
                let _ = std::fs::remove_file(&tmp_target);
            }
            result
        })();

        if cancel.is_cancelled() {
            cleanup_staging(&staging_dir)?;
            return Err(cancelled());
        }

        match copy_result {
            Ok(()) => {
                used_names.insert(final_name.to_lowercase());
                succeeded += 1;
                if params.include_manifest {
                    manifest_items.push(ManifestItem {
                        file: final_name,
                        root_alias: root_alias_or_basename(meta),
                        root_relative_path: if meta.rel_path.is_empty() {
                            meta.file_name.clone()
                        } else {
                            format!("{}/{}", meta.rel_path, meta.file_name)
                        },
                        sort_index: index,
                        view_rotation: meta.view_rotation,
                        rating: meta.rating,
                        color_label: meta.color_label,
                        favorited: meta.favorited,
                        tags: meta.tags.clone(),
                        albums: meta.albums.clone(),
                    });
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::StorageFull {
                    // 任务级故障(§3.2.4):磁盘满不是单项问题,继续只会耗尽整批,停任务。
                    cleanup_staging(&staging_dir)?;
                    return Err(err(CODE_IO, "磁盘空间不足 | disk full"));
                }
                record_item_issue(
                    meta.file_name.clone(),
                    ITEM_CODE_IO,
                    &mut item_results,
                    &mut skipped_or_failed_total,
                );
            }
        }
    }

    if cancel.is_cancelled() {
        cleanup_staging(&staging_dir)?;
        return Err(cancelled());
    }
    if params.include_manifest {
        let manifest = Manifest::new(params.source.clone(), exported_at_utc, manifest_items);
        if let Err(e) = manifest.write_into(&staging_dir) {
            cleanup_staging(&staging_dir)?;
            return Err(e);
        }
    }

    // 终局目录名去重(审查 P1):同参数目录短时间内连续导出,秒级时间戳标签可能撞名——探测
    // 存在性追加 `-2`/`-3` 后缀,不静默覆盖旧导出;rename 失败同样清理 staging(此前只有
    // cancel/manifest 失败分支才清,唯独此分支漏了,遗留孤儿 `.tmp` 目录)。
    let mut final_dir = params.target_parent.join(final_dir_name(timestamp_label));
    let mut suffix = 2u32;
    while final_dir.exists() {
        final_dir = params
            .target_parent
            .join(format!("{}-{suffix}", final_dir_name(timestamp_label)));
        suffix += 1;
    }

    if cancel.is_cancelled() {
        cleanup_staging(&staging_dir)?;
        return Err(cancelled());
    }
    publish_staging(&staging_dir, &final_dir)?;

    Ok(ExportOutcome {
        final_dir,
        succeeded,
        skipped_or_failed: item_results,
        skipped_or_failed_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: i64, root: &str, rel: &str, file_name: &str, availability: &str) -> ExportItemMeta {
        ExportItemMeta {
            id,
            root_path: root.to_string(),
            root_alias: None,
            rel_path: rel.to_string(),
            file_name: file_name.to_string(),
            file_size: 3,
            file_mtime: 1_700_000_000,
            sort_datetime: 1_700_000_000,
            view_rotation: 0,
            rating: 0,
            color_label: 0,
            favorited: false,
            availability: availability.to_string(),
            tags: vec![],
            albums: vec![],
        }
    }

    fn write_src(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"abc").unwrap();
        p
    }

    #[cfg(windows)]
    fn export_with_locked_copy(cancel_requested: bool) -> AppError {
        use std::os::windows::fs::OpenOptionsExt;

        let source = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let first = write_src(source.path(), "first.txt");
        let second = write_src(source.path(), "second.txt");
        let sentinel = write_src(parent.path(), "keep.txt");
        let other_staging = parent.path().join(staging_dir_name("other-job"));
        std::fs::create_dir(&other_staging).unwrap();
        let other_file = write_src(&other_staging, "foreign.txt");
        let staging = parent.path().join(staging_dir_name("locked-copy"));
        let items = [
            meta(
                1,
                &source.path().to_string_lossy(),
                "",
                "first.txt",
                "online",
            ),
            meta(
                2,
                &source.path().to_string_lossy(),
                "",
                "second.txt",
                "online",
            ),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "locked-copy",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: false,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        let mut held = None;
        let result = run_export(
            &params,
            &items,
            &cancel,
            "locked-copy",
            "now".into(),
            |index, _| {
                if index == 2 {
                    let copied = staging.join("first.txt");
                    assert_eq!(std::fs::read(&copied).unwrap(), b"abc");
                    // 第 2 项回调时首项已关闭并落名；独占句柄确定性阻止删除及目录发布。
                    held = Some(
                        std::fs::OpenOptions::new()
                            .read(true)
                            .share_mode(0)
                            .open(copied)
                            .unwrap(),
                    );
                    if cancel_requested {
                        cancel.cancel();
                    }
                }
            },
        );
        assert!(held.is_some());
        assert!(staging.is_dir(), "清理失败后应保留可手动处理的暂存目录");
        assert!(!parent.path().join(final_dir_name("locked-copy")).exists());
        let error = result.unwrap_err();
        println!("locked copy: cancel={cancel_requested}, observed={error}");
        assert!(std::fs::remove_file(staging.join("first.txt")).is_err());
        drop(held);
        assert_eq!(std::fs::read(staging.join("first.txt")).unwrap(), b"abc");
        std::fs::remove_dir_all(&staging).unwrap();
        assert!(!staging.exists());
        assert_eq!(std::fs::read(first).unwrap(), b"abc");
        assert_eq!(std::fs::read(second).unwrap(), b"abc");
        assert_eq!(std::fs::read(sentinel).unwrap(), b"abc");
        assert_eq!(std::fs::read(other_file).unwrap(), b"abc");
        assert_eq!(std::fs::read_dir(source.path()).unwrap().count(), 2);
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 2);
        println!(
            "handle released: manual cleanup succeeded; sources and adjacent content unchanged"
        );
        error
    }

    #[cfg(windows)]
    #[test]
    fn cancel_cleanup_failure_reports_stable_error() {
        let error = export_with_locked_copy(true);
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "code": "export_cleanup_failed",
                "message": "导出暂存清理失败，请关闭占用后手动清理目标目录内本任务的暂存目录 | export staging cleanup failed; release file locks and manually remove this job's staging directory"
            })
        );
    }

    #[cfg(windows)]
    #[test]
    fn publish_cleanup_failure_reports_stable_error() {
        let error = export_with_locked_copy(false);
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "code": "export_cleanup_failed",
                "message": "导出暂存清理失败，请关闭占用后手动清理目标目录内本任务的暂存目录 | export staging cleanup failed; release file locks and manually remove this job's staging directory"
            })
        );
    }

    #[test]
    fn preexisting_staging_is_never_adopted_or_deleted() {
        let parent = tempfile::tempdir().unwrap();
        let staging = parent.path().join(staging_dir_name("occupied"));
        std::fs::create_dir(&staging).unwrap();
        let sentinel = write_src(&staging, "foreign.txt");
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "occupied",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: false,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = run_export(&params, &[], &cancel, "occupied", "now".into(), |_, _| {});
        assert!(sentinel.exists(), "已有目录内容不得由本次取消删除");
        assert_eq!(std::fs::read(sentinel).unwrap(), b"abc");
        assert!(matches!(
            result,
            Err(AppError::Export { code: CODE_IO, .. })
        ));
    }

    #[test]
    fn cancel_after_readonly_item_copied_cleans_owned_staging() {
        let source = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let first = write_src(source.path(), "first.txt");
        write_src(source.path(), "second.txt");
        let original_permissions = std::fs::metadata(&first).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&first, readonly).unwrap();
        let sentinel = write_src(parent.path(), "keep.txt");
        let staging = parent.path().join(staging_dir_name("partial"));
        let items = [
            meta(
                1,
                &source.path().to_string_lossy(),
                "",
                "first.txt",
                "online",
            ),
            meta(
                2,
                &source.path().to_string_lossy(),
                "",
                "second.txt",
                "online",
            ),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "partial",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: true,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        let mut observed_completed_item = false;
        let result = run_export(
            &params,
            &items,
            &cancel,
            "partial",
            "now".into(),
            |index, _| {
                if index == 2 {
                    let copied = staging.join("first.txt");
                    assert_eq!(std::fs::read(&copied).unwrap(), b"abc");
                    assert!(std::fs::metadata(copied).unwrap().permissions().readonly());
                    observed_completed_item = true;
                    cancel.cancel();
                }
            },
        );
        assert!(observed_completed_item);
        assert!(matches!(
            result,
            Err(AppError::Export {
                code: CODE_CANCELLED,
                ..
            })
        ));
        assert!(!staging.exists());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(sentinel).unwrap(), b"abc");
        assert_eq!(std::fs::read(&first).unwrap(), b"abc");
        assert!(std::fs::metadata(&first).unwrap().permissions().readonly());
        std::fs::set_permissions(first, original_permissions).unwrap();
    }

    #[test]
    fn publish_rejects_target_created_after_name_selection() {
        let parent = tempfile::tempdir().unwrap();
        let staging = parent.path().join(staging_dir_name("publish"));
        std::fs::create_dir(&staging).unwrap();
        write_src(&staging, "exported.txt");
        let final_dir = parent.path().join(final_dir_name("publish"));
        assert!(!final_dir.exists());
        std::fs::create_dir(&final_dir).unwrap();
        let result = publish_staging(&staging, &final_dir);
        assert!(matches!(
            result,
            Err(AppError::Export { code: CODE_IO, .. })
        ));
        assert!(final_dir.is_dir());
        assert_eq!(std::fs::read_dir(final_dir).unwrap().count(), 0);
        assert!(!staging.exists());
    }

    #[test]
    fn cancellation_during_last_progress_never_publishes() {
        let root = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        write_src(root.path(), "a.jpg");
        let params = ExportParams {
            target_parent: target.path(),
            job_id: "last-cancel",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: true,
            source: ExportSource::Selection,
        };
        let items = [meta(
            1,
            &root.path().to_string_lossy(),
            "",
            "a.jpg",
            "online",
        )];
        let cancel = CancellationToken::new();
        let result = run_export(
            &params,
            &items,
            &cancel,
            "last-cancel",
            "now".into(),
            |_, _| cancel.cancel(),
        );
        assert!(matches!(
            result,
            Err(AppError::Export {
                code: CODE_CANCELLED,
                ..
            })
        ));
        assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 0);
    }

    /// staging → 正式目录:成功导出后 staging 消失、正式目录含全部文件 + manifest,mtime 保留。
    #[test]
    fn run_export_happy_path_renames_to_final_dir() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let checked_parent = ensure_target_writable(parent.path()).unwrap();
        assert!(checked_parent.is_absolute());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
        write_src(src_dir.path(), "a.jpg");
        write_src(src_dir.path(), "b.jpg");

        let items = vec![
            meta(1, &src_dir.path().to_string_lossy(), "", "a.jpg", "online"),
            meta(2, &src_dir.path().to_string_lossy(), "", "b.jpg", "online"),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job1",
            naming: NamingScheme::Sequence,
            conflict: ExportConflict::Rename,
            include_manifest: true,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        let outcome = run_export(
            &params,
            &items,
            &cancel,
            "20260719-000000",
            "2026-07-19T00:00:00Z".into(),
            |_, _| {},
        )
        .unwrap();

        assert_eq!(outcome.succeeded, 2);
        assert!(outcome.final_dir.is_dir());
        assert!(!parent.path().join(staging_dir_name("job1")).exists());
        assert!(outcome.final_dir.join("001-a.jpg").exists());
        assert!(outcome.final_dir.join("002-b.jpg").exists());
        assert!(outcome
            .final_dir
            .join(super::super::manifest::MANIFEST_FILE_NAME)
            .exists());

        // mtime 来自 DB 记录的 `meta.file_mtime`(1_700_000_000),不是复制瞬间源文件在磁盘上的
        // 实际 mtime(`write_src` 刚写入,应为「现在」)——这正是要验证的契约:显式回写 DB 值,
        // 不依赖 std::fs::copy 在某些平台「碰巧」保留源 mtime。
        let dst_mtime = std::fs::metadata(outcome.final_dir.join("001-a.jpg"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            dst_mtime
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1_700_000_000,
        );
    }

    /// 取消:只删本 job 的 staging 目录,不产出正式目录,不影响 target_parent 里的其它内容。
    #[test]
    fn run_export_cancelled_cleans_only_own_staging() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        write_src(src_dir.path(), "a.jpg");
        std::fs::write(parent.path().join("untouched.txt"), b"keep").unwrap();

        let items = vec![meta(
            1,
            &src_dir.path().to_string_lossy(),
            "",
            "a.jpg",
            "online",
        )];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job2",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: false,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let final_dir_would_be = parent.path().join(final_dir_name("20260719-000000"));
        let e = run_export(
            &params,
            &items,
            &cancel,
            "20260719-000000",
            "2026-07-19T00:00:00Z".into(),
            |_, _| {},
        )
        .unwrap_err();

        match e {
            AppError::Export { code, .. } => assert_eq!(code, CODE_CANCELLED),
            _ => panic!("wrong variant"),
        }
        assert!(!parent.path().join(staging_dir_name("job2")).exists());
        assert!(!final_dir_would_be.exists());
        assert!(parent.path().join("untouched.txt").exists());
    }

    /// 临时名仍可能是合法原名：先导出的 `.export-tmp-2` 不得被第二项临时写入覆盖再消失。
    /// 两项均须完整、内容互不污染，且原始资产不变。
    #[test]
    fn run_export_tmp_name_decoupled_from_final_name_no_clobber() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join(".export-tmp-2"), b"A-content").unwrap();
        std::fs::write(src_dir.path().join("x"), b"B-content").unwrap();

        let items = vec![
            meta(
                1,
                &src_dir.path().to_string_lossy(),
                "",
                ".export-tmp-2",
                "online",
            ),
            meta(2, &src_dir.path().to_string_lossy(), "", "x", "online"),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job6",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: false,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        let outcome = run_export(
            &params,
            &items,
            &cancel,
            "20260719-000000",
            "2026-07-19T00:00:00Z".into(),
            |_, _| {},
        )
        .unwrap();

        assert_eq!(outcome.succeeded, 2);
        assert_eq!(
            std::fs::read_to_string(outcome.final_dir.join(".export-tmp-2")).unwrap(),
            "A-content"
        );
        assert_eq!(
            std::fs::read_to_string(outcome.final_dir.join("x")).unwrap(),
            "B-content"
        );
        assert_eq!(outcome.skipped_or_failed_total, 0);
        assert_eq!(std::fs::read_dir(&outcome.final_dir).unwrap().count(), 2);
        assert_eq!(
            std::fs::read_to_string(src_dir.path().join(".export-tmp-2")).unwrap(),
            "A-content"
        );
        assert_eq!(
            std::fs::read_to_string(src_dir.path().join("x")).unwrap(),
            "B-content"
        );
    }
}
