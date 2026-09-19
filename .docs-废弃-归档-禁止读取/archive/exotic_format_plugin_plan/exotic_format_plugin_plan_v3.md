# 冷门格式插件子系统 · 总纲 v3.1

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 全套为**已建成子系统的 as-built 设计快照**(后端已落地,Part6 §1.1 有账);Part0 §0.3 已降历史参考,插件平台职责由 refactor_2026/Part6 接管,现行权威 = Part6。

> 状态：实施前评审稿
> 日期：2026-06-25
> 取代：v1/v2 全套（已移至 `plan-docs/archive/`）
> 修正记录：见 `exotic_format_plugin_plan_v3.1_addendum.md`（R1–R13 审查证据；修正已并入本总纲与各分卷 v3.1）
> 首个垂直切片：PSD 缩略图

## 0. 文档集

| 文件 | 范围 |
|---|---|
| `exotic_format_plugin_plan_v3.md` | 决策、边界、数据流、风险、路线图 |
| `exotic_format_plugin_v3_part1_foundation.md` | 能力目录、schema、扫描、路由、跨流水线门控 |
| `exotic_format_plugin_v3_part2_pipeline_worker.md` | Worker 协议、PSD Worker、任务调度、产物回填 |
| `exotic_format_plugin_v3_part3_license_distribution.md` | 信任模型、许可、签名注册表、安全安装、更新卸载 |
| `exotic_format_plugin_v3_part4_frontend_release.md` | 前端、正式 E2E、安全/性能/跨平台发布门禁 |
| `exotic_format_plugin_plan_v3.1_addendum.md` | 审查证据链与 R1–R13 变更索引 |

v1/v2 已废弃，移至 `plan-docs/archive/`。发生冲突时，以本 v3.1 文档集为准。

## 1. 一句话定位

冷门格式子系统是独立于主解码引擎的后台任务系统：由**签名能力目录**识别格式，由**能力级任务表**记录进度，由**厂商签名的长驻 Worker 进程**执行解码，由统一 Sink 回填现有缩略图缓存。

插件边界同时承担四种职责：交付边界、故障边界、调度边界、商业授权边界。它**不是权限沙箱边界**；v3 不把进程隔离误称为安全隔离。

## 2. v2 审查后必须修正的事实

1. 未安装插件时，仅扫描 `plugins/*/manifest.json` 无法知道 PSD 可购买；首次锁占位链路不成立。
2. `media_items.exotic_status DEFAULT 0` 会把普通媒体全部标成待处理，也无法表达 thumbnail/metadata/text 的独立状态。
3. `thumbnail::generator` 当前不持有 `AppState`；把 Host 注入 generator 会扩大热路径签名改动。
4. CLIP、人脸、主派生会继续领取 PSD/RMVB；只修改 thumbnail generator 不能形成独立边界。
5. 激活或新扫描后没有可靠的 Pipeline 唤醒点。
6. 源文件、Worker 版本、渲染参数变化没有缓存失效指纹。
7. “先解包后验签”、只校验主可执行文件、数据库可开启 dev mode 均不满足发布安全要求。
8. 原生 Worker 继承当前用户权限。签名可证明来源，不能阻止已签名代码访问用户文件或网络。

## 3. 目标与非目标

### 3.1 目标

- G1：常见格式热路径无插件时近零成本；固定基准吞吐回退不超过 2%。
- G2：格式识别、安装状态、授权状态、处理状态互相独立。
- G3：新增已登记格式只更新签名能力目录并发布插件包，不修改 Host 业务代码。
- G4：每个能力独立任务、独立重试、独立失效；支持 thumbnail/metadata/text 等扩展。
- G5：Worker 崩溃、超时、畸形输出不破坏主进程与缓存。
- G6：购买、下载、安装、激活、自动处理、更新、卸载形成闭环。
- G7：Windows/macOS 正式签名分发；缺平台构建时明确降级。
- G8：每个发布包携带精确 SBOM、许可证材料与审核记录。

### 3.2 非目标

- v3 不开放任意第三方原生 Worker 市场。只运行产品厂商签名的 Worker。
- v3 不承诺原生 Worker 权限沙箱。第三方生态必须先完成 Windows AppContainer/受限令牌与 macOS App Sandbox 方案。
- PSD 切片只交付 `thumbnail`；PSD 编辑、图层浏览、高保真色彩管理不在范围。
- 不在应用内实现支付；购买仍跳外部商店。

## 4. 冻结决策

| 编号 | 决策 |
|---|---|
| D1 | Worker 子进程 + 统一长度帧 IPC；每进程同一时刻只处理一个请求 |
| D2 | 主程序内置基础能力目录；远程签名目录可增量更新并持久缓存 |
| D3 | 已安装 manifest 只能实现能力目录已登记的格式，不能自行劫持 jpg/mp4 等常见格式 |
| D4 | 删除 `media_items.exotic_status` 方案；使用 `exotic_tasks` 能力级任务表 |
| D5 | 主缩略图在 `thumbnail_commands` 的统一队列入口路由；generator 保持纯解码函数 |
| D6 | CLIP/人脸/主派生通过 `exotic_tasks` 排除尚未完成的冷门源任务 |
| D7 | Coordinator 接收扫描、目录刷新、安装、激活、配置、重试时钟事件，幂等唤醒 Pipeline |
| D8 | Release 构建不存在运行时 dev-mode 开关；开发跳过授权仅由 Cargo feature + debug 构建启用 |
| D9 | 包在 staging 中先验证签名清单，再安全解包、全量复核、原子切换 |
| D10 | v3 仅信任厂商签名 Worker；签名代表来源信任，不代表权限隔离 |
| D11 | License 使用 Ed25519 离线验签；校验 plugin_id、sku、有效期、token 版本与 key_id |
| D12 | schema 版本落地时取当前最高版本 +1；不得预占固定数字 |

## 5. 三份真相与状态模型

### 5.1 三份真相

| 真相 | 来源 | 回答的问题 |
|---|---|---|
| 能力真相 | 内置/远程签名 Catalog | 某格式是否有产品、属于哪类媒体、提供哪些能力、哪些平台可用 |
| 安装真相 | 已验证的 Package Manifest | 当前磁盘安装了什么版本、每个文件应是什么 hash |
| 授权真相 | keyring 中的签名 License | 当前用户是否可运行该 SKU |

不得再用“磁盘上是否存在 manifest”同时推导可购买、已安装、已授权三个概念。

### 5.2 格式可用态

后端返回结构化 `FormatResolution`，不压缩成三态枚举：

```rust
pub enum Availability {
    AvailableUninstalled,
    InstalledUnlicensed,
    Authorized,
    LicenseExpired,
    UnsupportedPlatform,
    IncompatibleHost,
    InvalidInstallation,
    Disabled,
    NoOffering,
}

pub struct FormatResolution {
    pub format: String,
    pub media_kind: MediaKind,
    pub plugin_id: Option<String>,
    pub capabilities: Vec<Capability>,
    pub availability: Availability,
    pub store_url: Option<String>,
    pub installed_version: Option<String>,
}
```

`Availability` 只描述可用性；任务的 pending/running/done/error 由 `exotic_tasks` 描述。

### 5.3 任务模型

```sql
CREATE TABLE exotic_tasks (
    id                 INTEGER PRIMARY KEY,
    item_id            INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    plugin_id          TEXT NOT NULL,
    capability         TEXT NOT NULL,
    status             INTEGER NOT NULL DEFAULT 0,
    input_fingerprint  TEXT,
    attempts           INTEGER NOT NULL DEFAULT 0,
    next_retry_at      INTEGER,
    claimed_at         INTEGER,
    lease_owner        TEXT,
    last_error_code    TEXT,
    last_error_message TEXT,
    output_path        TEXT,
    worker_version     TEXT,
    created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(item_id, plugin_id, capability)
);

CREATE INDEX idx_exotic_tasks_ready
ON exotic_tasks(plugin_id, capability, status, next_retry_at);
```

状态：`0=pending / 1=processing / 2=done / 3=retryable_error / 4=terminal_error`。未安装、未授权、禁用不写入任务状态；Scheduler 领取时通过 `FormatResolution` 门控。

指纹：

```text
SHA256(media.cache_key | plugin_id | worker_version | capability_api_version | normalized_settings)
```

源文件变化、Worker 升级、能力 API 或渲染参数变化，均使旧 `done` 失效并重新入队。

## 6. 分层架构

```text
Builtin Catalog ─┐
Signed Registry ─┴─> CatalogStore ──> format -> offering / media_kind / capabilities
                                      │
Scanner ── classify(common first, catalog second) ──> media_items + exotic_tasks
                                      │
Thumbnail Router ── common ─────────> existing generator
        │
        └────────── exotic ─────────> placeholder / Scheduler wake
                                      │
Coordinator <── scan/install/activate/update/config/retry events
     │
     └── Pipeline ─> atomic claim ─> WorkerSupervisor ─> validated result ─> Sink
                                                                  │
                                                                  └─> thumb cache + DB + layout event
```

### 6.1 常见格式优先

`classify_media_type` 先执行现有常见格式表。仅返回 `None` 时查询 Catalog；Catalog 中标为 `override_common=false` 的插件不能覆盖常见格式。格式冲突由签名 Catalog 在发布侧拒绝，不在客户端按“版本更高者”临时决胜。

### 6.2 主缩略图让路边界

新增 `thumbnail/router.rs`：

```rust
pub enum ThumbnailRoute {
    Existing,
    Common,
    Exotic(FormatResolution),
}
```

`ipc/thumbnail_commands.rs` 的单项、批量、全库生成统一调用 Router。`Exotic` 且任务未完成时：不调用 `generate_thumbnail/decode_media_step`，不写失败状态；向 Coordinator 发 wake，返回可显示的门控/处理中状态。这样无需给纯 generator 注入 `AppState`。

### 6.3 跨流水线门控

- CLIP/人脸 Producer：存在未完成 exotic thumbnail 任务时不领取；任务完成后优先读取生成的 WebP。
- 主派生 backfill：若同一产物已由 exotic capability 认领，则不建立重复 `video_cover/doc_thumb/audio_cover` 任务。
- 优先级：`scan > common thumbnail > interaction(前台) > { common derivation, exotic 同级·共享全局并发池 } > AI/face`（R4：derivation 与 exotic 同级，防大视频库饿死 exotic）。
- `ai_yield_blockers()` 追加 exotic token（使 AI/人脸让步给 exotic）；**同时**新增 `should_yield_exotic() = scan || thumbnail || interaction`（exotic 子进程派发前让步，**不含** derivation）。二者都要，落地见 Part1§2.4 / Part2§3.6（R1）。

## 7. 调度与失效

Coordinator 使用有界事件通道；以下事件均调用同一个幂等 `wake(reason)`：

- App 启动与孤儿恢复；
- 扫描事务提交了新的 exotic task；
- Catalog 刷新后完成任务 backfill；
- 插件安装、升级、修复、激活；
- 用户恢复自动处理或修改并发配置；
- retryable task 到达 `next_retry_at`。

同一时刻只允许一条 Pipeline。领取必须在数据库事务中完成；禁止“先 SELECT、后批量 UPDATE”的竞态窗口。暂停只停止新领取；停止取消在途任务并回滚为 pending；退出负责终止全部 Worker。

重试策略：进程崩溃、暂时 IO 错误、超时进入指数退避；不支持格式变体、输出非法、许可失效进入 terminal 或 gating 状态。插件连续崩溃达到阈值后熔断，等待用户修复/升级。

## 8. Worker 协议与信任边界

### 8.1 统一帧

禁止“JSON 行 + 另一套二进制长度前缀”混合协议。每帧统一为：

```text
magic[4] | protocol_u16 | frame_type_u16 | request_id_u64 |
json_len_u32 | blob_len_u32 | json_bytes | blob_bytes
```

- JSON 最大 1 MiB；Blob 最大 64 MiB；超限立即杀 Worker。
- Host 生成单调 request_id；响应必须匹配当前在途请求。
- stdout 只允许协议帧；日志只写 stderr。Host 持续排空 stderr，防止管道写满死锁。
- 每个 Worker 同时一个任务；并发通过进程池实现。
- Supervisor 持有 `Child`；stdout/stderr 独立读取线程回传 channel。`recv_timeout` 后 Supervisor 可直接 kill 并回收进程。

### 8.2 输出验证

Host 不信任 Worker 返回值：检查 item/request ID、能力类型、长度、WebP 魔数、可解码尺寸、像素上限。产物写临时文件并 `fsync/flush + atomic rename`；完成落盘后才在同一 DB 事务更新 task 与 `media_items.thumb_*`。

### 8.3 原生代码边界

进程隔离只解决崩溃、死循环和部分资源约束。v3 Worker 是产品厂商签名的受信任原生代码，拥有与主程序近似的用户权限。UI 与文档必须明确此事实。

第三方 Worker 上架前置条件：完成 OS 权限沙箱、文件句柄/只读副本输入、默认断网、资源强限制与独立安全审计。未完成前，Registry 拒绝非厂商签名 key。

## 9. 分发与合规原则

- Registry index 必须签名，包含 `schema/key_id/sequence/generated_at/expires_at`；客户端拒绝回滚 sequence。
- Package 使用签名 `package-manifest.json`，列出每个文件的规范相对路径、大小、SHA-256、类型与执行位；拒绝未列出的额外文件。
- 安装只在 staging 目录进行：校验路径 → 解包 → 全文件复核 → 平台代码签名检查 → 原子替换；保留上一个版本用于失败回滚。
- Release 中不存在 SQLite 可开启的授权绕过开关。
- 法律判断不得由 `commercial_ok: bool` 自声明替代。每个插件版本必须附 SPDX SBOM、第三方许可证、必要源码/重链接材料、专利评估记录与审核编号。
- “子进程调用 GPL 程序”“动态链接 LGPL”“委托 OS 解码”均只作为待法务确认的工程候选，不写成自动合规结论。

## 10. 路线图

| Part | Phase | 交付 |
|---|---|---|
| Part1 | P0–P2 | PSD 技术探针、Catalog、task schema、扫描、Router、跨流水线门控 |
| Part2 | P3–P4 | 统一协议、PSD Worker、Supervisor、Coordinator、Pipeline、Sink |
| Part3 | P5–P6 | License、签名 Registry、Package 安全安装、升级/卸载、平台签名 |
| Part4 | P7–P9 | 前端状态机、正式 E2E、攻击测试、性能门禁、发布材料 |

不得跳过 Part1 的 PSD 技术探针。若选定 crate 不支持 PSB/CMYK/16-bit，则缩小首发 manifest；不能先宣称支持、后靠测试碰运气。

## 11. 全集完成定义

1. 全新离线安装扫描 PSD：可从内置 Catalog 得到 `AvailableUninstalled`，显示购买占位，主解码器不报错。
2. 正式 Release 不含 dev 授权旁路；有效 token 激活后自动唤醒处理，无需重启或手工点击开始。
3. 修改 PSD、升级 Worker、改变目标档位均触发正确失效与重做。
4. PSD 未完成前不进入 CLIP/人脸；RMVB 等后续插件不会与主派生重复处理。
5. 恶意 ZIP、路径穿越、额外 DLL、签名篡改、Registry 回滚、超长 IPC 帧全部被自动测试拦截。
6. Worker 崩溃/超时/非法输出不留下半文件、processing 孤儿或主进程崩溃。
7. 固定常见格式基准中：无 exotic 工作时吞吐回退 ≤2%，启动额外耗时 ≤20 ms，空闲时无 Worker 进程。
8. Windows Worker 通过 Authenticode；macOS Worker 通过 codesign/notarization/Gatekeeper 实机验证。
9. PSD 发布包具备 SBOM、许可证材料、审核记录、可复现 hash 与回滚包。

## 12. 风险登记

| 风险 | 等级 | 处理 |
|---|---|---|
| 原生 Worker 权限过大 | 高 | v3 限厂商签名；第三方生态延后至 OS 沙箱完成 |
| 包供应链/更新回滚 | 高 | 签名 index + sequence + 全文件清单 + staging 原子安装 |
| 状态与缓存失效错误 | 高 | 能力级任务 + 输入指纹 + 源/版本/配置失效测试 |
| 跨流水线重复处理 | 高 | Task EXISTS 门控 + 固定优先级 + 集成测试 |
| PSD 解码能力不完整 | 中 | P0 样本矩阵技术探针；按证据收缩首发范围 |
| 百万库任务查询退化 | 中 | 独立窄表/覆盖索引/EXPLAIN 与百万数据基准 |
| 离线 License 被逆向 | 中、接受 | 无显式旁路；承认本地校验最终可被修改 |
| 法律/专利判断错误 | 高 | 精确依赖版本材料 + 专业审核；文档不作自动结论 |
