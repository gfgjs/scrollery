---
id: 2026-07-24-Spec06_AI人脸OCR
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec06 · AI / 人脸 / OCR / 远程校对

> 一句话:本篇讲 Scrollery 的四条"智能"能力线——CLIP 语义搜索、人脸检测与聚类、OCR 文字提取、远程 LLM 校对——它们共享同一条推理隔离铁律与同一个 `ai-worker` 子进程契约面。服务对象:要扩展这四条线之一,或要新增第五条推理能力的工程师/LLM 代理。读前无需先读其余篇,但改 DB schema 前建议对照 `Spec01`(数据层)。

## 1. 概览

Scrollery 的"AI"不是一个模块,是四条**共享同一进程边界**的独立能力线:

1. **CLIP 语义搜索**:图像/文本嵌入(embedding)→ 内存余弦相似度打分 → Top-K 结果落临时表。
2. **人脸识别**:检测(SCRFD/YuNet)+ 嵌入(ArcFace/SFace)→ 增量最近质心聚类(**不是** HDBSCAN,见 §3.2 更正)→ 人工审批(confirm/reassign/reject/merge)。
3. **OCR 文字提取**:PP-OCRv5(det+cls+rec 三阶段)ONNX,画廊图片与视频截帧两个入口,**第 4 个付费插件**(见 §3.4 与 §7 的架构外扩说明)。
4. **远程校对**:文档文本分块经用户自配的 OpenAI 兼容 LLM 端点校对,产出 track-changes 差异。

**铁律(全篇最重要的一条不变量)**:**推理恒在 `ai-worker` 子进程,host(`src-tauri` 主进程)零 ort/tokenizers/ndarray**。这不是约定,是编译期物理事实——`src-tauri/Cargo.toml:89` 对 `scrollery-ai-core` 依赖声明 `default-features = false`,而 ort/tokenizers/ndarray/image/imageproc 全部挂在该 crate 的 `inference` feature 后面(依赖声明 `crates/scrollery-ai-core/Cargo.toml:16-34`,`inference` feature 列表 `:46-54`,声明头 `:40-42` + `crates/scrollery-ai-core/src/lib.rs:24-42`)。host 只编译进"纯契约面"——模型注册表(`profile.rs`/`face_profile.rs`/`ocr_profile.rs`)、几何类型(`face_types.rs`)、嵌入字节序工具(`embedding.rs`)、解码图类型(`decoded.rs`)。`ai-worker` 这一 crate(`crates/exotic-workers/ai-worker/Cargo.toml:20`)不声明 `default-features = false`,拿到完整 `inference` feature——它是 workspace 里**唯一**链接 ort 的可执行体。

### 代码位置

| 路径 | 职责 |
|---|---|
| `src-tauri/src/ai/pipeline.rs` | CLIP 分析控制面(Producer/Writer,与推理后端无关) |
| `src-tauri/src/ai/worker_pipeline.rs` | CLIP 分析派发到 `ai-worker` 的中段(唯一路径,T16 起) |
| `src-tauri/src/ai/worker_client.rs` | `ai-worker` 子进程句柄:spawn/会话管理/批请求/硬止损重试 |
| `src-tauri/src/ai/search.rs` | 语义搜索:常驻 f16 嵌入缓存 + rayon 余弦打分 |
| `src-tauri/src/ai/search_control.rs` | 搜索闸门与结果提交(`SearchControl`/`run_search`,超轮次结果作废);嵌入缓存为 `search.rs` 的 `EmbeddingCache` |
| `src-tauri/src/ai/face_pipeline.rs` | 人脸分析控制面 + worker 派发(1107 行,CLIP 姊妹实现) |
| `src-tauri/src/ai/face_cluster.rs` | 增量最近质心聚类 + 全量重聚类(819 行) |
| `src-tauri/src/ai/{profile,face_profile,clip}.rs` | 再导出薄壳,真身在 `scrollery-ai-core` |
| `src-tauri/src/ai/ocr_registry.rs` | OCR 模型下载清单(资产源钉定) |
| `src-tauri/src/ai/remote_registry.rs` | CLIP 模型库动态发现(HuggingFace tree API) |
| `src-tauri/src/ai/runtime_config.rs` | 跨模块共享的运行时配置解析(models_dir/active_profile 等) |
| `src-tauri/src/ipc/ai_commands.rs` | CLIP 命令(15 条) |
| `src-tauri/src/ipc/face_commands.rs` | 人脸命令(23 条) |
| `src-tauri/src/ipc/ocr_commands.rs` | OCR 命令(4 条) |
| `src-tauri/src/ipc/proofread_commands.rs` | 远程校对命令(5 条) |
| `src-tauri/src/db/queries/ai.rs` | CLIP 嵌入/状态查询(513 行) |
| `src-tauri/src/db/queries/faces.rs` | 人脸/人物 DB 操作(2480 行) |
| `src-tauri/src/download/mod.rs` | 通用下载引擎(HTTPS 强制/Range 续传/镜像回退/sha256 校验) |
| `crates/scrollery-ai-core/` | 推理契约面 + 推理实现(`inference` feature 内) |
| `crates/exotic-workers/ai-worker/` | 独立可执行子进程,唯一推理运行时载体 |

### 整机中的位置

```
                         ┌─────────────────────────────┐
                         │   src-tauri (host, 主进程)   │
  Tauri IPC ──────────►  │  ai_commands / face_commands │
                         │  / ocr_commands / proofread   │
                         │         │                     │
                         │  ai::pipeline / face_pipeline  │  ← Producer/Writer 控制面
                         │  (领取 DB 任务/攒批/落库/让步)  │     (DB-only, 零 ort)
                         │         │                     │
                         │  ai::worker_client::AiWorkerClient │ ← 唯一推理通路
                         │         │ stdin/stdout 协议帧    │    (exotic_protocol v2)
                         └─────────┼─────────────────────┘
                                   │ spawn / SessionInit / EmbedBatch /
                                   │ FaceDetectEmbed / OcrBatch
                                   ▼
                         ┌─────────────────────────────┐
                         │  ai-worker (独立子进程)       │
                         │  scrollery-ai-core::engine    │  ← ort Session 池
                         │  (inference feature 全开)     │     CLIP/SCRFD/ArcFace/OCR
                         └─────────────────────────────┘
```

host 侧调用 `ai_commands`/`face_commands`/`ocr_commands` → 落 DB 任务队列或直接经 `AiWorkerClient` 发协议帧;`ai-worker` 只做推理,不碰 DB、不做业务编排、不持久化任何东西(除会话内的 ort Session)。远程校对(`proofread_commands`)完全独立于 `ai-worker`,是 host 直接发 HTTPS 请求给用户配置的 LLM 端点(见 §3.4)。

## 2. 数据模型与状态

### 2.1 DB 表(全表定义见 `Spec01` 数据层;此处只列本篇消费的关键列)

**`ai_embeddings`**(`src-tauri/src/db/schema.rs:225-233`):
```sql
CREATE TABLE ai_embeddings (
    item_id      INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    model_name   TEXT    NOT NULL,   -- 架构 id = 向量空间键,换模型旧向量失效
    embedding    BLOB    NOT NULL,   -- f32 LE,维度由 ModelProfile.embed_dim 定
    version      INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (item_id, model_name)
);
```
`media_items.ai_status`(0=pending/1=processing/2=done/3=error,`schema.rs:237-240`)驱动分析流水线的领取与进度。

**`persons`**(人物簇,`schema.rs:408-419`)+ **`faces`**(人脸实例,`schema.rs:422-438`):
```sql
CREATE TABLE persons (
    id, name, cover_face_id, centroid BLOB,  -- 质心,增量归类用,f32 LE
    face_count, is_named, is_hidden, is_ignored,
    model_name TEXT NOT NULL DEFAULT 'yunet-sface'  -- 人脸模型轨隔离,同 faces.model_name
);
CREATE TABLE faces (
    id, item_id, person_id,           -- person_id 可空 = 未归类
    model_name TEXT NOT NULL,          -- 嵌入模型身份 = 向量空间(同 CLIP 的 model_name 语义)
    bbox_x/y/w/h REAL,                 -- 归一化 [0,1],与显示分辨率解耦
    landmarks BLOB,                    -- 5 关键点,f32 LE 5×2
    det_score REAL, quality REAL,      -- 综合质量分,挑 cover / 滤低质聚类
    embedding BLOB NOT NULL,           -- f32 LE
    is_confirmed INTEGER DEFAULT 0     -- 用户确认/手动指派,重聚类不打散(§4 不变量)
);
```
`face_rejections`(`schema.rs:590-592`)记负样本(某脸被用户判定"不是这个人"),供全量重聚类避免反复误聚回同一人物。`media_items.face_status`(同 `ai_status` 语义,独立开关,`schema.rs:442-443`)。

**为什么人脸破 `ai_embeddings` 的 `(item_id, model_name)` 单主键范式**:一图可多脸,`faces` 每脸自增 id,`item_id` 非唯一(`schema.rs:402-403` 注释)。

### 2.2 内存态

| 状态 | 归属 | 生命周期 |
|---|---|---|
| `EmbeddingCache`(`ai/search.rs:27-32`) | `AppState.ai_embedding_cache: RwLock<Option<..>>`(`state.rs:138`) | 常驻,首次搜索懒加载;新嵌入落库后 `invalidate_embedding_cache()` 使其失效重载(`ai/pipeline.rs:434`) |
| `AiWorkerClient`(`ai/worker_client.rs:122-131`) | `AppState.ai_worker: Mutex<..>`(`state.rs:133`) | 持有子进程句柄 + CLIP 会话簿记 + 独立的 OCR 会话簿记(`ocr_loaded` 字段,D-OCR-1) |
| `gpu_analysis_owner`(`state.rs:169`) | `Mutex<Option<&'static str>>` | CLIP 与人脸分析互斥共享的 GPU 分析槽,值为 `GPU_OWNER_AI`/`GPU_OWNER_FACE`(`state.rs:289-290`) |
| `ai_analysis_token` / `face_analysis_token`(`state.rs:148,153`) | `RunTokenSlot` | 见 §4 |

### 2.3 前端 store

`aiStore`/`faceStore`(Vue Composition API,消费 `ai_commands`/`face_commands` 的状态命令)驱动开始/暂停/停止 UI;具体字段待核实(本篇未逐行核对前端 store 源码,详见前端组件目录 `src/stores/`)。

## 3. 关键流程与算法

### 3.1 CLIP 语义搜索

**索引(后台流水线,`ai/worker_pipeline.rs:48-113`)**:
1. `active_profile(state)` 解析当前激活模型(`runtime_config.rs:67`),`build_session_spec` 组装会话规格。
2. `produce_tasks`(`pipeline.rs:143-248`)批量查 `ai_status=0` 项,标记 Processing,发入 crossbeam 有界通道(容量 1024)。
3. `dispatch_loop`(`worker_pipeline.rs:116-161`)攒批(批大小 = `resolve_batch_size`,见下)→ `dispatch_batch`:
   - 先取 CPU permit(`background_heavy_limiter`)→ 缺 AI 缓存的项现场解码派生(短边 336px,`derive::image::generate_ai_cache`,rayon 并行)→ 再取 GPU 令牌(**D2 顺序天条**:先 CPU 后 GPU,`worker_pipeline.rs:176`)→ 组装 `EmbedItem` 批 → `AiWorkerClient::embed_batch` 经协议帧发给 `ai-worker`。
4. `write_results`(`pipeline.rs:324-404`)按"满 512 或陈龄超 3s,先到先落"策略批量写 `ai_embeddings` + 置 `ai_status=2`,失败项标 `ai_status=3`。

**批大小解析**(`pipeline.rs:255-320`,`resolve_batch_size_from`):配置键 `ai_batch_size`(0=自动)→ 自动档按 `detect_vram_bytes()` 阶梯(≥12GB→256/≥8GB→128/≥4GB→64/≥2GB→32/其余→16)→ 手动值硬上限 256(防 OOM,对应 experience §2)→ 固定 batch 图像塔变体(如 `.img.b8.` 命名)会把有效 batch 抬到 ≥k。

**搜索(`ai_commands::semantic_search_cmd` → `ai/search.rs`)**:
1. 查询文本经 `AiWorkerClient::encode_text`(`RequestBody::EncodeText`)在 worker 侧编码为向量(host 零 tokenizers)。
2. `ensure_cache`(`search.rs:48-106`)确保当前模型的嵌入已从 SQLite 一次性载入常驻 f16 缓冲(取代旧版"每次查询重读全部嵌入"设计)。
3. `semantic_search_with_vector`(`search.rs:111-215`)用 rayon 对全部行并行点积(单位向量点积==余弦),`sort_unstable_by` 降序取 Top-K,写入 `ai_search_results` 临时表(先 DELETE 再批量 INSERT,事务内)。
4. **竞速补口**(J17 裁决,`search.rs:134-152`):`ensure_cache` 返回与取读锁之间有空隙,并发的 `invalidate_embedding_cache` 可能在此插入把缓存置空;首次读到 `None` 重跑一次 `ensure_cache` 再取,第二次仍空才报错(区分瞬态竞速与结构性故障)。

### 3.2 人脸检测 + 聚类 + 审批

**检测/嵌入(`ai/face_pipeline.rs`)**:与 CLIP 派发同构(Producer→攒批→CPU permit→GPU 令牌→Writer),但解码源三级定源不同(`resolve_face_decode_source`,`face_pipeline.rs:337-403`):缩略图分档(短边 ≥640)→ face 专属缓存(短边 640 WebP,`face_thumbs/`,与 CLIP 的 336px `ai_cache` **不共用**,因 640px 输入对 CLIP 缓存太小会损害小脸召回)→ 小原图直派(短边本就 ≤640)。派发批上限固定 16(`FACE_DISPATCH_BATCH`,`face_pipeline.rs:641-647`,理由:YuNet 逐图推理,批大小不影响吞吐,只影响超时/取消粒度)。

**聚类算法(`ai/face_cluster.rs`)——纠正待核实项**:map-backend 摸底稿称"HDBSCAN 聚类",**核实后实为增量最近质心贪心算法,并非 HDBSCAN**(`face_cluster.rs:2-3` 模块头明文:"Incremental nearest-centroid face clustering")。伪码:

```
assign_face(new_embedding, roster):
    for person in roster:
        sim = cosine(new_embedding, person.centroid)
        if sim >= threshold and sim is best so far:
            best = person
    if best found:
        best.centroid = normalize(running_average(best.centroid, new_embedding))
        best.face_count += 1
        return best.id
    else:
        create placeholder person (id = negative, not yet persisted)
        return placeholder.id
```
(`assign_face`,`face_cluster.rs:110-181`)。**没有全量复核阶段**——同一人因早期样本不足被分成多个未命名人物是已知 v1 局限,靠用户手动 `merge_face_persons` 弥补(`face_cluster.rs:5-18` 模块头显式记录此取舍)。全量重聚类(`recluster_all`/`plan_recluster`,`face_cluster.rs:330-518`)是罕见的显式命令,O(n²) 但保护契约:`is_confirmed` 脸与 `is_ignored`/命名人物永不被重排("重聚类不打散")。

**审批流(`ipc/face_commands.rs`)**:`confirm_faces`(锁定归属,504)、`reassign_faces`(改派+锁定,512)、`unassign_faces`(清空归属+清确认,529)、`reject_faces`(记负样本+移出,540)、`create_person`(从脸新建人物,558)、`merge_face_persons`(并簇,442)。全部经 `write_blocking` 下沉阻塞线程,写后 `state.bump_data_version()` 通知前端 `personId` 视图已变。

### 3.3 OCR 文字提取

四命令(`ipc/ocr_commands.rs`):`ocr_status`(门控+安装态,220)、`ocr_extract_image`(画廊图片,254)、`ocr_extract_frame`(视频截帧,289)、`download_ocr_models`(392)。

**推理路径**:两识别命令共用 `run_ocr_batch`(`ocr_commands.rs:157-212`)→ `AiWorkerClient::ocr_batch`(`worker_client.rs:479-578`)。OCR 会话(`OcrSessionSpec`)**独立于** CLIP 会话(`SessionSpec`)——worker 内 OCR 与 CLIP 双槽并存互不干扰(D-OCR-1,`worker_client.rs:91-93`),靠 `ocr_loaded: Option<String>` 字段单独簿记,换代/自杀即复位。

**模型档位**:PP-OCRv5 分 mobile/server 两档,四文件(det+cls+rec+dict),资产源钉定(`ai/ocr_registry.rs:37-88`):主源 ModelScope `RapidAI/RapidOCR` v3.9.2 直链,镜像 GitHub `GreatV/oar-ocr` v0.3.0(cls 无镜像)。`cls_input_hw` 真机定案为 **80×160**(`crates/scrollery-ai-core/src/ocr_profile.rs:60` 字段 + `:103,125` 两档取值;doc comment `:54-59` 记录定案过程——2026-07-23 真机对拍时按旧版 `ch_ppocr_mobile_v2.0_cls` 惯例猜值 `[3,48,192]` 触发 ONNX Runtime 维度不符报错,PP-OCRv5 `textline_ori` 分类器固定输入实为 `[3,80,160]`;mobile/server 两档同值,server 档理论同架构但尚未真机验证)。

**视频帧入口(`ocr_extract_frame`)**:前端 `<canvas>` 截帧 → base64 PNG 回传 → host 落临时文件(`*.tmp` 写 + 同卷 rename,`ocr_commands.rs:330-336`,遵守项目派生落盘纪律)→ 走同一 worker 通路 → finally 语义删除(成败都删,`ocr_commands.rs:352-355`)。输入尺寸上限:base64 解码前 `≈89MB`、解码后 64MB(`OCR_FRAME_DECODED_MAX_BYTES`,`ocr_commands.rs:39-41`)。

**第 4 个付费插件(架构外扩说明,见 §7)**:OCR 走 `exotic` 授权门(`ensure_ocr_authorized`,`ocr_commands.rs:66-85`),四态映射:`Authorized`→放行,`LicenseExpired`→`ocr_license_expired`,`AvailableUninstalled`/`InstalledUnlicensed`→`ocr_unlicensed`,其余→`ocr_unavailable`。**`download_ocr_models` 刻意不过此门**(J8 裁决,`ocr_commands.rs:385-390`):模型字节是公开 ONNX 权重,允许"先下后购"。

### 3.4 远程校对(proofread)

与 `ai-worker` **完全解耦**——host 直接经 `reqwest` 调用用户自配的 OpenAI 兼容端点。配置(`base_url`/`model`)存 `config.toml`(A2 决策唯一真源),API key 存 OS 凭据库(`keyring`,服务名 `KEYRING_SERVICE`,账户名 `proofread_api_key`,`proofread_commands.rs:17-25`)、**绝不落 DB 明文**。五命令:`get_proofread_config`(43)、`set_proofread_config`(69)、`set_proofread_key`(87)、`clear_proofread_key`(96)、`proofread_chunk`(109,前端分块调用,产出 track-changes 差异)。

### 3.5 线程/进程模型总表

| 阶段 | 归属 |
|---|---|
| DB 领取/让步/落库(Producer/Writer) | `tokio::spawn_blocking` 承载的阻塞线程,host 进程内 |
| 缺缓存现场解码 | rayon 全局池(`par_iter`),host 进程内 |
| CLIP/人脸/OCR **推理本体** | `ai-worker` 独立子进程,`ort` Session(CPU EP 或 DirectML EP) |
| 语义搜索打分 | rayon 并行点积,host 进程内(`spawn_blocking`) |
| 远程校对 HTTP 调用 | tokio async(`reqwest`),host 进程内 |

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令一览(错误码统一走 `AppError`,不泄漏内部字符串;详情见 `Spec10`-IPC 契约篇)

| 分类 | 命令数 | 代表命令 |
|---|---|---|
| CLIP(`ai_commands.rs`) | 15 | `semantic_search_cmd`、`start_ai_analysis`、`detect_ai_provider`、`get_ai_status` |
| 人脸(`face_commands.rs`) | 23 | `get_face_status`、`confirm_faces`/`reassign_faces`/`reject_faces`/`merge_face_persons`、`recluster_faces` |
| OCR(`ocr_commands.rs`) | 4 | `ocr_status`、`ocr_extract_image`、`ocr_extract_frame`、`download_ocr_models` |
| 校对(`proofread_commands.rs`) | 5 | `get_proofread_config`、`proofread_chunk` |

OCR 稳定错误码样例(`ocr_commands.rs:196-211`):`ocr_decode_failed`(IoError/MalformedInput)、`ocr_engine_failed`(ResourceLimit/InternalError)、`ocr_worker_failed`(兜底)、`ocr_model_missing`(类型化判别,非字符串匹配,`worker_client.rs:442-449`,2026-07-23 深审#1 修复项)。

### 4.2 不变量清单

| 不变量 | 位置 | 为什么(违反会怎样) |
|---|---|---|
| host 零 ort/tokenizers/ndarray | `src-tauri/Cargo.toml:89`(`default-features = false`) | 若 host 链接 ort,失去进程崩溃隔离——推理引擎 panic/GPU driver 挂死会直接杀死 UI 进程;编译期物理保证比运行时约定更硬 |
| SCRFD/ArcFace 非商用轨物理不编译 | `crates/scrollery-ai-core/Cargo.toml:57`(`face-noncommercial = []`)+ `src/face.rs`/`face_profile.rs` 逐处 `#[cfg(feature = "face-noncommercial")]` | InsightFace 权重仅限非商业研究许可;必须编译期物理排除而非运行时不可达,否则商业二进制里仍嵌入受限模型代码/字符串(合规审计红线,Part4-T1) |
| CLIP 与人脸共享唯一 GPU 分析槽,互斥 | `state.rs:169,289-290`,`GPU_OWNER_AI`/`GPU_OWNER_FACE` | 两条流水线若同时跑满 GPU,会互相拖慢且难以诊断;槽是 check-and-claim 原子操作 |
| `RunTokenSlot`「槽空=保留终态发布权」 | `state.rs:148,153`(定义见 `state.rs:317-389`) | 槽被新一轮占用时旧轮收尾不得再动全局状态,否则用户点"停止再开始"时旧轮的迟到清理会静默打断新一轮 |
| 终态门控 = `is_cancelled` 非 finish-bool | `ai/pipeline.rs:99-118` 注释 | AI/人脸流水线的完成语义比 thumbnail 更简单(无需"finish-bool"式二次判定),`!token_outer.is_cancelled()` 直接判定自然完成 vs 取消,刻意不与 thumbnail 的模式趋同 |
| face `person.model_name` 与 `faces.model_name` 双向隔离 | `face_cluster.rs:20-26`(Part4-T6) | 切换人脸模型后旧轨的人物名册与新轨互不可见、互不销毁——旧轨标注是用户资产,不能被新模型的分析静默覆盖 |
| CLIP `ai_cache`(336px)与 face `face_thumbs`(640px)不共用 | `face_pipeline.rs:23-26` | 640px YuNet 输入若吃 336px 缓存会悄悄损害小脸召回率,是运行期正确性问题而非性能问题 |
| D2 顺序天条:先 CPU permit 后 GPU 令牌 | `worker_pipeline.rs:176`,`face_pipeline.rs` 同构 | 颠倒顺序会让派生解码抢占已拿到的 GPU 令牌导致其闲置浪费,或引入死锁风险(两资源不同序请求) |
| 批请求硬止损:同一请求至多 2 次尝试 | `worker_client.rs:48`(`MAX_ATTEMPTS`) | 无限重试会让系统性故障(如模型文件损坏)表现为无限挂起而非快速失败上报 |
| `merge_persons`/`confirm_face_assignment` 等写命令统一走 `write_blocking` | `face_commands.rs` 全体写命令 | 项目硬约束:rusqlite 调用不得跨 `.await` 持锁,且不得阻塞 tokio 运行时(AGENTS.md 硬约束) |

### 4.3 不可擅改项(标注出处)

- CPU EP 待对拍前不得开 DirectML(D-OCR-2,`docs/planning/2026-07-23-OCR文字提取/construction-plan.md:26`;理由:v5 rec 是 SVTR transformer 系,与 `engine.rs` 坑9 DirectML 对 BERT int64 Gather 静默算错同型风险)。
- `swap_rb` 未定案前 OCR 管线不宣称"可用"——定案仪式是 `crates/scrollery-ai-core/tests/ocr_golden.rs` 的 golden 对拍测试(同上 construction-plan.md:419)。
- builtin 插件三面过滤(exotic 平台契约)不得回退。
- `ai_backend` 配置键已退役,读到非 `worker` 值仅 warn 忽略,不做 schema 迁移(`runtime_config.rs:26-39`,T16 收束的既定退场路径,不是遗漏)。

## 5. 边界与失败模式

### 5.1 MF 同步 ReadSample 死等根因(跨子系统边界,历史教训)

人脸/AI 流水线在解码源涉及视频关键帧时,曾因 **Media Foundation 同步 `ReadSample` 错误路径丢事件死等**(0x80004005,MP4 触发,MKV 是受害者)导致整条流水线卡死——根因是 MF 的同步取帧调用在特定错误路径下不产生任何回调事件,调用方永久阻塞在等待上。修复为异步回调化(`IMFSourceReaderCallback`)。此教训**不特定于 AI 子系统**,是视频解码层(`src-tauri/src/video/media_foundation.rs`)的通用契约,AI/人脸流水线作为其消费方间接受益。详见 `docs/worklogs/2026-07-22-AI与人脸流水线根治/`(本篇不复制该线的完整根因论证,只引用结论)。**红线**:勿回退到同步取帧模式;泄漏隔离是该修复的故意设计,非缺陷。

### 5.2 纯 CPU 模式单核退化(遗留于 `scrollery-ai-core::engine`,as-built 位置已随 T16 迁移)

`docs/experience.md` §1 记录的教训——CPU Provider 若把单 Session 限制为 1 线程、而流水线消费者本身单线程,会导致 CPU 分析实际只用 1 个物理核心。**as-built 现状核实**:该修复(CPU pool_size=2、恢复 session 内部多线程)如今体现在 `crates/scrollery-ai-core/src/engine.rs:372-376`——`AiProvider::Cpu => 2`(其余 provider `=> 1`,避免多 session 抢占 GPU driver 锁),此逻辑在 T16 后已随推理面整体搬进 `ai-worker` 子进程,但结论未变。

### 5.3 GPU 大批次显存交换断崖

`docs/experience.md` §2:Batch Size 512 在 12GB 显存卡上触发 WDDM 显存换页(VRAM↔RAM PCIe 交换),耗时从 8s 飙升到 35s。修复即 §3.1 所述的硬上限 256(`pipeline.rs:295-301`)+ 自动档阶梯 + 前端危险值警告。**不变量**:无论前端传何值,`resolve_batch_size_from` 恒夹到 ≤256。

### 5.4 已知边界(逐条,代码位置见对应流程节)

| 边界 | 处理 |
|---|---|
| 模型文件缺失 | `AppError::AiModelNotLoaded`,message 引导"请先下载"(`worker_client.rs:723-733`) |
| worker 进程死亡/超时/协议违例 | 弃用实例重建,重发一次,硬止损后返错(`worker_client.rs:242-340`) |
| worker 空闲 300s | 自杀退出,防 host 失联留 VRAM 僵尸(`ai-worker/src/main.rs:38-40`,`IDLE_SELF_EXIT`) |
| worker 端会话丢失(`SessionExpired`) | host 清快照后由下轮 `ensure_session`/`ensure_ocr_session` 重建(`worker_client.rs:285-294,517-524`) |
| OCR 视频帧超大小上限 | 预检 base64 长度 + 解码后字节数双重上限,拒绝而非静默截断(`ocr_commands.rs:299-316`) |
| OCR 临时帧文件崩溃残留 | 每次调用清扫 mtime>1h 的陈旧文件(`purge_stale_ocr_frames`,`ocr_commands.rs:363-379`) |
| 人脸模型文件被手动替换成截断文件 | size 快检(不做全量 sha256,决策 brief 否决全量校验的常驻税)按未安装处理(`face_commands.rs:39-66`) |
| 跨模型向量维度混算 | `cosine_similarity` 长度不匹配返回 0.0(视为不相似)而非 panic(`search.rs:223-235`) |
| 语义搜索与嵌入落库并发竞速 | ensure_cache/取锁之间的空隙,一次重试后仍空才报错(J17,`search.rs:134-152`) |
| 远程校对未配置端点/未设置 key | 明确错误提示,不静默失败(`proofread_commands.rs:114-123`) |

## 6. 重建指引(从零实现)

### 6.1 依赖顺序

1. **`exotic_protocol`**(独立 crate,协议帧定义:`RequestBody`/`SuccessBody`/`FailureBody`/`WorkerErrorCode` 等)——host 与 worker 的唯一共同语言,先于两侧实现。
2. **`scrollery-ai-core` 契约面**(`profile.rs`/`face_profile.rs`/`ocr_profile.rs`/`face_types.rs`/`embedding.rs`/`decoded.rs`,不含 `inference` feature)——host 与 worker 共同依赖的"模型是什么"知识,不含"如何推理"。
3. **`scrollery-ai-core` 推理面**(`engine.rs`/`clip.rs`/`face.rs`/`ocr/`,`inference` feature 内)——真正的 ort Session 池、预处理、推理调用。
4. **`ai-worker` 可执行体**(`crates/exotic-workers/ai-worker/`)——消费推理面,实现协议帧的 stdin/stdout 循环、握手、空闲自杀、panic 捕获。
5. **host 侧 `AiWorkerClient`**(`worker_client.rs`)——spawn/会话/批请求封装,仅依赖契约面 + `exotic_protocol`。
6. **host 侧控制面**(`pipeline.rs`/`worker_pipeline.rs`/`face_pipeline.rs`/`face_cluster.rs`)——DB 领取/攒批/派发/落库/聚类,零推理知识。
7. **IPC 命令层**(`ai_commands.rs`/`face_commands.rs`/`ocr_commands.rs`/`proofread_commands.rs`)——最后接前端。

### 6.2 外部 crate

| crate | 版本 | 用途 | 归属 |
|---|---|---|---|
| `ort` | 2.0.0-rc.12(`load-dynamic`+`directml`+`download-binaries`+`copy-dylibs`) | ONNX Runtime 绑定 | `scrollery-ai-core`(`inference` feature) |
| `ndarray` | 0.16 | 张量操作 | 同上 |
| `tokenizers` | 0.21(`fancy-regex`,禁默认特性) | CLIP 文本分词 | 同上 |
| `image` | 0.25(jpeg/png/webp) | 推理前预处理解码 | 同上 |
| `imageproc` | 0.27 | OCR 几何(轮廓/旋转矩形/透视 warp) | 同上 |
| `crossbeam-channel` | 0.5.15 | SessionPool 有界 channel | 同上 |
| `half` | 2 | f16 嵌入缓存 | host(`ai/search.rs`) |
| `rayon` | (workspace 版本待核实) | 并行点积/聚类打分/批量派生 | host |
| `reqwest` | 0.12(rustls) | 模型下载 + 远程校对 HTTP | host |
| `keyring` | 3 | OS 凭据库存 API key | host(`proofread_commands.rs`) |
| `ring` | 0.17 | 随机 fingerprint/nonce 生成 | host(`ocr_commands.rs::random_hex`) |

### 6.3 坑与教训(链 experience.md)

- CPU 推理单核退化(§1)、GPU 大批次显存交换(§2)、原图尺寸误区(§3)——见本篇 §5.2/§5.3。
- MF 同步 ReadSample 死等——见本篇 §5.1,链 `docs/worklogs/2026-07-22-AI与人脸流水线根治/`。
- ORT 路径劫持(experience §6):环境问题须在发生 CWD 复现,绝对路径不进仓库配置。
- 空 User-Agent 被 WAF 直接 403(`download/mod.rs:101-104`,2026-07-23 实测钉死,非 cookie 问题)——通用下载引擎恒带 UA。

### 6.4 验收

| 验证面 | 命令/路径 |
|---|---|
| Rust 单测(CLIP/人脸/OCR 契约) | `cargo test -p scrollery --lib`(涉及 `ai::` 模块的单测,如 `worker_client.rs:814+`、`face_cluster.rs`、`search.rs`、`search_control.rs`、`ocr_registry.rs:139+`) |
| ai-worker 独立编译 | `cargo build -p ai-worker` |
| face-noncommercial 编译门 | `cargo build --features face-noncommercial`(对照默认构建确认 SCRFD/ArcFace 符号缺失) |
| clippy | `cargo clippy --workspace`(项目硬约束) |
| GUI 真机验收 | ⏸ 未做(本篇仅覆盖代码 as-built,不含真机手测结论) |

## 7. 关联

- 上游正典:`../refactor_2026/Part4_AI与人脸插件化.md`(AI/人脸插件化的原始决策与权衡,理由链见其 §3.x)。**注意**:Part4 部分字段的权威性已转移——worker 化的收束里程碑(T16 起 host 恒 worker-only、进程内 ort 路径删除)在 `../refactor_2026/Part6_插件平台与exotic收尾.md` 落定,读 Part4 时若与 Part6 冲突,以 Part6 + 当前代码为准。
- **OCR 是原文之后新增的第 4 个插件**:Part0/Part4 的原始规划以"三插件"为架构假设(exotic 冷门格式插件三态);OCR 子系统(2026-07-23 立项)是该假设之外的新扩展,复用同一 `exotic` 授权/Availability 框架(`ensure_ocr_authorized`,§3.3),但插件位数已突破原设计的隐含上限——本篇明确点出这一架构外扩,后续新增第 5 条能力线时应参照 OCR 的接入方式而非把"三插件"当硬约束。
- 相关设计:`docs/experience.md` §1-3(AI 推理踩坑,本篇 §5.2/5.3 引用)。
- 相关规格篇:[Spec01 数据层](./Spec01_数据层.md)(`ai_embeddings`/`faces`/`persons` 全表定义权威处)、`Spec10`(IPC 契约,错误码全量清单权威处)。
