//! 免费激活与功能展示；激活状态不替代插件安装和模型准备。
use crate::error::{AppError, Result};
use crate::exotic::coordinator::WakeReason;
use crate::exotic::Availability;
use crate::{feature_activation, state::AppState};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// 查询单项免费激活，不读取购买凭据或系统凭据库。
#[tauri::command]
pub async fn get_feature_activation(
    feature_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::feature_activation::FeatureActivation> {
    super::blocking::read_blocking(&state, move |conn| {
        crate::feature_activation::status(conn, &feature_id)
    })
    .await
}

/// 点击即保存单项激活记录，成功后通知其他窗口刷新。
#[tauri::command]
pub async fn activate_feature(
    feature_id: String,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<crate::feature_activation::FeatureActivation> {
    let activation = super::blocking::write_blocking(&state, move |conn| {
        crate::feature_activation::activate(conn, &feature_id)
    })
    .await?;
    state.wake_exotic(WakeReason::FeatureActivated);
    if let Err(error) = app.emit("feature-activation-changed", &activation) {
        tracing::warn!("免费激活状态通知失败: {error}");
    }
    Ok(activation)
}

/// 单项功能的独立激活状态与资源准备情况。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOffering {
    pub id: String,
    pub name: String,
    pub activation_required: bool,
    pub activated: bool,
    pub builtin: bool,
    pub availability: Availability,
    pub resources: &'static str,
}

/// 同批功能快照。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOfferings {
    pub features: Vec<FeatureOffering>,
}

/// 汇集编辑和 Catalog 功能；资源未就绪时依然可以记录免费激活。
#[tauri::command]
pub async fn list_feature_offerings(state: State<'_, Arc<AppState>>) -> Result<FeatureOfferings> {
    let ocr = super::ocr_commands::ocr_status(state.clone()).await?;
    let enhance = super::enhance_commands::enhance_status(state.clone()).await?;
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<FeatureOfferings> {
        let conn = state.db_read_pool.get().map_err(AppError::from)?;
        let editing = feature_activation::status(&conn, "feature-editing")?;
        let mut features = vec![FeatureOffering {
            id: editing.feature_id,
            name: "图片高级编辑".into(),
            activation_required: true,
            activated: editing.activated,
            builtin: true,
            availability: if editing.activated {
                Availability::Ready
            } else {
                Availability::Inactive
            },
            resources: "ready",
        }];
        let host = state.exotic_host();
        let snap = state.exotic_catalog.snapshot();
        let mut seen = std::collections::HashSet::new();
        for (format, offering) in snap.iter_formats() {
            if !seen.insert(offering.plugin_id.clone()) {
                continue;
            }
            let resolution = host.resolve_format(format);
            let activation_required = feature_activation::requires_activation(&offering.plugin_id);
            let activated = activation_required
                && feature_activation::status(&conn, &offering.plugin_id)?.activated;
            let resources = match offering.plugin_id.as_str() {
                "exotic-ocr" if crate::ai::worker_client::ai_worker_exe().is_err() => {
                    "workerMissing"
                }
                "exotic-ocr" => match ocr.tiers.iter().find(|t| t.id == ocr.active_tier) {
                    Some(t) if !t.manifest_ready => "manifestUnready",
                    Some(t) if t.installed => "ready",
                    Some(_) => "modelMissing",
                    None => "manifestUnready",
                },
                "exotic-enhance" => {
                    // 资源描述与激活分开，执行入口仍检查模型和组件就绪情况。
                    if enhance.models.is_empty() || enhance.models.iter().any(|m| !m.manifest_ready)
                    {
                        "manifestUnready"
                    } else if !enhance.worker_ready {
                        "workerMissing"
                    } else if enhance.models.iter().all(|m| m.installed) {
                        "ready"
                    } else {
                        "modelMissing"
                    }
                }
                _ if !offering.builtin && resolution.installed_version.is_none() => "needsInstall",
                _ if offering.builtin => "included",
                _ => "ready",
            };
            features.push(FeatureOffering {
                id: offering.plugin_id.clone(),
                name: offering.display_name.clone(),
                activation_required,
                activated,
                builtin: offering.builtin,
                availability: resolution.availability,
                resources,
            });
        }
        features[1..].sort_by(|a, b| a.id.cmp(&b.id));
        Ok(FeatureOfferings { features })
    })
    .await
    .map_err(|e| AppError::internal("功能状态查询失败", e))?
}
