//! 远程 AI 校对的 IPC（§5.4）。
//!
//! 配置（base_url / model）存 config.toml(A2 前存 app_config)；API key 存系统凭据库
//! （keyring），不落明文 DB。校对按文本分块由前端逐块调用 `proofread_chunk`，结果以
//! track-changes 呈现、接受后存为新版本（接 §5.3）。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::proofread::{proofread_remote, ProofreadConfig};
use crate::state::AppState;

/// keyring 服务名 / 账户名 —— API key 的存放坐标。
use scrollery_plugin_api::KEYRING_SERVICE;
const KEYRING_ACCOUNT: &str = "proofread_api_key";

// keyring 是 OS 凭据库,其错误映射为 AppError::System(稳定 code=System,P1-8:此前裸 String)。
fn keyring_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| AppError::internal("凭据库访问失败 | keyring access failed", e))
}

// 两个读取入口共用分类，避免配置探测与实际校对给出不同判断。
fn read_proofread_key(entry: Result<keyring::Entry>) -> Result<Option<String>> {
    match entry?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(AppError::internal(
            "读取凭据失败 | credential read failed",
            error,
        )),
    }
}

/// 暴露给 UI 的校对配置。key 本身绝不返回 —— 仅返回是否已设置。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofreadConfigDto {
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
}

/// 读取校对配置（base_url / model / 是否已存 key）（§5.4）。
///
/// A2:base_url/model 均为 schema 设置类键,唯一真源已切到 `ConfigManager`(内存读);
/// keyring 探测仍是同步系统调用,留在 spawn_blocking。
#[tauri::command]
pub async fn get_proofread_config(state: State<'_, Arc<AppState>>) -> Result<ProofreadConfigDto> {
    let base_url = state.config.get("proofread_base_url").unwrap_or_default();
    let model = state.config.get("proofread_model").unwrap_or_default();
    let has_key = tokio::task::spawn_blocking(|| {
        read_proofread_key(keyring_entry()).map(|key| key.is_some())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;
    Ok(ProofreadConfigDto {
        base_url,
        model,
        has_key,
    })
}

/// 持久化校对端点配置（base_url / model）（§5.4）。
///
/// 两键均为设置类,唯一真源是 config.toml。提交走**统一设置入口**(一次批量写盘 + 串行门 +
/// 统一广播),不再逐键直接落盘——同一份改动应只写一次文件,且与其他设置写入串行。
#[tauri::command]
pub async fn set_proofread_config(
    app: tauri::AppHandle,
    base_url: String,
    model: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 本操作不涉及耗时工作,代次在提交前读取即可(仍随请求回传,重置后的迟到写入会被拒)。
    let generation = state.config.generation();
    let patch = std::collections::BTreeMap::from([
        ("proofread_base_url".to_string(), base_url),
        ("proofread_model".to_string(), model),
    ]);
    crate::config::settings::submit_settings_patch(&app, state.inner(), patch, generation)
        .await
        .map(|_| ())
}

/// 把 API key 存入系统凭据库（绝不入 DB）（§5.4）。
#[tauri::command]
pub async fn set_proofread_key(key: String) -> Result<()> {
    keyring_entry()?
        .set_password(&key)
        .map_err(|e| AppError::internal("写入凭据失败 | credential write failed", e))
}

/// 删除已存的 API key（§5.4）。
#[tauri::command]
pub async fn clear_proofread_key() -> Result<()> {
    match keyring_entry()?.delete_credential() {
        Ok(()) => Ok(()),
        // 未设置时删除视为成功（幂等）。
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::internal(
            "删除凭据失败 | credential delete failed",
            e,
        )),
    }
}

/// 经配置的远程 LLM 校对一段文本（§5.4）。前端分块后逐块调用，再以 track-changes 呈现差异供接受。
#[tauri::command]
pub async fn proofread_chunk(text: String, state: State<'_, Arc<AppState>>) -> Result<String> {
    // A2:base_url/model 已迁往 config.toml(内存读);keyring 取 key 仍是同步系统调用,离开
    // tokio worker（其后的远程调用本就是 async IO）。
    let b = state.config.get("proofread_base_url").unwrap_or_default();
    let m = state.config.get("proofread_model").unwrap_or_default();
    if b.trim().is_empty() {
        return Err(AppError::System(
            "未配置校对服务地址 | proofread base_url not set".into(),
        ));
    }
    let (base_url, model, key) =
        tokio::task::spawn_blocking(move || -> Result<(String, String, String)> {
            let key = read_proofread_key(keyring_entry())?.ok_or_else(|| {
                AppError::System("未设置 API Key | proofread API key not set".into())
            })?;
            Ok((b, m, key))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    let cfg = ProofreadConfig { base_url, model };
    proofread_remote(&cfg, &key, &text).await
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    fn entry() -> keyring::Entry {
        keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()))
    }

    #[test]
    fn absent_key_is_none() {
        assert!(read_proofread_key(Ok(entry())).unwrap().is_none());
    }

    #[test]
    fn stored_key_remains_available() {
        let entry = entry();
        entry.set_password("test-key").unwrap();
        assert_eq!(
            read_proofread_key(Ok(entry)).unwrap(),
            Some("test-key".into())
        );
    }

    #[test]
    fn keyring_creation_failure_is_not_absence() {
        assert!(read_proofread_key(Err(AppError::System("keyring unavailable".into()))).is_err());
    }

    #[test]
    fn credential_read_failure_is_reported_without_secret_details() {
        let entry = entry();
        entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::PlatformFailure(Box::new(
                std::io::Error::other("private secret"),
            )));
        let error = read_proofread_key(Ok(entry)).unwrap_err();
        let payload = serde_json::to_value(error).unwrap();
        assert_eq!(payload["code"], "System");
        assert!(payload["message"]
            .as_str()
            .unwrap()
            .contains("credential read failed"));
        assert!(!payload.to_string().contains("private secret"));
    }
}
