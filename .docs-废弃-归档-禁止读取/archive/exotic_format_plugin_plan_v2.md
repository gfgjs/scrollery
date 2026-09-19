# 冷门格式插件子系统 · 总纲 v2 | Exotic-Format Plugin Subsystem · Master Plan v2

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic 总纲 v2,被 v3.1 取代(v3 全套见同目录 exotic_format_plugin_plan/,亦已归档);现行权威 = refactor_2026/Part6。

> 为「音 / 视 / 文 / 图」四类媒体的**冷门格式**（psd · heic · rmvb · word · raw · flac 及更多）建立一套
> **独立于主引擎**、**独立任务管理**、**进程隔离 worker 插件**、**license 门控付费**的处理子系统。
> 以 **PSD（图）** 为垂直切片打通整条线，再按同一契约横向扩展其余格式。
>
> An independent, separately-scheduled, **process-isolated worker-plugin**, license-gated paid subsystem
> for *exotic* media formats across image/video/audio/document. Ship the **PSD** vertical slice first,
> then expand horizontally under one frozen contract.

---

## 0. 文档信息与阅读指引 | Meta & How to read this set

| 项 | 内容 |
|---|---|
| 版本 Version | **v2**（2026-06-24） |
| 状态 Status | 草案待评审 Draft / 关键决策已固化、少量待定见 §5 |
| 取代 Supersedes | `exotic_format_plugin_plan_v1.md`（中断的早期产物，仅留历史参考；**本 v2 与其分卷为唯一权威**） |
| 关联 Related | `implementation_plan_v1.2.md`（母设计）、`architecture_notes.md`（活文档不变量）、`feature_expansion_plan_v1.md`（§1.4 后端变体 / Lite·Perf）、`face-recognition-plan`（独立 pipeline 范式先例） |

### 0.1 本文档集的组织 | Document set layout

为规避"单文件过长导致生成中断"，本设计**拆为 1 总纲 + 4 分卷**。总纲是跨切面的"为什么/是什么"，分卷是逐 phase 的"怎么做"。**每个分卷自带"新会话继续提示词"与"完成定义(DoD)"，可在全新会话独立续作。**

| 文件 | 角色 | 覆盖 Phase | 何时读 |
|---|---|---|---|
| `exotic_format_plugin_plan_v2.md`（本文件） | **总纲 / Index** | — | 任何人入门、决策评审、跨卷查阅许可策略与术语 |
| `exotic_format_plugin_part1_foundation.md` | 地基：schema + 扫描识别 + 宿主路由 + 门控 | P0–P2 | 第一步施工；无 worker、无 pipeline 即可验收 |
| `exotic_format_plugin_part2_pipeline_worker.md` | 流水线 + Worker 协议 + PSD worker | P3–P4 | 引擎本体；产出第一张 PSD 缩略图 |
| `exotic_format_plugin_part3_license_distribution.md` | License + 签名 + 远程注册表 + 下载/安装/激活 | P5–P6 | 商业门控与插件分发 |
| `exotic_format_plugin_part4_frontend_slice_hardening.md` | 前端 UI + PSD 端到端切片 + 硬化/测试 | P7–P9 | 用户可见收口 + 上线前硬化 |

### 0.2 总纲目录 | Master TOC

1. 一句话定位 TL;DR
2. 背景与现状（**已核验的代码事实**）
3. 设计目标与原则
4. 架构总览
5. 关键决策（已固化 + 待定）
6. 数据模型总览
7. **编解码许可策略（业界调研核心产出）**
8. 插件形态选型（业界佐证）
9. 风险登记册
10. 路线图：分卷 × Phase × 续作提示词
11. 术语表
12. 附录 A：四类媒体插件蓝图

---

## 1. 一句话定位 | TL;DR

冷门格式子系统是「**第三套不挂主派生框架、自带 pipeline + 状态机 + 调度令牌的后台处理线**」——继 CLIP 语义分析、人脸识别之后的同构兄弟；其能力以**进程隔离的 worker 插件**形态交付，受 **license 门控**，购买后从远程注册表下载启用。架构上是「**face pipeline 的形状** + **下载/注册表的血肉** + **进程隔离的骨架** + **离线验签的门锁**」——四块里有三块在仓库已有成熟先例，真正全新的只有 **worker IPC 协议** 与 **license 门控**。

`✶ 设计哲学 ────────────────────────────────`
全系统最深层的一句话：**插件边界 = 许可边界 = 故障边界 = 调度边界**。
一个冷门格式被独立成"下载式 worker 插件"，同时解决了四个本来纠缠的问题——
法律上（GPL/专利重的解码器不污染主程序）、商业上（可单独售卖）、
稳定性上（畸形文件崩溃只丢一项）、性能上（可单独限核限速）。这是整个架构的总纲。
`──────────────────────────────────────────`

---

## 2. 背景与现状 | Background & Current State（已核验 Verified 2026-06-24）

### 2.1 冷门格式当前的两种失败模式 | Two failure modes

读码核验（非记忆推断），今天冷门格式存在**两种并存失败**：

| 失败模式 | 触发格式 | 已核验代码事实 | 用户感知 |
|---|---|---|---|
| **A. 入库但解不出** | `psd` · `heic/heif/avif` · RAW(`cr2/cr3/nef/arw/dng/raf/orf/rw2/pef/srw`) · office(`doc/docx/xls/xlsx/ppt/pptx/odt/ods/odp`) | `classify_media_type`（`utils/format.rs:44-60`）**已归类**这些扩展名 → walker 入库；但主图像引擎不识别 → `thumbnail/generator.rs` 走到 `AppError::UnsupportedFormat`（image 类，`generator.rs:290`）或 `thumb_status=2`（非 image 类，`generator.rs:271-273`） | 画廊空白/占位方块，无封面 |
| **B. 根本不识别** | `rmvb` · `rm` · `wpd` · `dwg` · `ai` 等**不在分类表**的扩展名 | `classify_media_type` 对其返回 `None`（video 表 `format.rs:48-50` 确认**无 rmvb/rm**）→ walker `continue` 跳过 | 文件完全不进库，用户不知其存在 |

> 结论：冷门格式不是"没做"，而是"**识别层与解码层脱节**"。本设计统一收口这两条裂缝——A 类已入库只需补解码；B 类需先补识别再补解码。

### 2.2 已核验的可复用资产 | Verified reusable assets

本设计**最大化复用**仓库现成机制，新代码集中在真正全新处。下表每一项均经本次读码确认存在：

| 复用对象 | 已核验位置 | 本设计如何借用 |
|---|---|---|
| **独立 pipeline 范式** | `ai/face_pipeline.rs`：4 线程 Producer→Preprocessor→DetectEmbed→Writer + `std::thread::scope` + `crossbeam_channel::bounded` + `CancellationToken` | 冷门 pipeline 照搬其骨架，消费端换成 **worker 子进程池** |
| **产物回填三件套** | `derive/pipeline.rs:480 write_results`、`layout/cache.rs:115 apply_thumb_results`、`db/queries.rs:995 update_thumb_result`、事件 `db:media_enriched`（`pipeline.rs:588`） | worker 产出缩略图后**完全复用**这条回填路径，画廊零改动显示 |
| **缩略图缓存键/路径** | `thumbnail/cache.rs`：`thumb_path()` / `thumb_db_path()` → `thumbnails/{size}/{prefix}/{hex}.webp`，键 `cache_key = xxh3_64(...)` | 冷门缩略图**复用同键同路径**，`MediaThumb` 像普通缩略图加载 |
| **下载基建（已抽共用）** | `ipc/ai_commands.rs:958 pub(crate) async fn download_assets`（断点续传 Range + sha256 + 镜像回退 + 进度 Channel）、`download_file`(:1117)、`sha256_matches`(:1179) | 插件包下载直接复用 `download_assets`，仅换资产清单来源 |
| **独立令牌 + 让步 + GPU 门闩** | `state.rs`：`ai_analysis_token`(:70) / `face_analysis_token`(:75) / `gpu_analysis_owner`(:91) / `note_interaction`(:177) / `should_yield_derivation`(:313) / 优先级阶梯 scan>thumbnail>derivation>AI(:258) | 新增 `exotic_analysis_token` + `should_yield_exotic`；**worker 是 CPU 进程不争显存 → 不需 `gpu_analysis_owner`** |
| **凭据安全存储** | `keyring` v3（`Cargo.toml:64`，`proofread/mod.rs` 已用） | license token 存 keyring，不落明文 DB |
| **能力注册表 + 在线发现** | `ai/remote_registry.rs`（架构→变体发现 + 10min 缓存 + 离线回退） | 插件注册表（远程目录）照此模式 |
| **解压** | `zip` v2（`Cargo.toml:58`，deflate） | 插件包解包 |

### 2.3 与 v1 草案的关键偏差（务必知悉）| Deltas from v1 draft

v1 是需求不明确时的中断产物，本次核验发现若干**不可照搬**之处：

1. **schema 版本号有撞号风险**：当前库到 **V8**（`db/migration.rs` 最后块 `if version < 8`，`:127` 有 `// if version < 9` 占位）。v1 写死"+V9"，但**人脸计划的 TODO2（已推迟）也预定 V9** 给 `persons` 加 `model_name`。→ **落地时取"下一个空位"，谁先落谁占；本设计文中以 `V_next` 指代，不写死数字。**
2. v1 在 §8.4 **截断**，完全缺：施工 phase、前端、PSD worker 实现、测试、风险、待决策、分卷续作提示词——本 v2 集补齐。
3. v1 未含**编解码许可策略**（§7）——这是本次业界调研的核心增量，直接决定每类插件"自建/委托 OS/LGPL 动态链/用户侧自装"的取材路线。

## 3. 设计目标与原则 | Goals & Principles

### 3.1 目标 | Goals

- **G1 独立**：冷门引擎自成子系统，不修改/不污染 `EngineArena`、`derive` 框架与主缩略图热路径。
- **G2 隔离调度**：独立令牌、独立工作池、独立让步策略，可单独暂停/限速/调优，**绝不拖慢常见格式**。
- **G3 可扩展插口**：**新增一个冷门格式 = 发布一个新插件包，主程序零改动**（开闭原则）。主程序代码里没有一处 `if format == "psd"`。
- **G4 插件化付费**：能力以独立可下载插件交付，受 license 门控；购买后下载启用，未购买则"需购买"占位。
- **G5 跨平台**：插件须能在 Windows/macOS（及未来移动端）分发；主程序对插件的契约与平台无关。
- **G6 轻量优先**：主安装包不因冷门能力变大（呼应 `feature_expansion_plan §1.1` Lite 原则）；冷门 = 纯按需下载。
- **G7 PSD 先行**：v1 切片只把 PSD（图）一条线打通，验证"识别→门控→下载→worker→缩略图→画廊"全链路。
- **G8 合规先行（新增）**：每个插件**显式标注 `commercial_ok`**，落地前按 §7 的许可矩阵亲验；GPL/专利重解码器走"用户侧自装"或"OS 委托"，绝不闭源捆绑进主程序或付费插件包。

### 3.2 不变量 | Invariants（引自 `architecture_notes.md`，不得破坏）

1. `cache_key = xxh3_64("{rel_path}/{file_name}|{mtime}") as i64`；缩略图落 `cache/thumbnails/{size}/{prefix}/{hex}.webp`。冷门缩略图**复用同键同路径**。
2. 用户可见查询恒附加 `is_deleted = 0 AND companion_of IS NULL`。
3. `thumb_status` 在 layout 缓存中按 id 原位 O(1) 同步；冷门产物回填走 `layout::cache::apply_thumb_results`（同派生）。
4. asset 协议运行时按扫描根/缓存目录授权；插件目录 `{app_data}/plugins/` 须一并 `allow_directory`（前端读插件图标等资源）。

### 3.3 两个正交维度的厘清 | Two orthogonal axes（重要，易混）

| 维度 | 取值 | 决定什么 | 主开关 |
|---|---|---|---|
| **后端变体**（已有，`feature_expansion §1.4`） | Lite / Perf | 同一**常见**能力用轻（MF/pdf.js/纯 Rust）还是重（FFmpeg/pdfium）后端 | Cargo feature（编译期） |
| **冷门插件**（本设计） | 未装 / 已购未装 / 已装已授权 | 是否**拥有某冷门格式能力** + 是否**付费解锁** | license + 运行期下载 |

> 一个冷门插件**自身**也可有 Lite/Perf（如 RMVB 插件 Lite=探测系统解码器、Perf=自带 FFmpeg），但那是插件内部事，对主程序透明。**本设计只管"插件这一维"**。

---

## 4. 架构总览 | Architecture Overview

### 4.1 分层全景 | Layered view

```
┌──────────────────────────────────────────────────────────────────────────┐
│ 扫描层 Scan   walker → classify_media_type（P1 扩展冷门扩展名表，补 B 类识别）│
│               入库 media_items(media_type, file_format, exotic_status=0)     │
└───────────────┬──────────────────────────────────────────────────────────┘
                │ exotic 项（被某 manifest 认领的 format）
┌───────────────▼──────────────────────────────────────────────────────────┐
│ 插件宿主 Host  (src-tauri/src/exotic/host.rs)                               │
│  · 启动扫描 plugins/*/manifest.json → 建内存「格式→插件」路由表              │
│  · 查 license（keyring）→ 标注每格式：已授权 / 需购买 / 无插件               │
└───────────────┬──────────────────────────────────────────────────────────┘
                │ 仅「已装+已授权」的格式进入处理；其余回写门控态供前端占位
┌───────────────▼──────────────────────────────────────────────────────────┐
│ 独立流水线 Exotic Pipeline (src-tauri/src/exotic/pipeline.rs，仿 face)      │
│  Producer(查 pending) → Dispatcher(按 format 路由) → [Worker 进程池] → Writer│
│  · 独立 exotic_analysis_token  · 独立工作池  · should_yield_exotic 让步      │
└───────────────┬───────────────────────────────┬──────────────────────────┘
                │ stdio: JSON 帧 + 二进制帧        │ 产物（WebP 字节 / 元数据 JSON）
┌───────────────▼───────────────┐  ┌────────────▼──────────────────────────┐
│ Worker 插件（独立可执行，长驻）  │  │ 产物落地 Sink（复用 derive 回填）         │
│ {app_data}/plugins/<id>/bin/…  │  │  缩略图 → thumb cache + media_items      │
│  · psd-worker(纯 Rust psd+webp)│  │  元数据 → audio_meta/video_meta/doc_meta │
│  · 各格式 worker 自带解码栈     │  │  layout_cache O(1) 同步 → 画廊刷新       │
└────────────────────────────────┘  └─────────────────────────────────────┘

    ╔══════════════════════════ 旁路子系统 Side concerns ══════════════════════╗
    ║ License  (exotic/license.rs)   Ed25519 离线公钥验签 + keyring 存取          ║
    ║ Registry (exotic/registry.rs)  远程插件目录发现 + 下载（复用 download_assets）║
    ║ Package  (exotic/package.rs)   zip 解包 + sha256 + manifest.sig 验签        ║
    ╚════════════════════════════════════════════════════════════════════════════╝
```

### 4.2 与现有子系统的关系 | Relationship to existing subsystems

| 现有子系统 | 是否改动 | 说明 |
|---|---|---|
| `EngineArena` / `ImageEngine`（主图像解码） | **不改** | 冷门格式**不**注册进 arena。主缩略图路径不再"对冷门报错"，而是"识别为冷门→让路"（§4.4）。 |
| `derive` 派生框架 | **不改** | 冷门自带 pipeline，**不**新增 `DerivationKind`。理由同 face：派生框架是主引擎一部分，并入破坏 G1/G2。 |
| `thumbnail/generator.rs` | **极小改** | 仅在分派前加判断：format 被 host 认领 → 返回"让路"结果（不报错、不占主 rayon 池），交冷门 pipeline。见 Part1 P1。 |
| `state.rs` 优先级阶梯 | **小增** | 新增 `exotic_analysis_token` + `should_yield_exotic`；阶梯插入位置见 Part2 P4。 |
| `scanner/walker.rs` | **小增** | B 类未识别格式：扩展 `classify_media_type` 或新增"冷门扩展名表"使其入库（见 Part1 P1）。 |
| `db` schema | **+V_next** | 新增 `exotic_status` 列 + `exotic_plugins` 缓存表 + config 默认值。见 §6 / Part1 P0。 |

### 4.3 数据流：一张 PSD 的一生 | Lifecycle of one PSD

1. **扫描**：walker 识别 `.psd`（**已在分类表**，media_type=image）→ `file_format=psd`、`exotic_status=0` 入库。
2. **宿主路由**：host 查路由表——`psd` 属插件 `exotic-image-psd`；license 查询=已授权且已装。
3. **入队**：Producer 查 `exotic_status=0 且 format 已授权已装` → 标 `processing(1)` → 派给 worker 进程池。
4. **Worker 解码**：psd-worker 收 `{op:"thumbnail", path, target_long_edge:480}` → 解码 PSD 合成图 → 缩放 → 编码 WebP → 回传字节。
5. **回填**：Writer 把 WebP 写入 thumb cache（复用 `cache_key`）→ mirror `media_items.thumb_status=1/thumb_path/thumbhash` → `layout_cache` O(1) 同步 → emit `db:media_enriched` 刷新画廊。
6. **显示**：`MediaThumb` 像普通缩略图显示——**前端零改动**。

> 若第 2 步 license=需购买：项保持 `exotic_status=0`，Producer **不领取**（类比 derive 的 `disabled_kinds`），前端按"需购买"渲染锁占位（§6 / Part4 P7）。

### 4.4 对主引擎的唯一侵入："让路"而非"接管" | The single touch: yield, not takeover

`thumbnail/generator.rs::decode_media_step_inner` 现对 `psd` 必然 `UnsupportedFormat`。改为**分派前先问 host 路由表**"这个 format 是否冷门"，是则不进主解码路径、也不报错：

```rust
// generator.rs 分派处（伪代码；接 host 的内存路由表，避免每项查 DB）
// 冷门格式不在主路径解码：既不报错、也不占主 rayon 池，交还冷门 pipeline。
// thumb_status 维持 0（未生成），由 exotic_status 驱动其真正处理/门控。
if exotic_host.claims_format(&item.file_format) {
    return Ok(DecodeResult::Ready(ThumbResult {
        item_id, thumb_status: 0, thumb_path: None, thumbhash: None,
    }));
}
```

> 这是对主引擎**唯一**的侵入，且是"让路"——主引擎不知道 PSD 怎么解，只知道"这不归我管"，保持 G1。

## 5. 关键决策 | Key Decisions

### 5.1 已固化 | Resolved

| # | 决策 | 取值 | 理由 |
|---|---|---|---|
| D1 | 插件形态 | **进程隔离 worker 子进程 + stdio IPC** | 隔离/ABI 稳定/跨平台/可调优；业界标准（§8） |
| D2 | 付费交付形态 | **独立插件包下载**（购买后才提供下载链） | 用户已拍板；天然 license 门控边界 |
| D3 | 未授权冷门文件的处理 | **入库 + "需购买"占位**（不跳过、不删） | 用户已拍板；让用户感知"有内容、需解锁" |
| D4 | License 校验 | **Ed25519 离线公钥验签**（无自建激活服务器） | 桌面应用、零运维、可断网；私钥仅签发侧 |
| D5 | 不并入主引擎 | 自带 pipeline，**不**进 `EngineArena`、**不**加 `DerivationKind` | 用户明确要求；隔离稳定性/性能/商业边界 |
| D6 | worker 不走 Tauri `externalBin` | 运行期下载到 `plugins/<id>/`，后端 `std::process::Command` 绝对路径启动 | externalBin 是编译期捆绑，与"购买后下载"矛盾；后端 Rust 不受 shell allowlist 限制 |
| D7 | worker 长驻 | 一个进程流式处理多任务，**非**逐文件 spawn | 解码库加载数十 ms，百万级库下逐文件 spawn 不可接受 |
| D8 | 编解码取材策略 | **四档**（纯 Rust 宽松许可 / OS 委托 / LGPL 动态链 / 用户侧自装），见 §7 | 法律合规 + 专利规避 + 体积控制 |
| D9 | schema 版本号 | **取落地时下一个空位**（文中记 `V_next`），不写死 | 避免与人脸 TODO2 撞 V9（§2.3） |

### 5.2 待定 | Open（落地前需拍板）

| # | 待决 | 选项 | 倾向 |
|---|---|---|---|
| O1 | 门控态是否落库 (决策选a) | (a) 仅内存路由 + `exotic_format_status` 命令前端实时查；(b) 加 `exotic_status=4 需授权/5 无插件` 持久化 | **倾向 (a)**：路由随插件增删动态变化，落库易脏；前端查一次缓存即可。`exotic_status` 仅留 0/1/2/3 处理进度语义 |
| O2 | 购买渠道 (决策选a) | (a) 外部官网/商店跳转 + 邮件发 license token；(b) 应用内购买（需支付 SDK） | **倾向 (a)** 起步：零支付合规负担；(b) 作 v2+ |
| O3 | 防破解强度 (决策选 仅离线验签，后期考虑升级防破解，需做好预留) | v1 仅离线验签（可被逆向绕过）；增量手段=机器指纹绑定 / worker 校验 license 派生口令 / 关键解码逻辑下沉 worker | **v1 切片不做**，先跑通；§9 风险登记列为已知可接受 |
| O4 | PSD worker 是否独立 crate (决策选a) | (a) 仓库内 `crates/exotic-workers/psd-worker`；(b) 完全独立仓库 | **倾向 (a)**：与协议 crate 同仓便于联调；分发仍是独立二进制 |
| O5 | 协议 crate 归属 (决策选 独立 crate) | `crates/exotic-protocol`（主程序 + 各 worker 共享依赖） | 倾向独立 crate，主程序与 worker 同版本引用，避免协议漂移 |

---

## 6. 数据模型总览 | Data Model Overview（详细 DDL 见 Part1 P0）

设计取舍（与 ai_status/face_status 同构）：

- **独立状态列** `media_items.exotic_status`：仿 `ai_status`(V2)/`face_status`(V8)。**不复用** `media_derivations`（并入即破坏 G1）。语义 **0 待处理 / 1 处理中 / 2 完成 / 3 错误**（O1 倾向不引入 4/5 持久门控态）。
- **插件缓存表** `exotic_plugins`：磁盘 manifest 是真相，但"已装/已授权"需快速查 → 落一张缓存表，启动时由 host 扫 `plugins/` 重建、license 校验后写 `authorized`。
- **格式路由不落库**：`format→plugin` 是内存结构（host 启动由 manifests 构建），随插件增删动态变化，无需持久化。
- **config 默认值**：`exotic_enabled`(总开关) / `exotic_auto_process`(随扫描自动处理已授权格式) / `exotic_max_workers`(0=自动)。

```sql
-- 概览（完整带注释 DDL 见 Part1）。迁移块 if version < V_next 守护，仅执行一次。
ALTER TABLE media_items ADD COLUMN exotic_status INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_media_exotic ON media_items(exotic_status)
                                            WHERE exotic_status IN (0,1,3);
-- exotic_plugins(id PK, name, version, media_kind, formats(JSON), capabilities(JSON),
--                license_tier, authorized, commercial_ok, installed_at)
-- app_config: exotic_enabled=1 / exotic_auto_process=1 / exotic_max_workers=0
```

> **门控态判定（O1 倾向 (a)）**：后端命令 `exotic_format_status()` 返回"格式→{installed,authorized,available}"映射；前端 `MediaThumb` 据 `item.file_format` 命中冷门表且 `!authorized` → 渲染锁占位。`exotic_status` 不承载门控，只承载已授权项的处理进度。

## 7. 编解码许可策略 | Codec Licensing Strategy（业界调研核心产出 / Core research payload）

> 这是 v1 完全缺失、却最影响"每类插件能不能做、怎么做"的一节。**冷门格式的难点 90% 不在技术、在许可与专利。**
> 一句话总纲：**软件许可（GPL/LGPL/MIT）与专利（HEVC 池等）是两条独立的合规线，必须分别清算。**

### 7.1 四档取材策略 | Four-tier sourcing strategy（D8）

每个待支持格式按"解码器从哪来"归入下面**唯一一档**，决定它的工程与商业路线：

| 档 | 取材方式 | 法律/专利状态 | 可否捆绑进付费插件包 | 典型格式 | 工程路线 |
|---|---|---|---|---|---|
| **T1 纯 Rust·宽松许可** | crates.io 上 MIT/Apache 的纯 Rust 解码器 | 最干净，无专利顾虑（格式本身无专利或已过期） | ✅ 可自由捆绑 | **PSD**(`psd` crate)、部分 RAW(`rawloader`)、TGA/部分老格式 | worker 静态链接，最省心；**PSD 切片即此档** |
| **T2 OS 委托** | 调系统解码器（Win HEIF 扩展 / WIC / Media Foundation；macOS ImageIO/AVFoundation） | **专利由 OS 厂商许可覆盖**，我方零专利暴露 | ✅ worker 仅调系统 API，无第三方二进制 | **HEIC/HEIF**(首选)、部分系统已装编解码的视频 | worker 调 OS API；缺扩展时引导用户装系统扩展，**不**自带 HEVC 解码器 |
| **T3 LGPL·动态链接 sidecar** | LGPL 库，**动态链接**且用户可替换 | LGPL 商用 OK（动态链接、可替换）；**但专利另算**（见 7.3） | ⚠️ 可，但须满足 LGPL 动态链接+可替换+附 LICENSE，且评估专利 | **RMVB/RM**(FFmpeg LGPL 构建)、冷门视频容器、HEIC 兜底(libheif+libde265) | worker 同目录放 `.dll/.so`，**绝不 `--enable-gpl`**；附第三方 LICENSE |
| **T4 GPL/专利重·用户侧自装** | GPL 库 或 专利/许可不可商业再分发的解码器 | **不可**由我方闭源捆绑或售卖 | ❌ 禁止捆绑 | x265 编码、APE(Monkey's Audio)、部分非商用模型权重、`--enable-gpl` 的 FFmpeg | 插件包**不含**该二进制；提供"指引用户自行安装"路径，由用户完成 GPL 组合 |

`✶ 为什么 T4 还能做 ────────────────────────`
GPL 的传染性发生在"分发组合体"那一刻。若**我方只分发一个会去调用"用户自己安装的 GPL 程序"的薄 worker**，
组合动作发生在用户机器、由用户完成，则我方未分发 GPL 组合体——这正是许多商业软件
（如调用用户已装 FFmpeg/LibreOffice）规避 GPL 传染的合法姿势。代价：体验差一截（用户要自己装）。
进程隔离架构让 T4 天然可行——worker 与 GPL 程序是两个进程，边界清晰。
`──────────────────────────────────────────`

### 7.2 逐格式雷区矩阵 | Per-format landmine matrix（落地前须亲验 Verify before shipping）

⚠️ **下表是调研结论 + 工程建议，非法律意见。任何格式落地前，务必由你/法务亲验当下许可与专利状态**（呼应人脸计划"许可须亲验"原则）。

| 格式 | media_kind | 软件许可 | 专利雷区 | 建议档 | 关键提示 |
|---|---|---|---|---|---|
| **PSD/PSB** | image | `psd` crate MIT | 无（格式公开、无活跃专利池） | **T1** | ✅ 最干净，**作 v1 切片**；纯 Rust 静态链 |
| **HEIC/HEIF** | image | libheif/libde265 = **LGPL**（动态链 OK） | **HEVC 专利池**(Access Advance / MPEG LA)，解码亦可能涉及，按设备/量计费 | **T2 首选 / T3 兜底** | **优先 OS 解码**（Win HEIF 扩展、macOS 原生）规避专利；自带 libde265 须评估 HEVC 池义务 |
| **AVIF** | image | libaom/dav1d = BSD/MIT（dav1d 极快） | AV1 专利相对开放（AOMedia），仍有 Sisvel 主张争议 | **T1/T3** | dav1d 解码 BSD 干净；较 HEVC 友好 |
| **RAW**(cr2/nef/arw/dng…) | image | LibRaw = **LGPL/CDDL 双许可**；`rawloader`(Rust)=LGPL；部分纯 Rust MIT | 多数无活跃专利；个别厂商加密(如 Canon CR3 部分) | **T1/T3** | LibRaw 商用友好但属 LGPL→动态链；DNG 最规范 |
| **RMVB/RM** | video | **FFmpeg 原生 RealVideo 解码器 = LGPL**（逆向，不依赖 RealPlayer） | RealVideo 较老，专利风险低 | **T3** | FFmpeg 构建**只 LGPL、绝不 `--enable-gpl`**；勿用 RealNetworks 自有 RPSL/RCSL 码 |
| **MKV/WebM/FLV 等冷门容器** | video | FFmpeg LGPL | 视内含编码而定（H.264/HEVC 有池；VP9/AV1 较开放） | **T3** | 容器解析 LGPL 干净；编码层专利随内容 |
| **Office**(doc/docx/xls/ppt…) | document | OOXML(docx/xlsx)=ZIP+XML 可自解析；旧 .doc=OLE 二进制更难；完整渲染需 LibreOffice(**MPL/LGPL**,体积巨大) | 无专利雷区 | **T1**(自解析取首页/元数据) **/ T4**(LibreOffice 渲染走用户侧) | 缩略图可仅渲染 docx 内嵌预览图/首段；高保真渲染体积太大，建议用户侧 |
| **APE**(Monkey's Audio) | audio | **专有许可，非 OSI、限制再分发** | 无专利但许可受限 | **T4** | ❌ 不捆绑；FFmpeg 有解码但源码许可需查；建议用户侧或不支持 |
| **FLAC/ALAC/DSD** | audio | FLAC=BSD；ALAC=Apache(苹果开源)；DSD 格式公开 | 无 | **T1** | 干净，可纯 Rust(`symphonia` 支持多数)直接捆绑 |
| **WMA/WMV** | audio/video | 走 OS(Media Foundation) 或 FFmpeg LGPL | 微软专利，OS 解码已覆盖 | **T2/T3** | Windows 上优先 MF |

### 7.3 软件许可 vs 专利：两条独立合规线 | License ≠ Patents

调研确认（见信源）：

- **软件许可线**：LGPL（libheif/libde265/LibRaw/FFmpeg 默认）→ **动态链接 + 用户可替换 + 附许可文本** 即可商业闭源使用；GPL（x265、`--enable-gpl` 的 FFmpeg）→ 闭源捆绑即传染，**致命**，只能走 T4 用户侧。
- **专利线（独立）**：HEVC/H.265 由 **Access Advance（HEVC Advance 池）** 与 MPEG LA 等管理，按 5 年不可终止增量授权、按设备/单元计费；**即使用 LGPL 的 libde265，专利义务依然可能存在**（"软件中仅解码是否豁免"存在社区争议、无定论）。→ **规避之道 = T2 OS 委托**（专利由 OS 厂商整机授权覆盖），这是大量商业软件的实际做法。
- **实务建议**：v1 切片选 PSD（**两条线都干净**）。HEIC 这类高需求但专利重的格式，**首选 OS 委托**；若必须自带解码器，单独法务评估 HEVC 池义务后再定价。

### 7.4 信源 | Sources

- HEVC/HEIC 专利与 libheif/libde265 许可：[HEVC Advance — Access Advance](https://accessadvance.com/licensing-programs/hevc-advance/)、[libheif #591 license/patents](https://github.com/strukturag/libheif/issues/591)、[strukturag/libheif](https://github.com/strukturag/libheif)
- Sidecar/子进程隔离模式：[Sidecar Pattern — Azure Architecture Center](https://learn.microsoft.com/en-us/azure/architecture/patterns/sidecar)、[Library vs Service vs Sidecar](https://atul-agrawal.medium.com/library-vs-service-vs-sidecar-ff5a20b50cad)
- FFmpeg LGPL/GPL 与 RealMedia：[FFmpeg Legal](https://www.ffmpeg.org/legal.html)、[FFmpeg Commercial License Guide](https://32blog.com/en/ffmpeg/ffmpeg-commercial-license-guide)、[RealVideo — Wikipedia](https://en.wikipedia.org/wiki/RV40)

## 8. 插件形态选型 | Plugin Form Factor（业界佐证 / Industry-backed）

三种主流原生插件形态对比（针对"重型媒体解码 + 付费 + 跨平台 + 隔离"诉求）：

| 形态 | 隔离性 | ABI 稳定性 | 跨平台分发 | 性能 | 安全 | 结论 |
|---|---|---|---|---|---|---|
| **A. 动态库 cdylib（libloading）** | ✗ 同进程，崩溃拖垮主程序 | ✗ Rust 无稳定 ABI，须 `extern "C"`/`abi_stable` 苦工 | △ 每平台 .dll/.dylib/.so | ◎ 原生零 IPC | ✗ 加载任意 dll 风险高 | **否决**（违背 G2 隔离、G3 ABI 脆弱） |
| **B. Worker 子进程 + stdio IPC** | ◎ **进程级隔离**，崩溃只丢一项 | ◎ 协议即序列化，天然稳定 | ◎ 每平台一个可执行 | ○ IPC 开销可忽略（只传路径与小缩略图字节） | ○ 下载二进制须签名校验，边界清晰 | ✅ **采用（D1）** |
| **C. WASM 沙箱** | ◎ 沙箱 | ◎ 稳定 | ◎ 单产物 | ✗ 解码库 WASM 生态不全、像素搬运贵 | ◎ 最安全 | 未来可选（轻量纯算格式），重解码不合适 |

**业界佐证**（§7.4 信源）：Sidecar/子进程隔离是成熟模式——"用进程级隔离给系统关键件加护栏，一个组件崩溃不影响整体"；延迟低于独立服务、略高于库内调用，但换来**容错 + 跨语言 + 可独立调优**。同类先例：**GIMP 插件**（独立进程 + wire 协议）、**LSP 语言服务器**（子进程 + JSON-RPC）、**Tauri sidecar**。本设计与之同构。

逐条对齐用户诉求：

- **"避免影响常见格式速度""不强行并入"** → 独立进程，地址空间分开，主程序解 JPEG 与 worker 解 PSD 物理隔离。
- **"容易造成问题"** → 畸形文件让 worker 崩溃，主程序只收错误码标 `exotic_status=3` 继续——比主路径的 `catch_unwind`（`generator.rs:49`，**只拦 Rust panic、拦不住 C 库段错误/死循环/内存爆炸**）更强。
- **"方便后期性能调优"** → worker 可独立设优先级、CPU 亲和、内存上限、并发数，互不影响。
- **"做好扩展插口"** → 新格式 = 新 worker + manifest，主程序按协议通信，**无需重编**。这是 cdylib 做不到的开闭性。

> Worker **不经** Tauri `externalBin`（编译期捆绑，与"购买后下载"矛盾），改为运行期下载到 `{app_data}/plugins/<id>/`，后端 `std::process::Command` 以**绝对路径**启动（后端 Rust 不受 Tauri shell allowlist 限制），启动前**强制 sha256 + 签名校验**（Part3 P5）。

---

## 9. 风险登记册 | Risk Register

| # | 风险 | 等级 | 缓解 |
|---|---|---|---|
| R1 | **许可/专利误用**（捆绑 GPL 或 HEVC 专利件） | 🔴 高 | §7 四档策略 + `commercial_ok` 字段 + 落地前法务亲验 + T2 OS 委托规避专利 |
| R2 | **下载执行第三方二进制的安全面** | 🔴 高 | 三道闸：sha256（传输完整性）+ manifest.sig 验签（来源真实性，间接锁二进制 hash）+ 启动前再校验（防安装后替换）。Part3 P5 |
| R3 | **schema 版本撞号**（与人脸 TODO2 争 V9） | 🟡 中 | D9：取落地时下一个空位，文中记 `V_next`；落地第一步先 grep 确认当前最高版本 |
| R4 | **worker 崩溃/死循环/内存爆炸** | 🟡 中 | 进程隔离 + `task_timeout_ms` 超时杀进程 + 崩溃自动重启 + 标 `exotic_status=3` 继续；v2 经 Job Object/cgroup 强约束内存 |
| R5 | **让步反馈循环饿死**（参考 enrichment 教训） | 🟡 中 | worker 是独立进程，让步只需 Dispatcher **暂停派发新任务**（在途自然跑完），不像 rayon 让步会与 2s 自动重排互锁；见 [[gotcha-background-yield-model]] |
| R6 | **license 被逆向绕过** | 🟢 低（可接受） | 任何本地校验皆可被逆向（商业软件通病）；O3 列增量手段，v1 切片不做 |
| R7 | **协议漂移**（主程序与 worker 版本不一致） | 🟡 中 | 三版本号（manifest.schema / protocol_version / min_host_version）握手校验，不匹配拒绝启用并提示升级。Part2 P3 |
| R8 | **百万级库下逐文件 spawn 开销** | 🟡 中 | D7 worker 长驻，流式处理多任务 |
| R9 | **跨平台 worker 缺失**（某平台无对应可执行） | 🟢 低 | manifest.bin 按 target triple 查；缺则该平台标"该格式暂不支持本平台" |
| R10 | **PSD 大文件/多图层内存峰值** | 🟡 中 | worker 内 `mem_soft_limit_mb` + 仅取合成预览图（PSD 通常内嵌合成图，无需重算图层合成） |

## 10. 路线图 | Roadmap：分卷 × Phase × 续作

### 10.1 全景 | Phase map

```
Part1 地基            Part2 引擎             Part3 商业            Part4 收口
─────────────        ─────────────         ─────────────        ─────────────
P0 骨架+schema   ──► P3 协议+PSD worker ─► P5 License+签名   ─► P7 前端UI
P1 扫描识别+让路      P4 独立流水线+回填     P6 注册表+下载安装    P8 PSD端到端切片
P2 宿主路由+门控                                                 P9 硬化+测试
```

依赖关系（关键）：
- **Part1 是一切的地基**，必须先落（schema + 识别 + 路由 + 门控查询）。落完即可：冷门项被识别入库、被标记、门控状态可查询——**此时前端已能渲染占位**（即便 Part4 未做，用占位也是可见进展）。
- **Part2 产出"引擎本体"**：用一个 **dev 模式手放的、未签名的** PSD worker 即可端到端产出缩略图，**不依赖 Part3 的 license/下载**（开发期用 `exotic_dev_mode` 跳过门控）。
- **Part3 给 Part2 套上商业外壳**：签名、license、远程下载安装。
- **Part4 收口用户体验** + 上线硬化。

> **可独立验收的里程碑**：Part1 完成 = 占位可见；Part2 完成 = PSD 缩略图可见（dev 模式）；Part3 完成 = 购买→下载→激活闭环；Part4 完成 = 正式可发布的 PSD 切片。

### 10.2 各分卷的"新会话续作提示词" | Continuation prompts

每个分卷文件**末尾自带一段"新会话续作提示词"**，包含：① 本卷目标一句话；② 前置依赖（哪些 Part/Phase 必须先完成）；③ 必读文件清单（本 plan 卷 + 相关源码路径）；④ 验收 DoD；⑤ 给新会话的起手 prompt 模板。

**在新会话续作的标准起手式**（复制到新会话）：

```
读 plan-docs/exotic_format_plugin_plan_v2.md（总纲）与 plan-docs/exotic_format_plugin_partN_*.md（本卷）。
按本卷 Phase 顺序施工。遵守仓库约定：thiserror / rusqlite 参数绑定 / Vue3 Composition+TS strict /
中英双语注释与日志 / 中文 commit / 改动及时 commit / 产物过大分多次 Edit。
先复述本卷 DoD 与前置依赖核验结果，再动手。
```

### 10.3 工作量与顺序建议 | Effort & sequencing

| 分卷 | 粗估规模 | 可并行？ | 建议先后 |
|---|---|---|---|
| Part1 | 中（schema + 2 命令 + walker/generator 小改 + host 骨架） | host 路由与 schema 可并行 | **最先** |
| Part2 | 大（协议 crate + worker crate + pipeline 4 线程 + 回填接线） | 协议 crate 定稿后 worker 与 pipeline 可并行 | 紧随 Part1 |
| Part3 | 中（license + package + registry + 4 命令 + 下载复用） | license 与 registry 可并行 | Part2 后 |
| Part4 | 中（exoticStore + 占位渲染 + 设置面板 + 端到端 + 硬化） | 前端与硬化可并行 | 最后 |

## 11. 术语表 | Glossary

| 术语 | 含义 |
|---|---|
| **冷门格式 Exotic format** | 主引擎不解码的格式；含 A 类(已分类未解码)与 B 类(未分类) |
| **Host 宿主** | `exotic/host.rs`，发现插件、建路由表、查 license 门控 |
| **Worker** | 进程隔离的解码可执行，长驻，按协议处理任务 |
| **Manifest 清单** | `manifest.json`，主程序↔插件唯一契约（formats/capabilities/media_kind/bin/limits/license） |
| **路由表 Route table** | 内存 `format → plugin_id` 映射，host 启动构建 |
| **门控 Gating** | license 校验结果（已授权/需购买/无插件） |
| **media_kind** | image/video/audio/document，决定 worker 产物落地路径 |
| **capability** | worker 支持的 op：thumbnail/metadata/text |
| **`commercial_ok`** | 插件内解码栈是否允许商业分发（§7 亲验） |
| **T1–T4** | 四档取材策略（§7.1） |
| **`V_next`** | 落地时下一个空闲 schema 版本号（避免撞号，§2.3/D9） |
| **dev 模式** | 开发期跳过 license 门控、手放 worker 的开关（`exotic_dev_mode`） |

---

## 12. 附录 A：四类媒体插件蓝图 | Plugin Blueprints（验证契约普适性）

同一契约容纳四类，证明插口设计无类型偏置。**仅 `exotic-image-psd` 为 v1 切片实现，其余为契约预留示例。**

| 插件示例 | media_kind | formats | capabilities | 建议档 | worker 解码栈（建议） |
|---|---|---|---|---|---|
| **exotic-image-psd**（v1 ✅） | image | psd, psb | thumbnail, metadata | T1 | 纯 Rust `psd` crate + `image`(webp) |
| exotic-image-heic | image | heic, heif | thumbnail, metadata | T2/T3 | **优先 OS**(Win HEIF/WIC、macOS ImageIO)；兜底 libheif+libde265(动态链) |
| exotic-image-raw | image | cr2,cr3,nef,arw,dng,raf,orf,rw2,pef,srw | thumbnail, metadata | T1/T3 | `rawloader`/LibRaw(动态链)；多数 RAW 内嵌 JPEG 预览可快取 |
| exotic-video-rmvb | video | rmvb, rm | thumbnail, metadata | T3 | FFmpeg(**LGPL only**, worker 自带 dll)取封面帧+时长 |
| exotic-audio-hires | audio | ape, dsf, dff, tak | thumbnail, metadata | T1(flac 系)/T4(ape) | `symphonia` 覆盖部分；APE 走 T4 用户侧 |
| exotic-doc-office | document | doc,docx,xls,xlsx,ppt,pptx | thumbnail, metadata, text | T1/T4 | docx/xlsx 自解析取内嵌预览+元数据；高保真渲染走用户侧 LibreOffice |

> 横向扩展节奏（建议）：PSD(切片) → RAW(高需求、T1 干净) → HEIC(高需求但走 T2 OS 委托) → RMVB(T3) → Office(T1 元数据先行) → Hi-Res 音频。

---

> **下一步**：按 §10 路线图，从 `exotic_format_plugin_part1_foundation.md` 开始施工。
> 本总纲为活文档，决策变更（尤其 §5 待定项、§7 许可亲验结论）应回写本文件。
