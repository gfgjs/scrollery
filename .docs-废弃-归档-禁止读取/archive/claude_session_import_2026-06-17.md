# Claude 会话导入摘要（2026-06-17）

> 📦 **已归档(2026-07-10 文档治理补标)**:历史会话上下文快照(2026-06-17 导入),仅溯源用。

本文件由 Codex 根据本机 Claude Code 项目会话生成，用于把历史上下文迁移到当前项目文档中。原始日志位于：

- `%USERPROFILE%\.claude\projects\D--photoapp-picasa-next\memory\*.md`
- `%USERPROFILE%\.claude\projects\D--photoapp-picasa-next\*.jsonl`

导入范围：

- 12 个 Claude memory 文件。
- 27 个 Claude jsonl 会话。
- 重点读取了最近 AI/模型导出、视频/音频/文档/网络盘、侧边栏、画廊性能与文件树相关会话。

## 项目基线

Picasa Next 是一个极限性能、跨平台的媒体资产管理器，目标是十万到百万级资源的流畅浏览与语义检索。

技术栈：

- 后端：Rust + Tauri 2。
- 前端：Vue 3 + Vite + TypeScript strict + Pinia。
- 数据库：SQLite，`rusqlite`，写连接 `Mutex<Connection>`，读连接 `r2d2` 连接池。
- AI：ONNX Runtime，DirectML/CPU 后端，Chinese-CLIP 为当前主线。

项目设计文档主要在 `plan-docs/`：

- `implementation_plan_v1.2.md`：主架构。
- `architecture_notes.md`：活文档总结。
- `perf_hardening_plan_v2.md`：百万级性能加固。
- `feature_expansion_plan_v1.md`：视频、文档、音频、收藏、网络盘等扩展路线，后续多处实现以此为准。
- `重要提示.md`：长期开发约定。

长期约定：

- Rust 错误处理使用 `thiserror`。
- SQL 必须使用参数绑定。
- 前端使用 Composition API + TypeScript strict。
- 代码注释、日志、打印文本要求中英双语。
- Git commit 使用中文；只有用户明确要求时才 push。
- 发现冗余、错误或架构/功能/UI 建议时主动说明。

## 当前重要状态

导入时工作区不是干净状态，存在既有未提交/未跟踪内容，Codex 不应无故覆盖或回退：

- `src-tauri/Cargo.toml` 已修改。
- `.claude/` 未跟踪。
- `.models/` 未跟踪。
- `export_clip_l14_336_onnx.py` 未跟踪。
- `validate_clip_l14_336_onnx.py` 未跟踪。

Git 分支为 `dev`，导入前本地相对 `origin/dev` ahead 2。最近可见提交包括：

- `9d79d40` 修复语义搜索结果错乱：文本编码器强制走 CPU（DirectML 静默算错文本模型）。
- `56bcfbe` Revert "修复语义搜索结果不准：对图像向量做均值中心化（修复 CLIP 各向异性）"。
- `15b30b3` dev 不再优化打包，提升编译速度。
- `7be783e` 新增设置开关：可切换是否提取视频封面 / 关键帧。
- `489b78a` 曾尝试图像向量均值中心化，后续已 revert。

## 最近 AI 与模型会话

### DirectML 文本编码器问题

最近一次语义搜索错乱的真实根因不是模型质量，也不是阈值，而是 DirectML 静默算错 Chinese-CLIP 文本编码器。

现象：

- 大规模新需求实现前，AI 搜索准确度高，20% 阈值可用。
- 切换/适配多模型后，搜索“黄色的猫”“白色的猫”等出现明显无关结果。
- 图像编码侧基本正常，文本查询向量被污染。

结论：

- 多模型重构换用 `eisneim/cn-clip` ONNX 后，文本编码器是 BERT，含 int64 token id、embedding `Gather` 等算子。
- 这些算子在 DirectML 上可能不报错但输出错误。
- 修复方向是文本编码器强制 CPU，图像编码器仍可按既有策略使用 DirectML/GPU。
- 不要再通过图像向量均值中心化修“准确度”，该方向已 revert。

### 模型切换架构

AI 模型切换已落地两层：

- Layer A，commit `6a3e7d6`：`ModelProfile` 抽象，`clip.rs`、engine、pipeline、search 改为 profile 驱动，新增 `list_model_registry`、`set_active_model`，切换前校验已安装。
- Layer B，commit `3153bba`：程序内模型下载闭环，`.part` 临时文件、原子改名、HTTP Range 续传、大小与 sha256 双校验、镜像回退、Channel 进度。

当前原则：

- `ai_embeddings` 以 `(item_id, model_name)` 为主键，不同模型向量可共存。
- 不同模型 embedding 不能互比；切换模型意味着该模型下全库重算，但切回用过的模型可以复用已有向量。
- MVP 优先 Chinese-CLIP 同族多尺寸：B/16、L/14、L/14@336，共用 BERT 词表、mean/std、输入名，只改 `image_size` 和 `embed_dim`。
- SigLIP/Jina/OpenAI CLIP 等异构家族涉及 BPE/SentencePiece 或其它 tokenizer，应作为后续阶段处理。

### L/14@336 ONNX 导出

最新 Claude 会话任务是：用户已把 `clip_cn_vit-l-14-336` 放到项目 `.models`，要求导出 ONNX。

该会话在继续阶段触发 Claude rate limit，最后未完整收尾。当前项目中已有未跟踪脚本：

- `export_clip_l14_336_onnx.py`
- `validate_clip_l14_336_onnx.py`

会话中提到的关键导出经验：

- ViT-L/14@336 输出维度为 768，输入尺寸为 336。
- 需要分别导出图像编码器和文本编码器，和现有 B/16 契约保持同类输出：未归一化特征，Rust 侧再做 L2。
- Torch 新导出路径对 `(None, text)` 占位不稳定，使用旧路径 `dynamo=False`。
- opset 需要 >= 17，以避免 LayerNorm 在 fp16 转换和 ORT 优化中触发融合崩溃。
- Windows 大 ONNX 单次写盘可能触发大文件写入问题，思路是先导出到内存，再用 ONNX external data 保存。
- fp16 产物不能只验证“能加载”，还要和 PyTorch fp32 参考输出逐条余弦相似度比对。若文本 fp16 数值不稳定，应回退 fp32。
- 文本编码器尤其容易出现“能跑但向量坍缩/算错”，必须严查。

## 功能实现记忆

### 侧边栏手风琴

`AppSidebar.vue` 已从大文件拆成 VSCode 风格手风琴结构：

- `AppSidebar.vue`：薄容器。
- `AccordionSection.vue`：通用手风琴段。
- `sections/{Library,Tools,Folders,Management}Section.vue`：具体区块。
- `useSidebarSections.ts`：展开状态、sticky 注册与持久化。
- `usePointerDrag.ts`：指针拖拽。
- `useConfirm.ts` + `ConfirmDialog.vue`：Promise 式确认框。

关键不变量：

- `AccordionSection` 是双根 fragment，不能包一层外层 div。
- 每个 section header 必须成为 `.sidebar__scroll-area` 的直接子元素，sticky 才能跨区块堆叠。
- collapse 动画元素 `.acc-body` 必须零 padding，padding 放到 `.acc-body__inner`，否则收起末尾会顿一下。
- sticky header hover 状态也必须保持不透明背景，避免滚动内容透出来。

文件树扩展：

- 左侧文件树已支持目录下显示文件列表。
- `list_directory_files` 只列直接文件，排除 deleted 与 Live Photo companion。
- 目录 row 自身另有一层 sticky，所有目录共用一个 top，靠 DOM 后出现者覆盖先出现者。

### Tauri 拖拽

Tauri v2 `dragDropEnabled` 默认拦截 webview 内 HTML5 DnD，导致内部拖拽 `drop` 不可靠。

项目选择：

- 保留原生 OS 文件拖入能力。
- 项目内拖动用 pointer events 实现。
- 不要为了内部拖拽关闭 `dragDropEnabled`。

现有使用：

- 文件树移动/复制。
- TOOLS 排序。

注意：

- 拖拽 ghost 必须 `pointer-events: none`，否则会挡住 `document.elementFromPoint`。
- `tauri.conf.json` 改动需要完整重启 Tauri dev，Vite HMR 不够。

### Gallery 视图不是 router 驱动

主画廊不是靠 vue-router 参数驱动实际过滤。

实际状态源：

- `uiStore.activeSmartAlbum`
- `uiStore.activeDirectoryId`
- `uiStore.activeCollection`

三者互斥，`setSmartAlbum`、`setActiveDirectory`、`setActiveCollection` 会互相清理。

`useJustifiedLayout.ts` 读取这些状态组装 `MediaFilter` 后调用 `compute_layout`。新增画廊过滤视图时，应新增 uiStore active 状态、更新 compute/watch 和后端 filter，不要只依赖 route params。

### 视频 P2

Media Foundation 视频后端落在 `src-tauri/src/video/`，由派生框架与 enricher 调用。

关键约定：

- `MFVideoFormat_RGB32` 内存顺序是 B,G,R,X，转 RGBA 必须重排。
- RGB32 可能为 bottom-up，负 stride 需要翻转行序。
- 读取旋转信息后，帧需要转正；90/270 度交换显示宽高。
- 使用 `IMFSourceReader` + `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`，输出系统内存。
- `MFStartup` 进程内 `Once`，不调用 `MFShutdown`。
- MF 不支持 mkv/webm/flv/ogv，这些留给 perf/FFmpeg。

接线：

- 视频比例/旋转在 enricher 回填。
- 视频封面与关键帧走派生框架。
- `useDerivationAutoStart` 在前端自动启动派生，否则视频封面不会出现。
- Google Motion Photo 和 Apple Live Photo 播放路径已复用 `get_companion_video_url`。
- 关键帧 sprite 为 10 帧水平条带，前后端常量必须一致。

后续新增两个设置开关：

- 是否提取视频封面。
- 是否提取视频关键帧。

### 后台让步模型

派生任务和 AI 可以对交互让步：

- 使用 `AppState::note_interaction` / `is_interactive`。
- 浏览时暂停或节流，空闲后恢复。

enrichment 绝不能用交互让步：

- enrichment 是用户等待的导入工作。
- enrichment 进度事件每 2 秒触发布局计算。
- 布局计算又会上报 interaction。
- 如果 enrichment 也让步，会被自己的反馈循环饿死。

正确方案：

- enrichment 使用 `reserved_core_pool()`，保留 1 核给单线程布局。
- 派生流水线 producer/consumer/writer 协调线程用 `std::thread::scope`，不要用 `rayon::scope` 长时间阻塞工作线程。

### 文档 P4

P4 文档子系统已完成：

- 文档缩略图。
- 文档阅读器。
- 替换。
- 版本。
- 远程 AI 校对。

关键点：

- epub 封面后端 `zip` 解析 OPF 后走派生框架。
- pdf/svg 缩略图由隐藏的 `DocThumbRenderer.vue` 前端离屏渲染并回写，不依赖派生流水线启动。
- `get_pending_derivations` 需要排除 pdf/svg 的 `doc_thumb`，避免后端消费者错误接手。
- `/doc/:id` 由 `DocumentViewer.vue` 按格式分发 `TextReader`、`PdfReader`、`EpubReader`。
- `usePager.ts` 抽象翻页。
- epub 运行时仍需优先排查 CSP/iframe/asset 协议问题。
- 版本文件存 `<appData>/documents/<item>/<id>.<ext>`。
- SQLite 列名 `"replace"` 需要加引号。
- API key 存 keyring，服务 `picasa-next`，账户 `proofread_api_key`。

### 音频 P3 与网络盘 P5

P3 音频已完成：

- `src-tauri/src/audio/mod.rs` 使用 `lofty` 读取标签、封面、歌词。
- 音频元数据走 enricher，不走派生。
- 音频封面走 `audio_cover` 派生。
- `get_audio_detail` IPC 懒读标签和歌词，按需抽取全分辨率封面。
- 前端 `/audio/:id` 和 `AudioPlayer.vue` 已接入。

P5 网络盘状态：

- 8A 可用：Windows 映射盘/UNC 路径通过原生文件夹对话框添加，`normalize_root_path` 保留 UNC 前缀。
- 8B 地基已落：`storage_backends`、`scan_roots.backend_id`、`StorageBackend` trait、`LocalFs`、`WebDavBackend`、CRUD/测试 IPC、`NetworkStorageSection.vue`。
- 8B 待办：scanner 仍走 `walkdir/std::fs`，尚未改为通过 `StorageBackend` 遍历远程；自定义 Tauri URI Range 流式代理尚未做。

## 会话索引

按最近修改时间倒序整理：

| 时间 | 会话 | 主题 | 结果/线索 |
| --- | --- | --- | --- |
| 2026-06-17 23:15 | `1bf572c3` | L/14@336 导出 ONNX | 会话因 rate limit 中断；留下导出/验证脚本，需继续验证产物。 |
| 2026-06-17 22:52 | `40e3564d` | AI 搜索错乱 | 确认 DirectML 静默算错文本编码器；文本强制 CPU；均值中心化方向已撤销。 |
| 2026-06-17 22:05 | `b3dc06c8` | 视频封面/关键帧开关 | 设置页新增两个开关，后端派生逻辑接入。 |
| 2026-06-17 21:49 | `633b6354` | P3 音频 + P5 网络盘 | P3/P5 已完成并提交；另补后台让步模型优化。 |
| 2026-06-17 21:47 | `2f239f41` | 视频封面与悬停预览 | 修复自动派生触发与悬停播放黑闪。 |
| 2026-06-17 21:47 | `252e5161` | 左侧文件树显示文件与目录 sticky | 新增目录直接文件列表和文件树目录 sticky。 |
| 2026-06-17 21:47 | `5520511d` | AI 模型切换与下载闭环 | Layer A/B 已落地；B/16 下载闭环打通；L/14/L/14@336 资产待导出自托管。 |
| 2026-06-17 19:04 | `eb9135a3` | 项目架构总结 | 输出过一次整体架构概览。 |
| 2026-06-16 11:13 | `947dcd53` | feature expansion 方案优化 | 明确 lite/perf、三分发包、轻量优先规则。 |
| 2026-06-16 09:45 | `7767c702` | P4 文档 | 文档缩略图/阅读器/替换/版本/AI 校对完成。 |
| 2026-06-16 09:45 | `18307837` | P2 视频 | Media Foundation 视频后端、封面、关键帧、悬停预览等完成。 |
| 2026-06-15 23:48 | `c3a5d88e` | P0 基础 | 非图片尺寸修复与 V4 schema 地基。 |
| 2026-06-15 23:48 | `1eea83e2` | P1 快速见效 | 悬停自动播放与 gallery/collection 等相关接线。 |
| 2026-06-15 23:47 | `2aeeb6b8` | 画廊定位、文件树联动、拖拽增强 | 多批优化完成，包含空父目录滚到首个有媒体子目录、跨根移动/复制拖拽。 |
| 2026-06-14 19:20 | `d7475557` | 同名目录跳转错误 | 改用目录唯一标识定位，而非 folder name。 |
| 2026-06-13 21:54 | `29471b80` | 侧边栏重构后细节修复 | 修复折叠动画末尾顿挫。 |
| 2026-06-13 17:42 | `765d9190` | 侧边栏手风琴重构 | 形成当前 sidebar 架构与 sticky 不变量。 |
| 2026-06-13 17:11 | `975f5ffe` | 侧边栏重构早期会话 | 与后续重构同主题。 |
| 2026-06-13 13:16 | `1aeca891` | sidebar 拆分 | 将大组件拆为顶部、图库、工具、文件夹、管理、底部等模块。 |
| 2026-06-13 13:08 | `e757c50d` | Claude Code 默认提示词 | 讨论 `CLAUDE.md` / settings 机制。 |
| 2026-06-13 13:08 | `f129ef71` | sidebar 动画与 sticky 交互 | 侧边栏 UX 调整早期会话。 |
| 2026-06-12 19:03 | `d641240d` | 60px 缩略图全屏卡顿 | 讨论 `img.decode()` 与画廊性能。 |
| 2026-06-12 16:27 | `80b2504d` | 拖拽、工具排序、性能 | 读取重要提示，形成 dev conventions、drag-drop gotcha 与项目 overview memory。 |
| 2026-06-11 16:32 | `1aa2333e` | 架构审查与 Electron 可行性 | 输出架构审查，尝试 B1 平移 bug 修复。 |
| 2026-06-11 16:32 | `2c6768df` | 文件树拖拽方案 | 给出文件树移动/复制与工具排序方案。 |

## 下一步接续建议

近期最自然的接续点是 L/14@336 模型导出：

1. 先检查 `export_clip_l14_336_onnx.py` 与 `validate_clip_l14_336_onnx.py` 当前状态，确认是否因 Claude 中断留下半成品或编码问题。
2. 验证 `.models` 中权重文件、已导出的 ONNX/extra_file 是否存在。
3. 跑导出脚本前注意内存峰值；文本 fp16 必须和 PyTorch fp32 做余弦相似度校验，不合格回退 fp32。
4. 产物稳定后，补 model registry 的 L/14@336 assets、size、sha256、profile，并确保 `embed_dim=768`、`image_size=336`。
5. 重新跑 `cargo check`、`npm run typecheck`，必要时跑 AI 搜索 smoke test。

若接续普通功能开发，应优先遵守本文件“功能实现记忆”中的坑位，尤其是 sidebar fragment sticky、Tauri pointer drag、gallery uiStore 驱动、enrichment 不让步、DirectML 文本模型禁用等约束。
