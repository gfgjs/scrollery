# 冷门格式插件子系统 · v3.1 勘误强化卷

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 状态：规范性增量 / 实施前审查结论
> 日期：2026-06-25
> 适用范围：`exotic_format_plugin_plan_v3.md` 与 `exotic_format_plugin_v3_part1~4`
> 优先级：本卷与 v3 并读；发生冲突时，以本卷为准。
> 证据原则：现有实现事实以源码为准；候选依赖能力以 P0 探针为准。

## 0. 核验裁决

| 编号 | 裁决 | 核验结果 |
|---|---|---|
| R1 | 部分成立 | 跨进程让步机制确实缺失；但“不能只新增”表示“仅新增不够”，不等于禁止 `should_yield_exotic()`，原文不存在该字面矛盾。 |
| R2 | 成立 | 当前无单实例机制；`lease_owner` 必须落地，不能依赖尚不存在的单实例兜底。 |
| R3 | 部分成立 | 确实只有 batch/full 两个命令且无共享 Router；但“两个入口都经 `par_iter(generate_thumbnail)`”不精确，当前启用的 pipeline 分支直接调用 decode/encode。 |
| R4 | 成立 | strict `derivation > exotic` 可在持续派生负载下饿死 exotic。 |
| R5 | 成立 | `normalized_settings` 未定义，且必须使用 `snap_to_tier` 后的档位。 |
| R6 | 成立 | `cache_key` 仅含路径与秒级 mtime；mtime 不变或同秒覆盖无法失效。 |
| R7 | 条件成立 | 若 Router 按要求放在 batch 的 `needs_gen` 前置过滤点，则必须批量补取 `file_format`；也可延后到完整 `MediaItem` 已加载处，但会削弱前置门控并增加状态处理复杂度。 |
| R8 | 部分成立 | `layout_cache::apply_thumb_results` 是错路径；`ModelAsset` 原文属于位置含糊，并非明确声称位于 `ai_commands`。统一给出准确路径。 |
| R9 | 成立 | face 模块两处“无需修改/原样不动”注释会与 R1 实现冲突。 |
| R10 | 成立 | 通用下载器若直接替换公开进度结构，会破坏 AI/face 前端 Channel 契约。 |
| R11 | 成立 | 多种 sequence/version 缺少统一词汇表。 |
| R12 | 成立 | P0 要求锁版本，却未给候选起点与已知探针风险。 |
| R13 | 成立并补强 | 分类只需扩展名、不依赖 enrichment；但当前 walker 会提前丢弃未知扩展名，必须同步改 walker。 |

结论：架构级裂缝 2 项仍成立；重要缺陷 5 项中 R3、R7 需按上述边界改写；精确性问题 6 项中 R8 需降为“错路径 + 位置含糊”。

---

## R1 · exotic 跨进程让步：补齐机制与边界

### 核验

v3 已在 Part1§2.4 定义：

```text
should_yield_exotic = scan || common_thumbnail || common_derivation || interaction
```

总纲§6.3 的“不能只新增 `should_yield_exotic()`”语义：还必须把 exotic 加入 `ai_yield_blockers()`；不是禁止新增该函数。审查初稿对此句的字面解释不成立。

真实缺口：函数由谁调用、何时调用、对子进程有什么效果，均未定义。

源码证据：

- `src-tauri/src/state.rs:282-300`：`ai_yield_blockers()` 只返回阻塞源，本身不调度进程。
- `src-tauri/src/ai/pipeline.rs:287-294`：CLIP producer 读取 blocker 后在主进程线程 `sleep(500ms)`。
- `src-tauri/src/ai/face_pipeline.rs:286-293`：face producer 同样 `sleep(500ms)`。
- `src-tauri/src/derive/pipeline.rs:310-313,414-415`：派生 producer/dispatcher 通过主进程线程 sleep 暂停新派发。
- v3 Part2§3.5：exotic 解码发生在 Worker 子进程；Host 线程 sleep 不能抢回已派发 Worker 的 CPU。

### 规范修正：两层让步

#### 1. OS 调度层软让步

Worker 创建时使用低于普通优先级：

- Windows：`BELOW_NORMAL_PRIORITY_CLASS`；继续保留隐藏窗口与 Job Object 约束。
- macOS：经实测选择 `nice`/QoS utility 等等价机制；记录创建失败与降级行为。

此层始终生效。Part2§3.6 的“低优先级进程”必须显式挂入优先级设计与平台测试。

#### 2. Host 派发层硬门控

`AppState` 新增判断；只用于 Claimer/Dispatcher 领取或派发**新任务之前**：

```rust
/// exotic 为跨进程解码；此判断只暂停新任务派发。
/// derivation 不在硬阻塞集合中，二者按 R4 共享后台重活并发池。
pub fn should_yield_exotic(&self) -> bool {
    self.is_scan_or_thumb_running() || self.is_interactive()
}
```

执行点：

1. Claimer 开始新一轮原子领取前检查；命中时不把更多任务置为 `processing`。
2. 已领取但尚未发给 Worker 的任务，在 Dispatcher 取全局 permit 前再次检查。
3. 暂停期间保留 Coordinator wake/重检信号，解除阻塞后自动恢复。

#### 3. 在途边界

已发给 Worker 的任务不做 sleep 式抢占。仅允许：

- 自然完成；
- 用户 stop/App 退出时按取消协议终止；
- 超过 `task_timeout` 后由 Supervisor `kill → wait`。

不得宣称 exotic 可对交互或高优先级任务“实时让步”。验收指标应测“停止新派发延迟”和“最多剩余在途数”。

### AI/face 反向让步接线

`state.rs` 增加 exotic 活动 token；`ai_yield_blockers()` 追加：

```rust
if self.exotic_analysis_token.lock().unwrap().is_some() {
    blockers.push("exotic");
}
```

锁规则：逐把 `lock → 读取 → drop`；禁止同时持有多把 token Mutex。

总纲§6.3 应改为：

> `ai_yield_blockers()` 追加 exotic token，使 AI/face 让步给 exotic；同时新增 `should_yield_exotic()`，使 exotic 在派发新任务前让步给 scan/thumbnail/interaction。两项缺一不可。已派发 Worker 不可 sleep 抢占；derivation/exotic 协调按 R4 执行。

---

## R2 · `lease_owner` 必选；单实例不得作为隐含前提

### 核验

- `src-tauri/Cargo.toml:16-23`：无 `tauri-plugin-single-instance`。
- 全仓无 `single_instance`/`single-instance` 接线。
- v3 Part2§4.2 已写“Claimer 写 lease owner”，但总纲/Part1 DDL 没有 `lease_owner` 列；文档内部不闭合。

### 规范修正

`exotic_tasks` 建表直接包含：

```sql
claimed_at  INTEGER,
lease_owner TEXT,
```

V_NEXT 为新建表，不执行额外 `ALTER TABLE`。

Lease 契约：

1. App 每次启动生成随机进程级 `instance_id`；仅存内存。
2. 原子领取事务同时写 `status=1, claimed_at=:now, lease_owner=:instance_id`。
3. 运行中任务周期续租；更新条件必须含 `id=:id AND status=1 AND lease_owner=:instance_id`。
4. `lease_ttl` 必须大于 `max(task_timeout + kill/wait 宽限, 3 × heartbeat_interval)`；具体值写入配置/常量并进入故障测试。
5. 孤儿恢复只回收过期租约：

```sql
UPDATE exotic_tasks
SET status = 0,
    claimed_at = NULL,
    lease_owner = NULL
WHERE status = 1
  AND (claimed_at IS NULL OR claimed_at < :lease_expired_before);
```

6. Writer 最终条件更新同时校验 `status=1`、fingerprint 与 `lease_owner=:instance_id`。失去租约的旧结果只能丢弃。

职责区分：原子 `status` 条件更新阻止双实例重复领取；`lease_owner + ttl` 防止活实例任务被错误恢复，并隔离过期 Writer。不得写成“lease 单独解决全部并发领取”。

若未来引入单实例插件：单列任务、评估扫描/AI/派生/窗口恢复等全局影响；不改变本卷 lease 必选要求。

---

## R3 · 缩略图真实入口：两个命令 + 共享判定

### 核验

现有公开 Tauri 命令只有：

- `src-tauri/src/ipc/thumbnail_commands.rs:26`：`batch_request_thumbnails`。
- `src-tauri/src/ipc/thumbnail_commands.rs:387`：`start_full_thumbnail_generation`。
- `src-tauri/src/lib.rs:435-436`：仅注册上述两个缩略图命令。

不存在“单项命令”。前端单项可通过只含一个 ID 的 batch 表达。

也不存在现成任务入队“公共入口”：

- batch 先在 `thumbnail_commands.rs:48` 查缓存，构造 `needs_gen`。
- `USE_PIPELINE=true`（`:23`）时，batch 在 `:288` 直接调用 `decode_media_step`；`:104-138` 的 `par_iter(generate_thumbnail)` 属未启用旧分支。
- full 有独立的 pending 查询、进度、并发与落库流程；旧分支在 `:450-508` 使用 `par_iter`，当前启用分支仍是独立 pipeline。

因此，审查结论的架构判断正确；“两个入口都按同一 `par_iter(generate_thumbnail)` 形态运行”不正确。

### 规范修正

新增纯判定函数，不把 DB、Coordinator 或 `AppState` 注入 generator：

```rust
pub struct ThumbnailRouteInput<'a> {
    pub item_id: i64,
    pub file_format: &'a str,
    pub thumb_status: i64,
    pub resolution: Option<&'a FormatResolution>,
    pub task_status: Option<ExoticTaskStatus>,
    pub fingerprint_valid: bool,
}

pub fn route_thumbnail(input: &ThumbnailRouteInput<'_>) -> ThumbnailRoute;
```

数据访问层负责批量预取 `FormatResolution`、task 状态与 fingerprint；纯函数只作判定。

接线点：

1. batch：缓存查询后、`needs_gen` 入列前。
2. full：每批 pending item 进入 decode queue 前。
3. 两处命中未完成 `Exotic`：不调用 `generate_thumbnail`、`decode_media_step`、`process_deferred_cpu`；不写 `thumb_status=2`；合并发送一次 Coordinator wake。

措辞替换：

- 删除“单项命令”“现成公共入口”。
- 改为“两个命令入口共享 `route_thumbnail()` 判定；batch 单元素作为单项场景测试”。
- exotic retry 命令只重置 task/wake；不伪装成第三个缩略图入口。后续 batch/full 请求必须观察到一致路由。

---

## R4 · 防止 derivation 长期饿死 exotic

### 核验

v3 同时规定：

```text
scan > common thumbnail > common derivation > exotic > AI/face
should_yield_exotic 包含 common_derivation
```

`src-tauri/src/state.rs:313-315` 的 `should_yield_derivation()` 只让步给 scan/thumbnail/interaction，不让步给 exotic。大视频库持续产生派生任务时，derivation token 可长期存在；若 exotic 把它作为硬 blocker，exotic 永远不领新任务。

### 冻结决策：derivation 与 exotic 同级，共享公平后台重活池

优先级改为：

```text
scan > thumbnail > interaction(前台)
     > { derivation, exotic：同级、共享公平后台重活池 }
     > AI/face
```

落地规则：

1. `should_yield_exotic()` 不检查 `is_derivation_running()`。
2. 新增 Host 级 `BackgroundHeavyLimiter`；derivation 重型任务与 exotic Worker 请求都必须先取同一个 permit。
3. Part2§3.6 原“全局 semaphore”不能只限制 exotic Worker；必须扩展为两个子系统共用的全局预算。
4. permit 获取须公平排队；每条流水线预取/已领取数量不得超过当前可派发容量，禁止 derivation 先囤满无界队列。
5. 任务完成、取消、Worker kill 后立即释放 permit；故障测试检查无泄漏。
6. scan/thumbnail/interaction 活动时，两类后台重活都停止**新派发**；已在途任务按各自取消/超时边界处理。
7. AI/face 继续让步给 derivation 与 exotic。

公平验收：持续注入 derivation 与 exotic 任务至少 30 分钟；两类完成计数均持续增长，exotic 最大排队等待有明确上界。只验证“总并发不超限”不足以证明不饥饿。

---

## R5 · `normalized_settings` 使用规范化档位与确定性序列化

### 核验

- 总纲§5.3 把 `normalized_settings` 纳入 fingerprint，但没有定义。
- `src-tauri/src/thumbnail/generator.rs:31-37`：有效档位为 `120/240/480/960`。
- `src-tauri/src/thumbnail/generator.rs:95-103`：`generate_thumbnail()` 先执行 `snap_to_tier(config.size)`。

原始请求 `470` 与 `480` 会产出同档结果。若 fingerprint 使用原始值，两次请求会得到不同指纹并重复处理。

### 规范修正

fingerprint 不再用未定义的字符串拼接；对固定字段顺序的版本化结构做规范序列化后 SHA-256：

```json
{
  "fingerprint_schema": 1,
  "media_cache_key": "<i64 十进制或固定 16 位十六进制，二选一后冻结>",
  "plugin_id": "exotic-image-psd",
  "worker_version": "1.0.0",
  "capability": "thumbnail",
  "capability_api_version": 1,
  "settings": {
    "target_tier": 480,
    "output_mime": "image/webp"
  }
}
```

要求：

1. `target_tier = snap_to_tier(requested_size)`；直接复用现有函数，不复制档位算法。
2. Request 的 `target_long_edge` 同样传 `target_tier`，保证请求、输出、指纹一致。
3. 所有会改变输出且不由 `worker_version/capability_api_version` 完整覆盖的配置都进入 `settings`，例如可配置 WebP quality、色彩意图、旋转策略。
4. JSON 固定字段顺序、数字格式与 UTF-8 编码；禁止对无序 `HashMap` 直接序列化后哈希。也可使用等价的 canonical CBOR/长度前缀编码，但实现与测试必须唯一。
5. 单元测试覆盖：同档不同原始尺寸指纹相同；跨档指纹不同；任一输出参数变化指纹不同。

---

## R6 · 声明 `cache_key` 的失效边界；内容哈希只能作为显式增强

### 核验

- `src-tauri/src/utils/hash.rs:27-37`：`cache_key = xxh3_64("{rel_path}/{file_name}|{file_mtime}")`；不含文件内容或文件大小。
- `src-tauri/src/scanner/walker.rs:91-96`：mtime 截断为 `as_secs()`，精度为秒。
- `src-tauri/src/db/models.rs:104`：`MediaItem.content_hash` 为 `Option<String>`。
- 当前 `src-tauri/src` 内未发现 `media_items.content_hash` 的写入路径；该列不是可直接依赖的现成强校验。

因此以下变化可能不触发 fingerprint 失效：

- 编辑器保留或回退 mtime；
- 同一秒内原地覆盖；
- 网络盘/恢复工具保留旧时间戳。

### 规范修正

默认语义：exotic 与现有缩略图缓存保持一致，按“相对路径 + 文件名 + 秒级 mtime”失效。文档与 UI 不得承诺内容级检测。

Part4§8.2 步骤 10 改为：

> 修改 PSD，并确认扫描读到的 mtime 已变化；`cache_key` 变化后旧 task/产物失效并自动重做。

追加边界用例：保持 mtime 的内容替换不会被默认策略捕获；测试应记录为已知语义，不误判成随机失败。

可选内容级增强：

1. 新建明确的后台内容哈希生命周期：计算时机、算法、取消、网络盘成本、SourceChanged 更新顺序。
2. 只有 `content_hash` 已知时才纳入 fingerprint；不得把 `NULL` 当作“内容未变化”的证明。
3. 必须评估全库 IO、首次扫描耗时与百万库成本；默认不开启。
4. 若启用，测试覆盖 mtime 不变但内容变化、hash 计算中 App 退出、源文件计算中变化。

---

## R7 · batch 前置路由必须批量取得 `file_format`

### 核验

`src-tauri/src/ipc/thumbnail_commands.rs:48-59` 的 batch 快速查询只返回：

```text
id, thumb_status, thumb_path, thumbhash
```

batch 入参只有 `item_ids`，完整 `MediaItem` 到旧/新 pipeline 内部才加载。若按 R3 在 `needs_gen` 之前路由，当前数据不足以按扩展名解析 Catalog。

### 规范修正

1. batch SQL 至少扩为 `id, file_format, thumb_status, thumb_path, thumbhash`。
2. 同一批次批量加载所需 exotic task 状态/fingerprint；禁止 Router 对每个 item 再发一次 DB 查询。
3. full 的 pending 取数也必须在进入 decode queue 前拿到 `file_format`；可返回轻量 route row，也可批量加载完整 `MediaItem`。
4. 保持 common-first：扩展名统一小写；先查内置常见格式，再查 Catalog。

若实现选择在完整 `MediaItem` 已加载后才路由：可不修改最初 cache SQL，但仍必须发生在任何 decode/失败状态写入之前，并证明无 N+1 与无 `thumb_status=2` 闪烁。首选方案仍是前置批量扩列。

---

## R8 · 符号路径勘误

| v3 写法/歧义 | 准确路径 | 源码证据 |
|---|---|---|
| `layout_cache::apply_thumb_results` | `crate::layout::cache::apply_thumb_results` | `src-tauri/src/ipc/thumbnail_commands.rs:87,221,362`；定义在 `src-tauri/src/layout/cache.rs:115` |
| `ModelAsset` 位置含糊 | `crate::ai::profile::ModelAsset` | `src-tauri/src/ai/profile.rs:38` |
| `ai_commands::download_assets` 简写 | `crate::ipc::ai_commands::download_assets` | `src-tauri/src/ipc/ai_commands.rs:958` |
| `DownloadProgress` 位置含糊 | `crate::ipc::ai_commands::DownloadProgress` | `src-tauri/src/ipc/ai_commands.rs:846` |

Part2§4.4 的 Sink 第 5 步必须改为完整模块路径。Part3§6.2 使用上述准确位置，避免实施者错误地在 `ai_commands` 内寻找 `ModelAsset` 定义。

---

## R9 · face 让步注释必须与 R1 同步

### 核验

- `src-tauri/src/ai/face_pipeline.rs:25`：模块注释写“`ai_yield_blockers()`（不新增、不修改）”。
- `src-tauri/src/ai/face_pipeline.rs:284-286`：调用点注释写“原样不动”。
- R1 要给 `ai_yield_blockers()` 增加 exotic blocker。

### 规范修正

实现 R1 的同一 commit 内更新两处注释。推荐语义：

```rust
// 复用 AppState::ai_yield_blockers()；该统一阻塞源已包含 scan、thumbnail、
// derivation、interaction 与 exotic。CLIP/face 共享同一优先级门控。
```

同时核对 `src-tauri/src/ai/pipeline.rs:285-294` 的 blocker 日志；确保新值 `exotic` 可观测。禁止只改代码、保留“无需修改/原样不动”注释。

---

## R10 · 下载器重构必须保留 AI/face 公共进度契约

### 核验

Rust 当前契约：`src-tauri/src/ipc/ai_commands.rs:846-861`。

```rust
pub struct DownloadProgress {
    pub model_id: String,
    pub current_file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub received: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}
```

前端直接依赖 camelCase 形态：

- `src/types/ai.ts:90-98`：`ModelDownloadProgress`。
- `src/types/face.ts:44-52`：`FaceModelDownloadProgress`。
- `src/components/settings/ModelLibrary.vue:80-84`：显示 `currentFile/fileIndex/fileCount/received/total`。
- `src/components/settings/FaceModelLibrary.vue:78`：读取 `received/total`。

Part3§6.2 草案中的通用结构 `download_id/file/state` 不能直接替换现有 Channel payload。

### 规范修正

重构顺序：

1. 抽取内部下载机制：Range、镜像回退、`.part`、hash/size、原子 rename、取消与限额。
2. 内部模块输出通用 `DownloadEvent`；AI/face 适配器继续构造原有 `DownloadProgress`，命令签名与序列化字段零变化。
3. 现有 `download_model`、`download_face_model` 回归通过后，exotic 再复用通用模块并定义自己的进度 DTO。
4. 通用 DTO 不得反向污染已有前端类型；需要新字段时优先放在 exotic DTO，或经兼容性评审后添加可选字段。
5. 测试冻结完整事件序列：准备、逐文件、续传、镜像回退、失败、最终 `done`。

`ModelAsset` 与 AI profile 的适配保留在 AI 层；通用模块只接收中性的 `DownloadAsset`。

---

## R11 · sequence/version 名词表

| 名词 | 作用域 | 语义 | 是否用于安全单调性 |
|---|---|---|---|
| `schema_version` | 本地数据库 | DB migration 版本；核验时 `CURRENT_VERSION=8`，故本项目 V_NEXT=9 | 本地只增 |
| `catalog_sequence` | 能力 Catalog 快照/投影 | 标识已接受的能力目录修订 | 是；拒绝较旧目录 |
| `registry_sequence` | 签名 Registry index | 防整个远程索引回滚/冻结 | 是；全局单调 |
| `package_sequence` | `(plugin_id, target)` 包 | 防旧包伪装成升级；版本字符串只展示 | 是；同作用域单调 |
| `protocol_version` | Host↔Worker IPC | 帧与消息兼容门控；握手时校验 | 否；不承担包回滚防护 |
| `capability_api_version` | 单 capability 输出契约 | 输出语义/参数契约版本；进入 fingerprint | 否；变化触发失效 |
| License token `version` | License payload | token 结构版本；验签前门控解析 | 否；不是发行序号 |
| `worker_version` | Worker 二进制 | 展示、兼容与 fingerprint 输入 | 否；升级单调性看 `package_sequence` |
| Host `version` | App 二进制 | 当前 Host 语义版本 | 否 |
| `min_host_version` | Catalog/package | Worker/包可运行的最低 Host 版本 | 否；兼容条件 |

规则：

1. 文档中不再使用裸“sequence”“version”表达安全结论；必须写完整名词。
2. `catalog_sequence` 与 `registry_sequence` 若最终合并为同一签名对象，schema 只能保留一个权威字段并统一命名；若分离，分别持久化最高已接受值。
3. 安全降级/回滚判断只使用对应 sequence；不得比较语义版本字符串替代。
4. `protocol_version` 当前是握手兼容校验，不预设一定存在版本协商。

---

## R12 · PSD P0 候选与已知探针风险

### 核验

原 Part1§0.1 要求锁定候选 crate 精确版本/feature，却未列任何起点。该缺口成立。

本机 Cargo 缓存可核验的一手源码版本：

- `psd 0.3.5`：`src/lib.rs:62-65` 明确不支持 PSB；`src/sections/file_header_section.rs:7,94-98` 只接受 version 1。
- `psd 0.3.5`：`src/sections/image_data_section.rs:10-13` 只声明 8/16-bit；`:101-115` 的 16-bit raw 路径仅显式下转换首通道，必须用真实 RGB/灰度样本验证其余通道。
- `psd 0.3.5`：header 能识别 CMYK，但 `src/psd_channel.rs:18-28,56-79` 按 RGBA 通道位置组装，未见 CMYK→RGB 色彩转换；不能据“可解析 ColorMode”宣称颜色正确。
- `psd 0.3.5`：ZIP compression 存在 unsupported/unimplemented 路径；`src/lib.rs:203-204` 也说明图层 flatten 未完整处理 blend mode。
- `image 0.25.10`：`src/io/format.rs:11-61` 的 `ImageFormat` 无 PSD；只能作为 resize/WebP 编码等后处理组件，不能当 PSD decoder。

以上只证明所核验版本的源码事实，不代表未来版本。P0 必须重新检查当时最新版本、许可证与变更记录。

### 候选起点

| 候选 | 定位 | P0 必测风险 |
|---|---|---|
| `psd` crate | 纯 Rust PSD parser/renderer 候选 | PSB 明确缺失；CMYK 色彩、16-bit 多通道、ZIP、blend mode、畸形 RLE、内存上限 |
| `psd` + `image` | `psd` 解码，`image`/现有编码器缩放与 WebP 输出 | decoder 风险不因后处理消失；像素缓冲复制与峰值内存 |
| Windows WIC / macOS ImageIO | OS 委托候选 | 平台能力不一致、版本差异、是否能稳定取得 merged composite、平台许可与发布测试 |
| 自研最小 PSD merged-image parser | 仅在现有 crate 探针失败时评估 | 攻击面、压缩变体、色彩管理、维护成本最高；不得在无独立安全预算时默认选择 |

Probe 报告必须记录：候选版本、features、源码 commit/hash、许可证、样本 hash、merged image/无 merged image、RGB/CMYK、8/16/32-bit、Raw/RLE/ZIP、PSB、峰值内存、耗时、panic/错误码。Catalog/manifest 只登记实测通过范围。

---

## R13 · 分类只依赖扩展名；但必须改 walker 的提前过滤

### 核验

设计中的 `CatalogStore.by_format` 以规范化扩展名为键。宽高、时长、EXIF 等 enrichment 结果不参与 exotic 分类；扫描阶段已具备全部输入。

现有实现边界：

- `src-tauri/src/scanner/walker.rs:73-77`：遍历时取得并小写化扩展名。
- `src-tauri/src/scanner/walker.rs:79-82`：当前只调用 `classify_media_type()`；未知扩展名立即 `continue`。
- `src-tauri/src/scanner/fast_scan.rs:373-384`：fast scan 入库已携带 `file_format` 与 `media_type`。

因此“扫描事务内 seed task、不等 enrichment”成立；但只改 fast scan 不够，PSD 会在 walker 阶段被提前丢弃。

### 规范修正

1. walker 使用启动时固定的 `Arc<CatalogSnapshot>` 调 `classify_scanned_file(ext, catalog)`。
2. 分类顺序：`classify_media_type(ext)` 优先；仅返回 `None` 时查 Catalog。
3. 同一轮 walk 使用同一 Catalog snapshot，避免刷新过程中同批文件分类不一致。
4. walker 不做额外文件 IO；Catalog 命中后把 `media_kind` 与小写扩展名传给 fast scan。
5. fast scan 事务内 upsert `media_items` 并 seed `exotic_tasks`；事务提交后合并发送一次 wake。
6. Catalog 刷新产生的新格式由集合式 backfill 补齐，不要求重跑 enrichment。

测试必须包含：当前 common 格式、Catalog 已知 exotic、Catalog 未知格式、大小写扩展名、扫描中 Catalog 刷新。

---

## 附录 A · v3 已核验正确项

| 项 | 源码证据 |
|---|---|
| D12：迁移版本取当前最高 +1；本次为 V9 | `src-tauri/src/db/migration.rs:21`：`CURRENT_VERSION=8`；`:127-130` 已留 V9 示例占位 |
| generator 不持 `AppState`；Router 可放 IPC/调度层 | `src-tauri/src/thumbnail/generator.rs:95-100`：参数为 `MediaItem/Path/EngineArena/ThumbConfig` |
| 修改 `ai_yield_blockers()` 可同时覆盖 CLIP+face | `src-tauri/src/ai/pipeline.rs:287`；`src-tauri/src/ai/face_pipeline.rs:286` |
| `exotic_tasks` 独立表，不污染 `MediaItem` | `src-tauri/src/db/models.rs:81-107` 无 exotic 字段 |
| keyring 使用固定 service + account 的模式可复用 | `src-tauri/src/ipc/storage_commands.rs:31`；`src-tauri/src/ipc/proofread_commands.rs:21` |
| `cache_key` 可作为轻量默认 fingerprint 输入 | `src-tauri/src/db/models.rs:93`；边界按 R6 明示 |

---

## 附录 B · 审查旁注纠错

原审查 Insight 以“`state.rs` 无 `reserved_core_pool`”推断“enrichment 使用 reserved-core-pool 的记忆失真”。该推断错误：

- `src-tauri/src/scanner/enricher.rs:45-50` 定义 `reserved_core_pool()`。
- `src-tauri/src/scanner/enricher.rs:356`：video enrichment 使用该池。
- `src-tauri/src/scanner/enricher.rs:474`：audio enrichment 使用该池。

正确结论：`reserved_core_pool` 不属于 `AppState`，而是 enrichment 模块局部实现。审查“现有代码如何”仍应回源码核验，但不能把“未在预期文件找到”当作“全仓不存在”。

---

## 实施顺序

1. Part1 schema：R2 lease 列/DAO；R13 walker 分类；R3/R7 Router 与批量取数；R9 注释同步。
2. Part1/Part2 调度：R1 双层让步；R4 共用公平后台重活池。
3. Part2 fingerprint/Sink：R5 规范化结构；R6 明示失效边界；R8 模块路径。
4. Part3 下载：R10 先保 AI/face 契约重构，再接 exotic。
5. 全卷：R11 统一名词；P0 按 R12 先探针后冻结 manifest。

R1/R2/R4 未完成前，不进入 Worker/Pipeline 正式实现。R3/R7 未完成前，不宣称主 generator 已完全让路。
