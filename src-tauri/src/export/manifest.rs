//! 导出 manifest 结构与写入。只记录成功导出的项;失败/跳过明细走任务结果,
//! 不进 manifest。不写源绝对路径 / 凭据 / 人脸框 / 人物身份。

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::utils::fs::rename_no_replace;

pub const MANIFEST_FILE_NAME: &str = "manifest.scrollery.json";
const CODE_IO: &str = "export_io";

/// 导出来源(方案 §2.4:「结构化枚举,不接收前端自由字符串」)。前端按发起入口
/// (选区工具条 / 相册工具栏 / 当前视图工具栏)构造对应变体,后端不做自由文本拼接。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportSource {
    /// 选区工具条 / 右键菜单发起的显式选择,无固定容器身份。
    Selection,
    /// 相册工具栏发起。
    Album { id: i64, name: String },
    /// 当前画廊/时间轴视图工具栏发起(标签/收藏/搜索结果等经当前视图 + 全选覆盖)。
    View { name: String },
}

/// manifest 单项(方案 §2.4 JSON 示例字段一一对应)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestItem {
    pub file: String,
    pub root_alias: String,
    pub root_relative_path: String,
    pub sort_index: usize,
    pub view_rotation: i64,
    pub rating: i64,
    pub color_label: i64,
    pub favorited: bool,
    pub tags: Vec<String>,
    pub albums: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub app: String,
    pub app_version: String,
    pub exported_at_utc: String,
    pub source: ExportSource,
    pub items: Vec<ManifestItem>,
}

impl Manifest {
    pub fn new(source: ExportSource, exported_at_utc: String, items: Vec<ManifestItem>) -> Self {
        Self {
            format_version: 1,
            app: "Scrollery".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at_utc,
            source,
            items,
        }
    }

    /// 原子写入 staging 目录内(方案 §2.4:「在全部文件落地后最后原子写入」)。staging 整体目录
    /// 随后由调用方一次性 rename 为正式目录,这里的 `.tmp`→rename 是双保险(即便 staging 目录
    /// 本身因某种原因被直接检视,也不会看到半写的 manifest)。
    pub fn write_into(&self, staging_dir: &Path) -> Result<()> {
        let final_path = staging_dir.join(MANIFEST_FILE_NAME);
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| AppError::Export {
            code: CODE_IO,
            message: "manifest 序列化失败".into(),
        })?;
        // staging 已有合法导出文件，临时名必须独占创建，不能截断同名资产。
        let mut tmp = tempfile::Builder::new()
            .prefix(&format!(".{MANIFEST_FILE_NAME}."))
            .suffix(".tmp")
            .tempfile_in(staging_dir)
            .map_err(|_| AppError::Export {
                code: CODE_IO,
                message: "写 manifest 失败".into(),
            })?;
        tmp.write_all(&bytes).map_err(|_| AppError::Export {
            code: CODE_IO,
            message: "写 manifest 失败".into(),
        })?;
        tmp.as_file().sync_all().map_err(|_| AppError::Export {
            code: CODE_IO,
            message: "manifest 落盘 fsync 失败".into(),
        })?;
        // 先关闭文件句柄再发布；写入或发布失败时仅清理本轮拥有的临时文件。
        let tmp = tmp.into_temp_path();
        rename_no_replace(&tmp, &final_path).map_err(|_| AppError::Export {
            code: CODE_IO,
            message: "manifest 改名落盘失败".into(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_exported_file_named_like_old_manifest_temp() {
        let staging = tempfile::tempdir().unwrap();
        let file_name = ".manifest.scrollery.json.tmp";
        let exported_path = staging.path().join(file_name);
        let original = b"exported asset content\x00\xff";
        std::fs::write(&exported_path, original).unwrap();
        let manifest = Manifest::new(
            ExportSource::Selection,
            "2026-10-11T00:00:00Z".into(),
            vec![ManifestItem {
                file: file_name.into(),
                root_alias: "root".into(),
                root_relative_path: file_name.into(),
                sort_index: 0,
                view_rotation: 0,
                rating: 0,
                color_label: 0,
                favorited: false,
                tags: vec![],
                albums: vec![],
            }],
        );

        manifest.write_into(staging.path()).unwrap();

        assert_eq!(std::fs::read(&exported_path).unwrap(), original);
        let written: serde_json::Value = serde_json::from_slice(
            &std::fs::read(staging.path().join(MANIFEST_FILE_NAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(written, serde_json::to_value(&manifest).unwrap());
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 2);
    }

    #[test]
    fn rejects_existing_manifest_and_cleans_owned_temp() {
        let staging = tempfile::tempdir().unwrap();
        let final_path = staging.path().join(MANIFEST_FILE_NAME);
        let original = b"existing manifest must survive";
        std::fs::write(&final_path, original).unwrap();
        let manifest = Manifest::new(
            ExportSource::Selection,
            "2026-10-11T00:00:00Z".into(),
            vec![],
        );

        let result = manifest.write_into(staging.path());

        assert!(matches!(
            result,
            Err(AppError::Export { code: CODE_IO, .. })
        ));
        assert_eq!(std::fs::read(final_path).unwrap(), original);
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 1);
    }
}
