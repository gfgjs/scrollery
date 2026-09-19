# 冷门格式插件子系统 v3.1 · Part 2 引擎（P3–P4）

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 范围：协议 crate、PSD Worker、WorkerSupervisor、Coordinator、任务 Pipeline、Sink。
> 前置：Part1 全部完成；PSD probe 已冻结首发格式与能力。
> 版本：v3.1（已并入 `exotic_format_plugin_plan_v3.1_addendum.md` 的 R1/R2/R4/R5/R6/R8）。
> 本卷用 debug Cargo feature 注入测试授权；不依赖正式 Registry/License。

## 本卷完成定义

1. 协议只有一种帧格式；有 request_id、长度上限、握手、稳定错误码。
2. stdout 污染、stderr 洪水、超长帧、响应错序均被 Host 拒绝且不阻塞。
3. PSD Worker 对首发样本矩阵生成有效 WebP；畸形输入返回结构化错误。
4. Worker 超时/崩溃后当前任务按策略重试，进程被回收，池容量恢复。
5. 扫描、Catalog backfill、测试激活、重试时钟均可幂等唤醒 Pipeline。
6. 产物临时写入、验证、原子替换；成功后同步 DB/layout，失败不留下半文件。
7. 文件或 Worker 版本变化后重新处理；旧响应不能覆盖新输入。

## Phase 3 — 协议与 PSD Worker

### 3.1 Workspace

新增：

```text
crates/exotic-protocol/
crates/exotic-workers/psd-worker/
src-tauri/src/exotic/worker.rs
src-tauri/src/exotic/supervisor.rs
```

主程序与 Worker 共享 `exotic-protocol`。协议兼容由 `PROTOCOL_VERSION` 与握手能力共同决定；不能仅依赖同仓依赖版本。

### 3.2 统一帧格式

所有整数使用小端：

```text
offset  size  field
0       4     magic = "EXOT"
4       2     protocol_version
6       2     frame_type
8       8     request_id
16      4     json_len
20      4     blob_len
24      N     UTF-8 JSON
24+N    M     blob
```

限制：`json_len <= 1 MiB`、`blob_len <= 64 MiB`、PSD thumbnail 解码后像素数不得超过 Host 配置上限。读取 header 后先校验 magic/version/type/length，再分配内存。

```rust
pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_JSON_LEN: u32 = 1 << 20;
pub const MAX_BLOB_LEN: u32 = 64 << 20;

pub enum FrameType {
    Hello = 1,
    Ready = 2,
    Request = 3,
    Success = 4,
    Failure = 5,
    Shutdown = 6,
}

pub struct Frame {
    pub frame_type: FrameType,
    pub request_id: u64,
    pub json: Vec<u8>,
    pub blob: Vec<u8>,
}
```

JSON 只放控制字段；缩略图放同一帧的 blob。禁止响应 JSON 再跟第二个独立 blob frame。

### 3.3 消息

```rust
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RequestBody {
    Thumbnail {
        item_id: i64,
        source_path: String,
        target_long_edge: u32,
        input_fingerprint: String,
    },
    Metadata { item_id: i64, source_path: String, input_fingerprint: String },
}

pub struct ReadyBody {
    pub worker_id: String,
    pub worker_version: String,
    pub protocol_version: u16,
    pub capabilities: Vec<String>,
    pub max_blob_len: u32,
}

pub struct SuccessBody {
    pub item_id: i64,
    pub input_fingerprint: String,
    pub mime: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub metadata: Option<serde_json::Value>,
}

pub struct FailureBody {
    pub item_id: i64,
    pub input_fingerprint: String,
    pub code: WorkerErrorCode,
    pub retryable: bool,
    pub message: String,
}
```

稳定错误码至少包含：`unsupported_variant`、`malformed_input`、`resource_limit`、`io_error`、`internal_error`。用户可见日志不得包含完整绝对路径或 License token。

### 3.4 stdout/stderr 契约

- stdout 仅协议帧；任何前导日志字节均视为协议损坏。
- Worker 日志只写 stderr；Host 启独立线程持续读取，做单行长度与速率限制。
- stderr 环形缓冲只保留最近 64 KiB，用于诊断；不能无限累积内存。
- stdin EOF、Shutdown、Host 消失时 Worker 立即退出。

### 3.5 PSD Worker

Worker 仅声明 probe 已验证的 formats/capabilities。处理步骤：

1. 打开源文件并检查文件大小上限；
2. 解码 merged image 或 probe 已确认的合成路径；
3. 使用 checked arithmetic 计算像素缓冲；
4. 限制最大宽高、像素数、输出字节；
5. 按目标长边缩放并编码 WebP；
6. 返回 width/height、mime=`image/webp`、原 input_fingerprint 与 blob。

每任务顶层使用 `catch_unwind` 只作为 Rust panic 最后防线；panic 后返回 `internal_error` 并主动退出进程，避免未知污染状态继续服务。内存错误或底层崩溃由 Host 进程监督处理。

开发 fixture 目录不进入正式安装目录。测试授权仅在：

```rust
#[cfg(all(debug_assertions, feature = "exotic-dev-fixtures"))]
```

下启用。Release 构建即使数据库被修改，也不存在跳过授权的代码路径。

### 3.6 WorkerSupervisor

进程结构：

```text
Supervisor/control thread ─ owns Child + stdin
           ├── stdout reader thread ─ FrameResult channel
           └── stderr drain thread  ─ bounded diagnostics
```

启动顺序：验证安装记录与当前文件 hash（Part3 接真实校验）→ 创建隐藏/低优先级进程 → take stdout/stderr → 启读取线程 → Hello/Ready 握手 → 校验 worker_id/version/protocol/capabilities。

`run_task`：

1. 分配 request_id；写完整 Request frame 并 flush；
2. `recv_timeout(task_timeout)`；
3. 校验 request_id、item_id、fingerprint、frame type；
4. 超时或 reader 断开：Supervisor `kill → wait`，标当前实例死亡；
5. Pool 补充新实例前执行崩溃退避，禁止快速重启风暴。

每个 Supervisor 同时一个请求。全局并发上限由 Host semaphore 控制；单插件上限取 `min(global_remaining, manifest.max_concurrency)`。新增插件不能绕过全局 CPU 上限。

进程优先级与让步（R1/R4）：Worker 子进程以低优先级创建（Windows `BELOW_NORMAL_PRIORITY_CLASS`、macOS `nice`/QoS `utility`），作为**始终生效**的 OS 软让步——这是 exotic 让步阶梯里 exotic 让出 CPU 的底层手段，与主进程线程的 sleep 让步本质不同（主进程无法 sleep 令子进程让出 CPU）。全局 semaphore 的并发预算由 **derivation 与 exotic 共享**（二者同级，见总纲优先级阶梯）；exotic 在 Claimer 派发前用 `should_yield_exotic()`（scan/thumbnail/interaction）暂缓领取，但**不**因 derivation 运行而停领，避免大视频库下被饿死。

关闭：先停止领取 → 等待宽限期 → 发送 Shutdown → 超时 kill → wait → join reader threads。Windows 升级/卸载必须在全部句柄释放后替换 exe。

### 3.7 Host 输出验证

缩略图 Success 必须满足：

- `mime == image/webp`；
- blob 非空且不超过 Host 上限；
- 用独立 WebP parser 验证尺寸；声明尺寸与实际尺寸一致；
- 长边不超过请求档位允许误差；总像素不超过限制；
- fingerprint 与当前 DB 重新计算值一致。

任一失败：丢弃 blob、杀死该 Worker、记 `invalid_worker_output` terminal error；不能把恶意输出写入 asset 目录。

## Phase 4 — Coordinator、Pipeline 与 Sink

### 4.1 Coordinator

新增 `src-tauri/src/exotic/coordinator.rs`：

```rust
pub enum WakeReason {
    Startup,
    ScanCommitted,
    CatalogBackfill,
    PluginInstalled,
    LicenseActivated,
    ConfigChanged,
    RetryDue,
}

pub struct ExoticCoordinator {
    tx: tokio::sync::mpsc::Sender<WakeReason>,
    pipeline_token: std::sync::Mutex<Option<CancellationToken>>,
}
```

事件有界、可合并；满时保留“需要再检查”原子位，不能静默丢失最后一次 wake。`wake()` 只负责通知；Coordinator 串行检查 `enabled/auto/paused/runnable task`，再原子启动唯一 Pipeline。

Pipeline 自然完成后再次检查 pending + 已到期 retry，解决运行期间新增任务的尾部竞态。安装、激活成功必须发送 wake；不能只刷新 Host。

暂停：持久化 `exotic_paused=true`，停止新领取，在途自然结束。停止：取消 token，Supervisor 终止在途 Worker，任务恢复 pending。App 退出使用 stop 语义但不修改用户 paused 配置。

### 4.2 Pipeline

```text
Producer/Claimer -> bounded Task channel -> Dispatcher/Worker pools
                                               |
                                               v
Writer/Sink      <- bounded Result channel <---+
```

阶段职责：

- Claimer：事务领取当前已安装、已授权、平台兼容、未熔断插件的任务；写 processing + claimed_at + lease owner。
- Dispatcher：按 plugin_id/capability 路由；同时服从全局与单插件并发上限。
- Supervisor：执行、超时、崩溃恢复；一个结果对应一个 request_id。
- Writer：重新核验 fingerprint；原子落盘；事务**条件**更新 task/media_items（`WHERE status=1 AND lease_owner=:instance_id`，防跨实例覆盖，R2）；批量同步 layout 与事件。

Channel 容量必须小于等于可控在途上限，避免大量任务提前标 processing。取消时 drain 未开始任务并恢复 pending。

### 4.3 指纹、重试与熔断

领取前计算期望 fingerprint。若 task=done 但 fingerprint 不一致，先原子失效为 pending。处理完成时再次读取媒体 cache_key 与安装版本；不一致说明源在处理期间变化，丢弃结果并重新入队。

指纹组成（R5，对齐总纲 §5.3）：`normalized_settings` = 规范 JSON `{ "tier": <snap_to_tier(requested) 后的档位>, "capability": "thumbnail" }`。**必须用吸附后档位**（复用 `thumbnail::generator::snap_to_tier`，勿在 exotic 侧另写吸附），且 Request 帧的 `target_long_edge` 也传吸附后值，使 Worker 输出与指纹一致——否则同档不同请求 size 会算出不同指纹、反复重做。新增渲染参数（旋转、色彩意图等）时追加进该 JSON 并 bump `capability_api_version`。

cache_key 失效边界（R6）：`cache_key = xxh3_64("{rel_path}/{file_name}|{file_mtime}")`，只编码路径 + mtime，**无内容哈希**，与主缩略图同语义。mtime 不变的内容编辑不触发失效（已知边界，非 exotic 新缺陷）。如产品需「mtime 不变也重算」，可选把 `media_items.content_hash` 纳入指纹，代价是须保证扫描阶段已算出 content_hash（当前为 `Option<String>`）；默认**不**纳入，保持轻量。

重试建议：

| 错误 | 状态 | 策略 |
|---|---|---|
| Worker crash / timeout | retryable | 1m、5m、30m；最多 3 次 |
| 临时 IO/文件占用 | retryable | 30s 起指数退避 |
| malformed/unsupported variant | terminal | 等源文件或 Worker 版本变化再失效 |
| invalid worker output | terminal + plugin strike | 达阈值熔断插件 |
| license/平台/禁用 | 不改 task | Scheduler gating，不计 attempts |

插件熔断状态保存在安装状态表或独立 health 表；升级/修复安装后清零。单个坏文件不能熔断插件，只有进程级或协议级失败计 strike。

### 4.4 缩略图 Sink

只实现 Part2 首发 `image + thumbnail`。步骤：

1. `thumb_path(cache_dir, tier, media.cache_key)` 得最终路径；
2. 在同目录创建唯一 `.tmp`，写 blob、flush，必要时同步文件；
3. WebP 二次验证后原子 rename；
4. DB 事务条件更新：task id/status/fingerprint/`lease_owner` 必须仍匹配（`status=1 AND lease_owner=:instance_id`，R2）；写 task done/output/worker_version；同步 `media_items.thumb_status=1/thumb_path/thumbhash`；
5. 事务成功后 `crate::layout::cache::apply_thumb_results`（R8，真实模块路径）；
6. 合并发 `db:media_enriched`，避免每项事件风暴。

DB 事务失败时删除临时文件；最终文件已替换但 DB 失败时允许留下可回收孤儿，启动维护任务按 DB 引用清理。绝不先写 task done 再落文件。

thumbhash 由 Host 对已验证像素计算，或 Worker 返回后由 Host复核；不能无条件信任 Worker 声明。

### 4.5 用户命令与事件

新增：

```text
start_exotic_processing   // 清 paused，wake
pause_exotic_processing   // paused=true；不领新任务
stop_exotic_processing    // 取消本次运行；不等于永久禁用
get_exotic_processing_status
retry_exotic_task(item_id, capability)
retry_exotic_plugin_failures(plugin_id)
```

进度按当前可运行任务与完成/错误数计算；未购买/平台不支持任务单列 `blockedByAvailability`，不能让进度条永久停在 0%。状态变化通过 `exotic:status-changed` 事件推送前端。

## 测试

### 协议与恶意 Worker

- header 分片读取、粘包、空 payload、最大合法帧；
- magic/version/type 错误、长度溢出、JSON 非 UTF-8、超长 blob；
- request_id/item_id/fingerprint 错配；
- stdout 前导日志、响应后额外字节；
- stderr 无限输出仍不死锁，环形缓冲有界；
- Worker 不响应、退出、panic、返回非法 WebP。

### 调度与状态

- 两个并发 start 只启动一条 Pipeline；
- scan/activate 在 Pipeline 即将退出时到达，不丢 wake；
- 原子领取无重复；取消后未执行任务恢复 pending；
- retry 时钟、最大次数、熔断与升级清零；
- 源文件在处理中变化，旧响应被丢弃；
- 全局并发小于多个 manifest 并发之和仍严格生效。

### Sink

- 模拟短写、磁盘满、rename 失败、DB commit 失败；
- 任一故障无半成品引用、无错误 done；
- 成功后 DB、layout cache、前端事件一致；
- 重启恢复 processing lease 与临时文件清理。

## 本卷产出清单

- [ ] `exotic-protocol` 单一帧、限制、错误码与测试
- [ ] `psd-worker` 仅实现 probe 通过范围
- [ ] WorkerSupervisor stdout/stderr/timeout/kill/wait 生命周期
- [ ] 全局 + 单插件进程池限制
- [ ] Coordinator 全部 wake 点与尾部竞态处理
- [ ] Pipeline 原子领取、取消、重试、熔断
- [ ] 指纹双重校验与原子 Sink
- [ ] debug-only fixture 授权；Release 无旁路
- [ ] 协议攻击、调度竞态、故障注入测试通过

## 续作提示词

```text
实施 plan-docs/exotic_format_plugin_plan/exotic_format_plugin_v3_part2_pipeline_worker.md。
前置：Part1 DoD 与 PSD probe 已完成。
先协议测试，再 Worker，再 Supervisor，最后 Coordinator/Pipeline/Sink。
Release 中不得出现数据库 dev_mode；每阶段中文 commit。
完成后进入 exotic_format_plugin_v3_part3_license_distribution.md。
```
