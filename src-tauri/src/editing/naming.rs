//! 编辑保存的目标文件名派生 + 冲突原子化认领(方案 C §3.2/§6,C-3 裁决:采用
//! `{stem}-edit.{ext}`,冲突递增 `-edit-2`)。

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::export::naming::sanitize_component;

/// 同名冲突最多尝试次数,超出视为异常(理论不可达,同 `export::naming::resolve_conflict`
/// 的兜底口径)。
const MAX_CONFLICT_ATTEMPTS: u32 = 9999;

pub const CODE_TARGET_CONFLICT: &str = "edit_target_conflict";
pub const CODE_IO: &str = "edit_io";

/// 目标文件名 stem(不含扩展名,扩展名恒由输出格式决定)。
///
/// `custom` 非空时改用其合规化结果作 stem——v1 前端(方案 §7)未提供改名 UI,此路径是
/// 为 `EditOutput.file_name` 的契约完整性预留,尚无消费者但类型已就位。默认(`None`
/// 或空白):`{原文件名去扩展名}-edit`(C-3)。
pub fn target_stem(custom: Option<&str>, original_file_name: &str) -> String {
    match custom {
        Some(name) if !name.trim().is_empty() => sanitize_component(name),
        _ => {
            let stem = Path::new(original_file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(original_file_name);
            format!("{}-edit", sanitize_component(stem))
        }
    }
}

/// 原子认领目标文件名(TOCTOU 安全,方案 §6「冲突检查与创建必须在同一原子流程内」):
/// 在 `dir` 内从 `{stem}.{ext}` 开始尝试,冲突则 `{stem}-2.{ext}`、`{stem}-3.{ext}`……
/// 用 `create_new` 独占创建空占位文件——认领成功即返回该路径,调用方随后把真实内容写进
/// 同目录 `*.tmp` 再原子改名覆盖此占位([`crate::editing::io::write_edited_image`])。
pub fn claim_target_path(dir: &Path, stem: &str, ext: &str) -> Result<PathBuf, AppError> {
    claim_target_path_with_limit(dir, stem, ext, MAX_CONFLICT_ATTEMPTS)
}

fn claim_target_path_with_limit(
    dir: &Path,
    stem: &str,
    ext: &str,
    max_attempts: u32,
) -> Result<PathBuf, AppError> {
    for attempt in 1..=max_attempts {
        let name = if attempt == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{attempt}.{ext}")
        };
        let candidate = dir.join(&name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_file) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                return Err(AppError::Edit {
                    code: CODE_IO,
                    message: "目标目录不可写 | target directory not writable".into(),
                })
            }
        }
    }
    Err(AppError::Edit {
        code: CODE_TARGET_CONFLICT,
        message: "同名文件冲突次数过多 | too many naming conflicts".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scrollery_edit_naming_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn target_stem_defaults_to_original_stem_plus_edit_suffix() {
        assert_eq!(target_stem(None, "photo.jpg"), "photo-edit");
        assert_eq!(target_stem(Some(""), "photo.jpg"), "photo-edit");
        assert_eq!(target_stem(Some("   "), "photo.jpg"), "photo-edit");
    }

    #[test]
    fn target_stem_custom_is_sanitized() {
        assert_eq!(target_stem(Some("a:b*c"), "photo.jpg"), "a_b_c");
    }

    #[test]
    fn claim_target_path_first_attempt_has_no_suffix() {
        let dir = tmp_dir("first");
        let p = claim_target_path(&dir, "photo-edit", "jpg").unwrap();
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), "photo-edit.jpg");
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claim_target_path_increments_on_conflict() {
        let dir = tmp_dir("conflict");
        std::fs::write(dir.join("photo-edit.jpg"), b"existing").unwrap();
        let p = claim_target_path(&dir, "photo-edit", "jpg").unwrap();
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), "photo-edit-2.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claim_target_path_skips_multiple_conflicts() {
        let dir = tmp_dir("multi_conflict");
        std::fs::write(dir.join("photo-edit.jpg"), b"1").unwrap();
        std::fs::write(dir.join("photo-edit-2.jpg"), b"2").unwrap();
        std::fs::write(dir.join("photo-edit-3.jpg"), b"3").unwrap();
        let p = claim_target_path(&dir, "photo-edit", "jpg").unwrap();
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), "photo-edit-4.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 认领是「独占创建」——不是先 `exists()` 判断再 `create`,这里断言认领后文件确实已经
    /// 落盘(占位),而不是仅返回一个尚不存在的候选路径(否则调用方之间仍有 TOCTOU 窗口)。
    #[test]
    fn claim_target_path_actually_creates_placeholder_atomically() {
        let dir = tmp_dir("atomic_placeholder");
        let p = claim_target_path(&dir, "x", "png").unwrap();
        let meta = std::fs::metadata(&p).unwrap();
        assert_eq!(
            meta.len(),
            0,
            "占位文件应为空,真实内容随后写临时文件再 rename 覆盖"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claim_target_path_exhausted_returns_conflict_code() {
        let dir = tmp_dir("exhausted");
        std::fs::write(dir.join("x.png"), b"0").unwrap();
        std::fs::write(dir.join("x-2.png"), b"1").unwrap();
        let err = claim_target_path_with_limit(&dir, "x", "png", 2).unwrap_err();
        match err {
            AppError::Edit { code, .. } => assert_eq!(code, CODE_TARGET_CONFLICT),
            other => panic!("expected AppError::Edit, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
