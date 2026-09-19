---
id: 2026-06-26-Part0_总纲与产品定稿
status: active
type: canon
line: refactor_2026
created: 2026-06-26
---

# Picasa Next 重构方案 · Part 0 · 总纲与产品定稿

> 版本：v1（2026-06-26 起草）
> 状态：**产品定稿待最终审批**（§0–§13 全部回填完成；唯一待用户拍板 = 最终产品名 §10.7）
> 作者基准：以 18-agent 侦察 workflow 的**实测核实**（含实跑 `cargo test --offline`）+ 用户逐项确认为准。**旧 plan-docs / 记忆 / 默认提示一律不轻信**，凡与本文冲突以本文及代码实测为准。

---

## §0 文档体系与使用方式

### 0.1 本重构文档集（`docs/refactor_2026/`）

| Part | 文件 | 职责 | 依赖 |
|---|---|---|---|
| **Part 0** | `Part0_总纲与产品定稿.md` | 产品定稿、竞品、版本/打包、盈利、防护、直销分发、重构总纲、全局约定 | — |
| Part 1 | `Part1_数据层.md` | schema 重设计、迁移事务化、DAO 补全、向量存储/索引、ANN 选型、**卷模型 SCHEMA_V10（§6）** | Part0 |
| Part 2 | `Part2_扫描与画廊流水线.md` | 删除检测（卷在线守门）、SourceChanged 元数据更新、增量扫描、**卷探测/插拔监听/离线态（§6）**、百万级布局、坐标平移修复、keyset 分页、时间轴滚动条 | Part1 |
| Part 3 | `Part3_缩略图派生与GPU引擎.md` | CPU 解码修复、WicEngine 注册、缓存治理、派生产物清理、mac 原生媒体层 | Part1, **Part2**（消费布局缓存 `apply_thumb_results` 回填口 + SourceChanged 失效钩子） |
| Part 4 | `Part4_AI与人脸插件化.md` | CLIP/人脸从「进程内」迁到「sidecar worker 插件」、ANN 检索、中文双档、人脸批量审批、模型切换安全 | Part1, Part6, **Part2**（`invalidate_derived_for_item` AI/face 钩子）, **Part3**（`ai_thumb` 缓存喂 CLIP） |
| Part 5 | `Part5_前端体验重构.md` | 虚拟化三层、稳定多选、星级/颜色标签、虚拟相册、首启向导、主题、IPC 错误统一、组件拆分、插件商店 UI | Part2, **Part6**（插件商店/gate 消费 EntitlementProvider IPC）, **Part1**（color_label 列）, **Part3**（`asset://`/thumbhash/阅读器）, **Part4**（语义搜索/人脸审批命令 + gate） |
| Part 6 | `Part6_插件平台与exotic收尾.md` | exotic→**全插件平台地基**、`fetch_exotic_registry`、AES 加密权重、EntitlementProvider 抽象、Part4 前端 gate | Part0 |
| Part 7 | `Part7_发布工程.md` | Cargo workspace、CI 测试门控、tauri-plugin-updater 热更新（普通依赖）、代码签名/公证、版本号 | Part6 |
| Part 8 | `Part8_商业化与分发.md` | 许可基础设施、定价落地、支付渠道、商店上架、官网/落地页/动图、获客 | **实现依赖** Part0/Part4(SCRFD 隔离/模型自托管)/Part5(gate)/Part6/Part7；**上架门禁依赖** 改名+合规(§10.5/§10.7) |

### 0.2 每个 Part 文档的规范结构

每个 Part **自包含**，供独立新会话执行，统一含：
1. **目标与范围**（本 Part 解决什么、不解决什么）
2. **现状实测**（相关代码现状 + `文件:行` 证据 + 与旧 plan 偏差）
3. **设计方案**（数据结构/接口/算法/线程模型，无省略）
4. **分步实施清单**（带优先级 P0–P3、工时估、依赖）
5. **风险与回滚**
6. **验收标准**（测试/可观察行为）
7. **给执行会话的提示词**（可直接粘贴启动新会话的 system/task 提示）

### 0.3 旧文档处置

- `docs/archive/implementation_plan_v1.2.md`、`feature_expansion_plan_v1.md`、`perf_hardening_plan_v2.md`、`architecture_notes.md`、`exotic_format_plugin_plan/`：**降为历史参考**，多处与代码实测不符（见 §1.3）。重构期以本文档集为唯一权威。〔2026-07-10 文档治理:上述文件已物理归档至 `docs/archive/`(exotic 目录随迁移阶段 2 入内),文首均带状态横幅。〕
- 🔴 **文档集内部权威（第 6 轮独立代码核验后，2026-06-27 定稿）**：**§3 正文即唯一权威，各 Part §8 仅历史留痕（审查决策溯源）；动工一律以 §3 正文为准。** 〔第 5 轮复审曾因「§8 裁决未完整回写正文」临时改判「以 §8 为准」，但该规则与各 Part 横幅 + Part6 §8.7「正文为唯一权威、不再以 §8 为准」**直接冲突**（第 6 轮核验 CANON-01）。第 6 轮已把残留未回写项**全部回写正文**：`SessionInitBody` 字段（Part6 §3.2.1 / Part4 §3.1.2，第 5 轮已回写）、GpuLimiter 会话生命周期令牌（Part6 §3.6 / Part4 §3.6，P0-2 回写）、AES `salt` 位置（Part6/Part4 §3.7.2，CANON-02 回写 = `ModelBlob.enc_salt` 非密文前缀）、G4 批量协议 / G6 错误码（Part6 §1.1，CANON-03 回写）——**正文与 §8 已无分歧，「以 §8 为准」兜底规则随之废止**。〕今后任何新 §8 裁决须**同步回写对应 §3 正文**方算定稿，杜绝再生分歧（缺「正文↔§8 一致性」自动校验前，回写是人工纪律红线）。各 Part 顶部「状态」一律视为 **定稿待执行**。外部法律/平台政策结论的来源与复核状态见 [external_references.md](external_references.md)。
- `architecture_notes.md` 中的「关键不变量」「数据流」仍有效，可继续引用，但「已知问题/坐标平移」等进入 Part2 正式处理。

---

## §1 重构背景与侦察核实结论

### 1.1 为什么重构

项目早期架构文档由旧模型（opus 4.6）产出，**未经市场调研**；中途产品重心从「单一图片」扩为「图片/视频/音频/文档四媒体」，又增「冷门格式 + 付费插件」，导致 **数据库设计与代码实现与架构文档大幅偏离**。本次重构以一轮 18-agent 侦察（竞品调研 + 全后端/前端子系统测绘 + plan-docs 偏差核实）为起点，目标：把方案重新对齐到**实测的代码现状**与**经市场验证的产品设计**。

### 1.2 整体健康度（实测）

- 后端能力 ~85%，前端 ~70%，**发布工程 ~15%**。
- 后端 **162 个 `#[test]` 实跑全绿**（`cargo test --offline`；🔴 此为 src-tauri 主体口径，**workspace 全量 = 184**：另含 exotic-protocol 14 + psd-worker 8，见 [Part7](Part7_发布工程.md) §2.1），但分布**极偏**（实测）：exotic 占 **113（主体 70%）**，db 15 / ai 7 / thumbnail 6 / scanner 2，而 **video / derive / engine / ipc / 前端 = 0 测试**。
  > 🔴 **「后端 ~85% 完成」须按此读**：绿灯集中在 exotic（付费插件子系统），**核心四媒体路径（视频取帧 / 派生流水线 / 解码引擎 / IPC 层）接近零验证** → 「85% 写完」≠「85% 已验证」。触碰这些零测试子系统的重构，须先补特征化测试锁住现行行为，见 §11.4.4「测试地基硬门槛」。
- 三条交付阻塞：① epub 扩展名未登记（P4 文档链静默死）；② CI 无 `cargo test`（162 测试对 PR 零门控）；③ 热更新（`tauri-plugin-updater`）完全未引入。

### 1.3 旧文档/记忆 vs 代码实测的矛盾（**重构必须纠正**）

| # | 旧说法（plan/记忆） | 代码实测 | 影响 |
|---|---|---|---|
| 1 | exotic「约 60% 未跑通」（用户认知） | **后端基本完成、162 测试绿**；install 端到端可用、运行期信任链闭合、付费授权可用 | 资源**不应**再投 exotic 后端 |
| 2 | exotic「install 端到端可用」 | 真**缺 `fetch_exotic_registry` IPC**：全新设备 Registry 缓存为空→装不了插件 | Part6 P0 补此命令 |
| 3 | 「百万级流畅浏览」（核心卖点） | **未达成**：坐标平移（>~25 万项）滚动错位 bug 已搁置，靠 `SAFE_MAX=10M` 休眠回避 | Part2 正面解决 |
| 4 | 记忆「人脸仅 F1 完成」（MEMORY.md 索引行） | **F1–F7+收尾全完成**（编译级验证），默认轨 YuNet+SFace 已 OpenCV 对拍；仅缺 F8b(SCRFD 对拍)+TODO2(模型切换迁移) | 人脸已可用，缺插件化+激活切换 |
| 5 | — | 向量检索是**暴力 O(N) 全表扫描**（[queries.rs](../../src-tauri/src/db/queries.rs):1874），百万级 ≈1–2GB 常驻 RAM + 秒级延迟 | Part1/Part4 引 ANN |
| 6 | 记忆「P4 文档全落地」 | **epub 整链静默死**（[format.rs](../../src-tauri/src/utils/format.rs) 漏登记扩展名，1 行修复）；`document_meta` 表 V1 起无 DAO | Part1/Part3 修 |
| 7 | plan「视频用 FFmpeg sidecar」 | 实为 **Media Foundation**（[enricher.rs](../../src-tauri/src/scanner/enricher.rs):351）；FFmpeg 仅注释占位 | plan 失效 |
| 8 | plan「TIFF tokio 超时保护」 | ~~实际无超时~~〔✅ 已修（commit `9938651`）：detached spawn + `recv_timeout` 5s 硬超时，[metadata.rs:19-39](../../src-tauri/src/scanner/metadata.rs#L19) 守卫、:80-87 包裹 TIFF 路径，+3 单测；原引 :54 行号已漂移〕 | ✅ 已修（Part2 T10，2026-07-02 回写） |
| 9 | 记忆暗示 `ai_hq_cache_enabled` 默认开 | 代码默认 `false`（[schema.rs](../../src-tauri/src/db/schema.rs):33），CPU 解码优化形同虚设 | Part3 处理默认值/引导 |
| 10 | — | IPC 143 命令中 **~72 个返回 `String`** 而非 `AppError`；前端无法区分 Cancelled/NotFound/Db | Part5 统一 |
| 11 | — | migration 9 个版本块**无事务包裹**→DDL 中途崩溃=半迁移静默不可恢复 | Part1 P0 |

> **方法论纪律（写入每个 Part）**：记忆分「索引层 vs 正文层」，索引先腐化（如矛盾 #4 是 MEMORY.md 索引行停在 F1，正文已记到 F7）。核实必须读**正文 + 代码**，凡「完成度/测试数/某功能是否存在」用 Grep/Read 实测，不采信文档断言。

---

## §2 产品定位与目标用户

### 2.1 一句话定位

**本地优先 · 隐私第一 · 完全离线的跨平台四媒体智能管理器** —— 继承经典 Picasa 的文件夹树哲学，以**百万级流畅画廊**、**离线 AI 语义搜索 + 人脸识别**、**付费格式/能力插件**为差异点；填补 Google Photos 无独立桌面端 + Picasa 停服后的遗产用户缺口。

### 2.2 核心定位约束（用户确认，骨架级，覆盖旧 plan）

1. **完全离线运行**，不依赖任何云服务；**不考虑订阅制**（隐私 + 离线无法云端计费）；AI api 接入无限期延后（AI 一律本地 ONNX）。
2. **极致轻量核心包（目标 <10MB，弹性）**：四媒体**基本格式**预览，**尽用系统原生 API**（Win: WIC / Media Foundation / Shell；mac: Image I/O / AVFoundation / PDFKit / QuickLook）。⚠️ **<10MB 是设计目标非硬性红线，可视需求适当放宽——【功能实现】+【用户体验】为最高优先级**；当原生能力不足、需内置必要依赖以保体验时可放宽，但仍以「能外置则外置（插件化）」为默认倾向。
3. **完整体验 = 核心 + 三个可下载插件**：① AI 分析（CLIP）② 人脸聚类 ③ 冷门格式。
4. 平台：**Windows + macOS 桌面并重**；iOS/Android 后置做浏览 companion。
5. 后期需可上架 **微软 Store、Steam**（易上的优先；App Store 延后）——架构现在预留（见 §9）。

### 2.3 目标用户（按优先级）

1. **Picasa 遗产用户（核心）**：35–55 岁，依赖文件夹树导航、批量重命名、**零破坏磁盘结构**；Google Photos 停止无限存储后正寻替代品。
2. **隐私敏感的半专业摄影师**：拒绝云上传，要本地 AI 标注 + 人脸 + RAW 管理（Eagle/Excire 用户群）。
3. **多媒体收藏家**：管理图/视/音/文四类、库规模 10 万–百万级的 Win/Mac 重度用户。
4. **家庭相册管理者**：多设备多年份、旧扫描照片整理，需模糊日期 + 批量人脸标注。
5. **设计师/内容创作者**：需多格式（含冷门 RAW/专业视频）+ 快速检索 + 批量工作流。

---

## §3 竞品提炼与差异化卖点

### 3.1 竞品扫描结论（6 路调研）

- **消费级**（Google Photos / Apple Photos / Eagle / Mylio / Excire / 原 Picasa）：时间轴滚动条、虚拟相册、文件夹树、批量人脸审批、星级评分是反复被验证的留存功能；订阅制与「云抽象删文件夹」是流失主因。
- **专业 DAM**（digiKam / Lightroom Classic / ACDSee / XnViewMP / Photo Mechanic）：缩略图缓存金字塔、目录数据库、RAW 内嵌 JPEG 快速通道（选片速度）、keyset 分页是大库工程关键。
- **自托管 AI**（Immich / PhotoPrism / Ente）：本地 CLIP 语义、人脸聚类、缩略图/占位图（thumbhash）、开源+付费云模式；但中文优化与商用人脸许可普遍语焉不详。

### 3.2 差异化卖点（相对竞品）

1. **中文语义搜索原生**：Chinese-CLIP 双档（ViT-B/16 轻 + ViT-L/14 高质量），用户可选精度/速度档；Immich/Excire/Eagle 均无中文专项优化。
2. **Picasa 文件夹哲学 + 现代 AI**：唯一同时具备「文件夹树零破坏磁盘结构」（Picasa 遗产）与「本地 CLIP 语义 + 人脸」（现代 AI）的桌面客户端。
3. **百万级无卡顿画廊**：三层虚拟化（Section 年月 / 行 / 行内 justified）+ SQLite WAL + `asset://` 协议直通缩略图，Tauri 原生无浏览器 IPC 瓶颈。
4. **全离线隐私第一**：所有 AI 推理本地 ONNX，零数据上云。
5. **付费插件生态（已有技术地基）**：exotic 子系统 Ed25519 签名 + 激活链路可用；市场上少有以此方式扩展能力的工具。
6. **商用安全人脸**：默认 YuNet(MIT) + SFace(Apache) 双档；高精度 SCRFD+ArcFace 明确标 `commercial_ok=false`。
7. **四媒体统一管理**：图/视/音/文同一画廊、同一时间轴、同一搜索入口。
8. **买断无订阅焦虑**：核心永久免费，插件一次性买断。

### 3.3 必须避免的反模式（竞品翻车教训）

- 强制云同步/不支持纯本地（Google Photos 流失主因）。
- 相册操作破坏磁盘结构（必须保留文件夹树视图）。
- 多选/拖拽手势**无序频繁乱改**（Google Photos 多次删改引发信任危机）→ 须**有序演进**:开发期可随实测/市场验证迭代,但不无依据乱改。🔴 2026-06-30 修正(用户约束「开发期不冻结契约」):旧表述「一旦发布即冻结」已取代——教训是「别**乱**改」,不是「禁止改」;未经实测比较/市场验证的东西不预先定死。
- 首屏展示全部功能面板（Mylio 痛点）→ 渐进式披露。
- AI 推荐不可覆盖（Apple Photos 高频投诉）→ **用户手动选择优先级最高**。
- 大库首次导入无进度/不可中断续传（Excire 已解决而竞品仍痛）→ v1 必须解决。
- 缩略图生成阻塞 UI / 崩溃（Adobe Bridge 致命缺陷、Picasa 崛起原因）→ 全程异步分批 + 进度。
- **按媒体类型收费**（系统 API 零成本支持四媒体却收「视频浏览费」→ 必被贴 crippleware，CapCut 翻车前车之鉴）。

---

## §4 功能优先级矩阵

> MoSCoW：must=v1 必须，should=v1 力争，could=有余力，later=预留接口后置。已存在的标 ✅，缺失/需重做标 🔨。

### must（核心留存 + 百万级硬门槛）

| 功能 | 现状 | 理由 |
|---|---|---|
| 文件夹树视图（磁盘结构零破坏） | ✅ 部分 | Picasa 遗产用户核心诉求，迁移门槛最低 |
| 虚拟画廊三层滚动（Section 年月→行虚拟化→justified） | 🔨 行级已有，坐标平移 bug | 百万级流畅硬门槛，当前未真正达成（矛盾#3） |
| SQLite WAL + thumbnails 分离 + 三档缩略图 | ✅ 部分 | 大库滚动卡顿底线 |
| **时间轴滚动条**（右侧按年月拖拽 + 热力图） | 🔨 缺 | 大库导航必备，竞品高频验证，成本低 |
| 稳定多选（框选/Shift/Ctrl）+ 批量操作菜单 | 🔨 部分 | Google Photos 反面教材；差异化底线 |
| 虚拟相册/Collection（引用模式，不移文件） | ✅ 部分 | 三竞品共识 |
| keyset 分页 + 复合索引 | 🔨 | OFFSET 百万行 500ms+，keyset 恒定 <5ms |
| `asset://` 协议直通缩略图（绕过 IPC 字节） | ✅ | Windows WebView2 IPC 传大图慢，asset 协议是唯一解 |
| RAW 内嵌 JPEG 快速通道 | 🔨 | Photo Mechanic 核心力，首次导入快 100× |
| 星级评分（1-5）+ 颜色标签 + 数字键快捷键 | 🔨 缺 | Apple Photos iOS27 验证需求长存；Picasa 星标用户直接迁移 |
| 四媒体类型预览（图/视/音/文）走系统原生 | ✅ 部分 | 核心包 <10MB 基础 |

### should（v1 力争）

人脸识别批量审批 UI（grouped likely matches，已有 YuNet+SFace 地基）🔨；画廊布局切换 Grid/Justified + 密度滑块 🔨；首次启动向导（3 步）🔨；ThumbHash 占位符（DB 存、前端即时渲染）✅部分；AI 自动关键词（CLIP + 文件夹名双通道）🔨；中文语义搜索双档 ✅部分；增量扫描（size+mtime）🔨；后台 Activity Manager（优先级队列 + 让步）✅部分；亮/暗主题 ✅部分。

### could（有余力）

Survey 光桌模式；Action/快捷键宏；Fuzzy Date 模糊时间；FTS5 全文检索（替代 LIKE）。

### later（预留接口后置）

Catalog 年度分库 + ATTACH（>20 万触发）；移动端 companion；WebDav/云盘（当前只有连接测试 `storage::count_entries`，远程遍历未接入）；Windows Shell 缩略图 Handler（IThumbnailProvider）。

---

## §5 打包模型（架构级版本形态）—— 本次重构最大架构决策

### 5.1 总体

**极致轻量核心包（目标 <10MB，弹性）+ 三个可选付费插件**。核心永久免费、无试用限制、无功能门控；三插件均一次性买断、离线授权、可应用内下载或官网离线包双击导入，**统一走同一套签名 Registry + sidecar worker 框架**（复用并升级 exotic 子系统）。

> **体积优先级（用户 2026-06-26 明确）**：<10MB 是设计目标，**非硬性红线**；**功能实现 + 用户体验最高优先**。当系统原生能力不足、需内置必要依赖以保证体验时可适当放宽体积；但默认倾向仍是「能外置则外置」（推理/大模型/重解码一律插件化，绝不进核心）。换言之放宽的是「核心保留少量必要原生增强库」，不是「把 ort/模型塞回核心」。

```
┌─────────────────────────────────────────────┐
│ 核心包 <10MB (Win installer 6-9MB / mac dmg 5-8MB) │
│  · 四媒体基础浏览，全走系统原生 API            │
│  · 不含 ort / 任何 ONNX / tokenizers           │
│  · SQLite(bundled) / reqwest / ring / zip / lofty │
└───────────────┬─────────────────────────────┘
                │ exotic 框架(签名Registry+sidecar worker+安装)
   ┌────────────┼────────────┬─────────────────┐
   ▼            ▼            ▼
[AI插件]     [人脸插件]    [冷门格式插件]
ai-worker    face-worker   psd-worker(已成)+…
ort+CLIP     ort+YuNet     纯解码器
代码~10MB    +SFace        无ONNX
+模型700MB   30-50MB单包    5-15MB
(blob分离)
```

### 5.2 核心包内容（系统原生覆盖）

- **图像**：JPEG/PNG/BMP/GIF/TIFF/ICO —— Win 走 [wic_engine.rs](../../src-tauri/src/engine/gpu/wic_engine.rs)，mac 走 Image I/O CGImageSource。HEIC/WebP/AVIF **运行时检测降级**（缺失时 fallback image-rs + 提示装系统扩展，而非硬性要求）。
- **视频帧**：MP4/MOV/AVI/MTS/WMV/3GP/TS —— Win [media_foundation.rs](../../src-tauri/src/video/media_foundation.rs)，mac AVFoundation `AVAssetImageGenerator`（`appliesPreferredTrackTransform=true` 处理旋转）。MKV(H264/AAC) Win MF 可开；WebM/FLV/OGV 排除（前端显 unsupported badge）。
- **音频**：元数据 + 封面 —— `lofty`（纯 Rust，已实现）。
- **文档**：PDF 走前端 pdf.js 离屏渲染；SVG/EPUB 走 WebView（已实现，epub 待修扩展名登记）。或 mac 走 PDFKit/QLThumbnailGenerator。
- **去 ort**：从 `Cargo.toml` 移除 `ort` 依赖 + `tauri.conf.json` 移除 4 个 ORT DLL `bundle.resources` 条目（省 ~20–25MB）；`tokenizers` 随 ort 移入 AI 插件（再省 2–4MB）。

### 5.3 三插件规格

| 插件 | sku | 内容 | 交付 | 体积 |
|---|---|---|---|---|
| **AI 分析** `ai-clip` | `ai-clip-pro` | `ai-worker`(ort + tokenizers + Chinese-CLIP ViT-B/16 fp16)；文本编码器**固定 CPU**（DirectML 对 BERT int64 Gather 静默算错，进程隔离后约束不变） | Registry 流式下载 / .ppx；**代码包与模型 blob 分两步**，blob 各含 url/sha256/size | 代码~10MB + 模型~700MB（可选 ViT-L/14-336 fp32 HQ ~1.7GB 独立 blob） |
| **人脸聚类** `ai-face` | `ai-face-pro` | `face-worker`(ort + YuNet ~2MB + SFace ~20MB，MIT/Apache 商用)；可选 SCRFD+ArcFace 非商用变体须另一 sku | 同上；模型小，可合入代码 zip 单包 | 30–50MB |
| **冷门格式** `exotic-formats` | `exotic-formats-pro` | `psd-worker`(已完成) + 后续 RMVB/RAW 等 worker；无 ONNX，纯解码器 | 同上；单包，无 blob 分离 | 5–15MB |

### 5.4 插件分发（双通道统一管线）

**包格式 `.ppx`**（= 改扩展名的签名 zip：内含 `package-manifest.json` + worker 二进制 + 可选小模型；大模型 blob 不打进 .ppx，单独提供或安装后按需拉取）。

两条路径**最终汇入同一** `install_staged_zip`（[installer.rs](../../src-tauri/src/exotic/installer.rs):69）→ verify_and_extract → catalog_subset_check → commit_install → DB upsert：
1. **应用内下载**：`fetch.rs` 流式下载 + 实时 SHA-256 → `staging/<plugin_id>.zip` → install_staged_zip。
2. **官网离线包双击**：Tauri `RunEvent::OpenedWithFiles` / deep-link 捕获 `.ppx` 路径 → 新命令 `install_exotic_plugin_from_path` → 复制到 staging → 从 zip 读 manifest 取 plugin_id → 同一 install_staged_zip。**离线路径同样强制 Ed25519 验签 + sequence 防回滚**（不可绕过）。

**文件关联（无管理员）**：
- Windows：`tauri.conf.json` `bundle.fileAssociations` 加 `{ext:"ppx"}`；NSIS `installMode=currentUser` 写 `HKCU\Software\Classes\.ppx`，**无 UAC**；`.ppx` 本体是 zip 无可执行 stub → SmartScreen 不弹窗；`UCPD.sys` 仅保护 http/https/.pdf，不影响 .ppx。
- macOS：`Info.plist` `CFBundleDocumentTypes` 声明 UTI `org.picasa-next.ppx`；LaunchServices 用户级注册；安装后 `lsregister -f` 刷新；网络来源 .ppx 首次弹 Gatekeeper 确认属正常。

### 5.5 关键架构决策（进 Part4/Part6/Part7）

1. **worker 进程统一**：AI/人脸/冷门格式三类插件全部走 sidecar 独立进程 + exotic 框架（WorkerConn/Supervisor/exotic-protocol stdio 帧协议）。**拒绝进程内 dlopen**（Tauri #8090 未解决 + ort load-dynamic 限制 + 无崩溃隔离，三重障碍）。
2. **Registry 统一**：三插件共用 [registry.rs](../../src-tauri/src/exotic/registry.rs) `VerifiedRegistry` + `installer.rs` `install_staged_zip`，`plugin_id` 参数化区分；[coordinator.rs](../../src-tauri/src/exotic/coordinator.rs) `evaluate_run()` 门控已参数化可复用。
3. **ort 完全外置 + 模型 blob 分离**：`RegistryEntry` 新增 `model_blobs[]`（url/sha256/size/kind=`model_weight`），模型权重独立分步下载，避免单 zip 超 `InstallLimits.max_total_size=512MB`；对 `model_weight` 豁免压缩比检查（ONNX 近 1:1）。
4. **GPU 资源协调**：ai-clip 与 ai-face worker 进程隔离，GPU 显存互斥由 Coordinator 在 `plugin_id` 层协调（同时只允许一个 AI worker 持 GPU session）。
5. **协议扩展**：`exotic-protocol` 增 `InferenceRequest` / `EmbeddingResponse`(f32[512]=2KB/图) / `FaceDetectResponse`(bbox+关键点)；或单独建 `ai-protocol` crate 隔离演化。
6. **macOS FFI 桥**：引 `objc2` crate（~200KB）实现 Image I/O + AVFoundation + PDFKit 绑定；**工作量约 Windows 路径的 2–3 倍**（Win+mac 并重的直接代价，进 Part3/Part7 工时）。

> **开放技术决策（在 Part4 实测拍板，非阻塞）**：① ai-clip 与 ai-face 合并为单 `ai-worker`（省进程/共享 ort 初始化）还是分离（崩溃隔离更彻底、GPU 更可控）—— 按显存实测定；② ONNX 默认档位 ViT-B/16 fp16（默认）vs ViT-L/14（可选 HQ 升级包）；③ macOS 初期 CPU-only 还是开 CoreML（需 GPU entitlement + 运行时测试，建议 P6.5 平台签名后再开）。

---

## §6 卷可用性感知（移动盘/网络盘插拔处理）—— 设计原则

> 第七轮调研产出。**动机**：原数据模型隐含「扫描根 = 永久在线的绝对路径」，被移动盘打破——盘符漂移（Win `E:`→`F:`、mac `/Volumes` 重名追数字）+ 拔出会让 Wave0 的差集删除**误标整盘 `is_deleted`**（会上社区头条的数据事故）。**统一抽象**：移动盘与网络盘都是「时而离线的卷」→ `volumes` 表统一建模（`local`/`removable`/`network`），一并覆盖已有网络盘连接测试雏形。

**卷稳定身份（替代盘符）**：

| 平台 | 稳定 ID | 取法 | 稳定性 |
|---|---|---|---|
| Windows | 卷 GUID `{…}` | `GetVolumeNameForVolumeMountPointW` 截 `\\?\Volume{GUID}\` | 跨盘符/换口/重插稳定；重格式化或换机变（GUID 存注册表 MountedDevices，非磁盘） |
| macOS | 卷 UUID | DiskArbitration `kDADiskDescriptionVolumeUUIDKey` | APFS/HFS+ on-disk、跨机稳定；exFAT/FAT32 内核合成（克隆盘可能碰撞） |
| 网络盘 | 规范化 UNC | 现有 `normalize_root_path` `//host/share` | — |

盘符/挂载点仅存为 `last_mount_path`（提示 + 运行期路径重组），**非身份键**。

**插拔监听**：Win `WM_DEVICECHANGE`(`DBT_DEVTYP_VOLUME`，须真顶级隐藏窗口、非 message-only)；mac DiskArbitration `DARegisterDiskAppeared/Disappeared`（专用 CFRunLoop 线程）；30/60s 轮询兜底；后台线程 `AppHandle.emit` 推前端。crate：Win `windows-sys`（已缓存）、mac `disk-arbitration-sys`（依赖链短，联网 `cargo fetch` 后锁入）。

🔴 **硬规则「离线 ≠ 删除」**：`availability` 三态 `online`/`offline`/`missing` 与 `is_deleted` 严格互补。`offline`（卷离线）→**绝不写 `is_deleted`、不跑差集**；`missing`（卷在线但文件确实没了）→ 唯一可标删路径，且须先确认卷在线。**Wave0 差集删除：SQL 最外层 `WHERE volume_id IN (SELECT id FROM volumes WHERE is_online=1)` + 写删前逐条再校验在线（防 TOCTOU）；`fast_scan` 入口若卷离线即 `return Ok(0)`。**

**SCHEMA_V10**：新增 `volumes`(`stable_id` UNIQUE/`label`/`kind`/`last_mount_path`/`last_seen`/`is_online`) + `scan_roots` 加 `volume_id`/`volume_subpath`（`path` 降级为「最后已知路径」）+ `media_items` 加 `volume_id`/`volume_relative_path`/`availability` + 2 个部分索引。`cache_key` **不变**（rel_path 基，免疫盘符，缩略图缓存 100% 命中）。迁移 `availability DEFAULT 'online'` 零成本。

**重挂载**：`probe_volumes` 比对 GUID/UUID——盘符变只更新 `last_mount_path` + 重授 asset scope；按卷 ID 增量补扫（幂等 upsert，不重复入库）。asset scope 只增不撤（Tauri 无撤销 API），靠 IPC 读文件前查 `availability` 拦截（返回 `VOLUME_OFFLINE`）。

**UX**：离线项灰显 + `CloudOff` 角标 + **缓存缩略图仍可见**；打开原图离线 → `VOLUME_OFFLINE` → 「请插入设备 `<label>`」弹窗（非破图）；重连自动恢复；设置「已知卷」面板（重命名/删除/重扫）；缺失文件手动重链接（`content_hash` 优先匹配）。

**落点**：Part1（SCHEMA_V10 + DAO）/ Part2（`scanner/volume_probe.rs` + 扫描编排离线守门 + 监听线程）/ Part5（UX）/ 本节（原则）。
**开放决策**：克隆盘 UUID 碰撞策略；差集删除自动 vs 手动「查找缺失项」（digiKam 式，更安全，建议倾向手动/确认制）；`missing→deleted` 是否加「连续 N 次缺失」缓冲；disk-arbitration crate 最终选型（联网后定）。

---

## §7 盈利方案（方案 C，已拍板）

### 7.1 模式

**核心永久免费（四媒体基础浏览）+ 三插件各自一次性买断。** 明确否决方案 A（按媒体类型收费 → crippleware）与纯方案 B（全功能买断 → 700MB 插件不该捆绑强下）。

**为何 C**：① 技术成本与收费逻辑精确匹配（系统 API 零成本支持四媒体，AI/人脸/冷门格式才是独立研发的大组件）；② Part3 授权基础设施（ring Ed25519、[license.rs](../../src-tauri/src/exotic/license.rs)、[registry.rs](../../src-tauri/src/exotic/registry.rs)）已完整，三插件买断直接复用，开发成本近零；③ `sku='free'` 直接激活、付费 sku 走完整验签，前端免三套 gate；④ 获客漏斗最宽（核心免费→大基数→AI 演示价值→插件转化）；⑤ 完全离线，Ed25519 本地验签零网络。

### 7.2 定价（直销基准价）

| 项 | 定价 |
|---|---|
| 核心（Win+mac 四媒体基础浏览） | **永久免费**，无试用期限制 |
| AI 分析插件（CLIP 语义/自动标签） | $12.9 / ¥89，一次买断，含模型更新 1 年 |
| 人脸聚类插件（YuNet+SFace） | $9.9 / ¥69，一次买断 |
| 冷门格式插件（PSD/PSB/RAW/RMVB…） | $9.9 / ¥69，一次买断 |
| 三插件包（bundle ~20% 折） | $24.9 / ¥168（对标 Eagle 全价 $34.95） |
| 大版本升级 | 50% 折（对标 Beyond Compare），小版本永久免费 |
| **中国区** | **CNY 区域定价（USD 40-50%）**，降低盗版动机的最有效非技术手段 |

> **盈利叙事的诚实边界**（写入 Part8）：AI/人脸所用 CLIP/YuNet/SFace 权重**部分来自公开仓库**（`gficcg/clip_cn_vit-onnx`、`opencv_zoo`）。因此插件卖的是**「开箱即用的集成 + 中文优化 + 自动更新 + 体验」**，**不是「秘密权重」**；加密权重（§8 层1）主要挡懒人 + 为未来私有/微调模型铺路，对公开权重不形成强护城河。定价/文案据此，避免「独家模型」式夸大。

### 7.3 渠道（详见 §9 直销分发）

当前唯一渠道是**自家直销 `direct`**（零抽成、完整 Ed25519 授权 + keyring 凭据）。**上架微软 Store / Steam 不做事先代码或配置预埋**：真要上架时按届时平台现状重评（现行实现现状见 §9.5，策略与时序见 §9.6）。支付：国际 FastSpring/Paddle（MoR 代缴税）；**中国区必须接微信支付/支付宝**，否则 CNY 定价形同虚设。

---

## §8 防破解 / 防白嫖：四层纵深（已拍板：层1 + 层2）

### 8.1 前提认知（诚实，写入 Part6/Part8）

纯本地离线软件，防破解只有「**威慑**」与「**抬高成本**」，**无「绝对阻断」**——任何本地 license 校验都能被逆向 patch（NOP 验签跳转）。Rust+Tauri 原生二进制比 Electron 明文 JS 逆向成本高一个数量级，但不消除此天花板。**目标 = 让白嫖比付费更麻烦**（$9.9 插件，白嫖要 2 小时反汇编，99% 的人会直接买）。**云端校验/kill-switch 明确否决**（与完全离线+隐私第一冲突，且法律/口碑代价高）。

### 8.2 四层（全部复用 exotic Ed25519 框架，零新外部 crate；`ring` 已含 hkdf+aead，`zeroize` 已在本地 registry）

| 层 | 机制 | 挡住 | 隐私代价 | 工时 | 状态 |
|---|---|---|---|---|---|
| **层0 威慑** | Ed25519 `verify_strict` + 用途分离双信任根 + 签名 Registry 单调防回滚 + 逐文件 SHA-256 包清单白名单 + 启动前完整性复核([installer.rs](../../src-tauri/src/exotic/installer.rs):275，🔴 **SEC-02 已修**：`EXOTIC_PSD_WORKER_PATH` 验签旁路加 `#[cfg(debug_assertions)]`、Release 不编入，杜绝经环境变量加载未签名 worker) + zip 安全解包 + OS keyring 存 token | keygen 伪造✅ 篡改 token✅ 跨插件复用✅ 装后替换 worker✅（含 env 旁路，SEC-02 已堵）供应链注入✅ zip 穿越/炸弹✅ Registry 回滚✅ | 零 | 0 | **已完成（162 测试绿）+ SEC-02 代码修复** |
| **层1 加密权重** | 尚未实现，不预留密钥字段、DTO 或安装分支；未来有真实加密权重需求时重新设计 | 当前授权仍用签名 token 与 keyring | 不新增网络/PII | 未立项 | 未来研究 |
| **层1b 设备绑定** | 并入层2 激活流程：激活时签发带 `subject_hash`=HMAC(license_id, cpu+disk) 的设备绑定 token（不含可伪造 MAC、不传服务器） | token 共享、VM 直拷 | 零（哈希不可逆，激活时本地算） | （并入层2） | 🔨 待做 |
| **层2 可选一次性在线激活**（P3，已选） | 平台代码签名（patch 版无法以正版外观流通）+ **购买时可选**联网激活一次（传 HMAC 哈希不传 PII，服务端签发设备绑定增强 token，之后**永久离线**）+ 激活时同步拉 CRL（退款吊销）。**默认离线激活仍可用** | 退款滥用、无限设备激活；提高分发门槛 | 仅激活时单次联网，传不可逆哈希，须隐私政策声明 | 签名证书 + 激活微服务 | 🔨 待做 |

### 8.3 诚实强度评级（写入 Part8 与对外文档基调）

- **挡住**：99% 脚本小子/keygen/共享/直接拷目录。
- **挡不住（不夸大）**：有动机的逆向工程师 patch、付费用户运行期内存 dump 已解密权重、VM 快照克隆、付费用户主动泄露。—— 这些是所有本地 AI 产品的共同上限，连联网也救不了内存 dump。

### 8.4 落地清单（→ Part6）

当前实现不含 AES 权重解密、`enc_seed` 或 `DerivedSecret` 预留。未来加密权重能力需独立定义信任边界、密钥生命周期及验证，不在现行授权/安装路径中提前铺设接口。当前仍保护签名 token 与系统 keyring，不引入周期联网校验或 kill-switch。

---

## §9 渠道与授权实现（EntitlementProvider 抽象；当前直销唯一）

> 核心原则：**当前只有直销 `direct` 一个渠道**（无渠道 feature、无渠道桩），做完整四层防护（§8）；`EntitlementProvider` 是**真实付费/免费两条实现**，不是给未来渠道预留的骨架。**上架（Store/Steam）不做事先代码或配置预埋**——真要上时按届时的平台现状重评，不提前塞入 `#[cfg]`、空桩、渠道枚举或预留列。

### 9.1 两个抽象 trait（升格现有 `LicenseSource`）

```rust
// crates/scrollery-plugin-api/src/lib.rs（跨 crate 契约叶 crate；实现分居主机与渠道桩）
pub trait EntitlementProvider: Send + Sync {
    fn evaluate(&self, plugin_id: &str, sku: &str, now: i64) -> LicenseStatus;
    fn activate(&self, plugin_id: &str, sku: &str, credential: &str, now: i64)
        -> Result<ActivationInfo, EntitlementError>; // credential = Ed25519 token / Store receipt / Steam receipt
    fn deactivate(&self, plugin_id: &str) -> Result<(), EntitlementError>;
    fn source_tag(&self) -> &'static str; // "direct" | "ms_store" | "steam"
}
pub trait PluginDeliverySource: Send + Sync {
    fn fetch_package(&self, plugin_id: &str, version: &str, dest: &Path) -> Result<PathBuf, DeliveryError>;
    fn is_available_offline(&self, plugin_id: &str) -> bool;
    fn channel(&self) -> DeliveryChannel; // DirectRegistry | SteamDepot | StoreBundled
}
```
`ExoticHost.licenses` 字段类型 `Arc<dyn EntitlementProvider>`（trait 与 DTO 在 `crates/scrollery-plugin-api`）。**两条真实实现**同在 `src-tauri/src/exotic/license.rs`：付费 `KeyringLicenseStore`（`source_tag()="direct"`，token 存系统 keyring、用编入 Host 的信任根公钥集**按原始 token 字节验签**，keyring 保留为唯一凭据存储）与免费/未授权回退 `FreeStubEntitlement`（`source_tag()="free"`，恒 `Unlicensed`，信任根解析失败时 fail-closed 降级）。组合根单点装配、无渠道分支；`channel_stubs.rs` 已随渠道退役删除。

### 9.2 各渠道交付与授权模型

| 渠道 | 打包 | 插件交付 | 授权 | 抽成 |
|---|---|---|---|---|
| **direct 官网直销（主）** | Win32 EXE/MSI（Tauri 原生） | `RegistryHttpDelivery`：HTTPS 拉 AES 加密 `.ppx` → installer 验签解密解包 | `KeyringLicenseStore`（完整四层，§8） | **0%**（自家支付） |
| 微软 Store | **未实现、不预埋**（首次上架时按平台现状重评） | 无 | 无 `MsStoreProvider` | （历史评估 15%） |
| Steam | **未实现、不预埋**（首次上架时按受众重评） | 无 | 无 `SteamProvider` | （历史评估 30%） |

### 9.3/§9.4 多渠道构建变体与「<10MB 核心」冲突化解 —— 整段删除（P16，2026-09-15）

原 §9.3 的三渠道 feature（`channel-direct`/`channel-msstore`/`channel-steam`）、`channel_stubs.rs` 渠道桩、互斥与零渠道 `compile_error!` 守卫、`DeliveryChannel`/`InstallSource` 枚举、CI 三渠道依赖树断言，以及 §9.4 依赖渠道变体的三条化解（MSIX 内置 worker、Store 禁二次 DRM 的 cfg 排除、Steam depot 跳过 AES）**均已随渠道退役删除**，正文整段移除、不再作为规范。现行实现现状见 §9.5，策略与时序见 §9.6。

### 9.5 渠道相关实现现状（原「多渠道预留清单」）

- **授权抽象与路由**：`EntitlementProvider` trait 与 DTO 落 `crates/scrollery-plugin-api`（叶 crate）；**两条真实实现**同在 `src-tauri/src/exotic/license.rs`——付费 `KeyringLicenseStore` 与免费/未授权回退 `FreeStubEntitlement`（fail-closed）。`source_tag()` 只区分 `"direct"` / `"free"`；**无** `channel_stubs.rs`、**无** `DeliveryChannel` 枚举、**无**按渠道 feature 选择的装配分支。`ExoticHost.licenses` 类型为 `Arc<dyn EntitlementProvider>`。
- `CatalogOffering`/`RawOffering`：**不**含 `store_product_id` / `steam_dlc_app_id`（渠道预留字段未落地；不预埋）。
- `Cargo.toml [features]`：**无** channel feature。现行 `default = ["custom-protocol","lite"]`、`lite = []`、`perf = ["ffmpeg","netfs"]`、`ffmpeg = []`、`netfs = ["dep:reqwest_dav"]`；无渠道 `#[cfg]` 门控。
- `installer.rs`：**无** `InstallSource` 枚举与渠道分支，`RegistryExpect` 是唯一路径（该文件注「唯一渠道 = 直销，空渠道 msstore/steam 已删」）。
- `main.rs`：**无** `SteamAPI_RestartAppIfNecessary` 占位块（未来上架不预埋）。
- **渠道归属**：代码侧恒 `"direct"`；DB 列（`exotic_plugins.entitlement_source` 等）以当前 schema 为准——本批不改 schema，统一由 schema 批次处理。

### 9.6 推荐渠道策略与时序

**主直销**（官网 Win32 EXE，0% 抽成，四层授权已成）→ **v1.0 后 3 月内上微软 Store EXE 路径**（Store 做流量入口，收款仍走官网，0% 抽成，规避 MSIX WACK 风险，插件购买引导至官网）→ **MSIX 待 Tauri #14935 修复后评估**（仅当 StoreContext IAP 的 15% 换更高转化时切）→ **Steam 视受众**（创作者社群可作第二曝光渠道，30% 抽成偏高、定位曝光非主收入）。**各渠道定价对齐**（Steam 不允许官网更便宜）；直销 license **不跨渠道迁移**（渠道隔离防权益纠纷，`entitlement_source` 记录原始渠道）。

> **开放决策（不阻塞重构，上架前定）**：① Steam 是否注册(🔴 2026-07-06 订正:原文「$400」系误记,Steamworks 官方文档实证 Steam Direct=**$100/产品**且累计毛收入 $1,000 后返还;重评触发条件=直销上线 30 天转化数据,详见 [2026-07-06 决策 brief](../decisions/2026-07-06-决策brief-渠道·开源边界·face校验.md))（视用户 Steam 习惯，先验证直销转化）；② MSIX 内置 worker vs 运行时下载（模型 >500MB 时重评）；③ 直销 vs Store IAP（Store 代缴 VAT 价值可能 >15% 成本，待 WACK 修复重评）；④ Steam DRM 弱（`RestartAppIfNecessary` 可内存绕过），高价插件可叠服务端 `CheckAppOwnership`；⑤ `steamworks 0.13` + `windows` 新 features 需先确认 `~/.cargo/registry` 已缓存或在线预取。

> **对 Part 划分的影响**：EntitlementProvider 升格归 **Part6**（插件平台，补丁式扩展不破坏 162 测试）；多渠道 CI 构建变体归 **Part7**；MsStoreProvider/SteamProvider 真实实现 + 商店提交归 **Part8**（含 winapp pack / SteamPipe 脚本、商品页、各 Provider ~2 天/个）。**音视频/文档（核心免费基础浏览）不使用 EntitlementProvider**；🔴 **AI/人脸/exotic 三付费插件经插件平台使用 EntitlementProvider + AES + gate**（Part4 §3.1/§3.7 消费 Part6 §3.8，非「不使用」）。

<!-- 哨兵: §9 已回填(第4轮 workflow wvhak6ove) -->

---

## §10 开源策略与依赖合规

### 10.1 开源决策（2026-09-14 更新，取代 2026-09-12 的 MPL-2.0 决议）

**第一方公开源码统一 AGPL-3.0-only**：前端 `src/`、Rust 主机 `src-tauri/`、随附现有 worker 及授权实现（含 keyring 直销授权与 Ed25519 验签）全部进公开镜像，采用 GNU Affero General Public License v3.0 only（标准正文不修改，SPDX: `AGPL-3.0-only`）。**公开源码统一许可**，不按功能模块区分；未进公开镜像的私有内容不适用，未来新组件一旦源码公开同样适用。**另设独立商业授权并行**：只依赖 AGPL 行使权利不需要购买商业授权，普通商业使用不因此必须付费。商业范围有两条独立路径——**官方商业版**面向普通终端用户（个人或公司），在接受 EULA 并满足其约定条件后取得商业授权；**OEM / 闭源服务授权**面向厂商、集成方与需要闭源集成或再分发的合作方，按需单独签约。两者范围不同、互不替代，但不要求同一方同时签署两份协议。第三方依赖与模型按各自原许可。决议存证见 [决策 brief:AGPL-3.0-only 与单独商业授权](../decisions/2026-09-14-决策brief-AGPL-3.0-only与单独商业授权.md)。

**为何 AGPL-3.0-only**：第 13 条要求**按 AGPL 运行且落入该条范围的修改版**在通过网络向用户提供交互服务时，向这些用户提供取得对应源码的机会；第三方依赖仍按各自原许可。版权人另授商业许可时，可就其覆盖的第一方源码给出不同于 AGPL 的替代条件，但不改变第三方条款。NAS Server / Web 尚属规划，**当前没有已发布服务端实现，也没有配套源码下载设施，不声称已经具备**；对应源码入口义务在实现并对外提供服务时落地。具体义务按 AGPL 正文执行，品牌边界见 `TRADEMARK.md`。

### 10.2 开 / 闭代码边界（精确）

- **源码开放不等于功能免费**：付费插件与载荷、官方签名构建、托管更新与支持仍属商业边界。
- **验签公钥可公开**；**签名私钥与私密签发凭证永不入源码/公开镜像**。签发工具源码可以公开。
- **内部内容继续不投影**：`docs/`、`.agents/`、内部工作记忆和工具缓存等由发布快照规则过滤，不随公开镜像发布。规则与流程见 [同步现行说明](Part6_3c_Copybara同步配置草稿.md)。
- 商标与品牌边界按 `TRADEMARK.md` 执行。

### 10.3 现有私有 canonical → 公开镜像拓扑（保留）

保留既有拓扑：私有 canonical 超集 → 公开镜像投影。公开镜像包含主程序源码、构建配置及随附第三方材料；既有内部文件排除规则保留。

原「授权门控下沉闭源 pro crate / 开源默认 free-stub / 生产公钥保密」**不再作为规范**（技术迁移见 §10.4）。

### 10.4 实现形态（技术迁移已执行）

**Graphviz 限定附加许可（2026-09-14 用户批准）**：在标准 AGPL 正文之外，根 `ADDITIONAL-PERMISSION.md` 按第7条允许与指定 Graphviz 2.40.1 / Viz.js 2.1.2 组合并分发，保留现有渲染功能。Graphviz 仍属 EPL-1.0；不授予任意闭源组合权，第一方的其余 AGPL 义务与单独商业授权路线不变。分发组合目标码时，对应源码须包括所用 Graphviz/Viz.js 源码与构建脚本。此附加许可须有相关版权人的授权支持，不能代第三方授权。

**许可元数据统一**：各第一方 manifest 的基础许可为 AGPL-3.0-only（优先 workspace 继承，独立 workspace 单独声明）；限定附加权限以 `ADDITIONAL-PERMISSION.md` 为准，不使用未经登记的自造 SPDX exception 标识。

**授权实现归一**：主机既有 `KeyringLicenseStore` 作唯一直销实现，未授权/信任根无效时由同文件的 `FreeStubEntitlement` fail-closed 回退（原 `channel_stubs` 模块与渠道 feature 已随 P16 删除）；`crates/scrollery-pro` 与 `crates/scrollery-free-stub` 已删除。保留 `crates/scrollery-plugin-api` / `crates/scrollery-exotic-trust`、付费校验与正式签发工具。

**公钥配置归一**：本次归一保留既有 key ID、公钥、用途和有效期；构建注入仍为整组替换，轮换时保留需继续信任的历史键。

**公开同步**：从私有仓指定的已提交修订生成公开快照，只过滤内部文件，不改源码或 lock；公开提交使用固定 bot 身份与固定说明，不复制私有提交历史。任何公开分支推送前须扫描拟公开树与提交元数据；公开暂存分支的原锁构建与测试通过后才提升 main。具体入口见 [同步现行说明](Part6_3c_Copybara同步配置草稿.md)。

**公开声明对齐**：README / CONTRIBUTING 表述为「第一方公开源码 AGPL-3.0-only，另可取得单独商业授权」，源码编译不等同于取得全部付费功能或官方服务。

### 10.5 依赖与模型协议合规审计（实测 Cargo.lock 669 条目 / cargo metadata 报告 653 Rust crate + 199 npm）

> 🔴 第 8 轮核验：`grep -c '^\[\[package\]\]' Cargo.lock`=**669**（非旧值 653）；653 系 `cargo metadata` 口径（排除自身/dev-only/未启用 target）。两数并存须注明口径，勿混用。根 `NOTICE.md` 已入库，由 `scripts/generate-notice.mjs` 从 `Cargo.lock`/`package-lock.json` 生成、以 `--check` 校验新鲜度；SBOM 仍待发行前生成。

> 方法：`cargo metadata --offline` 解析 license 字段 + 读 Cargo.lock/package-lock + WebFetch 核验。
> 🔴 **下表与「98% 宽松许可、Win/mac 目标上无 GPL/AGPL/LGPL 实际编译」为历史审查快照，2026-09-14 未复验**：本轮未编译、未做完整依赖审计，该统计不构成当前实测结论，也不作为第一方改 AGPL 后的兼容性依据。现行发行以 `NOTICE.md`（生成物）与第三方/AGPL 组合兼容性核验（LibRaw 许可分支与对应源码、Graphviz EPL-1.0 组合边界）为准，后者仍待发行前完成。

| 风险 | 项 | 协议 | 处置 |
|---|---|---|---|
| 🔴 **blocker** | **SCRFD(det_10g.onnx) + ArcFace R50(w600k_r50.onnx)**（InsightFace） | **非商用研究专用** | **绝不打进任何付费/免费包**。当前是死代码+未下载(安全)；商业化前从 catalog 彻底隔离或标「仅科研/用户自导入」；CI 断言这两个 .onnx 不出现在发行包。需商用高精度→购 InsightFace 授权或换 RetinaFace+CosFace(MIT/Apache)。→ **Part6/Part4** |
| 🟠 **medium** | **gficcg/clip_cn_vit-onnx**（HF） | **未声明 license** | 上游 OFA-Sys/Chinese-CLIP 是 MIT。**改为用 `export_clip_l14_336_onnx.py` 从官方源自行导出 + 自托管(附 MIT NOTICE)**；或联系作者补 MIT。短期 About 页注明来源。→ **Part4** |
| 🟡 low | cssparser/cssparser-macros/selectors/dtoa-short/servo_arc（经 wry/dom_query） | MPL-2.0 文件级 copyleft | 静态链接包含闭源文件合法；只需 About/NOTICE 列名 + crates.io 链接，不触发对本项目源码的额外义务。 |
| 🟡 low | jszip 3.10.1（经 epubjs） | MIT OR GPL-3.0 双授权 | NOTICE 明确「以 MIT 使用」即可。 |
| 🟡 low | ring 0.17 | ISC+Apache-2.0+MIT 三重 | NOTICE 列三来源；主流库无实质风险。 |
| ✅ 已统一 | 第一方 crate（exotic-protocol / psd-worker / psd-probe 等） | **AGPL-3.0-only** | 统一到根 `[workspace.package] license`，各 member 经 `license.workspace = true` 继承；独立 workspace 的 raw-worker/raw-probe 各自声明（2026-09-14 由 MPL-2.0 改定）。 |

**合规白名单（确认可商用 + 开源无传染）**：Tauri(Apache/MIT)、ONNX Runtime(MIT)、ort(Apache/MIT)、tokenizers(Apache)、rusqlite/SQLite(MIT/PD)、lofty(MIT/Apache)、pdfjs-dist(Apache)、epubjs(BSD-2)、psd(MIT/Apache)、Vue3/Pinia(MIT)、YuNet(MIT)、SFace(Apache)、Chinese-CLIP 上游(MIT)。
**超版权范围**：中国大陆发行人脸功能涉 PIPL /《人脸识别技术应用安全管理办法》（CAC+公安部，2025-06-01 施行；🔴 第 8 轮核验：原误作「人脸识别技术管理办法」、缺「应用安全」二词，已对齐 external_references L4 官方全称），须商业发行前单独法律评估。

### 10.6 非代码护城河

开源后代码可 fork，但真护城河是**渠道与品牌**（VSCodium 无法访问 MS 插件市场，从未威胁 VS Code）：① **独立品牌商标注册**（CN/US/EU Class 9+42 注册一个**全新独立品牌**——见 §10.7；「Picasa-Next」本身**无法注册且侵权**，原计划作废；fork 须改名，见 `TRADEMARK.md`）；② **CLA-assistant**（GitHub App 零运维，贡献者授再授权权，为未来 relicense 留法律退路——Redis/Terraform 反面教材）；③ **官方分发与签名**：正式安装包、托管更新与上架渠道由项目掌握，源码构建不自动获得这些内容；④ **贡献对象是整个现有应用**：本仓现有功能与授权代码都接受 PR（保持 CLA 与商标规则），未来独立商业组件另定许可并单独处理。

> **对 Part 的影响**：§10 归口——**Part0**(LICENSE/NOTICE/CONTRIBUTING/CLA 配置)；**Part6**(授权实现归一 + `FreeStubEntitlement` fail-closed 回退 + 公钥配置归一；SCRFD catalog 隔离)；**Part4**(CLIP 权重自导出自托管；AES 解密)；**Part7**(公开投影构建测试门禁 + 签名隔离)。

> **开放决策（上架前定，不阻塞重构）**：① 是否注册 Entity CLA（视社区规模）；② 首个独立商业组件的内容与许可（待有真实产品形态时定，不预先承诺）。

### 10.7 产品命名与商标风险（⚠️ 必须改名）

> **本文档全程出现的「Picasa Next / picasa-next」是工作代号，非最终产品名，必须在公开/上架前替换为独立品牌。** 法律核实结论（第六轮调研）：

**裁决：HIGH 风险，必须立即改名。** 「PICASA」是 Google LLC 的活跃注册商标（USPTO Reg. 2952412，Class 9 数字图像软件，状态 "Registered and Renewed"）。要点：① **停服 ≠ 商标放弃**（Google 维持注册成本极低）；② **残余商誉**——Picasa 2004-2016 全球数亿用户，2016 停服至今仅 10 年，未达商誉消灭临界（Hornblower 案 ~15 年）；③ **同品类正面冲突**（图片管理软件用完整在先词根 = 混淆可能性最高档，Sleekcraft/DuPont 双重命中）；④ **中国驰名商标跨类保护**（《商标法》§13 + 反不正当竞争 §6，即便在华无注册）；⑤ **平台投诉即下架**——微软 Store/Steam 商标投诉机制，Google 只需提交投诉无需诉讼，是最低成本执法路径，与你「开源冲 Stars + 上架」的增长策略直接对冲。

**§10.6 原「注册 Picasa-Next 商标」作废**：USPTO 会以 §2(d)（与在先注册混淆）驳回，且申请行为本身触发 Google 商标监控、反而加速对抗。

**可做（描述性合理使用 Nominative Fair Use）**：营销文案中指代迁移来源可用——「为曾用 Google Picasa 的用户打造」「Picasa 停服后的本地替代」「支持导入 Picasa 数据库格式」。三要件：不在**产品名/Logo/商店标题/域名/注册实体**中含 Picasa；不暗示 Google 赞助/关联；仅最低限度提及。中国市场更保守（界面不突出 Picasa + 「与 Google 无关联」免责，但中国法院免责声明不能完全免责）。正式文案发布前过 IP 律师。

**改名范围（确定新名后一次性替换，归 Part0/Part8 上架前执行；Part7 仅加 identifier 锁定 CI 断言、不执行替换）**：GitHub repo + Org、域名（不得含 picasa 词根）、`Cargo.toml` package name、`tauri.conf.json` `identifier`（**上架后变更代价极高，首次上架前必锁定**）、内部 `picasa_next` 模块前缀、所有文档/营销物料。

**候选名（第六轮，待用户拍板，均需正式清权）**：首选 **Mnemo**（记忆女神，5 字符，造词清洁度最高）；次选 **Foteca**（影像馆）、**Arclive**（arc+archive）、**Tessera**（马赛克拼片）、**Lumeo**（光）；备选 Heliobox/Caskade/Fotheque/Soleil/Pixora/Kaleido(域名已占)。命名原则：不绑 photo/pic 单一词根（四媒体）、≤10 字符、跨 Win/mac/Store/Steam/域名/GitHub 一致、中英双市场可读。

**清权流程（采用前，归 Part0/Part8）**：USPTO TESS + EUIPO eSearch + CNIPA（**Class 9 软件 + Class 42 软件服务**双类，英文 + 中文音译）+ 域名(.com/.app/.io)+ App Store/Microsoft Store/Steam 重名 + GitHub Org/社媒 handle；公开/上架前出正式 **FTO 意见书**（US/EU/CN 三辖区，约 $1,500–4,000/名）；确认后 USPTO ITU + EUIPO EUTM + CNIPA（委托本地代理）尽快申请。**开源 repo 名与商业品牌名可解耦**（repo 用技术描述名如 `local-media-vault`，降低贡献者商标顾虑）。

---

## §11 重构推进总纲（多线并行）

> 推进方式 = **多线并行**（用户选定），按**依赖关系**编排成波次，非严格串行。下图为 Part 依赖；同波次内各 Track 可并行。

### 11.1 Part 依赖图

```
Part0 总纲(本文) ── 全局基准
   │
Part1 数据层 ──┬── Part2 扫描/画廊
               ├── Part3 缩略图/派生/GPU
               └── Part4 AI/人脸插件化 ──┐
Part6 插件平台/exotic ──────────────────┤(Part4 依赖插件框架)
   │                                     │
   ├── Part5 前端体验(含插件商店UI,依赖Part2/Part6)
  └── Part7 发布工程(CI/签名/公证/热更新,依赖Part6)
                                          │
Part8 商业化分发(依赖Part0/Part6/Part7) ──┘
```

### 11.2 波次编排

**Wave 0 — 地基与速赢（1-2 周；🔴 拆 0a→0b，非全互不阻塞）**：**0a 先行**（无内部依赖）= Part1 T1-T4（事务化 + SCHEMA_V10 DDL）、Part6 T10（workspace + pro 下沉）+ epub + fetch_registry、Part5 速赢、test_collation；**0b 依赖 0a** = Part2 删除检测（依赖 Part1 V10 的 `volume_id`）、Part7 workspace CI 适配（依赖 Part6 T10 workspace）。下列按 Part 归类，执行按 0a→0b 排序：
- *Part1*：migration 每版本块加 `BEGIN IMMEDIATE…COMMIT` 事务（P0，矛盾#11）；`test_collation` 改内存 DB + `#[cfg(test)]`（P0，污染工作区）。
- *Part2*（数据安全 P0，提前到 Wave0）：SourceChanged 删旧 image/video/audio_meta 触发 re-enrich（行为归 Part2，[queries.rs:553-573](../../src-tauri/src/db/queries.rs#L553)，**非 enricher.rs**）；fast_scan 后差集标记 `is_deleted=1`（删除检测缺失）**——⚠️ 强依赖 §6 卷在线判断 + `media_items.volume_id`（Part1 SCHEMA_V10 先就绪）：差集仅对在线卷生效、写删前再校验在线，否则拔移动盘重扫会误删整盘**。
- *Part7*：`ci.yml` 加 `cargo test`（P0，4 行，162 测试进门控）；CI 适配 workspace（`cargo test --workspace` 根目录执行）。🔴 **建根 Cargo workspace 本身归 Part6 T10**（与 pro 下沉捆绑，Part7 只消费 + CI 适配，见 Part7 §1.3）。
- *Part6*：[format.rs](../../src-tauri/src/utils/format.rs) 加 epub（P0，1 行解锁 P4 文档链）；`fetch_exotic_registry` IPC（P0，全新设备可装插件）。
- *Part5 速赢*：MediaGrid `showToast→ui.addToast`（8+ 处，批量操作 toast 修复）；AppShell `:data-theme` 绑定修正；幽灵 IPC 常量清理。

**Wave 1 — 核心能力（依赖 Wave0 地基，2-4 周）**
- *Part1*：document_meta DAO 补全；scan_roots backend_id 接线；**向量存储抽象 + ANN 选型**（usearch / sqlite-vec，VectorStore trait 预留）。
- *Part2* ‖ *Part3* 并行：keyset 分页 + 复合索引；时间轴滚动条；增量扫描；坐标平移滚动 bug 修复（百万级真正达成）‖ CPU 解码 ResizeHint 修复；WicEngine 注册进 EngineArena（HEIC）；派生产物清理；**mac 原生媒体层（objc2 桥）**。
- *Part6*：**AES 加密权重（层1）** + EntitlementProvider/PluginDeliverySource 抽象（§9）；`uninstall` 加锁。
- *Part5*：稳定多选（框选/Shift/Ctrl）；星级+颜色标签；虚拟相册强化；首启向导；IPC 错误统一（分批 ai/face/doc 起）；ScanChannelPayload 类型修正。
- *Part7*：`tauri-plugin-updater` 热更新 + 签名密钥对；CI 加 Windows Authenticode + dev 分支触发。

**Wave 2 — 插件化与商业化（依赖插件框架，3-5 周）**
- *Part4*：CLIP/人脸从「进程内 ort」迁到 **sidecar worker 插件**（M1 建 ai-worker crate → M2 主二进制去 ort → M3 接 Registry + 移除 ORT DLL bundle → 核心 <10MB 达标）；ANN 检索接入；中文双档；人脸批量审批 UI；模型切换安全（persons 加 model_name 列）。
- *Part5*：插件商店 UI（安装/激活/购买引导/下载进度）。
  - *Part7*：CI 测试门控 + 版本单一源 + `tauri-plugin-updater` 热更新（普通依赖）+ Win Authenticode / macOS Notarization（覆盖所有 sidecar）；原「多渠道构建变体」已随 P16 退役。
- *Part8*：许可基础设施 + 可选在线激活微服务（层2）；定价/支付落地；官网/落地页/动图简介。

**Wave 3 — 性能打磨与测试补全（贯穿，按需）**
- ANN 大库实测调优；get_directory_tree 维护列；缓存频率保护；Scheme1 死代码清理；MediaGrid 拆分。
- 测试补全贯穿每 Part：justified 算法、CLIP preprocess/tokenizer、face_pipeline、video MF stride/rotation、epub OPF——**每 Part 完成同步补对应测试，非最后集中补**。

### 11.3 P0 速赢清单（Wave 0，单独列出，可一天内多数完成）

| 项 | 工时 | 文件 | 状态（2026-06-28 代码实测核验） |
|---|---|---|---|
| format.rs 加 epub（解锁 P4 文档链） | 5 min | [format.rs](../../src-tauri/src/utils/format.rs) | ✅ 已实施 `4c9f365`：classify_media_type + doc_subtype + 测试；载荷性确认（walker.rs:88 经此判收录），顺带纵深防御互斥（epub 不可被 exotic 冒名） |
| ci.yml 加 cargo test | 0.5h | `.github/` | ✅ 已实施（2026-07-02，R0-1，`d9c0436`）：ci.yml 触发扩 `pull_request`+`dev`，加 `cargo test --workspace --locked` + 前端 `vue-tsc --noEmit`/`vitest run` job；CI 实跑已实证（`d1430dc` run 28579149102 全绿）。开机冒烟钩子接线归 Part7-T1 余量（PICASA_SMOKE_TEST，见 todo.md G1） |
| SourceChanged 删旧 meta（行为归 Part2） | 0.5h | [queries.rs:553-573](../../src-tauri/src/db/queries.rs#L553)（非 enricher.rs） | 🔵 真缺口但窄·**归 Part2**：多数变更项走 0×0 占位被重 enrich；残留仅「首屏已取真实尺寸项」+ document/video_meta 独立表（upsert 不碰）。完整修法须配 Part2 enrichment 重触发 + 缺失检测 P0-1，不单独半修 |
| test_collation 改内存 DB | 10 min | [connection.rs](../../src-tauri/src/db/connection.rs) | ✅ 已实施 `ec5cab7`：共享缓存内存库（零文件残留、保读池覆盖）+ 强化断言（原仅插单行没测排序，已补自然序 1<2<10） |
| MediaGrid showToast→addToast | 30 min | MediaGrid.vue | ✅ 已实施（Part5 P0 速赢，`522a94c`）：全部改 `ui.addToast(type, msg)`，全仓 grep 零 `showToast` 残留 |
| AppShell data-theme 绑定 | 5 min | AppShell.vue | ✅ 已实施（Part5 P0 速赢，`522a94c`）：`AppShell.vue` 绑 `:data-theme="ui.isDark ? 'dark' : 'light'"`（绑 resolved 值而非 raw `ui.theme`，注释已注明 'system' 不匹配 CSS 规则的坑） |
| uninstall 加 install_lock | 1 行 | [exotic_commands.rs](../../src-tauri/src/ipc/exotic_commands.rs) | ✅ 已做（[exotic_commands.rs:502](../../src-tauri/src/ipc/exotic_commands.rs#L502) 已有 `exotic_install_lock`，先于本清单完成；本次复核确认） |
| migration 加事务 + 幽灵 IPC 清理 | 1-2h | [migration.rs](../../src-tauri/src/db/migration.rs) | 🟡 拆分：migration 事务化 ✅ 已实施（上轮 `6f8415a`）；「幽灵 IPC」实为前端 [ipc.ts:126](../../src/constants/ipc.ts#L126) `CLEAR_ALL_DATA`（后端无此命令），归 **Part5 §3.4** |

---

## §11.4 v0.1 可发布最小集与务实交付切割线（单人产能 × 价值优先）

> 🔴 **本节与 §11.1/§11.2 正交，且为交付范围的最终裁决**：波次（Wave）描述**依赖排序**（谁能编译 / 谁先就绪）；本节描述**交付切割线**（先发布什么、延后什么）。二者冲突时——**本节决定上线范围，波次只决定其内部顺序**。动机（terminal review 关切 2）：方案总盘横跨 后端 / 前端 / GPU / mac FFI / 跨进程协议 / AES 基础设施 / 签发服务端 / 三渠道上架 / 官网，对单人开发是多季度量；若无显式 MVP 切割线，最大风险 = 「完美规划下的无限期未发布」。彻底性是优点，也是陷阱。

### 11.4.1 v0.1 = 「Windows 直销可变现核心」（最短变现路径）

**纳入 v0.1（其余一律 fast-follow，不阻塞首发）**：
- **平台**：**仅 Windows**（mac 见 §11.4.2 降级 fast-follow）。
- **核心免费**：四媒体基础浏览 + Wave0 全部数据安全 P0（迁移事务化 / 删除检测三重守门 / SourceChanged 重 enrich / epub 登记）+ 坐标平移运行时验证 + 稳定多选（交互稳定可用,契约临时可改非冻结）+ 速赢（toast / 主题 / 类型 / 幽灵常量）。
- **一个付费插件直销闭环**：选 **exotic-formats**（后端最成熟、**无 ort、无 GPU 协调、无 worker 化依赖、无 ONNX 权重**）作首个变现插件——走「层0 已完成验签 + 直销 license 签发（Part8 D1）」即可收款，**不依赖 AI worker 化 / AES / GPU 令牌**。
- **发布工程最小集**：CI `cargo test` 门控（Part7 T1-T3）+ Win Authenticode 签名（T13）+ 热更新（T7-T9）。

**v0.1 显式延后（fast-follow 里程碑，不阻塞首发）**：mac 实功能（§11.4.2）、AI/人脸 worker 化全段（Part4 P2 / Part6 G1-G6）、层1 AES（§11.4.3）、ANN 检索（暴力 O(N) 在 <50 万库可接受，Part1 阈值即此意）、MS Store / Steam 渠道、时间轴 scrubber、虚拟相册强化、巨组件拆分。

> 依据：[Part8](Part8_商业化与分发.md) §8 收官说明已点明「直销四层闭环（D1-D3）是变现最短路径，可在商店渠道之前先打通」——本节把它**升格为首发范围的硬切割**。

### 11.4.2 macOS：从「并重」降级为「fast-follow」（交付排序，非愿景否定）

§2.2.4「Win+mac 桌面并重」是**长期产品愿景，保留不变**；但**首次变现的交付排序**调整为 **Win 先行、mac fast-follow**（terminal review 关切 4）。依据：mac 当前根本不编译（[Part3](Part3_缩略图派生与GPU引擎.md) §2.7 实测 ≥12 文件 66 处待门控）、objc2 桥四框架自述 2-3× Win 工作量、每 sidecar 单独签名公证、CoreML 推迟 P6.5——单人 pre-revenue 全程背 mac 税会显著拖慢首发。

- **保留（低成本、必须留）**：Part3 T8「mac 三步走第一步 = `cfg` 门控让 mac 能 `cargo check`」+ Part7 CI 双平台 `cargo check`——**作防回归门控**（Win 改动不破 mac 可编译性）。
- **后置**：Part3 T9（objc2 实功能：ImageIO / AVFoundation / PDFKit / QuickLook）+ Part7 mac codesign / notarization 实做 → **v0.1 之后第一个 fast-follow 里程碑**。

### 11.4.3 层1 AES：后置至 v0.1 变现之后（不预留渠道/AES 字段）

§8 防护决策「层1 + 层2」**不变**，但**层1 AES 实装从 Wave1 移至 v0.1 变现后**（terminal review 关切 6）。依据：方案自身诚实评级（§8.3 / Part8 RK13）已定——层1 保护的是**公开权重**、可被**解析一个泄漏 token 击穿**、只挡懒人；而层0（已完成）+ 层2（可选在线激活）已挡住 99% 脚本小子。

- **当前（v0.1）即做**：[Part6](Part6_插件平台与exotic收尾.md) T9 EntitlementProvider 抽象（授权实现归一，见 §10.4）。**不预留 `enc_seed` 字段**：P16 已删除渠道枚举与 `InstallSource`，AES 也不是首发所需（首发插件 exotic-formats 无 ONNX 权重）。
- **后置**：AES-256-GCM 实装 + HKDF 派生 + 具名 shm 硬化 → 随**首个加密权重插件（AI / 人脸）**一并交付（彼时 worker 化已就绪、AES 才有承载对象）；字段与密钥派生随实现一并设计，不提前塞入 schema/DTO。

### 11.4.4 测试地基硬门槛（特征化测试前置，非「完成后补」）

§11.2 原则「测试补全贯穿每 Part」**升级为硬门槛**（terminal review 关切 1）：实测 162 主体测试中 exotic 占 113（70%），而 **video / derive / engine / ipc / scanner 接近零**（§1.2）——恰是「百万级流畅」与「四媒体」的命门路径。**规则**：触碰这些零测试子系统的**任何重构任务，动代码前先补特征化测试（characterization test）锁住当前行为**，否则重构无回归安全网。

- **优先补测路径**（动其代码前先锁）：`media_foundation.rs` stride / 旋转 / bottom-up / 损坏帧（Part3 Q16 已知 bug 区）、`justified.rs` 布局算法、`apply_thumb_results` O(batch) 回填、增量扫描 `seen_ids` 守门（Part2 P0，**误删风险**）。
- 各 Part §6 验收的「+ 对应单测」由**建议**改为**合并前置条件**（先有锁定行为的测试、再改代码）。

> 🔵 **首批已落地（2026-06-28，cargo test 实证）**：① **开机冒烟测试钩子** `PICASA_SMOKE_TEST`（[lib.rs](../../src-tauri/src/lib.rs) `RunEvent::Ready` → exit 0）——CI headless 启动构建产物 + 断言退出码非 101，把「开机 panic」挡在合并前（本轮 coordinator `tokio::spawn` 无 reactor panic 即此类；CI 接线见 [Part7](Part7_发布工程.md) §3.1.6）；② **迁移事务化**（[Part1](Part1_数据层.md) §3.1 🔵，根治半迁移死循环）；③ **startup 7 处 `.expect()`/`.unwrap()` 优雅降级**（[lib.rs](../../src-tauri/src/lib.rs) `fatal_startup_error`：清晰 stderr + 原生弹窗 + 退出码 1，取代不可读 panic）。**Step Zero 三件套已就绪。**

---

## §12 全局约定（所有 Part 执行时遵守）

### 12.1 编码约定

- **Rust 错误处理用 `thiserror`**；命令层逐步统一返回 **`AppError`（非 `String`）**（矛盾#10，Part5/IPC）。
- **数据库仅用 `rusqlite`**：写 `Mutex<Connection>` 串行化、读 `r2d2` 池（4 连接）WAL；**所有 SQL 参数绑定**，禁字符串拼接。
- **锁序固定 `db_writer → layout_cache`**（或独立），永不反向，防死锁。
- **前端 Vue 3 `<script setup>` Composition API + TypeScript strict**；vanilla CSS 变量多主题。
- **代码注释、日志中英双语**；面向用户的查询一律追加 `is_deleted = 0 AND companion_of IS NULL`。
- `cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`；缩略图 `cache/thumbnails/{size}/{2-hex}/{key_hex}.webp`。

### 12.2 让步与线程模型

- 派生/AI **对交互让步**（`note_interaction`）；**enrichment 绝不让步**（会与 2s 自动重排反馈循环饿死导入）→ 用 `reserved_core_pool`。
- tokio 处理异步 IPC；rayon + `spawn_blocking` 处理 CPU 密集（扫描/缩略图/布局/余弦）；**rayon 内永不 `.await`**。
- 插件 = **sidecar worker 进程**（崩溃隔离）；GPU 显存跨 worker 由 Coordinator `plugin_id` 层互斥。

### 12.3 安全与许可

- **验签用 `ring`（非 ed25519-dalek）**；Host 只验不签，私钥离线永不入仓/二进制；release/license 用途严格分离。
- 加密权重用 `ring::hkdf`+`ring::aead::AES_256_GCM`，密钥用后 `zeroize`；enc_seed 高熵 OsRng、HKDF info 带 `plugin_id‖model_id`（对齐 Part4 §3.7.2 / Part6 §3.7.1）。
- 商店渠道信任平台 entitlement，**不重复**做 Ed25519/AES（§9）。
- **开源边界（§10）**：第一方公开源码（含授权实现与现有 worker）统一 AGPL-3.0-only，另设单独商业授权。**验签公钥可公开，私钥与私密签发凭证不入源码/公开镜像**；代码签名仍在私有 CI 完成，凭据不入公开 Actions。

### 12.4 工作流约定

- **改动代码后及时用中文 commit**；**仅在用户通知时 push**。
- **大文件分多次小步 `Edit` 追加**，不单次 `Write` 重写；plan 文档增量写入后置哨兵。
- 依赖：**可联网 / 可 `cargo vendor` 预取**（解除「仅缓存 crate」限制），但**优先复用已在依赖树的 crate**（如 ring 已含 hkdf/aead，勿引新加密库）。
- **核实纪律**：旧 plan-docs / 记忆 / 默认提示一律先核实再用；「完成度/测试数/功能是否存在」用 Grep/Read 实测；发现矛盾明确指出（见 §1.3 方法论）。

---

## §13 给执行会话的总提示词（可粘贴启动新会话）

> 在新会话执行某个 Part 时，先粘贴本段，再粘贴对应 PartN 文档。

```
你是 Picasa Next 的资深全栈架构师。项目 = Tauri2(Rust) + Vue3(Vite+TS strict) 跨平台
(Win+mac 桌面并重，iOS/Android 后置) 本地优先、完全离线、隐私第一的四媒体(图/视/音/文)
资产管理器，目标百万级流畅画廊 + 本地 AI 语义 + 人脸 + 付费插件。

执行任务前必读：
1. docs/refactor_2026/Part0_总纲与产品定稿.md（产品定稿/打包模型/盈利/防护/约定，全局基准）
2. 本任务对应的 docs/refactor_2026/PartN_*.md

硬约束（覆盖一切旧 plan）：
- 核心包 <10MB（弹性目标、功能+体验优先），四媒体基本格式尽用系统原生 API；AI/人脸/冷门格式是三个可下载插件，
  统一走 exotic 子系统的「Ed25519 签名 Registry + sidecar worker 进程」框架。
- 完全离线、无订阅；盈利=核心免费+三插件买断；防护=Ed25519(层0已完成)+AES加密权重(层1)
  +可选一次性在线激活(层2)；后期需上架微软Store/Steam，授权走 EntitlementProvider 抽象。

铁律：
- 旧 docs/记忆/默认提示不可轻信，凡涉及完成度/测试数/功能存在性，用 Grep/Read 实测，
  与文档矛盾时明确指出（记忆索引层比正文先腐化）。
- thiserror；rusqlite(写 Mutex+读 r2d2 池 WAL)；SQL 参数绑定；锁序 db_writer→layout_cache；
  注释/日志中英双语；面向用户查询追加 is_deleted=0 AND companion_of IS NULL；
  派生/AI 让步、enrichment 不让步(reserved_core_pool)；rayon 内不 await。
- 验签用 ring 非 dalek；加密用 ring hkdf+aead；优先复用已在依赖树的 crate。
- 改动后中文 commit；仅用户通知时 push；大文件小步 Edit 不整写。

发现任何多余/错误/可改进(架构/功能/逻辑/UI)立即告知。输出完整不省略。
```

---

*（Part 0 正文完，§0–§13 全部定稿。「产品名」为唯一待用户拍板项（§10.7），不阻塞 Part1-8 技术推进。后续 Part 1–8 另文，按 §11 波次推进。）*

<!-- 哨兵: Part0 全文定稿(§0–§13；§6 卷可用性 / §10 开源合规含 §10.7 命名风险)；唯一待用户拍板=最终产品名(§10.7)；待审阅后写 Part1 -->
