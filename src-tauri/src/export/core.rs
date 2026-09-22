//! 导出引擎(方案 A §3):staging 复制 + 命名 + manifest + 库内目标判定。纯函数式设计(不碰
//! `AppState`/DB),时间戳/job_id/取消令牌均由调用方注入,便于单测确定性(同 `backup::core` 姿态,
//! IPC/门闩/进度事件的 app 层接线在 `ipc::export_commands`)。

use std::collections::HashSet;
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

fn rand_suffix() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = [0u8; 8];
    if SystemRandom::new().fill(&mut buf).is_ok() {
        return crate::utils::hash::to_hex_lower(&buf);
    }
    format!("{:x}", std::process::id())
}

/// 目的父目录合法性 + 可写性(方案 §3.2.1)。随机前缀探测文件立即清理(同 backup 的
/// `ensure_dest_writable` 姿态)。返回 canonicalize 后的路径,供后续库内判定复用。
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
    let probe = canon.join(format!(
        ".scrollery-export-writable-probe-{}",
        rand_suffix()
    ));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(canon)
        }
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

    std::fs::create_dir_all(&staging_dir)
        .map_err(|_| err(CODE_IO, "创建导出暂存目录失败 | create staging dir failed"))?;

    let total = items.len();
    let progress_stride = (total / 200).max(1); // 至多约 200 次事件,大批量不淹没 IPC
    let mut used_names: HashSet<String> = HashSet::with_capacity(total);
    if params.include_manifest {
        // 审查 P2:预占 manifest 文件名——否则某导出项(Original 档 + 源文件同名)的最终名恰为
        // `manifest.scrollery.json` 时,manifest 的 tmp→rename 会静默覆盖该已导出文件,
        // 而 `succeeded`/manifest 仍记其存在。
        used_names.insert(MANIFEST_FILE_NAME.to_lowercase());
    }
    let mut manifest_items = Vec::with_capacity(total);
    let mut item_results: Vec<ExportItemResult> = Vec::new();
    let mut skipped_or_failed_total = 0usize;
    let mut succeeded = 0usize;

    for (i, meta) in items.iter().enumerate() {
        if cancel.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return Err(cancelled());
        }

        let index = i + 1;
        // 审查 P3:进度回调须在每次迭代必经处触发——此前挂在循环尾,离线/冲突/路径过长等分支
        // 全靠前面的 `continue` 跳过它;若末尾若干项连续被跳过,进度条会停在 <100% 直到终态事件
        // 才推进。挪到循环体最前面,与后续任何 `continue` 分支解耦。
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
        ) else {
            record_item_issue(
                meta.file_name.clone(),
                ITEM_CODE_SKIPPED_CONFLICT,
                &mut item_results,
                &mut skipped_or_failed_total,
            );
            continue;
        };

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

        // 审查 P2:tmp 名与最终名脱钩(用循环下标而非 final_name 派生)——此前 `.{final_name}.tmp`
        // 与最终名共用同一目录命名空间,若某项最终名恰为 `.X.tmp`(Original 档下源文件本名如此),
        // 后续最终名为 `X` 的项复制时会直接覆盖它再 rename 走,前者从产出中消失但已计入 succeeded。
        let tmp_target = staging_dir.join(format!(".export-tmp-{index}"));
        let final_target = staging_dir.join(&final_name);

        let copy_result: std::io::Result<()> = (|| {
            let mut source = super::source::open(&meta.root_path, &meta.rel_path, &meta.file_name)?;
            let mut target = std::fs::File::create(&tmp_target)?;
            copy_cancellable(&mut source, &mut target, cancel)?;
            target.set_permissions(source.metadata()?.permissions())?;
            drop(target);
            let mtime = filetime::FileTime::from_unix_time(meta.file_mtime, 0);
            filetime::set_file_mtime(&tmp_target, mtime)?;
            std::fs::rename(&tmp_target, &final_target)
        })();

        if cancel.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
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
                let _ = std::fs::remove_file(&tmp_target);
                if e.kind() == std::io::ErrorKind::StorageFull {
                    // 任务级故障(§3.2.4):磁盘满不是单项问题,继续只会耗尽整批,停任务。
                    let _ = std::fs::remove_dir_all(&staging_dir);
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
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(cancelled());
    }
    if params.include_manifest {
        let manifest = Manifest::new(params.source.clone(), exported_at_utc, manifest_items);
        if let Err(e) = manifest.write_into(&staging_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
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
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(cancelled());
    }
    if std::fs::rename(&staging_dir, &final_dir).is_err() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(err(
            CODE_IO,
            "导出目录改名落盘失败 | export dir rename failed",
        ));
    }

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

    #[test]
    fn copy_stops_between_chunks_when_cancelled_during_read() {
        struct CancellingReader {
            reads: usize,
            cancel: CancellationToken,
        }
        impl std::io::Read for CancellingReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.reads += 1;
                if self.reads == 2 {
                    self.cancel.cancel();
                }
                buf.fill(7);
                Ok(buf.len())
            }
        }
        let cancel = CancellationToken::new();
        let mut source = CancellingReader {
            reads: 0,
            cancel: cancel.clone(),
        };
        let mut copied = Vec::new();
        let result = copy_cancellable(&mut source, &mut copied, &cancel);
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(source.reads, 2);
        assert_eq!(copied.len(), 256 * 1024);
        assert!(copied.iter().all(|byte| *byte == 7));
    }

    #[test]
    fn export_rejects_source_parent_traversal() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        write_src(&outside, "secret.txt");
        let target = tempfile::tempdir().unwrap();
        let params = ExportParams {
            target_parent: target.path(),
            job_id: "boundary",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: true,
            source: ExportSource::Selection,
        };
        let items = [meta(
            1,
            &root.to_string_lossy(),
            "../outside",
            "secret.txt",
            "online",
        )];
        let result = run_export(
            &params,
            &items,
            &CancellationToken::new(),
            "boundary",
            "now".into(),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.skipped_or_failed_total, 1);
        assert!(!result.final_dir.join("secret.txt").exists());
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

    #[test]
    fn cancellation_after_skipped_last_item_never_publishes() {
        let root = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let params = ExportParams {
            target_parent: target.path(),
            job_id: "skip-cancel",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: true,
            source: ExportSource::Selection,
        };
        let items = [meta(
            1,
            &root.path().to_string_lossy(),
            "",
            "missing.jpg",
            "missing",
        )];
        let cancel = CancellationToken::new();
        let result = run_export(
            &params,
            &items,
            &cancel,
            "skip-cancel",
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

    #[test]
    fn ensure_target_writable_rejects_missing_dir() {
        let bogus = std::env::temp_dir().join("scrollery-export-test-missing-xyz");
        let e = ensure_target_writable(&bogus).unwrap_err();
        match e {
            AppError::Export { code, .. } => assert_eq!(code, CODE_TARGET_INVALID),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn ensure_target_writable_accepts_real_dir() {
        let dir = tempfile::tempdir().unwrap();
        let canon = ensure_target_writable(dir.path()).unwrap();
        assert!(canon.is_dir());
    }

    #[test]
    fn is_inside_library_detects_containment() {
        let src = tempfile::tempdir().unwrap();
        let root = src.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let inside = root.join("sub");
        std::fs::create_dir_all(&inside).unwrap();
        let canon_inside = dunce::canonicalize(&inside).unwrap();
        let roots = vec![root.to_string_lossy().to_string()];
        assert!(is_inside_library(&canon_inside, &roots));

        let outside = src.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let canon_outside = dunce::canonicalize(&outside).unwrap();
        assert!(!is_inside_library(&canon_outside, &roots));
    }

    /// staging → 正式目录:成功导出后 staging 消失、正式目录含全部文件 + manifest,mtime 保留。
    #[test]
    fn run_export_happy_path_renames_to_final_dir() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
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

    /// 离线源不阻断整批:记单项 source_missing,其余照常导出。
    #[test]
    fn run_export_offline_item_recorded_not_fatal() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        write_src(src_dir.path(), "a.jpg");

        let items = vec![
            meta(
                1,
                &src_dir.path().to_string_lossy(),
                "",
                "missing.jpg",
                "missing",
            ),
            meta(2, &src_dir.path().to_string_lossy(), "", "a.jpg", "online"),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job3",
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

        assert_eq!(outcome.succeeded, 1);
        assert_eq!(outcome.skipped_or_failed_total, 1);
        assert_eq!(outcome.skipped_or_failed[0].code, ITEM_CODE_SOURCE_MISSING);
    }

    /// 审查 P3:进度回调移到循环体最前(每次迭代必经),不再挂在循环尾被 `continue` 分支跳过——
    /// 末项被跳过(此处离线)时仍须收到 `processed == total` 的终局回调,否则进度条卡在 <100%
    /// 直到终态事件才推进。
    #[test]
    fn run_export_reports_final_progress_even_when_last_item_is_skipped() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        write_src(src_dir.path(), "a.jpg");

        let items = vec![
            meta(1, &src_dir.path().to_string_lossy(), "", "a.jpg", "online"),
            meta(
                2,
                &src_dir.path().to_string_lossy(),
                "",
                "missing.jpg",
                "missing",
            ),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job8",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Rename,
            include_manifest: false,
            source: ExportSource::Selection,
        };
        let cancel = CancellationToken::new();
        let mut calls: Vec<(usize, usize)> = Vec::new();
        let outcome = run_export(
            &params,
            &items,
            &cancel,
            "20260719-000000",
            "2026-07-19T00:00:00Z".into(),
            |processed, total| calls.push((processed, total)),
        )
        .unwrap();

        assert_eq!(outcome.succeeded, 1);
        assert!(
            calls.contains(&(2, 2)),
            "末项被跳过时仍须收到 processed==total 的终局进度回调,实收:{calls:?}"
        );
    }

    /// 冲突策略 Skip:同名第二项记 skipped_conflict,不覆盖第一项。
    #[test]
    fn run_export_conflict_skip_records_and_keeps_first() {
        let src_dir = tempfile::tempdir().unwrap();
        let sub_dir = src_dir.path().join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let parent = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join("a.jpg"), b"first").unwrap();
        std::fs::write(sub_dir.join("a.jpg"), b"second").unwrap();

        let items = vec![
            meta(1, &src_dir.path().to_string_lossy(), "", "a.jpg", "online"),
            meta(
                2,
                &src_dir.path().to_string_lossy(),
                "sub",
                "a.jpg",
                "online",
            ),
        ];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job4",
            naming: NamingScheme::Original,
            conflict: ExportConflict::Skip,
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

        assert_eq!(outcome.succeeded, 1);
        assert_eq!(
            outcome.skipped_or_failed[0].code,
            ITEM_CODE_SKIPPED_CONFLICT
        );
        let content = std::fs::read_to_string(outcome.final_dir.join("a.jpg")).unwrap();
        assert_eq!(content, "first");
    }

    /// 审查 P2:tmp 名与最终名脱钩——此前 `.{final_name}.tmp` 与最终名共用同一命名空间,若某项
    /// 最终名恰为 `.x.tmp`(源文件本名如此),会被后续最终名为 `x` 的项的 tmp 写入悄悄覆盖再消失
    /// (succeeded 已计数但产出物不见)。两项均须完整、内容互不污染。
    #[test]
    fn run_export_tmp_name_decoupled_from_final_name_no_clobber() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        std::fs::write(src_dir.path().join(".x.tmp"), b"A-content").unwrap();
        std::fs::write(src_dir.path().join("x"), b"B-content").unwrap();

        let items = vec![
            meta(1, &src_dir.path().to_string_lossy(), "", ".x.tmp", "online"),
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
            std::fs::read_to_string(outcome.final_dir.join(".x.tmp")).unwrap(),
            "A-content"
        );
        assert_eq!(
            std::fs::read_to_string(outcome.final_dir.join("x")).unwrap(),
            "B-content"
        );
    }

    /// 审查 P2:manifest 文件名预占——导出项的最终名恰与 `MANIFEST_FILE_NAME` 相同(Original 档 +
    /// 源文件同名)时,该项须被冲突去重改名,不得被 manifest 的写入静默覆盖。
    #[test]
    fn run_export_reserves_manifest_file_name_from_item_collision() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        write_src(src_dir.path(), MANIFEST_FILE_NAME);

        let items = vec![meta(
            1,
            &src_dir.path().to_string_lossy(),
            "",
            MANIFEST_FILE_NAME,
            "online",
        )];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job7",
            naming: NamingScheme::Original,
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

        assert_eq!(outcome.succeeded, 1);
        // 冲突去重在 stem/扩展名之间插 `-2`(resolve_conflict 姿态)。
        let renamed = outcome.final_dir.join("manifest.scrollery-2.json");
        assert!(renamed.exists(), "撞名项应被改名而非覆盖 manifest");
        let manifest_text =
            std::fs::read_to_string(outcome.final_dir.join(MANIFEST_FILE_NAME)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
        assert_eq!(parsed["items"][0]["file"], "manifest.scrollery-2.json");
    }

    /// 审查 P1:终局目录名撞车(同一秒内连续两次导出用同一时间戳标签)时追加 `-2` 后缀,
    /// 不静默覆盖已存在的正式目录;新导出的文件落进新分配的目录里。
    #[test]
    fn run_export_dedupes_final_dir_name_on_collision() {
        let src_dir = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        write_src(src_dir.path(), "a.jpg");
        // 抢先占用第一次会用到的正式目录名,模拟撞名。
        let first_final_dir = parent.path().join(final_dir_name("20260719-000000"));
        std::fs::create_dir_all(&first_final_dir).unwrap();
        std::fs::write(first_final_dir.join("existing.txt"), b"keep").unwrap();

        let items = vec![meta(
            1,
            &src_dir.path().to_string_lossy(),
            "",
            "a.jpg",
            "online",
        )];
        let params = ExportParams {
            target_parent: parent.path(),
            job_id: "job5",
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

        let expected_dir = parent
            .path()
            .join(format!("{}-2", final_dir_name("20260719-000000")));
        assert_eq!(outcome.final_dir, expected_dir);
        assert!(outcome.final_dir.join("a.jpg").exists());
        // 原目录(及其内容)未被触碰。
        assert!(first_final_dir.join("existing.txt").exists());
    }
}
