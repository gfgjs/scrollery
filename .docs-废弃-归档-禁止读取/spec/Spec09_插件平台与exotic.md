---
id: 2026-07-24-Spec09_插件平台与exotic
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec09-插件平台与exotic

> 本篇讲 exotic 冷门格式插件平台（Catalog/授权/Registry/安装/任务队列）+ 格式并集层 + 私有/公开两仓拓扑的 **as-built 现状**。服务读者：要扩展/接入新格式插件的人类工程师，以及只读本篇 + 所引 `path:line` 就要重建该子系统的低能力 LLM 代理。读前建议先扫一遍 [Part6 插件平台与 exotic 收尾](../refactor_2026/Part6_插件平台与exotic收尾.md) 的 §1（目标）以建立历史脉络——但**本篇正文以现码为准**，Part6 只在「为什么这样设计」处被引用，不复制其论证。

---

## 1. 概览

exotic 是 Scrollery 的**冷门格式插件平台**：把 PSD、CAD 等主程序不内置解码的格式，以独立子进程 Worker 的形式接入缩略图/派生流水线，同时承载 AI/人脸 worker 的授权与调度框架。它维护「三份互不推导的真相」（能力 / 安装 / 处理，见 §2）并把三者折叠成一个前端可读的可用态。

**代码位置**：

| 目录/文件 | 职责 |
|---|---|
| `src-tauri/src/exotic/` | 平台核心：Catalog、Registry、安装、任务队列、Coordinator、Supervisor、License |
| `src-tauri/src/formats/mod.rs` | 已注册格式运行时并集（内置表 ∪ exotic Catalog 投影） |
| `src-tauri/src/ipc/exotic_commands.rs` | 20 条 Tauri 命令（安装/激活/处理控制/Registry 刷新） |
| `src-tauri/src/db/queries/exotic.rs` | 三表的 DAO（任务领取/续租/插件安装记录） |
| `crates/exotic-protocol` | host↔worker 共享的帧/消息协议（纯库，无密钥材料） |
| `crates/scrollery-plugin-api` | 叶 crate：`EntitlementProvider` 抽象 + DTO，零密钥 |
| `crates/scrollery-exotic-trust` | Ed25519 验签原语（`VerifyingKeyset`/`verify_token`） |
| `crates/exotic-workers/{psd-worker,psd-probe,ai-worker,...}` | Worker 二进制（PSD 解码 / 探测 / AI 推理，各自独立 `Cargo.toml`） |

**在整机中的位置**：

```
前端插件商店/格式弹层
      │ IPC (exotic_commands.rs, 20 条)
      ▼
ExoticHost（能力+安装+授权三份真相折叠）──resolve_format/is_task_runnable──> Coordinator
      │                                                                        │
      ├─ CatalogStore（内置 JSON + Registry 投影）                              │ PluginDescriptor 注册表
      ├─ EntitlementProvider（组合根单点装配：KeyringLicenseStore；            │ 逐插件调度
      │  信任根解析失败→FreeStubEntitlement fail-closed 回退）                  ▼
      └─ exotic_plugins / exotic_tasks（DB，三份真相之二）              Supervisor 拉起 Worker 子进程
                                                                          │ stdin/stdout 长度前缀帧
                                                                          ▼
                                                                  psd-worker / ai-worker（独立进程）
```

`formats::merged_formats` 是旁支：扫描器/格式弹层要「全部已注册格式」时，把内置表与 `CatalogSnapshot` 投影合并，exotic 本身对此无感知（详见 §3.5）。

---

## 2. 数据模型与状态

### 2.1 三份真相（不互相推导）

| 真相 | 载体 | 回答什么 |
|---|---|---|
| 能力真相 | `CatalogOffering` / `CatalogSnapshot`（内存快照）+ `exotic_catalog_formats` 表 | 某扩展名有没有产品、哪个插件、哪些能力、哪些平台 |
| 安装真相 | `exotic_plugins` 表 / `InstalledPluginRecord` | 磁盘装了什么版本、manifest_hash 是什么、状态是否损坏 |
| 处理真相 | `exotic_tasks` 表 / `ExoticTaskRow` | 某 (item, plugin, capability) 处理到哪一步、是否需要重试 |

三表设计详见 `src-tauri/src/db/schema.rs:454-530`（Schema V9）；全表权威定义见 [Spec01 数据层](./Spec01_数据层.md)，本节只摘关键列。

**`exotic_catalog_formats`**（`schema.rs:469-481`）：`format`(PK，小写扩展名) / `plugin_id` / `capabilities_json` / `license_tier` / `platforms_json` / `catalog_sequence`（防目录回滚）/ `source`(builtin|remote)。

**`exotic_plugins`**（`schema.rs:484-492`）：`plugin_id`(PK) / `version` / `manifest_hash` / `package_sequence`（**防包回滚，安装只许更高**）/ `install_state`(installed|disabled|broken)。（V12 追加的 `entitlement_source` 渠道列已随渠道维度退役，已随 P23 连同 DDL/SQL/模型一并删除。）

**`exotic_tasks`**（`schema.rs:497-515`）：`item_id`+`plugin_id`+`capability` 唯一约束；`status`(0 pending/1 processing/2 done/3 retryable_error/4 terminal_error，`exotic/task.rs:13-24`)；`input_fingerprint`（源/参数变化即失效，见 §3.4）；`claimed_at`+`lease_owner`（跨实例租约，见 §3.6）。索引 `idx_exotic_tasks_ready`(plugin_id,capability,status,next_retry_at) 供领取查询、`idx_exotic_tasks_item` 供跨流水线门控（CLIP/face 的 `NOT EXISTS` 判 exotic 是否已 done）。

### 2.2 内存态结构

- `ExoticHost`（`exotic/mod.rs:191-202`）：组合 `catalog: Arc<CatalogStore>` + `installed: Arc<dyn InstalledSource>` + `licenses: Arc<dyn EntitlementProvider>`；`CatalogStore` 内部 `RwLock<Arc<CatalogSnapshot>>`（`catalog.rs:317-345`），热路径一次读锁 + `Arc::clone`，刷新时整体替换（禁半更新）。
- `Availability` 九态枚举（`exotic/mod.rs:50-69`）：`AvailableUninstalled`/`InstalledUnlicensed`/`Authorized`/`LicenseExpired`/`UnsupportedPlatform`/`IncompatibleHost`/`InvalidInstallation`/`Disabled`/`NoOffering`。
- `RegistryCache`（`registry.rs:278`）：本地磁盘缓存 index.json/.sig/.seq，`load_verified` 每次带 `VerifyingKeyset` 重新验签（不信任磁盘明文）。

### 2.3 状态归属

| 状态 | 归属 |
|---|---|
| Catalog 内置数据 | 编译期 `include_str!("../../resources/exotic-catalog.json")`（`catalog.rs:21`） |
| Catalog 运行时快照 | 进程内存 `RwLock<Arc<CatalogSnapshot>>`，不落盘 |
| 安装/任务真相 | SQLite（`exotic_plugins`/`exotic_tasks`），WAL + busy_timeout |
| License token | OS keyring（`service="scrollery"`，见 `scrollery-plugin-api::KEYRING_SERVICE`），**DB 不存 token** |
| Registry 缓存 | `<app_data>/exotic/registry/`（`index.json`/`.sig`/`.seq`，同卷 tmp→rename） |
| 已安装插件二进制 | `<app_data>/exotic/plugins/<plugin_id>/`（current/staging/backup 三态目录，见 §3.3） |

---

## 3. 关键流程与算法

### 3.1 Catalog reconcile（能力真相装载）

`CatalogSnapshot::parse`（`catalog.rs:196` 起）严格校验、**整体拒绝不部分接受**：

1. schema 版本必须等于 `SUPPORTED_CATALOG_SCHEMA`(=1)，否则 `UnsupportedSchema`。
2. 逐 offering 校验：`plugin_id`/`format` 字符集、`capabilities` 非空、`override_common` 客户端恒拒（`OverrideCommonNotAllowed`——该权限只存在于内置审核表）。
3. **撞常见格式即拒绝整表**（`CommonFormatConflict`，`catalog.rs:193-195` 注释）：用 `classify_media_type` 判断某 format 是否已被内置认领；这是纵深防御，挡住「远程 Catalog 伪装成 jpg/mp4 抢劫解码器」。
4. `builtin` 分发形态（`RawOffering.distribution == "builtin"`，D-OCR-5，如 exotic-ocr）：无安装包的能力插件，`format` 字符串是标记而非真实扩展名——`formats::merged_formats` 过滤掉它（`formats/mod.rs:63-64`），但 `resolve_format`/`is_task_runnable` 仍可直接解析（授权通路不受影响）。

远程 Catalog 走同一 `parse` 路径（三面过滤：`override_common` 拒绝 / 常见格式冲突拒绝 / schema 不符拒绝），无单独宽松通道——**「三面过滤勿回退」是本子系统红线**，理由见 Part6 §1.2「不在本 Part」+ 联动 review §8.1 决策1 的纵深防御论证。

### 3.2 授权判定（三真相折叠）

`ExoticHost::availability_of`（`exotic/mod.rs:390-442`）门控链，**顺序即防御纵深**：

```
平台不支持(target triple) → UnsupportedPlatform
  │
Host 版本不满足 min_host_version → IncompatibleHost
  │
[仅 debug+exotic-dev-fixtures] fixture 命中 → Authorized（Release 编译期不存在此分支）
  │
builtin offering？
  ├─ 是 → 无 sku → InstalledUnlicensed；有 sku → 直接查 EntitlementProvider.evaluate
  └─ 否 → 未安装 → AvailableUninstalled
           已安装但 broken/disabled → InvalidInstallation/Disabled
           已安装且启用 → 无 sku → InstalledUnlicensed；有 sku → 查 EntitlementProvider.evaluate
```

`is_task_runnable(plugin_id, capability)`（`exotic/mod.rs:457-470`）是 Coordinator 领任务前的门：`Authorized` 才放行，未安装/未授权/平台不支持/禁用/损坏一律 `false`——**这些状态不写任务行**，由此函数拦在领取前（§2.1 处理真相的定义边界）。

`EntitlementProvider` trait（`crates/scrollery-plugin-api/src/lib.rs`）：`evaluate`/`source_tag`/`activate`/`deactivate`；`activate`/`deactivate` 默认 fail-closed（`ActivationUnsupported`），只有真实实现（keyring 直销 `KeyringLicenseStore`）才覆写。`source_tag` 当前取值 `"direct"`（直销）/ `"free"`（未授权回退桩）。

### 3.3 Registry 拉取 + 安装编排

**Registry 验签**（`registry.rs:149` `verify_and_parse`）顺序：大小封顶(4MB) → **验签先于解析**（`VerifyingKeyset::verify_any`，`KeyPurpose::Release`）→ 反序列化 → schema 校验 → 逐条目校验 → 单调防回滚（`registry_sequence` 小于本地已接受值即 `RollbackRejected`）→ 过期标志（过期仍返回供展示，但安装路径拒绝）。

`fetch_exotic_registry` IPC（`ipc/exotic_commands.rs:475-506`）：命令**只触发**、不接受任何 URL（下载坐标由 `DEFAULT_REGISTRY_BASE_URL` 编译期常量决定，可用 `PICASA_REGISTRY_BASE` 环境变量覆盖），下载 `index.json`+`index.sig` 原始字节 → `RegistryCache::accept` 验签+防回滚+原子写缓存。命令体经 `state.exotic_install_lock` 与 install/uninstall/rollback 串行，防止「拉取改写缓存」与「安装读缓存」竞态撕裂。

**安装编排** `install_staged_zip`（`installer.rs:119-` 起）步骤：

1. 直接进入验签+解包——P16 后 `install_staged_zip` **已无来源参数**，唯一渠道 = 直销，无渠道 fail-closed 早退分支。
2. 验签+解包（`install::verify_and_extract`）：zip 加固（防 zip-bomb：压缩比/总解压量/文件数上限，`InstallLimits`）+ 清单白名单 + 逐文件 hash。
3. `check_catalog_subset`：manifest 声明的 formats/capabilities 必须是 Catalog 子集，commit 前拒绝——插件不得自我声明 Catalog 未授权的能力。
4. `commit_install`（`install.rs:398`）：原子目录切换 `current` → `backup`，`staging` → `current`（同卷 rename）。
5. `upsert_exotic_plugin` 写 DB（仅此步持锁），成功后 `discard_backup`；任一步失败走 `rollback_to_backup`，**不写 DB**。

命令层前置：`quiesce_exotic`（8 秒，暂停 Coordinator + kill Worker + 释放 Win 句柄，否则 Windows 下占用目录无法改名）。模型权重 blob（`RegistryEntry.model_blobs: Vec<ModelBlob>`，`registry.rs:96-114`）独立于 zip 分步下载，**排在 zip 之前**（`exotic_commands.rs:595-611`）：不进 zip 故 `InstallLimits` 的压缩比检查天然不适用，下载幂等可跳过已就位文件，失败无 staging 残留需清理。

### 3.4 任务队列（处理真相状态机）

```
pending(0) ──claim_exotic_tasks(原子UPDATE...RETURNING)──> processing(1)
   ▲                                                            │
   │                                                    ┌───────┴───────┐
   │                                                    ▼               ▼
   │                                              done(2)         retryable_error(3) / terminal_error(4)
   │                                                                     │
   └─────── SourceChanged(input_fingerprint 变化) 重置为 pending ────────┘
```

`claim_exotic_tasks`（`db/queries/exotic.rs:160-187`）单条 `UPDATE...SELECT...RETURNING` 完成领取，避免 SELECT/UPDATE 之间竞态窗口；`WHERE` 子句含隐藏根排除（V21，与缩略图/AI/派生同口径：隐藏根下的 exotic 任务不烧解码算力）。领取写 `claimed_at`+`lease_owner`（进程级 `instance_id`，仅内存生成）。

`input_fingerprint`（`exotic/fingerprint.rs`）= 规范 JSON 序列化后 SHA-256，字段冻结顺序：`fingerprint_schema`/`media_cache_key`/`plugin_id`/`worker_version`/`capability`/`capability_api_version`/`settings`（含吸附后档位 `target_tier`，复用 `thumbnail::generator::snap_to_tier`，不另写档位算法）。源文件变化、Worker 升级、能力 API 版本变化都使旧 `done` 失效——这是本子系统缓存判据的唯一权威计算处。

### 3.5 格式并集层（`formats::merged_formats`）

```rust
pub fn merged_formats(catalog: &CatalogSnapshot) -> Vec<FormatDescriptor> {
    builtin_formats() ∪ catalog.iter_formats().filter(|(_, off)| !off.builtin)
}
```
（`formats/mod.rs:54-81`）内置 ∪ 非 builtin-distribution 的 Catalog 条目，按 `ext` 升序、**不写死总数**（当前 66 内置 + 1 PSD 是数据快照非协议常量）。`formats` 模块独立存在的原因是避免循环依赖：`utils::format` 不能引用 `exotic::catalog`（反向依赖已存在于 Catalog 装载期的 `classify_media_type` 冲突检测），故合并层单独放一处向下依赖两侧（`formats/mod.rs:4-7` 注释）。

### 3.6 并发/线程模型

- **DB**：所有 exotic 查询走 `rusqlite`，写操作在 spawn_blocking 或专用 DB 线程（项目铁律，见 [Spec14 不变量与约定](./Spec14_不变量与约定.md)）。
- **Coordinator**（`coordinator.rs`）：单一调度器，有界事件通道 + `dirty` 原子位（通道满时不静默丢弃最后一次唤醒）；串行循环，天然保证「两个并发 start 只启动一条 Pipeline」。`PluginDescriptor` 注册表（plugin_id/worker_id/capabilities/handshake_timeout/uses_gpu）驱动多插件调度（PSD 硬编码已全部移出调度路径，psd-worker 仍是注册表一员）。
- **Supervisor**（`supervisor.rs:1-17` 头注）：每个 Supervisor 同一时刻只处理一个请求；并发由进程池（Pipeline 层）实现；子进程持有 stdout reader 线程（FrameResult channel）+ stderr drain 线程（有界 64KiB 环形缓冲，防写满死锁）；超时/断开/协议违例 → kill→wait 标死，Pipeline 做崩溃退避补新实例；Drop 兜底 kill+wait+join，绝不留孤儿进程。
- **跨实例互斥**：`lease_owner` 防跨实例任务覆盖（多实例场景下 `renew_exotic_lease`/`finish`/`fail` 均带 `WHERE lease_owner=?`，失去租约的旧结果只能丢弃，`db/queries/exotic.rs:189-249`）。
- **BackgroundHeavyLimiter**（`limiter.rs`）：FIFO 公平并发池，exotic Worker 请求与派生（derivation）重型任务共享同一票号队列，谁也不硬让步谁——防大视频库派生饿死 exotic。GPU 令牌（`GpuToken`，Part4 D2 定案）复用同一骨架，额度恒 1，**留在主进程**（跨进程 GpuToken 帧的设计已被 D2 推翻，AI/face 详见 [Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md)）。

### 3.7 两仓拓扑（as-built）

**现状**：本仓 `C:\workspace\scrollery` 是 **canonical 源**，公开镜像由已提交树快照单向投影。投影**只做内部文件过滤**（内部文档、代理配置、工作记忆、缓存及内部治理工具），代码与 `Cargo.lock` 原样同步——公开树与私有树**同构**，不再有源码剥离或 lock 重写。过滤与导出共用 `scripts/publish-oss.ps1`；现行说明见 [Part6_3c](../refactor_2026/Part6_3c_Copybara同步配置草稿.md)。

```
canonical 仓（本仓）── 唯一源真相
   全 workspace（含授权实现）+ 内部文件
        │  已提交树快照（单向、私有源只读）
        │  origin_files glob 排除内部文档/配置；代码与 lock 原样投影
        ▼
公开镜像仓 ── 纯投影，仅 sync-bot 提交
   同一份源码与 Cargo.lock；门禁 = 原锁 --locked 构建测试 + gitleaks
```

**公开边界**（Part0 §10）：源码开放不等于功能免费——付费插件载荷、官方签名构建、托管更新与支持仍是商业边界；**验签公钥可公开，私钥与私密签发凭证不入源码/公开镜像**。

---

## 4. 契约与不变量（施工红线）

### 4.1 IPC 命令面（20 条，`ipc/exotic_commands.rs`，全量见 [Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)）

| 命令 | 要点 |
|---|---|
| `list_exotic_format_resolutions` | 全部 Catalog 格式的可用态（含未安装的 `AvailableUninstalled` 占位） |
| `get_exotic_item_state` | 单媒体项的 exotic 处理态 |
| `list_installed_exotic_plugins` | 已装插件（安装真相投影） |
| `get_plugin_entitlement` | 前端 gate/购买引导判定 DTO（`PluginEntitlement`：availability+source_tag+sku+store_url） |
| `start/pause/stop_exotic_processing` | Coordinator 处理开关 |
| `get_exotic_processing_status` | 处理进度快照 |
| `list_exotic_task_details` | 任务明细（诊断/前端列表） |
| `retry_exotic_task` / `retry_exotic_plugin_failures` | 单任务/整插件失败重试 |
| `activate_exotic_plugin` / `deactivate_exotic_plugin` | License 激活/撤销 |
| `fetch_exotic_registry` | 拉远程签名 index → 验签 → 写缓存（P0 闭环，§3.3） |
| `list_exotic_registry` | 读本地已验签缓存 |
| `install_exotic_plugin`（单一定义，`exotic_commands.rs`） | 从已验签 Registry 选条目 → HTTPS 下载（size/sha256 校验）→ 安全安装；P16 后无渠道 `#[cfg]` 变体 |
| `repair_exotic_plugin` / `rollback_exotic_plugin` / `uninstall_exotic_plugin` | 安装态修复/回滚/卸载 |

错误一律 `AppError::Exotic { code, message }`，`code` 稳定跨 IPC（如 `rollback_rejected`/`bad_signature`/`no_registry_cache`/`registry_expired`/`plugin_not_in_registry`/`invalid_plugin_id`）。

### 4.2 不变量清单

| 不变量 | 为什么违反会怎样 |
|---|---|
| **先验签再解析**（Registry/Package manifest） | 未验签数据驱动下载/安装 = 任意远程代码执行面；`verify_and_parse`/`verify_manifest` 均把验签放在反序列化之前 |
| **`package_sequence`/`registry_sequence` 单调防回滚** | 缺此不变量则攻击者可重放旧签名包降级到已知漏洞版本；`RollbackRejected` 是唯一合法拒绝路径，不可绕过（`fetch_exotic_registry` 命令注释明确标注「安全红线，不可绕过」） |
| **三面过滤（override_common 拒绝/常见格式冲突拒绝/schema 不符拒绝）勿回退** | Catalog 若允许远程覆盖常见格式，等于允许远程劫持内置解码器路由 |
| **未安装/未授权/禁用不写任务状态** | 若写了，普通媒体会被误标"待处理"，且撤销授权后旧任务行残留造成状态泄漏 |
| **`lease_owner` 跨实例互斥** | 缺此不变量，多实例并发写会互相踩任务、"槽空=保留终态发布权"语义失效 |
| **`is_task_runnable` 门控顺序**（平台→Host版本→fixture→安装→授权） | 顺序即防御纵深；例如授权判定若跑在安装态判定之前，会在插件未安装时误报"已装未授权"而非"可购买" |
| **`ExoticHost.authorized_fixture` 字段整体 `#[cfg]` 门控** | Release 构建下该字段编译期不存在，避免"运行时误判 debug 分支"类回归；这是比运行时开关更强的防御纵深（`exotic/mod.rs:195-202`） |
| **公开树与私有树同构** | 两侧同一份 manifest 与依赖图，构建期不存在「某 crate 缺失」的分叉；公开门禁 = 原锁 `--locked` 构建测试 + gitleaks 密钥扫描 |
| **授权实现归一**（`KeyringLicenseStore` 唯一实现 + `FreeStubEntitlement` fail-closed 回退） | 同一目标只保留一套主流程；未授权一律 fail-closed 恒 `Unlicensed`。**源码公开不等于功能免费**——商业边界是私钥/签发凭证与付费载荷，非验签算法可见性 |
| **组合根单点装配**（`default_entitlement_provider`） | 授权 provider 装配收敛到单一函数，无渠道分叉 |

### 4.3 「不可擅改」项

- 三份真相互不推导（`exotic_catalog_formats`/`exotic_plugins`/`exotic_tasks` 分表）——见 `schema.rs:457-465` 头注，理由链见 Part6 §3 正文。
- `registry.rs::verify_and_parse` **对收到的原始 index bytes 先验签**、缓存原样落盘（解析结果不重序列化回写）——删改 index 字段不影响验签结论，此顺序不得倒置（理由见 Part6 §8.4）。
- `builtin` 分发形态跳过安装态门直接验 license（D-OCR-5）——与 package 流的 `KeyringUnavailable` 分叉处理是**有意分叉**（`mod.rs:414-419` 注释 + 测试 `builtin_offering_keyring_unavailable_diverges_from_package_flow` 明确"防未来'统一'两分支回归"）。

---

## 5. 边界情况与失败模式

| 边界 | 处理 |
|---|---|
| 全新设备无 Registry 缓存 | `list_exotic_registry` 返回空列表；`install_exotic_plugin` 报 `no_registry_cache`；须先调 `fetch_exotic_registry` |
| Registry 已过期 | `list_exotic_registry` 仍返回缓存内容（展示用），但 `install_exotic_plugin` 检测 `verified.expired` 即拒绝新安装（`registry_expired`） |
| 收到更低 `registry_sequence`/`package_sequence`（回滚攻击/CDN 故障返旧数据） | `RollbackRejected`，缓存/DB 不变，报错，不静默接受 |
| 安装校验失败（签名/清单/子集不符） | 回滚到 `backup` 目录，DB 不写；插件保持安装前状态 |
| 插件已装但 broken（完整性复核失败） | `InvalidInstallation`，前端提示修复（`repair_exotic_plugin`） |
| keyring 读取失败（系统凭据库不可用） | `LicenseStatus::KeyringUnavailable`；package 流按 `InstalledUnlicensed` 处理，builtin 流按 `AvailableUninstalled` 处理（**两者有意不同**，见 §4.3） |
| 系统时钟早于 UNIX_EPOCH（VM/容器时钟异常） | `now_secs()` 返回 0，fail-closed（0 < 任何合法 `not_before`，一律未授权），同时告警日志区分"时钟异常"与"token 问题"（`mod.rs:506-517`） |
| 并发安装/卸载/回滚/Registry 刷新 | `state.exotic_install_lock` 串行化，防目录/缓存撕裂 |
| 跨实例任务领取冲突 | `lease_owner` 原子 `UPDATE...RETURNING` 领取，失去租约的旧结果 `WHERE lease_owner=?` 直接丢弃更新 |
| 任务取消 | `BackgroundHeavyLimiter::acquire` 周期性检查 `CancellationToken`，取消即退队不泄漏票 |
| Worker 崩溃/协议违例 | Supervisor kill→wait 标死，Pipeline 崩溃退避后补新实例；stderr 最近 64KiB 保留供诊断 |
| Model blob 下载中断（大文件 GB 级） | 断点续传（通用下载引擎），sha256 最终校验，失败不影响已安装的插件二进制本体 |

---

## 6. 重建指引（从零实现）

### 6.1 依赖顺序

1. **`exotic-protocol`**（纯库，无外部服务依赖）：帧类型 + 消息结构，host 与 worker 二进制共享。
2. **`scrollery-plugin-api`**（叶 crate，零依赖除 `thiserror`）：`EntitlementProvider` trait + DTO。
3. **`scrollery-exotic-trust`**（依赖 `ring`）：`VerifyingKeyset`/`verify_token` 验签原语。
4. **`exotic/license.rs`**（src-tauri 内）：keyring 直销实现 + `FreeStubEntitlement` fail-closed 未授权回退（原 `channel_stubs` 模块已随 P16 删除）。
5. **`src-tauri/src/exotic/{catalog,registry,package,install,installer}.rs`**：能力/安装真相 + 校验链。
6. **DB Schema V9**（三表）→ `db/queries/exotic.rs`（DAO）。
7. **`coordinator.rs`+`supervisor.rs`+`worker.rs`**：调度 + 子进程生命周期。
8. **`ipc/exotic_commands.rs`**：20 条命令注册进 `ipc/registry.rs`。
9. **授权装配**：`KeyringLicenseStore` 实现 plugin-api trait（唯一实现），组合根单点装配（信任根解析失败降级 fail-closed 回退）。
10. **公开快照**：共享脚本过滤内部文件，代码与 lock 原样；公开 push 前扫描拟公开树及提交元数据，公开 gate 通过后快进 main。

### 6.2 外部 crate

| crate | 用途 |
|---|---|
| `ring` 0.17 | Ed25519 验签（`verify_strict`，非 dalek，见 Part6 §1.3 理由） |
| `base64` 0.22 | token/index 签名的 base64url 编解码 |
| `sha2` 0.10 | 包/blob 完整性校验 |
| `zip` 2 | 插件包解包（同时是 EPUB 容器库，见 [Spec07](./Spec07_文档与阅读器.md)） |
| `keyring` 3 | OS 凭据存储（token 落地，DB 不存） |
| `reqwest` 0.12 (rustls) | Registry/包/model blob 下载 |
| `rusqlite`/`r2d2` | 三表 DAO |

### 6.3 坑与教训（链 experience.md）

- Cargo 会解析并锁定**所有**声明依赖（含 optional path dep）——已废止的「公开仓 manifest 不写某 crate」方案即因目录缺失令 `cargo metadata` 失败；当今公开树与私有树同构，不存在该分叉。
- workspace `members` 禁用 `crates/*` glob：会命中无 `Cargo.toml` 的目录、且只展开一层漏掉两层深的 worker crate（Part6 §3.9.1 P0-5）。
- 组合根授权 provider 装配必须收敛单点函数，否则「读路径走注入 provider、写路径另建 keyset」会导致信任根分裂（Part6 R1-1 裁决，`scrollery-plugin-api/src/lib.rs:112-116` 注释）。
- 更多跨子系统通用教训见 `docs/experience.md`（工具/门禁/正则扫描类，非 exotic 专属，此处不重复）。

### 6.4 验收

- 单测：`src-tauri/src/exotic/mod.rs` 内 `#[cfg(test)] mod tests`（授权折叠矩阵，含 builtin 分叉专测）；`installer.rs`/`registry.rs`/`catalog.rs` 各自模块内测试（校验链、防回滚、三面过滤）。
- 集成：`db/queries/exotic.rs` 内 `claim_exotic_tasks`/`renew_exotic_lease` 的跨实例互斥测试（第 838 行注释"别的实例续租不到本实例任务"）。
- 门禁：项目根 `.github/workflows/ci.yml`（本仓侧）+ 公开镜像侧 `oss-gate.yml`（`cargo check/test --workspace --locked` + gitleaks 密钥扫描，见 Part6_3c）。
- 待核实：本次摸底未逐条重跑 `cargo test`（属摸底/撰写任务，不做施工验证）；exotic 相关测试总数以最近一次门禁记录为准，未在本篇现场核实计数。

---

## 7. 关联

- 上游正典：[Part6 插件平台与 exotic 收尾](../refactor_2026/Part6_插件平台与exotic收尾.md)（§1 目标范围、§3 设计理由、§8 联动 review 裁决——协议 G1-G6/AES/EntitlementProvider 的架构决策论证在此，本篇不复制）。
- 公开快照同步细节：[Part6_3c 同步现行说明](../refactor_2026/Part6_3c_Copybara同步配置草稿.md)（排除面、操作入口、公开门禁、运维注意）。
- IPC 全表/错误码权威：[Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)。
- 商业化/许可基建；第三方商店渠道属未来路线（当前无渠道代码预留）：[Spec13 构建发布商业化](./Spec13_构建发布商业化.md)。
- DB Schema 全量：[Spec01 数据层](./Spec01_数据层.md)。
- AI/人脸 worker 的推理协议细节（`exotic-protocol` 的 SessionInit/EmbedBatch/EncodeText op 家族、GpuToken、模型权重传输）：[Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md)（本篇只讲平台承载框架，不讲推理本身）。
- 前端插件商店 UI：[Spec11 前端架构](./Spec11_前端架构.md)。
