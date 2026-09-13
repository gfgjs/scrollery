//! 导出 manifest 结构与写入(方案 A §2.4)。只记录成功导出的项;失败/跳过明细走任务结果,
//! 不进 manifest。不写源绝对路径 / 凭据 / 人脸框 / 人物身份。

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

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
        let tmp = staging_dir.join(format!(".{MANIFEST_FILE_NAME}.tmp"));
        let final_path = staging_dir.join(MANIFEST_FILE_NAME);
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| AppError::Export {
            code: CODE_IO,
            message: "manifest 序列化失败".into(),
        })?;
        {
            let mut f = std::fs::File::create(&tmp).map_err(|_| AppError::Export {
                code: CODE_IO,
                message: "写 manifest 失败".into(),
            })?;
            f.write_all(&bytes).map_err(|_| AppError::Export {
                code: CODE_IO,
                message: "写 manifest 失败".into(),
            })?;
            f.sync_all().map_err(|_| AppError::Export {
                code: CODE_IO,
                message: "manifest 落盘 fsync 失败".into(),
            })?;
        }
        std::fs::rename(&tmp, &final_path).map_err(|_| AppError::Export {
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
    fn source_serializes_structured_kind() {
        let s = ExportSource::Album {
            id: 12,
            name: "精选".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["kind"], "album");
        assert_eq!(v["id"], 12);
        assert_eq!(v["name"], "精选");
    }

    #[test]
    fn write_into_produces_valid_json_with_no_tmp_left() {
        let dir = tempfile::tempdir().unwrap();
        let items = vec![ManifestItem {
            file: "001-a.jpg".into(),
            root_alias: "照片库".into(),
            root_relative_path: "2025/a.jpg".into(),
            sort_index: 1,
            view_rotation: 90,
            rating: 4,
            color_label: 2,
            favorited: true,
            tags: vec!["家人".into()],
            albums: vec!["精选".into()],
        }];
        let m = Manifest::new(
            ExportSource::Selection,
            "2026-07-19T00:00:00Z".into(),
            items,
        );
        m.write_into(dir.path()).unwrap();
        let final_path = dir.path().join(MANIFEST_FILE_NAME);
        assert!(final_path.exists());
        assert!(!dir
            .path()
            .join(format!(".{MANIFEST_FILE_NAME}.tmp"))
            .exists());
        let text = std::fs::read_to_string(&final_path).unwrap();
        assert!(!text.contains("C:\\"), "manifest 不应含绝对路径痕迹");
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["formatVersion"], 1);
        assert_eq!(parsed["items"][0]["rootRelativePath"], "2025/a.jpg");
    }
}
