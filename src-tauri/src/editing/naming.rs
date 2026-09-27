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
