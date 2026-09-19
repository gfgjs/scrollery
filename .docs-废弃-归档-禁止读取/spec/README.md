---
id: 2026-07-24-spec-README
status: active
type: index
line: asbuilt-spec
created: 2026-07-24
---

# Scrollery 施工规格集(as-built 重建级)

本目录是 Scrollery 的 **as-built 重建级施工规格**:以当前 `src-tauri/`、`src/` 代码为唯一事实源,详尽到数据模型、核心算法、不变量与边界,面向**人类工程师**与**低能力 LLM 编码代理**两类读者——目标是「只读本集 + 所引 `path:line` 代码,即可理解、扩展、乃至从零重建各子系统」。

> **与重构正典的关系(务必先懂)**:`docs/refactor_2026/Part0–Part8` 是 2026-06-26 的重构**计划**正典,讲「**为什么**这样设计」(定位/盈利/防护/架构决策理由),是稳定资产。但它成文后大量施工已使代码前进/漂移——schema 从 V10 到 **V23**、产品名从「Picasa Next」定案为 **Scrollery**、色彩管理/视频硬解/OCR 第 4 插件等**正典无对应章节**。本集描述**现状是什么、怎么建**,引用正典讲理由、不复制其论证;**计划与代码冲突,一律以代码为准**,并在文中点明落差。

---

## 篇目一览(17 篇)

| 篇 | 覆盖 |
|---|---|
| [Spec00 产品与架构全景](./Spec00_产品与架构全景.md) | 门面:Scrollery 定位、整机分层架构、进程模型(host + ai-worker 子进程 + exotic worker + webview)、技术栈、构建变体/渠道、本集导航 |
| [Spec01 数据层](./Spec01_数据层.md) | SQLite 30 表 schema(V23)、迁移引擎、`NATURAL_CMP`/`TREE_SORT_KEY` 自定义排序、连接池/WAL/并发契约 |
| [Spec02 扫描与画廊](./Spec02_扫描与画廊.md) | 扫描引擎/增量/删除检测/卷探测、布局引擎(keyset 分页)、时间轴/minimap 轴体系(后端数据流) |
| [Spec03 缩略图与派生](./Spec03_缩略图与派生.md) | WebP/sprite/LRU/thumbhash、视频/文档/音频派生流水线、缓存治理与孤儿防线 |
| [Spec04 图像与色彩管线](./Spec04_图像与色彩管线.md) | 裁剪/旋转/调色、moxcms ICC CMS、色域 sRGB/P3/DCI-P3、EXIF/XMP(**正典无色彩章节,净新增**) |
| [Spec05 视频与音频](./Spec05_视频与音频.md) | Media Foundation 异步取帧/DXVA 硬解/XVP、自研播放器、lofty 音频(**视频硬解净新增**) |
| [Spec06 AI/人脸/OCR](./Spec06_AI人脸OCR.md) | ai-worker 隔离、CLIP 语义搜索、embedding cache、增量最近质心人脸聚类、审批流、PP-OCRv5 OCR(第 4 插件)、远程校对 |
| [Spec07 文档与阅读器](./Spec07_文档与阅读器.md) | txt 编码检测/分章/简繁/书签/版本、foliate EPUB 分片 + PDF.js + shiki、文档缩略图 |
| [Spec08 存储/备份/导出/文件操作](./Spec08_存储备份导出文件操作.md) | 卷/WebDAV、备份恢复(相位机/原子写/验签)、导出(mtime 保留)、文件操作(路径穿越防守) |
| [Spec09 插件平台与 exotic](./Spec09_插件平台与exotic.md) | Catalog/Registry/授权门控/Ed25519 trust/gate、格式并集、两仓拓扑(canonical + Copybara 内部文件过滤) |
| [Spec10 IPC 与错误契约](./Spec10_IPC与错误契约.md) | **229 命令**分组全表、`AppError` **40 变体**稳定错误码、capabilities 权限、前端 `ipc.ts` 契约层 |
| [Spec11 前端架构](./Spec11_前端架构.md) | Vue app shell/16 路由/19 Pinia store/BucketVirtualScroll 虚拟滚动/6 主题/i18n/命令体系/IPC 层/UI 原语 |
| [Spec12 配置/状态/日志](./Spec12_配置状态日志.md) | `config.toml` 真源 + 热加载、AppState/RunTokenSlot/调度、JSON 日志/span/诊断包/worker 汇入 |
| [Spec13 构建/发布/商业化](./Spec13_构建发布商业化.md) | lite/perf 变体、三渠道 cfg 互斥、自托管 CI、updater、许可激活、核心免费 + 插件买断、四层防护 |
| [Spec14 不变量与约定](./Spec14_不变量与约定.md) | 跨切面**红线总目录**(10 域,每条附「为什么 + 违反后果」)、experience 24 条教训、CSP 规范≠现状 gap |
| [Spec15 流水线全景](./Spec15_流水线全景.md) | **横切总览**:15 条数据处理流水线对照表(触发/阶段链/引擎/并发/现状)+ 跨流水线基础件 + CI 工程流水线 |

---

## 怎么读(两条路径)

**人类工程师**:先读 [Spec00](./Spec00_产品与架构全景.md) 建立整机心智 → 按需读目标子系统篇 → [Spec14](./Spec14_不变量与约定.md) 速览红线。要扩展某功能:读该功能篇 + [Spec01 数据层](./Spec01_数据层.md)(涉数据)+ [Spec10 IPC](./Spec10_IPC与错误契约.md)(涉前后端接口)+ [Spec14](./Spec14_不变量与约定.md)(不变量)。

**低能力 LLM 编码代理**:施工前**必读** [Spec14 不变量](./Spec14_不变量与约定.md)(红线清单)+ [Spec00 架构](./Spec00_产品与架构全景.md) + 目标子系统篇;涉数据加 [Spec01](./Spec01_数据层.md),涉 IPC 加 [Spec10](./Spec10_IPC与错误契约.md)。每篇的「**重建指引**」节是照做入口(依赖顺序 + crate/包 + 坑 + 验收命令)。

---

## 全集约定

- **七段骨架**:每篇 = 概览 / 数据模型与状态 / 关键流程与算法 / 契约与不变量 / 边界与失败 / 重建指引 / 关联。
- **`path:line` 锚点**:每条行为/结构描述都锚到代码,可直跳;标「**待核实**」处表示未在仓内核实(如跨进程细节、易漂移常量、尚未落地的计划项)。
- **单一权威**:全表/全命令/全错误码/不变量各有归属篇(schema→Spec01、IPC/错误码→Spec10、红线总目录→Spec14),他篇就近摘要 + 链接,不重复全量。
- **as-built 纪律**:描述代码现状;与正典计划冲突以代码为准并点明。
