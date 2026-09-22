//! 官方版授权与功能展示。摘要只汇集事实，不参与格式派发。

use crate::error::{AppError, Result};
use crate::exotic::coordinator::WakeReason;
use crate::exotic::{Availability, PluginEntitlement};
use crate::{official, state::AppState};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// 单项能力及其独立的资源准备状态。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOffering {
    pub id: String,
    pub name: String,
    pub paid: bool,
    pub builtin: bool,
    pub availability: Availability,
    pub resources: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// 官方版授权和所有可发现能力的同批摘要。
pub struct FeatureOfferings {
    pub entitlement: PluginEntitlement,
    pub features: Vec<FeatureOffering>,
}

/// 本机官方版授权，不因某个插件未安装而隐藏已购状态。
#[tauri::command]
pub async fn get_official_entitlement(
    state: State<'_, Arc<AppState>>,
) -> Result<PluginEntitlement> {
    let provider = state.entitlement_provider();
    tokio::task::spawn_blocking(move || official::entitlement(provider.as_ref()))
        .await
        .map_err(|e| AppError::internal("授权查询失败", e))?
}

/// 一次激活固定套装；token 只能验到编译期商品，客户端不能指定权益。
#[tauri::command]
pub async fn activate_official_license(
    token: String,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<()> {
    let provider = state.entitlement_provider();
    tokio::task::spawn_blocking(move || {
        let p = official::product();
        provider.activate(&p.product_id, &p.sku, token.trim(), official::now_secs())
    })
    .await
    .map_err(|e| AppError::internal("激活任务失败", e))?
    .map_err(|e| AppError::Exotic {
        code: e.code(),
        message: "官方版激活失败".into(),
    })?;
    state.wake_exotic(WakeReason::LicenseActivated);
    // 凭据已落盘，事件只刷新界面；发送失败不应谎报激活失败。
    if let Err(e) = app.emit("official-license-changed", ()) {
        tracing::warn!("授权状态通知失败: {e}");
    }
    Ok(())
}

/// 只移除本机凭据，不取消订单或删除任何插件/用户文件。
#[tauri::command]
pub async fn deactivate_official_license(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<()> {
    let provider = state.entitlement_provider();
    tokio::task::spawn_blocking(move || provider.deactivate(&official::product().product_id))
        .await
        .map_err(|e| AppError::internal("移除授权任务失败", e))?
        .map_err(|e| AppError::Exotic {
            code: e.code(),
            message: "移除本机授权失败".into(),
        })?;
    state.wake_exotic(WakeReason::ConfigChanged);
    if let Err(e) = app.emit("official-license-changed", ()) {
        tracing::warn!("授权状态通知失败: {e}");
    }
    Ok(())
}

/// 汇集编辑与 Catalog 能力，以及现有 OCR/增强资源状态；免费项不查询凭据。
#[tauri::command]
pub async fn list_feature_offerings(state: State<'_, Arc<AppState>>) -> Result<FeatureOfferings> {
    let entitlement = get_official_entitlement(state.clone()).await?;
    let ocr = super::ocr_commands::ocr_status(state.clone()).await?;
    let enhance = super::enhance_commands::enhance_status(state.clone()).await?;
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let mut features = vec![FeatureOffering {
            id: "feature-editing".into(),
            name: "图片高级编辑".into(),
            paid: true,
            builtin: true,
            availability: entitlement.availability,
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
                    // 资源描述与授权分开；授权优先级仍由既有 readiness 控制执行。
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
                paid: offering.license_tier != "free",
                builtin: offering.builtin,
                availability: resolution.availability,
                resources,
            });
        }
        features[1..].sort_by(|a, b| a.id.cmp(&b.id));
        FeatureOfferings {
            entitlement,
            features,
        }
    })
    .await
    .map_err(|e| AppError::internal("功能状态查询失败", e))
}
