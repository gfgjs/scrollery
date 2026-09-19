---
id: 2026-07-24-ai-pipeline-map
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# AI/人脸流水线架构摸底

后续「降噪/超分」子系统设计的输入文档。映射现有 worker 拓扑、调度、存储、OCR 接入机制。

---

## 1. Worker 拓扑

### 1.1 进程结构

**AI/Face Worker** 是独立子进程，由 exotic 框架管理：

- **主进程**：`src-tauri/src/exotic/coordinator.rs:32–36` — 单一 Coordinator 调度器，幂等唤醒唯一一条 Pipeline
- **Worker 进程**：`crates/exotic-workers/ai-worker/src/main.rs:1–50` — AI 推理 Worker（Part4-T15）
  - 握手快速恒定（Hello→Ready，5s 档，D3 §2）
  - **模型加载不在握手**——显式 SessionInit 请求（host 侧 300s 档），模型加载在工作线程
  - 严格串行：一次一请求，无并发状态
  - 空闲自杀兜底：300s 无活动即 `exit(0)` — `main.rs:43`

### 1.2 Supervisor（进程监督）

`src-tauri/src/exotic/supervisor.rs:1–56` 管理子进程生命周期：

- 持有 Child 句柄 + stdin（经 WorkerConn 写协议帧）
- stdout reader 线程 → 帧 channel → WorkerConn.rx
- stderr drain 线程 → 64 KiB 环形缓冲（诊断）
- **关键边界**：超时/断开/协议违例 → kill → wait，标实例死亡；池补新实例前由 Pipeline 做崩溃退避
- Drop 兜底 kill + wait + join，绝不留孤儿进程

### 1.3 Worker 行协议与日志

**协议帧** — `exotic-protocol` v2（stdout 只走协议帧）：

- Request/Success/Failure/Progress/Shutdown 帧类型 — `crates/exotic-protocol/src/message.rs`
- SessionInit/EmbedBatch/FaceDetectEmbed/OcrSessionInit/OcrBatch 操作
- **日志** — stderr 单行 JSON — `exotic_protocol::WorkerLogLine`（D-313/D-314）
  - Supervisor 用 LineScanner 行级扫描 + 转发进主 tracing — `supervisor.rs:78–100`
  - Target 固定为 `"scrollery::worker"`，运行时字段 `"worker": "ai"`

### 1.4 崩溃重启与重试预算

- **Supervisor 层**：进程异常退出 → pipeline 标实例死亡，下轮 wake 补新实例（池补）
- **Pipeline 层**：批级失败（Failure 帧）→ 终止本轮，在途项保持 Processing，下次运行恢复 — `worker_pipeline.rs:19–22`
- **取消机制**：CancellationToken — `worker_pipeline.rs:130–131` 检查 `token.is_cancelled()` 即时生效
- **RunTokenSlot 用于人脸**（独立机制）— 见问题 3

---

## 2. ONNX 运行时（ort）

### 2.1 库与执行提供者

- **库**：`ort` crate，feature `"load-dynamic"`（手动管理 DLL 路径，避免自动下载） — `engine.rs:1–75`
- **执行提供者探测**：DetectML → CUDA → CoreML → OpenVINO → CPU — `provider.rs:5–80`
  - Windows 默认 DirectML（GPU-agnostic，支持 AMD/NVIDIA/Intel）
  - **关键红线**：文本编码器固定走 CPU，图像编码器可用 GPU（engine.rs:66–74 踩坑记录）
  - Provider 回声落库：session 就绪后传 provider 字符串，host 写回配置 — `worker_pipeline.rs:99–101`

### 2.2 Session 创建与模型路径

**两段式加载**：`session.rs:1–53` — 纯校验 + ort 构建

1. **validate_and_resolve**（纯校验，零 ort）— `session.rs:60–100`
   - models_root canonicalize 可达
   - arch_id/face_profile_id 在注册表可解析
   - 每个 ModelDescriptor 完整性：路径前缀检查（防宿主被劫持）、字节数与 sha256 逐一相符
   - 角色集完备：ImageEncoder + TextEncoder 必备（CLIP 成对）；FaceDetect + FaceRecog 可选但成对

2. **load_with_progress**（ort Session 构建，流式 Progress 帧）— `session.rs` 中工作线程执行
   - Stage-0 preflight：ORT runtime 快速检查（60s 档，D3 §2）— `main.rs:407`
   - Session 创建（可达分钟级，但期间发 Progress 帧 + 10s 心跳，host 收帧即重置静默计时）
   - 返回 AiEnginePool（Session 池）— `engine.rs:99–100`

### 2.3 输入预处理（Resize/Normalize）

位置分布：

- **图像编码**：`clip.rs` 模块（不展示但位置明确）
  - Resize 到 224×224（CLIP 标准）
  - Normalize 用 ImageNet 均值/方差（或模型特定值）
  - 输出为 `Vec<f32>` token
  
- **人脸检测**：`face.rs` 中 YuNet 预处理
  - 解码或从缓存读取，Letterbox 到 640×640（FACE_CACHE_SHORT_EDGE — `face_pipeline.rs:25`）
  
- **OCR**：`crates/scrollery-ai-core/src/ocr/` 三模型管线
  - Det：灰度，limit_side_len 降采样后送检测
  - Rec：归一化后送识别

---

## 3. 任务调度

### 3.1 AI（CLIP）分析触发与派发

**入口**：`src-tauri/src/ai/worker_pipeline.rs:47–113`

- 触发：由 `pipeline.rs:produce_tasks()` 查询 `count_pending_ai_items` 驱动（db queries — `db/queries/ai.rs`）
- **并发槽**：
  - F5 GPU 分析槽（AI 与人脸互斥）— 见问题 3.2
  - CPU permit：background_heavy_limiter（D2 先 CPU 后 GPU 令牌天条） — `worker_pipeline.rs:15–16`
- **与 coordinator 的关系**：
  - Coordinator 单一幂等 wake — `coordinator.rs:1–11`
  - Pipeline 按注册表（PluginDescriptor — `coordinator.rs:86–98`）逐能力评估与运行
  - 扫描/安装/激活/配置/重试时钟都触 wake —— coordinator 收 wake 事件，唯一 Pipeline 自然完成后再查 pending
- **进度事件**：
  - IPC 事件由 `write_results()` 发送 — `worker_pipeline.rs:95`
  - 前端 aiStore 订阅 `GET_AI_STATUS`、`START_AI_ANALYSIS` 等 — `aiStore.ts:49–55`
  - 实时更新 `status.analyzedItems` — `aiStore.ts:27`

### 3.2 人脸识别管线

**入口**：`src-tauri/src/ai/face_pipeline.rs:94–100`

- 触发：查询 `face_status=0` 的 media_items — `face_pipeline.rs:9`
- **三级定源**（缩略图档位 → face 缓存 640 → 小原图直派）— `face_pipeline.rs:4–6`
  - 缩略图档位（预测短边 ≥ detect_size）
  - face 缓存（短边 640 WebP，`FACE_CACHE_SHORT_EDGE` — `thumbnail/cache.rs`）
  - 小原图直派（白名单格式）— `face_pipeline.rs:10–14`
- **并发槽**：F5 GPU 分析槽（与 AI 互斥）；let步复用 `ai_yield_blockers()`
- **Writer**：成功项先删后插 + 置 Done，跑增量聚类 — `face_pipeline.rs:15–17`

### 3.3 派生（Thumbnail/Keyframe）

**入口**：`src-tauri/src/derive/pipeline.rs:78–100`

- 触发：`get_pending_derivations()` 按 kind 过滤，exclude_kinds 支持非破坏性暂停 — `db/queries/derivations.rs:44–80`
- **kind 扩展**：新增 kind 无需改动框架（kind-agnostic 设计）— `pipeline.rs:10–14`
  - 每种 kind 是纯函数 `kind::run()`
  - `DerivationContext` 携带源路径、缓存目录、缩略图尺寸等 — `pipeline.rs:62–71`
- **让步**：`should_yield_derivation()` 同 AI
- **取消机制**：CancellationToken — `pipeline.rs:88`（与 AI/人脸同机制）

---

## 4. 结果落账

### 4.1 DB 表与 Queries

**AI 分析**：

- 表：`ai_items`（ai_status: 0=pending, 1=processing, 2=done, 3=error）
- 嵌入向量：`ai_embeddings`（item_id FK，embedding blob）— queries 见 `db/queries/ai.rs`
- 写入线程模型：spawn_blocking — `worker_pipeline.rs:95` 的 `write_results()` 跑独立线程

**人脸**：

- 表：`faces`（face_status 同机制）、`face_instances`（检测结果 + 嵌入）
- 聚类：`face_clusters`（增量聚类由 writer 触发）— `face_pipeline.rs:55–57`
- 写入模式：小批 16 项落库脸行 + 状态（`STATUS_FLUSH_EVERY` — `face_pipeline.rs:57`）；聚类大批落库

**派生**：

- 表：`media_derivations`（item_id FK、kind、status 同 AI）
- 缓存路径：`thumb_cache/` 等（派生 kind 自行管理）
- 写入：`batch_finish_derivations()` — `derive/pipeline.rs:28`

**OCR**：

- 表：`ocr_items`（item_id FK、ocr_status、text blob）— 待后续立项
- 写入：预期同 AI 模式（spawn_blocking）

---

## 5. OCR（PP-OCRv5）复用 ai-worker

### 5.1 模型加载注册点

**独立会话槽**（D-OCR-1） — `ai-worker/src/main.rs:239–240`：

- CLIP 会话 `sess: Option<SessionState>`
- OCR 会话 `ocr_sess: Option<OcrSessionState>`
- 双槽互不干扰，SessionClose/OcrSessionClose 各自清空各槽

### 5.2 模型资产钉定

- 资产清单：`ocr_registry.rs:25–80`（七件物理文件：mobile/server 各 det/cls/rec + 共用 dict）
- 主源：RapidAI/RapidOCR ModelScope v3.9.2（sha256 本机实测，非转述）
- 镜像：GitHub GreatV/oar-ocr v0.3.0（cls 无镜像）
- 文件名单源：`scrollery_ai_core::ocr_profile::OcrProfile`

### 5.3 新 AI 能力接入清单（最小接线）

**核心步骤**：

1. **定义能力**
   - 在 `exotic_protocol::capability` 加常量（如 `pub const DENOISE: &str = "denoise"`）
   - Worker Ready.capabilities 里申明

2. **定义 RequestBody 变体**
   - `exotic_protocol::RequestBody` 加新 enum 臂（如 `DenoiseImage { items: Vec<DenoiseItem> }`）
   - 对应 Success/Failure 结果体

3. **Worker 侧实现**（`ai-worker/src/main.rs` + 新模块）
   - 新模块 `mod denoise;` + `handle_denoise()` 函数
   - SessionInit 时加载新模型（复用 validate_and_resolve 校验框架 — `session.rs`）
   - main.rs 的 handle_request 加新臂（SessionInit/Close/Batch 三件套）
   - 参考 OCR 双槽独立模式（`ocr_sess` 同 `sess`）

4. **Host 侧调度**
   - `coordinator.rs` PluginDescriptor 表加新行（plugin_id、worker_id、capability、uses_gpu）
   - Catalog 加新能力注册
   - `ai-worker/Cargo.toml` 依赖：若需超分/降噪专用库（如 RIFE/NAFNet），声明

5. **前端 Store + 命令**
   - `aiStore.ts` 或新 Store（`denoiseStore.ts`）管理状态、enable_denoise 开关
   - `IPC.START_DENOISE_ANALYSIS` 等命令，对标 `IPC.START_AI_ANALYSIS`
   - 设置页 DynamicSettingControl 绑定 `settingsMap['enable_denoise']`（见 memory#2）

6. **DB 落账**
   - 新表 `denoise_items`（item_id FK、denoise_status、result_path）
   - queries 函数同 AI 模式（produce_tasks → write_results）

---

## 6. 前端状态管理与设置

### 6.1 AI Store

**位置**：`src/stores/aiStore.ts:1–60`

- Status 结构：provider、gpuName、vramGb、batchSize、clipLoaded、totalItems、analyzedItems、pendingItems、errorItems、isAnalyzing — `aiStore.ts:19–33`
- 分析控制委托共享 `useAnalysisController` — `aiStore.ts:47–55`
  - 命令映射：START_AI_ANALYSIS、PAUSE_AI_ANALYSIS、RESTART_AI_ANALYSIS、STOP_AI_ANALYSIS

**人脸 Store**：`src/stores/faceStore.ts` — 同架构，独立 F5 GPU 槽

**派生 Store**：`src/stores/derivationStore.ts` — 进度追踪

### 6.2 设置项与开关

**配置源**：`src/components/settings/SettingsView.vue` + `DynamicSettingControl.vue`

- `enable_ai` 开关绑定 `settingsMap['enable_ai']`（D-002 设计 — MEMORY#2）
- `enable_face` 开关绑定 `settingsMap['enable_face']`
- `enable_video_keyframes` 等派生开关
- **模型/参数选择 UI**：
  - 模型档位选择（无现成 UI 先例；与 AI provider 同级别配置）
  - BatchSize 配置（存 settings，runtime 快照） — `worker_pipeline.rs:54`
  - 图 AI 缓存短边（336px 固定 — CLIP_CACHE_SHORT_EDGE）

---

## 7. 图片处理接入点候选

### 7.1 图片编辑管线

**位置**：`src-tauri/src/editing/` 模块

- **Cropper**：前端 `src/components/media/player/ImageEditor.vue` 或独立组件（Cropper.js 2.1.1）
- **色彩调整**（E3）：`editing/adjust.rs:1–52`
  - 亮度/对比度/饱和度三参数，应用在 sRGB gamma 编码域
  - 源 ICC 经 moxcms(relative colorimetric) 转 sRGB → f32 公式 → 量化回写
  - 无 ICC 按 sRGB 直入，原位深保持（8→8、16→16、f32→f32）
  - **降噪/超分接入点**：
    - 若作为编辑操作（类似 E1=Cropper、E3=Adjust）：在 EditOps 加 `denoise` 和 `upscale` 字段，editing/denoise.rs + editing/upscale.rs 实现，管线同 adjust
    - 若作为 AI 派生（类似缩略图/关键帧）：见 7.2

### 7.2 派生（缩略图/预览）管线

**位置**：`src-tauri/src/derive/pipeline.rs` + `src-tauri/src/derive/kind/`

- 每种派生 kind 是纯函数 `kind::run(DerivationContext) → Result`
- 现有 kind：thumbnail、keyframe、doc_thumb 等
- **降噪/超分作为派生**：
  - 新建 `derive/kind/denoise.rs` + `derive/kind/upscale.rs`
  - 输入：source_path（原图）+ 派生缓存目录
  - 输出：result_path（降噪/超分后的图）
  - DB：`media_derivations` 表里新 kind 行，status 状态机同现有派生

### 7.3 导出路径

**位置**：`src-tauri/src/export/core.rs:1–58`

- Staging 复制 + 命名 + manifest 机制
- **导出版本选择**：
  - 源图（未处理）
  - 编辑后图（若调整/裁剪过）
  - 派生图（缩略图档位 / 预览 / 降噪 / 超分等）
- 若降噪/超分结果落账为派生文件，导出时可按用户选择版本输出
- 路径构建：`export/naming.rs` — 按命名方案 + 冲突解决

### 7.4 接入决策（仅列位置，不做方案）

**编辑操作**（作为 EditOps 扩展）：

- 修改：`editing/denoise.rs`（新建）+ `editing/upscale.rs`（新建）
- 命令：`ipc/editing_commands.rs` 加新 IPC 命令（apply_denoise、apply_upscale）
- 预览：viewer_color 管线复用（色彩变换同 adjust） — `viewer_color/render.rs`

**派生操作**（作为 kind 扩展）：

- 新建：`derive/kind/denoise.rs` + `derive/kind/upscale.rs`
- 配置：settings 里加 `enable_denoise`、`enable_upscale` 开关 + 模型/质量参数
- DB：`media_derivations` 加对应 kind 行

**与 AI 集成**（若超分/降噪用 AI 模型）：

- ai-worker 新建 Session 槽（同 OCR 模式 D-OCR-1）
- 或复用现有 CLIP Session 池（但模型加载、execution provider 可能冲突，待评估）
- Pipeline 触发：coordinator 按能力路由

---

## 未覆盖清单

1. 超分/降噪具体模型（RIFE/RealESRGAN/NAFNet 等）的 ort 适配、量化策略 — 需上游模型方案定稿
2. 批处理策略对超分/降噪的影响（内存预算、延迟可接受性） — 需实测与性能评估
3. 编辑 vs 派生 vs AI 能力的规范化决策 — 用户界面及工作流梳理

---

## 附注

本摸底按现有 AI/OCR/派生管线逆向梳理，关键锚点均以 `path:line` 格式标注。降噪/超分子系统设计时：

- 若模型走 ort + worker 进程化：复用 ai-worker 架构（会话槽、协议帧、重试预算）
- 若模型走本地轻量库（如 OpenCV denoise）：归入派生 kind 框架
- 编辑 vs 派生选择：取决于实时预览需求（编辑走 viewer_color 管线，派生走后台批处理）
