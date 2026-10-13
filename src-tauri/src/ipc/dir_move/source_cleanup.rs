//! Windows 跨卷源清理：锁定已核验对象后按句柄删除，晚到的新内容不递归清空。

use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows::Win32::Foundation::{BOOLEAN, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
};

use super::{AppError, Result, SourceCleanup};

fn pin(path: &Path, delete_access: bool, directory: bool) -> Result<File> {
    let file = OpenOptions::new()
        .access_mode(0x8000_0000 | if delete_access { 0x0001_0000 } else { 0 })
        // 允许读取，禁止现有及未来写入/删除句柄；有外部写者时直接保留源。
        .share_mode(0x1)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)
        .map_err(|e| AppError::MoveFile(e.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|e| AppError::MoveFile(e.to_string()))?;
    // 依据打开后的对象类型拒绝链接与重解析点，不能跟随目录项检查后的替换。
    if metadata.file_attributes() & 0x400 != 0 || metadata.is_dir() != directory {
        return Err(AppError::MoveFile("source cleanup object changed".into()));
    }
    Ok(file)
}

fn delete_pinned(file: &File) -> Result<()> {
    let disposition = FILE_DISPOSITION_INFO {
        DeleteFile: BOOLEAN(1),
    };
    // File 在调用期间持有句柄；仅标记这个已核验对象，目录非空时系统拒绝删除。
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    }
    .map_err(|e| AppError::MoveFile(e.to_string()))
}

pub(super) fn remove_verified(
    src: &Path,
    dst: &Path,
    after_verified: &mut impl FnMut(&Path, &Path),
) -> Result<SourceCleanup> {
    let source_dir = pin(src, true, true)?;
    let _target_dir = pin(dst, false, true)?;
    for entry in std::fs::read_dir(src).map_err(|e| AppError::MoveFile(e.to_string()))? {
        let entry = entry.map_err(|e| AppError::MoveFile(e.to_string()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let directory = entry
            .file_type()
            .map_err(|e| AppError::MoveFile(e.to_string()))?
            .is_dir();
        if directory {
            if remove_verified(&from, &to, after_verified)? != SourceCleanup::Removed {
                return Ok(SourceCleanup::Changed);
            }
        } else {
            let mut source = pin(&from, true, false)?;
            let mut target = pin(&to, false, false)?;
            if super::digest_input(&mut source, "")? != super::digest_input(&mut target, "")? {
                return Ok(SourceCleanup::Changed);
            }
            after_verified(&from, &to);
            delete_pinned(&source)?;
            // 源关闭后删除落盘，目标句柄继续保持到本次删除完成。
            drop(source);
            drop(target);
        }
    }
    delete_pinned(&source_dir)?;
    drop(source_dir);
    Ok(SourceCleanup::Removed)
}
