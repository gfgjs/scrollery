---
id: 2026-07-24-plugin-store-map
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# Exotic 插件商店体系摸底映射

> 为「降噪/超分子系统作为付费插件」的设计输入，仅读摸底，禁改仓内文件。

## 1. Catalog 结构与类型定义

### JSON Schema（编译期嵌入）
- **文件**：`src-tauri/resources/exotic-catalog.json`
- **层级**：`{ schema: u32, sequence: u32, offerings: [offering, ...] }`
- **条目结构**（RawOffering → CatalogOffering）：
  ```json
  {
    "plugin_id": "exotic-XXX",
    "name": "显示名称",
    "media_kind": "image|video|audio|document",
    "formats": ["ext1", "ext2"],
    "capabilities": ["thumbnail|metadata|text|embedding|face_detect_embed"],
    "license_tier": "paid|free",
    "sku": "xxx-engine-2026",         // 付费必需；验签用
    "platforms": ["x86_64-pc-windows-msvc", "aarch64-apple-darwin"],
    "min_host_version": "0.1.0",
    "override_common": false,
    "store_url": "https://example.invalid/plugins/xxx",
    "distribution": "package|builtin"  // builtin=无安装包，直接验license
  }
  ```

### 枚举定义（path:line）
- **MediaKind**：`src-tauri/src/exotic/catalog.rs:29-34`
  - Image, Video, Audio, Document（小写序列化）
- **Capability**：`src-tauri/src/exotic/catalog.rs:50-61`
  - Thumbnail, Metadata, Text, Embedding, FaceDetectEmbed（rename_all="lowercase" 除 FaceDetectEmbed）
- **CatalogOffering**（运行时投影）：`src-tauri/src/exotic/catalog.rs:144-163`
  - 聚合单插件的全部信息 + `builtin: bool` 门控标志

### OCR 条目（已列项）
```json
{
  "plugin_id": "exotic-ocr",
  "name": "OCR 文字提取",
  "media_kind": "image",
  "formats": ["ocr"],
  "capabilities": ["text"],
  "license_tier": "paid",
  "sku": "ocr-engine-2026",
  "platforms": ["x86_64-pc-windows-msvc"],
  "distribution": "builtin"
}
```
**关键**：`formats: ["ocr"]` 非真实扩展名，`is_valid_format()` 校验通过但永不与扫描相遇。

---

## 2. 分发与安全路径

### 下载引擎
- **文件**：`src-tauri/src/ipc/model_download.rs`
- **API**：`pub async fn download_assets(client, models_dir, assets, mirror_first, on_progress, download_id)`
- **特性**：
  - HTTP Range 断点续传（resumable per-file）
  - 镜像回退（mirror_first flag）
  - size + sha256 验证
  - `.part` → 原子 rename（同卷保证）
  - 进度事件流式推给前端（Channel<DownloadProgress>）
  - 文件名安全白名单校验（禁越界 `../..` 攻击）
- **UA 处理**：见 OCR 线落地 commit 11d766e（缺 UA 修复）

### 编译期 Keyset 与签名校验
- **公钥集来源**：
  - Direct 渠道：`src-tauri/src/exotic/license.rs` 的 `KeyringLicenseStore`（公开树占位实现）
  - Pro 私有树：组合根 swap 至闭源 `DirectEntitlement`（真实签名集）
- **dev/test 旁路**：`PICASA_EXOTIC_DEV_KEYSET` 环境变量（debug 构建 + `exotic-dev-fixtures` feature）
- **校验流程**（path:line）：`src-tauri/src/exotic/license.rs:82-95`
  - 1. `verify_token(token, keyset, plugin_id, sku, now)` — 时间窗+SKU检查
  - 2. 通过后写 keyring（`keyring::Entry::new(KEYRING_SERVICE, plugin_id)`）
  - 3. fail-closed：任何验签失败 → 拒绝授权，不覆盖已有有效 token

### 公开仓 Raw 源拓扑
- **内置 Catalog**：`include_str!("../../resources/exotic-catalog.json")`（编译期嵌入，随应用签名发布）
- **Registry 源**（Part8）：后端从私有 registry 源解析并验签（公开树为占位实现）
- **模型资产源**：HTTPS 直链（主源） + mirror_url（国内镜像回退）

### 安装目录布局
- **模型目录**：`{models_dir}/{arch_id}/` 或 `{models_dir}/{profile_id}/`（canonicalize + 同卷校验）
- **临时文件**：`{path}.part` → 同卷 rename（原子写入；若异卷 fallback 需自实现）
- **缓存目录**：`{ai_cache_dir}/{key[..2]}/{key}.webp`（worker 只读）

### 完整性校验（hash）
- **格式**：sha256（小写 hex）；0 或 None = 暂不校验
- **校验点**：`download_assets()` 完成后验证，不符拒绝解压/使用
- **破坏恢复**：用户手动补全或重新下载（不自动垂直降级）

### 写盘原子性
- `.part` 后缀临时文件 → `rename()` 同卷原子替换
- 若 target 目录与 temp 不同卷，需改用 `std::fs::rename` + fallback copy+delete（当前 ok）

---

## 3. Coordinator 状态机与 Worker 生命周期

### 调度循环（path:line）
- **文件**：`src-tauri/src/exotic/coordinator.rs`
- **入口**：事件通道 + dirty flag（满时置标志不丢 wake）
- **运行条件**（evaluate_run）：enabled + 未暂停 + 授权+平台+能力齐 + 有待处理任务 + Worker 可用
- **唤醒原因**：`WakeReason` enum — Startup, ScanCommitted, PluginInstalled, LicenseActivated, RetryDue, UserRequested

### Worker 生命周期
- **启动**：`spawn` → Hello/Ready 握手（5s 超时，D3 §2 模型加载不在握手）
- **会话加载**：SessionInit（OCR 180s，D-OCR-2，three models CPU EP 不发 Progress 心跳）
- **工作**：批处理（EmbedBatch / FaceDetectEmbed / OcrBatch）
- **卸载**：SessionClose（30s，健康 worker 毫秒级；timeout 则 kill 回收）
- **空闲自杀**：300s（防 VRAM 僵尸）

### 超时表（path:line）
- **文件**：`src-tauri/src/exotic/coordinator.rs:44-84` — `op_timeouts` 模块
- **关键值**：
  - THUMBNAIL = 30s
  - SESSION_INIT = 90s（静默心跳模式）
  - OCR_SESSION_INIT = 180s flat（保守，无实测）
  - OCR_BATCH = 60s + 30s × item_count
  - FACE_DETECT_EMBED = 60s + 6s × item_count（线性放宽）

### 失败恢复（path:line）
- **文件**：`src-tauri/src/ai/worker_client.rs:11-16`
- **MAX_ATTEMPTS = 2**：进程异常 → 重建 worker + 重建会话 + 重发一次
- **终止错误**（EmbedDimMismatch 等）→ 不重试，直返错误
- **重试计数**：`exotic_tasks.retry_count`（DB 跟踪）

### 插件注册表（动态驱动）
- **函数**：`fn plugin_descriptors(snap: &CatalogSnapshot) -> Vec<PluginDescriptor>`（path:line:104-129）
- **PluginDescriptor** 结构：
  ```rust
  pub struct PluginDescriptor {
    pub plugin_id: String,
    pub worker_id: String,           // 握手期望值
    pub capabilities: Vec<Capability>,
    pub handshake_timeout: Duration,
    pub uses_gpu: bool,              // PSD=false, AI/Face=true(待)
  }
  ```
- **现有**：PSD（psd-worker）、RAW（raw-worker）
- **扩展路径**：注册表新增一项 PluginDescriptor → 调度代码零改（解耦）

---

## 4. OCR 插件先例（最关键）

### Catalog 条目形态
- **已列入**：`exotic-catalog.json` 第 18-31 行
- **字段特征**：`"formats": ["ocr"]`（虚拟），`"distribution": "builtin"`（D-OCR-5），`"sku": "ocr-engine-2026"`

### 模型资产挂载
- **契约文件**：`crates/scrollery-ai-core/src/ocr_profile.rs`（纯数据，零 ort）
- **结构**：OcrProfile（id, display_name, det_file, cls_file, rec_file, dict_file, +几何+阈值+swap_rb）
- **档位**：
  - `pp-ocrv5-mobile`：det/cls/rec 合计 ~30MB（交互默认）
  - `pp-ocrv5-server`：~190MB（高精度）
- **文件名单源**：`docs/planning/2026-07-23-OCR文字提取/model-assets.md`（改名仅动此处单点）
- **cls 不共用**（主线修正）：两档各有专属 PP-LCNet_x0_25/x1_0 textline_ori 模型；仅 dict_file 共用

### 付费/Builtin 过滤（三面）
1. **merged_formats**：catalog formats 列 merge 后按扩展名快速判
2. **media_kind**：Image/Video/Audio/Document 过滤
3. **fast_scan walker**：扫描播种时三面判断（ocr format 虚拟，永不匹配真实文件）

### cache_key 处理
- **来源**：`cache_key: Option<String>` in `OcrItem`（批协议）
- **worker 约束**：只读 `{ai_cache_dir}/{key[..2]}/{key}.webp`（越界拒绝）
- **一期**：OCR 不持久化结果（不进 DB 缓存），cache_key 保留但未使用（D-420 后置二期）

### 前端商店卡片与安装流
- **主视图**：`src/views/PluginStoreView.vue`（Part5 T11）
- **数据层**：`src/composables/useExoticStore.ts` — mergeStorePlugins() 全外连接 registry × installed
- **StorePluginRow** 结构：pluginId, availableVersion, installedVersion, installState, formats, sku, registryExpired, upgradable
- **内置特殊处理**：`builtinOfferings` ref（T11）— OCR 等 builtin 插件单独一区展示
- **激活流程**：ExoticActivateDialog（复用 PSD 先例） + 前端 gate（未授权点击 → toast + 跳插件商店）
- **门控 UX**（D-OCR-6）：按钮常显 + 未授权引导 + 模型未下载引导

### OCR 为接入插件商店新增/改动的文件清单（施工模板）

| 域 | 新增/改动文件 | 优先级 | 备注 |
|---|---|---|---|
| **协议** | `crates/exotic-protocol/src/message.rs` | P0 | OcrSessionInit/Close/Batch、OcrItem/Line/Result、capability::OCR_TEXT |
| **模型契约** | `crates/scrollery-ai-core/src/ocr_profile.rs` | P0 | OcrProfile struct + 两档注册表（纯数据） |
| **OCR 推理** | `crates/scrollery-ai-core/src/ocr/mod.rs` + det/cls/rec/dict/geometry.rs | P0 | OcrEngine、det→cls→rec→坐标系、CTC 解码 |
| **Worker** | `crates/exotic-workers/ai-worker/src/batch.rs` | P0 | handle_ocr_batch() + 三模型 CPU EP |
| **Catalog** | `src-tauri/resources/exotic-catalog.json` | P0 | 新增 OCR offering（已列） |
| **Catalog 解析** | `src-tauri/src/exotic/catalog.rs` | P0 | RawOffering + distribution 字段 + builtin 标志 |
| **License 门控** | `src-tauri/src/exotic/mod.rs:availability_of()` | P0 | builtin 分支（D-OCR-5，跳过安装态直接验 license） |
| **协调** | `src-tauri/src/exotic/coordinator.rs` | P1 | OCR descriptor 注册或 AI descriptor 扩展、op_timeouts::OCR_SESSION_INIT/OCR_BATCH |
| **IPC 命令** | `src-tauri/src/ipc/ocr_commands.rs` | P1 | ocr_status、ocr_extract_image、ocr_extract_frame、download_ocr_models（已施工） |
| **Worker 客户端** | `src-tauri/src/ai/worker_client.rs` | P1 | OcrSessionSpec、build_ocr_session_spec()、send_ocr_batch() |
| **模型注册表** | `src-tauri/src/ai/ocr_registry.rs` | P1 | ocr_assets()、ocr_tier_installed()（对标 ai_commands） |
| **下载命令** | `src-tauri/src/ipc/ai_commands.rs` 扩展 | P1 | 可复用 download_assets()，OCR 单独调用路径 |
| **前端类型** | `src/types/exotic.ts` | P2 | ExoticRegistryEntry 扩展字段（format="ocr" 自动识别） |
| **前端商店** | `src/views/PluginStoreView.vue` | P2 | 自动支持（StorePluginRow 复用）；OCR 卡片样式 TBD |
| **设置** | `src/views/SettingsView.vue` | P2 | OCR 模型下载分节、参数调节 UI（模型选择/cls_input_hw） |
| **DB** | `schema.sql` | P2 | exotic_tasks.capability 新值 `"ocr_text"`（非本务） |

### 决策锚（D-418..D-421，OCR 线已确认）
- **D-418**：引擎 = PP-OCRv5 ONNX 复用 ai-worker
- **D-419**：载体 = ai-worker 自研管线进 scrollery-ai-core；产品形态 = 插件商店付费插件
- **D-420**：模型分发 = 首用下载 + 设置页预下载；多档位可选（mobile/server）
- **D-421**：视频取帧 = 前端 canvas 复用截帧链路

---

## 5. 前端 UI 与设置链

### 插件商店组件
- **主视图**：`src/views/PluginStoreView.vue`
  - 刷新 Registry 按钮、目录过期横幅
  - 处理进度条（done/total/blockedByAvailability/error）
  - 进度详情：文件列表、筛选桶（pending/processing/done/error）、活动优先排序
- **数据层**：`src/composables/useExoticStore.ts`
  - `loadRegistry()` / `loadInstalled()` / `loadStatus()` / `refreshRegistry()`
  - `install()` / `uninstall()` / `repair()`
  - `mergeStorePlugins()` 全外连接

### 安装进度事件
- **事件流**：Channel<ExoticProcessingStatus>
- **ExoticProcessingStatus** 结构：running, paused, processing, done, blockedByAvailability, error
- **详情行**：ExoticTaskDetail — itemId, dirPath, fileName, format, capability, status, lastErrorCode
- **刷新频率**：通过 `get_exotic_processing_status` IPC 轮询

### Settings 链（状态管理）
- **状态键**（STATE_KEYS）：`exotic_auto_process` → 自动 / 暂停 / 停止
- **设置 Map**（settingsMap）：`exotic_enabled`, `exotic_paused` 等配置项
- **DynamicSettingControl toggleBindings**：UI 双向绑定（开/关）

### 用户可选模型/参数 UI 形态
- **CLIP 先例**：`src/stores/aiStore.ts` — `listModelRegistry()` 返 variants 列表
  - 各 variant 显示尺寸/精度/速度 trade-off
  - `setActiveModel(modelId)` 持久化选择
- **OCR 对标**（未施工）：
  - 模型选择卡片（mobile 轻量 vs server 精度）
  - `cls_input_hw` 等参数可能需要 UI 展示（当前 profile 固定）
  - 下载 UI：进度条、暂停/恢复、重试（复用 download_assets 进度事件）

---

## 6. IPC 与权限

### Exotic 相关 Command 清单
- **文件**：`src-tauri/src/ipc/exotic_commands.rs`

| 命令 | 签名 | 返回 | 用途 |
|---|---|---|---|
| `list_exotic_format_resolutions` | `()` | `Vec<FormatResolution>` | 列出全部格式可用态（首次离线也可用） |
| `get_exotic_item_state` | `(item_id)` | `(FormatResolution, Option<TaskStatus>)` | 单项可用态 + 缩略图任务态 |
| `list_installed_exotic_plugins` | `()` | `Vec<InstalledExoticPlugin>` | 已安装插件（Part1 为空） |
| `get_plugin_entitlement` | `(plugin_id)` | `PluginEntitlement` | 授权判定（availability + sku） |
| `get_exotic_processing_status` | `()` | `ExoticProcessingStatus` | 处理摘要（done/total/blocked/error） |
| `list_exotic_task_details` | `(bucket, limit, offset)` | `Vec<ExoticTaskDetail>` | 详情列表（分页、筛选） |
| `start_exotic_processing` | `()` | `()` | 恢复自动处理（清 paused） |
| `pause_exotic_processing` | `()` | `()` | 暂停处理 |
| `stop_exotic_processing` | `()` | `()` | 停止本次运行 |

### OCR 独占命令（`ocr_commands.rs`）
- `ocr_status(item_id)` → Availability（门控+安装态）
- `ocr_extract_image(item_id)` → `Vec<OcrLine>`（画廊识别）
- `ocr_extract_frame(frame_base64, size)` → `Vec<OcrLine>`（视频帧识别，前端截帧回传）
- `download_ocr_models(tier, on_progress)` → Channel<DownloadProgress>（模型下载）

### Capabilities 声明位置
- **文件**：`src-tauri/capabilities/`（Tauri v2 权限声明）
- **OCR 特殊性**（T7 发现）：四命令都是 app 自有（非插件），`core:default` 权限覆盖
  - 无需新增 capabilities 条目
  - clipboard 操作复用 `navigator.clipboard.writeText()`（与 LogWindowView 同）

### 错误类型
- **层次**：
  1. 协议层错误：`WorkerErrorCode` enum（path:line:422）
     - UnsupportedVariant, MalformedInput, ResourceLimit, IoError, InternalError, GpuUnavailable, SessionExpired, ModelLoadFailed, ...
     - 整数语义跨版本固定，serde 用 snake_case 字符串
  2. 应用层错误：`AppError` enum（`src-tauri/src/error.rs`）
     - `#[error]` 属性实现 Display（不泄露 subject_hash 等敏感信息）
     - OCR 可扩展 `#[error("OCR error: {0}")] Ocr(String)`
  3. IPC 返回：`Result<T>` = `std::result::Result<T, AppError>`（自动 serde::Serialize）

### 稳定 Code 化
- WorkerErrorCode 用 u32 + snake_case 名字确保跨版本一致
- 前端据此分流重试策略（retryable vs terminal）

---

## 7. 模型资产体量参照

### 现有模型文件大小（实测/下载清单）

#### CLIP（cn-clip-vit-b16）
- `image.img.fp32.onnx` ≈ 344MB
- `text.onnx` ≈ 109MB
- `vocab.txt` ≈ 110KB
- **合计**：≈ 453MB

#### 人脸（YuNet + SFace）
- `yunet_202402.onnx` ≈ 20MB
- `sface_afmatting.onnx` ≈ 136MB
- 对齐模板 ≈ 零星 KB
- **合计**：≈ 156MB

#### OCR（PP-OCRv5，两档）
- **mobile 档**：
  - det ≈ 2.3MB
  - cls（PP-LCNet_x0_25） ≈ 1.3MB
  - rec ≈ 8.6MB
  - dict（共用） ≈ 112KB
  - **小计**：≈ 12.2MB

- **server 档**：
  - det ≈ 49MB
  - cls（PP-LCNet_x1_0） ≈ 24MB
  - rec ≈ 118MB
  - dict（共用） ≈ 112KB
  - **小计**：≈ 191MB

### 存放路径约定
- **Models 目录基准**：`AppState.models_dir()`（同卷、canonicalize）
- **CLIP**：`{models_dir}/cn-clip-vit-b16/`
- **人脸**：`{models_dir}/yunet-sface/`（或 `{models_dir}/{face_profile.id}/`）
- **OCR**：`{models_dir}/pp-ocrv5-mobile/` + `{models_dir}/pp-ocrv5-server/`（或统一 `{profile_id}/`）
- **缓存**：`{ai_cache_dir}/` 别于 models（可能不同目录或磁盘）

### 降噪/超分参照建议
- **尺度**：假设 RealESRGAN / BSRGAN 等 4× upscaler ≈ 60-120MB（单档）
- **布局**：`{models_dir}/denoiser-{variant}/` + `{models_dir}/upsampler-{variant}/`
- **下载**：复用 `download_assets()` API，定义 ModelAsset 列表
- **档位**：参考 OCR mobile/server 分档逻辑，可提供 quality/speed trade-off

---

## 未覆盖（轮次预算达）

- [ ] 真机验证 swap_rb 通道序定案（OCR golden 对拍未跑）
- [ ] 下载引擎 UA header 补丁细节（commit 11d766e 归档）
- [ ] Part8 私有 registry 源拓扑（本线只读公开树占位实现）
- [ ] GUI 手测清单（见 OCR 线 progress 补录）
- [ ] 模型资产完整清单 URL + sha256（见 model-assets.md 追记段）

---

## 锚点索引

- Catalog：`src-tauri/src/exotic/catalog.rs:24-163`
- License：`src-tauri/src/exotic/license.rs:19-100` (KeyringLicenseStore)
- Coordinator：`src-tauri/src/exotic/coordinator.rs:44-129` (op_timeouts + plugin_descriptors)
- Model Download：`src-tauri/src/ipc/model_download.rs:43-69`
- OCR Catalog：`src-tauri/resources/exotic-catalog.json:18-31`
- OCR Profile：`crates/scrollery-ai-core/src/ocr_profile.rs:20-78`
- Protocol：`crates/exotic-protocol/src/message.rs:87-100` (OcrSessionInit/Close/Batch)
- Exotic Commands：`src-tauri/src/ipc/exotic_commands.rs:19-250` (8 commands listed)
- OCR Commands：`src-tauri/src/ipc/ocr_commands.rs:1-50`
- Plugin Store View：`src/views/PluginStoreView.vue:1-100`
- Exotic Store：`src/composables/useExoticStore.ts:1-100`

---

版本历史：

- 2026-07-24 创建，OCR 线 D-418..421 已确认，recon-exotic.md + construction-plan.md 为摸底源
