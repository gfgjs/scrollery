---
id: 2026-07-23-recon-exotic
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# OCR 作为付费插件的施工地形图

**摸底日期**：2026-07-23  
**范围**：exotic 插件体系、商店前端、AI 模型列表、ai-worker 批协议

---

## 一、Exotic 插件体系

### 插件定义与注册
- **Catalog 定义**：`src-tauri/src/exotic/catalog.rs` — `CatalogOffering` 结构体聚合单插件信息
  - 核心字段：`plugin_id`、`display_name`、`formats[]`、`capabilities[]`、`license_tier`、`sku`（付费插件）
  - `MediaKind` 枚举：Image / Video / Audio / Document
  - `Capability` 枚举：Thumbnail / Metadata / Text / Embedding / FaceDetectEmbed
- **内置 Catalog**：`src-tauri/resources/exotic-catalog.json`（编译期嵌入）
  - 当前仅含 PSD 插件（`exotic-image-psd`，paid，sku=`psd-engine-2026`）
  - JSON 结构：`schema` / `sequence` / `offerings[]` 三层
  - 新增 OCR 需在 `offerings[]` 添一条，指定 `formats: ["ocr"]`、`capabilities: ["embedding", "text"]`、`license_tier: "paid"`、`sku`

### 门控与可用态
- **可用态枚举**：`src-tauri/src/exotic/mod.rs:49-69` — `Availability`
  - `AvailableUninstalled` — 有产品、未安装
  - `InstalledUnlicensed` — 已装、未授权
  - `Authorized` — 已授权、可运行
  - 其余：平台不兼容、Host 版本不符、损坏、禁用、过期等
- **门控点**：`ExoticHost::availability_of()` (`src-tauri/src/exotic/mod.rs:386-423`)
  - 顺序：平台 → Host 版本 → (dev fixture) → 安装状态 → 授权状态
  - 返回 `FormatResolution` 供前端展示

### 现有插件清单
- **PSD**：`exotic-image-psd`（thumbnail 能力、Windows x86_64 + macOS arm64）
- **AI**：`ai-worker` 为单独 worker crate (`crates/exotic-workers/ai-worker/src/main.rs`)
  - 非 exotic catalog 条目，由 ai 管线直接启动（T15 合并单 worker，embedding + face_detect_embed 能力）

### 付费/免费体现
- **编译期分支**：`src-tauri/src/exotic/mod.rs:276-295` — `default_entitlement_provider()`
  - `feature = "channel-direct"` 时走直销（内置或私有 DirectEntitlement）
  - 无条件 else 回退 `KeyringLicenseStore`（公开树占位实现，验签恒失败）
- **token 存储**：keyring（系统 API），由 `activate_exotic_plugin` IPC 写入
- **验签入口**：`license::evaluate()` — SKU + token 时间窗检查（fail-closed）
- **dev/test 旁路**：`PICASA_EXOTIC_DEV_KEYSET` 环境变量指定本地签名集（仅 debug 构建）

---

## 二、插件商店前端

### 核心组件
- **主视图**：`src/views/PluginStoreView.vue`
  - 展示：插件卡片 + 安装状态 + 格式 + SKU 徽章
  - 操作：刷新 Registry → 装 / 卸 / 升 / 修复 / 回滚
  - 进度面板：处理摘要（done/total/blocked/error） + 详情展开（任务列表、筛选桶）
  
- **数据层**：`src/composables/useExoticStore.ts`（T11）
  - `mergeStorePlugins()` — registry × installed 全外连接，生成 `StorePluginRow[]`
  - API：`loadRegistry()` / `loadInstalled()` / `loadStatus()` / `refreshRegistry()` / `install()` / `uninstall()` / `repair()`

### 数据来源与新增流程

| 数据源 | 命令 | 前端消费 | 新增 OCR 需修改 |
|--------|------|---------|-----------------|
| Registry | `LIST_EXOTIC_REGISTRY` | `useExoticStore.loadRegistry()` | 后端从 registry 源解析并验签 |
| Installed | `LIST_INSTALLED_EXOTIC_PLUGINS` | `useExoticStore.loadInstalled()` | DB `exotic_plugins` 自动包含 |
| Processing | `GET_EXOTIC_PROCESSING_STATUS` | 进度面板 | 后端自动追踪 |

新增 OCR 条目涉及文件：
- `src-tauri/resources/exotic-catalog.json`（添加 offering）
- 后端 registry 源（Part8，私有树维护）
- 前端类型定义 `src/types/exotic.ts`（OCR format 无特殊处理，复用通用字段）

### 许可流程
- **激活**：`ACTIVATE_EXOTIC_PLUGIN` → token 写 keyring，DB 标 `install_state: "installed"`
- **门控判定**：`GET_PLUGIN_ENTITLEMENT` → 后端返 `PluginEntitlement`（availability + source_tag + sku）
- **前端 gate**：ExoticActivateDialog（复用 PSD 流程）+ 购买链接跳转

---

## 三、AI 模型下载列表

### 组件与数据流

| 层级 | 文件 | 职责 |
|------|------|------|
| 前端列表 | `src/stores/aiStore.ts:244-268` | `listModelRegistry()` / `setActiveModel()` / `downloadModel()` 三接口 |
| 后端命令 | `src-tauri/src/ipc/ai_commands.rs` | `list_ai_models` / `list_model_registry` / `set_active_model` / `download_model` 实现 |
| 模型源 | `src-tauri/src/ai/remote_registry.rs` | 硬编码模型元数据（arch / batch / fp16/fp32 / 文件名） |
| 下载管线 | `src-tauri/src/ipc/model_download.rs` | Channel<ModelDownloadProgress> 流式回传 |

### 关键定义位置
- **模型注册表**：`src-tauri/src/ai/remote_registry.rs`
  - `ArchMeta` — 架构元数据（CLIP 图像塔、文本塔、batch 尺寸）
  - `ModelRegistry` — UI 列表投影（可用变体 + 激活态）
  - 变体文件名规则：`{arch_id}.img.{fp_marker}.onnx` + `.text.onnx` + 补充文件

- **模型下载**：`src-tauri/src/ipc/model_download.rs`
  - `download_assets(imageFile)` — 拉取 image / text / extra / shared .tar / vocab（HTTP 流式）
  - 目标路径：`{models_dir}/{arch_id}/` （canonicalize + 同卷校验）
  - 进度通道：每 65KB 块回传一次

### 新增 OCR 模型组需修改的文件
1. **元数据**：`src-tauri/src/ai/remote_registry.rs`
   - 新增 `OcrArchMeta` struct（或扩展通用字段）
   - 注册 det/cls/rec 三模型的多档位（如 `small/medium/large`）
   - 文件名规则（模仿 CLIP：`ocr-det-small.img.fp32.onnx` 等）

2. **模型注册表**：UI 需看到 OCR 模型分组，在 `listModelRegistry()` 响应中聚合返回
   - `ModelRegistry.ocrVariants?` 或扩展 `variants` 字段

3. **下载逻辑**：`model_download.rs` 中区分 CLIP vs OCR 下载坐标
   - OCR 三文件异步下载拼装（或单次 .tar）

4. **激活逻辑**：`SET_ACTIVE_MODEL` 对 OCR 可能需特殊处理（多文件选择 vs 单 imageFile）

---

## 四、AI-Worker 批处理协议

### 协议定义
- **Frame 格式**：`crates/exotic-protocol/src/frame.rs`
  - magic(2B) + version(2B) + type(1B) + request_id(8B) + json_len(4B) + blob_len(4B) + json + blob
  - `PROTOCOL_VERSION = 2`（v2 additive：EncodeText 新增，不动版本号）

- **消息体**：`crates/exotic-protocol/src/message.rs:28-87`
  - `RequestBody` enum （tag-based JSON）：
    - `Thumbnail { item_id, source_path, target_long_edge, input_fingerprint }`
    - `Metadata { ... }`
    - `SessionInit { session_id, models, model_profile, models_root, ai_cache_dir, image_provider }`
    - `SessionClose { session_id }`
    - `EmbedBatch { items: Vec<EmbedItem> }` — CLIP 图像批嵌入
    - `FaceDetectEmbed { items: Vec<FaceItem>, det_score_thresh }`
    - `EncodeText { texts: Vec<String> }` — 文本编码（T17，semantic search 用）

### Worker 端处理
- **ai-worker 主循环**：`crates/exotic-workers/ai-worker/src/main.rs:14-49`
  - 握手（Hello → Ready）快速恒定，模型加载不在握手（D3 §2）
  - SessionInit 配置 300s 超时（模型可能加载分钟级）
  - 严格串行：一次一请求，无并发

- **批处理**：`crates/exotic-workers/ai-worker/src/batch.rs:1-44`
  - `EmbedBatch` / `FaceDetectEmbed` 逐项化：项失败不连坐整批
  - 嵌入本体不进 JSON，走同帧 blob（f32 LE）
  - 人脸批承载两类几何：face_det（JSON）+ embeddings（blob）

### 宿主端发送与等待
- **Worker 生命周期**：`src-tauri/src/ai/worker_client.rs:4-20`
  - spawn → ensure_session → 批请求（EmbedBatch / EncodeText）→ 输出校验 → SessionClose
  - 超集放宽：spec 不需人脸时可复用带人脸的合并会话

- **错误恢复**：`worker_client.rs:11-16`
  - 进程级异常 → 重建 worker + 重建会话 + 重发一次（MAX_ATTEMPTS=2）
  - 终止错误（EmbedDimMismatch 等）→ 不重试，直返

- **调度**：`src-tauri/src/exotic/coordinator.rs:40-70`
  - EmbedBatch 超时：120s（硬参数）
  - FaceDetectEmbed 超时：60s 基础 + 6s/item（项数线性放宽）
  - EncodeText 超时：30s

### 新增 OcrBatch 需修改的文件

1. **协议定义**：`crates/exotic-protocol/src/message.rs`
   - 新增 `OcrBatch { items: Vec<OcrItem> }` variant
   - 新增 `OcrItem` struct（item_id / cache_key / source_path / fingerprint）
   - 新增 `OcrBatchSuccess` struct（per-item results + 文本 blob）
   - `capability::OCR_TEXT = "ocr_text"` 常量

2. **Worker 处理**：`crates/exotic-workers/ai-worker/src/batch.rs` 或新建 `ocr.rs` 模块
   - `handle_ocr_batch()` — 并行解码/推理，收集 OCR 文本结果
   - 文本本体走同帧 blob（UTF-8 JSON array of strings + 长度前缀）或压缩格式

3. **宿主端**：`src-tauri/src/ai/worker_client.rs`
   - 扩展 `SessionSpec` 支持 ocr_profile（模型地址等）
   - 新增 `send_ocr_batch()` method（调用同一 worker）
   - 输出校验 `validate_ocr_batch_output()`

4. **协调调度**：`src-tauri/src/exotic/coordinator.rs`
   - `plugin_descriptors()` 添 AI 插件条目（如已有），或新增 `ocr-worker` descriptor
   - `Capability::OcrText` 注册到 AI 或 OCR worker
   - `op_timeouts::OCR_BATCH` 定义（初值 120s，实测调）

5. **DB 新增**：
   - `exotic_tasks.capability` 新值 `"ocr_text"`
   - task 状态表支持 OCR 任务跟踪（同 embedding/face_detect_embed）

---

## 摸底结论

### 四域交点：OCR 插件化施工路径

| 域 | 新增/修改点 | 优先级 |
|----|-----------|--------|
| **Catalog** | `exotic-catalog.json` 添 OCR offering（formats: ["ocr"]、capabilities: ["text"]） | P0 |
| **商店** | 前端自动支持（StorePluginRow 复用） | P0（无改动） |
| **模型** | `remote_registry.rs` 添 OCR 模型元数据；`model_download.rs` 可能需区分下载路径 | P1 |
| **协议** | `exotic-protocol::RequestBody::OcrBatch` + `OcrBatchSuccess`；`capability::OCR_TEXT` | P1 |
| **Worker** | 复用 `ai-worker` 或新建 `ocr-worker`；新增批处理逻辑 | P1 |
| **宿主** | `worker_client.rs` 新增 `send_ocr_batch()`；`coordinator.rs` 注册 descriptor | P1 |
| **DB/日志** | `exotic_tasks.capability` 新值；OCR 任务流跟踪 | P2 |

### 关键风险与红线
- **token 验签链**：OCR SKU 必须与 Catalog 保持一致（fail-closed）
- **多文件模型管理**：det/cls/rec 三文件的完整性检查 + 原子加载（对齐 ModelDescriptor）
- **worker 进程生命周期**：SessionClose 显式卸载，空闲 300s 自杀（防 VRAM 僵尸）
- **超时配置**：OCR 推理时间（尤其 source_path 回退解码）线性于图像分辨率

---

## 附录：文件清单速查

### 核心路径
- 插件系统：`src-tauri/src/exotic/{mod.rs, catalog.rs, coordinator.rs, license.rs, worker.rs}`
- 商店前端：`src/views/PluginStoreView.vue`、`src/composables/useExoticStore.ts`
- 协议：`crates/exotic-protocol/src/{lib.rs, message.rs, frame.rs}`
- AI 模型：`src-tauri/src/ai/{remote_registry.rs, worker_client.rs}`、`src/stores/aiStore.ts`
- Worker：`crates/exotic-workers/ai-worker/src/{main.rs, batch.rs, session.rs}`

### 新增 OCR 配置清单
- [ ] `exotic-catalog.json`：新增 OCR offering
- [ ] `exotic-protocol/src/message.rs`：`OcrBatch`、`OcrItem`、`OcrBatchSuccess`、`capability::OCR_TEXT`
- [ ] `ai-worker/src/batch.rs`：`handle_ocr_batch()` 逻辑
- [ ] `worker_client.rs`：`send_ocr_batch()` 接口
- [ ] `coordinator.rs`：OCR descriptor 注册（或 AI descriptor 扩展）
- [ ] `remote_registry.rs`：OCR 模型元数据（det/cls/rec 三模型）
- [ ] DB schema：`exotic_tasks` 新增 OCR 任务类型支持
- [ ] 前端类型：`src/types/exotic.ts` 无需改（format="ocr" 自动识别）

