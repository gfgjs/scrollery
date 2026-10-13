//! 存储后端 IPC（网络盘，P5 8B，§3.8）。
//!
//! 存储后端的 CRUD + 连通性测试。密码存系统 keyring（账户 `storage_backend_<id>`），绝不入 DB ——
//! 仅持久化 `cred_ref`（与校对 key 同模式）。`test_backend` 构建后端并列其 base 目录；轻量版
//! （无 `netfs`）测试 WebDAV 返回清晰的「需性能版」提示（降级到 8A）。

use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::db::models::StorageBackendInfo;
use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::storage::{count_entries, ConnParams};

use scrollery_plugin_api::KEYRING_SERVICE;

fn cred_account(id: i64) -> String {
    format!("storage_backend_{id}")
}

// keyring 是 OS 凭据库,其错误映射为 AppError::System(稳定 code=System,P1-8:此前裸 String)。
fn keyring_entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| AppError::internal("凭据库访问失败 | keyring access failed", e))
}

/// 来自添加/测试表单的连接参数。密码仅在内存（保存时 → keyring）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInput {
    pub kind: String, // 'local' | 'smb' | 'webdav'
    pub name: Option<String>,
    pub host: Option<String>,
    pub base_path: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl BackendInput {
    /// 连接测试参数：借用本结构字段，不做 clone 透传。
    fn conn_params(&self) -> ConnParams<'_> {
        ConnParams {
            host: self.host.as_deref(),
            base_path: self.base_path.as_deref(),
            username: self.username.as_deref(),
            password: self.password.as_deref(),
        }
    }
}

/// 列出所有已配置的存储后端（§3.8）。密码绝不返回。
#[tauri::command]
pub async fn list_backends(state: State<'_, Arc<AppState>>) -> Result<Vec<StorageBackendInfo>> {
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<Vec<StorageBackendInfo>> {
        let pool = s.db_read_pool.get()?;
        q::list_storage_backends(&pool)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 保存前测试后端的连通性/凭据（§3.8）。成功时返回 base 路径下的项数。在 `spawn_blocking` 运行
/// （WebDAV 客户端内部 block_on，不能套在异步运行时内）。
#[tauri::command]
pub async fn test_backend(input: BackendInput) -> Result<usize> {
    tokio::task::spawn_blocking(move || -> Result<usize> {
        count_entries(&input.kind, &input.conn_params())
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 添加存储后端（§3.8）：持久化行，再把密码存入 keyring（账户 `storage_backend_<id>`）并把该账户
/// 记为 `cred_ref`。返回已保存的行。
#[tauri::command]
pub async fn add_backend(
    input: BackendInput,
    state: State<'_, Arc<AppState>>,
) -> Result<StorageBackendInfo> {
    let name = input
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| input.host.clone())
        .unwrap_or_else(|| input.kind.clone());

    // R1-3：DB 写 + keyring 存密（同步系统调用）+ 回读，整段离开 tokio worker。
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<StorageBackendInfo> {
        let id = {
            let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            q::insert_storage_backend(
                &conn,
                &input.kind,
                &name,
                input.host.as_deref(),
                input.base_path.as_deref(),
                input.username.as_deref(),
                None, // cred_ref filled in below once we know the id
                None,
            )?
        };

        // 把密码存入 keyring 并经 cred_ref 关联（绝不入 DB）。
        if let Some(pw) = input.password.as_deref().filter(|p| !p.is_empty()) {
            let account = cred_account(id);
            keyring_entry(&account)?
                .set_password(pw)
                .map_err(|e| AppError::internal("写入凭据失败 | credential write failed", e))?;
            let conn = s.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute(
                "UPDATE storage_backends SET cred_ref = ?1 WHERE id = ?2",
                rusqlite::params![account, id],
            )?;
        }

        let backends = {
            let pool = s.db_read_pool.get()?;
            q::list_storage_backends(&pool)?
        };
        backends.into_iter().find(|b| b.id == id).ok_or_else(|| {
            AppError::System("backend not found after insert | 插入后未找到后端".into())
        })
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 移除存储后端（§3.8）：清理凭据成功后删除行，失败保留关联供重试。
#[tauri::command]
pub async fn remove_backend(id: i64, state: State<'_, Arc<AppState>>) -> Result<()> {
    // R1-3：DB 写 + keyring 清理整段离开 tokio worker。
    let s = Arc::clone(&state);
    tokio::task::spawn_blocking(move || remove_backend_with_entry(&s.db_writer, id, keyring_entry))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// 凭据操作位于两段短 DB 锁之间；失败时必须保留关联供同一操作重试。
fn remove_backend_with_entry(
    writer: &std::sync::Mutex<rusqlite::Connection>,
    id: i64,
    entry_for: impl FnOnce(&str) -> Result<keyring::Entry>,
) -> Result<()> {
    let cred_ref = {
        let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
        q::storage_backend_cred_ref(&conn, id)?
    };
    if let Some(account) = cred_ref {
        // 清理凭据；NoEntry 视为成功（幂等）。
        match entry_for(&account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                return Err(AppError::internal(
                    "删除凭据失败 | credential delete failed",
                    e,
                ))
            }
        }
    }
    let conn = writer.lock().unwrap_or_else(|e| e.into_inner());
    q::delete_storage_backend(&conn, id)
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    fn writer() -> std::sync::Mutex<rusqlite::Connection> {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE storage_backends (id INTEGER PRIMARY KEY, cred_ref TEXT);
            INSERT INTO storage_backends VALUES (1, 'storage_backend_1');",
        )
        .unwrap();
        std::sync::Mutex::new(conn)
    }

    fn remaining(writer: &std::sync::Mutex<rusqlite::Connection>) -> Option<String> {
        use rusqlite::OptionalExtension;
        writer
            .lock()
            .unwrap()
            .query_row(
                "SELECT cred_ref FROM storage_backends WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    }

    fn entry() -> keyring::Entry {
        keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()))
    }

    #[test]
    fn entry_failure_keeps_backend_reference() {
        let writer = writer();
        let result =
            remove_backend_with_entry(&writer, 1, |_| Err(AppError::System("unavailable".into())));
        assert!(result.is_err());
        assert_eq!(remaining(&writer).as_deref(), Some("storage_backend_1"));
    }

    #[test]
    fn delete_failure_keeps_reference_and_allows_retry() {
        let writer = writer();
        let failed = entry();
        failed.set_password("test-password").unwrap();
        failed
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::PlatformFailure(Box::new(
                std::io::Error::other("private credential"),
            )));
        assert!(remove_backend_with_entry(&writer, 1, |_| Ok(failed)).is_err());
        assert_eq!(remaining(&writer).as_deref(), Some("storage_backend_1"));
        let retry = entry();
        retry.set_password("test-password").unwrap();
        remove_backend_with_entry(&writer, 1, |account| {
            assert_eq!(account, "storage_backend_1");
            Ok(retry)
        })
        .unwrap();
        assert_eq!(remaining(&writer), None);
    }

    #[test]
    fn missing_credential_allows_backend_removal() {
        let writer = writer();
        remove_backend_with_entry(&writer, 1, |_| Ok(entry())).unwrap();
        assert_eq!(remaining(&writer), None);
    }
}
