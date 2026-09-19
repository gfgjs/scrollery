---
id: 2026-09-15-全仓代码消融第一轮
status: snapshot
type: review
line: 全仓代码消融
created: 2026-09-15
last-verified: 2026-09-15
---

# 全仓代码消融：第一轮分析与实施方案

> 后续实施已完成（2026-09-15）：[实际删减、验证、提交与最终指标](实施报告.md)。下文保留第一轮分析口径，不作为最终实施结果。

> 源码基线：`e7db489b66f3d939784678ccf4d6849b7eec5304`。本轮只分析并交付方案，没有修改产品代码、删除测试或操作用户数据库。以下“建议删除”均不是“已经删除”。

> 2026-09-15用户实施裁决：P12 H-Lab全栈保留；P23所有历史数据可舍弃，直接维护当前DDL，不做历史兼容。其它方案采纳；实际实施状态以本线status和实施三件套为准。

## 1. 代码量与审查口径

**当前 Git 跟踪的第一方源码：264,655 LOC，927 个文件；非空行 244,507。**

这里的 LOC 是 UTF-8 文本按 `splitlines()` 计算的物理行数，包含注释、空行和 Rust 内嵌测试；非空行也包含注释，不能称为净代码行。按整个 Git 跟踪集合统计，不把未跟踪文件、依赖目录、构建缓存、运行数据或本轮新文档算进基线。

| 第一方源码分组 | 文件 | LOC | 非空行 |
|---|---:|---:|---:|
| 前端，排除独立测试、开发夹具和 vendor | 382 | 85,395 | 79,393 |
| Rust 主程序，排除独立测试和 bin 探针 | 251 | 113,359 | 105,054 |
| Rust 共享库与 worker，排除独立测试和 probe | 52 | 16,173 | 15,011 |
| 独立测试文件 | 180 | 34,431 | 30,889 |
| 开发夹具、基准与探针 | 14 | 5,006 | 4,655 |
| 工程脚本与模型工具 | 40 | 9,597 | 8,853 |
| 根构建代码与页面启动代码 | 8 | 694 | 652 |
| **合计** | **927** | **264,655** | **244,507** |

独立测试按 `.spec.*`、`.test.*`、`/tests/`、`tests.rs`、`*_tests.rs` 识别。测试文件以外的 Rust `#[cfg(test)]` 块仍在原组，故前三行 **214,927 LOC 不是纯生产逻辑净行数**。验收时必须将生产实现、测试、注释的净变化分别解释，不能靠删测试、删注释、压行或搬到依赖中兑现目标。

额外存量：全仓跟踪文件 2,622 个；`docs/` 1,025 个文件、124,119 文本行；`public/vditor/`、`src/vendor/`、`third-party/` 共 575 个文件，其中 492 个可读 UTF-8 文本文件、60,464 行，约 50.1 MB。第三方压缩文件的 LOC 无业务复杂度意义，不计第一方收益。配置/资源清单单列 40 个文件、14,668 行，此组含 `package-lock.json`，不把它当成 14,668 行手写配置。

| 可复核的结构指标 | 基线 | 限定 |
|---|---:|---|
| 主程序顶层 `pub mod` | 32 | 固定以 `src-tauri/src/lib.rs` 的直接模块为准；不是臆定的业务“核心数” |
| Tauri 注册命令 | 261 | `ipc/registry.rs` 的 handler 清单 |
| 前端 IPC 常量 | 256 | `src/constants/ipc.ts` |
| 未发现前端生产 `IPC.KEY` 引用的已注册命令 | 29 | 仅线索，需排除直接调用、别名、native caller 与手动工具 |
| 用户设置键 / DB 状态键 | 79 / 19 | `config/schema.rs` 的两份清单 |
| Rust trait / struct / enum 声明 | 20 / 590 / 137 | 文本声明计数，包含测试及 cfg 分支，不是生产实例数 |
| TS interface / Vue 文件 | 359 / 115 | 排除 vendor，包含测试内声明 |
| Rust 测试属性 / ignore 属性 | 1,597 / 22 | 静态属性数，不是已运行、通过或覆盖率 |

复核附件：[分组基线](baseline.json)、[逐文件清单](baseline-files.tsv)、[IPC 候选清单](ipc-inventory.json)、[结构与设置清单](structure-inventory.json)、[环境变量符号与读取位置](environment-inventory.json)。环境清单只统计代码中的名称，不读取变量值；动态拼接和工作流注入不保证穷尽。

## 2. 当前真实依赖关系

### 入口与业务主路径

1. `index.html` → `src/main.ts` → Vue、Pinia、i18n、router 和内建命令注册。主窗口挂 `App.vue`，日志窗口按 label 懒加载；浏览器 harness 有明确开发入口，不能和真实桌面路径混算。
2. `src-tauri/src/main.rs` → `lib.rs::run`：创建 Tauri、初始化数据目录、DB 恢复与迁移、配置、asset scope、AppState 与后台任务，再注册 261 个命令。它是桌面应用组合入口；未发现可替代它的已落地 NAS/Web 服务入口。
3. 入库：扫描根 IPC → scanner 快扫/补充元数据 → rusqlite 数据表 → 布局取数/缓存 → 前端可视区窗口 → 缩略图生成与显示。快扫、补充元数据、派生图像具有不同完成条件，不因都是“扫描”就合成一个同步循环。
4. 浏览：共享 GalleryRouteLayer 承载全库/文件夹/收藏/人物/回收站及重复项镜头 → 视图描述符与后端查询 → 布局几何、窗口取数 → Canvas/DOM 展示；独立文档、音频和内容查看器各有路由。不同媒介能力不能仅为“一套路由”硬合并。
5. 编辑/导出/移动/恢复：UI 命令 → IPC 边界 → 文件/数据库操作 → 原子写入或阶段日志 → UI 刷新。恢复日志和布局版本检查直接关系用户文件及批量操作对象，属于保留边界。
6. AI/OCR/增强与冷门格式：主机编排 → worker 协议 → 子进程/推理核 → 派生结果写回。下载/安装另有签名、摘要、路径检查；这些不是普通内部函数间的重复校验。

### 模型、存储和配置事实来源

| 面 | 当前事实来源与消费者 | 消融判断边界 |
|---|---|---|
| 媒体/目录/扫描根及状态 | `db/models/*`、`db/queries/*`；扫描、布局、IPC 共用 | 缩减过宽 DTO 可以做；不能把 Rust/IPC 序列化和前端类型简单认定为三份无用复制 |
| 视图/选择描述符 | `db/models/view.rs`、`queries/layout/*`；布局、导出、批量操作 | 删除闲置公开命令时保留实际被批量操作使用的查询与 ViewStale 保护 |
| DB 格式 | `db/migration.rs` 当前 V33、`db/schema/{early,mid,late}.rs` | 新库仍跑历次升级；可合并建库结构，历史用户数据与恢复入口另立迁移边界 |
| 配置 | `config/schema.rs` 79 设置键 → `config.toml`；19 状态键在 DB | 不是两份完全相同配置；要剪除旧路由和无消费键，而非强行把运行状态写进用户配置 |
| 备份 | `backup/manifest.rs` 的 v1 包、DB/文档载荷与摘要 | 版本号“v1”是当前格式，不是应删的 legacy；导入长度/路径/完整性保护保留 |
| worker/插件 | `exotic-protocol`、catalog、registry/manifest、license/token | 协议当前版本、进程隔离和不可信文件边界保留；预留渠道/重复协议路径单独审查 |
| 构建 | 根 Cargo 版本锚；默认 `custom-protocol,lite,channel-direct`，另有 perf/netfs/ffmpeg/渠道及研究 feature | feature 实际消费者与发布产物比注释更权威；默认未编译不代表没有需求 |
| 手动工具 | Rust bin/examples/probe、scripts、tools、开发环境变量 | 无 UI 调用是正常属性，必须核实操作入口；开发签名工具不等于历史垃圾 |

### 审查覆盖与证据等级

全仓文件清单与源码统计已覆盖；分区审查覆盖前端、后端核心/持久化、媒体/AI、插件/worker，以及工程/发布工具。这里的“全仓分析”不等于逐行证明所有函数不可达，更不等于对全部平台完成动态覆盖。候选采用三档：**A：真实引用和替代路径已明确，可优先实施；B：方向成立，实施前需局部表征或实测；C：缺乏当前产品/数据证据，暂列待确认。**

预算与覆盖限制：核心分区子审查自报约68次调用/13分钟，超过约定35次/12分钟；回传后已停止该分区探索，不能把超预算探索当作全量证明。插件 fetch/fingerprint/sink/supervisor/package/registry/validate/worker_log 的细粒度分支、全部DB查询的逐函数消费闭包、所有 capability 权限与全部平台并未完成穷尽证明。这些只标记为已识别模块/已有真实入口，不宣称其内部没有剩余剪枝机会。工程分区资源计数与主基线有差异，已一律以 Git 跟踪逐文件清单重新计量。

关键词只用于定位，`fallback`、`manager`、`strategy`、`v1` 的出现次数不作为可删除量。特别是用户可选选区策略、GPU/CPU 不同能力和跨进程恢复，不能用“单实现/多路径”字面规则直接裁掉。

## 3. 候选与逐条消融判断

### 3.1 原架构的主要复杂度来源

- **旧入口下线，整条后端链仍存在。** 典型是独立去重页、旧行号取数和旧 AI 模型目录接口：仅隐藏 UI 不会减少注册命令、DTO、查询和测试。
- **新设计先造出并行抽象，却一直没有替换旧主路径。** `VectorStore`、ReaderLocator 和远程 StorageBackend 都具有这种特征，区别在于后者仍有用户可见的连接管理入口，不能全都按零调用处理。
- **性能方案保留两套完整实现和永久切换阀。** bucket/普通虚拟滚动、DOM/Canvas 的收益必须由现有功能和实测分别说明；真正重复的是装配/透传层，不能默认删除降级能力。
- **历史版本契约进入每次启动和每份 DTO。** V1–V33 迁移、DB→TOML、旧主题字段、旧阅读位置和未落地渠道，把历史与未来需求一起变成当前维护面。
- **开发实验和测试便利反向塑造生产 API。** H-Lab、只供测试观察的方法、只有基准消费的旧一体化缩略图包装应从当前运行面退出；仍在使用的 harness 与故障测试入口则有明确价值。
- **注释的设计意图落后于代码。** 不能用“未来会消费”“不得清理”作为保留理由；也不能用“尚未接线”的旧注释证明实际代码未接线。GPU 令牌就是必须反查全仓消费者的反例。

以下每项按消融七问给出：当前场景/是否存在、历史或未来属性、谁已保证条件、接口如何缩小、是否直报错误、最简单替代及验证。行数是基线文件体量或规划估算，**不是已完成的净删行数**；凡注明“文件体量”都包含注释/可能的测试，禁止直接累加为业务收益。

### 3.2 A 类：已有充分引用证据，优先删除

**P01｜删除未接线的 ReaderLocator。** 证据：`src/utils/readerLocator.ts:51` 起的创建/编解码/重锚只被自身 spec 引用；`src/types/reader.ts:73` 为其类型。真实阅读与书签走 `BookReader.vue:417,449,517` 的 `cfi:` 和旧 `scroll:` 字符串。删除损失的是从未接线的 `loc1` 未来能力，不会破坏现行 CFI 路径；现行阅读器已负责定位，不需要多级重锚 fallback。整删模块、孤立类型、专用测试及失效规范引用，不重写另一套定位器。文件体量 165 LOC，类型/测试另计。验证：typecheck、现行阅读位置和书签测试；首次实施人工打开 EPUB/TXT 并恢复位置。

**P02｜删除旧设置页 scroll-spy。** `src/composables/useSettingsScrollSpy.ts:17` 零生产消费者；`src/utils/settingsNav.ts:36` 的唯一生产消费者就是它。当前 `SettingsView.vue:802–841` 用路由分区、搜索选中与 `scrollIntoView`。删除不会改变当前导航，只移除过去整页滚动高亮的实现；直接使用现行分区状态即可，没有需保留的 fallback。两文件共 172 LOC，配套旧测试单列删除。验证：typecheck、设置分区/搜索/深链局部回归。

**P03｜删除授权 composable 的未使用有状态部分。** `src/composables/usePluginEntitlement.ts:42` 的工厂只被 spec 实例化；生产 `PluginGate.vue:70`、`ContentViewer.vue:530` 只使用 `gateModeFor`，插件商店自行读取 entitlement。删工厂及它的 loading/ref/派生字段，保留纯 gate 判定和现有授权入口；没有真实场景依赖此工厂，不需要新通用 gate manager。约 40–55 LOC 的实现面。验证：保留纯 gate 测试，删除工厂专属测试，typecheck。

**P04｜删除 latestWrite 的测试专用观察接口。** `src/utils/latestWrite.ts:47` 的 `isPending` 仅 spec 调用，生产 mediaStore 使用 `push`。保留“最新一次写入胜出”的队列本体，用写入次数/最终值/顺序断言行为，不再为测试暴露内部状态；约 3–6 LOC。错误传播和并发语义不改，运行保护不能随观察方法一起删。验证：已有 latestWrite 行为测试。

**P05｜整删休眠 VectorStore。** `src-tauri/src/ai/vector_store.rs:41` 的 trait、BruteForceStore、DynVectorStore 和 ANN_THRESHOLD 没有生产实例/调用；真实检索在 `ai/search.rs:21` 的 EmbeddingCache 与 search_control。文件内“未来 worker 化时接线”“不得清理”不是当前用户需求；worker 已存在也没有使用它。保留实际缓存/维度与模型身份检查，删休眠实现及专属测试、mod/re-export 和失效注释。318 LOC 文件体量，含约 90 行专用测试，实际逻辑净减需 diff 分列。验证：现行语义检索、模型切换/结果发布表征测试，局部 Rust 检查。

**P06｜删除旧布局查询入口。** `ipc/layout_commands.rs:963` 的 `get_layout_rows` 不再有前端调用；mediaStore 实际调用 `get_layout_rows_by_y`/`get_bucket_rows`。`get_separator_y_by_group_id` 的定位信息已在 `LayoutSummary.monthBuckets` 被时间轴消费。删除两命令、未消费的常量和仅由它们调用的 cache 方法，保留 by-y/bucket 的 hydration 与版本检查；不重写取数。约 35–65 LOC。验证：layout/cache/items_cache 相关测试、时间轴点击与两种布局。

**P07｜删除旧缩略图取消命令及孤立状态。** `ipc/thumbnail_commands.rs:645` 的 `cancel_thumbnail_request` 全仓没有消费者；它是 `cancelled_thumb_ids` 唯一生产写者，`:422` 只有对应读取检查，AppState 还保存并初始化集合。现行可视区调度没有使用这套单 id 取消 API。整删命令、集合、读取分支和常量文件中“dispatcher 消费”的失实注释；保留实际 request/generation/cancellation token 的失效处理。约 15–35 LOC。验证：缩略图请求换代/离屏/停止的既有局部测试，确认没有迟到结果污染当前视图。

**P08｜删除旧模型目录 API 和重复视频状态 API。** `ipc/ai_commands.rs:470,501` 的 list_ai_models/import_ai_model 无生产消费者；当前 aiStore 走 LIST_MODEL_REGISTRY、SET_ACTIVE_MODEL 和 DOWNLOAD_MODEL。`ipc/video_commands.rs:254` 的 video_component_status 无消费者，前端使用 resolve_video_playback 的 `needsComponent` 等 mode 和 VIDEO_CACHE_STATS。删三个命令及孤立 DTO/常量，保留模型下载完整性和视频 mode 判定；缺组件仍明确报状态，不做静默兜底。约 65–110 LOC。验证：模型枚举/切换、视频需要组件/直接播放分支的局部测试。

**P09｜内联固定交付源，删除空实现工厂。** `exotic/installer.rs:87` 的 PluginDeliverySource 及三种 Delivery 类型只返回枚举；真实调用在 `ipc/exotic_commands.rs:705`，恒为 DirectRegistryDelivery。将这个调用直接写成 InstallSource::DirectRegistry，删除 trait/三种零状态类型及验证返回常量的测试。非 direct 安装本来在副作用前拒绝；本批保留来源枚举和拒绝边界，渠道整体退役在 P16 处理。约 30–45 LOC。**它有生产调用，删除理由是无价值转发，不是零引用。** 验证：installer 正常安装与拒绝非 direct 的行为测试。

**P10｜删除恒为 None 的解密预留。** `crates/scrollery-plugin-api/src/lib.rs:94` 的 ActivationInfo.enc_seed 在 `exotic/license.rs:134`、`editing/entitlement.rs:95` 均为 None，无实际读取；自定义 Debug 和相关测试只保护不存在的 AES 功能。缩小 activate 返回值为当前真正需要的结果（若无数据则 `Result<(), LicenseError>`），删除该 DTO/字段/专用脱敏测试。未来解密需求不构成保留理由；现行 keyring/token 的敏感信息保护仍保留。约 15–35 LOC 及调用点简化。验证：两条真实 activate 成功/失败/不覆盖旧凭据的测试。

**P11｜删除配置类型和元数据的空维度。** `config/schema.rs:46` 的 Float 没有 SettingDef 使用者，`file.rs:135,217,354` 与 migrate 有专属分支；hot/restart_required 两列相反，而 watcher 实际读取 `hot`。删 Float 分支和冗余 restart_required 声明，必要返回值由 hot 推导；不增加新的 schema 框架。缺省/类型错误仍在文件与 IPC 边界拒绝。元数据重复约 80 行，合计规划 90–130 LOC。验证：schema/file/watcher 的默认值、非法值、热更新/需重启分类测试。Path 与 Str 暂不强行合并：先核实是否承担路径语义消费，不能只看渲染相同。

**P26｜删除只用于旧 AI 后端提示的路径。** `ai/runtime_config.rs:28` 的 warn_legacy_ai_backend 读取 DB 旧键，只在值不为 worker 时写一条退役日志；实际推理路径已经固定为 worker。这不是仍被支持的第二后端，也不影响数据库资产。删函数、调用点和只服务旧后端的配置种子/断言，不为了旧配置提示继续借连接读 DB；旧键可保持惰性无读，不必为抹去一行历史值升级 schema。约 15–25 LOC。验证：引用归零、worker 启动配置局部检查。此项不删除 ORT 路径解析或实际 provider 选择。

### 3.3 B 类：收益明确，按整条路径退役或局部表征后实施

**P12｜保留 H-Lab 全栈实验（用户明确裁决）。** HGalleryLabView、useHVirtualScroll、hgallery_commands、horizontal/hcache、路由与工具栏入口及AppState缓存全部保留。它是用户当前要求保留的开发实验，不纳入净删目标。五个主文件1,970 LOC及配套测试不因其它画廊剪枝连带退役。

**P13｜旧去重页面后端整链消融。** 29 条机械候选中有 11 条旧去重列表/目录树/清理接口；当前前端已改用主画廊 DuplicateLens。`ipc/dedup_commands.rs` 1,998 LOC、`db/queries/dedup.rs` 3,343 LOC 是审查面，**绝不能整文件删**：start/stop/status、摘要 generation 与当前 lens 查询仍有业务消费者。以旧 11 命令为根摘除 DTO/游标编码/查询/专属测试，遇到 scanner/dedup task/layout 的共享引用立即保留或收窄接口。删除会失去已退场独立页的专用 API 与旧清理入口；目前没有对应前端操作，不为未知外部调用方兼容。现行重复项分析/浏览/保护字段不改。本批收益需按被切出的真实依赖子图计量，不把 5,341 LOC 误报成可删量。验证：先固定当前 groups/folders 镜头、分析取消/换代和常规回收站行为，再删旧链。

**P14｜继续缩小闲置 IPC 面。** 清单包括 resolve_selection/count_selection/get_trash/search_media/pause_derivation/get_version_content/move_media_items/copy_media_items/clear_cache/retry_exotic_task/deactivate_editing_feature/set_active_face_model。无生产前端调用可优先删公开壳；底层函数需逐个核对 native/script/bench 和其它命令。例如 `queries::resolve_selection` 被 media/export 真实调用，必须保留；clear_cache 的 kind 映射仍被缩略图重建使用；人脸激活需先核实配置侧门控。独立移除 `/duplicates` 旧书签 redirect（现行导航使用 `/?duplicates=groups`）属于接受旧 URL 失效，不是删除重复镜头。无调用不是整模块删令。本批不因“也许外部会用”维持旧 API，也不在证据未补足前承诺删除所有 29 个。验证：缩小注册表/常量、相关业务的现行入口回归，不保留命令数量快照来阻止删减。

**P15｜精简未接入扫描的 StorageBackend。** `storage/mod.rs:14,49` 明示原生远程遍历/Range 代理尚未接入 scanner；LocalFs/WebDav 提供 list/stat/read，但真实 UI 当前是连接管理/连通性测试。整层 storage 587 LOC，加 storage_commands 163 LOC，共 750 LOC 的审查面。建议先删没有生产消费者的 stat/read_range 及 RemoteEntry 不消费字段，连接测试降为简单函数；保留当前可见连接管理时不能宣称整层无用。若当前产品只承诺 OS 挂载盘/UNC，则进一步连同 perf/netfs、连接表单/CRUD/凭据一起退役，既有账号清理由明确的数据处理步骤完成。本批不删除路径/凭据保护后留下半残入口。验证：现有连接测试与设置页；完整退役再验证本地/UNC 扫描、根重连。删除规模由是否保留连接管理决定。

补充可先处理的闭包：`db/queries/storage.rs:53` 的 get_storage_backend_config 无构建消费者；`queries/scan/roots.rs:42` 的 set_scan_root_backend 仅测试调用，新增扫描根没有绑定后端的真实入口。可先删约20行的无消费者读函数；backend_id 的模型/投影/列删除属于持久化批次，不为一次小清理提前改库结构。

**P16｜退役未实现的渠道维度。** MsStoreEntitlementStub、SteamEntitlementStub 恒 Unlicensed，steam_restart_if_necessary_stub 仅日志；对应 Cargo feature、三渠道互斥、渠道字段透传和 CI cargo tree 检查共同构成预留面。建议按本次需求只保留已实现 direct，删除 store/steam 空渠道及它们专属配置/测试；没有真实 Store/Steam 收据或交付实现，不以路线图预留为保留理由。它们确实被 feature 和依赖树检查引用，因此不是“完全无调用”。商业/授权能力本身不删；FreeStub 在信任根无效或只读路由时真实使用，保留 fail-closed。先核对签名 registry 的序列化/未知字段策略再删 ms_store_product_id/steam_dlc_app_id，不能因删字段改变待验签字节。约 100–250 LOC 量级，最终按 CI/代码 diff 核算。验证：direct、无授权、无效签名、当前安装/更新链；跨渠道测试随被删除行为一起退役。

**P17｜删除 coordinator 的旧 catalog 默认映射。** `exotic/coordinator.rs:139` 在缺 worker_id 时硬编码 PSD/RAW/video 映射；随包 catalog 对当前缩略图 offering 已显式给 worker_id。改为只消费显式值，旧测试夹具补当前字段。OCR/Enhance 独立服务允许不进入 exotic 调度，不能把其缺字段都当损坏；当前外部已签名 catalog 若仍缺 worker_id，须在加载/解析边界明确报告，不静默落另一套映射。约 7–15 LOC，收益小但移除两份事实来源。验证：catalog/coordinator 当前样本与缺字段行为，先核实磁盘 catalog 加载来源。

**P18｜内联 GalleryLayoutSource 的透传层。** `galleryLayoutSource.ts:54` 唯一生产消费者为 useGalleryVirtualEngine；实际 grid/justified 都由后端产出 LayoutRow，当前没有不同前端 source 实现。用已有 mediaStore 和 useJustifiedLayout 的返回值直接装配，删另一个接口和转发对象；不能把 watch/生命周期调用移到 setup 之外。文件体量 74 LOC，预计真正净减 25–45 LOC，**不是移入相同 74 行就算完成**。验证：引擎 spec 的开关、KeepAlive、resize deferred 行为及两种布局。

**P19｜缩小选择模式注册机制。** `selection/registry.ts:12` 只有 classicMode，useSelection:35 不带参数调用 getSelectionMode，注册/枚举只为预留和测试。可删除 Map、动态注册/枚举及未知 id fallback，直接装配当前实现。**保留 SelectionMode/Intent/State 的解耦与用户选择语义，遵守项目“用户可选选区模式保持解耦”约束。** 不将 classic 逻辑硬塞进手势代码，也不删除 explicit/all 的非物化表示。38 LOC 文件体量，预计净减 20–30 LOC。验证：选区行为测试，包括跨视口 Shift、Ctrl+A、反选、拖动；不以 mock 注册能力作为产品验收。

**P20｜基准专用的旧缩略图总入口。** generate_thumbnail/ThumbResultOrDeferred 在当前 IPC 主路径无调用，**但** `src-tauri/benches/logging.rs:171` 真实使用它。建议让基准调用当前 decode/encode 流程，再删仅为基准留在产品模块里的旧包装及 DTO；如果基准必须新增同等复杂度适配，则此批不成立，暂保留。约 35 行旧实现面，净收益需要扣除基准改动。验证：基准代码局部检查和真实 decode/encode 行为测试；不为此启动完整性能测试。

**P21｜剪除第三方分发附带物和 MathJax。** 第一方 VditorDocumentModule:22 只设置 cdn/cache，未选择 MathJax；本地锁定依赖默认 KaTeX，MathJax 为条件动态加载。Git 跟踪的 MathJax **46 文件、6,634,702 字节**；另 dist/ts 110 文件和 dist/types 1 文件为开发声明/源附带物，不是页面运行资源。建议删除确认无动态加载的这些文件，再审查重复 index/min/method 和示例 SVG；保留 method.min.js、当前公式/字体/语言/图形渲染实际资源与许可材料。Vditor 内容可触发动态下载，不能只做 import 图裁剪。此项计**资源字节/文件数**，不计第一方 LOC。验证：Markdown 公式、代码高亮、图形、预览/导出的网络资源清单与缺失资源错误；需要打包验证时另按已授权范围执行。

### 3.4 C 类：大收益方向，但缺乏删除当前路径的充分证据

**P22｜两套虚拟滚动收敛为 bucket。** useGalleryVirtualEngine 同时装配 useVirtualScroll 和 useBucketVirtualScroll，`bucket_segmented_scroll` 是真实设置，不是固定常量；两套引擎文件分别为 439/694 物理行。建议目标只留 bucket、删旧引擎/切换阀/对应 IPC，但先完成真实大库长距离滚动、坐标平移、时间轴跳转、布局切换、窗口缩放、跨视口选区与恢复比较。用户已授权删除历史兼容，不等于已经证明两套当前功能等价。DOM/Canvas 也暂不一并退役：两者均被生产路径使用，需按交互/可访问性/图像格式逐项验收。无真机证据本轮不承诺它们可删。

**P23｜直接建立当前DDL，删除全部历史升级链（用户已授权）。** 当前仅单人开发，所有旧库与旧数据无需保留。删除V1–V33串行升级、历史修复/回填及其专属测试，以一份当前schema建立新库；不建旧库升级桥、导入转换器或兼容分支。实现必须包含当前有效索引、默认值和数据约束；保留当前事务、WAL/busy_timeout及备份恢复的完整性保护。旧开发库不自动伪装成新结构；测试只使用独立临时库，启动需要重置时仅针对明确的应用开发数据目录，不波及源资产。验收当前结构对拍、新库与当前格式备份恢复，历史版本测试同步退役。

**P24｜集中配置转换并退役旧主题/未知键回落。** StartupConfig 的 Option<String>、TOML 字面量、前端 parse 与旧 `theme` 迁移形成重复转换；get/set_app_config 对未知键落 DB 是为枚举漏项保留的兜底。建议先补全实际动态键调用清单，再把 79 设置/19 状态键各自封闭，未知写入在唯一 IPC 边界报错；旧 theme/DB→TOML/`scroll:` 阅读位置等按真实存量迁移一次后删除，不永久保留双读。不要为 79 个键新增一套通用配置编译器；先删死分支，再收窄热点 DTO。配置文件是外部用户输入，文件载入校验与 IPC 写入校验是两个入口，可共用规则但不能直接移除任一个入口。验证：缺省、新旧配置、未知键、手改 TOML、热更新与重启分类；纯“换成更复杂类型”而无净删不立项。

**P25｜合并 AI 缓存重复生产段。** 缩略图 generator 与 derive/image 可能对同一 AI cache 重复实现落盘/状态刷新，但调度、快慢解码和源修订时序不同。只在确认输入像素、方向、ICC、短边阈值、路径命名、原子发布与 DB source_revision 一致后合并共同段；不能把当前按性能拆开的工作重新变成串行阻塞。这项仍需像素/状态表征，暂不计净删目标。

配置审查另发现一处真实旁支问题：`ipc/enhance_commands.rs:93` 从 ConfigManager 读 `ai_provider`，而该键属于 DB STATE_KEYS，ConfigManager 在无设置定义时返回 None；增强状态的 provider 因而可能始终缺失。它属于数据源误用，应在 P24 相关工作中局部修正或另行处理，本轮未修改。状态键在 TOML 被判未知是正常分工，不应为了“统一”允许将运行状态塞进用户配置。

### 3.5 本轮明确保留，以及被复核否决的误报

| 对象 | 真实保留理由与证据 |
|---|---|
| GPU 令牌与 CPU 重活限流 | `ai/worker_pipeline.rs:256`、`ai/face_pipeline/dispatch.rs:274`、`enhance/service.rs:414,763` 都实际获取 gpu_token；防止不同任务同时挤占 GPU，并遵守 CPU→GPU 固定取锁顺序。分区初筛的“零调用”结论已被终审推翻 |
| logDir ref | `SettingsView.vue:499,502` 读取，`:914,950` 写入；不是死返回字段，删除会破坏日志目录显示/打开/设置 |
| 缩略图旧包装的 bench caller | logging.rs:171 确有调用，所以 P20 必须同批改基准；不能直接归为无调用函数 |
| Worker 的行为边界 | WorkerSupervisor 和 VideoThumbnailWorker 是两个真实 ThumbnailWorker，两个 factory 有实际能力差异；不能仅因为有 mock 就声称抽象必要，也不能忽略第二个生产实现 |
| GPU/WIC/CPU、平台视频分支 | 原生解码能力、图像格式、驱动/设备与运行平台差异是现实；能合并公共步骤，不能在未测平台上声称 fallback 永远不会进入 |
| ViewStale、source_revision、generation、取消与生命周期门 | 防止布局变化后批量操作错对象、旧扫描/解码结果覆盖新状态，分别绑定不同异步任务身份；不是同一个 invariant 的四次检查 |
| rusqlite 写序/WAL/busy_timeout/阻塞线程 | 持久化和 UI 不被同步 IO 卡住的项目约束；删 wrapper 时保留事务、写锁、spawn_blocking 语义 |
| 目录移动日志、恢复交换、原子写入 | 文件已移动但 DB 未提交是真实半完成状态，直接抛错不能恢复一致性；tmp→rename 与恢复日志保留 |
| EPUB 脚本门、asset scope、路径/压缩包/像素输出检查 | 来自不可信电子书、插件包或文件系统边界；与 TS 内部类型保护不是一回事 |
| 授权/签名/下载摘要/keyring | 拒绝无效授权、不覆盖旧凭据、验证包及执行前文件是现行真实路径；删除未来渠道不删除当前付费/信任功能 |
| harness、source-snapshot、探针/对拍工具 | 有主题/Canvas 基准和手动验收消费者；不是 npm script 未注册就可删。尚未接线研究模型工具可后续按是否保留研究能力整批裁决，不能拿工具 LOC 冒充生产简化 |
| 两仓 CI、安装包校验、许可资源 | 私有/公开仓及源码/发货载荷的检查对象不同。已于当日退役的文档门不再重复列为可删成果；第三方许可证不作为剪枝目标 |

没有采用“文件名叫 manager/provider 就删”或“仅测试有 mock 就保留”这两种判断。四份 localStorage 偏好函数虽同构，但默认值/入口有实际差异；本轮不为了几十行新建一套泛化状态工厂。

## 4. 实施批次与验收

### 分批执行顺序

| 批次 | 范围与顺序 | 交付物 / 停止条件 | 最低充分验证 |
|---|---|---|---|
| 1：无消费者代码 | P01–P05、P09–P11、P26；字段与调用点同批删除 | 删除实现+专属测试+失效文档，当前行为不变；发现真实消费者则该项退出本批 | diff/引用检查、typecheck、涉及模块行为测试；Rust 编译到必要 crate/target，禁止默认全 workspace 构建 |
| 2：闲置命令与旧页面链 | P06–P08、P13–P14；先拆旧去重 DTO/查询闭包，再删注册与常量 | 命令数实减、当前 DuplicateLens/start-stop-status 不退化；共享业务函数保留 | 先补当前行为表征，再跑相关去重/布局/选区/缩略图测试和入口手测 |
| 3：实验/预留维度 | P15–P17、P19；P12明确保留，不扩实现 | H-Lab保留；空渠道/空存储方法减少；不增加替代平台或新框架 | 主画廊/本地与UNC/当前授权安装局部验证；模型/实验工具另记开发收益 |
| 4：路径与数据简化 | P18、P20；P25 仅有等价证据时加入 | 透传对象/DTO实减，净行数必须减少；只是搬代码则撤销该项 | 生命周期/resize/基准调用检查；缓存合并需像素和 source_revision 对拍 |
| 5：资源剪枝 | P21 | 文件和字节实减，和第一方代码指标分开 | 内容触发资源访问清单、公式/图形/预览导出；打包检查单独安排 |
| 6：需要动态证据的收敛 | P22 | 只有 bucket 等价验收通过才删旧引擎与设置；DOM/Canvas 单独评估 | Windows 真实大库/弱机、坐标极值、所有选区模式；其它平台未测明确保留边界 |
| 7：存量格式收口 | P23–P24，最后做 | 当前结构单源、全部历史升级退出；旧数据无需保留 | 临时新库/当前备份/配置表征与故障恢复；无需历史兼容 |

每批委派白名单子代理实现，主会话负责设计边界、关键 diff 与验证证据复核。按文件所有权划分并行批，`state.rs`、`ipc/registry.rs`、Cargo/配置 schema 等公共装配点集中整合，不能让多个代理同时修改。维持 High；白名单不可用时停止复杂实施，不由主会话绕过。

不新增永久“剪枝框架”、测试门或通用包装层。依赖安装/升级、完整构建、第三方源码编译仍遵守项目明确授权约束；首轮报告没有替后续这些操作取得授权。

### 测试选择与当前覆盖局限

- 现有 Vitest 使用 Node 环境，包含 `src/**/*.spec.ts`、`scripts/**/*.spec.mjs`；既有选择/布局/IPC 状态等测试可以复用，SSR/纯函数通过不能替代真实 WebView 交互。
- Rust CI 对默认 workspace 和另一个 Linux 平台运行测试，RAW worker 有独立 GNU workspace 的验证。主 workspace 测试不能覆盖所有独立 workspace、所有 feature 组合。
- ORT/模型/大图和硬件测试有 ignore 条目；1597 个静态测试属性不等于1597个实际运行测试。没有 runtime coverage 数据，不能声称“所有核心行为已覆盖”。
- 当前核心行为改动前先补缺失表征；针对被裁掉的历史接口/未接线抽象删除旧测试，保留或改写到实际行为。避免为私有方法/声明数量/固定注册项补镜像测试。
- 每批约定检查通过后停止，不重复全套测试；跨批整合只复查交叉影响。旧迁移、GPU回退、文件破坏风险与授权改动各自要求相应行为证据。

### 量化目标与防止“假减量”

第一轮不承诺“必删30%/50%”：当前证据不支持这种数字。P01/P02/P05 三个完整闲置实现簇就有 655 物理 LOC；H-Lab五文件1,970 LOC按用户裁决保留，不再列为收益。旧去重 5,341 LOC 是混合审查面，尚未切出可删子图。以这些证据为基础，先追求**数千行级的实际净减**，每批精算，不用估算总和替代结果。

| 指标 | 修改前 | 本轮分析结束 | 实施验收口径 |
|---|---:|---:|---|
| 第一方源码 LOC | 264,655 | 264,655 | 使用相同文件分类重新统计新增与删除，不只对原927文件取交集 |
| 第一方源码文件 | 927 | 927 | 减少真实实现文件，测试/工具单列 |
| 主程序顶层模块 | 32 | 32 | 例如 storage 整体退出才记模块 -1；挪进子目录不记简化 |
| IPC 命令 | 261 | 261 | 候选最多29个，逐项确认；H-Lab的2个实验命令按用户裁决保留 |
| 设置键 / 状态键 | 79 / 19 | 79 / 19 | 根据实际消费删键；历史数据迁移另列，不把纯元数据列删减算设置数下降 |
| trait / struct / enum | 20 / 590 / 137 | 相同 | 同一声明计数口径；合并字段/状态须描述真实减少，不能只改类型写法 |
| 第三方物料 | 575文件 / 50,091,714字节 | 相同 | MathJax可审查46文件/6,634,702字节，ts/types另111文件；独立于第一方LOC |
| 业务分支/转换/核心调用链 | 未做AST/动态基线 | 无变化 | 每批列明被删除分支和前后调用链；不虚报全仓分支减少百分比 |

执行期每批在本工作线记录“已删除/已合并/已简化/已保留/待确认”，附实际文件、净减行数、入口/配置/类型变化和验证结果；不维护逐行重复说明。

## 5. 本轮剪枝记录与验证

| 分类 | 本轮实际状态 |
|---|---|
| 已删除 | 无，产品代码净删 0 行 |
| 已合并 | 无 |
| 已简化 | 无 |
| 已保留 | 数据与文件边界、并发/版本保护、权限与签名、当前格式/平台能力差异；具体证据随候选列出 |
| 待确认 | 真实历史数据库/备份版本、未落地产品实验/渠道的当期需求，以及平台样本与动态性能证据 |

实际验证仅为 Git 跟踪清单、逐文件计数、配置/命令/类型声明统计和源码引用审查。本轮未运行前后端测试、覆盖率、完整构建、依赖安装、GPU/模型加载、安装包或跨平台验收。历史文档中的“已通过”不作为本轮测试结果。

交付检查通过：基线分组与逐文件行数相互一致，26个候选ID不重不漏，新文档直接链接/元数据/行尾检查通过，`git diff --check` 通过；Git差异仅在docs，产品文件变化为0。过程已归档至[第一轮工作记录](../../worklogs/2026-09-15-全仓代码消融/closeout.md)，后续状态由[本线分片](../../status/全仓代码消融.md)维护。
