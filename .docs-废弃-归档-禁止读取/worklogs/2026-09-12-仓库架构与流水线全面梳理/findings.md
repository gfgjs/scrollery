---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-12
---

# 发现与证据：仓库架构与流水线全面梳理

## 需求
- 真实数据与控制流、所有主要流水线、五档完成度、关键问题与 P0–P3 计划。
- 不以文件存在判断完成；给出调用点、函数和测试证据。

## 发现
- 初始工作区已有两项删除：.claude/skills/planning/SKILL.md、.commandcode/taste/taste.md；本任务不碰。
- package.json 显示 Vue 3 / Pinia / Tauri 2；Cargo workspace 含主应用、隔离 worker、AI 核心、插件 API、free/pro 组合。
- 可用子代理工具支持用户允许的三种模型，待实际调用验证。

## 外部资料
- 无；本任务以本地代码为准。

## 主会话已核实的架构
- 双入口：src/main.ts 按 Tauri window label 分主窗口/日志窗口；src-tauri/src/main.rs -> lib.rs::run -> storage/db/config/logging boot -> AppState -> coordinator/video service/tasks/tray/watcher。
- UI 为 Vue Router + Pinia；src/utils/ipc.ts 统一 invoke/AppError；命令注册表供键盘/工具栏/右键共享；浏览器 harness 用固定 IPC 数据，不能视为真机验证。
- 画廊链：useJustifiedLayout.compute -> mediaStore.computeLayout -> IPC compute_layout -> SQLite 查询或 ItemsCache 命中 -> Rust grid/justified -> LayoutCache -> 按视口/段取行 -> DOM 或 Canvas。
- compute_layout 实际支持 grid 和 justified；useGalleryVirtualEngine/galleryLayoutSource 的“当前仅 justified/A2 将新增 grid”注释已陈旧。
- 普通画廊还通过 useViewIds.refresh 全量传输 ID 数组并在前端构造 Map，虚拟化仅保证行/卡片非全量；超大库内存须分层讨论。
- 两滚动引擎(传统/bucket)互斥常驻；DOM 默认、Canvas localStorage 实验开关；横向 HGalleryLab 路由仍可达、独立后端缓存。它们是活跃选项/实验，不可直接判死代码。
- viewStore 持四个互斥字段，resolveView 统一优先级；查询构造仍分别在 useJustifiedLayout 和 useViewDescriptor，已有共享解析但未合一。
- AppState 持 DB writer/read pool、ItemsCache/LayoutCache、data_version/dedup_view_epoch/database_lifecycle_epoch、按根 generation 与写门闩、各类取消 token 和资源占用状态；不能只凭锁数量判错，但跨域一致性维护成本高。
- 配置已由 config.toml 管设置；ConfigManager 迁移旧 DB 配置并支持 notify watcher；UI 还有前端实验 localStorage，须区分持久设置/运行状态/实验偏好。
- 初始怀疑 duplicate_lens move 后复用已排除：类型 derive Copy，不是编译错误。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 基于源码的启动、状态、布局与主要流水线地图 | 审查报告 |
| F-002 | 跨流水线状态一致性、取消恢复及真机验证需作为后续整合主线 | 本线状态/报告规划 |
| F-003 | 普通画廊全量 ID 物化、双滚动与 DOM/Canvas 实验分支形成性能验证矩阵 | 本线状态/报告规划 |
| F-004 | 部分陈旧注释与孤立 IPC 事件常量可以源码证实，不应直接当作当前能力 | 审查报告 |
| F-005 | 目录跨卷不更新媒体卷身份，FS成功后DB失败缺明确半完成恢复 | 本线状态 P0-1 |
| F-006 | 前端1781、Rust1495通过但12忽略，NOTICE与改名门失败，安装包真机未验 | 本线状态 P0-2/P1-2 |
| F-007 | 备份范围、增强供给、WebDAV接线、去重browse-only和平台发行边界 | 审查报告 |
| F-008 | 文档门21处旧元数据强制违规，另149处baseline豁免；新记录未报违规 | 本线状态 P2-5 |

## 新增已核实细节
- src/composables/useLensBrowseGate.ts + MediaGrid.vue:1241/1535：重复镜头是明确 browse-only，隐藏/阻断选择、拖拽、右键和批量操作；镜头不全量取 ID，查看器向后端按 layoutVersion 请求相邻项。
- src-tauri/src/db/queries/layout/query_builder.rs：普通取数与 view_to_sql 共用谓词构造；已有 view_layout_parity 等测试。因此后端不是两套完全独立筛选 SQL，重复点主要在前端 DTO 构造。
- src-tauri/src/tasks.rs：卷在线态 15s 对账、filename rank 延迟15s、PRAGMA optimize 延迟3min/周期24h、缓存GC延迟5min/周期24h、日志100ms；自动备份、exotic、video service 由 lib.rs 另起。
- src/constants/ipc.ts:433 的 db:media_updated 本次源码搜索仅常量声明，无监听/发射命中，为可确认孤立常量。
- src-tauri/src/ipc/system_commands.rs:110 clear_logs 在 async command 内直接同步枚举/删除，并吞掉删除错误；src-tauri/src/ipc/config_commands.rs:434 在配置写锁内同步建缓存目录并吞错。这是局部工程缺口，不能夸大为全仓IO均未隔离。
- docs/README 的中文 schema 说明与 .worklogrc.jsonc 实际 ASCII schema 有漂移；新永久文档以实际配置使用 snapshot/review 等字段。
- 缩略图多档服务在 get_layout_rows* async 命令内直接 probe/read_dir、hydrate 持 ItemsCache 读锁逐项 exists；是实在的磁盘热路径，不是纯内存出口，建议 P1测量与移出锁/阻塞执行器。
- AI代理确认主进程 scrollery-ai-core default-features=false，推理移到 ai-worker/enhance-worker；ai/pipeline转发与clip/face/profile再导出是活跃边界，不是双引擎。ai/vector_store.rs 的 trait/BruteForceStore 仅自测引用，实际 search.rs 全量 f16 + Rayon打分，无已接通 ANN。
- 主会话核验增强 registry：PENDING_USER_REPO、bytes=0、sha=None，enhance_manifest_ready 明确 false。内核已写不等于用户下载→使用闭环。
- 插件默认 registry 为 .invalid，但支持运行时 PICASA_REGISTRY_BASE 与编译期 PICASA_REGISTRY_BASE_DEFAULT；pro/trust build.rs支持编译期 keyset注入/debug内测自取。不能写成“任何生产/内测均无法授权/安装”，仅默认发行配置未闭合，本次不验证外部签发。
- AI/face autoResume：并行争会话槽，失败方没有自动排队/定时唤醒，保留active意图但停止；需手动/重启等触发。CPU预算池、GPU批许可和AI/face会话owner职责不同，不应因三者并存就要求合并。
- 媒体代理部分P0判断经主会话驳回复核：非当前档文件被GC删除不会“留下磁盘孤儿”；缩略图既有512档不主动生成64档是明确回退策略/性能欠账，不是数据丢失P0；编辑create_new独占可否阻止跨进程同名需复核。
- ai_cache_short_edge schema明确只作用新缓存、不重建旧缓存；不能把“不重建”当未经声明的功能故障。模型.part也是续传有意保留，不能仅无启动清扫判bug。

## 最终复核与裁定
- 编辑同名临时文件竞态的泛化结论已撤回：目标 create_new 独占先成立，不能绕过它直接推断并发覆盖。
- 旧库索引恢复缺失的判断已撤回：schema当前32，V31和v31_repairs_quick_candidate_index_for_existing_db已覆盖；V27–29保留槽不是坏迁移。
- 移动目录真实问题成立：文件跨卷搬移→子树root/path更新→媒体仅cache_key/thumb_path更新；volume_id仍旧，卷轮询按旧卷批量offline；扫描COALESCE不能治好非NULL错误。未执行用户原件搬移，不称已发生数据丢失。
- 文件已搬而DB提交失败无SavedNeedsIndex类明确结果；tree snapshot清理存在，但不是完整补偿；复制后扫描失败可手动重扫，但metadata连续性仍需验收。
- 备份真实包只有DB、appdata文档和manifest，未包含config.toml/原件/模型/keyring；自动检查首120s后每小时，成功间隔24h。恢复swap与rollback真实被引导流程消费。
- 去重working/publish与取消保护有专项测试；镜头仅浏览，旧软删除IPC不是当前可用清理链。
- TXT/MD syntheticBook、PDF pdfjs-dist、EPUB foliate-js为实际渲染关系。
- 本次Windows验证：Vitest156文件1781通过；Rust1495通过12忽略0失败；check/tests、clippy/all-targets、fmt、vue-tsc、ESLint、Vite通过。NOTICE与改名门各exit1；未运行Tauri打包/安装、真实模型、外部API、Linux/macOS或远端workflow。
- RAW worker/probe独立GNU workspace不在根测试覆盖；私有CI真实配置self-hosted Linux，公开oss-gate配置ubuntu-latest，但本次未核验远端运行。
- 搜索前端有searchToken与searchError；后端共享结果表无query generation条件，因此登记反序写入风险与测试项，不称已复现故障。
- 最终报告含九部分、19条主要流水线及单独WebDAV断点图、五档完成度与15项P0–P3任务。跨域一致性为核心后续整合主线，不建议无需求的通用框架重建。
- 交付前文档检查：worklog-kit check报21处旧文档元数据违反，未命中新报告/状态/收口；index check首次要求归档目录用反引号登记，已据实际约定订正；不清理旧docs正文、不重置baseline。
