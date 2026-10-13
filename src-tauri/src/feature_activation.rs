//! 逐项免费激活记录。与首次引导标记一样保存为本地应用状态，不是购买凭据。

use rusqlite::Connection;
use serde::Serialize;

use crate::db::queries::{get_config, set_config};
use crate::error::{AppError, Result};

/// 需要用户逐项确认免费激活的功能。
pub const FEATURE_IDS: [&str; 4] = [
    "feature-editing",
    "exotic-ocr",
    "exotic-enhance",
    "exotic-image-psd",
];

/// 单项激活状态；安装、模型和平台准备情况由各功能另行提供。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureActivation {
    pub feature_id: String,
    pub activated: bool,
}

/// 插件宿主读取本地激活记录；运行期使用 DB，隔离测试注入独立状态源。
pub trait ActivationSource: Send + Sync {
    /// 查询指定功能是否已在本机免费激活。
    fn is_activated(&self, feature_id: &str) -> Result<bool>;
}

/// 判断功能是否需要单独的免费激活。
pub fn requires_activation(feature_id: &str) -> bool {
    FEATURE_IDS.contains(&feature_id)
}

/// 功能执行前读取同一状态；无需任何许可证或联网校验。
pub fn require_activated(conn: &Connection, feature_id: &str) -> Result<()> {
    if status(conn, feature_id)?.activated {
        Ok(())
    } else {
        Err(AppError::Exotic {
            code: "feature_inactive",
            message: "请先免费激活此功能".into(),
        })
    }
}

fn state_key(feature_id: &str) -> Result<&'static str> {
    match feature_id {
        "feature-editing" => Ok("feature_editing_activated"),
        "exotic-ocr" => Ok("feature_ocr_activated"),
        "exotic-enhance" => Ok("feature_enhance_activated"),
        "exotic-image-psd" => Ok("feature_psd_activated"),
        _ => Err(AppError::Exotic {
            code: "unknown_feature",
            message: "此功能不需要免费激活".into(),
        }),
    }
}

/// 从内部状态表读取单项记录；没有记录即尚未激活。
pub fn status(conn: &Connection, feature_id: &str) -> Result<FeatureActivation> {
    let activated = get_config(conn, state_key(feature_id)?)?.as_deref() == Some("true");
    Ok(FeatureActivation {
        feature_id: feature_id.to_owned(),
        activated,
    })
}

/// 保存单项免费激活；写入失败直接返回错误，不对外报告成功。
pub fn activate(conn: &Connection, feature_id: &str) -> Result<FeatureActivation> {
    set_config(conn, state_key(feature_id)?, "true")?;
    Ok(FeatureActivation {
        feature_id: feature_id.to_owned(),
        activated: true,
    })
}
