---
id: 2026-06-26-Part6_插件平台与exotic收尾
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 6 · 插件平台与 exotic 收尾

> 依赖：[Part0](Part0_总纲与产品定稿.md)（§5.5 关键架构决策、§8 防护层1、§9 多渠道分发预留、§10 开源策略与依赖合规）。
> 被依赖（供应方）：[Part4](Part4_AI与人脸插件化.md)（消费本 Part 的 G1-G6 框架升级 + AES 原语 + ai/face worker 接入）、[Part5](Part5_前端体验重构.md)（消费本 Part 的插件商店/gate IPC + EntitlementProvider 判定）。
> 状态：定稿待执行（已并入 terminal review；正文即权威，§8 为历史留痕）。执行前必读 Part0 §13 + 本文。**旧 docs/记忆不可轻信，以代码实测为准。**
>
> ⚠️ **术语澄清**：本文「Part6」= 重构方案第 6 部分；exotic 旧文档里的「P5/P6.x」是 exotic 子系统自身的里程碑编号（信任根/Registry/安装），**两套编号不同**，引用时注明。

---

## §1 目标与范围

### 1.1 本 Part 解决什么

exotic 子系统后端**已基本完成**（memory：P5 信任根/License + P6.1 签名 Registry + P6.3 包校验/安全解包 + P6.3b 原子切换 + P6.4a-e 安装编排/激活/全命令面/quiesce/下载/启动前完整性复核，162 测试绿）。本 Part 把它从「冷门格式专用」**升级为全插件平台地基**，承接 AI/face，并补防护与多渠道：

1. **exotic 收尾缺口**：① `fetch_exotic_registry` IPC（Part0 §1 实测缺口 #2：**全新设备 Registry 缓存空 → 装不了插件**，P0）；② R10 下载统一（AI/face 模型下载与 exotic 下载去重）。
2. 🔴 **插件平台框架升级承接 Part4 G1-G6**（Part4 §2.3 的 6 缺口落地）：① G1 协议扩展（`SessionInit/SessionReady/EmbedBatch`）；② G2 `GpuLimiter` 跨进程令牌（`GpuToken` 帧 + Coordinator Semaphore，**令牌在 `SessionInit` 即获取、持会话生命周期**，§3.6 P0-2 回写）；③ G3 model blob 大文件分发（`kind=model` + `InstallLimits` 豁免压缩比 + 分步下载）；④ G4 批量协议（**一 Request=一批 `EmbedBatch{item_ids, cache_keys}`，传 cache_keys 非 tensor blob、blob≈0、无「限批」**，§8.1 决策1；旧「86MB tensor / 限批 ≤128 / 调高 MAX_BLOB」方案 A 作废）；⑤ G5 **Coordinator 通用化**（去 `PSD_PLUGIN_ID` 硬编码 → plugin registry 驱动多插件）；⑥ G6 错误码扩展（`GpuUnavailable/SessionExpired/ModelLoadFailed/EmbedDimMismatch`，对齐 §3.2.2/§8.6 canonical；旧 `OrtError/GpuOom/ModelNotLoaded` 作废）；⑦ 长驻 session 生命周期（Supervisor 改造，握手超时 120–300s）。〔🔴 **CANON-03 回写**：§1.1 概述 ④/⑥ 已从初稿旧值回写至 §8 canonical，正文与 §8 一致。〕
3. 🔴 **AES 加密权重原语**（Part0 §8 防护层1，Part4 §3.7 消费）：`ring::aead` AES-256-GCM + license 派生密钥（HKDF `enc_seed`）；**主进程解密 + `commit_from_memory`**（Part4 §8 定方案）。
4. 🔴 **EntitlementProvider 抽象**（Part0 §9）：`EntitlementProvider` trait（交付源包装已删除）；当前唯一渠道 = 直销，实现为 keyring `KeyringLicenseStore` + 未授权回退 `FreeStubEntitlement`（两者同住 `exotic/license.rs`；空渠道桩已随 P16 删除，见 §3.11）；前端 **gate 判定 IPC**（供 Part5）。
5. **exotic→全插件平台**：ai-worker / face-worker 作为插件接入统一 Registry / Catalog（与 psd-worker 同框架）。

### 1.2 不在本 Part（归属其它 Part）

- **AI/人脸推理逻辑本身**（worker 内 ort/CLIP/YuNet 前向）→ **Part4**（本 Part 提供框架，Part4 实现推理）。
- **插件商店 / gate 前端 UI** → **Part5**（本 Part 提供 IPC + 判定）。
- **平台签名 / 公证 / CoreML entitlement** → **Part7**。
- **商店真实上架**（MsStoreProvider / SteamProvider 实现 + winapp pack / SteamPipe + 商品页提交）→ **Part8**（本 Part 只预留 trait + Catalog 字段 + feature 渠道变体骨架）。
- **定价 / license 签发服务 / 诚实文案** → **Part8**。

### 1.3 全局约定 + 红线（继承 Part0 §8/§10/§12）

- 🔴 **开源边界**（Part0 §10）：第一方公开源码统一 AGPL-3.0-only，授权实现（`KeyringLicenseStore` + Ed25519 验签）随源码公开——**源码开放不等于功能免费**，付费插件载荷、官方签名构建、托管更新与支持仍是商业边界。**验签公钥可公开；签名私钥与私密签发凭证绝不入源码/公开镜像**。
- 🔴 **验签用 `ring`（非 dalek）**，`verify_strict` RFC 8032 严格；fail-closed；**不新增未缓存 crate**（memory `gotcha-offline-cargo`，`ring::aead`/`ring::hkdf` 已在 ring）。
- 🔴 **162 测试绿不可破**：补丁式扩展（Part0 §5.5 决策 2 精神），不重写既有信任链。
- 让步：派生/exotic 同级共享 `BackgroundHeavyLimiter`；rayon 内永不 `.await`；新增命令返回 `AppError`；中英双语注释；改后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（代码取证，文件:行）

> workflow `wxce3mvvn`（4 agent 测绘 Registry/install/fetch · License/crypto/AES/Entitlement · Coordinator/协议承接 · 开源闭源/多渠道）回传，逐项 file:line。**核心信任链成熟、缺口在外延。**

### 2.1 Registry / Catalog / install / 下载（收尾缺口）

**Registry（成熟）**（[registry.rs](../../src-tauri/src/exotic/registry.rs)）：`RegistryEntry`(plugin_id/version/package_sequence i64 单调/media_kind/formats/capabilities/sku/min_host_version/target/package_url HTTPS/package_size/package_sha256/store_url)；`RegistryIndex`(schema/key_id/sequence u64 全局单调/expires_at/plugins)。`verify_and_parse`：4MB 上限 → `keyset.verify_any(Release, …)` **验签先于解析** → 反序列化 → `validate_entry`；`CryptoError` 折叠 `BadSignature` 防探测。单调防回滚 `RollbackRejected`，`accept()` 验签+防回滚后原子写 index.json/.sig/.seq。🔴 **无 `store_product_id`/`steam_dlc_app_id` 多渠道字段**；🔴 **无 `model_blobs[]`/`kind=model` 大文件分发**（包为 MB 级小 zip）。

**Catalog（成熟）**（[catalog.rs](../../src-tauri/src/exotic/catalog.rs)）：`CatalogOffering`(plugin_id/display_name/media_kind/formats/capabilities/license_tier/platforms/min_host_version/sku/store_url)；格式撞 `classify_media_type` 常见格式 → `CommonFormatConflict` 拒整表；`override_common=true` 一律拒；任一失败整表拒（不部分接受）。🔴 同样无多渠道字段。

**install 编排（成熟）**（[installer.rs](../../src-tauri/src/exotic/installer.rs):69-133）：`install_staged_zip` = 验签+清单白名单+zip 加固+逐文件 hash → `check_catalog_subset`（manifest ⊆ Catalog，commit 前拒）→ manifest_hash → `commit_install`(原子目录 old→backup/staging→current) → `upsert_exotic_plugin`(仅此步持锁) → discard_backup；失败回滚不写 DB。命令层先 `quiesce_exotic`(8s：暂停 Coordinator+kill Worker+释放 Win 句柄)，`exotic_install_lock` 串行化。

🔴 **fetch_exotic_registry 缺口确认**（[exotic_commands.rs](../../src-tauri/src/ipc/exotic_commands.rs)，**P0 阻断**）：全部 16 个 `#[tauri::command]` 中**只有 `list_exotic_registry`（读本地缓存）、无任何命令拉远程 index.json+index.sig → `RegistryCache::accept()`**。全新设备 `exotic_registry_dir()` 无 index → list 返空 → `install_exotic_plugin` `load_verified=None` 直接报「无可用 Registry 缓存」→ **前端无法发起刷新 → 插件装不了**。

**下载（R10 去重机会）**：
- exotic `fetch_package`（[fetch.rs](../../src-tauri/src/exotic/fetch.rs):51-142）：HTTPS 强制 + 拒非 HTTPS 跳转 + 流式 sha256 + size 封顶 + .part rename。🔴 **无 Range 续传 / 无镜像回退 / 无 Channel 进度**。`InstallLimits`(max_file 256MB/max_total 512MB/max_files 4096/max_ratio 200)。
- AI/face `download_assets`（[ai_commands.rs](../../src-tauri/src/ipc/ai_commands.rs):951-1111，**CLIP 与人脸已共用**）：**有 Range 续传 + 双源镜像回退 + Channel 进度** + sha256 + .part rename。
- 🔴 **R10**：sha256 循环 / reqwest Client / .part rename / size+sha256 校验**两套各写一份**（exotic vs AI）→ 应抽通用 `download/mod.rs`，exotic 补 Range+进度+镜像。

### 2.2 信任根 / License / AES / EntitlementProvider

**信任根（成熟）**（[crypto.rs](../../src-tauri/src/exotic/crypto.rs)）：Ed25519 **双信任根 purpose 分离**——`release-2026-01`(签 Registry/package) / `license-2026-01`(签 License token)，`verify` 严格校验 purpose、`WrongPurpose` 折叠 `BadSignature`。`ring` `verify_strict` 语义（RFC 8032，拒非规范 R/小阶点）。公钥 `include_str!("../../resources/exotic-keyset.json")` 编译期嵌入（**当前占位公钥、私钥已弃**，发布前流水线替换）。

**License（成熟）**（[license.rs](../../src-tauri/src/exotic/license.rs)）：token=`base64url(payload).base64url(sig)`，payload(version/key_id/license_id/plugin_id/sku/subject_hash 脱敏/issued_at/not_before/expires_at)；签名覆盖**原始 payload bytes**（防序列化绕过）。OS keyring(`service="picasa-next"`, account=plugin_id)；DB 不存 token；`activate()` 先 `verify_token` 再写 keyring，**sku 取自 Catalog 非 token**。

**授权判定（成熟）**（[mod.rs](../../src-tauri/src/exotic/mod.rs):281-346）：`availability_of()` 门控链（platform→host→dev fixture→installed→install_state→sku→`licenses.evaluate`→Authorized）；`is_task_runnable(plugin_id, capability)` Catalog→claims_capability→resolve_format→Authorized，Coordinator 领任务前调、未授权拦在领取前。

🔴 **AES 完全不存在**（crypto.rs 全文 576 行无 aead/aes/encrypt/decrypt，零命中）→ Part4 §3.7 加密权重**从零新建**（`ring::aead` 已在依赖、未用）。

🔴 **EntitlementProvider 不存在**（全仓零命中）：现有授权抽象仅 `LicenseSource` trait（license.rs:235，`evaluate(plugin_id, sku, now)`），两实现 `UnlicensedSource`(桩)/`KeyringLicenseStore`(生产)；**无多渠道 provider 分发层**（Part0 §9）。

### 2.3 协议 / Coordinator / Supervisor 承接 AI/face（G1-G6 精确落点）

> Part4 §2.3 列了 6 缺口；本节给**框架层精确改造落点**（取证 agent 逐函数定位）。

| 缺口 | 现状 | 精确改造落点 |
|------|------|------------|
| **G5 Coordinator 通用化** | `coordinator.rs:30-33` 硬编码 `PSD_PLUGIN_ID/PSD_WORKER_ID/CAPABILITY`；但 `evaluate_run`(:272) **已参数化** plugin_id/capability | `maybe_run_until_drained`(:147-265) 顶层写死常量 ≥4 处（:175/:236/:159）→ 引入 `PluginDescriptor{plugin_id,worker_id,capabilities}` 注册表 + `run_loop` 逐项循环。**比 Part4 估计轻**（接口已通用，只改顶层调用） |
| **G1 协议帧** | `frame.rs:40-72` FrameType 6 值（Hello..Shutdown） | 加 SessionInit=7/SessionReady=8/GpuTokenRequest=9/Granted=10/Release=11（EmbedBatch 作 Request op 或 =12）；`from_u16`(:61) match 同步；**PROTOCOL_VERSION 升版** + 握手校验更新 |
| **G1 RequestBody/握手** | `message.rs:33-46` RequestBody(Thumbnail/Metadata)；`worker.rs:149-203` 仅 Hello→Ready 两步 | RequestBody 加 `EmbedBatch{item_ids,cache_keys,fingerprint,batch_size}`（cache_keys 非 source_paths，§8.1 决策1）；`item_id()`/`input_fingerprint()` match 扩展；新增第三步 `SessionInit→SessionReady`（`init_session()` 方法）；`SessionInitBody` 在 lib.rs:16 pub 导出 |
| **G2 GpuLimiter** | `limiter.rs:35-39` 仅 CPU permit（FIFO） | 新 `GpuLimiter`（可提 `FairLimiter<T>` 泛型，GPU 额度=1）；`PipelineDeps`(:123) 加 `gpu_limiter: Option`；`worker_loop`(:432) CPU permit 后取 GPU permit；AppState 暴露 `exotic_gpu_limiter` |
| **G4 批量 blob** | `frame.rs:33` MAX_BLOB=64MiB（read:174/write:201 双校验，**无硬编码数字**） | ⚠️ **取证修正 Part4**：batch=128×768d float32≈**384KiB ≪ 64MiB**，**无需调高**（Part4 §3.1.2 担心的 86MB 是图像 tensor、若只传向量则远小）；HelloBody.max_blob_len 协商已到位 |
| **G6 错误码** | `message.rs:86-97` WorkerErrorCode 5 值 | 加 `GpuUnavailable`(retryable，可降 CPU)/`SessionExpired`(retryable)/`ModelLoadFailed`(terminal)/`EmbedDimMismatch`(terminal)；`as_str()`(:101)/`default_retryable()`(:112) 同步；writer_loop strike 分类（GpuUnavailable 计 strike 防风暴、SessionExpired 不计） |
| **长驻 session** | `supervisor.rs:14` 每请求串行短任务；`HANDSHAKE_TIMEOUT=5s`(coordinator.rs:40) | 新 `run_session_task`/`init_session`；超时改 **per-plugin**（WorkerConfig 已有 `handshake_timeout` 字段，仅构造处 coordinator.rs:198 改，AI 调 ≥30s） |
| **Worker trait** | `pipeline.rs:66-77` `ThumbnailWorker` 仅 run_thumbnail | 提通用 `WorkerTask`(is_alive/shutdown/version) + 分立 `EmbedWorker`；`WorkerFactory`(:103) 泛化 |
| **W7 低优先级** | `worker.rs:111-114` mac/Linux 空实现 | mac `setpriority`/QoS BACKGROUND、Linux `nice(10)`；`pre_exec` hook；**先验 libc 在 offline cargo 缓存** |

### 2.4 开源边界 + 多渠道预留

**workspace**（根 `Cargo.toml`）：单一 workspace，members 显式逐一列出（禁 `crates/*` glob），含 src-tauri、exotic-protocol、exotic-workers/*、plugin-api、exotic-trust、ai-core。

**授权实现**（[mod.rs](../../src-tauri/src/exotic/mod.rs)）：直销走主机既有 `KeyringLicenseStore`（`license.rs`），未授权回退 `license.rs::FreeStubEntitlement`（与 keyring 实现同模块）；验签原语在 `crates/scrollery-exotic-trust`（开源，无生产私钥）。

**渠道维度（P16 收敛）**：唯一渠道 = 直销(direct)。`EntitlementProvider` 已建，安装函数无来源参数，`ExoticHost.licenses` 已为 `Arc<dyn EntitlementProvider>`；`store_product_id`/`steam_dlc_app_id` 字段、`channel-msstore`/`channel-steam` feature 与渠道桩均已删除（2026-09-15 消融 P16，见 §3.11）。

**开源边界**：`verify_token`/`KeyringLicenseStore`/`VerifyingKeyset`/内置公钥集**随第一方公开源码统一 AGPL-3.0-only 公开**（Part0 §10）——公开公钥与验签算法不构成授权泄漏；真正的商业边界是**私钥与签发凭证**（不入源码/公开镜像）、付费插件载荷与官方签名构建。dev fixture 旁路已 `#[cfg(any(test, all(debug_assertions, feature="exotic-dev-fixtures")))]` Release 编译期消除（已落地）。

### 2.5 现状问题清单（编号 → §3 设计对应）

| # | 问题 | 证据 | 对应设计 |
|---|------|------|---------|
| R1 | fetch_exotic_registry 缺失 → 全新设备装不了（P0 阻断） | exotic_commands.rs:348 | §3.1 |
| R2 | exotic 下载缺 Range/镜像/进度 + 与 AI 各写一套（R10） | fetch.rs:51 / ai_commands.rs:951 | §3.1 |
| R3 | RegistryEntry 无 model_blobs[]/kind=model 大文件分发（G3） | install.rs:34 | §3.6 |
| L1 | AES 完全不存在（防护层1 从零，Part4 §3.7 消费） | crypto.rs 全文无 aes | §3.7 |
| L2 | EntitlementProvider 不存在（多渠道抽象，Part0 §9） | license.rs:235 LicenseSource only | §3.8 |
| L3 | 授权验签/公钥/keyring 随第一方源码公开（公钥与算法可公开，商业边界=私钥/凭证/载荷） | crypto.rs:23 / license.rs | §3.9 |
| C1 | Coordinator 硬编码 PSD（G5，≥4 调用点，但 evaluate_run 已通用） | coordinator.rs:30/147 | §3.3 |
| C2 | 协议帧/RequestBody/握手/错误码 缺 AI 扩展（G1/G6） | frame.rs:40 / message.rs:33,86 | §3.2 |
| C3 | GpuLimiter 缺失（G2） | limiter.rs:35 | §3.5 |
| C4 | Supervisor 短任务模型 + 握手 5s（长驻 session） | supervisor.rs:14 / coordinator.rs:40 | §3.4 |
| C5 | ThumbnailWorker trait 仅 thumbnail（承接需泛化） | pipeline.rs:66 | §3.4 |
| C6 | mac/Linux worker 低优先级空实现（W7） | worker.rs:111 | §3.4 |
| O1 | 授权实现曾分立 pro/free-stub 双 crate（重复实现，已归一） | crates/ | §3.9 |
| O2 | 多渠道字段/trait/feature 未预留 | catalog.rs:99 / Cargo.toml:127 | §3.11 §3.8 |

---

## §3 设计方案

> 原则：**补外延、不动核心**（成熟信任链/安装/keyring 不重写，补丁式扩展保 162 测试）。优先级：① P0 阻断（fetch_registry）→ ② 承接框架（G1-G6，供 Part4）→ ③ 防护与边界（AES/EntitlementProvider/授权实现归一）→ ④ 多渠道预留。

### 3.1 fetch_exotic_registry + R10 下载统一（R1 + R2，P0）

🔴 **3.1.1 fetch_exotic_registry IPC（P0 阻断，最先）**：新命令拉远程 → 验签 → 写缓存——

```rust
// 全新设备/刷新：拉 index.json + index.sig → RegistryCache::accept()（已含验签+防回滚）→ 写缓存
#[tauri::command]
async fn fetch_exotic_registry(state, force: bool) -> Result<RegistrySummary, AppError> {
    let (index_bytes, sig_bytes) = download_registry_index(registry_url()).await?; // HTTPS
    // 🔴 accept 是实例方法 &mut self、必带 keyset：先 load 取实例，再 accept（registry.rs:242-260）
    let keyset = builtin_keyset()?;                         // VerifyingKeyset（或 &dyn ManifestVerifier，见 §3.8）
    let mut cache = RegistryCache::load(&dir);
    cache.accept(&index_bytes, &sig_bytes, &keyset, now)?;  // 验签+单调防回滚先于解析
    Ok(summarize(cache.load_verified(&keyset)?))            // load_verified 同样需 keyset
}
```

- 前端首启 / 进插件商店 / 「刷新」按钮调用（Part5 §3.5.1 消费）；过期 Registry（`list_exotic_registry` 注释「过期仍返但不允许新装」）由此命令刷新。
- 复用 `RegistryCache::accept`（验签 + 单调防回滚已在 registry.rs）——**仅补「下载 + 调用」薄层**。

> 🔵 **已实施（2026-06-28，cargo test 实证；与上方伪代码的分歧已定稿）**：
> - **下载层**落 `exotic/fetch.rs`：新增 `download_registry_index(base_url) → (index_bytes, sig_bytes)`，拉 `{base}/index.json` + `{base}/index.sig` 原始字节、**不在此验签**（交 `accept`）。顺手抽出共用 `secure_client()`（全程 HTTPS + 超时 + 拒重定向降级），`fetch_package` 改用之去重（R10 通用引擎前的最小收敛，T2 仍后置）。各文件流式封顶 `MAX_REGISTRY_FILE`(4MiB)。
> - **`registry_url()` → 部署常量 `DEFAULT_REGISTRY_BASE_URL` + 环境变量 `PICASA_REGISTRY_BASE` 覆盖**：原伪代码引用的 `registry_url()` 全仓不存在。Registry 是「远程发行真相」，URL 是唯一部署变量；当前用 `.invalid` 占位域名（RFC 2606 保留，响亮标识「待替换真实 CDN」，命名/法务定稿后改），可经 env 覆盖（dev/自托管/测试不重编）。后续如需「按安装实例覆盖」可下沉 app_config。
> - **故意不实现伪代码的 `force: bool` 参数**：`force` 唯一合法用途是绕过「刷新节流」，但当前无节流机制；而它**绝不可**绕过 `accept` 的单调防回滚（安全红线）。一个只能绕过不存在的节流、又绝不该绕过安全检查的参数 = 误导性死参。节流真加上时 force 再随它来。
> - **返回 `RegistrySummary{plugin_count, sequence, expired}`**（camelCase），前端「刷新」后据 `plugin_count>0` 即知「装得了插件」。命令体经 `exotic_install_lock` 与 install/uninstall/rollback 串行（防拉取改写缓存与安装读缓存撕裂）。
> - **测试**：`fetch::registry_index_rejects_non_https`（非 HTTPS 基址发请求前即拒）+ `exotic_commands::default_registry_base_is_https`（守住「基址绝不降级为非 HTTPS」）。happy-path 含真实网络 I/O，不做单测（留 §6 端到端「全新设备 fetch→list→install」联调）。

**3.1.2 R10 通用下载引擎**（R2，去重）：抽 `src-tauri/src/download/mod.rs`——

- 统一：reqwest Client 构建 + 流式 sha256 + size 封顶 + `.part` 原子 rename + **HTTP Range 续传** + **镜像源回退** + **Channel 进度**。
- AI/face `download_assets`（已共用，ai_commands.rs:951）+ exotic `fetch_package` 收敛到此引擎；exotic 侧**补 Range/进度/镜像**（大模型 blob §3.6 必需续传）。
- ⚠️ exotic 的 HTTPS 强制 + 拒非 HTTPS 跳转策略保留进通用引擎（安全不降级）。

### 3.2 协议扩展 G1 + G6（供 Part4 ai/face worker）

> `exotic-protocol` 是 host 与 worker 双端共享纯库（§2.4）。**扩展 `exotic-protocol`、不建新 crate**（Part4 §8 已定，关闭开放决策）。

**3.2.1 帧类型 + 握手**（G1）：
- `FrameType`(frame.rs:40) 加 `SessionInit=7/SessionReady=8/GpuTokenRequest=9/GpuTokenGranted=10/GpuTokenRelease=11`；`EmbedBatch` 作 `RequestBody` op 变体（复用 Request=3 帧）。`from_u16`(:61) match 同步。**`PROTOCOL_VERSION` 升 2** + **psd-worker 同步重编译**（monorepo）。⚠️ **帧层 frame.rs:156 硬等值校验不支持运行期「版本协商」**（旧 v1 帧在读到 Hello 前即被拒）→ worker 与协议**同步升版、无混版**(§8.2 C1,纠正初稿「host 取交集」误述)。〔🔴 本行帧类型清单已被 D2/D3(2026-07-02)推翻并经 T10 落地(2026-07-03):`GpuTokenRequest/Granted/Release` 三帧**不进协议**(令牌全留主进程,D2);`SessionInit/SessionReady` **不作新 FrameType**——SessionInit 走 RequestBody op、SessionReady=其 Success 响应(D3)。落地后 FrameType 仍六值;「PROTOCOL_VERSION 升 2 + psd-worker 同波重编译」已执行。〕
  - 🔴 **P0-3 已装 worker 版本错配防护（第 6 轮独立核验补）**：「monorepo 同步」仅保证**随 app 捆绑**的 psd-worker 与 host 同版；**用户 `AppData` 已装的远程下载插件 worker** 在 host 升级后可能仍是旧协议版本（离线升主程序 / 插件更新失败 / 回滚 / Registry 不可达）。因帧层硬 `==` 拒版本、**握手前即断**，host 在**定位/拉起 worker 前**（`resolve_worker_path` → Coordinator spawn 前）须比对已装 manifest 的 `worker_protocol_version` 与当前 `PROTOCOL_VERSION`：不符则**拒绝拉起该插件、标 `needs_reinstall`、触发重下载/安装匹配版本**。🔴 **第 7 轮终审校正（原述「加字段」失实）**：`PackageManifest.protocol_version` 字段**已存在**（[package.rs:89](../../src-tauri/src/exotic/package.rs#L89)）、**安装期已校验**（[install.rs:143](../../src-tauri/src/exotic/install.rs#L143) `manifest.protocol_version != host_proto`）、**握手期亦校验**（[worker.rs:181](../../src-tauri/src/exotic/worker.rs#L181) 不符返回 `Err("协议版本不兼容…")`——是**干净报错、非崩溃**）。**真缺口 = spawn 前无前置复核**：旧协议 worker 仅在握手（已付 spawn 代价）才被拒、且报通用错而非 `needs_reinstall` 语义。修法 = 在 `verify_installed_integrity` / `resolve_worker_path` 增「已装 `manifest.protocol_version` vs `PROTOCOL_VERSION`」**前置比对**（**非新增字段**），不符即标 `needs_reinstall` 早退。捆绑 worker 不受影响。〔✅ P0-3 已落地(2026-07-03,随 T13):`verify_installed_integrity` 增协议版本前置比对(稳定码 `protocol_mismatch` = needs_reinstall 语义,修复命令与前端可按码分支)+ `resolve_worker_path` 失败留稳定码 warn 日志;有「装时合法、host 升版后 spawn 前被拒」单测。DB 状态标记/自动重下载随 Part8 商店 UX。〕
- `RequestBody`(message.rs:33) 加 `EmbedBatch{item_ids, cache_keys: Vec<String>, fingerprint, batch_size}`（🔴 字段为 `cache_keys` 非 source_paths，对齐 §8.1 决策1——worker 按 `ai_cache_dir/{cache_key[..2]}/{cache_key}.webp` 拼路径读解码）；`item_id()`/`input_fingerprint()` match 扩展。`SessionInitBody{session_id, model_handle: ModelHandle, image_provider, model_profile: ModelProfileSnapshot{arch_id, image_file, text_file, batch_size}}` + `SessionReadyBody{embed_dim, caps}` 顶部定义 + `lib.rs:16` pub 导出。🔴 **字段以 §8.1 决策2 + §8.3 为权威**（`model_handle`=OS 具名共享内存/handle/fd，**非** `model_paths`/明文 bytes，详见 Part4 §3.7.3；`image_provider`=文本 EP 由 worker 内硬编码 CPU；`model_profile` 统一原 `provider`/`batch_capacity`）。〔本行原为 `{model_paths, provider, batch_capacity}` 旧稿，第 5 轮复审回写正文。〕

**3.2.1a 🔴 P0-3 协议逐项化 + 多句柄 + 缓存根定稿（第 8 轮核验，覆盖 3.2.1 单值草稿）**：

上方 `SessionInitBody{model_handle}`（单句柄）与 `EmbedBatch{...fingerprint}`（单指纹、响应 `embeddings[]` 无逐项核对）无法承载目标模型，按下定稿：

① **多模型句柄**（CLIP 图/文双编码器、face YuNet 检测 + SFace 识别 → 单 handle 不够）：
```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole { ImageEncoder, TextEncoder, FaceDetect, FaceRecog }

pub struct SessionInitBody {
    pub session_id: u64,
    pub model_handles: Vec<(ModelRole, ModelHandle)>,  // role→handle，worker 按角色取
    pub model_profile: ModelProfileSnapshot,           // {arch_id, image_file, text_file, batch_size}
    pub ai_cache_dir: String,                          // ② 受限缓存根（见下）
}
```
worker 按 `ModelRole` 取对应 `ModelHandle`（§3.7.3 具名 shm）mmap 各自解密权重；文本编码器小且 CPU、若未加密可走 path，但句柄机制统一以 role 寻址。

② **缓存根（补「worker 只收 cache_keys、不知 ai_cache_dir」缺口）**：`EmbedBatch` 仅传 `cache_keys`，worker 须拼 `{ai_cache_dir}/{key[..2]}/{key}.webp` → `ai_cache_dir` 经 `SessionInit` 传**受限根路径**，worker 只读该根下、拒越界。ai_cache 非敏感（图像非密、AES 只护权重，Part4 §3.1.3）→ 传路径而非 OS 句柄即可，与模型权重的具名 shm 区别对待。

③ **逐项对齐结构 + 长度校验 + 陈旧防护**（补 `fingerprint` 单值、响应无逐项 fingerprint/error/长度校验缺口）：
```rust
RequestBody::EmbedBatch {
    items: Vec<EmbedItem>,   // 每项独立 fingerprint
}
// EmbedItem { item_id: i64, cache_key: String, fingerprint: String }

pub struct EmbedBatchSuccess { pub results: Vec<EmbedResult> }  // 与 items 严格同序同长
pub enum EmbedResult {
    Ok  { item_id: i64, fingerprint: String, embedding: Vec<f32> },   // 回带 item_id+fingerprint 供核对
    Err { item_id: i64, fingerprint: String, code: WorkerErrorCode }, // 逐项失败不连坐整批
}
```
- **强制长度一致**：host 收到断言 `results.len() == items.len()`，否则整批判 `InternalError` 重试。
- **逐项核对（陈旧/错位防护）**：每项回带 `item_id + fingerprint`，host 比对请求项指纹，不符即丢弃该项——延续现有单项 `input_fingerprint` 核对语义（message.rs:57-67）到批量。
- **embed_dim 校验**：`embedding.len() == SessionReadyBody.embed_dim`，否则该项 `EmbedDimMismatch`（§3.2.2 G6 已含）。
- `RequestBody::item_id()`/`input_fingerprint()` 的 match 改为遍历 `items`（不再单值；批量上下文返回 `Vec` 或按 index 取）。

> 联动:Part4 §3.1.2 表 G1/G4 行的 `SessionInit`/`EmbedBatch` 字段以本 3.2.1a 为权威;Part4 §3.7.3 具名 shm 仅承载 `ModelRole` 为 `*Encoder`/`Face*` 的加密权重句柄。

> 🔴 **T10 落地修正(2026-07-03,实现=`crates/exotic-protocol/src/message.rs`)**:
> ① `EmbedResult::Ok{embedding: Vec<f32>}` 不成立——128 项 × 768d 的 JSON 文本 ≈2.5MB 超
> `MAX_JSON_LEN`(1MiB,超限即协议异常杀 worker;本节 §3.2.3 算的 384KiB 是二进制口径),且违反
> 「JSON 只放控制字段、二进制走同帧 blob」天条 → 落地为 `Ok{item_id, fingerprint}`,嵌入按 Ok 项序
> 连续排布于 Success 帧 blob(f32 LE),host 校验 `blob.len()==ok_count×embed_dim×4`;
> ② `model_handles: Vec<(ModelRole, ModelHandle)>` 元组落地为具名 `Vec<ModelDescriptor{role, handle,
> len, sha256}>`(叠加 D1 逐模型完整性字段;`ModelHandle::{Path,Named}` 两级通道见 D1);
> ③ `session_id` 不进批量 op(Supervisor 严格串行下冗余;SessionExpired 由 worker 无会话状态判定);
> ④ 载运按 D3:SessionInit/SessionClose/EmbedBatch/FaceDetectEmbed 全走 RequestBody op,零新帧,
> SessionReady=SuccessBody.session;⑤ 快照补 `face_profile_id?`、SessionReady 补 `face_embed_dim?`
> (合并 session 需 CLIP+face 双 profile 声明)。

> ✅ **T17 增补(2026-07-03,additive 不升版)**:v2 op 家族补 `EncodeText{texts}` + `SuccessBody.text_embed`(`TextEmbedSuccess{count}`)——T10 三源合并漏列文本编码,T16 删主进程 ort+tokenizers 后语义搜索断链,T15 发现缺口、T17 收口。向量走 Success 帧 blob(texts 序连续,每项 embed_dim×f32 LE);**全批原子**(无逐项 Ok/Err:文本编码不读盘、tokenizer 接受任意串,无逐项失败模式);不新增 capability;host 校验 = count 与请求数一致 + blob 精确长度(`validate_encode_text_output`);psd-worker 兜底臂同波扩。

> ✅ **face 接线波增补(2026-07-03,additive 不升版)**:`FaceItemResult::Ok` 补 `width`/`height`(worker **实际解码尺寸**)——FaceDet 几何是解码图像素坐标,而解码发生在 worker 端(源可为缩略图档位),host 归一化落库与 quality 派生必须用实际尺寸,预测尺寸有舍入误差。`serde(default)` 容旧帧,host 校验将 0 尺寸视作协议违例(两端同仓同步分发);同波把 EmbedBatch 的批上限防御对称补进 `handle_face`(原先仅 embed 有)。

> 🔵 **原型已验证（2026-06-28，cargo test 实证）**：scratchpad 临时 crate 落 `SessionInitBody`/`ModelHandle`/`EmbedItem`/`EmbedResult` + host 侧 `reconcile`，6 单测通过——serde wire round-trip 等价 + 多句柄按 `ModelRole` 寻址 + 逐项核对逻辑正确（长度不符整批判错、陈旧 fingerprint 丢弃、embed_dim 不符丢弃、逐项 `Err` 不连坐）。逐项化协议可序列化、可校验。落 `message.rs` 时按此结构 + `reconcile` 移入 host。

**3.2.2 错误码**（G6）：`WorkerErrorCode`(message.rs:86) 加 `GpuUnavailable`(retryable，可降 CPU)/`SessionExpired`(retryable)/`ModelLoadFailed`(terminal)/`EmbedDimMismatch`(terminal)；`as_str()`(:101)+`default_retryable()`(:112) 同步；**writer_loop strike 分类**（GpuUnavailable 计 strike 防故障风暴、SessionExpired 不计）。

**3.2.3 ⚠️ G4 取证修正（回溯 Part4，§8.1 决策1 定稿）**：Part4 §3.1.2 初稿担心「单批 86MB > 64MiB MAX_BLOB」——那是**方案 A（主进程解码发 NCHW tensor）**的问题。**决策1 裁定方案 B**：`EmbedBatch` 传 `cache_keys`（worker 拼 `ai_cache_dir/{2hex}/{hex}.webp` 自解码），回程传 embeddings（128×768d≈384KiB）→ **blob≈0，MAX_BLOB 无需调高**。**方案 A 作废**（不再有「主进程统一解码 / 限批 ≤128 / 调高 MAX_BLOB」）。联动 review 已同步修 Part4 §3.1.2/§3.6/§8。

### 3.3 Coordinator 通用化 G5（C1）

> evaluate_run 已参数化、**改造比 Part4 估计轻**（修正 Part4 §2.3 G5「大幅重构」判断）。

- 引入 **`PluginDescriptor{plugin_id, worker_id, capabilities, handshake_timeout, uses_gpu}`** 注册表（Vec，从 Catalog + 已装插件构建）。
- `maybe_run_until_drained`(coordinator.rs:147-265) 改**接受 descriptor 参数**；`run_loop` 对注册表逐项调用（去 PSD 硬编码 ≥4 处：:175/:236/:159/:147）。
- `evaluate_run`(:272) 🔴 **第 8 轮核验 P1-4 已修（commit 待提交）**：原断言「不改/已通用」失实——:312 `has_ready_exotic_task` 曾硬编码 `CAPABILITY_STR("thumbnail")`，AI/face 多能力插件会查错任务类型；已改用传入的 `capability.as_str()`（现行调用方均传 `Capability::Thumbnail`，PSD 路径行为不变、162 测试不破）。其余参数化部分确已通用；`resolve_worker_path`(:159) 改按 descriptor.plugin_id。
- ⚠️ **psd-worker 仍是注册表一员**(thumbnail capability),与 ai/face 同框架;不破现有 162 测试(psd 路径行为不变)。〔✅ C1/T13 已落地(2026-07-03):`PluginDescriptor{plugin_id, worker_id, capabilities, handshake_timeout, uses_gpu}` + `plugin_descriptors(catalog_snapshot)` 注册表(capabilities 取自 Catalog;worker_id/uses_gpu 为运行时支持信息,Catalog/manifest 尚无此数据,Part8 扩展后转全数据驱动);`maybe_run_until_drained` 拆双层(注册表外环 + 单(插件,能力) drained 内环),PSD 硬编码全部出调度路径;非 thumbnail 能力显式跳过留 T15 接缝;coordinator 全部测试绿〕

### 3.4 长驻 session + Supervisor + worker 泛化 + 低优先级（C4 + C5 + C6）

**3.4.1 长驻 session 生命周期**（C4）：
- `Supervisor` 加 `init_session()`（Hello→Ready 后追加 `SessionInit→SessionReady`）+ `run_session_task`（长驻多轮嵌入，替 run_thumbnail 串行模型）。
- `HANDSHAKE_TIMEOUT`(coordinator.rs:40，现 5s) 改 **per-plugin**（WorkerConfig 已有 `handshake_timeout` 字段，仅构造处 :198 改）；**session init 超时独立 120–300s**（ORT 加载，Part4 §8）；进程握手仍快（5s）。
- worker 空闲超时卸载 session 释放 VRAM（对齐 Part4 AiEnginePool 自然完成 drop）。

〔✅ C4/T15 已落地(2026-07-03):Supervisor 增 `init_session`/`close_session` + `session: Option<SessionDescriptor>` 快照;「run_session_task」以 `run_request`(op 无关)+ trait `run_batch` 形态落地;SessionInit 超时用 T13 的 `op_timeouts::SESSION_INIT`(300s),进程握手仍 5s(per-plugin 已随 T13 PluginDescriptor.handshake_timeout);空闲卸载=worker 侧自杀 timer(最后一帧后 300s `exit(0)`)已落 ai-worker,host 侧「空闲卸载」已随 T17 收敛落地(2026-07-03):**运行结束(自然完成/取消)即 close_session**——AI 管线的运行结束本就是唯一空闲边界(进程内路径同样在结束时置空引擎),不设独立 idle timer;worker 空闲自杀 300s 兜底不变〕

**3.4.2 Worker trait 泛化**（C5）：`ThumbnailWorker`(pipeline.rs:66) → 提通用 `WorkerTask`(is_alive/shutdown/version) + 分立 `EmbedWorker`(run_embed_batch/init_session)；`WorkerFactory`(:103) 泛化（psd 用 ThumbnailWorker、ai/face 用 EmbedWorker，共享 Claimer/Limiter 基建）。〔✅ C5/T15 已落地(2026-07-03):`WorkerTask`(公共面)+ `ThumbnailWorker`/`EmbedWorker`(init_session/close_session/run_batch)分立,WorkerSupervisor 三者全实现,psd 路径行为不变;**分叉裁决:WorkerFactory 未加 embed 方法**——AI 派发器(T17)落地时按实际驱动形态定工厂,避免无消费者抽象;批结果校验=worker.rs `validate_embed_batch_output`/`validate_face_batch_output`(同序同长/逐项 item+fingerprint 核对/blob 精确长度=维度×数量)〕

**3.4.3 mac/Linux 低优先级**（C6，W7）：`apply_low_priority`(worker.rs:111 空实现) 补——mac `setpriority(PRIO_PROCESS,0,10)` 或 QoS BACKGROUND；Linux `nice(10)`；`CommandExt::pre_exec` hook（两平台可用）。⚠️ **先验 `libc` 在 offline cargo 缓存**（memory `gotcha-offline-cargo`）；AI worker CPU 占用远高于 psd、未降优先级影响显著。

### 3.5 GpuLimiter 跨进程 G2（C3，落 Part4 §3.6/§8）

- **`FairLimiter<T>` 泛型提取**（limiter.rs）：`BackgroundHeavyLimiter`(CPU) + `GpuLimiter`(GPU) 复用同一 FIFO 公平实现；GpuLimiter 额度默认 **1**（单 GPU 序列化，Part0 §5.5 决策 4：同时只一个 AI worker 持 GPU）。
- **进程内注入**：`PipelineDeps`(pipeline.rs:123) 加 `gpu_limiter: Option<Arc<GpuLimiter>>`；`worker_loop`(:432) 取 CPU permit **后**再取 GPU permit（uses_gpu 插件才取，psd 传 None）；AppState 暴露 `exotic_gpu_limiter`。
- 🔑 **跨进程令牌**（worker 是独立进程，Part4 §8）：worker 推理前发 `GpuTokenRequest` 帧 → Coordinator 持 `tokio::Semaphore` acquire → 回 `GpuTokenGranted` → worker run → 完成/崩溃 Supervisor Drop 发 `GpuTokenRelease`。🔴 **互斥不双锁**（§8.3 裁决，非"双层叠加"）：**分离进程** → 仅跨进程 GpuToken 帧（进程内 `PipelineDeps.gpu_limiter=None`）；**合并单 worker 进程** → 仅进程内 `GpuLimiter`（无跨进程令牌帧）。两者互斥，否则每批多余 acquire、GPU 串行。具体分支待 Part4 §3.11 拍板定 T6。〔本段原写"双层"旧稿，第 5 轮复审回写正文。〕

### 3.6 model blob 大文件分发 G3（R3，供 Part4 ViT-L ~1GB）

- **`RegistryEntry` 加 `model_blobs: Vec<ModelBlob>`**（`{url, sha256, size, kind: "model_weight", encrypted: bool}`，`#[serde(default)]` 向后兼容）；`PackageManifest` 加 `kind=model` 文件类。
- **大文件不进 zip**：模型独立分步下载（走 §3.1.2 通用引擎 **Range 续传**，~1GB 必需），`InstallLimits` 对 `kind=model` **豁免压缩比检查**（ONNX 近 1:1）+ 不计入 `max_total_size=512MB`（单独大文件预算）。
- **安装编排扩展**：install 后若 entry 有 model_blobs → 分步拉取 → sha256 校验 → 落 models_dir（加密则 §3.7 解密）；下载中断可续传，不阻塞 worker 二进制安装。

### 3.7 AES 加密权重原语 + license 派生密钥（L1，落 Part0 §8 / Part4 §3.7+§8）

> 防护层1。**从零新建**（crypto.rs 无 AES）。`ring::aead`/`ring::hkdf` 已在依赖、未用（无新 crate）。商业边界见 §3.9。

**3.7.1 AES-256-GCM 原语**（pro crate `aes_decrypt.rs`）：
- 加密：分发的 ONNX 是密文（`nonce(12B) || ciphertext || tag(16B)`，nonce 每次随机+前缀存）。
- 解密：**主进程**（pro crate）`ring::aead::LessSafeKey::open_in_place` → 明文 bytes。

**3.7.2 license 派生密钥**（核心，硬编码无意义）：
- `key = HKDF-SHA256(ikm=enc_seed, salt=per-model 随机 salt(32B，存 ModelBlob.enc_salt), info=plugin_id||model_id)`（`ring::hkdf`）。
  - 🔴 **CANON-02 回写（salt 位置定稿，第 6 轮独立核验）**：salt 存 `ModelBlob.enc_salt`（base64、随 Registry 签名保护、明文可见无害），**非密文前缀**；密文仅 `nonce(12B)||ciphertext||tag(16B)`。§3.6 `model_blobs` 加 `enc_salt: Option<String>` 字段（§8.5 已定）。打包侧（Part8 §3.2）与解密侧（Part4 §3.7.2）**按此唯一约定**，杜绝"从密文前缀取 salt → 实取到 nonce → HKDF 派错 key → AES-GCM open 失败 → 付费模型整体解不开"。〔删除初稿"随密文前缀存 salt"旧值，对齐 §8.5 / Part4 §3.7.2。〕
- 🔴 **`enc_seed` 字段当前不存在（2026-09-15 消融 P10）**：早稿设计为 license.rs payload 加 `enc_seed: Option<String>`，该预留已删（连同 `ActivationInfo`）；**AES 未实现期间不保留生产契约**，实装时按真实消费方重新定字段与传递方式。「无 license → 无法派生密钥 → 密文不可解」的防护意图不变，当前无对应代码。
- 🔴 **enc_seed 粒度 = 插件主种子（per-plugin-id，非 per-license）**：同一 `plugin_id` 的所有 license 签入**同一** `enc_seed`，确保 CDN 每插件一份密文可被所有持证用户解密（per-license 唯一会致密文无法复用、存储爆炸）；per-license 唯一性由 `license_id`/`subject_hash` 保证。轮换（疑似泄漏）= 重加密 CDN 密文 + 签发新 token（Part8 §3.2.2 签发/打包侧据此）。
- 🔴 **DerivedSecret 门控**（§8.2 C2，Part0 §8.4 红线）：`activate()` 成功后主进程 HKDF 派生 `DerivedSecret`（密钥句柄）写 `ExoticHost` 内存映射 `plugin_id→DerivedSecret`；`is_task_runnable` 对 AES 插件**额外查 DerivedSecret 存在**，与 `LicenseStatus::Authorized` **双门**——patch bool gate 拿不到密钥。Part4 消费侧信任此门控、不重复实现。
  - 🔴 **保持 `is_task_runnable -> bool` 返回类型不变**（现 4 处调用方 + 测试假设 bool：mod.rs:333 / pipeline.rs:141 闭包 / coordinator.rs:305 / exotic_commands.rs:178）：DerivedSecret 缺失时**返回 `false`**，拒领原因（NoKey/NoLicense）仅作**内部 `BlockReason` 日志**区分、**不**改成 `No(NoKey)` 枚举（避免 API 破坏波及 4 处调用点）。〔修正初稿「返回 No(NoKey)」的隐含 API 破坏，第 5 轮复审。〕

**3.7.3 喂 Session**（决策2，§8.1）：主进程解密 → **共享内存映射** → `SessionInit` 传 **handle**（**非 bytes**：ViT-L ~1.7GB 远超 MAX_BLOB 64MiB 不可走帧）→ worker `mmap` → `Session::builder().commit_from_memory()`（**ort rc.12 已支持、无需 API 改**，impl_commit.rs:93，纠正初稿「须 ORT API 改」误述）。**明文不落盘、不经帧**；worker 用完 munmap、主进程 zeroize。
- 🔴 **跨进程 handle 传递机制**（第 5 轮复审补，详见 Part4 §3.7.3）：worker 是 `spawn` 的独立 exec 进程，**匿名映射 handle/fd 在子进程无效**。**统一用具名共享内存**（Win `CreateFileMapping`+name / mac·Linux `shm_open`+name；Linux 亦可 memfd+`/proc/self/fd`），`SessionInit.model_handles` 内每个 `ModelHandle{Named(String)|Fd(i32)|Win32Handle(u64)}`（🔴 多句柄 `role→handle` 以 §3.2.1a 为权威，此处单数为历史草稿）**传 name 字符串**最简、免 `DuplicateHandle`/`SCM_RIGHTS`。worker 同 name `OpenFileMapping`/`shm_open` 后 mmap。安全硬化见 Part4 §3.7.3a（P1-7）。
- 🔴 **具名 shm 安全硬化（P1-7，第 8 轮核验，详见 Part4 §3.7.3a）**：shm 承载解密明文权重 → 须 高熵不可猜名(CSPRNG) + Win DACL / POSIX `0600`+`O_EXCL` + open 后即 `shm_unlink` + 崩溃 Supervisor Drop 兜底清理 + 启动扫残留，防同机其它进程/用户窥取（AES 否则被本地绕过）。
- ⚠️ **诚实边界**（Part0 §7）：公开权重加密只挡懒人，付费用户内存可 dump；为私有/微调模型铺路，文案不夸大。

### 3.8 EntitlementProvider 抽象（L2，Part0 §9，供 Part5 gate）

🔴 **`LicenseSource` 升格 `EntitlementProvider`**（trait 在开源叶 crate `plugin-api`；direct 实现为 `KeyringLicenseStore`）：

```rust
pub trait EntitlementProvider: Send + Sync {
    fn evaluate(&self, plugin_id: &str, sku: &str, now: i64) -> LicenseStatus;
    fn source_tag(&self) -> &'static str;          // "direct" | "ms_store" | "steam"
    // 🔴 权威签名（§8.3 联动 review 裁决，对齐 Part0 §9.1；覆盖初稿两参数版）：
    //    activate 加 sku/now；返回值为当前真正需要的结果——无 payload。
    fn activate(&self, plugin_id: &str, sku: &str, credential: &str, now: i64) -> Result<(), LicenseError>;
    fn deactivate(&self, plugin_id: &str) -> Result<(), LicenseError>;
}
// 2026-09-15 消融 P10：`ActivationInfo`/`enc_seed`（恒为 None 的解密预留）连同专用脱敏测试已删；
// 未来 AES 若实装，按真实消费方重新定返回契约，当前不预留。
```

- **独立 `ManifestVerifier` trait**〔🔴 已放弃（§8.7）：包校验直接用 `VerifyingKeyset`，不再引入 trait 转发层〕。

- **实现**：`KeyringLicenseStore`（Ed25519 keyring，唯一真实现，`license.rs:43`）；未授权回退 `FreeStubEntitlement`（fail-closed，恒 `Unlicensed`，`license.rs:137`）。
- `ExoticHost.licenses: Arc<dyn LicenseSource>` → `Arc<dyn EntitlementProvider>`。
- **交付源（P09/P16 已退役）**：原 `PluginDeliverySource` trait 与三种只返回枚举的 Delivery 类型已删（无价值转发，非零引用，P09）；`InstallSource` 枚举本身及其 `SteamDepot`/`StoreBundled` 取值亦已删除（P16），`install_staged_zip` 现**无来源参数**。渠道整体退役归 Part8。
- 🔴 **前端 gate 判定 IPC**（供 Part5 §3.5.2）：`get_plugin_entitlement(plugin_id) → { availability, source_tag, sku, store_url }`——前端据此 gate/购买引导；**前端不持验签逻辑**（判定全在后端 EntitlementProvider）。

### 3.9 授权实现归一 + 开源边界（L3 + O1）

> 🔴 Part0 §10：第一方公开源码统一 AGPL-3.0-only，授权实现随源码公开。**源码开放不等于功能免费**——付费插件载荷、官方签名构建、托管更新与支持仍是商业边界；**私钥与私密签发凭证**永不入源码/公开镜像。

**3.9.1 workspace 拓扑**：根 `Cargo.toml` 单一 workspace，members 显式逐一列出，**禁用 `crates/*` glob**（glob 会命中无 `Cargo.toml` 的 `crates/exotic-workers/` 目录致 `cargo metadata` 报错，且只展开一层、漏掉两层深的 worker）。

**3.9.2 授权实现归一**：
- **direct 渠道**：主机既有 `KeyringLicenseStore`（`license.rs`）作唯一直销实现，验签原语经 `crates/scrollery-exotic-trust` 复用。
- **未授权回退**：`exotic/license.rs::FreeStubEntitlement`（fail-closed，恒 `Unlicensed`，不 panic）。
- **已删除**：`crates/scrollery-pro` 与 `crates/scrollery-free-stub` 两个重复实现 crate——同一目标保留一套主流程。
- **保留**：`crates/scrollery-plugin-api`（跨 crate 契约叶 crate）、`crates/scrollery-exotic-trust`（Ed25519 验签原语）、付费校验、渠道 feature 与正式签发工具。

**3.9.3 公钥配置**：`PICASA_EXOTIC_KEYSET_FILE` 编译期注入通道保留。**本次归一保留既有 key ID、公钥、用途和有效期；构建注入仍为整组替换，轮换时保留需继续信任的历史键**（注入是把文件整体烧进二进制，不做键级合并）。**验签公钥可公开；签名私钥与私密签发凭证不入源码/公开镜像**（签发工具源码本身可以公开）。

**3.9.4 公开镜像门禁**：已提交树快照只做内部文件过滤，**不剥离源码或改 `Cargo.lock`**；拟公开树和提交元数据在公开 push 前接受 gitleaks 扫描，公开暂存分支继续运行前端、Rust workspace 和独立 RAW 的原锁验证。保留私有 canonical → 公开 bot 快照拓扑，具体规则与操作入口见 [同步现行说明](Part6_3c_Copybara同步配置草稿.md)。

### 3.10 ai-worker / face-worker 接入插件平台（L 承接 Part4）

> Part4 实现 worker 内推理；本 Part 提供**接入框架**——ai/face 作插件与 psd 同框架。

- **Registry/Catalog 注册**：ai-clip/ai-face 作 `CatalogOffering`（capability=`Embedding`/`FaceDetectEmbed`，新 capability §3.2）+ `RegistryEntry`（含 `model_blobs[]` §3.6 携带 ONNX）。
- **走统一编排**：install（验签+原子切换）+ 长驻 session（§3.4）+ GpuLimiter（§3.5）+ AES 模型（§3.7）+ EntitlementProvider 授权（§3.8）。
- **Coordinator 注册表**（§3.3）纳入 ai/face descriptor（`uses_gpu=true`、握手超时 ≥30s）。
- **psd-worker 不变**（thumbnail capability，注册表一员，162 测试守护）。
- ⚠️ **本 Part 交付框架可承接；worker 内推理代码 = Part4**（前后端/host-worker 成对，Part4 §4 P2 全段依赖本 Part 先行）。

### 3.11 渠道维度：单一直销（P16 收敛）

- **现状**：唯一渠道 = 直销(direct)。`store_product_id`/`steam_dlc_app_id` 字段、`channel-msstore`/`channel-steam` feature、渠道桩（`MsStoreEntitlementStub`/`SteamEntitlementStub`）、`lib.rs` 渠道互斥门与 Steam 启动占位、CI 三渠道依赖树断言均已删除（2026-09-15 消融 P16）。
- **交付源**：`PluginDeliverySource` trait（消融 P09）与 `InstallSource` 枚举（P16）均已删除；`install_staged_zip` 现**无来源参数**，恒走「验签 Registry → HTTPS 下载 → zip 内 manifest/hash 逐文件复核」，`InstallError::ChannelUnsupported` 同步删除。
- **保留不变**：`verify_token`/`KeyringLicenseStore`/`VerifyingKeyset`、签名 Registry 对**原始 bytes** 先验签、下载 size/sha256/HTTPS 保护、keyring 授权存取、`FreeStubEntitlement` fail-closed 回退——Ed25519 验签与 keyring 授权从不做渠道门控（第 7 轮终审结论不变）。
- 重新引入第三方渠道（Store/Steam）时按 feature + cfg 物理排除 + Provider 路线重做；本 Part 不再保留其骨架与字段。

---

## §4 分步任务清单（优先级 + 依赖）

> 优先级：**P0 阻断（fetch_registry）→ P1 承接框架（G1-G6，供 Part4 worker 化）+ 授权实现归一（EntitlementProvider/AES）→ P2 接入 + 多渠道预留**。补外延、不动核心、保 162 测试。

### P0 · 阻断 + 下载收尾

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T1** ✅🔴 | `fetch_exotic_registry` IPC（拉远程→`RegistryCache::accept` 验签→缓存）**【已实施，见 §3.1.1 🔵】** | exotic_commands.rs、registry.rs、fetch.rs | — | §3.1.1 / 全新设备可装 |
| **T2** | R10 通用下载引擎 `download/mod.rs`（Range+镜像+进度，AI/face/exotic 共用） | 新模块、fetch.rs、ai_commands.rs | — | §3.1.2 |

### P1 · 承接 Part4 框架（G1-G6）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| ✅ **T3**(2026-07-03,与 Part4-T10 为同一施工) | 协议扩展 G1/G6(RequestBody 四新 op+升版+错误码4;**FrameType 零新增**,D2/D3 裁决)〔落地形状见 §3.2.1a 的 T10 修正注;psd-worker 同波重编译,全套仅本地验证三绿〕 | exotic-protocol frame.rs/message.rs/lib.rs | — | §3.2 |
| ✅ **T4**(2026-07-03,与 Part4-T13 为同一施工 `647891b`) | Coordinator 通用化 G5(PluginDescriptor 注册表 + 双层调度循环,PSD 硬编码全出调度路径;测试全绿仅本地) | coordinator.rs | — | §3.3 / 162 测试绿 |
| ✅ **T5**(2026-07-03,与 Part4-T15 为同一施工 `6d1ab57`+`403b10f`) | 长驻 session(Supervisor session 快照+init_session/close_session)+ Worker trait 泛化(WorkerConn.run_request+pipeline trait 拆 WorkerTask/ThumbnailWorker/EmbedWorker)。⚠ per-plugin 握手超时被 D3 修正推翻:握手恒 5s,300s 预算归 SessionInit 请求档 | supervisor.rs、pipeline.rs、worker.rs | ← T3 | §3.4.1-2 |
| ✅ **T6**(2026-07-03,与 Part4-T11 为同一施工) | GpuLimiter G2。⚠ 落地形状被 D2 修正:**令牌全留主进程、协议零扩展**(GpuToken/GpuPermit 薄封装复用 BackgroundHeavyLimiter 公平队列,AppState.gpu_token;非 FairLimiter 泛型、无跨进程令牌帧) | limiter.rs、pipeline.rs、coordinator.rs | ← T3 | §3.5 |
| ✅ **T7**(2026-07-03,与 Part4-T12 为同一施工) | model blob 分发 G3(RegistryEntry.model_blobs[]+fetch_model_blob 断点续传+安装 1.5 步先 blob 后 zip)。落地注:「InstallLimits 豁免」的正确解释=blob 不进 zip,zip-bomb 检查天然不适用,InstallLimits 零改动 | registry.rs、install.rs、installer.rs | ← T2 | §3.6 |
| **T8** | mac/Linux worker 低优先级（setpriority/nice/QoS，pre_exec，先验 libc 缓存） | worker.rs:111 | — | §3.4.3 |

### P1 · 授权实现归一 + AES

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T9** ✅ | EntitlementProvider 抽象（LicenseSource 升格 trait + direct/渠道桩 + get_plugin_entitlement IPC） | license.rs、mod.rs、exotic_commands.rs | — | §3.8 |
| **T10** ✅ | 授权实现归一（`KeyringLicenseStore` 唯一 direct 实现 + `channel_stubs` fail-closed 回退 + 删 pro/free-stub 双 crate）+ workspace members 显式化 | 根 Cargo.toml、crates/、CI | ← T9 | §3.9 / 同一目标一套主流程 |
| **T11** 🔴 | AES-256-GCM 原语（ring::aead）+ license 派生密钥（HKDF enc_seed，per-plugin 主种子）+ **DerivedSecret 双门控**（activate 写 `plugin_id→DerivedSecret`、`is_task_runnable` 额外查）+ commit_from_memory | AES 原语模块、license.rs payload、mod.rs/coordinator.rs 门控 | ← T10 | §3.7 / 验收：无 DerivedSecret 即拒领（patch bool 拿不到密钥）；🔴**端到端对拍 gate**（第7轮终审）：固定 enc_seed+enc_salt 加密真实小 ONNX → 客户端从 token 取 seed → HKDF → 解密 → `commit_from_memory` → **实跑推理、输出与明文模型一致（误差<1e-4）**，证「付费用户解得开」，不可纸面定稿。🔴 **优先级后置（Part0 §11.4.3）**：AES 实装延后至 v0.1 变现后、随首个加密权重插件（AI/人脸）交付；v0.1 不保留 `enc_seed` 字段，首发 exotic-formats 无 ONNX 权重不需 AES |

> 🔴 **2026-09-15 消融 P09/P10 更新**：T11 所述 `enc_seed` 字段预留已删（`ActivationInfo` 一并删除，`activate` 返回 `Result<(), LicenseError>`），AES 实装时不沿用旧返回契约；T13 落地注中的 `PluginDeliverySource`/Delivery 类型亦已随 P09 删除，来源枚举也已随 P16 删除，安装函数无来源参数。

### P2 · 接入 + 多渠道预留

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T12** | ai/face worker 接入插件平台（Catalog/Registry 注册 + descriptor + 走统一编排） | catalog.rs、registry.rs、coordinator.rs | ← T3-T11 + **Part4 worker** | §3.10 |
| 已退役 **T13** | 渠道 feature/字段/InstallSource/空桩与专属检查已删除；当前只保留直销安装与真实授权 | 现行实现见 §3.11/§8.4 | — | P16/P23 |

---

## §5 风险与回滚

| 风险 | 触发 | 缓解 / 回滚 |
|------|------|------------|
| 🔴 **Coordinator 通用化破 162 测试** | PluginDescriptor 重构误改 psd 路径行为 | psd-worker 作注册表一员、thumbnail 路径**行为不变**；现有测试为护栏；T4 单独提交单独验 |
| 源码公开后防白嫖 | 源码含完整验签与门控逻辑 | 商业边界是**私钥/签发凭证**（不入源码/公开镜像）与**付费载荷**（enc_seed 在 token 内，无授权拿不到密钥）——公开验签算法不构成授权泄漏 |
| 协议升版兼容 | PROTOCOL_VERSION 升 2，旧 psd-worker 握手失败 | 🔴 **版本协商不可行**（frame.rs:156 硬等值校验，旧 worker v1 帧在帧层即被拒、读不到 Hello，§8.2 C1）→ **psd-worker 与 exotic-protocol 同步升版重编译**（monorepo，无运行期混版） |
| 授权实现归一行为漂移 | 合并重复实现引入差异 | 归一前后行为对拍（同 token 同结果）；既有授权测试作护栏 |
| model blob 大文件下载中断 | ViT-L ~1GB 网络中断全量重下 | T2 Range 续传**必需**（非可选）；断点恢复测试；sha256 最终校验 |
| AES enc_seed 依赖 license 签发 | token 无 enc_seed 字段则解密失败 | license 签发服务（Part8）须同步加 enc_seed；过渡期未加密模型仍明文（`encrypted=false`） |
| workspace 重构影响构建 | 加根 Cargo.toml 改变构建路径 | 渐进：先加 workspace 不动现有，逐 crate 纳入；CI 双平台 check |
| GpuLimiter 跨进程令牌延迟 | 每批推理叠加 IPC 往返 | 合并单 ai-worker 退化为进程内（Part4 §3.11）；批粒度大摊薄延迟 |
| ⚠️ **G4 取证修正需回溯 Part4** | Part4 §3.1.2 担心 86MB blob | §8.1 决策1 定方案（传 cache_keys、worker 读 ai_cache 自解码，非图像 tensor）→ 联动 review 同步修 Part4 §3.1.2/§8 |

---

## §6 验收标准

**P0 阻断**：

1. **fetch_registry**：全新设备（无本地缓存）→ `fetch_exotic_registry` → 验签 + 防回滚 + 写缓存 → `list_exotic_registry` 返回插件 → `install_exotic_plugin` 成功（**P0 闭环：装得了插件**）。
2. **R10**：AI/face/exotic 共用 `download/mod.rs`；exotic 下载有 Range 续传 + 镜像回退 + Channel 进度（前端可见进度条）。

**P1 承接框架**：

3. **协议**：psd-worker 与 exotic-protocol **同步升版重编译**（**非**版本协商，§8.2 C1 帧层硬等值校验）；ai/face worker `SessionInit→SessionReady` 握手成功；EmbedBatch 往返。
4. **Coordinator**：psd+ai+face 多插件注册表调度；**162 测试绿**（psd 路径零回归）。
5. **长驻 session**：ai-worker 加载模型常驻 + 多轮嵌入；握手超时 per-plugin（AI ≥30s）；空闲卸载释放 VRAM。
6. **GpuLimiter**：多 worker GPU 序列化（额度 1，不并发持 GPU）；跨进程令牌生效。
7. **model blob**：ViT-L ~1GB 分步下载 + Range 续传 + 中断恢复；豁免压缩比检查。

**P1 防护边界（红线）**：

8. **EntitlementProvider**：trait 升格；direct 走 `KeyringLicenseStore`、msstore/steam 走 `channel_stubs` fail-closed 桩；`get_plugin_entitlement` IPC 供 Part5 gate。
9. 🔴 **授权实现归一**：唯一 direct 实现 + fail-closed 回退；`crates/scrollery-pro` 与 `crates/scrollery-free-stub` 已删除；验签公钥可公开，私钥/签发凭证不入源码与公开镜像。
10. **AES**：无 license（无 enc_seed）→ 密文模型不可解；有 license → HKDF 派生密钥解密 → `commit_from_memory` 加载（明文不落盘）。

**P2 接入 + 多渠道**：

11. **ai/face 接入**：作插件走统一 install/session/GpuLimiter/AES/授权；与 psd 同框架（配 Part4 worker 实现）。
12. **多渠道**：`store_product_id`/`steam_dlc_app_id` 字段（旧 Registry `#[serde(default)]` 兼容不报错）；channel feature 骨架编译。

---

## §7 执行提示词（新会话直接用）

```
任务：实施 Picasa Next 重构 Part6（插件平台与 exotic 收尾）。

【先读】
1. docs/refactor_2026/Part0_总纲与产品定稿.md §5.5(架构决策)/§8(防护层1)/§9(多渠道分发)/§10(开源策略与合规)
2. docs/refactor_2026/Part4_AI与人脸插件化.md §2.3(G1-G6 缺口)/§3.1-3.7/§8 —— 本 Part 是 Part4 worker 化的框架供应方
3. docs/refactor_2026/Part5_前端体验重构.md §3.5(插件商店/gate 消费本 Part IPC)
4. docs/refactor_2026/Part6_插件平台与exotic收尾.md 全文（§2 现状取证 + G1-G6 精确改造落点表，§3 设计，§4 任务表）

【铁律】
- 补外延、不动核心：exotic 信任链(双信任根/防回滚/原子安装/quiesce/keyring)成熟,补丁式扩展,**保 162 测试绿**。
- 🔴 开源边界:第一方公开源码统一 AGPL-3.0-only(含验签与门控实现);商业边界=私钥/签发凭证(不入源码/公开镜像)+付费载荷(enc_seed 在 token 内,无授权拿不到密钥)。不建闭源 pro crate、不设 free-stub 独立 crate。
- 验签用 ring(非 dalek)verify_strict;AES 用 ring::aead/ring::hkdf(已在依赖,无新 crate);不新增未缓存 crate(先验 libc 缓存)。
- 协议升版=psd-worker 与 exotic-protocol 同步升版重编译(非版本协商,§8.2 C1 帧层硬等值);Coordinator 通用化 psd 路径行为不变。
- 新增命令 AppError;中英双语注释;改后中文 commit;仅用户通知时 push;大文件小步 Edit。

【顺序】
P0 阻断: T1 fetch_exotic_registry(全新设备可装) → T2 R10 通用下载引擎
P1 承接框架: T3 协议扩展G1G6 → T4 Coordinator通用化G5(PluginDescriptor) → T5 长驻session+worker泛化
   → T6 GpuLimiter G2 → T7 model blob G3 → T8 mac/Linux 低优先级
P1 授权归一: T9 EntitlementProvider抽象 → T10 授权实现归一(KeyringLicenseStore唯一+channel_stubs回退+删双crate) → T11 AES+license派生密钥
P2 接入: T12 ai/face worker接入(配 Part4 worker)；T13 渠道预留已退役

【验收】按 §6 十二条;P0 T1(装得了插件)+T4(162测试绿)+T10(授权实现归一)是核心门槛。
【关键认知】evaluate_run 已参数化,G5 通用化比 Part4 估计轻(只改顶层调用);G4 取证修正:EmbedBatch 传 cache_keys(轻量,worker 读 ai_cache 自解码)则 MAX_BLOB 无需调高,联动 review 已修正 Part4;G2 跨进程令牌走 GpuToken 帧。
```

---

## §8 联动 review 修正记录（webto43dl，2026-06-27）

> 4 reviewer（Part6↔Part4 接缝 / Part6↔Part5↔Part0 一致 / 源码核验 / 可行性完整性）。本节裁决全部 issue，**无省略**。下列修正**覆盖正文对应处**（正文关键误述同步改）。

### 8.1 核心架构决策（canonical，两文档统一）

**决策1 · G4「谁解码」= 方案B（EmbedBatch 传 cache_keys、worker 自解码）**

reviewer 指出 Part4 §3.1.3（主进程解码发 NCHW tensor）与 Part6 §3.2.1（传 source_paths）冲突。**裁决方案B**（修正 p6p4-seam 推荐的方案A），依据：
- 🔑 **顺应框架现状**：现有 exotic `RequestBody::Thumbnail` 本就传 source path 让 **worker 读文件解码**（psd-worker 读 PSD）。EmbedBatch 传 `cache_keys` 让 worker 拼 `ai_cache_dir/{2hex}/{hex}.webp` 读解码与此**一致**；方案A 传 tensor blob 反而是偏离框架的新模式。
- 🔑 **避免 86MB blob 难题**：传 tensor（128×3×336²×4≈**173MB**，非 86MB）远超 MAX_BLOB 64MiB → 须限批 ≤8（ViT-L）伤 GPU 利用率，或调高 MAX_BLOB 削弱内存炸弹防护。传 cache_keys 则 blob≈0、批可大。
- 🔑 **不破坏 AES 隔离**：AES 保护**模型权重**（敏感）；ai_cache 是非敏感缩略图，worker 读不涉密。「数据面分离」精确化为「**模型权重严格隔离（AES，决策2）+ 图像走 worker 读受限 ai_cache 目录（非敏感）**」。
- **安全边界**：worker 只读 `ai_cache_dir/{cache_key}`（传 cache_key、worker 拼路径），不读任意文件系统。
- **EmbedBatch 字段定稿**：`{item_ids, cache_keys: Vec<String>, fingerprint, batch_size}`；worker 含 WebP 解码（image crate，轻、已在依赖）。
- 🔁 **回溯 Part4**：§3.1.2 删「86MB 限批」、§3.1.3 改「worker 读 ai_cache 自解码（与 thumbnail 框架一致）」、§8.1 G4 行更新（见 §8.6）。

**决策2 · AES 模型 bytes 传输 = 匿名内存映射 + mmap（明文不落盘、不经 64MiB 帧）**

模型敏感（不可走 worker 读明文盘）+ ViT-L ~1.7GB 超 MAX_BLOB 不可走单帧 blob。**裁决**：
- 主进程 AES 解密 → 写**匿名内存映射**（Linux/mac `memfd`/`shm_open`，Win `CreateFileMapping`）→ `SessionInit` 传 **handle/fd** → worker `mmap` 明文 → `commit_from_memory`。明文不落盘、不经帧。worker 用完 munmap，主进程 zeroize。
- ✅ **`commit_from_memory` 已可用**（reviewer 实测 ort-2.0.0-rc.12 `SessionBuilder::commit_from_memory(&[u8])`，impl_commit.rs:93）→ **删 §3.7.3「须 ORT API 改」误述**。
- `SessionInitBody` 携 `model_handle`(OS handle/fd),非 model_paths/model_bytes。〔🔴 机制已被 [D1](2026-07-02-Part4-D1-模型载荷传输与AES解密.md)(2026-07-02)修正:匿名映射的 handle/fd 数值跨 exec 进程无效(Part4 §3.7.3 已证)→ 落地为两级 `ModelHandle::{Path(明文,首期), Named(AES 具名 shm,④)}`;「明文不落盘、不经帧、commit_from_memory」的裁决内核不变。〕

### 8.2 Critical 修正

| # | 问题 | 修正（覆盖正文） |
|---|------|------|
| C1 协议升版「版本协商」不可行 | frame.rs:156 硬等值校验，旧 worker v1 帧在帧层即被拒、读不到 Hello 无法协商 | **改：psd-worker 与 exotic-protocol 同步升版**（monorepo 成本低，无运行期混版）→ §3.2.1「版本协商」改「PROTOCOL_VERSION 升 2 + psd-worker 同步重编译」 |
| C2 DerivedSecret 门控缺失 | Part0 §8.4 要求门控查「DerivedSecret 是否存在」（密钥即授权证明，patch bool 拿不到密钥），Part6 未设计 | 补 §3.7：`DerivedSecret` 类型（HKDF 密钥句柄）；`activate()` 成功写 `ExoticHost` 内存映射 `plugin_id→DerivedSecret`；`is_task_runnable` 对 AES 插件**额外查 DerivedSecret**（无则 `No(NoKey)` 拒领），与 LicenseStatus 双门 |
| C3 ManifestVerifier trait | 初稿计划引入 `ManifestVerifier` trait 包裹包校验 | 🔴 **已放弃**（§8.7）：包校验直接用 `VerifyingKeyset`，不再加 trait 转发层 |
| C4 层2 subject_hash 预留缺失 | Part0 §8.2 层2 设备绑定需 LicensePayload.subject_hash，§3.7 只加 enc_seed | 补 §3.7 `LicensePayload` 加 `subject_hash: Option<String>`(`#[serde(default)]`，Part8 层2 填，直销离线无)；`activate()` 留 `device_fingerprint` 扩展点 |
| C5 workspace 重构影响 | path 依赖/Tauri build/CI 评估不足 | §3.9.1：workspace 纳入 src-tauri+exotic-protocol+exotic-workers，`cargo build/test --offline` 全绿（resolver="2"）；members 显式列全 + 明确 tauri-build workspace 路径 |

### 8.3 接缝对齐（major）

- **EntitlementProvider 对齐 Part0 §9.1（权威）**：`activate(plugin_id, sku, credential, now) -> Result<(), LicenseError>`（验证与持久化凭据，不返回 payload）。§3.8 注「以 Part0 §9.1 为准」。
- **SessionInitBody 双 EP**：`provider`→`image_provider`（文本 EP 固定 CPU 由 worker 内硬编码，对齐 Part4 §3.3）；`batch_capacity` 与 Part4 `profile` 统一为 `model_profile: ModelProfileSnapshot{arch_id, image_file, text_file, batch_size}`。
- **GpuLimiter vram 预留**：`GpuTokenRequest` JSON 加 `vram_hint_mb: Option<u32>`(`#[serde(default)]`，P2 用)，对齐 Part4 §3.6。
- **GpuLimiter 双层澄清**：分离进程→仅跨进程 GpuToken 帧（进程内 limiter=None）；合并单进程→仅进程内。**互斥不双锁**，待 Part4 §3.11 拍板定 T6。
- **AES 解密阶段统一**：发生在「model_blobs 下载后 → sha256 校验（**对密文**）→ 内存解密 → commit_from_memory」，**非** install zip 解包阶段；Part0 §8.4「installer.rs 解包后解密」指向 model blob 流程（§8.6）。
- **Part5 进度依赖**：Part6 §4 T11/T12 加「← T2 R10（下载进度 Channel）」；Part5 §3.5.1 风险加「T2 未完成则进度条不工作」。
- **availability 枚举明示**：§3.8 返回 `NotInstalled|Installed|Authorized|UpdateAvailable|PlatformUnsupported`，供 Part5 商店徽章。

### 8.4 当前直销交付契约

- **数据结构**：catalog/registry 不含商店商品字段，当前安装记录不含渠道来源列。
- **交付源包装（2026-09-15 已退役）**：三个 Delivery 类型和 trait 均删除，安装函数不再接来源参数。
- **installer.rs InstallSource 枚举（该预留已退役，P16）**：`InstallSource` 及其 `SteamDepot`/`StoreBundled` 取值、`InstallError::ChannelUnsupported` 均已删除，`install_staged_zip` 不再接来源参数；`RegistryExpect` 与 zip 内 manifest/hash 逐文件复核保留不变。
- **Steam 占位 + DB 列（均已退役）**：`#[cfg(feature=channel-steam)]` 的 `SteamAPI_RestartAppIfNecessary` 占位已随 P16 删除；`exotic_plugins.entitlement_source` 列（V12，DEFAULT 'direct'）已随 P23 连同 DDL/SQL/模型删除。
- **渠道 cfg 门控（已作废，P16）**：`channel-msstore` build 与 `StoreBundledDelivery` 路由、以及 `fetch_package`/`license.rs` 上的 `#[cfg(feature = "channel-direct")]` 均已删除；日后引入平台渠道须重新设计物理排除与合规断言。🔴 **第 7 轮终审结论不变：`verify_token`/`VerifyingKeyset`/`verify_strict` 不做门控**——Ed25519 签名验证是公开算法、无密钥材料、非「下载-执行代码」机制，留开源全量编入（对齐 §8.7）。
- **条件编译与依赖（已收敛）**：公开与私有树使用**同一份 manifest 与同一份依赖图**——不再有 `pro` feature、不再有 optional path dep、不再有 INTERNAL 标记块剥离。授权装配在组合根 `default_entitlement_provider` 单点完成：`KeyringLicenseStore`（keyring 直销），信任根解析失败降级 `FreeStubEntitlement` fail-closed。

### 8.5 测试任务 + 验收 + 细节补充

- **§4 补测试**（Part0 §11 每 Part 补测）：T3→exotic-protocol 单测（SessionInit/EmbedBatch 序列化往返、v2 FrameType 编解码）；T4→Coordinator 集成测（多 descriptor、psd 零回归）；T6→GpuLimiter 单测（FIFO/permit/Semaphore）；T9→EntitlementProvider 测（未授权 fail-closed 恒 Unlicensed、渠道桩对拍）；T11→AES+HKDF 向量测（已知明文/密钥/salt/nonce→密文、无 enc_seed 报错）。
- **T4 验收锚修正**（ai/face worker 在 Part6 阶段不存在）：§6 标准 4 改「Coordinator 参数化、psd 行为零回归（162 绿）；ai/face descriptor **占位注册、worker_path pending 不启进程**」（多插件实调度留 T12）。
- **AES nonce/salt 格式**（§3.7 补）：密文 `nonce(12B)||ciphertext||tag(16B)`；`salt(32B)` 存 `ModelBlob.enc_salt`(base64、随 Registry 签名保护、明文可见无害)；`key=HKDF-SHA256(ikm=enc_seed, salt, info=plugin_id||model_id)`；nonce 每次分发 OsRng 刷新。§3.6 `model_blobs` 加 `enc_salt: Option<String>`。
- **capability 常量**：exotic-protocol lib.rs 加 `CAP_THUMBNAIL/CAP_EMBEDDING/CAP_FACE_DETECT_EMBED`（防大小写不一致静默失败）；T3 补。
- **registry_url 来源**：§3.1.1 明确 `env!("PICASA_REGISTRY_URL")` 编译期注入（或 `include_str!` endpoint 文件，commercial 替换），**编译期常量非运行期可配**（防恶意 registry 替换）。
- **错误码命名统一**：以 Part6 §3.2.2 为准（`GpuUnavailable/SessionExpired/ModelLoadFailed/EmbedDimMismatch`）；Part4 §3.1.2 G6 的 `GpuOom/ModelNotLoaded/OrtError` 对齐之（OrtError 并入 ModelLoadFailed terminal 子类）。

### 8.6 回溯其它 Part

- 🔁 **Part4**：§3.1.2 删 86MB/限批（决策1）；§3.1.3 改「worker 读 ai_cache 自解码」；§8.1 G4 行更新；§3.7.2 nonce/salt 并入正文（不只 §8.4.4）；§2.3 G5「大幅重构」加勘误「改造量适中（顶层去硬编码，evaluate_run 不改）」；§3.1.2 G6 错误码名对齐 Part6 §3.2.2。
- 🔁 **Part0**（小勘误，不动定稿正文、此处标注）：§0 表 Part6 行「Part4 前端 gate」→「**Part5** 前端 gate IPC」（gate UI 在 Part5、Part6 供判定 IPC）；§8.4「installer.rs 解包后解密」指向 model blob 下载流程（决策2）。
- 🔁 **worker.rs:111 去重**：Part4 §4 T19 与 Part6 §4 T8 同一任务 → **Part6 T8 执行、Part4 验收**（Part4 T19 标「← Part6 T8 合并、不重复」）。

### 8.7 源码行号修正 + 核验结论

- §2.3 G5 maybe_run_until_drained PSD 硬编码精确 **6 处**（coordinator.rs:164/175/200/201/222/237，非原「≥4 处 :175/:236/:159」）。
- §2.1 download_assets 范围 **ai_commands.rs:958-1111**（:951 是注释起点）。
- §2.4 `BUILTIN_KEYSET_JSON include_str!`(crypto.rs:23) + `VerifyingKeyset::builtin()`(crypto.rs:161) 分列。
- **开源边界澄清**（§3.9）：验签原语（`VerifyingKeyset` + `verify_strict`）与内置公钥集、授权实现、AES 解密**均为公开源码**（第一方公开源码统一 AGPL-3.0-only，Part0 §10）；真正的秘密是**签名私钥与私密签发凭证**，以及付费插件载荷——「公钥保密」不再是规范。
- ✅ **源码核验 9/9 核心论断属实**（evaluate_run 通用化 / 协议帧计数 / GpuLimiter 落点 / HANDSHAKE_TIMEOUT / enc_seed 可行 / MAX_BLOB 双校验 / 开源边界量化 / 下载去重 / mac 空实现+libc 缓存），仅 3 处行号小偏差（已修上）。
- ✅ **联动 review 总评**：接缝框架意图层基本对齐（reviewer 一致认可 G1-G6 供需映射、AES/GpuLimiter 主方案、EntitlementProvider 闭合）；本 §8 解决全部 critical（2 架构决策 + C1-C5）+ major（接缝/多渠道/feature/测试）+ minor。🔴 **§8 修正已并入 §3 正文（terminal review）；正文为唯一权威、动工以正文为准，本 §8 仅审查历史留痕**（对齐 Part0 §0.3，不再「以 §8 为准」）。

---

> **Part 6 正文完**。上游消费方：Part4（worker 化 P2 全段依赖本 Part 框架先行）、Part5（插件商店/gate 消费 IPC + EntitlementProvider）。下游：Part7（发布工程：渠道 feature 构建变体 + 平台签名）、Part8（商业化：MsStore/Steam Provider 实现 + 上架 + license 签发服务 + enc_seed）。
> 回溯需求（联动 review 处理）：**Part4 §3.1.2/§8 G4 取证修正**（EmbedBatch 传 cache_keys、worker 读 ai_cache 自解码 → MAX_BLOB 无需调高，不传 tensor）。
> 执行前必读：Part0 §13 + 本文 §7。
