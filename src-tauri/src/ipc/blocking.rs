//! R1-3 · rusqlite 下沉 blocking 线程池的共享助手（全 ipc/ 命令族复用）。
//!
//! CLAUDE.md 硬化条款：async command 内的任何 rusqlite 调用——包括「看起来很快」的读——
//! 一律 `spawn_blocking`，不做逐条估时豁免（SQLite 同步跑在 tokio worker 上会拖垮并发 IPC）。
//! 闭包收 `&rusqlite::Connection`（读池连接经 Deref 强转），既有 `q::*` 查询零改动直接复用；
//! 复杂命令（多段读写混排 / 文件 IO 交织）不强套本助手，可自建 `spawn_blocking` 块（同规则）。

use std::sync::Arc;

use tauri::State;

use crate::error::{AppError, Result};
use crate::state::AppState;

/// 只读查询下沉 blocking 线程池（读池连接在闭包期间持有、返回即还池）。
pub async fn read_blocking<T, F>(state: &State<'_, Arc<AppState>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
{
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let pool = state_arc.db_read_pool.get().map_err(AppError::from)?;
        f(&pool)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 写查询下沉 blocking 线程池（db_writer 互斥锁在 blocking 线程上等待/持有）。
pub async fn write_blocking<T, F>(state: &State<'_, Arc<AppState>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
{
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        // 锁中毒即恢复(into_inner),与全库 db_writer 获取点统一(审查 R11):不因一次 panic
        // 永久 brick 所有 DB 写——SQLite 事务原子,持锁期 panic 会回滚,恢复出的连接仍可用。
        // 旧实现返回 System 错误(永久失败臂),与 thumbnail(静默跳过)、ai/face(into_inner)三套并存。
        let conn = state_arc
            .db_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        f(&conn)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}
