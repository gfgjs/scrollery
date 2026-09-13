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
        // key 是否存在：能取到密码即视为已设置（NoEntry → 未设置）。
        keyring_entry()
            .ok()
            .and_then(|e| e.get_password().ok())
            .is_some()
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?;
    Ok(ProofreadConfigDto {
        base_url,
        model,
        has_key,
    })
}

/// 持久化校对端点配置（base_url / model）（§5.4）。
///
/// A2:两键均为 schema 设置类,唯一真源已切到 config.toml——原 DB 写会被读侧
/// (`get_proofread_config`/`proofread_chunk`,已改走 ConfigManager)忽略,必须同步改走
/// `set_and_persist`。文件 IO 是阻塞操作,下沉 spawn_blocking(硬约束)。
#[tauri::command]
pub async fn set_proofread_config(
    base_url: String,
    model: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        s.config.set_and_persist("proofread_base_url", &base_url)?;
        s.config.set_and_persist("proofread_model", &model)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
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
            let key = keyring_entry()?.get_password().map_err(|_| {
                AppError::System("未设置 API Key | proofread API key not set".into())
            })?;
            Ok((b, m, key))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??;

    let cfg = ProofreadConfig { base_url, model };
    proofread_remote(&cfg, &key, &text).await
}
