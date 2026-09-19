---
id: 2026-06-26-Part4_AI与人脸插件化
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 4 · AI 与人脸插件化

> 依赖：[Part0](Part0_总纲与产品定稿.md)（§5 打包模型、§5.5 关键架构决策、§7 盈利诚实边界、§8 防护层1）、[Part1 数据层](Part1_数据层.md)（`VectorStore` trait、两阶段 ANN 选型、向量 schema）、[Part6 插件平台](Part6_插件平台与exotic收尾.md)（**消费其插件平台地基：worker 框架升级 / `fetch_exotic_registry` / AES 加密原语 / 签名 Registry / `InstallLimits` / `model_blobs` 下载**）、[Part3](Part3_缩略图派生与GPU引擎.md)（`ai_thumb` 缓存喂 CLIP）、[Part2](Part2_扫描与画廊流水线.md)（`invalidate_derived_for_item` 的 AI/face 失效钩子）。
> 状态：定稿待执行（已并入 terminal review；正文即权威，§8 为历史留痕）。执行前必读 Part0 §13 + Part1 §7 + Part6 §7 + 本文。**旧 docs/记忆不可轻信，以代码实测为准。**
>
> ⚠️ **实施序**：本 Part 的「插件化」**依赖 Part6 平台地基先就位**（Part0 §0 表：Part4 依赖 Part1+Part6）。Part4 聚焦 AI/人脸**推理侧**改造（去 ort、worker 化、ANN、双轨人脸、GPU 协调、模型加密），平台原语（Registry/AES/Supervisor）由 Part6 提供、本 Part 调用。

---

## §1 目标与范围

### 1.1 本 Part 解决什么

AI/人脸是「离线语义搜索 + 人脸识别」核心卖点，也是「<10MB 核心」最大的体积来源（ort + ONNX DLL ~25MB + 模型）。本 Part 把二者从「编译进主二进制」迁到「sidecar worker 插件」，并补 ANN、双轨人脸、GPU 协调、模型加密：

1. 🔴 **AI/人脸推理外置 sidecar worker**（Part0 §5.5 决策 1，核心架构）：`ai-worker`(ort+CLIP) / `face-worker`(ort+YuNet+SFace) 走 exotic 的 `WorkerConn/Supervisor/exotic-protocol` stdio 帧协议；**拒绝进程内 dlopen**（Tauri #8090 + ort load-dynamic + 无崩溃隔离三重障碍）。
2. 🔴 **去 ort**（Part0 §5 line 197）：核心 `Cargo.toml` 移除 `ort`、`tokenizers` 随之移入 AI 插件；`tauri.conf.json` 移除 4 个 ORT DLL `bundle.resources`（省 ~25MB）。**这是 <10MB 的关键一步。**
3. **CLIP 中文双档**（Part0 §3 差异点）：ViT-B/16 fp16 默认（轻）+ ViT-L/14-336 fp32 可选 HQ；**文本编码器固定 CPU**（DirectML 对 BERT int64 Gather 静默算错，进程隔离后约束不变）。
4. **ANN 检索**（Part0 §1 实测 #5，向量暴力 O(N) queries.rs:1874）：暴力全表扫描 → 两阶段 ANN（消费 Part1 `VectorStore` trait + simsimd/sqlite-vec/usearch 选型）。
5. **人脸 pipeline 插件化 + 双轨**（Part0 §4 卖点 6、§1 实测 #4）：默认轨 YuNet(MIT)+SFace(Apache) 商用安全；可选 SCRFD+ArcFace **非商用另一 sku**；批量审批后端；**模型切换安全**（TODO2：active-track 切换 + persons 按模型迁移）。
6. **GPU 跨 worker 协调**（Part0 §5.5 决策 4）：ai-clip/ai-face worker 隔离，GPU 显存互斥由 Coordinator 在 `plugin_id` 层协调（同时只一个 AI worker 持 GPU session）。
7. **模型 blob 分离 + AES 加密权重**（Part0 §5.5 决策 3、§8 层1）：`RegistryEntry.model_blobs[]` 分步下载（避免单 zip 超 512MB）；ONNX 权重 AES-256-GCM 加密、密钥 license 派生（无 license 下到的是密文）。
8. **ai_thumb 喂 CLIP**（接 Part3 §3.6）：复用短边 336 的 AI 缓存解码喂 CLIP，避免解原图饿死 GPU（memory `gotcha-ai-analysis-cpu-decode`）。
9. **模型库动态发现**（memory `ai-model-library-dynamic`）：从 `gficcg/clip_cn_vit-onnx` 在线发现「架构→batch 变体」；架构 id=向量身份不变量、变体只换图像 onnx；固定 batch 须补齐尾批。
10. **合规隔离**（Part0 §1 红线）：SCRFD/ArcFace 非商用 **CI 断言不打包**；CLIP 权重无 license → 自 OFA-Sys MIT 源导出自托管。
11. **开放技术决策拍板**（Part0 §5.5）：① ai-clip+ai-face 合并单 `ai-worker` vs 分离（按显存实测）；② 默认档位；③ mac CPU-only vs CoreML（建议 P6.5 后）。
12. **AI/face 失效钩子**（接 Part2 §3.3 / Part3 §3.4）：`invalidate_derived_for_item` 复位 `ai_status`/`face_status`，源变更后 AI/人脸结果不停留旧图。

### 1.2 不在本 Part（归属其它 Part）

- **插件平台地基本身**（worker 框架升级、`fetch_exotic_registry` IPC、AES 加密**原语**、`EntitlementProvider` 抽象、签名 Registry、`InstallLimits`、`model_blobs` 下载机制）→ **Part6**（本 Part 消费）。
- **`VectorStore` trait 定义 / 向量 schema / ANN 库选型** → **Part1**（本 Part 落地推理侧调用）。
- **前端 AI/人脸 UI**（语义搜索框、人脸审批 UI、插件 gate/购买引导）→ **Part5**（本 Part 提供命令）。
- **商业化定价 / license 签发服务 / 诚实文案** → **Part8**。
- **平台签名 / CoreML GPU entitlement** → **Part7**（mac GPU 加速 P6.5 平台签名后再开）。

### 1.3 全局约定 + 三红线（继承 Part0 §12 + §1 + §8）

- 🔴 **合规红线**：SCRFD(det_10g)+ArcFace(w600k_r50) InsightFace 非商用——**绝不进任何付费/免费发行包**，CI 断言不打包；CLIP 权重自导出（OFA-Sys MIT 源）自托管。
- 🔴 **防白嫖红线**：ONNX 权重 AES-256-GCM 加密、密钥 license 派生（`enc_seed` 进 token）；门控与密钥派生经 Part6 `EntitlementProvider` 与 AES 原语实现（第一方公开源码统一 AGPL-3.0-only，Part0 §10），**商业边界是私钥/签发凭证与付费载荷**，非源码可见性。
- **诚实边界**（Part0 §7）：CLIP/YuNet/SFace 权重部分来自公开仓库，插件卖「集成+中文+更新+体验」非「秘密权重」；加密挡懒人+为私有模型铺路，不夸大独家。
- 让步：AI 对交互让步（`should_yield_derivation` 同阶梯）；rayon 内永不 `.await`；worker 协议帧经 stdio；新增 DAO/命令返回 `AppError`；中英双语注释；改后中文 commit、仅用户通知时 push；大文件小步 Edit。

---

## §2 现状实测（代码取证，文件:行）

> workflow `w8993ybw5`（4 agent 并行测绘 CLIP/人脸/exotic-worker 框架/去 ort 改造面）回传，逐项 file:line 核实。**与旧 docs/记忆冲突处，以本节为准。**

### 2.1 CLIP / AI 推理现状

**ort 集成**（[engine.rs](../../src-tauri/src/ai/engine.rs)）：`ort` crate（`load-dynamic`，运行时 dlopen `onnxruntime.dll` ≥1.26 经 `ORT_DYLIB_PATH`）→ `Session::builder().commit_from_file()`。EP 分支（engine.rs:487-542）：Win→DirectML(Level1+intra=1+no_mem_pattern)、CUDA/CoreML/OpenVINO(Level3)、CPU(Level1+all_cores)。**推理进程内同步阻塞**：`Session::run()` 在 `spawn_blocking` 串行，`SessionPool`（crossbeam channel + RAII 借还），GPU `pool_size=1`、CPU `pool_size=2`。

**模型加载**（engine.rs:175-226 / [profile.rs](../../src-tauri/src/ai/profile.rs):272-296 / [clip.rs](../../src-tauri/src/ai/clip.rs):550-659）：图像/文本编码器**完全分离**为两个 `SessionPool`。路径 `app_data_dir/models/`，文件名由 `ModelProfile.image_file/text_file` 决定，DB 配置 `ai_active_model`(架构 id) + `ai_active_image_file`(变体) 读取。tokenizer = `tokenizers` crate WordPiece（vocab.txt + BertNormalizer + TemplateProcessing `[CLS]$A[SEP]` + Truncation 52 + Padding Fixed 52）。**双档切换**：架构 id = 向量空间主键（cn-clip-vit-b16 / -l14-336），变体只换图像 onnx，切换时 drop 旧 `AiEnginePool` 重载。已知 5 架构：ViT-B/16 fp16/fp32(512维)、ViT-L/14(768)、ViT-L/14@336(768)、ViT-H/14(1024)。

🔑 **文本编码器固定 CPU**（engine.rs:64-73/267-280）：硬编码 `AiProvider::Cpu`，**不受 override**。原因：eisneim cn-clip BERT 文本塔含 int64 token id 的 embedding Gather，**DirectML 静默算错**（不报错不回退）→ 查询向量污染（「白色的猫」返回纸箱/快递）。图像编码器（ViT）不受影响走最优 EP。文本小、每搜一次，CPU ~200ms 可接受。**外置后须保持图/文双 EP 分离**，不能合并进同一 session。

🔴 **向量检索暴力 O(N)**（[search.rs](../../src-tauri/src/ai/search.rs):28-157）：向量小端 f32 BLOB 存 `ai_embeddings`(model_name+item_id+embedding)；检索时一次性读全表 → 转 f16（half）打包 `EmbeddingCache.data`(Vec<f16> 行主序) + `ids`。**1M×512×2B=1GB 常驻**（f16，半于 f32 2GB）。`rayon` 并行全表点积 `dot(query_f32, row_f16→f32)`，O(N×D) **无索引**，百万级每搜 ~1s+。`sort_unstable` 取 top-K 存 `ai_search_results`。批：DB BATCH=512；推理 batch 由 `ai_batch_size` 或 VRAM 自动档（2G→16/4G→32/8G→64/12G+→256），**固定 batch 抬升至 ≥k + 尾批补齐复制末帧、推理后取前 cur 行**。

**pipeline 调度**（[pipeline.rs](../../src-tauri/src/ai/pipeline.rs):89-359 / [ai_commands.rs](../../src-tauri/src/ipc/ai_commands.rs):392-433）：`start_ai_analysis` → `ensure_engine_initialised` → `try_acquire_gpu_analysis`(与人脸互斥) → `start_ai_pipeline`(tokio→spawn_blocking→rayon::scope)。**让步**：生产者每批前 `ai_yield_blockers()` 非空 sleep 500ms（**仅生产者让步、推理线程不让**，避免与 2s 重排饿死）。`ai_status` 状态机 0/1/2/3，崩溃遗留 Processing 经 `reset_processing_ai_items` 续传。**自然完成 drop `AiEnginePool` 释放 VRAM** + `invalidate_embedding_cache`。

**模型库动态发现**（[remote_registry.rs](../../src-tauri/src/ai/remote_registry.rs):27-262）：HF tree API（`gficcg/clip_cn_vit-onnx`，双源 hf.co/hf-mirror，缓存 10 分钟 static Mutex）→ `classify()` 按文件夹归类架构，识别图像变体（`.img.` + `parse_batch` bN→Fixed/dyn→Dynamic）、文本塔（`.txt.`）。**架构 id=向量身份不变量**，变体只换 `ai_active_image_file`，文本塔同架构共用。LFS oid=sha256 直生校验清单。静态 fp16 B/16（eisneim 仓库）走 `static_fp16_b16_assets()` 固定清单。

### 2.2 人脸子系统现状

**pipeline 全链**（[face_pipeline.rs](../../src-tauri/src/ai/face_pipeline.rs) / [face.rs](../../src-tauri/src/ai/face.rs)）：4 线程 Producer→Preprocessor(rayon)→DetectEmbed(单/多 worker)→Writer。**检测** YuNet（letterbox 640² BGR NCHW、priors+strides{8,16,32}+NMS IoU0.3）→ `DetectedFace{bbox,landmarks[5],score}`；**对齐** 5 点相似变换（手写高斯消元）→ 112²；**嵌入** SFace(0-255 RGB,128维)/ArcFaceStd((x-127.5)/127.5,512维) L2 归一化；**聚类** `cluster_new_faces`（增量最近质心，阈值 0.363/0.40）。**与 CLIP 完全独立流水线**，但**共用** `AiEnginePool` + `ai_yield_blockers()` + `gpu_analysis_owner` 互斥槽。face `pool_size=2`（GPU，注释「DX12 锁争用风险待实测」）。

🔴 **双轨 + commercial_ok**（[face_profile.rs](../../src-tauri/src/ai/face_profile.rs):132-205 / [face_commands.rs](../../src-tauri/src/ipc/face_commands.rs):363-462）：
- 默认 `yunet-sface`：`commercial_ok=true`，assets 含已校验 sha256 直链（opencv_zoo），`download_face_model` 可一键下。
- 可选 `scrfd-arcface-r50`：`commercial_ok=false`，**assets 空** → `download_face_model` 遇空直接 Err（仅手动导入）。
- ⚠️ **`list_face_model_registry` 原样暴露两轨（含 commercial_ok=false），后端无渠道级过滤守卫**，靠前端处理。
- 🔴 **SCRFD/ArcFace 是结构性死代码**：`set_active_face_model` **命令不存在**，`face_model_active` 仅 SCHEMA_V8 播种 yunet-sface、从不被写 → 引擎永远加载 YuNet，`detect_scrfd`/ArcFace 路径**不可达**。磁盘放 onnx 使 `installed=true` 但不激活（待 InsightFace 对拍）。`detect_scrfd`（face.rs:253-400）标注 **UNVERIFIED 未对拍**。〔2026-07-02 更新:`set_active_face_model` 已建成(gated,`fa0e951`),但 `verified` 对拍门拒绝激活 SCRFD 轨——「不可达」从**无命令**变为**有门有据**,对拍通过前维持现状〕

**schema**（[schema.rs](../../src-tauri/src/db/schema.rs):406-452，SCHEMA_V8）：
- `persons`：id/name/cover_face_id/centroid(BLOB f32)/face_count/is_named/is_hidden/is_ignored。🔴 ~~**无 model_name 列**（单模型假设）~~〔✅ V11 已加列;2026-07-02 `fa0e951` 查询侧全面按模型隔离〕。
- `faces`：id/item_id(CASCADE)/person_id(SET NULL)/**model_name**(向量空间键)/bbox/landmarks(BLOB)/det_score/quality/embedding(BLOB f32：SFace 512B/ArcFace 2048B)/`is_confirmed`/created_at。
- `media_items.face_status`：独立列 0/1/2/3（与 ai_status 独立）。

**批量审批**（face_commands.rs:287-355 / [queries.rs](../../src-tauri/src/db/queries.rs):2335-2415）：**已有** `merge_face_persons`(合并+质心加权重算)/`rename_face_person`/`set_face_person_hidden`/`recluster_faces`(全量重聚类,锁 is_confirmed)。🔴 **缺**：无 `confirm_face_assignment`/`reassign_face_to_person`/`unassign_face`/approve-reject likely_match——**`is_confirmed` 字段就绪（schema + recluster 消费）但无任何 IPC 能写入它 → 批量审批 UI 入口缺失**。

🔴 **模型切换安全（TODO2 未做）**（face_cluster.rs:22-26）：`set_active_face_model` 不存在，切换整体推迟。隐患：① `persons` 无 model_name → 切换（128→512 维）后旧质心维度不符，`cosine_similarity` 的 `debug_assert` debug 下 panic、release 下算错；② `faces` 有 model_name 可隔离但 persons 没有；③ 切换时 persons 旧质心处置未设计；④ `reset_face_data` 全量删 persons 会丢其他模型标注。**CLIP 侧对比**：`set_active_model` 已实现 + `sync_ai_status_to_model` 重同步——人脸侧无对应。

### 2.3 exotic worker 框架（作插件化地基）

**协议**（[crates/exotic-protocol/src/frame.rs](../../crates/exotic-protocol/src/frame.rs):7-188 / message.rs）：唯一帧 = 24B 定长头（magic"EXOT"+protocol_version u16+frame_type u16+request_id u64+json_len u32+blob_len u32）+ JSON + blob。`PROTOCOL_VERSION=1`、`MAX_JSON=1MiB`、`MAX_BLOB=64MiB`（先读头校验再分配，防内存炸弹）。帧类型：Hello/Ready(握手)/Request/Success/Failure/Shutdown。stdout 只走帧、日志走 stderr。`WorkerConn`：`Box<dyn Write>` + crossbeam `Receiver<Frame>`（独立读帧线程）。握手校验 protocol_version + worker_id + required_capabilities 全覆盖。spawn：`Command` stdio piped，Win 加 `BELOW_NORMAL_PRIORITY_CLASS|CREATE_NO_WINDOW`，🔴 **mac/Linux 未接低优先级**（worker.rs:110-113 注释「Part4 接」）。

**Supervisor**（[supervisor.rs](../../src-tauri/src/exotic/supervisor.rs)）：持 Child+WorkerConn+stderr 环形缓冲(64KiB)+2 线程+alive。握手失败立即 kill+reap。run 后 TimedOut/Disconnected/Protocol → `kill_and_reap`(幂等) 标 alive=false。shutdown 发 Shutdown 帧→宽限→超时 kill→join。Drop 兜底防孤儿。**每 Supervisor 同时只一个请求（串行）**，并发靠进程池 `MAX_POOL=2`。**无健康心跳**，仅任务完成后 `is_alive()`。崩溃退避 sleep 500ms。

**Coordinator**（[coordinator.rs](../../src-tauri/src/exotic/coordinator.rs)）：单串行循环（tokio），mpsc(32)+dirty AtomicBool 合并 wake。7 种 WakeReason；`maybe_run_until_drained` 跑到无就绪任务。**启动前完整性复核** `resolve_worker_path`(验签 manifest + 全文件 hash)，失败 fail-closed 不拉起。gate：worker 可用→enabled→not_paused→auto→runnable→has_ready。🔴 **无 GPU 互斥机制**（零 GPU 感知），仅 `BackgroundHeavyLimiter`(CPU/IO)。🔴 **硬编码单插件 `PSD_PLUGIN_ID` + 单 capability `Thumbnail`**。

**Pipeline 派发**（[pipeline.rs](../../src-tauri/src/exotic/pipeline.rs)）：`spawn_blocking` + `std::thread::scope` 4 线程 Claimer+Worker池×2+Writer/Sink+续租。原子领取 `claim_exotic_tasks`(UPDATE RETURNING)、`finalize_success`(write+flush+sync+rename 原子落盘)、strike≥5 熔断、租约 LEASE_TTL=120s + 续租 40s、孤儿恢复。

🔴 **承接 ai-worker/face-worker 的 6 大缺口**（worker.rs/message.rs/limiter.rs/coordinator.rs）：

| # | 缺口 | 现状 | 证据 |
|---|------|------|------|
| G1 | **无长驻 session 握手** | 只有单次 Request→Response，无 ModelLoad/SessionReady；ORT 加载数秒，握手 timeout 5s 可能不足 | message.rs:33-47 / coordinator.rs:39 |
| G2 | **无 GPU 协调** | `BackgroundHeavyLimiter` 纯 CPU/IO 计数器，零 GPU 感知；MAX_POOL=2 并行持 GPU 无显存预算 | limiter.rs:35-39 |
| G3 | **无 model 大文件分发** | PackageManifest 只 kind=worker/file，无 kind=model/分片；ViT-L fp32 ~1GB 不适合单 zip | installer.rs:37-43/354 |
| G4 | **无批量/流式** | 强制 1:1 request_id；CLIP 批推理/face「一批入多结果」无法流式 | worker.rs:277-278 |
| G5 | **Coordinator 硬编码单插件** | PSD_PLUGIN_ID/单 capability，无通用多插件调度 | coordinator.rs:30-31 |
| G6 | **错误码无 AI 专用** | WorkerErrorCode 仅 5 值，无 OrtError/GpuOom/ModelNotLoaded（JSON 枚举扩展兼容可控） | message.rs:83-118 |

**能力协商**（worker.rs:185-195 / catalog.rs:47-65 / installer.rs:183-224）：握手 capabilities 字符串列表；`CatalogOffering.capabilities=Vec<Capability>`(Thumbnail/Metadata/Text) 写 `exotic_tasks.capability` 作路由键；安装 `check_catalog_subset` 验 manifest 声明 ⊆ Catalog。⚠️ `RequestBody::Metadata` 已定义但 **Pipeline 未实现路由**（仅 Thumbnail 有 claimer_loop）。

### 2.4 去 ort 改造面 + AES + GPU 协调 + 合规

**去 ort 依赖面**（[Cargo.toml](../../src-tauri/Cargo.toml):95/108 + [tauri.conf.json](../../src-tauri/tauri.conf.json):68-71）：

- `ort = "2.0.0-rc.12"` features `load-dynamic/directml/download-binaries/copy-dylibs`（Cargo.toml:95）。
- `tokenizers = "0.21"`（:108，静态链接，仅 clip.rs BERT 分词用）。
- **4 个 DLL 打 bundle**：`onnxruntime.dll`/`DirectML.dll`/`dxcompiler.dll`/`dxil.dll`（来自 `node_modules/onnxruntime-node/...`，tauri.conf.json:67-72；体积 DirectML.dll 为大头、须 stat 实测——作者注释 Cargo.toml:94 称 ort 增「~20MB」、方案5 不 bundle「重回 5MB」）。
- **直接 `use ort::` 仅 3 处**：clip.rs:173、engine.rs:78-79、face.rs:25；+ `error.rs:58 Ai(#[from] ort::Error)` + `ai_commands.rs:149 ort::init()`。
- 🔑 **可剥离 vs 残留边界**：外置后**推理核心**（engine/clip/face/pipeline）随 ort 移出 worker；主二进制**残留**「元数据 + 下载 + DB 层」——`AiEnginePool` 定义、AI 配置字段、ai_commands 下载/进度命令、remote_registry 发现、profile 结构。即 worker 化是「把推理核心搬走，控制面留主进程」。

🔴 **AES 加密权重——完全不存在**（Cargo.toml:61/114-117 + [exotic/crypto.rs](../../src-tauri/src/exotic/crypto.rs):1-50）：

- 全仓 src/ **无任何 AES/GCM/encrypt/decrypt**。`exotic/crypto.rs` 纯 Ed25519（ring 验签），无对称加密。zip 依赖注释「去掉 bzip2/zstd/aes」。
- 模型权重**当前明文 ONNX** 存储 + 下载（仅 sha256 完整性，无加密）。
- ✅ **ring 已在依赖**（:117，验签用），`ring::aead` 支持 AES-256-GCM 但**当前未用** → 可同 crate 扩展、无需新依赖（符合 memory `gotcha-offline-cargo`「不新增未缓存 crate」），但**密钥分发方案须结合 license 派生**（硬编码密钥无意义）。

🔴 **GPU 协调——双层都缺**（engine.rs:238-320）：
- AI 进程内：CLIP `pool_size=1`（DX12 锁争用）、face `pool_size=2`（注释「明知风险」）；CLIP 与 face 分属独立 `SessionPool`，**无跨子系统 GPU 互斥**（软门闩 `gpu_analysis_owner` 是进程内的）。
- worker 框架侧：§2.3 G2，零 GPU 感知。
- → **外置成多进程后软门闩失效（跨进程），worker 框架又无 GPU 令牌 → 必须新建跨进程 GPU 协调**（Part0 §5.5 决策 4）。

**合规隔离现状**（face_profile.rs:182-197 + face.rs:253-400）：
- SCRFD/ArcFace **代码完整但 dead code**：`scrfd-arcface-r50` profile（`scrfd_10g_bnkps.onnx`/`w600k_r50.onnx`，commercial_ok=false，assets 空），`detect_scrfd` 实现 + UNVERIFIED。**无任何 bundle 清单引用这两个 onnx**（当前安全）。
- 🔴 **但代码 + 路径常量仍在主二进制** → 需 `#[cfg(feature="face-noncommercial")]` 隔离 + CI 断言不打包（不能只靠「不可达」）。
- CLIP 来源 `gficcg/clip_cn_vit-onnx`（remote_registry.rs:27/117/155，主动发现源，无官方背书，离线/企业需自建镜像）；vocab.txt 独立来自 `OFA-Sys/chinese-clip-vit-base-patch16`(MIT)。

### 2.5 现状问题清单（编号 → §3 设计对应）

| # | 问题 | 证据 | 对应设计 |
|---|------|------|---------|
| A1 | 推理进程内同步阻塞、Session !Send、与 AppState 深耦合 → 外置需进程边界（**数据面=EmbedBatch 传 cache_keys、worker 读 ai_cache 自解码，§8.1 决策1；非 tensor 序列化**） | engine.rs:238/406 / search.rs:144 | §3.1 |
| A2 | ort + tokenizers + 4 DLL(~160MB) 在核心 → 去 ort | Cargo.toml:95/108 / tauri.conf:68-71 | §3.2 |
| A3 | 文本编码器固定 CPU（DirectML BERT 算错），图/文双 EP 须保持分离 | engine.rs:267-280 | §3.3 |
| A4 | 向量暴力 O(N×D)、1GB 常驻、百万级 ~1s+ | search.rs:144-157 | §3.4 |
| A5 | 模型库动态发现已成熟（架构→batch 变体），但 10min 缓存非持久、双仓库扁平共存维护成本 | remote_registry.rs:33 / profile.rs:99 | §3.9 |
| F1 | SCRFD/ArcFace 死代码但代码+路径在主二进制 → cfg 隔离 + CI 断言 | face.rs:253-400 / face_profile.rs:182 | §3.10 |
| F2 | `list_face_model_registry` 无渠道过滤、暴露 commercial_ok=false | face_commands.rs:363 | §3.10 |
| F3 | 批量审批缺命令（is_confirmed 无写入入口） | face_commands.rs / schema.rs:433 | §3.5 |
| F4 | ~~模型切换 TODO2 未做~~〔✅ 2026-07-02 `fa0e951`:persons.model_name 隔离 + cosine 维度护栏(不匹配→0.0 不 panic)+ gated 切换〕 | face_cluster.rs / schema.rs | §3.5 |
| F5 | `face_variant_installed` 无 sha256/size 校验（默认轨有，不对等） | face_commands.rs:38-39 | §3.5 §3.10 |
| W1 | 长驻 session 无握手阶段（G1） | message.rs:33-47 | §3.1 |
| W2 | worker 框架无 GPU 协调（G2） | limiter.rs:35-39 | §3.6 |
| W3 | 无 model 大文件分发（G3） | installer.rs:37-43 | §3.7 |
| W4 | 无批量/流式协议（G4） | worker.rs:277-278 | §3.1 |
| W5 | Coordinator 硬编码单插件（G5） | coordinator.rs:30-31 | §3.1 |
| W6 | 错误码无 AI 专用（G6） | message.rs:83-118 | §3.1 |
| W7 | mac/Linux worker 未接低优先级 | worker.rs:110-113 | §3.1（让步） |
| C1 | AES 加密权重完全不存在（ring 可扩展，密钥须 license 派生） | crypto.rs / Cargo.toml:61 | §3.7 |
| C2 | GPU 跨进程协调缺失（软门闩进程内失效） | engine.rs:238-320 | §3.6 |
| C3 | CLIP 第三方仓库依赖（gficcg 无背书） | remote_registry.rs:27 | §3.9 §3.10 |

---

## §3 设计方案

> 原则：**控制面/数据面分离**——下载/profile/DB/状态机/让步留主进程，推理（Session/VRAM）进 worker。**协议/框架原语扩展归 Part6，AI/face 语义归本 Part**（§2.3 的 G1-G6 是两 Part 接缝）。三红线（合规/AES/诚实）贯穿。

### 3.1 worker 化架构 + 协议扩展（A1 + W1-W7，核心）

**3.1.1 进程拓扑**：`ai-worker`(ort+CLIP 图/文双 EP) + `face-worker`(ort+YuNet+SFace)；合并 vs 分离待 §3.11 拍板。worker 复用 exotic `WorkerConn/Supervisor/exotic-protocol`，但**生命周期是「长驻推理 session」非「短任务池」**：加载模型后常驻处理多批，空闲超时卸载释放 VRAM（对齐现状 pipeline.rs:89-152 自然完成 drop）。

**3.1.2 协议扩展（Part6 加帧原语，Part4 定 AI 语义）**——对接 6 缺口：

> 🔴 **第 8 轮核验 P0-3（真 P0，卡 worker 化、须先定稿）**：下表协议须补三处——① 单 `model_handle` 不足承载多模型（face YuNet+SFace / CLIP 图文双权重）→ 改 `model_handles: role→handle`（或单一共享内存容器+明确 offsets）；② `EmbedBatch` 读 `ai_cache_dir/{2hex}/{hex}.webp` 但 `ai_cache_dir` 不在任何协议字段 → 经 `SessionInit` 传受限缓存根句柄；③ `fingerprint` 单值 + 回程 `Success{embeddings[]}` → 改**逐项**对齐结构（每项 fingerprint/error + 强制 `results.len()==items.len()` 校验 + 拒陈旧结果）。**协议定稿见 Part6 §3.2.1a**（`model_handles: Vec<(ModelRole, ModelHandle)>` / `ai_cache_dir` / `EmbedItem`+`EmbedResult`），亦录 review_2026-06-28 §六。

| 缺口 | 扩展 | 落点 |
|------|------|------|
| G1 长驻 session | 新消息 `Request::SessionInit{session_id, model_handle, image_provider, model_profile: ModelProfileSnapshot{arch_id, image_file, text_file, batch_size}}` → `Response::SessionReady{caps, embed_dim}`（🔴 字段以 Part6 §8.1 决策2 + §8.3 为准：`model_handle`(OS handle/fd,非 model_paths)〔🔴 T10 修正(2026-07-03):落地为两级 `ModelHandle::{Path,Named}`——明文传路径、AES 走**具名** shm,非裸 OS handle/fd(D1;匿名句柄跨 exec 无效,§3.7.3);字段全量以 Part6 §3.2.1a×D1×D3 合并落地稿(exotic-protocol/message.rs)为准〕、`image_provider`(文本 EP 由 worker 内硬编码 CPU)、`model_profile`(统一 batch_capacity/profile)）；**两段握手**：进程握手快(5s)、session init 慢(异步等就绪，ORT 加载数秒~分钟) | exotic-protocol(Part6) + ai-worker |
| G4 批量 | **一 Request = 一批**：`Request::EmbedBatch{item_ids[], cache_keys[]}` → `Success{embeddings[]}`〔🔴 T10 落地:逐项化 `EmbedBatch{items: Vec<EmbedItem{item_id,cache_key,fingerprint}>}`,响应逐项 Ok/Err 不连坐、嵌入走同帧 blob(Part6 §3.2.1a + T10 修正注)〕（🔴 传 cache_keys 非 tensor blob，§8.1 决策1；worker 读 `ai_cache_dir/{2hex}/{hex}.webp` 自解码）；CLIP 已固定 batch，批即天然单位，**不需真流式**。**blob≈0 → 无 MAX_BLOB 压力**（旧「86MB tensor / 限批 ≤128 / 调高 MAX_BLOB」方案 A 作废）；batch_size 仅受 GPU 显存约束〔🔴 「worker 侧 GpuLimiter」随 §3.6 被 D2 推翻:令牌在 **host 侧**(exotic/limiter.rs GpuToken),worker 无 GPU 感知〕 | message.rs / worker.rs |
| G5 多 capability | 新 `Capability::Embedding`(CLIP) / `FaceDetectEmbed`(人脸)；Coordinator 通用化（去 PSD 硬编码，按 plugin_id+capability 路由） | catalog.rs / coordinator.rs(Part6) |
| G6 错误码 | `WorkerErrorCode` 加 `GpuUnavailable/SessionExpired/ModelLoadFailed/EmbedDimMismatch`（🔴 对齐 Part6 §3.2.2 canonical；初稿 OrtError→ModelLoadFailed、GpuOom→GpuUnavailable、ModelNotLoaded→ModelLoadFailed，§8.6） | message.rs |
| W7 低优先级 | mac/Linux 接 `apply_low_priority`（`setpriority`/nice），补 worker.rs:110-113 空实现 | worker.rs |

> ✅ **T17 补遗(2026-07-03):v2 增补 `EncodeText{texts}` op(additive,不动 PROTOCOL_VERSION)**——上表 op 家族(T10 三源合并)漏列文本编码:T16 删主进程 ort+tokenizers 后语义搜索查询向量无处生成,文本塔只在 worker(EP 恒 CPU,§8.6)。响应 = Success 帧 + `SuccessBody.text_embed`(count 双向核对),向量走同帧 blob(texts 序连续,每项 embed_dim×f32 LE);**全批原子**(文本编码无逐项 IO 失败模式);不新增 capability(与图像塔同属 embedding 会话)。字段权威 = Part6 §3.2.1a 的 T17 增补注。

**3.1.3 控制面留主进程**：profile 解析、模型下载/校验、DB（ai_embeddings/faces/persons/状态机）、`ai_yield_blockers` 让步、续传（reset_processing）全留主进程；主进程持 channel/pid，worker 持 Session/VRAM。〔✅ T17 已落地(2026-07-03):AiEnginePool 保留为进程内双活半边,worker 半边=AppState.ai_worker(AiWorkerClient);让步/续传/状态机经 produce_tasks/write_results 与进程内共用同一套代码〕
🔁 **图像传输决策反转（Part6 联动 review §8.1 决策1，覆盖初稿「主进程解码发 tensor」）**：改为 **worker 读受限 ai_cache 自解码**——`EmbedBatch` 传 `cache_keys`，worker 拼 `ai_cache_dir/{2hex}/{hex}.webp` 读 WebP 解码（worker 含 image crate，轻）。依据：① 顺应现有 exotic「worker 读 source path 解码」框架（psd-worker 同）；② 避免 86MB+ tensor blob 超 MAX_BLOB 的限批/调高难题；③ ai_cache 非敏感（AES 只护**模型权重**，图像可由 worker 读受限目录）。**「数据面分离」精确化**：模型权重 AES 严格隔离（§3.7/§8.2.1 决策2 匿名内存映射，机制细化为具名 shm 传 name，见 §3.7.3）+ 图像走 worker 读受限 ai_cache。详见 §8.6。

**3.1.4 ⚠️ 边界诚实**：方案 B（worker 读 ai_cache 自解码）下**数据面 blob≈0**（只传 cache_keys 字符串），无大 tensor 跨进程开销——旧稿「86MB blob 跨进程 + mmap 优化」**已作废**（§8.6 决策1）。worker 化净收益：<10MB 核心 + 崩溃隔离 + 解 Session !Send；新增仅 worker 读 ai_cache 的磁盘 IO（解码本就必需，无额外成本）。

### 3.2 去 ort（A2，<10MB 关键一步）

- **移除依赖**：Cargo.toml 删 `ort`(:95) + `tokenizers`(:108)；二者随推理核心移入 `ai-worker` crate。
- **删 bundle DLL**（🔴 执行归 **Part7 T6** 打包资源层，本 §3.2 仅说明、Part4 T16 不碰 tauri.conf）：tauri.conf.json:67-72 删 4 个 ONNX DLL（onnxruntime/DirectML/dxcompiler/dxil）。〔🟢 口径更新(2026-07-10 审查 D3):当前 tauri.conf.json 已无任何 onnxruntime/DirectML/dxcompiler/dxil 条目(grep 零命中)——此项**已成事实**,Part7 T6 无须再执行删除,勿按本行重复操作。〕⚠️ **体积须 stat 实测、勿把估计当数字**：作者注释 Cargo.toml:94 称 ort `download-binaries` 增「~20MB」、方案5「不 bundle DLL 重回 5MB」（:97-99）；DirectML.dll 通常是大头（社区常见 >100MB，取决版本）。无论精确值，去 bundle DLL 是 <10MB 核心必要项。
- 🔎 **利用既有预研**（Cargo.toml:94/97-99）：作者已留「方案5 = 不 bundle DLL、用系统库 `ORT_DYLIB_PATH`」注释。但 **Windows 无系统默认 onnxruntime/DirectML.dll**，「系统自带」不可靠 → **正解是 ai-worker 自带 DLL**（worker 化天然解决：DLL 进插件包、不进核心 bundle）。
- **拆 ort 类型耦合**：`error.rs:58 Ai(#[from] ort::Error)` 移除（改为 worker IPC 错误码映射）；clip.rs:173/engine.rs:78-79/face.rs:25/ai_commands.rs:149 随推理核心迁出。
- **残留控制面**（不动）：AiEnginePool 改为「worker 句柄」、AI 配置/下载/profile/remote_registry 留主进程。
- 🔴 **过渡双活安全网（terminal review 关切 5：去 ort 是全盘最大「全有或全无」架构切换）**：worker 化（T15）与去 ort（T16）之间须有**进程内 ort 与 worker 短期并存**的中间态，**不可一步删 ort**——否则 worker 集成若在后期失败，核心无 in-process 回退、整个 AI/人脸子系统不可用。落地：进程内推理路径（engine/clip/face）过渡期置于 `#[cfg(feature="ai-inproc")]`（默认开），`ai-worker` 新路径并存（运行时探测或 config 切换）；**先令两路并存 + worker 端到端跑通（含 T14 `commit_from_memory` 实跑对拍 gate）+ 灰度一版**，确认 worker 稳定后再执行 T16 删 ort + 移除 `ai-inproc` 死代码。**优先级排序见 Part0 §11.4.1**(worker 化整段属 v0.1 后 fast-follow)。**✅ 本安全网已按计划走完并收束(2026-07-03)**:双活以 config `ai_backend` 切换实现(未用 cfg feature,形态等价);e2e 程序化对拍+用户 GUI 三测(T18.5/b/c 性能修复后 14s)确认 worker 稳定 → T16 S2-S4 删进程内路径与 ort 依赖(`3d67321`/`7acccd5`),黄金向量(S3,`ef17d42`)承接对拍职责。
- **验收**：核心 `cargo build` 依赖树无 ort/tokenizers；bundle 无 ONNX DLL；体积大降。

### 3.3 CLIP 中文双档 + 文本 CPU（A3）

- **worker 内保持图/文双 EP**（A3 红线）：图像编码器走最优 EP（DirectML/CUDA/CoreML），**文本编码器固定 CPU**（DirectML BERT int64 Gather 静默算错，进程隔离后约束不变）。两 Session 不同 EP 同 worker 进程并存。
- **双档切换**：架构 id（向量空间主键）+ 变体（图像 onnx）由主进程 profile 控制；切换 = 主进程发新 `SessionInit` → worker reload。**架构 id 变 = 向量空间变** → 须配合 §3.5 同样的「切换迁移」语义（ai_status 重置 + 重嵌入，CLIP 侧 `sync_ai_status_to_model` 已有，保留）。
- 默认 ViT-B/16 fp16（轻）、可选 ViT-L/14-336 fp32 HQ（§3.11 默认档位拍板）。

### 3.4 ANN 检索（A4，接 Part1 VectorStore）

> 向量检索**留主进程**（纯向量运算，不需 worker；worker 只产 embedding）。落地 Part1 的 `VectorStore` trait + 两阶段 ANN。

- **写入路径**：worker 回 embeddings → 主进程经 `VectorStore::upsert`（替代直写 ai_embeddings BLOB，Part1 定义 trait）。
- **检索路径**：替换 search.rs:144-157 的 rayon 暴力 O(N×D)：
  - **小库（<500K）**：保留 f16 常驻缓存 + simsimd 加速点积（仍精确，快路径）。
  - **大库（≥500K，Part1 §3.4 阈值）**：sqlite-vec / usearch ANN 索引（两阶段：ANN 粗筛 top-K' → 精排 top-K）。
- **embed_dim 随架构变**（512/768/1024）→ VectorStore 按 `model_name`(架构 id) 分区，跨架构不混（与 faces.model_name 同理）。
- ⚠️ **离线/索引一致性**：ANN 索引随 embedding 增量更新；架构切换或重嵌入时索引重建（§3.5 迁移语义复用）。
- 选型细节/trait 定义见 Part1 §3.4，本 Part 只接推理侧写入 + 检索调用。

### 3.5 人脸批量审批 + 模型切换安全（F3 + F4 + F5）

**3.5.1 批量审批命令补全（F3）**：`is_confirmed` 字段已就绪、recluster 已消费，缺写入入口。补：

- `confirm_face_assignment(face_ids[])`：置 `is_confirmed=1`（锁定，recluster 不再移动）。
- `reassign_face_to_person(face_ids[], person_id)`：改 person_id + `is_confirmed=1`（用户手动纠正聚类错误）。
- `unassign_face(face_ids[])`：person_id=NULL（误检/非人脸移出）。
- 🔴 `reject_face_candidate(face_ids[], person_id)`：记「这些脸**不是**该 person」负样本 → 写 `face_rejections(face_id, person_id)`（Part1 V10，`PRIMARY KEY(face_id, person_id)` + FK `ON DELETE CASCADE`）；**recluster 跳过已拒绝对**，防质心相近反复误聚（DBX-01）。Part5 §3.6 `rejectCandidate` 的唯一后端入口；须随本节一并加入 `lib.rs` `generate_handler!`（IPC-1）。〔第 7 轮终审：从 §8.4 留痕回写本正文，关闭 DBX-01 回写缺口。〕
- `create_person_from_faces(face_ids[], name?)`：新建 person + 绑定（从 likely_match 一键建人）。
- 🔴 `list_likely_matches(filter?: {person_id?, limit?}) -> Vec<LikelyMatchGroup>`：**查询命令**——按候选 person 分组返回未确认脸（`is_confirmed=0` 的聚类候选）。**Part5 §3.6.2 / T10 审批 UI 的唯一查询入口，硬依赖此命令**（此前漏定义：全仓无、§3.5.1 初稿只列 4 写命令）。`LikelyMatchGroup{person_id, person_name?, candidate_faces: Vec<FaceThumb>, confidence}`。
- 配合 Part5 的「grouped likely matches」批量审批 UI（确认/拒绝/合并）。全部单事务、`AppError`。

**3.5.1a 🔴 审批后 persons 派生重算 + is_confirmed 语义 + 同模型守卫（P1-6，第 8 轮核验）**：§3.5.1 命令仅写 faces 行（person_id/is_confirmed），但改了 person 归属 → 受影响 person 的派生字段必须连带重算，否则 `persons.centroid`/`cover_face_id`/`face_count` 陈旧、UI 计数错乱。

- **受影响 person 重算**：`reassign`（src 失一脸、dst 得一脸）/`unassign`（失一脸）后，对**所有受影响 person**调 `recompute_person_aggregates(person_id)`：重算加权质心 + `cover_face_id`（取最高质量脸，face_cluster.rs:327-335）+ `face_count`。范本=现有 `merge_face_persons`(queries.rs:2335-2415，合并后显式质心加权重算 + 写回 face_count，§2.2)。person 归零按 recluster 同策略删/留。
- **`unassign` 的 `is_confirmed` 语义（堵 recluster 再吸附）**：`unassign_face` 置 `person_id=NULL` 时**必须同时清 `is_confirmed=0`**——否则残留 `is_confirmed=1 + person_id=NULL`，下次 recluster 的 is_pinned 闭包(face_cluster.rs:307-312)把「confirmed 但无 person」当 free 自由吸附，等于自动撤销用户的 unassign。
- **同 model_name 强守卫**：`reassign_face_to_person` 写路径前置断言 `face.model_name == person.model_name`（§3.5.2 双过滤）——跨模型 reassign 会把 128 维脸塞进 512 维 person → 质心重算 panic / 语义错乱；不符则拒（`AppError`）。
- **事务边界**：faces 写 + persons 派生重算**同一事务**（§3.5.1「全部单事务」明确含重算，避免半重算致计数与质心不一致）。

**3.5.2 模型切换安全（F4，TODO2 落地）**——`persons` 单模型假设是核心债：

- **方案：persons 按 model_name 隔离**（Part1 加 `persons.model_name` 列 + 迁移）。切换轨 = 切到该 model_name 的 persons 名册；旧轨 persons **保留不删**（不同模型嵌入不可混，但标注是用户资产，不能丢）。
- `set_active_face_model(track_id)` 新命令：写 `face_model_active` → 触发 `sync_face_status_to_model`（仿 CLIP `sync_ai_status_to_model`）：该 model_name 下 `face_status` 已有 faces 标 Done、其余 Pending（重跑 pipeline 产新 model_name 的 faces + persons）。
- **维度安全**：`cosine_similarity` 前校验 `a.len()==b.len()`，跨 model_name 查询天然隔离（faces.model_name + persons.model_name 双过滤）→ 杜绝 128/512 维混算 panic。
- `reset_face_data` 改为**按 model_name 重置**（不再全删 persons，保其他模型标注，F4 隐患④）。
- ⚠️ **SCRFD 轨激活前置**：仅当 §3.10 合规门 + InsightFace 对拍（F4/D1 的 UNVERIFIED）通过才允许切到 scrfd-arcface-r50；默认轨切换不受限。

**3.5.3 变体安装校验对等（F5）**：🟢 已按 **2026-07-06 T5 选项 B 裁决**收窄(原文「补 sha256+size 校验(与默认轨对等)」被推翻,按原文施工会把数十 MB 模型的常驻 sha256 启动税加回):`face_variant_installed` 只补 **size 快检**(清单载有期望 size 时不符即判未安装,专抓手动放入 LFS pointer 文本/截断文件),**全量 sha256 刻意不进判定路径**——官方下载路径落盘前已做 size+sha256 全校验,存在≈已校验。现行实现与裁决现场注释见 face_commands.rs `face_variant_installed`。(2026-07-10 审查 D2 回写)

### 3.6 GPU 跨进程协调（W2 + C2，Part0 §5.5 决策 4）

> 🔴 **本段已被 D2 裁决（2026-07-02）推翻,按本段正文施工会重建已否决的方案**。现行方案见 §8.2.2 修正注与 T11 任务行:**host 侧 GpuToken**——令牌全留主进程、**每批推理 acquire/RAII 释放**(非会话生命周期)、协议零扩展、额度恒 1(落地 exotic/limiter.rs + AppState.gpu_token;worker_pipeline 派发批内 CPU permit→GPU token,D2 顺序)。「会话级令牌 + Coordinator/协议帧协调 + worker 侧 GpuLimiter」整套构想作废;两 session 同驻显存的 OOM 由 **gpu_analysis_owner 进程内互斥(CLIP↔face 单持有者)+ 合并单 worker(T15/T20)** 共同防住,不需要跨进程机制。下文保留仅作历史设计记录。(2026-07-10 审查 D1 回写)

> 外置成多进程后，进程内软门闩 `gpu_analysis_owner` 失效（跨进程）；worker 框架零 GPU 感知 → **新建跨进程 GPU 令牌**。

- **GpuLimiter（主进程持有，Part6 框架层）**：一个「GPU session 令牌」信号量（默认额度 1 = 同时只一个 AI worker 持 GPU，Part0 §5.5 决策 4）。🔴 **令牌覆盖会话生命周期（P0-2 回写，第 6 轮独立核验）**：worker 在 **`SessionInit`（加载模型权重入显存）之前**即经 Coordinator 申请令牌，**持有至 session 卸载 / 空闲超时 / 进程退出才释放**（RAII，Supervisor 在 worker 崩溃后回收令牌）——**而非每批推理 acquire/release**。额度 1 因此既串行化推理、**又物理阻止两 worker 同时把 session 驻留显存**（若仅门控推理、不门控 SessionInit/VRAM 分配，则两 session 仍可同时驻留 → OOM；故令牌必须前置到 SessionInit）。
- **令牌编入 worker 调度**：ai-worker 与 face-worker 抢同一 GPU 令牌（互斥）；CPU-only worker（如文本编码器若独立）不占令牌。
- **与现有 limiter 分层**：`BackgroundHeavyLimiter`(CPU/IO) 不变；新增 `GpuLimiter` 正交（GPU 任务同时受两者约束）。
- **显存预算（可选进阶）**：令牌可带显存额度（按 VRAM 自动档，对齐 search 现有 2G/4G/8G/12G 档），多小 worker 并行时按预算放行。P2。
- ⚠️ **合并 ai-worker 则 GPU 协调简化**（§3.11）：单 worker 内 CLIP/face 共享一个 Session 调度，GPU 互斥退化为进程内（更简单）；分离则必须跨进程令牌。**这是 §3.11 合并 vs 分离的关键权衡输入**。

### 3.7 模型 blob 分离下载 + AES 加密 + license 派生密钥（W3 + C1，Part0 §5.5 决策 3 + §8 层1）

**3.7.1 model blob 分离（W3）**：`PackageManifest` 加 `kind=model`（区别 worker 可执行）；`RegistryEntry.model_blobs[]`(url/sha256/size/kind=model_weight)（Part6 框架）。大模型（ViT-L fp32 ~1GB）独立分步下载，**豁免压缩比检查**（ONNX 近 1:1），避免单 zip 超 `InstallLimits.max_total_size=512MB`。〔✅ T12 已落地(2026-07-03):`RegistryEntry.model_blobs[]{url,sha256,size,kind,file_name}`(file_name 为 T12 补充——§3.7.1 四字段无落盘名则无法确定性落盘;白名单单路径分量+8GiB 单文件上限)+ `fetch_model_blob`(幂等跳过/`.part` 断点续传/LargeFile 超时档)+ 安装命令 1.5 步**先 blob 后 zip**(失败零清理面,就位权重为内容寻址无害产物、重试复用不回滚)。「豁免压缩比」以 **blob 不进 zip** 的方式天然成立(这正是分步下载的动机),InstallLimits 零改动〕

**3.7.2 AES 加密权重（C1，从零新建，防白嫖层1）**：

- **算法**：`ring::aead` AES-256-GCM（ring 已在依赖，无新 crate，符合 `gotcha-offline-cargo`）。
- **加密时机**：分发的 ONNX 是密文；安装/首次加载时解密到内存喂 `Session::builder`（不落明文盘，或落盘后即时加密缓存）。
- 🔑 **密钥 license 派生**（核心，硬编码无意义）：`enc_seed` 进 license token（Part0 §8）；`key = HKDF-SHA256(ikm=enc_seed, salt=per-model 随机 32B 存 ModelBlob.enc_salt, info=plugin_id||model_id)`（ring::hkdf）；密文 `nonce(12B)||ct||tag(16B)`，nonce 每次分发刷新（对齐 §8.6 / Part6 §8.5 / Part8 §3.2）。**无 license → 下到密文无法解 → 白嫖无效**。
  - 🔴 **enc_seed 粒度 = 按 plugin_id 固定（插件主种子，非 per-license）**：同插件所有 token 复用同一 enc_seed → CDN 每插件一份密文可被所有持证用户解密（per-license 唯一会致密文无法复用、存储爆炸）。权威定义 Part8 §3.2.2 / Part6 §3.7.2。
  - 🔴 **DerivedSecret 双门控**（消费侧信任、不重复实现）：解密/门控由 Part6 §3.7 主进程负责（`activate()` 写 `plugin_id→DerivedSecret`，`is_task_runnable` 对 AES 插件额外查），Part4 worker 只信任此门控（patch bool 拿不到密钥）。
- **商业边界**：AES 解密与密钥派生随第一方源码公开（Part0 §10）；真正不可得的是 `enc_seed`（在 license token 内、由签发端持有）与私钥凭证——**无 license 即无密钥，下到密文也解不开**。授权判定经 Part6 `EntitlementProvider`，未授权 fail-closed（恒 `Unlicensed`）。
- ⚠️ **诚实边界**（Part0 §7）：CLIP/YuNet/SFace 权重来自公开仓库，加密挡懒人 + 为私有/微调模型铺路，**对公开权重不形成强护城河**（付费用户内存可 dump 解密后权重）。文案据此，不夸大「独家模型」。

### 3.7.3 模型 handle 跨进程传递（决策2 落地，🔴 第 5 轮复审补：原方案缺 OS 级机制）

决策2（§8.2.1）定：主进程解密 → 写**匿名内存映射** → `SessionInit` 传 handle/fd → worker `mmap` → `commit_from_memory`。但 worker 是 `Command::spawn` 出的**独立 exec 进程（非 fork）**（[worker.rs:90-107](../../src-tauri/src/exotic/worker.rs#L90) 现不设句柄继承），**匿名映射的 handle/fd 数值在子进程无效**——必须显式跨进程传递，否则 1.7GB 模型加载失败。补 OS 级机制：

- **Windows**：`CreateFileMapping`（匿名）句柄进程局部。spawn 子进程后用 `DuplicateHandle(GetCurrentProcess(), hMap, hChildProcess, &dup, 0, FALSE, DUPLICATE_SAME_ACCESS)` 复制到子进程，把 `dup` 数值写进 `SessionInit.model_handle`；worker 端 `MapViewOfFile` 该 handle。**或更简**：改用**具名**映射（`CreateFileMapping` 带随机 name），`SessionInit` 传 name 字符串，worker `OpenFileMapping(name)`——免 DuplicateHandle + 免子进程句柄获取（推荐）。
- **Linux**：`memfd_create` 的 fd **不随 `exec` 继承**。两路：① 清除 `FD_CLOEXEC` 让 fd 跨 exec 存活、传 `/proc/self/fd/{n}` 路径让 worker `open`（简单）；② Unix domain socket + `SCM_RIGHTS` cmsg 传 fd（标准但繁）。
- **macOS**：无 `memfd`，用 `shm_open(name, ...)`（具名 POSIX 共享内存），`SessionInit` 传 name，worker `shm_open` 同 name `mmap`——与 Windows 具名方案对称（**统一推荐"具名共享内存传 name 字符串"**，跨三平台最简、避免句柄继承复杂度）。
- **改造点**：`spawn_worker_process` 对 `uses_model` 插件按上述选定方案准备（具名方案无须改 spawn 继承位）；`SessionInitBody.model_handle` 类型由"裸数值"明确为 `ModelHandle{ Named(String) | Fd(i32) | Win32Handle(u64) }`（具名优先）。明文用完即 `munmap`/`shm_unlink`、主进程 `zeroize`。
- **验收**：worker 成功 mmap 并 `commit_from_memory` 加载 ViT-L 1.7GB；密文/明文均不落盘、不经帧；进程退出后具名对象被 `shm_unlink`/`CloseHandle` 清除（无泄漏）。

**3.7.3a 🔴 具名共享内存安全硬化（P1-7，第 8 轮核验）**：具名 shm 承载**解密后明文权重**，须防同机其它进程窥取（否则 AES 防护被本地绕过）：

- **高熵不可猜名**：`name = picasa-{plugin_id}-{session_id}-{32B CSPRNG hex}`（`ring::rand`），非可预测序号。
- **Windows DACL**：`CreateFileMapping` 传 `SECURITY_ATTRIBUTES`，DACL 仅授当前用户 + 目标 worker 进程（拒其它用户 / 低完整性进程）。
- **POSIX 0600 + O_EXCL**：`shm_open(name, O_CREAT|O_EXCL, 0600)`——`O_EXCL` 防同名抢占（已存在即失败、重生成名），`0600` 仅属主可读写。
- **open 后即 unlink**：worker `shm_open`/`OpenFileMapping` 成功后，主进程立即 `shm_unlink`(POSIX) / 关 name 引用——已映射内存仍有效，但 name 此后不可再 open（杜绝后来者按 name 窥取）。
- **崩溃清理**：主进程/worker 任一崩溃 → Supervisor Drop 兜底 `shm_unlink`/`CloseHandle`（POSIX 具名对象进程死后仍驻留至 unlink，须显式清）；启动扫残留 `picasa-*` shm 清理。
- **生命周期 ownership**：name 由主进程生成 + 持清理责任；worker 只 open + munmap，不 unlink（免双 unlink 竞态）。
- **诚实边界**：对公开权重，付费用户本机仍可内存 dump（§3.7 已述）；本硬化挡的是**同机其它进程/其它用户**窥取，对私有/微调模型有实义。

### 3.8 ai_thumb 喂 CLIP（接 Part3 §3.6）

> ✅ **T18 已落地**(2026-07-03 `893b1d2`):worker 派发路径缺缓存现场派生(`generate_ai_cache`,与派生管线 run_ai_thumb 共用一份实现,CPU permit 配额内串行、失败标 Error 同进程内语义);run_ai_thumb 原直写最终路径违反「派生产物一律原子落盘」红线,同波改 write_atomic(tmp→rename)。触发背景:用户 GUI 首测 `ai_backend=worker` 全部跳过——磁盘 110 个 ai_thumbs 与当前 DB cache_key 仅 1 个匹配(孤儿=旧 mtime 键),库 1554 项覆盖率≈0;本段「无命中回退派生原图」正是为此设计,现已接通。
>
> ✅ **T18.5 性能回归修复**(2026-07-03 `8941eda`):用户 GUI 实测切 worker 后 1550 张分析 10s→5min(重跑同慢)。harness 定位:EP 无辜(双后端 cos=1.0),根因=worker 化丢掉进程内路径的并行结构(host 单派发线程+worker 串行解码/预处理,CPU 段 ~100ms/张 串行硬扛,推理仅 ~6ms/张)。修复:worker 端并行解码+预处理(embed 全批 ≤16 线程/face 分块=4,std::thread::scope 零新依赖)、ai-core 预处理像素循环扁平化(算术逐位不变,对拍 cos 仍 1.000000)、host 现场派生 rayon 并行化。证据=harness 新增 Phase B2 吞吐相位:64 张单批 112ms/张→13ms/张(8.6x),×1550 外推 173s→21s(debug 构建);443×2 测试全绿仅本地。
>
> ✅ **T18.5b/c 批内流水重叠+并行度放开**(2026-07-03 `40138b7`):用户二测 22s 但 GPU 仅 35%/CPU 仅 50%——相位交替(先全解码再全推理)使批耗时=两段之和。改「解码线程池喂有界 channel+推理侧攒子批(≤16)边收边推」=两段取大;解码并行度探测核数不设上限(用户拍板;face 块=4 保留为内存上限,更名 FACE_DECODE_CHUNK)。B2 吞吐 13→8ms/张,×1550 外推 13s;对拍 cos 仍 1.000000。

- **复用 Part3 的 `ai_thumb` 缓存**（短边 336 WebP，`ai_thumbs/{hex}.webp`）：ai_cache 由**派生侧预产**（Part3 §3.6，queries.rs:1935 路径，按文件存在性发现）；**worker 端按 cache_key 读该 WebP 解码喂 CLIP**（§8.1 决策1），无命中回退派生原图产 ai_cache。避免「解原图饿死 GPU」（memory `gotcha-ai-analysis-cpu-decode`：CPU99%/GPU45%）。
- **worker 化后**（§8.1 决策1）：**worker 读受限 ai_cache 自解码**（EmbedBatch 传 cache_keys，worker 拼 `ai_cache_dir/{2hex}/{hex}.webp` 读 WebP）；主进程**不解码、不发 tensor**。
- **档位匹配（统一方案①，review 跨 Part 对齐）**：ai_cache **固定短边 336**（Part3 `AI_CACHE_SHORT_EDGE`，单一 cache 路径 `ai_thumbs/{hex}.webp` 无档位子目录）；ViT-B/16(224) 走 **解码引擎 `ResizeHint::ShortEdge(224)` 先缩短边 → worker 内 `CenterCrop(224)`**（🔴 第 8 轮核验：原写「worker 内 `center_crop` 336→224」漏 Resize、措辞误导——正确流程 `Resize(短边=224)→CenterCrop(224)`，对短边 336 直接裁 224 会丢约 33% 画面、改 embedding；源码 pipeline.rs:520 `ShortEdge(image_size)` + clip.rs `preprocess_decoded` 已正确实现，本行仅措辞对齐，开销极小），**不另存 224 缓存**——避免双 cache + 路径结构改动。ViT-L@336 直接用。
- **ai_cache 查找按 `cache_key`（含 mtime）**：源文件变更后 cache_key 变 → 旧 ai_thumb 自动成孤儿（交 §3.3 GC，Part3），新 key 文件不存在时重新解码（正确）。**非按 item_id 固定路径**（否则读旧图错误向量）。失效无需在 `invalidate_derived_for_item` 内主动删 ai_thumb 磁盘文件。
- `ai_hq_cache_enabled` 联动（Part3 §3.6）：装 AI 插件自动开。

### 3.9 模型库动态发现持久化 + 自托管 Registry（A5 + C3）

> 动态发现机制（架构→batch 变体）已成熟（remote_registry.rs，§2.1⑥），仅两处加固：

- **缓存持久化（A5）**：10min 模块级 static Mutex 缓存进程重启丢失 → 持久化到 DB/磁盘（带 TTL），冷启动免重复联网（离线场景关键）。
- **自托管 Registry（C3，去第三方仓库依赖）**：`gficcg/clip_cn_vit-onnx` 无官方背书、有下线风险 → **官方自托管镜像**（CDN，Part0 §10 非代码护城河）作首选源，HF/hf-mirror 作回退。配合 §3.7 CLIP 自导出（OFA-Sys MIT 源），自托管 = 合规 + 可控 + 离线/企业可用。
- **双仓库扁平共存清理（A5）**：eisneim(旧 fp16) vs gficcg(新 fp32) 同 models 目录扁平 → 统一命名/目录结构，降维护成本。

### 3.10 合规隔离（F1 + F2 + C3，红线）

🔴 **3.10.1 SCRFD/ArcFace cfg 隔离 + CI 断言（F1）**：

- `detect_scrfd`(face.rs:253-400) + `scrfd-arcface-r50` profile(face_profile.rs:182-197) + 文件名常量 → 全部 `#[cfg(feature="face-noncommercial")]` 门控。
- **默认 build 不含该 feature** → 商业发行包**物理不编译** SCRFD/ArcFace 代码与路径常量（不能只靠运行时「不可达」）。
- **CI 断言**：商业 build pipeline 断言 `cargo tree`/二进制无 scrfd/arcface/w600k/insightface 符号 + bundle 清单无对应 onnx（对齐 Part0 §10 oss/commercial 双 pipeline）。

🔴 **本段已被 2026-07-06 T2 消解裁决推翻**(见任务表 T2 行):T1 的 cfg 物理隔离使默认(=商业同形)build 注册表**物理不含**非商用轨(编译期断言 `noncommercial_track_absent_by_default` 锁死),运行时过滤守卫与「守卫在 pro 渠道层」职责均不再需要。原文存档如下:
**3.10.2 渠道级过滤守卫（F2，已消解）**：原设计为「商业发行版后端过滤 `commercial_ok=false` 条目、守卫在闭源 pro 渠道层」——已被 T1 的 cfg 物理隔离取代（默认 build 注册表物理不含非商用轨），不再需要运行时守卫与 pro 渠道层。

**3.10.3 CLIP 自导出自托管（C3）**：`gficcg` 无 license → 从 OFA-Sys/Chinese-CLIP（MIT）用 `export_clip_l14_336_onnx.py`（memory `gotcha-clip-onnx-export`：ViT-L 文本塔 fp16 数值塌缩→fp32、opset≥17）自导出，附 MIT NOTICE，自托管 CDN。🔴 **P1-12（L6 已联网核实）**：商用依据 = 上游仓库 `MIT-LICENSE.txt`（含 sell 条款）、**非** HF 模型卡（后者无 license 字段）；审计文案据此，避免「权重无许可」质疑。

**3.10.4 中国人脸合规「形态 × 义务」矩阵（P1-11，L4 已联网核实《人脸识别技术应用安全管理办法》2025-06-01 施行）**：义务随产品形态触发，避免一刀切拖慢本地形态落地：

| 产品形态 | 触发义务 | 落地动作 |
|---|---|---|
| **本地自用**（数据不出端，用户处理自有照片） | 单独同意 + 便捷撤回 + 最短保存 | 人脸功能**默认关闭**、首次启用弹**独立**同意（非埋总隐私政策）；一键关闭即**清库**（删 `faces` 向量 + ai/face 缩略缓存，复用 §3.3 失效 + Part2 删除门闩覆盖 face 派生物）；未成年人若不外发数据风险低 |
| **上架分发**（MS Store/Steam/官网） | + 显著告知 + 未成年人监护人同意 | 安装/首启合规告知；区域功能开关（中国区可延后/限制 SCRFD 轨） |
| **云端服务**（为他人/机构集中存储 ≥10 万人脸） | + 事前 PIA（记录存 3 年）+ 30 工作日内省级网信备案 | **本地客户端形态不触发**；若演进为 B 端/云端才落，文案不得宣称「绝不受监管」 |

> 本产品默认 = 本地自用形态，触发面有限；**仍须中国法律评估**（L4 / Part0 §10.5）。人脸非唯一验证方式（办法要求存在替代手段）——本产品人脸仅做相册聚类、非身份验证，天然满足。

### 3.11 开放技术决策拍板（Part0 §5.5）

> Part0 留给 Part4 实测拍板的三项。**本 plan 给推荐 + 依据 + 何时定**：

| 决策 | 推荐 | 依据 | 何时定 |
|------|------|------|--------|
| ① ai-clip + ai-face **合并单 ai-worker** vs 分离 | **倾向合并**（初期） | 合并省进程/共享 ort init/**GPU 协调退化为进程内**（§3.6，简单）；分离崩溃隔离更彻底但需跨进程 GPU 令牌。人脸+CLIP 不同时高频运行（已有互斥），合并损失小 | worker 化原型跑通后按显存/崩溃率实测；保留 trait 边界便于日后拆 |
| ② 默认档位 | **ViT-B/16 fp16**（默认）+ ViT-L/14-336 fp32 可选 HQ 升级 | 轻量优先（Part0 §2）；B/16 512 维 ~700MB，L/14 1.7GB 仅高需求 | 已定（profile.rs:99 默认即 B/16），HQ 作付费/可选下载 |
| ③ mac CPU-only vs CoreML | **初期 CPU-only**，CoreML 待 **P6.5 平台签名 + entitlement** 后 | CoreML 需 GPU entitlement + 运行时测试（Part0 §11 line 228）；先保 mac 能跑 | P6.5 后（Part7） |

### 3.12 AI/face 失效钩子（接 Part2 §3.3 / Part3 §3.4）

- 统一入口 `invalidate_derived_for_item(id)`（Part3 §3.4 定义）**补 AI/face 分支**：源变更时 `ai_status=0` + 删该 item 的 ai_embeddings 行（+ VectorStore 删向量 + ANN 索引标脏）；`face_status=0` + 删该 item 的 faces 行（person_id 经 CASCADE/SET NULL 处理，触发受影响 person 质心重算或标脏）。
- 🔑 **为何必须**：文件换了内容（mtime+size 变），旧 CLIP 向量/人脸框是**旧图的**——不失效则语义搜索/人脸命中错图。与 Part2 P1（meta）、Part3 Q5（媒体派生）同一红线的 AI/face 延伸。
- **删除媒体**：faces 已 `ON DELETE CASCADE`（schema.rs）、ai_embeddings 须同步删（确认有无 FK，无则在删除路径显式删 + VectorStore 删）。
- 实现：本 Part 在 `invalidate_derived_for_item` 内补 AI/face 两段；Part2/Part3 已搭骨架，此处填肉。

---

## §4 分步任务清单（优先级 + 依赖 + Part6 接缝）

> 标记：**【P6】= Part6 框架层接缝**（协议/GPU 令牌/model blob/Coordinator/AES 原语，Part6 升级框架、Part4 接入）；其余为 Part4 本体。**实施序：Part6 框架升级先行**（Part0 §0 表 Part4 依赖 Part6），故 P0 优先排「不依赖 Part6 的红线 + 解锁项」。

### P0 · 红线 + 独立解锁（不依赖 Part6，先做）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T1** 🔴 | SCRFD/ArcFace `cfg(face-noncommercial)` 隔离 + CI 断言不打包 | face.rs:253-400、face_profile.rs:182、CI | — | §3.10.1 |
| **T2** ✅ | ~~`list_face_model_registry` 渠道过滤守卫~~ → **2026-07-06 消解裁决**:被 T1 cfg 物理隔离吸收——默认(=商业同形)build 注册表**物理不含**非商用轨且编译期断言锁死(ai-core `noncommercial_track_absent_by_default`),运行时过滤属冗余;§3.10.2「守卫在 pro 渠道层」随之作废;研究 build(--features face-noncommercial)显现属设计意图,商业流水线符号扫描(Part7 T16/17)为第二兜底。原表述:渠道过滤守卫（商业版后端滤 commercial_ok=false） | face_commands.rs:363、pro crate | — | §3.10.2 |
| ✅ **T3** | 人脸批量审批命令（confirm/reassign/unassign/**`reject_face_candidate`**(写 face_rejections 负样本)/create_person_from_faces **+ `list_likely_matches` 查询命令**🔴Part5 T10 硬依赖） | face_commands.rs、queries.rs | — | §3.5.1 |
| ✅ **T4** | 失效钩子补 AI/face 分支（ai_status/face_status 复位 + 删向量/faces） | `invalidate_derived_for_item`（接 Part2/3） | ← Part2 §3.3 | §3.12 |
| ✅ **T5** | ~~补 sha256+size 校验对等~~ → **2026-07-06 拍板选项 B 并实施**:size 快检入判定(+4 测试,LFS pointer/截断当场识破);全量 sha256 刻意不进判定路径(数十 MB 模型常驻启动税,本裁决收窄 §3.5.3「对等」表述,详见 docs/2026-07-06 决策 brief §3) | face_commands.rs:36 | — | §3.5.3 |

> **执行进度（2026-06-30，无人值守）**：
> - ✅ **T3 已交付**（2 commit）。StageA：queries.rs 6 DAO（confirm_face_assignment / reassign_face_to_person /
>   unassign_face / reject_face_candidate / create_person_from_faces + list_likely_matches）+ 私有助手
>   `recompute_person_aggregates`（成员脸真均值重算质心/封面/计数，归零复刻 `rebuild_person_clusters` 删空簇策略）；
>   models.rs `FaceThumb`/`LikelyMatchGroup`；face_commands.rs 6 IPC（命令名 confirm_faces / reassign_faces /
>   unassign_faces / reject_faces / create_person / list_likely_face_matches，避开与 DAO 同名冲突，沿用
>   merge_persons↔merge_face_persons 命名约定）+ lib.rs 注册；9 单测。StageB：`plan_recluster` 接通
>   `get_face_rejections`，全量重聚类 free 脸贪心匹配跳过被拒既有 person（占位新簇不受限）；+2 单测。
>   §3.5.1a 三守卫均落地（同模型断言、unassign 清 is_confirmed、归属变更同事务连带重算）。
> - ✅ **T4 已交付**（1 commit）。`invalidate_derived_for_item` 补 AI/人脸段：ai_status/face_status 复位 0 +
>   删 ai_embeddings/faces + 受影响 person 重算（复用 recompute_person_aggregates）；扩展 source_changed 单测。
> - ⏸️ **T5 暂缓（待用户决策）**：现状代码 CLIP `variant_installed`(ai_commands.rs:87) 与 face
>   `face_variant_installed` 均**刻意只按存在判定**，有文档理由（ai_commands.rs:84「下载命令按 size+sha256
>   校验并修复」）。照 plan 给 face 单独加 sha256 会①推翻该设计决策②破坏 CLIP↔face 对等③每次 UI 轮询
>   `list_face_model_registry` 对 ~37MB SFace 做完整哈希（性能代价）。折中（size-only 廉价捕获 LFS pointer/截断）
>   vs 严格 sha256 是设计取舍，无人值守不擅自推翻有据设计——留待拍板。
> - 其余 P0：T1（SCRFD cfg 隔离 + CI 断言）含 cargo feature 与 CI 基建、T2（渠道过滤）已由 T1 的 cfg 物理隔离吸收
>   ——均带构建/架构决策尾巴，未在本轮动。P1+ 依赖 Part1 ANN/Part6 框架，不在无人值守安全面。

> **本轮审查回写（2026-06-30，5 路 agent 取证）**：
> - **T6 数据层前置已就绪**：plan 把 `persons.model_name` 列、`face_rejections` 表列为 T6/§8.4 依赖，实测二者已由
>   Part1 数据层迁移交付（schema.rs:577 `ADD COLUMN model_name`、:588 `CREATE TABLE face_rejections`）。即 T6
>   缺的仅是 `set_active_face_model` / `sync_face_status_to_model` 命令实现（grep 函数定义零命中，当前仅存 doc 注释）。〔✅ 两命令已于 2026-07-02 落地(`fa0e951`):gated 激活(verified 对拍门)+ face_status 同步〕
> - **陈旧注释待清**：`ai/face_cluster.rs:21` 与 `db/queries.rs:2379` 注释仍称「persons 表无 model_name 列」，与
>   schema.rs:577 已加列矛盾（加列后注释未同步），实现 T6 前应清理以免误导。〔✅ 已清(`fa0e951`),连同 face_commands registry/download 的「无激活路径/deferred」注〕
> - **对 Part5 结论**：T3（含 `list_likely_face_matches` 查询 + `reject_faces`）+ T4 已就绪，是 Part5 §3.6 人脸审批
>   UI 的硬后端依赖，**Part5 主体不被 Part4 阻塞**；仅「人脸模型切换 UI」(T6)与「百万级 ANN 性能」(T7)为非核心潜在 gap。

### P1 · 模型切换 + ANN + 自托管（依赖 Part1）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T6** ✅ 已实施(2026-07-02 `fa0e951`;T_t3 四测;UI 切换入口未接,归 Part5 设置页波) | persons.model_name 隔离 + `set_active_face_model` + `sync_face_status_to_model` + reset 按 model | face_commands.rs、queries.rs | ← Part1（persons.model_name 列） | §3.5.2 |
| **T7** | ANN 检索接入 `VectorStore`（替暴力 O(N)；小库 simsimd / 大库 sqlite-vec-usearch） | search.rs、写入路径 | ← Part1 §3.4（trait+选型） | §3.4 |
| **T8** | 动态发现缓存持久化 + 自托管 Registry 首选源 | remote_registry.rs | — | §3.9 |
| **T9** 🔴 | CLIP 自导出（OFA-Sys MIT 源）+ 自托管 CDN | export 脚本、registry 源 | — | §3.10.3 |

### P2 · worker 化核心（【P6】框架升级先行）

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| ✅ **T10**【P6】(2026-07-03) | 协议扩展 SessionInit/SessionReady + EmbedBatch + Capability(Embedding/FaceDetectEmbed) + ErrorCode〔落地形状=Part6 §3.2.1a×D1×D3 三源合并(见 D3 §3 横幅);PROTOCOL_VERSION=2、psd-worker 同波重编译、capability 常量+catalog 枚举两端对拍;仅本地验证三绿〕 | exotic-protocol、worker.rs、message.rs | Part6 协作 | §3.1.2 |
| ✅ **T11**【P6】(2026-07-03) | ~~GpuLimiter 跨进程 GPU 令牌~~ → **host 侧 GpuToken**(D2 修正:令牌全留主进程、协议零扩展;额度恒 1、FIFO、取消感知、RAII/panic 展开无泄漏)〔落地 exotic/limiter.rs 薄封装 + AppState.gpu_token 共享实例 + 4 单测;与 gpu_analysis_owner 分层不合并;acquire 接线随 T13/T15 批派发;仅本地验证三绿〕 | exotic/limiter.rs、state.rs | — | D2 |
| ✅ **T12**【P6】(2026-07-03) | model blob 分发(kind=model 语义 + 分步下载 + 压缩比豁免=blob 不进 zip 天然成立)〔RegistryEntry.model_blobs+校验、fetch_model_blob 幂等/续传、安装命令先 blob 后 zip;落 models 目录 → D1 Level A 直接可用;仅本地验证三绿〕 | registry.rs、fetch.rs、exotic_commands.rs、package.rs | Part6 协作 | §3.7.1 |
| ✅ **T13**【P6】(2026-07-03) | Coordinator 通用化(去 PSD 硬编码,plugin_id+capability 路由)〔PluginDescriptor 注册表(capabilities 取自 Catalog)+ 双层 drained 循环 + 非 thumbnail 能力显式跳过(T15 接缝)+ per-op timeout 表(coordinator::op_timeouts,D3 §5)+ P0-3 spawn 前协议版本前置复核(installer);psd 路径行为不变,仅本地验证三绿〕 | coordinator.rs、installer.rs、pipeline.rs | Part6 协作 | §3.1.2 |
-| **T14**【P6】 🔴 | AES-256-GCM 原语（ring::aead）+ license 派生密钥（HKDF enc_seed） | crypto.rs、AES 原语模块 | Part6 协作 | §3.7.2 + 🔴**端到端对拍 gate**（第7轮终审）：固定 enc_seed+enc_salt 加密真实小 ONNX → 从 token 取 seed → HKDF → 解密 → `commit_from_memory` → **实跑一次推理、输出向量与明文模型一致（误差<1e-4）**；非仅加解密往返，不可纸面定稿。🔴 **优先级后置（Part0 §11.4.3）**：层1 AES 实装延后至 v0.1 变现后、随首个加密权重插件（AI/人脸）交付；v0.1 仅保留 `enc_seed` 字段（首发 exotic-formats 无 ONNX 权重、不需 AES） |
| ✅ **T15**(2026-07-03) | 推理核心迁出 `ai-worker` crate(合并单 worker,T9.5 数据支持、正式拍板随 T20)〔共享核 picasa-next-ai-core(engine/clip/face/profile 注册表迁出;src-tauri 六模块退化再导出薄壳,进程内路径保留=T16 过渡双活前提)+ ai-worker 子进程(SessionInit 两段校验 D1 §3 / EmbedBatch / FaceDetectEmbed / 空闲自杀 300s)+ Supervisor 泛化(run_request/session 快照/init·close_session)+ host 批量输出校验;**原行「迁 pipeline」修正:ai/pipeline 是控制面,按 §3.1.3 留主进程(T17)**;仅本地验证三绿〕 | crates/picasa-next-ai-core、crates/exotic-workers/ai-worker、exotic/{worker,supervisor,pipeline}.rs | ← T10-T14 | §3.1.1 |
| ✅ **T16**(2026-07-03) | 去 ort **Rust 依赖层** ✅ S0-S5 全段〔S0=用户 GUI 三测 14s(T18.5/b/c 性能修复后);S3a 黄金向量导出 `ef17d42`(进程内参考侧最后快照,sha16 防错拍);S2+S3b host 切 worker-only `3d67321`(删进程内推理中段/薄壳/state.ai_engine,净删 ~1300 行;`ai_backend` 退役保键忽略值;SessionReady additive 补 provider/gpu_name 回声=拍板③落地;harness 改黄金对拍+删 vram_probe);S4 Cargo 收口 `7acccd5`(src-tauri 零 ort/tokenizers/ndarray、ai-core default-features=false、AppError::Ai 改携带字符串+AiError non_exhaustive)。**证据:cargo tree -p picasa-next 零推理依赖(=tauri build 单包形态)+ 黄金对拍 cos=1.000000 全绿**;443×2 测试仅本地〕;**打包资源层**(tauri.conf 4 DLL + package.json onnxruntime-node)归 Part7 T6,本任务未碰 | Cargo.toml、error.rs、ai/*、ipc/*、state.rs、bin/worker_e2e | ← T15+e2e ✅ | §3.2 |
| ✅ **T17**(2026-07-03) | 控制面留主进程(AiEnginePool→worker 句柄、下载/profile/DB/让步留主进程)〔AiWorkerClient 句柄(spawn 死亡重建/会话快照比对切换/硬止损=同批至多 2 次尝试)+ worker 派发路径(Producer/Writer 与进程内同源复用,攒批→CPU permit→GPU 令牌(D2)→EmbedBatch;缺 ai_cache 项预检跳过保持 Processing,T18 闭合)+ **补 v2 EncodeText op**(T15 缺口收口,§3.1.2 补遗)+ `ai_backend` 双活开关(缺省 inproc 行为零变化);✅ worker e2e 已验收(2026-07-03 `54ee6fe` 程序化 harness `worker_e2e`:双后端余弦全 1.000000、检索 top-1 全一致,详 todo.md e2e 行);仅本地验证三绿〕 | state.rs、ai_commands.rs、ai/worker_client.rs、ai/worker_pipeline.rs | ← T15 | §3.1.3 |
| ✅ **T18**(2026-07-03) | ai_thumb 喂 CLIP(**worker 读 ai_cache 自解码**,EmbedBatch 传 cache_keys;非主进程发 tensor,§8.1 决策1)〔`893b1d2`:worker 派发缺缓存**现场派生**(§3.8「无命中回退派生原图产 ai_cache」原文落地):提取 generate_ai_cache 与 run_ai_thumb 共用,顺带修 run_ai_thumb 直写红线(改 write_atomic);触发背景=用户 GUI 实测全跳过(DB∩盘=1/110 孤儿缓存,覆盖率≈0);442×2 测试全绿仅本地〕 | worker 解码路径、ai_cache、derive/image.rs | ← Part3 §3.6、T15 | §3.8 |
| ✅ **T18.5**(2026-07-03) | worker 派发性能回归修复(1550 张 5min→13s 外推)〔`8941eda`:worker 端并行解码+预处理(scope 线程零新依赖)+ ai-core 像素循环扁平化(逐位一致)+ host 现场派生 rayon 并行;`40138b7` T18.5b/c:批内流水重叠(解码 channel 喂推理边收边推)+解码线程=探测核数(用户拍板不设上限;face 分块=4 为内存上限);harness Phase B2 吞吐证据 112→13→8ms/张;根因=worker 化丢并行结构,EP 无辜(cos=1.0)〕 | ai-worker/batch.rs、ai-core/clip.rs、ai/worker_pipeline.rs、bin/worker_e2e.rs | ← T17、T18 | §3.8 |
| **T19** | mac/Linux worker 低优先级（setpriority/nice） | worker.rs:110-113 | — | §3.1.2 |

### P3 · 优化 + 拍板

| ID | 任务 | 落点 | 依赖 | 验收锚点 |
|----|------|------|------|---------|
| **T20** | 开放决策实测拍板（合并 vs 分离 / 默认档位 / mac CPU-CoreML） | 原型实测 | ← T15 | §3.11 |
| ~~**T21**（可选）~~ **作废** | 方案B 无 tensor blob（§8.1 决策1，传 cache_keys）→ 原「mmap 优化 86MB tensor」无对象；模型 handle 传输已由决策2 匿名内存映射实现（§3.7.3） | — | — | §8.6 |
| **T22**（可选） | GpuLimiter 显存预算进阶（VRAM 档位放行多 worker） | GpuLimiter | ← T11 | §3.6 |

---

## §5 风险与回滚

| 风险 | 触发 | 缓解 / 回滚 |
|------|------|------------|
| 🔴 **Part6 接缝阻塞** | T10-T14 框架升级未就绪，Part4 worker 化卡住 | 实施序 Part6 框架先行（Part0 §0）；P0/P1（红线+审批+ANN）不依赖 Part6 可先交付价值；worker 化作独立里程碑 |
| ~~worker 化 IPC tensor 开销~~ **作废**（§8.1 决策1） | 方案B 传 cache_keys、worker 读 ai_cache 自解码，blob≈0 → 无 86MB tensor 跨进程问题 | 净收益 <10MB+崩溃隔离+解 !Send，无 tensor 开销代价 |
| 去 ort 后核心仍依赖 ort 类型 | error.rs:58 `From<ort::Error>` 未拆 → 编译仍拉 ort | ✅ T16-S4 已拆(`7acccd5`,AppError::Ai 携带字符串+AiError non_exhaustive);CI 依赖树断言留 CI 门控波 |
| 🔴 AES 密钥分发无效 | 硬编码密钥=形同无加密 | 密钥 license 派生（HKDF enc_seed，随 token 下发）；无授权 fail-closed 恒 Unlicensed；诚实标边界（挡懒人非逆向） |
| 合规 cfg 漏门控 | 某调用点未 `cfg` → SCRFD 编进商业包 | CI 断言二进制+bundle 无 scrfd/arcface/w600k/insightface（双 pipeline，Part0 §10） |
| persons.model_name 迁移 | 存量 persons 无 model_name 列 | 迁移回填默认轨 yunet-sface；旧标注保留不删 |
| 模型切换维度混算 | 128↔512 维 cosine 混算 panic/算错 | faces+persons 双 model_name 过滤天然隔离 + len 校验；切换重置按 model |
| GPU 跨进程令牌死锁 | 多 worker 抢令牌互等 | 非对称获取（仿现有 gpu_analysis_owner try_acquire 快失败）；超时释放 |
| ai-worker 合并决策错 | 合并后崩溃隔离不足/显存挤占 | 保留 trait 边界（§3.11）便于改分离；原型实测再定 |
| SCRFD 激活静默算错 | detect_scrfd UNVERIFIED 未对拍即激活 | §3.5.2 前置门：合规 + InsightFace 对拍通过才允许切；默认轨不受限 |

---

## §6 验收标准

**P0 红线 + 解锁**：

1. **合规**：商业 build `cargo tree`/二进制无 scrfd/arcface/w600k/insightface 符号；bundle 无对应 onnx；`list_face_model_registry` 商业渠道不含 commercial_ok=false。
2. **批量审批**：confirm/reassign/unassign/create_person 可写 `is_confirmed`/person_id；recluster 保护已确认脸（不移动）。
3. **失效钩子**：替换文件（mtime+size 变）→ ai_status/face_status 复位 + 旧向量/faces 删 → 重算（语义搜索/人脸不命中旧图）。

**P1 切换 + ANN**：

4. **模型切换安全**：切 face track 不 panic、不混维度；persons 按 model_name 隔离、旧轨标注保留；CLIP 切档 `sync_ai_status` 重同步。
5. **ANN**：百万级语义搜索从 ~1s+ 降到亚秒（大库 ANN 索引生效）；小库 simsimd 精确快路径。
6. **自托管**：CLIP 模型从官方自托管源可下（HF 回退）；自导出权重附 MIT NOTICE。

**P2 worker 化（去 ort 核心）**：

7. **去 ort**：核心 `cargo build` 依赖树无 ort/tokenizers；bundle 无 4 ONNX DLL；包体大降（移出 4 ONNX DLL，DirectML.dll 为大头；实际 stat 前后对比记入 Part7）。
8. **worker 化**：ai-worker `SessionInit` 加载模型常驻 → `EmbedBatch` 返回 embeddings；worker crash 不拖垮主进程（崩溃隔离）；mac/Linux worker 低优先级生效。
9. **GPU 协调**：CLIP 与 face 分析不同时占 GPU、无 DX12 争用 OOM。〔🟢 口径更新(2026-07-10 审查 D3):落地形态为**合并单 worker + host 侧 GpuToken(每批) + gpu_analysis_owner 进程内互斥**,原文「CLIP 与 face worker(两进程)+ 跨进程令牌」指向已被 D2/T20 取代的架构〕
10. **AES**：无 license 下到的密文不可解；有 license 派生密钥解密加载成功；明文不落盘（或即时加密缓存）。
11. **ai_thumb**：CLIP 编码复用 ai_cache（短边 336）、不解原图（GPU 利用率升，CPU 解码降）。

---

## §7 执行提示词（新会话直接用）

```
任务：实施 Picasa Next 重构 Part4（AI 与人脸插件化）。

【先读】
1. docs/refactor_2026/Part0_总纲与产品定稿.md §5(打包模型)/§5.5(关键架构决策:worker统一/ort外置/GPU协调/blob分离)/§7(盈利诚实边界)/§8(防护层1)
2. docs/refactor_2026/Part1_数据层.md §3.4(VectorStore trait+两阶段ANN选型) + persons.model_name 列
3. docs/refactor_2026/Part6_插件平台与exotic收尾.md §7 —— 本 Part 强依赖 Part6 框架升级(协议扩展/GpuLimiter/model blob/Coordinator通用化/AES原语)，Part6 先行
4. docs/refactor_2026/Part3 §3.4(invalidate_derived_for_item)/§3.6(ai_thumb) + Part2 §3.3
5. docs/refactor_2026/Part4_AI与人脸插件化.md 全文（§2 现状取证为基线，§3 设计，§4 任务表，§2.3 G1-G6 是 Part4/Part6 接缝清单）

【铁律(三红线)】
- 合规：SCRFD/ArcFace(InsightFace 非商用)绝不进任何发行包→cfg(face-noncommercial)隔离+CI断言;当前是死代码但代码+路径常量仍在主二进制,必须 feature-gate。
- 防白嫖：ONNX 权重 AES-256-GCM 加密(ring::aead,无新crate)+密钥 license 派生(HKDF enc_seed);商业边界=enc_seed/私钥凭证/付费载荷,非源码可见性(第一方公开源码统一 AGPL-3.0-only)。
- 诚实：CLIP/YuNet/SFace 权重来自公开仓库,卖集成+中文+体验非秘密权重;加密挡懒人非逆向,文案不夸大独家。

【控制面/数据面分离】下载/profile/DB/状态机/让步留主进程,推理(Session/VRAM)进 worker;**图像 worker 读受限 ai_cache 自解码(EmbedBatch 传 cache_keys,非主进程发 tensor,§8.1 决策1)**。
【关键约束】文本编码器固定 CPU(DirectML BERT int64 Gather 静默算错),外置后图/文双 EP 保持分离;架构id=向量空间主键,切换=重嵌入;persons 须加 model_name 隔离(切换不混维度)。

【顺序】
P0(不依赖Part6,先做): T1 SCRFD cfg隔离+CI → T2 渠道过滤 → T3 批量审批命令 → T4 失效钩子AI/face → T5 变体校验
P1(依赖Part1): T6 模型切换安全(persons.model_name) → T7 ANN接VectorStore → T8 动态发现持久化 → T9 CLIP自导出自托管
P2(【P6】框架先行): T10 协议扩展 → T11 GpuLimiter → T12 model blob → T13 Coordinator通用化 → T14 AES+license派生 → T15 推理核心迁出 → T16 去ort → T17 控制面留主进程 → T18 ai_thumb喂CLIP → T19 mac低优先级
P3: T20 开放决策拍板 → T21 mmap优化 → T22 显存预算

【验收】按 §6 十一条;P0 三条红线+T16 去ort 是硬门槛。
【关键认知】exotic worker 框架承接 AI/face 有 6 缺口(G1-G6):长驻session握手/GPU协调/model大文件/批量流式/Coordinator通用化/错误码——这是 Part4↔Part6 接缝;GPU 互斥外置后软门闩失效(跨进程),必须新建跨进程令牌;去 ort 移出 4 ONNX DLL(DirectML.dll 大头,具体 stat 实测,勿把估计当数字)+ort/tokenizers 编译体积。
```

---

## §8 评审修正记录（对抗 review w3mniao14，2026-06-26）

> 4 reviewer 对抗审查。本节记录全部 Part4 相关 issue 的分诊处置，**无省略**。critical 项须在进入 P2 worker 化前落定。

### 8.1 已改正文

| issue | 处置 |
|------|------|
| 86MB blob vs MAX_BLOB 64MiB | ✅ **方案B 作废此问题**（§8.1 决策1）：EmbedBatch 传 cache_keys、worker 读 ai_cache 自解码 → blob≈0，无需调高/限批 |
| ai_thumb 短边 336 vs 224 跨 Part 冲突 | ✅ §3.8 改方案①（固定 336 + worker center_crop 224，单 cache）+ cache_key 查找说明 |

### 8.2 🔴 Critical 设计补充（P2 动工前必须落定）

1. **AES 解密主体进程**（§3.7.2，§8.1 决策2 定稿）：worker 是独立二进制、**不承担密钥派生与解密** → **主进程解密**：用 license 派生密钥 `ring::aead` 解密 ONNX 密文 → 写**匿名内存映射**（Linux/mac `memfd`/`shm_open`，Win `CreateFileMapping`；机制最终细化为**具名 shm 传 name**，ModelHandle::Named 优先，见 §3.7.3）→ `SessionInit` 传 **handle/fd**（🔴 **非明文 bytes 经帧**——ViT-L 1.7GB 超 MAX_BLOB）→ worker `mmap` → **`Session::builder().commit_from_memory()`**（🔴 **ort rc.12 已支持，无须 API 改动**）从内存构建 Session。明文不落盘、不经帧；worker 用完 `munmap`、主进程 `zeroize`。§4 T14/T15 据此。（早稿「encrypted=false 明文 bytes 字段 / 须 API 改动 / worker 收明文 tensor 流」**作废**，§8.6 决策2。）〔✅ 施工级定稿 [D1](2026-07-02-Part4-D1-模型载荷传输与AES解密.md)(2026-07-02):**两级通道**——明文权重(P2 首期全部)直接传文件路径 `ModelHandle::Path`,AES 后置期零 shm 复杂度;具名 shm `ModelHandle::Named` 仅加密权重(④)启用;枚举一次定型不二次破坏升版〕
2. **GpuLimiter 跨进程实现**（feasibility critical，§3.6 空方案）：进程内 `Mutex` 对 worker 不可见 → **跨进程令牌走协议帧**：新帧 `GpuTokenRequest/Granted/Release`；Coordinator 持 `tokio::sync::Semaphore`(额度1)，收 Request 时 acquire、回 Granted，worker 完成/崩溃时 Supervisor `Drop` 发 Release。⚠️ **每批推理叠加一次 IPC 往返延迟**。🔑 **若 §3.11 选合并单 ai-worker → 跨进程令牌退化为进程内、本节大幅简化**——这是「倾向合并」的又一有力论据。〔🔴 本条帧方案已被 [D2](2026-07-02-Part4-D2-GPU令牌与并发仲裁.md)(2026-07-02)**修正**:实测批派发权在 host(Supervisor 严格串行+host 派批),令牌完全留主进程——发批前 acquire、RAII 释放,**协议零扩展、无 IPC 往返延迟**;且合并/分离两形态代码路径一致,连本条的形态分叉论据也一并消除〕
3. **长驻 session 握手 vs Supervisor 串行模型**（feasibility major，§3.1.2 G1）：现 `HANDSHAKE_TIMEOUT=5s`（coordinator.rs:39）**不够 ORT 加载（数秒~分钟）** + Supervisor「每请求串行」模型需改。**定**：① `SessionInit` 握手超时独立配置 **120–300s**（与常规任务 5s 分开）；② Supervisor `run/send_request` 加 timeout 参数或「等到 SessionReady/断开」模式；③ 合并单 worker 则可在 Hello/Ready 扩展字段返回 session 就绪状态，进一步简化。〔🔴 本条已被 [D3](2026-07-02-Part4-D3-长驻Session握手与Supervisor改造.md)(2026-07-02)**修正**:实测进程握手(Hello/Ready 5s)与模型加载本就无关——握手不动,**300s 归 SessionInit 请求 timeout**(op 级,复用 Request/Success 帧);SessionReady=SessionInit 的 Success 响应,③ 的 Hello/Ready 扩展方案弃用(会把会话状态耦合进进程握手);Supervisor 的 per-call timeout 参数实测已存在,改造面=op 泛化+session 字段〕

### 8.3 跨 Part 依赖修正（悬空/勘误）

1. **persons.model_name 悬空依赖**（consistency major）：Part4§3.5.2/T6 要求该列但 **Part1 未规划** → **回溯 Part1**：§3.7 补债 `ALTER TABLE persons ADD COLUMN model_name TEXT NOT NULL DEFAULT 'yunet-sface'`（与 faces.model_name 对称，归 V10 或单独版本）+ §4 加任务；**回填 SQL 同事务**；`cosine_similarity` 前加 **硬断言（非 debug_assert）** `a.len()==b.len()`；T6 依赖标注改「← Part1 persons.model_name 列（须先就绪）」。
2. **VectorStore 调用假设 vs Part1 trait**（consistency critical）：**回溯 Part1**——① AppState 补持有结构 `ai_vector_stores: HashMap<model_name, DynVectorStore>`（按架构 id 多实例，因 embed_dim 512/768/1024 不混）；② **阈值单位明确**：`ANN_THRESHOLD=500_000` 对 CLIP 计 ai_embeddings 行数(=item 数)、对人脸计 **faces 行数**（一图多脸，可能更早触发，与「人脸数远小」假设需调和——人脸可单设阈值或接受暴力，faces 通常 <50万）；③ **人脸聚类是否走 VectorStore**：现 `cluster_new_faces` 全量读质心（O(P)，P=persons），**P≪N、暂不引 ANN**（§2.5 补 F6 标注「persons 通常 <数千，线性可接受」），不强制走 VectorStore::search。
3. **Part0§5.5 决策2 勘误标注**（consistency major）：Part0 称「coordinator.rs `evaluate_run()` 已参数化可复用」与实测「硬编码 `PSD_PLUGIN_ID`」矛盾 → **以 Part4§2.3 实测为准**（参数化是设计目标非现状，由 Part6 T13 实现）；不改 Part0 定稿正文，此处标注勘误。
4. **协议 crate 归属**（major，Part0§5.5 决策5 开放）：**定不建 `ai-protocol` 新 crate，扩展 `exotic-protocol`**（关闭开放决策）。
5. **三角循环依赖解环**（major）：Part3 T11(ai_thumb 联动)↔Part4 T18(喂CLIP)↔Part6 install → **解耦**：Part3 T11 依赖 Part6 install hook 写 config（不依赖 Part4 T18）；Part4 T18 依赖 Part3 ai_cache **文件约定**（不依赖 T11 是否已开）。T18 无 ai_cache 时**静默降级解原图**（功能正确、性能次优），§6 验收区分快/降级两路径。

### 8.4 设计补充（实施时生效）

1. **人脸 reject 负样本**（completeness major，**✅ 第 7 轮终审已回写 §3.5.1 正文 + T3**）：补 `reject_face_candidate(face_ids[], person_id)` + `face_rejections(face_id, person_id)` 表（🔴 **DBX-01 定稿，第 6 轮独立核验**：已补入 **Part1 V10 DDL**，`PRIMARY KEY(face_id, person_id)` + FK `ON DELETE CASCADE`；**不走 `faces.rejected_person_ids BLOB` 方案**），recluster 跳过已拒绝对，防质心相近反复误聚。命令须随 §3.5.1 一并加入 `lib.rs` `generate_handler!`（IPC-1）。〔第 6 轮称「补第 5 命令」时 §3.5.1 仅 4 写命令；后 `list_likely_matches` 入正文，本命令实为继其后的写命令——第 7 轮终审发现 §3.5.1 正文/T3 始终缺此命令（DBX-01 回写不完整），已补齐，权威正文 ↔ Part1 表 ↔ Part5 reject UI 三处对齐。〕
2. **合并 vs 分离前置实测**（completeness major）：T10-T14 设计分叉依赖合并/分离（GpuLimiter 进程内 vs 跨进程、Coordinator 一个 plugin_id vs 两个）→ **加前置 `T9.5` 最小原型实测 ai+face 合并进程 VRAM 用量**（<4GB 合并、否则分离），结果作 T10-T14 配置输入，避免先实现后推翻。〔✅ T9.5 已实测(2026-07-02,`src-tauri/src/bin/vram_probe.rs`,DXGI 本进程 CurrentUsage 口径):**合并单进程 361.6 MB** @ViT-B/16 fp16(CLIP-only 265.7 / face 边际 ≈96;adapter0 预算 11316 MB;drop 后归零=DirectML 实际归还);HQ 档 ViT-L fp32 上界折算 ≈2.5-3GB 仍 <4096 阈值 → **数据支持合并**。正式拍板随 T20 补崩溃率半边(§3.11:保留 trait 边界便于日后拆)〕
3. **测试任务**（completeness minor，Part0§11）：§4 补——`T_t1` CLIP tokenizer 中英样例回归（文本 CPU，10 query 向量对参考值）；`T_t2` 人脸批量审批命令单测（confirm/reassign/unassign 事务 + recluster 保护）；`T_t3` 模型切换安全（persons.model_name 隔离、维度不符不 panic）。
4. **§3.7.2 AES 细节**（feasibility major）：HKDF 须明确 salt（per-model 随机 32B **存 `ModelBlob.enc_salt`、非密文前缀**——CANON-02 定稿，对齐本 Part §8.6 / Part6 §3.7.2）+ info(plugin_id+model_id 字节)；GCM nonce 每次随机 + 随密文前缀存；密文 `nonce(12B)||ct||tag(16B)`；`ring::hkdf`/`ring::aead` 在 ring 但当前未调用，须新写。〔删除初稿"salt 随密文前缀存储"旧值。〕

### 8.5 已知接受 / 已正确（reviewer 确认 strengths）

- 文本编码器固定 CPU、合规 cfg 隔离 + CI 断言、§3.11 开放决策三要素、协议帧防内存炸弹设计、`invalidate` 覆盖四面、向量 O(N)→ANN 方向——reviewer 列为可靠设计。
- `invalidate_derived_for_item` owner 与事务边界：**与 Part3 §8.4 一致**（owner=Part3，SQL 进事务、VectorStore 内存操作事务后执行）；Part4 §3.12 只填 AI/face 段。

---

## §8.6 Part6 联动 review 回溯（webto43dl，2026-06-27）

> Part6 联动 review 反向修正本 Part 若干处。canonical 决策在 Part6 §8，本节登记受影响处。

- 🔁 **决策1 · G4 改方案B**（worker 读 ai_cache 自解码，**非**主进程发 tensor，见 §3.1.3 已改）：连带受影响——§3.1.4「86MB blob 跨进程」**作废**（不传 tensor）；§3.8「NCHW tensor 发 worker」改「worker 读 ai_cache 自解码」；§4 T18 改「EmbedBatch 传 cache_keys、worker 解码」；T21「mmap 优化 tensor」改为「优化模型 handle 传输」（决策2）；§5 风险「86MB blob 开销」删。**EmbedBatch 字段定稿** `{item_ids, cache_keys, fingerprint, batch_size}`。
- 🔁 **决策2 · AES 模型传输 = 匿名内存映射**：§3.7.3/§8.2.1「SessionInit 附明文 bytes」改「主进程解密→memfd/CreateFileMapping→传 handle→worker mmap→commit_from_memory」（ViT-L 1.7GB 超 MAX_BLOB 不走帧）；`commit_from_memory` **ort rc.12 已支持**（删「须 ORT API 改」）。
- 🔁 **AES nonce/salt 并入 §3.7.2 正文**（不只 §8.4.4）：密文 `nonce(12B)||ct||tag(16B)`；`salt(32B)` 存 `ModelBlob.enc_salt`（随 Registry 签名）；`key=HKDF(enc_seed, salt, plugin_id||model_id)`；nonce 每次分发刷新。
- 🔁 **G6 错误码**对齐 Part6 §3.2.2：`GpuUnavailable/SessionExpired/ModelLoadFailed/EmbedDimMismatch`（§3.1.2 的 OrtError 并入 ModelLoadFailed terminal、GpuOom→GpuUnavailable、ModelNotLoaded→ModelLoadFailed）。
- 🔁 **SessionInitBody 字段**（Part6 §8.3）：`image_provider`（文本 EP 由 worker 内硬编码 CPU）+ `model_profile: ModelProfileSnapshot{arch_id, image_file, text_file, batch_size}` + `model_handle`（决策2）。
- 🔁 **worker.rs:111 低优先级去重**：§4 T19 与 Part6 §4 T8 同一任务 → **Part6 T8 执行、本 Part 验收**（T19 不重复实现）。
- 🔁 **§2.3 G5 勘误**：「Coordinator 大幅重构」→「改造量适中」（`evaluate_run` 已参数化，仅顶层去硬编码，Part6 §3.3）。

---

> **Part 4 正文完**。下游：Part5（前端，消费语义搜索/人脸审批命令 + 插件 gate UI）、Part6（插件平台框架升级 = 本 Part 的 G1-G6 接缝 + AES 原语 + EntitlementProvider）、Part7（mac CoreML/平台签名）、Part8（AI/人脸定价 + license 签发 + 诚实文案）。
> 执行前必读：Part0 §13 + Part1 §7 + Part6 §7 + 本文 §7。
> ⚠️ **实施强依赖 Part6 框架升级先行**（worker 化 P2 全段）；P0/P1 可先独立交付。
