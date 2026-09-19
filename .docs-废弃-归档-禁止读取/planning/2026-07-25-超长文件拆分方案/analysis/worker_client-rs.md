---
id: 2026-07-25-worker_client-rs
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# 拆分分析:src-tauri/src/ai/worker_client.rs

> 只读分析,不改代码。目标文件现状 1697 行 / 71628 字节(≈72KB,与榜单口径一致)。锚点以符号名为主,行号区间为辅(并行会话正在精简注释,行号会漂移)。

## 0. 结论先行

本文件不是"进程管理/协议/重试/日志汇入"四组均质分布——`日志汇入`(WorkerLogLine 行协议等)不在本文件内,那是 `span-worker-log-line` 线的产物,归属别处;本文件实测只有**三个逻辑组 + 一个测试模块**,且测试模块占比过半(883 行 / 34911 字节 ≈ 49%)。故本方案第一优先级永远是"先搬测试",而不是先拆逻辑。

## 1. 现状结构图

按物理顺序 + 职责归组(行号为当前读到的区间,已知会因并行注释精简线漂移):

| 分组 | 符号锚点 | 约行数 | 约字节 | 职责 |
|---|---|---|---|---|
| 模块文档+导入+常量 | 顶部 doc comment、`use` 块、`HANDSHAKE_TIMEOUT`/`SHUTDOWN_GRACE`/`MAX_ATTEMPTS`/`RETRY_BACKOFF` | L1–51 | ~2.3K | 错误恢复契约的文字化(硬止损=重试一次)+ 常量 |
| **核心类型** | `SessionSpec`(+`matches`)、`OcrSessionSpec`、`ShaEntry`、`Spawner`(类型别名)、`EnsureError`、`AiWorkerClient`(字段) | L53–131 | ~2.8K | 会话规格/会话匹配语义(超集放宽 D-face 裁决)+ sha 备忘条目 + 失败二分枚举 + 句柄本体字段 |
| **A. 进程管理** | `AiWorkerClient::new`/`with_spawner`/`session`/`drop_worker`/`ensure_worker`;自由函数 `ai_worker_exe`/`spawn_ai_worker` | L133–181, L597–633 | ~3.3K | worker 子进程生命周期:spawn/存活探测/体面弃用(`SHUTDOWN_GRACE`)/exe 路径解析(`PICASA_AI_WORKER_PATH` 覆盖) |
| **B. 会话协议组装** | `ensure_session`/`ensure_ocr_session`(方法);`build_session_init`/`build_ocr_session_init`/`model_descriptor`/`build_session_spec`/`build_ocr_session_spec`(自由函数) | L183–240, L409–467, L635–813 | ~13.1K | CLIP 会话与 OCR 会话(D-OCR-1 双槽独立)按 `SessionSpec`/`OcrSessionSpec` 与 worker 快照比对、不符则 close→init;sha256 备忘防 GB 级文件重算;从 `AppState` 组装规格 |
| **C. 批请求重试分发** | `run_validated`(方法)、`embed_batch`/`encode_text`/`face_detect_embed`/`ocr_batch`/`close_session`(pub 方法) | L242–407, L469–588 | ~13.3K | `MAX_ATTEMPTS=2` 硬止损 attempt 圈:进程级(Process)弃实例重试、`SessionExpired` 清快照重 init、retryable 退避重发、terminal 直接冒泡;`ocr_batch` 是与 `run_validated` **同构但独立**的第二套 attempt 圈(注释明确"不复用 run_validated") |
| **D. 测试** | `#[cfg(test)] mod tests { .. }`:`MockEmbedWorker` + 18 个 `#[test]` | L815–1697 | ~34.9K(占全文件 49%) | mock spawner 脚本化;覆盖 W1(进程级 init 异常自愈)/W2(retryable 退避重发)/W3(batch_size 增缩切换)/face 超集放宽/session_expired/terminal 不重试/OCR 五项回归 |

关键观察:A/B/C 三组不是清晰分层——`run_validated`/`ocr_batch`(C)直接调用 `ensure_session`/`ensure_ocr_session`(B),后两者又直接调用 `ensure_worker`/`drop_worker`(A)。这是同一个 attempt-圈状态机的三层嵌套调用,不是独立子系统,拆分必然产生跨组私有方法调用。

## 2. 拆分方案

### 目标结构(目录化模块,5 个文件)

```
src-tauri/src/ai/worker_client/
├── mod.rs        原地保留:模块文档、常量、核心类型(SessionSpec/OcrSessionSpec/
│                 ShaEntry/Spawner/EnsureError)、AiWorkerClient 结构体定义 + Default;
│                 声明四个子模块;pub use 回导出对外自由函数
├── process.rs    A组:new/with_spawner/session/drop_worker/ensure_worker
│                 + ai_worker_exe/spawn_ai_worker
├── session.rs    B组:ensure_session/ensure_ocr_session
│                 + build_session_init/build_ocr_session_init/model_descriptor
│                 + build_session_spec/build_ocr_session_spec
├── dispatch.rs   C组:run_validated/embed_batch/encode_text/face_detect_embed
│                 /ocr_batch/close_session
└── tests.rs      D组:#[cfg(test)] mod tests 原样搬入(内容不改一行)
```

`mod.rs` 内 `impl AiWorkerClient` 只留结构体定义与 `Default`,四组方法各自在自己文件里开独立 `impl AiWorkerClient { .. }` 块——Rust 同 crate 内 inherent impl 允许跨文件多块,这是本方案成立的前提。

### use 路径保持(外部调用点核实,见下"依赖方向"表)

- `pub struct AiWorkerClient` / `pub struct SessionSpec` / `pub struct OcrSessionSpec` 定义留在 `mod.rs`,`crate::ai::worker_client::{AiWorkerClient, SessionSpec, OcrSessionSpec}` 路径不变。
- 方法(`embed_batch`/`encode_text`/`face_detect_embed`/`ocr_batch`/`close_session`/`session`)是 `AiWorkerClient` 的 inherent 方法,无论物理定义在哪个子文件,调用方 `client.method(..)` 语法不变、**不需要任何 `pub use` 回导出**。
- 自由函数需在 `mod.rs` 显式回导出,保持路径:
  - `pub use process::ai_worker_exe;`(`worker_e2e.rs`、`enhance/service.rs` 注释引用)
  - `pub use session::{build_session_spec, build_ocr_session_spec};`(`ipc/ai_commands.rs:202`、`ai/face_pipeline.rs:670`、`ipc/ocr_commands.rs:26,261,296`)

### 依赖方向 / 可见性改动清单(唯一的机械改动)

跨子模块私有调用需要把可见性从"模块私有"升到 `pub(super)`(仅在 `worker_client` 子树内可见,不外泄到 crate 其余部分,`AppError`/pub API 边界不变)。逐项核实如下:

| 符号 | 定义于 | 被谁跨文件调用 | 需要的可见性 |
|---|---|---|---|
| `ensure_worker` | process.rs | session.rs::`ensure_session`/`ensure_ocr_session` | `pub(super) fn` |
| `drop_worker` | process.rs | dispatch.rs::`run_validated`/`ocr_batch` | `pub(super) fn` |
| `ensure_session` | session.rs | dispatch.rs::`run_validated` | `pub(super) fn` |
| `ensure_ocr_session` | session.rs | dispatch.rs::`ocr_batch` | `pub(super) fn` |
| `build_session_init` | session.rs | tests.rs(3 处直调,校验 sha 备忘/models 组装) | `pub(super) fn` |

其余全部**不需要**改可见性:
- `AiWorkerClient` 私有字段(`worker`/`spawner`/`sha_cache`/`next_session_id`/`ocr_loaded`)定义在 `mod.rs`,`process.rs`/`session.rs`/`dispatch.rs` 都是其子模块——Rust 隐私规则「私有项对定义模块及其所有后代可见」天然覆盖,零改动。
- `SessionSpec::matches`(私有方法,定义于 mod.rs)被 session.rs 调用——同理天然可见。
- `EnsureError`/`ShaEntry`/常量——定义于 mod.rs,四个子模块都是后代,天然可见。
- `build_ocr_session_init`/`model_descriptor`——只在 session.rs 内部互调,不跨文件,维持全私有。
- `tests.rs` 内 `use super::*` 继续成立(它仍是 `worker_client`/mod.rs 的直接子模块,拿到的是 mod.rs 命名空间下一切可见项,和今天同源同构)。

## 3. 风险与不变量

- **进程生命周期状态机不可拆散语义**:`drop_worker` 清 `ocr_loaded`(注释标"失效点①")与 `ensure_worker` 首次 spawn 路径再次清 `ocr_loaded`(注释标"失效点②")是两处刻意的冗余复位,覆盖不同触发路径(换代 vs 首次实例化)。移动到 `process.rs` 时必须整体保留,不得以"看似重复"为由合并/删除任一处。
- **std Mutex 跨 await 红线**:本文件所有方法均为同步/阻塞(无 `async fn`、无 `.await`)。真正的 `std::sync::Mutex<AiWorkerClient>` 守卫在 `state.rs`(`use std::sync::{Arc, Mutex, RwLock}` 已核实),由各调用点(`worker_pipeline.rs`/`face_pipeline.rs`/`ipc/ai_commands.rs`/`ipc/ocr_commands.rs`)在同步代码块内取锁、`ocr_commands.rs` 模块文档明确写"全程在 spawn_blocking 闭包内,不跨 .await 持锁"。拆分**绝不能**给任何搬移后的方法加 `async`/引入 `.await`,否则会破坏调用方"锁全程同步"的外部契约。
- **错误类型契约**:`EnsureError`(Process/Terminal 二分)是纯内部枚举,不 `pub`、不跨越对外 API 边界,在 `run_validated`/`ocr_batch` 处收敛转换为 `AppError`/`Result`——拆分后仍需保持它是 `worker_client` 子树内部类型,不得因为"跨文件用着方便"而放宽成 `pub(crate)` 对外暴露。`AppError::Ocr{code:"ocr_model_missing"}` 的字符串 code(2026-07-23 深审#1 裁决,`ipc/ocr_commands.rs` 依赖该 code 分流前端引导重下载)必须原样保留,不随文件移动改名。
- **MAX_ATTEMPTS 归零预算红线(用户特别红线,已确认适用本文件)**:`run_validated` 与 `ocr_batch` 各自持有独立的 `for attempt in 1..=MAX_ATTEMPTS` 计数,是**逐次调用重新归零**的预算,不是跨调用累积的。两者结构高度同构(`ocr_batch` 注释自陈"不复用 run_validated"),表面看是可提炼的重复代码,但本方案**不建议合并/抽公共泛型重试器**——那已超出"纯结构移动"范畴,属于语义相关的重构,与用户红线冲突,故只原样搬入 `dispatch.rs`,不做去重。
- **泄漏隔离故意设计红线**:worker 以独立子进程运行(T16 起 host 恒 worker-only,进程内 ort 整段已删)本身就是刻意的资源隔离设计(DirectML/ort 潜在泄漏由进程边界兜底,Supervisor kill + 重建是设计的一部分)。拆分**不得**顺带建议"合并回进程内避免 IPC 开销"或"池化多个 worker 常驻避免重建成本"之类改变隔离语义的方案。
- **D-OCR-1 双槽独立不变量**:CLIP 会话(`worker.session()`/`SessionDescriptor`)与 OCR 会话(`ocr_loaded: Option<String>`)是同一 worker 进程内两个独立槽位,互不干扰。拆分后 `session.rs` 内 `ensure_session`(CLIP)与 `ensure_ocr_session`(OCR)仍需保持互不触碰对方状态——当前代码已如此,只需在搬移时逐行核对没有意外交叉引用。
- **测试脚本行为对拍**:18 个 `#[test]` 全部依赖 `MockEmbedWorker` 的 `alive`/`session`/`init_script`/`batch_script` 精确时序(尤其 W1/W2/W3 三个回归),纯移动文件不改内容即可保持通过;唯一实质改动是把 `build_session_init` 的可见性从私有升到 `pub(super)`,这是构建期可见性问题而非运行期行为改动。

## 4. 收益与优先级

拆后预估(字节数按当前行区间实测折算,未来注释精简线落地后会整体再降):

| 文件 | 预估大小 | 相对原文件 |
|---|---|---|
| mod.rs | ~7.0KB | 9.8% |
| process.rs | ~3.3KB | 4.6% |
| session.rs | ~13.1KB | 18.3% |
| dispatch.rs | ~13.3KB | 18.6% |
| tests.rs | ~34.9KB | 48.7% |

五文件合计与原文件基本相等(结构移动,无新增/删减逻辑)。除 `tests.rs`(测试代码,阅读成本模型不同于生产逻辑)外,四个逻辑文件全部落在 20KB 以内,单文件职责单一(进程/协议/重试/类型定义分离),符合"组件单一职责"工程默认。

施工顺序建议(风险从低到高排列,每步应可独立编译验证):

1. **第一步(最高收益、零可见性改动)**:仅把 `#[cfg(test)] mod tests { .. }` 整块搬到新建的 `worker_client/tests.rs`,原文件改名目录化为 `worker_client/mod.rs`,`mod tests;` 声明照旧、内容一字不改。此步已经吃掉 49% 体积,且因 `tests` 相对 `mod.rs` 的父子关系不变,`use super::*` 无需任何调整。
2. **第二步**:抽 `process.rs`(A 组),`ensure_worker`/`drop_worker` 标 `pub(super)`。
3. **第三步**:抽 `session.rs`(B 组),`ensure_session`/`ensure_ocr_session`/`build_session_init` 标 `pub(super)`,`build_session_spec`/`build_ocr_session_spec` 在 `mod.rs` `pub use` 回导出。
4. **第四步**:抽 `dispatch.rs`(C 组),此时 process.rs/session.rs 已就位,`dispatch.rs` 直接引用其 `pub(super)` 方法即可,无需再改可见性。
5. **收尾**:`mod.rs` 只留常量/核心类型/结构体定义/`Default`/四个 `mod` 声明/两个 `pub use`,核对全文件无遗留死代码。

## 5. 验证策略

- **编译门槛(每步后必跑)**:`cargo check -p scrollery`(crate 名已核实为 `scrollery`,`src-tauri/Cargo.toml`);重点看是否有 `private field/method` 可见性报错——若有说明第 2 节可见性清单有遗漏,照报错逐条补 `pub(super)`,不做过度放宽(不升级到 `pub(crate)`)。
- **静态检查**:`cargo clippy -p scrollery --all-targets`(`--all-targets` 覆盖 `#[cfg(test)]`,确认 `tests.rs` 单独文件后 clippy 仍能发现同样告警,无遗漏)。
- **单测**:`cargo test -p scrollery ai::worker_client`(或按新模块路径 `ai::worker_client::tests::`)跑本文件 18 个既有测试,**必须全绿且用例数不变**——这是本次纯结构移动的核心回归证据,尤其 W1/W2/W3 三个回归与 OCR 五项回归。
- **涉面单测(跨文件调用点)**:`worker_pipeline.rs`/`face_pipeline.rs`/`ipc/ai_commands.rs`/`ipc/ocr_commands.rs`/`bin/worker_e2e.rs` 均以路径 `crate::ai::worker_client::{AiWorkerClient, SessionSpec, OcrSessionSpec, build_session_spec, build_ocr_session_spec, ai_worker_exe}` 引用——拆分后应在这 5 个文件所在 crate 目标上跑 `cargo check` 确认零改动(它们不该出现任何编译差异,因为对外路径设计上保持不变)。
- **不要求新增测试**:本次是纯结构移动,不改变可测行为,不新增测试用例;若第 2 节可见性调整导致任何既有测试需要改 `use` 路径(例如 `use super::session::build_session_init;` 而非纯 `use super::*;`),视为构建期调整,不算行为变更。

## 顺手发现

无。
