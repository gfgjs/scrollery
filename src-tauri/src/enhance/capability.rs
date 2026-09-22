//! 增强能力的状态与执行准入共用同一判定，避免引导进入无法完成的下载或购买流程。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnhanceReadiness {
    Ready,
    WorkerMissing,
    ManifestUnready,
    Unlicensed,
    ModelMissing,
}

/// 安装与发行前提优先于授权引导；授权原始状态仍由 availability 单独提供。
pub fn readiness(
    authorized: bool,
    worker_ready: bool,
    manifest_ready: bool,
    installed: bool,
) -> EnhanceReadiness {
    if !worker_ready {
        EnhanceReadiness::WorkerMissing
    } else if !manifest_ready {
        EnhanceReadiness::ManifestUnready
    } else if !authorized {
        EnhanceReadiness::Unlicensed
    } else if !installed {
        EnhanceReadiness::ModelMissing
    } else {
        EnhanceReadiness::Ready
    }
}

impl EnhanceReadiness {
    /// IPC 使用稳定错误码；Ready 没有错误。
    pub fn ensure_ready(self) -> crate::error::Result<()> {
        let (code, message) = match self {
            Self::Ready => return Ok(()),
            Self::WorkerMissing => ("enhance_worker_missing", "增强组件缺失，请重新安装应用"),
            Self::ManifestUnready => ("enhance_manifest_unready", "增强模型发行清单尚未就绪"),
            Self::Unlicensed => ("enhance_unlicensed", "影像增强尚未授权或授权已过期"),
            Self::ModelMissing => (
                "enhance_model_missing",
                "增强模型文件缺失或大小不符，请重新下载",
            ),
        };
        Err(crate::error::AppError::Enhance {
            code,
            message: message.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_and_admission_agree_for_every_combination() {
        for bits in 0..16 {
            let authorized = bits & 1 != 0;
            let worker = bits & 2 != 0;
            let manifest = bits & 4 != 0;
            let installed = bits & 8 != 0;
            let actual = readiness(authorized, worker, manifest, installed);
            let expected = if !worker {
                EnhanceReadiness::WorkerMissing
            } else if !manifest {
                EnhanceReadiness::ManifestUnready
            } else if !authorized {
                EnhanceReadiness::Unlicensed
            } else if !installed {
                EnhanceReadiness::ModelMissing
            } else {
                EnhanceReadiness::Ready
            };
            assert_eq!(actual, expected);
            assert_eq!(actual.ensure_ready().is_ok(), bits == 15);
        }
    }
}
