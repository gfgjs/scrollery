---
id: 2026-07-24-Spec00_产品与架构全景
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec00 · 产品与架构全景

> 一句话:本篇是 17 篇 as-built 重建级规格集的**门面/导航篇**——服务两类读者(新加入的人类工程师、上下文有限的低能力 LLM 编码代理),建立整机心智模型(产品是什么、分几层、跑几个进程、用什么栈),再指路到各子篇的深机制。读前无须先读其他篇;读后按「§7 导航」挑要去的篇。

## 1. 概览

### 1.1 产品定位

**Scrollery**(中文名"画卷")是面向 Windows / macOS / iOS / Android 的高性能跨平台资产管理器(照片/视频/文档/音频的本地库管理、浏览、编辑、备份)。这是 **as-built 正式名**——代码与本文档集一律使用 `Scrollery`,不使用旧代号 `Picasa Next`(旧代号仅见于历史文档标题,如 `docs/refactor_2026/Part0_总纲与产品定稿.md:9` 的文档标题,该文档是 2026-06-26 起草时的重构计划,命名当时未定案)。改名定案见 [`docs/decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md`](../decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md);app identifier 为 `com.scrollery.app`。

- **目标用户**:拥有大规模本地媒体库(百万级文件量级,`docs/refactor_2026/Part2_扫描与画廊流水线.md` 谈及的"百万级布局"即此量级)、看重隐私(数据不上云)与性能(GPU 加速缩略图/视频管线)的用户。
- **商业模型**:核心功能免费 + 冷门格式插件(exotic)买断制。插件平台机制见 [Spec09](./Spec09_插件平台与exotic.md);盈利逻辑、竞品分析、护城河的完整理由链见 `../refactor_2026/Part0_总纲与产品定稿.md §10`(不复制其论证,如 §10.3 防白嫖设计、§10.6 非代码护城河)。
- **现状注(as-built vs 目标)**:四平台是目标形态,**当前仓库代码仅 Windows 线全量施工**。证据:
  - 视频硬解后端 `src-tauri/src/video/media_foundation.rs` 是 Win32 Media Foundation 专属实现,`src-tauri/src/lib.rs` 中 `video` 模块的平台代码由 `cfg(windows)` 门控(详见 [Spec05](./Spec05_视频与音频.md));
  - 生产 CSP(`src-tauri/tauri.conf.json:44`)当前只含 `http://ipc.localhost` / `http://asset.localhost` 形式的 origin,**不含 `tauri:` scheme**——按项目硬约束(`../../AGENTS.md`)macOS/iOS/Linux 构建启动时必须补 `tauri:` 并做设备验证,这一步尚未发生;
  - `src-tauri/Cargo.toml` 中 `windows = "0.58"` / `windows-core = "0.58"` 依赖仅在 `cfg(windows)` 生效,未见 macOS/iOS 等价的原生层依赖。
  这些平台缺口分别详述于 [Spec05](./Spec05_视频与音频.md)(视频后端)与 [Spec13](./Spec13_构建发布商业化.md)(CSP 分档/构建矩阵)。

### 1.2 代码位置总览

| 位置 | 内容 |
|---|---|
| `src-tauri/src/` | Rust host 进程,26 个模块(见 §3 模块表) |
| `src-tauri/capabilities/` | Tauri v2 权限声明(`default.json` 主窗口、`logs.json` 日志窗口) |
| `crates/exotic-workers/` | 独立可执行 worker crate:`ai-worker`、`psd-worker`、`psd-probe`(workspace member,见 `Cargo.toml:24-36`) |
| `crates/exotic-protocol` | host↔worker IPC 帧协议(共享 crate) |
| `crates/scrollery-ai-core` | AI 契约面:host 依赖此 crate 而非直接依赖 `ort`/`tokenizers`(隔离推理) |
| `crates/scrollery-plugin-api` / `scrollery-exotic-trust` | 插件平台契约、签名验证/许可证(见 [Spec09](./Spec09_插件平台与exotic.md)) |
| `src-tauri/src/exotic/license.rs` | keyring 直销授权实现 + 未授权 fail-closed 回退(商业授权实现在宿主内,见 [Spec13](./Spec13_构建发布商业化.md)) |
| `src/` | Vue 3 前端(webview 内运行,见 §3 前端结构) |

### 1.3 整机架构图

```
┌─────────────────────────────── webview (Vue 3 SPA) ───────────────────────────────┐
│  main 窗口(路由化 AppShell)          logs 窗口(独立 label,共享同一 index.html)   │
│  src/main.ts:32 按 window label 分流 —— label == 'logs' 时挂载 LogWindowView.vue   │
└───────────────────────────────────────┬────────────────────────────────────────────┘
                                         │ Tauri IPC(invoke,src/utils/ipc.ts::invokeIpc)
                                         ▼
┌───────────────────────────── host(Rust,scrollery_lib) ─────────────────────────────┐
│  src-tauri/src/lib.rs::run()(:89)── Builder::invoke_handler(ipc::registry::handler())│
│  UI 相关短任务留 tokio 异步线程;DB/CPU 重活经 spawn_blocking(§4 并发模型)           │
│  26 个模块(db/scanner/thumbnail/video/ai/backup/exotic/... ,见 §3)                  │
└───────┬───────────────────────┬──────────────────────────┬─────────────────────────┘
        │ 子进程(exotic-protocol 帧)│                       │ 子进程(同协议)
        ▼                       ▼                          ▼
┌───────────────┐     ┌──────────────────┐        ┌──────────────────────┐
│  ai-worker     │     │  psd-worker/probe │        │  其它 exotic 插件二进制 │
│(host 零 ort/   │     │(冷门格式解码探针) │        │(按需下载+验签定位,     │
│ tokenizers,    │     │                   │        │ 见 Spec09)            │
│ CLIP/人脸/OCR  │     └──────────────────┘        └──────────────────────┘
│ 推理隔离于此)  │
└───────────────┘
```

- 前端与 host 之间**只**经 Tauri IPC 通信(命令名枚举于 `src/constants/ipc.ts`,统一调用封装 `src/utils/ipc.ts::invokeIpc`),错误经 `AppError`(`src-tauri/src/error.rs:11`)结构化回传,前端 `parseAppError()` 解析为 `IpcError`(`src/utils/ipc.ts` 同文件)。完整命令表/错误码见 [Spec10](./Spec10_IPC与错误契约.md)。
- host 与各 worker 子进程之间经 `exotic-protocol` crate 定义的帧协议通信,不共享内存/不共享 Rust 类型系统边界。AI 推理与冷门格式解码物理隔离到子进程,是刻意的架构决策(降低崩溃/内存暴涨对主进程的影响面,推理库 `ort`/`tokenizers` 不链入 host 二进制)。

## 2. 数据模型与状态(全景摘要)

本篇不定义新类型,只给"状态归属在哪"的顶层地图,细节权威见对应子篇:

| 状态类别 | 归属 | 权威篇 |
|---|---|---|
| 媒体库结构化数据(媒体条目、扫描根、EXIF、评分、标签、收藏、人脸库…) | SQLite(rusqlite,单文件 DB,连接池 `src-tauri/src/db/connection.rs`) | [Spec01](./Spec01_数据层.md) |
| 派生产物(缩略图 WebP、视频关键帧 sprite、文档缩略图) | 文件系统 `<app_data>/thumbs/` | [Spec03](./Spec03_缩略图与派生.md) |
| 应用配置(扫描路径、性能参数、渠道/授权) | `<app_data>/config.toml`,`src-tauri/src/config/toml.rs` | [Spec12](./Spec12_配置状态日志.md) |
| 运行期任务状态(扫描进度、取消令牌槽 `RunTokenSlot`) | 内存 `AppState`,`src-tauri/src/state.rs:317-389` | [Spec12](./Spec12_配置状态日志.md) |
| 前端 UI/业务状态 | Pinia,19 个 store(`src/stores/`,见 §3.2 表) | [Spec11](./Spec11_前端架构.md) |
| 日志/诊断 | JSON 结构化日志 + 诊断包,`src-tauri/src/logging/` | [Spec12](./Spec12_配置状态日志.md) |

## 3. 关键流程与算法 —— 26 后端模块 + 前端结构一览

### 3.1 后端 26 模块(`src-tauri/src/lib.rs:25-55` mod 声明)

| 模块 | 一句话职责 | 详解归属 |
|---|---|---|
| `ai` | 语义搜索、嵌入缓存、模型管理;推理恒在 `ai-worker` 子进程,host 零 `ort` | [Spec06](./Spec06_AI人脸OCR.md) |
| `audio` | 音频封面/标签/歌词提取(`lofty`) | [Spec05](./Spec05_视频与音频.md) |
| `backup` | 数据备份与恢复流水线(预检→备份→恢复→重启生效) | [Spec08](./Spec08_存储备份导出文件操作.md) |
| `config` | `config.toml` 读写、热加载(`notify`)、设置持久化 | [Spec12](./Spec12_配置状态日志.md) |
| `db` | 连接池、迁移、schema、分类查询、自定义排序规则 | [Spec01](./Spec01_数据层.md) |
| `derive` | 派生流水线:视频封面/关键帧、文档缩略图、音频元数据 | [Spec03](./Spec03_缩略图与派生.md) |
| `download` | 模型/资产下载(AI/OCR/人脸模型) | [Spec06](./Spec06_AI人脸OCR.md) |
| `editing` | 图片简单编辑:旋转/裁剪/调色(ICC CMS) | [Spec04](./Spec04_图像与色彩管线.md) |
| `engine` | 扫描引擎核心:文件系统遍历、媒体识别、增量更新 | [Spec02](./Spec02_扫描与画廊.md) |
| `error` | IPC 错误统一类型 `AppError`(`src-tauri/src/error.rs:11`) | [Spec10](./Spec10_IPC与错误契约.md) |
| `exotic` | 冷门格式插件平台:Catalog、授权、Registry、安装管理 | [Spec09](./Spec09_插件平台与exotic.md) |
| `export` | 整理成果导出:目录结构、文件复制、mtime 保留 | [Spec08](./Spec08_存储备份导出文件操作.md) |
| `formats` | 已注册格式运行时并集(内置 + exotic Catalog 投影) | [Spec09](./Spec09_插件平台与exotic.md) |
| `ipc` | 命令注册与派发,27 个子模块、229 条 Tauri 命令,统一注册点 `src-tauri/src/ipc/registry.rs::handler()` | [Spec10](./Spec10_IPC与错误契约.md) |
| `layout` | 画廊布局引擎:行布局计算、视图 ID 映射、Y 坐标查询 | [Spec02](./Spec02_扫描与画廊.md) |
| `logging` | 结构化 JSON 日志、诊断包、span 埋点 | [Spec12](./Spec12_配置状态日志.md) |
| `proofread` | 远程 AI 校对(文档内容校正,OpenAI/LLM 集成) | [Spec07](./Spec07_文档与阅读器.md) |
| `reader` | 文档阅读器:编码检测、分章、简繁转换、书签 CRUD | [Spec07](./Spec07_文档与阅读器.md) |
| `scanner` | 扫描根管理、重链、隐显、重扫控制 | [Spec02](./Spec02_扫描与画廊.md) |
| `state` | 全局 `AppState`、任务调度、事件通道 | [Spec12](./Spec12_配置状态日志.md) |
| `storage` | 存储后端:卷管理、WebDAV(perf 特性)、本地路径 | [Spec08](./Spec08_存储备份导出文件操作.md) |
| `thumbnail` | 缩略图生成:WebP 有损、sprite、LRU 缓存、进度追踪 | [Spec03](./Spec03_缩略图与派生.md) |
| `tree` | 文件树数据源:受限 FS 访问、"所有文件"两态 | [Spec02](./Spec02_扫描与画廊.md) |
| `utils` | 自然排序、路径规范化、哈希、文件操作 | 按调用方归属篇就近摘要 |
| `video` | 视频处理:Media Foundation 同步取帧、DXVA 硬解、XVP 尺寸协商(Windows 专属,§1.3 现状注) | [Spec05](./Spec05_视频与音频.md) |
| `viewer_color` | 查看器色域管理:sRGB/P3/DCI-P3 切换、ICC 导入 | [Spec04](./Spec04_图像与色彩管线.md) |

> IPC 命令共 229 条,分布在 `src-tauri/src/ipc/` 下 27 个 `*_commands.rs` 文件;全量分类表与错误码是 [Spec10](./Spec10_IPC与错误契约.md) 的权威内容,本篇不重复枚举。

### 3.2 前端结构(`src/`)

| 目录 | 一句话职责 |
|---|---|
| `src/components/` | Vue SFC 组件库,按功能域分类(画廊/查看器/播放器/编辑器/设置…) |
| `src/composables/` | ≥30 个 `useXxx` 可组合函数(调色、布局、交互、编辑) |
| `src/stores/` | Pinia,19 个 store(见下表) |
| `src/router/` | Vue Router,16 条路由(见下表) |
| `src/views/` | 路由目标页面组件 |
| `src/utils/ipc.ts` | 统一 IPC 调用 `invokeIpc()` + 错误解析 `parseAppError()` |
| `src/constants/ipc.ts` | 全部后端命令名枚举(`IPC.*`),杜绝裸字符串 |
| `src/themes/` | 6 主题 CSS 变量系统 |
| `src/i18n/` | vue-i18n 中英双语 |
| `src/harness/` | 浏览器 UI 测试 harness(IPC mock) |

**Pinia 19 store**(`src/stores/`,详细字段见 [Spec11](./Spec11_前端架构.md)):`uiStore`、`configStore`、`viewerStore`、`mediaStore`、`searchStore`、`filterStore`、`viewStore`、`historyStore`、`scanStore`、`fileJobStore`、`aiStore`、`faceStore`、`personStore`、`collectionStore`、`derivationStore`、`exportStore`、`backupStore`、`toastStore`、`logWindowStore`。

**路由 16 条**(`src/router/`):`/`、`/folder/:id`、`/favorites`、`/live-photos`、`/recent`、`/collections`、`/collections/:id`、`/persons`、`/persons/:id`、`/plugins`、`/settings/:section?`、`/doc/:id`、`/audio/:id`、`/view/:id`、`/hgallery-lab`、`/trash` —— 均为 `MediaGrid.vue` 复用同一网格组件按过滤条件区分,或各自专属页面(`DocumentViewer.vue`/`AudioPlayer.vue`/`ContentViewer.vue`/`PersonsView.vue`/`PluginStoreView.vue`/`SettingsView.vue`/`HGalleryLabView.vue`),完整职责见 [Spec11](./Spec11_前端架构.md)。

### 3.3 窗口分流(双窗口,单一入口)

`src/main.ts` 是 main 窗口与 logs 窗口**共享的同一 entry**,按 Tauri 窗口 `label` 分流:

1. `src/main.ts:31` 判定是否运行于 Tauri(`'__TAURI_INTERNALS__' in window`)。
2. `src/main.ts:32`:`isUiHarness || !isTauri` 时视为 `main`(裸浏览器 dev 场景兜底),否则读 `getCurrentWindow().label`。
3. `label === 'logs'`(`src/main.ts:34-43`):**动态** `import('./views/LogWindowView.vue')`,只挂 Pinia + i18n,不接路由/不接 AppShell——刻意用动态 import 避免日志窗口专属依赖(如 `@tanstack/vue-virtual`)被静态打包进两窗口共享的主 chunk,踩破打包体积预算门(`src/main.ts:35-37` 注释)。
4. 否则(main 窗口,`src/main.ts:45-53`):挂载完整 `App.vue`(含路由 `router`、i18n),`App.vue` 即 `AppShell`(标题栏/侧栏/内容区/底栏/命令上下文,`src/App.vue`)。

两窗口的 Tauri 权限声明分离:`src-tauri/capabilities/default.json`(主窗口,`"windows": ["main"]`)、`src-tauri/capabilities/logs.json`(日志窗口专用,更窄权限)。权限模型细节见 [Spec10](./Spec10_IPC与错误契约.md)/[Spec14](./Spec14_不变量与约定.md)。

## 4. 进程与线程模型

| 进程 | 二进制/入口 | 关键约束 |
|---|---|---|
| **host 主进程** | `scrollery_lib`(`src-tauri/src/lib.rs::run()`,`:89`,由 `src-tauri/src/main.rs:7` 的 `fn main()` 调起) | 持有 Tauri `Builder`、DB 连接池、全部 IPC 命令注册;**不链接 `ort`/`tokenizers`/`ndarray`**(AI 推理隔离到 `ai-worker`) |
| **ai-worker 子进程** | `crates/exotic-workers/ai-worker`(workspace member,`Cargo.toml:29`) | CLIP/人脸(SCRFD+ArcFace)/OCR(PP-OCRv5)推理执行地;经 `exotic-protocol` 帧与 host 通信;详见 [Spec06](./Spec06_AI人脸OCR.md) |
| **exotic 插件 worker** | `crates/exotic-workers/psd-worker`、`psd-probe` 等 | 冷门格式解码/探测,按需下载 + 运行期验签定位(非 Tauri sidecar externalBin 机制,详见 [Spec09](./Spec09_插件平台与exotic.md)、`../refactor_2026/Part7_发布工程.md §2.3`) |
| **webview** | main 窗口 + logs 窗口(见 §3.3) | 前端 Vue 3 SPA,经 Tauri IPC 与 host 通信,自身不直接访问文件系统/DB |

**host 内部线程职责划分**(项目硬约束,`../../AGENTS.md`):

- **UI/异步线程**(tokio 运行时,`tokio = { features = ["full"] }`,`src-tauri/Cargo.toml`):IPC 命令入口、事件分发、编排逻辑。
- **`spawn_blocking`**:DB 查询、CPU 密集任务(缩略图编解码、图像处理)必须下沉,不得占用 tokio worker 线程。例证:`src-tauri/src/state.rs:553` 的 `tokio::task::spawn_blocking` 用于 NATURAL_CMP 全库排序查询(注释标注实测 ~546ms)。
- **锁纪律**:`std::sync::Mutex` guard **绝不**跨 `.await`(`src-tauri/src/state.rs:230` 注释重申);需要跨 `spawn_blocking` 持有的场景改用 `tokio::sync::Mutex`(`src-tauri/src/state.rs:271` 附近示例)。
- **DB 写路径**:单一 writer 连接或专属 DB 线程(`db/connection.rs`),读走连接池(r2d2),WAL + busy_timeout(desktop 场景)。完整契约见 [Spec01](./Spec01_数据层.md)。

## 5. 契约与不变量(施工红线,全景层面)

本篇只列**跨切面**、影响整机理解的红线;完整分域编目(错误处理/DB/文件操作/第三方集成等每条"规则+为什么+违反后果+出处")是 [Spec14](./Spec14_不变量与约定.md) 的权威内容,本篇不重复。

- **IPC 边界**:前端只能通过 `src-tauri/src/ipc/registry.rs::handler()` 注册的命令访问后端能力;新增命令改 `ipc/*_commands.rs` + `registry.rs`,不改 `lib.rs`(`../../AGENTS.md`;命令表/错误码权威见 [Spec10](./Spec10_IPC与错误契约.md))。
- **推理隔离不可退让**:host 二进制不得引入 `ort`/`tokenizers`/`ndarray` 等推理依赖,一切 AI 推理经 `ai-worker` 子进程(§4);违反会使前述"崩溃/内存隔离"架构目标落空(详见 [Spec06](./Spec06_AI人脸OCR.md))。
- **开源边界**:第一方公开源码统一 AGPL-3.0-only——授权实现(含验签与 keyring 直销)随源码公开;另设单独商业授权并行,不购买商业授权也可按 AGPL 行使权利;**源码开放不等于功能免费**,商业边界是签发私钥/密凭证与付费插件载荷(Part0 §10)。NAS Server/Web 尚属规划,不声称已有服务端实现或源码下载设施。公开镜像只过滤内部文件,代码与 `Cargo.lock` 原样同步。
- **workspace members 显式列举,禁 glob**:`Cargo.toml:7-9` 注释指出 `crates/*` glob 会命中无 `Cargo.toml` 的目录导致 `cargo metadata` 报错,且只展开一层、漏掉两层深的 `psd-worker`/`psd-probe`——members 必须逐一显式列出。
- **CSP 平台分档**(§1.3 已述):Windows/Android 用 `http://*.localhost` 系 origin;macOS/iOS/Linux 起线时必须补 `tauri:` scheme 并做设备验证,当前 `src-tauri/tauri.conf.json:44` 尚未包含,是已知平台缺口而非疏漏(详见 [Spec13](./Spec13_构建发布商业化.md))。

## 6. 边界情况与失败模式(全景层面)

本篇不展开单个子系统的失败模式(见各子篇 §5);全景层面需要读者预先知道的边界:

- **多平台代码不对等**:任何"重建/移植到 macOS/iOS/Android"的任务,不能假设 Windows 分支的实现天然有跨平台等价物——`video`/部分 `storage`(WebDAV perf 特性)、CSP 配置均按平台分叉,施工前先确认目标平台是否已有实现(§1.3)。
- **worker 子进程不可达时**:`ai-worker`/exotic worker 若下载失败或验签失败,对应功能(语义搜索/人脸/OCR/冷门格式)按各自子篇的降级路径处理,host 主体功能(浏览/扫描/缩略图)不受影响——这是子进程隔离架构的直接收益,细节见 [Spec06](./Spec06_AI人脸OCR.md)/[Spec09](./Spec09_插件平台与exotic.md)。
- **公开镜像 vs 本仓**:公开镜像只经内部文件过滤,源码与依赖图与本仓**同构**;`exotic::default_entitlement_provider` 的授权装配在两侧一致(唯一渠道 = 直销 → `KeyringLicenseStore`,信任根解析失败降级 `FreeStubEntitlement` fail-closed)。

## 7. 重建指引(从零实现,总纲)

依赖顺序(每层的具体重建步骤见对应子篇 §6):

1. **DB 层**([Spec01](./Spec01_数据层.md)):schema、迁移、连接池——一切上层模块的地基。
2. **扫描与画廊**([Spec02](./Spec02_扫描与画廊.md)):扫描根管理 + 布局引擎,产出可浏览的媒体列表。
3. **缩略图/派生**([Spec03](./Spec03_缩略图与派生.md)):画廊需要缩略图才可用。
4. **图像/视频/音频/文档**([Spec04](./Spec04_图像与色彩管线.md)/[05](./Spec05_视频与音频.md)/[07](./Spec07_文档与阅读器.md)):各媒体类型的查看/编辑管线,可并行建。
5. **AI/人脸/OCR**([Spec06](./Spec06_AI人脸OCR.md)):依赖 ai-worker 子进程拓扑,可后建(非画廊浏览刚需)。
6. **存储/备份/导出**([Spec08](./Spec08_存储备份导出文件操作.md)):数据安全网,建议在核心功能稳定后建。
7. **插件平台/exotic**([Spec09](./Spec09_插件平台与exotic.md)):商业模型载体,依赖前述格式注册机制(`formats` 模块)。
8. **IPC/错误契约**([Spec10](./Spec10_IPC与错误契约.md))与**前端架构**([Spec11](./Spec11_前端架构.md)):贯穿以上各层,建议与后端模块同步建。
9. **配置/状态/日志**([Spec12](./Spec12_配置状态日志.md)):运行期基础设施,尽早建以便调试其余各层。
10. **构建/发布/商业化**([Spec13](./Spec13_构建发布商业化.md)):最后收口——签名、CI 门控、发布与商业化。

**顶层依赖/版本(代表性,非全量,全表见 [Spec01](./Spec01_数据层.md)/[Spec11](./Spec11_前端架构.md))**:

| 层 | 关键依赖 | 版本 | 用途 |
|---|---|---|---|
| 后端 | `tauri` | 2(`src-tauri/Cargo.toml`,`features = ["protocol-asset","unstable","tray-icon"]`) | 窗口/IPC/插件系统 |
| 后端 | `rusqlite` | 0.31(`bundled,collation,functions`) | DB,无 ORM |
| 后端 | `tokio` | 1(`full`) | 异步运行时 |
| 后端 | `moxcms` | 0.8.1 | ICC CMS 色彩管理 |
| 后端 | `thiserror` / `anyhow` | 1 / 1 | domain 错误 / 内部编排(分工见 [Spec14](./Spec14_不变量与约定.md) §4.1) |
| 前端 | `vue` | 3.5.13(`package.json`) | 框架,Composition API |
| 前端 | `pinia` | 3.0.4 | 状态管理 |
| 前端 | `vue-router` | 4.6.4 | 路由 |
| 前端 | `@tauri-apps/api` | 2 | IPC/窗口/系统 API |
| 前端 | `@tanstack/vue-virtual` | 3.13.33 | 长列表虚拟滚动(画廊/日志窗口) |
| 前端 | `cropperjs` | 2.1.1 | 图像裁剪(编辑器) |
| 前端 | `pdfjs-dist` | 4.10.38 | PDF 渲染 |

- **版本号单一事实源**:`src-tauri/Cargo.toml:3` 注释标注 `version.workspace = true`——crate 版本锚在根 `Cargo.toml:45` 的 `[workspace.package] version = "0.1.0"`,升版只改根一处(Part7-T5 决策,理由见 `../refactor_2026/Part7_发布工程.md §3.2`)。前端 `package.json:4` 当前独立标注 `"version": "0.1.0"`,与 workspace 版本数值一致但非同一机制锚定——若后续升版,需人工同步两处或另建校验(待核实:是否已有 CI 步骤校验两者一致,未在本次核实范围内确认)。
- **坑与教训**:详见 `../experience.md`(24 条踩坑教训索引),全景层面最相关的是 §12(工具自扫描避让)与门禁类经验,具体子系统的坑见各篇 §6。
- **验收总入口**:`.github/workflows/ci.yml` 是门禁源(`../../AGENTS.md` Delivery 节);Rust 侧 `cargo test`(workspace 根)、前端侧 `npm run lint` + `vue-tsc` + vitest,细分见各子篇 §6 与 [Spec13](./Spec13_构建发布商业化.md)。

## 8. 17 篇导航

| 篇 | 一句话覆盖 |
|---|---|
| [README.md](./README.md) | 17 篇总索引 |
| **Spec00(本篇)** | 产品定位、整机架构、进程/线程模型、技术栈总表、构建变体总览、17 篇导航 |
| [Spec01_数据层](./Spec01_数据层.md) | SQLite schema、迁移、连接池、自定义排序规则、查询分类 |
| [Spec02_扫描与画廊](./Spec02_扫描与画廊.md) | 文件系统扫描引擎、增量更新、布局计算、虚拟滚动数据源 |
| [Spec03_缩略图与派生](./Spec03_缩略图与派生.md) | 缩略图生成/缓存、视频关键帧派生、文档/音频派生产物 |
| [Spec04_图像与色彩管线](./Spec04_图像与色彩管线.md) | 图片编辑(裁剪/旋转/调色)、ICC CMS、查看器色域管理 |
| [Spec05_视频与音频](./Spec05_视频与音频.md) | Media Foundation 取帧/硬解、播放器进度条、音频标签/歌词 |
| [Spec06_AI人脸OCR](./Spec06_AI人脸OCR.md) | 语义搜索、人脸检测/聚类/审批、OCR,ai-worker 子进程隔离 |
| [Spec07_文档与阅读器](./Spec07_文档与阅读器.md) | PDF/EPUB/文本渲染、分章、简繁转换、远程 AI 校对 |
| [Spec08_存储备份导出文件操作](./Spec08_存储备份导出文件操作.md) | 卷管理、WebDAV、备份/恢复、导出、文件移动/复制/删除 |
| [Spec09_插件平台与exotic](./Spec09_插件平台与exotic.md) | 冷门格式插件 Catalog、授权、Registry、安装,商业模型载体 |
| [Spec10_IPC与错误契约](./Spec10_IPC与错误契约.md) | 229 条 Tauri 命令全表、`AppError` 错误码、权限声明 |
| [Spec11_前端架构](./Spec11_前端架构.md) | Vue 3 组件/composables/store/路由全景、IPC 调用层 |
| [Spec12_配置状态日志](./Spec12_配置状态日志.md) | `config.toml`、`AppState`/`RunTokenSlot`、结构化日志/诊断包 |
| [Spec13_构建发布商业化](./Spec13_构建发布商业化.md) | lite/perf 特性、直销渠道构建、CSP 分档、CI 门控、签名/更新 |
| [Spec14_不变量与约定](./Spec14_不变量与约定.md) | 全集不变量/红线单一权威索引,分域编目 |
| [Spec15_流水线全景](./Spec15_流水线全景.md) | 15 条数据处理流水线横切总览(触发/阶段链/引擎/并发/现状),跨流水线基础件 + CI 工程流水线 |

### 两条阅读路径

- **人类工程师路径**(建心智模型 → 定位要改的子系统):Spec00(本篇)→ 按需求定位到具体子篇的「1. 概览」+「4. 契约与不变量」→ 需要理由时才跳 `../refactor_2026/PartN`。
- **低能力 LLM 施工路径**(拿到具体任务、上下文有限):直接进目标子篇,该篇「2-3 节」给出自包含的数据模型/流程,「4. 契约与不变量」给红线,**仅当需要跨切面契约(错误码全表/不变量总览/IPC 全表)时**才跳 Spec10/Spec14 摘一段,不需要通读 Spec00 之外的其他篇。

## 9. 关联

- 上游正典:`../refactor_2026/Part0_总纲与产品定稿.md`(产品定稿、商业模型、开源策略、命名决策的完整论证)、`../refactor_2026/Part7_发布工程.md`(workspace 拓扑、构建变体、CI 门控的完整论证)。
- 相关决策:`../decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md`(命名定案)。
- 全局不变量入口:[Spec14_不变量与约定](./Spec14_不变量与约定.md)。
- 相关规格篇:见 §8 导航表全部 17 篇(README + Spec00-15)。
