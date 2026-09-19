---
id: 2026-07-24-Spec14_不变量与约定
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec14 · 不变量与约定

> 一句话:本篇是**全集不变量/红线的单一权威索引**——服务两类读者(新加入的人类工程师、低能力 LLM 编码代理),每条不变量给「规则陈述 + 为什么 + 违反后果 + 出处 + 详解归属篇」,不复制深机制。动笔或改动跨切面契约前先查本篇目录,再跳归属篇看细节。

## 1. 概览

Scrollery 是面向 Windows / macOS / iOS / Android 的高性能素材管理器(`../../AGENTS.md`)。本篇不描述任何单一子系统,而是把散落在仓根 `AGENTS.md`(Hard Constraints + Engineering Defaults)、`docs/experience.md`(24 条踩坑教训)与代码编译门控(`cfg(windows)`/`cfg(feature=...)`)里的**跨切面红线**汇总成一张可查表。

- 来源三处:
  1. `../../AGENTS.md` — 项目级硬约束(Rust 错误分工、DB、并发、路径安全、CSP 等)。
  2. `../experience.md` §1-24 — 可执行的踩坑教训,每条一句规则。
  3. 代码 `cfg(windows)` / `cfg(feature="...")` 门控点 — 编译期而非运行时决定的行为分支。
- 每条不变量后缀「(→ SpecNN)」表示该子系统篇有更完整的机制展开;本篇只给**规则本身 + 出处**,不重复其推导过程。
- 在整机中的位置:本篇不接入任何调用链,是文档集内部的横向索引——任何子系统篇的「4. 契约与不变量」小节都可以反过来指回本篇的对应条目,避免同一条规则在多篇里各写一份、悄悄漂移。

## 2. 数据模型与状态

本篇不定义新的 struct/表;涉及的核心类型在归属篇有权威定义,这里只列不变量条目本身依赖的类型锚点:

| 类型 | 位置 | 归属篇 |
|---|---|---|
| `AppError`(错误统一枚举) | `src-tauri/src/error.rs:11-`、`Serialize` 实现见 `src-tauri/src/error.rs:346` | [Spec10_IPC与错误契约](./Spec10_IPC与错误契约.md) |
| `RunTokenSlot`(取消令牌槽) | `src-tauri/src/state.rs:317-389` | [Spec12_配置状态日志](./Spec12_配置状态日志.md) |
| `DbWriter` / `DbPool` | `src-tauri/src/db/connection.rs:20-21` | [Spec01 数据层](./Spec01_数据层.md) |

## 3. 关键流程与算法

本篇无独立算法;不变量条目里涉及的流程(如 RunTokenSlot 的 `begin`/`finish` 状态机、WAL 启动截断)在下表「§4 分域编目」内以「规则陈述」形式给出,机制细节交叉引对应篇。

## 4. 契约与不变量(施工红线)——分域编目

### 4.1 Rust 错误处理分工

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| `thiserror` 用于结构化 domain/library 错误,枚举定义在 `src-tauri/src/error.rs:11`(`AppError`,`#[derive(Debug, Error)]`) | 每个变体需要稳定 `code` 字段回传前端,`thiserror` 的 `#[error(...)]` 宏配合手写 `Serialize`(`src-tauri/src/error.rs:346`)保证格式统一 | 用 `anyhow` 包一切会丢失变体信息,IPC 层无法给前端稳定错误码 | `../../AGENTS.md`(Hard Constraints §1) |
| `anyhow` 仅用于内部编排(调用方只记日志、不需要结构化匹配),依赖声明 `src-tauri/Cargo.toml:170` | 编排层错误链路深、类型繁杂,`anyhow::Error` 免去为每层写专属枚举 | 在需要暴露给 IPC 的路径上误用 `anyhow` 会让内部字符串直接泄漏到前端(违反 §4.2) | 同上 |

### 4.2 数据库隔离

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| 仅用 `rusqlite`(`src-tauri/Cargo.toml:43`,`features = ["bundled", "collation", "functions"]`),所有 SQL 参数必须绑定(不做字符串拼接) | 绑定参数是防注入唯一防线;`bundled` 保证跨平台内嵌 SQLite 版本一致 | 拼接 SQL 是注入面;版本漂移导致跨平台行为不一致 | `../../AGENTS.md` §2 |
| async 命令里每次 `rusqlite` 调用必须走 `spawn_blocking` 或专用 DB 线程,代表点 `src-tauri/src/db/queries/thumbnail.rs:125` | `rusqlite::Connection` 非 async;直接调用会阻塞 Tokio 执行器线程,拖垮所有并发任务 | UI 卡顿甚至假死(阻塞的是共享的 async 运行时) | `../../AGENTS.md` §2;详解见 [Spec01 §3](./Spec01_数据层.md) |
| 写连接单 Mutex 序列化:`pub db_writer: DbWriter`(`src-tauri/src/state.rs` 字段定义处,类型 `Mutex<Connection>` 见 `src-tauri/src/db/connection.rs:21`) | SQLite 单文件写不支持多写者并发;单 Mutex 是最简单的正确序列化点 | 并发写导致 `SQLITE_BUSY` 或数据竞争 | `src-tauri/src/db/connection.rs:20-21` |
| 读连接走 `r2d2::Pool<SqliteConnectionManager>`(`DbPool`,`src-tauri/src/db/connection.rs:21`),WAL 模式下并发读 | 读远多于写,池化并发读避免读请求排队 | 无池化则每次读都建新连接,延迟劣化 | 同上 |
| 每条连接(写 + 每个读池连接)统一应用 `PRAGMA journal_mode=WAL; busy_timeout=5000` 等(`src-tauri/src/db/connection.rs:25-33` 常量 `PRAGMAS`,经 `apply_pragmas` 应用) | WAL 允许读写并发;`busy_timeout` 让偶发写冲突自动重试而非立即报 `SQLITE_BUSY` | 无 WAL 则读写互斥,吞吐骤降;无 `busy_timeout` 则并发写冲突直接报错而非重试 | `src-tauri/src/db/connection.rs:25-33` |
| 启动期 WAL 截断(`checkpoint_wal_at_boot`,`src-tauri/src/db/connection.rs:63-107`)必须在 tracing 订阅器就绪后、读池归还后、管线拉起前调用 | 崩溃/强杀会让 WAL 跨会话累积;过早调用(日志未就绪)则截断过程静默,排查失效 | 调用时机错(过早)会丢失截断日志;调用太晚(管线已拉起)会与并发读者冲突,截断不完整 | `src-tauri/src/db/connection.rs:59-62` 注释;备份专项细节见 [Spec08](./Spec08_存储备份导出文件操作.md) |

### 4.3 IPC 契约(就近摘要;全量见归属篇)

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| 错误必须实现 `serde::Serialize`(`AppError` 手写实现,`src-tauri/src/error.rs:346`) | Tauri IPC 需要把 Rust 错误序列化传回前端 JS | 不可序列化的错误类型会在编译期或运行时直接失败 | `../../AGENTS.md` §4 |
| 暴露稳定 `code`/变体,不泄漏内部字符串;各错误变体自带 `code: &'static str`(如 `Exotic { code, message }`,`src-tauri/src/error.rs:91,99,106,116,124,134,146,159,167,175,184,195`) | 前端要按 `code` 做分流展示(多语言/重试逻辑),内部 debug 字符串格式易变、不该成为前端契约 | 前端硬编码解析 `message` 字符串,内部改措辞就会静默破坏前端逻辑 | `src-tauri/src/error.rs` 全量清单权威归属 [Spec10_IPC与错误契约](./Spec10_IPC与错误契约.md) |
| Tauri v2 原生插件与暴露命令必须在 `src-tauri/capabilities/` 声明权限(现有 `default.json`、`logs.json`) | Tauri v2 权限模型要求显式声明,未声明的命令/插件能力在运行时被拒绝调用 | 权限未声明会导致功能在生产构建下静默不可用(dev 环境某些场景不受限,易漏测) | `src-tauri/capabilities/default.json:3,6` |

### 4.4 文件操作(就近摘要)

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| 用户提供路径必须先 `canonicalize` 再边界检查,代表实现 `resolve_within_root`(`src-tauri/src/utils/path.rs:156-`,解析 symlink/`.`/大小写后判断是否仍在 root 内) | 不 canonicalize 则符号链接/`..`可逃逸出用户授权的根目录 | 路径穿越漏洞(读写根目录外任意文件) | `src-tauri/src/utils/path.rs:147-156` 注释;单测覆盖 `src-tauri/src/utils/path.rs:307-` 起 |
| 派生文件先写 `*.tmp` 再同卷 rename,代表位置 `src-tauri/src/thumbnail/cache.rs`、`src-tauri/src/derive/**`(以模块级约定存在,逐点不在此穷举) | rename 在同一文件系统内是原子操作;直接写目标文件在崩溃时会留半写文件 | 崩溃时缩略图/派生文件损坏或残缺,读取时解码失败 | `../../AGENTS.md` §8;详解见 [Spec08](./Spec08_存储备份导出文件操作.md) |

### 4.5 并发

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| 禁止 `std::sync::Mutex` guard 跨越 `.await` | `std::sync::Mutex` 不感知 async,持锁跨 await 点会阻塞整个执行器线程,且可能死锁 | 执行器线程阻塞,拖累其他并发任务;某些运行时下直接死锁 | `../../AGENTS.md` §7 |
| `tokio::sync::Mutex` 仅在必须跨 await 持锁时使用,代表点 `exotic_install_lock`(`src-tauri/src/state.rs:128`,类型 `tokio::sync::Mutex<()>`) | async 感知锁允许跨 await 持有而不阻塞线程,但有额外开销,故仅在确实需要时使用 | 滥用会引入不必要的调度开销;该用而不用则违反上一条 | `src-tauri/src/state.rs:128` |
| `RunTokenSlot` 槽位语义:`finish(generation)` 是 compare-and-clear——槽内是本轮→清空返回 `true`;**槽已空→返回 `true`**(用户显式 stop 已 take,本轮仍保留 cancelled 终态的发布权);槽被更新一轮占用→不清槽返回 `false`(旧轮收尾不得再动全局状态) | 无代次区分时,旧轮迟到的收尾会 take+cancel 新一轮刚安装的 token(「停止→立即重启」竞态);该数据结构把此纪律沉淀为可复用件 | 若把「槽已空」误判为「本轮已失败」会丢失 cancelled 终态的正确发布;若旧轮收尾未经代次校验直接清槽,会打断新一轮的正在运行状态 | `src-tauri/src/state.rs:317-389`(实现)、`src-tauri/src/state.rs:352-354` 注释(compare-and-clear 三分支) |
| `RunTokenSlot` 全仓四(现为六)槽实例各自独立计数,`finish` 只与本槽存的代次比较,从无跨槽比较 | 共享计数器无必要,各任务类型(缩略图/AI/人脸/派生/备份/导出)互不干扰 | 若错误共享代次计数器,一个子系统的取消会误判另一子系统的完成状态 | `src-tauri/src/state.rs:92,148,153,177,242,256`(六个字段:`thumb_gen_token`/`ai_analysis_token`/`face_analysis_token`/`derivation_token`/`backup_token`/`export_token`) |
| IO/CPU 重活离 UI 线程 | Tauri WebView 与后端共享的 async 运行时若被重计算阻塞,前端所有 IPC 调用都会卡顿 | 界面无响应,用户体验为「假死」 | `../../AGENTS.md` §7 |

### 4.6 路径与安全(就近摘要,详见 §4.4)

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| 本地资源经 scoped `assetProtocol` + `convertFileSrc`,scope 限定 `$APPDATA/**`(`src-tauri/tauri.conf.json:45,47`) | WebView 直接访问任意本地文件是安全风险;scoped 协议把可读范围收紧到应用数据目录 | 无 scope 限制则理论上可通过协议读取系统任意文件 | `src-tauri/tauri.conf.json:44-48` |

### 4.7 CSP 平台分档 —— **规范定分档,现状未落(as-built gap)**

| 项 | 内容 |
|---|---|
| **规范要求**(`../../AGENTS.md` Hard Constraints 最后一条) | Windows/Android 构建用 `http://*.localhost` 来源;macOS/iOS/Linux 构建须包含 `tauri:` scheme(待该平台线开工时补加并真机验证);开发态可放行 `ws://` |
| **代码现状**(`src-tauri/tauri.conf.json:44`,单一 `app.security.csp` 字段,无平台分档文件) | `default-src 'self' ipc: http://ipc.localhost; connect-src 'self' ipc: http://ipc.localhost asset: http://asset.localhost blob: data:; img-src 'self' asset: http://asset.localhost blob: data:; media-src 'self' asset: http://asset.localhost blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; frame-src 'self' blob: data:` |
| **gap 判定** | 该配置全平台统一生效,**不含 `tauri:` scheme**,也没有按 `cfg(target_os)` 或多份 `tauri.conf.<platform>.json` 分档(仓内只有唯一 `src-tauri/tauri.conf.json`,已 `ls` 确认无 `tauri.conf.macos.json` 等变体)。当前仅 Windows 目标线在实跑,故规范里 macOS/iOS/Linux 分档条款尚无落地对象验证。**这是「规范定分档、实现未落」的已知缺口,不是已实现的能力,勿在其他篇写成「已按平台分档」** |
| 开发态 CSP 派生 | 由 `scripts/vite-plugin-dev-csp.mjs` 的 `deriveDevCsp()` 纯函数(`scripts/vite-plugin-dev-csp.mjs:19-35`)从生产 CSP **派生**(单一事实源),仅追加 `ws:` 到 `connect-src`,`apply: 'serve'` 保证不进生产构建;原因见该文件顶部注释(`app.security.devCsp` 字段虽存在但无代码路径读取,Tauri 的 `get_asset()` 才是 CSP 注入点,而 `devUrl` 模式下请求根本不经过 `get_asset()`) | `scripts/vite-plugin-dev-csp.mjs:1-19` |

### 4.8 AI 推理隔离

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| host 进程默认特性不带 `inference` 面(`ort`/`tokenizers`/`ndarray` 不进 host 单包依赖图),推理恒在 `ai-worker` 子进程内进行(彼处开 `inference` 特性) | 推理库体积大且有崩溃风险(ORT session 故障不该拖垮主进程);子进程隔离让崩溃可被 supervisor 捕获重启 | 若推理面进了 host,一次 ORT 崩溃会直接杀死整个应用进程 | `src-tauri/Cargo.toml:87-88`(注释)、`src-tauri/Cargo.toml:176`(工作区全量构建口径注释);详解见 `Spec06_AI人脸OCR.md`(该篇已存在,见 [Spec06](./Spec06_AI人脸OCR.md)) |

### 4.9 编译期门控

**渠道维度:已退役,唯一渠道 = 直销(direct)(P16,2026-09-15)**

`channel-msstore`/`channel-steam` 空渠道、`channel-direct` feature、`lib.rs` 渠道互斥 `compile_error!` 守卫与渠道桩均已删除。**已按源码核准**:`src-tauri/Cargo.toml` 的 `tauri-plugin-updater = "2"` 是**普通依赖**(不再 optional、不绑定任何 feature;`default = ["custom-protocol", "lite"]`)。桌面注册条件是运行期判据——编译进 context 的最终配置是否携带 `plugins.updater`(见 `src-tauri/src/lib.rs` 的 `tauri_context.config().plugins.0.contains_key("updater")` 分支)。基座 `tauri.conf.json` 保持无该块(dev/普通 build 因此不注册),只有直销发布 overlay `tauri.direct-release.conf.json` 携带,`scripts/verify-channel-bundle.mjs` 双向断言守卫。

**当前门控不变量**:第三方渠道若日后重新引入,须**重新设计**编译期物理排除(该渠道 build 的 `cargo tree` 无 `tauri-plugin-updater`、二进制无可达 keyring DRM/下载-执行路径)与提交前 CI 断言;当前仓内无此类渠道 feature/桩/字段可保留,也不得为「未来渠道」预铺空 feature。

**能力门控(并行维度,`src-tauri/Cargo.toml:250-279`)**

| 特性 | 定义位置 | 用途 | 为什么隔离 |
|---|---|---|---|
| `default` | `src-tauri/Cargo.toml`,`= ["custom-protocol", "lite"]` | 默认构建组合 | 基线口径 |
| `lite` | `src-tauri/Cargo.toml:264`,`= []` | 轻量变体(不带原生后端) | 与 `perf` 二选一维度 |
| `perf` | `src-tauri/Cargo.toml:265`,`= ["ffmpeg", "netfs"]` | 功能完整变体(捆绑原生后端) | 体积与授权(FFmpeg 等)只在需要时进入依赖图 |
| `netfs` | `src-tauri/Cargo.toml:271`,`= ["dep:reqwest_dav"]` | WebDAV 原生网络盘,仅 `perf` 编入 | 隔离网络盘依赖,`lite` 构建不携带 |
| `face-noncommercial` | `src-tauri/Cargo.toml:279`,`= ["scrollery-ai-core/face-noncommercial"]` | SCRFD/ArcFace 非商用研究权重,单独显式开启 | InsightFace 权重仅限非商业研究,编译期物理隔离(不能只靠运行时不可达),避免商业构建意外携带受限权重 |
| `exotic-dev-fixtures` | `src-tauri/Cargo.toml:259`,`= []` | 开发期测试授权注入 | 无真实 License 时仍可端到端测试冷门格式插件,生产构建绝不携带 |

违反后果(通用):任何一项特性若在错误的构建变体里意外开启,会造成许可证违规(`face-noncommercial`)、体积膨胀(`perf` 相关误入 `lite`)或测试后门泄漏到生产(`exotic-dev-fixtures`)。

**`cfg(windows)` 平台门控**(`git grep -n "cfg(windows)" -- src-tauri/src` 实测命中 24 处,横跨 12 个文件,代表点):

| 位置 | 用途 |
|---|---|
| `src-tauri/src/video/mod.rs` | Media Foundation 视频解码(Windows only) |
| `src-tauri/src/derive/pipeline.rs` | GPU DXVA 硬解(Windows only) |
| `src-tauri/src/engine/gpu/mod.rs` | DXGI/DirectML GPU provider |
| `src-tauri/src/backup/restore.rs` | Windows 特定恢复逻辑 |
| `src-tauri/src/scanner/volume_probe.rs` | Windows 卷探针 |
| `src-tauri/src/utils/path.rs` | Windows 路径规范化 |
| `src-tauri/src/thumbnail/qos.rs` | Windows 特定 QoS 逻辑 |
| `src-tauri/src/exotic/worker.rs`、`src-tauri/src/ipc/backup_commands.rs`、`src-tauri/src/storage/local.rs`、`src-tauri/src/tree/mod.rs`、`src-tauri/src/engine/mod.rs` | 其余 5 处,逐条含义未逐一核实,标「待核实」 |

为什么:项目当前仅 Windows 目标线在实跑(见 §4.7 CSP gap 同因),平台专属 API(Media Foundation、DXVA、DXGI)物理上只在 Windows 上存在,`cfg` 门控让跨平台编译时这些代码不参与构建而非运行时探测失败。

### 4.10 前端(就近摘要)

| 规则 | 为什么 | 违反后果 | 出处 |
|---|---|---|---|
| Vue 3 Composition API,TypeScript strict 模式,禁 `any` | 强类型在大型前端代码库里是重构安全网;Composition API 是当前团队约定风格 | 弱类型代码在跨模块重构时静默引入类型错误 | `../../AGENTS.md` §3;`tsconfig.json` `strict: true` |
| 极大数组用 `shallowRef`,代表用例 `src/components/layout/AppStatusBar.vue:175`、`src/components/media/MediaGridCanvas.vue:1502`、`src/composables/useBucketVirtualScroll.ts:39` | Vue 的深度响应式代理对超大数组/对象逐层拦截,开销随元素数线性增长;`shallowRef` 只在顶层触发响应,内部变更需显式 `triggerRef` | 大数组用普通 `ref` 会在每次读写触发昂贵的深度代理遍历,画廊等长列表场景明显卡顿 | `../../AGENTS.md` §3;详解见 [Spec11 前端架构](./Spec11_前端架构.md) |
| 长列表按需虚拟化,代表位置 `src/components/gallery/**/*.vue` | 素材库可能有数万条目,全量 DOM 渲染会导致内存与渲染耗时不可控 | 大目录打开时界面冻结甚至崩溃 | `../../AGENTS.md` §3 |

## 5. 边界情况与失败模式

本篇属于横向索引,不单独处理某个子系统的边界情况;§4.7 CSP 分档缺口即是一种「规范假设的多平台边界情况,当前实现仅覆盖单一平台」的记录方式范例——后续开工 macOS/iOS/Linux 目标线时,应先回到本条核对 `tauri.conf.json` 是否已按平台拆分,而不是假设分档已经存在。

## 6. 重建指引(从零实现)

- **依赖顺序**:先落 `../../AGENTS.md` 的硬约束(错误分工、DB 隔离规则),再落编译期门控(渠道互斥、`cfg(windows)`),这两层决定了后续所有子系统代码能否编译通过;最后核对 CSP/权限等运行时安全面。
- **外部 crate**(与本篇不变量直接相关的):`thiserror 1`(`src-tauri/Cargo.toml:52`)、`anyhow 1`(`src-tauri/Cargo.toml:170`)、`rusqlite 0.31`(`bundled, collation, functions`,`src-tauri/Cargo.toml:43`)、`r2d2`/`r2d2_sqlite`(读池,`src-tauri/src/db/connection.rs:9-10`)、`ring 0.17`(`src-tauri/Cargo.toml:191`,Ed25519 验签,详见下条)、`moxcms 0.8.1`(`src-tauri/Cargo.toml:100`,色彩管理,详见 [Spec04](./Spec04_图像与色彩管线.md))。
- **坑与教训**(链 `../experience.md`,每条一句可执行规则):
  - §1 CPU 模式单核退化 → 恢复内部多线程 + 缩小 SessionPool 上限(`../experience.md` 第 18-36 行)
  - §2 GPU 大批次显存交换 → 防爆硬限制 256 + 智能 Auto 模式(`../experience.md` 第 37-50 行)
  - §3 图片原尺寸与显存占用误区 → AI 前向传播只需固定尺寸,不关心源图分辨率(`../experience.md` 第 51-59 行)
  - §4 画廊虚拟滚动性能 → 快滚甩滚低保真闸门基于速度非布尔;过桥前先问「真用了多少」(`../experience.md` 第 64-91 行)
  - §5 缩略图派生一致性 → 删除派生文件必须同事务复位状态;自愈判据「文件∧状态」双查(`../experience.md` 第 93-102 行)
  - §6 ORT 路径劫持 → 环境问题须在发生 CWD 复现,绝对路径不进仓库配置(`../experience.md` 第 128-149 行)
  - §7 `cargo --bin` 过滤 → clean 后逐一核对产物存在性,勿信单条命令退出码(`../experience.md` 第 152-158 行)
  - §8 npm 格式化炸 build → 格式化修复一律 `npm run lint:fix`,勿独立跑 prettier(`../experience.md` 第 160-164 行)
  - §9 worker 迁移并发 → 先盘点原路径并发结构,逐条对账到新结构(`../experience.md` 第 192-194 行)
  - §10 e2e 对拍盲区 → 自洽全绿不等于覆盖率大于零,对拍数据须 join 生产键验证(`../experience.md` 第 196-198 行)
  - §11 布局缓存四契约 → 写路径提交 bump / SQL ORDER 与 `derive_order` 同步 / items 快照源载荷 / HIT 守卫同步(`../experience.md` 第 200-202 行)
  - §12 门禁自扫描陷阱 → 工具代码勿把 docs 前缀与 `.md` 后缀连成同一字面量;fixture 用变量拼接(`../experience.md` 第 259-261 行)
  - §13 NVMe 掉线假象 → 时有时无+size 0+反常时间戳,先查系统日志,重启是第一动作(`../experience.md` 第 265-292 行)
  - §14 顶栏折叠闪动 → 源头解耦(sizer)+判据补锚互补,症状类别建不变量而非逐触发器打补丁(`../experience.md` 第 104-121 行)
  - §15 编译期默认行为 → 评审「缺某机制」提案前先验证该机制是否已被编译期/框架默认值覆盖(`../experience.md` 第 204-206 行)
  - §16 改名/搬迁项目 → 判断风险靠内部自证点(git/Cargo/npm/Tauri),勿搜文件夹名字符串(`../experience.md` 第 166-172 行)
  - §17 Claude Code 项目数据键控 → 改名前须手动迁移整个旧 project 目录到新编码名(`../experience.md` 第 174-182 行)
  - §18 正则扫描剥注释 → 扫描器必须先剥 `<!--`/`/* */`/`//`,宁可漏判不可误判(`../experience.md` 第 208-226 行)
  - §19 集合级契约陷阱 → 「消费≡定义」只证合法性不证正确性;跨主题像素矩阵须人眼对比,勿设基线门(`../experience.md` 第 228-247 行)
  - §20 PowerShell 语言模式 → `.ps1` 脚本 + `iex` 受 ConstrainedLanguage 限制,内联命令链不受限(`../experience.md` 第 293-295 行)
  - §21 headless Edge 截图 → 锚点/`scrollTo` 不可靠,DOM 测量兜底,截图异常先探针再改码(`../experience.md` 第 297-303 行)
  - §22 Tauri asset ACAO → assetProtocol 对一切响应(含错误路径)都发 ACAO,`<video crossorigin="anonymous">` 须与 `:src` 同渲(`../experience.md` 第 186-188 行)
  - §23 License 尽调 → dual/conditional license 库须查实际编译配置(如 `--enable-lgpl`),勿信包装层许可证字段(`../experience.md` 第 249-251 行)
  - §24 状态持久化选层 → 随记录走(旋转角、播放位置)进 DB;跟应用走(音量、倍速)进 localStorage(`../experience.md` 第 253-255 行)
- **验收**:路径安全的边界测试见 `src-tauri/src/utils/path.rs:307-` 起(`resolve_within_root` 单测,含 symlink 逃逸/绝对路径/`..` 穿越等用例);相关 gate 命令按变更面选用(见 `../../.github/workflows/ci.yml`),本篇属跨切面索引,不单独定义 gate。

## 7. 关联

- 上游正典:`../../AGENTS.md`(Hard Constraints + Engineering Defaults,本篇条目的规范来源)。
- 相关教训:`../experience.md` §1-24(全量教训索引,已逐条摘录见 §6)。
- 相关规格篇:
  - [Spec01 数据层](./Spec01_数据层.md) — DB schema、连接管理机制详解。
  - [Spec04 图像与色彩管线](./Spec04_图像与色彩管线.md) — moxcms 相对色度、ICC 色域转换机制详解。
  - [Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md) — ai-worker 子进程隔离、`inference` 特性面详解。
  - [Spec08 存储备份导出文件操作](./Spec08_存储备份导出文件操作.md) — `*.tmp`→同卷 rename、WAL 截断在备份场景的应用详解。
  - [Spec09 插件平台与exotic](./Spec09_插件平台与exotic.md) — Ed25519 验签信任根(`crates/scrollery-exotic-trust/src/crypto.rs`)机制详解。
  - [Spec11 前端架构](./Spec11_前端架构.md) — `shallowRef`/虚拟化列表的完整应用面详解。
  - [Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md) — IPC 命令全表、错误码全量清单权威处。
  - [Spec12 配置状态日志](./Spec12_配置状态日志.md) — `RunTokenSlot` 在配置/状态管理场景的完整应用详解。
  - [Spec13 构建发布商业化](./Spec13_构建发布商业化.md) — 渠道互斥、`face-noncommercial` 等特性门控的完整商业化背景详解。
