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

#[cfg(test)]
mod tests {
    /// 「最近标记」启发式的三态：见测试正文说明。
    #[derive(PartialEq, Clone, Copy, Debug)]
    enum Marker {
        /// 尚无标记 / 位于 async fn 正文（在此处直查 = 违规）。
        AsyncBody,
        /// 位于 spawn_blocking / read_blocking / write_blocking / thread::spawn 之后。
        Blocking,
        /// 位于同步 fn 正文（同步助手的调用方自证 blocking 上下文，如 active_profile）。
        SyncFn,
    }

    /// R1-3 回归门（CLAUDE.md 硬化条款的 tripwire）：扫描范围内任何 rusqlite 连接**获取调用**
    /// （`db_writer.lock` / `db_read_pool.get`）都必须出现在 spawn_blocking / read_blocking /
    /// write_blocking / thread::spawn 标记**之后**；若「最近的上游标记」是 `async fn` 签名，
    /// 即判为「SQL 直跑 tokio worker」违规。
    ///
    /// 2026-07-06 审查 P1-5:扫描范围从 `src/ipc` 扩到 `state.rs` + `scanner/volume_watch.rs`
    /// （原「法外之地」——quiesce/resume 与卷对账都曾在 async 正文直跑 SQL）。相应地把匹配从
    /// 「裸标识符」收紧为「`.lock`/`.get` 访问调用」,避免 state.rs 的字段声明 `pub db_writer:
    /// DbWriter`、类型标注、以及 `db_read_pool.clone()`（传递池句柄,非查询）造成误报。
    ///
    /// 这是逐行扫描的启发式而非完备的语法证明：闭包结束后回到 async 正文的直查可能漏报,
    /// 但最常见的回归形态——新命令在 async 正文顶部直接拿连接查询——必然立刻红。
    #[test]
    fn ipc_commands_keep_rusqlite_off_async_workers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // 扫描目标：ipc/ 全部 + 两个曾漏网的单文件。
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for entry in std::fs::read_dir(root.join("ipc")).expect("read src/ipc") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(path);
            }
        }
        files.push(root.join("state.rs"));
        files.push(root.join("scanner/volume_watch.rs"));
        // 2026-07-10 审查 B3/B4:两个新漏网点入扫。lib.rs 的后台任务是 `spawn(async move {` 块
        // (非 async fn),须配合下方 `spawn(async` 标记规则才能进入 AsyncBody 判定。
        files.push(root.join("lib.rs"));
        files.push(root.join("exotic/coordinator.rs"));

        let mut violations: Vec<String> = Vec::new();
        for path in files {
            let src = std::fs::read_to_string(&path).expect("read source");
            let file = path.file_name().unwrap().to_string_lossy().to_string();

            let mut marker = Marker::AsyncBody;
            for (idx, raw) in src.lines().enumerate() {
                // 去掉行注释（`//` 之后），避免注释里提到 db_writer 造成误报；
                // 行内 `https://` 也会被截断,但截断只影响其后文本,不影响本判定。
                let line = raw.split("//").next().unwrap_or("");

                // 标记优先级：blocking 入口 > async fn 签名 > 同步 fn 签名。
                if line.contains("spawn_blocking")
                    || line.contains("read_blocking(")
                    || line.contains("write_blocking(")
                    || line.contains("write_blocking_str(")
                    || line.contains("thread::spawn")
                {
                    marker = Marker::Blocking;
                } else if line.contains("async fn ") || line.contains("spawn(async") {
                    // `spawn(async move {` 块与 async fn 同样跑在 tokio worker 上(2026-07-10
                    // 审查 B3:lib.rs 后台任务曾以此形态直跑 SQL 而不被本测抓到)。
                    marker = Marker::AsyncBody;
                } else if line.contains("fn ")
                    && (line.trim_start().starts_with("fn ")
                        || line.trim_start().starts_with("pub fn ")
                        || line.trim_start().starts_with("pub(crate) fn ")
                        || line.trim_start().starts_with("pub(super) fn "))
                {
                    marker = Marker::SyncFn;
                }

                // concat! 拆分字面量：避免本测试自身源码命中扫描（自匹配误报）。
                // 只认「获取连接」的访问调用,不认字段声明/类型标注/句柄 clone。
                if (line.contains(concat!("db_", "writer.lock"))
                    || line.contains(concat!("db_", "read_pool.get")))
                    && marker == Marker::AsyncBody
                {
                    violations.push(format!("{}:{} → {}", file, idx + 1, raw.trim()));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "以下位置疑似在 tokio worker 上直跑 rusqlite（R1-3 硬化条款）。\n\
             若为误报（如闭包结束后的合法用法），请重构使 SQL 进入 blocking 闭包，\n\
             或调整本启发式并说明理由：\n{}",
            violations.join("\n")
        );
    }
}
