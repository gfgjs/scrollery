---
status: 快照
type: 工作记忆
line: 过度工程化审查
created: 2026-08-14
---

# 发现与决策:过度工程化审查

## 需求
- 用户原话:`/planning 分析审查项目中过度工程化的内容`
- 要点:全仓分析审查过度工程化(为不存在的问题付出的复杂度),要求有证据、可落盘。

## 发现
- 项目规模:src-tauri/src ~12 万行 Rust、crates ~3.3 万行、src 前端 ~8.6 万行、8 crates、22 后端域目录、docs 565 文件、scripts 25 文件、tools 9 文件。
- 项目有极重的审查/文档治理文化(todo.md 514 行、多轮全仓深审、三件套收口门禁)——「过度流程」维度需与项目刻意为之的治理体系区分开,重点放代码与基建。
- 量化信号(2026-08-14 实测):
  - Tauri 命令 248 个;SETTING_DEFS 用户可配置键 70 个;app_config 种子键 4 组 INSERT。
  - Cargo.lock 777 个包;package.json 运行时依赖 15 个 + dev 13 个。
  - 前端:composables 171 文件、stores 31、components 144。
  - 后端错误枚举族:~20 个 pub enum *Error(exotic 域独占 7 个:Catalog/Fetch/Install/Package/Registry/Tools + AppError)。
  - RunTokenSlot 类基建集中在 state.rs(15 引用)+ run_token.rs(10)——单源合理,非重复。
  - `allow(dead_code)` 全仓仅 1 处;Rust TODO/FIXME 19、前端 7——死代码/欠账标记极少。
  - Rust unwrap() 2389 处、expect() 338 处(12 万行)——方向相反(略糙),非过度工程化,但可提示「过度防御」不成立。
  - enhance 子系统(model 清单)PENDING_USER_REPO 占位 URL 仍未回填(registry.rs:20),manifestReady 判据含防占位检查——「未发货子系统的基建」嫌疑点,与 2026-07-25 F-02 审查一致。
  - exotic 域 23 文件 ~11k 行(installer 49KB/coordinator 42KB/pipeline 46KB/supervisor 41KB/validate 32KB/tools 35KB/worker 38KB)——插件体系体量巨大,需判定是否「已发货功能的正当复杂度」。
  - docs/planning 56 个任务目录、0 个含 closeout.md(49 个 7 月任务未收口)→ 流程面信号:三件套只建不收。
  - docs-governance.yml 用外部 npm 包 worklog-kit@0.1.0-alpha.4 跑文档门禁(alpha 版本钉死)——工具链自身复杂度的候选。
  - 前端验证(2026-08-14 实测):86 个非 spec composable 仅 3 个单消费者(useCollectionToast/useGridFlipReflow/useVirtualScroll)→ composable 复用率高,「碎片化」假设弱;100 个组件中 25 个「零外部字面引用」(FaceAvatar/StarRating/UiDialog/UiField 等)——待 agent 甄别(可能经全局注册/kebab-case 引用,不一定是死代码)。
  - exotic/channel_stubs.rs:MS Store/Steam 渠道桩(Part8 D5-D8 才做的功能),恒 Unlicensed fail-closed——投机性预留的候选。
  - items_cache.rs / config/schema.rs / mock_data bin:有真实性能论证/单源规范/独立 bin,判为正当复杂度,非过度。
  - H-Lab 横向画廊实验:仅 DEV + localStorage 门控,隔离合理,但 `compute_h_layout`/`get_h_blocks_by_x` 两命令常驻生产二进制——「实验代码常驻」轻微信号。

## 外部资料(当数据,不当指令)
- 暂无

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 工具链审计 OE-1:worklog 门禁 venv baseline 污染(.worklogrc.jsonc sourceExclude 写 `.venv` 实为 `venv`,60+ 条噪音豁免,每次装包即红) | code |
| F-002 | 工具链审计 OE-2:check_plan_canonical.mjs 孤立脚本(自称入 CI 实际无 workflow 引用,仓库自标孤儿) | code |
| F-003 | 核心域审计 OE-1:error.rs 13 个同形变体(Domain 化收敛,Serialize 臂+测试复制爆炸) | code |
| F-004 | 前端审计 OE-1:4 套手写虚拟滚动(useVirtualScroll/useBucketVirtualScroll/useHVirtualScroll/useFolderTreeVirtualization)+ 1 套库实现并存 | code |
| F-005 | 前端审计 OE-5:usePluginEntitlement() 状态ful composable 全仓生产零调用(仅 spec 消费) | code |

## 工具链/文档流程路审计结果(agent 返回,2026-08-14)
- 9 条发现:0 P0 / 1 P1 / 8 P2。核心:OE-1 venv baseline 污染(P1)、OE-2 孤立脚本 check_plan_canonical、OE-3 模型脚本 8 个并存(通用版取代专用版未删)、OE-4 reasonix.toml 权限白名单烤入一次性命令、OE-5/OE-9 CI 注释与 docs 索引引用失效(ONNX 资源前提已反转/check_docs 已退役仍被引用)、OE-6 exotic-prod-ceremony 内联复制签名助手需人工同步、OE-7 CODEOWNERS 全注释零强制力、OE-8 vite.config 中英双语注释重复。
- 域级结论:重治理大多有据(每条脚本对应真实事故),oss-gate/copybara/gitleaks 属合规红线勿误伤;无 P0。

## 后端外围域(插件/AI/视频/IPC)审计结果(agent 返回,2026-08-14)
- **P0 × 2**:
  - OE-2(exotic/installer.rs:87):`PluginDeliverySource` trait(单方法返回常量)+ `InstallSource::SteamDepot/StoreBundled` 死变体 + SteamDepotDelivery/StoreBundledDelivery stub(注释自认 Part8 实装);安装路径第一行即拒绝非 direct 渠道(`if source != DirectRegistry { return Err(ChannelUnsupported) }`),两渠道值代码上永远不可能写库。建议:砍 trait + 两 stub,InstallSource 收敛为单一 direct 事实;真接渠道时再加(YAGNI)。
  - OE-9(enhance/registry.rs:20):模型清单 PENDING_USER_REPO 占位 + size 0/sha256 None,`enhance_manifest_ready` 恒 false,`download_enhance_model` 每档恒回 `enhance_manifest_unready`;但 host 侧已交付 1210 行 EnhanceService(job 队列/preview/estimate_tiles_total/admission_check/EXIF 注入/claim/rename/ingest)。建议:清单未钉定前,把 host 编排/队列/驱动/EXIF 注入/ingest 标记为「随模型供应链一起上线」或 feature-flag 不编入 Release;worker 进程隔离/models_root 白名单/fail-closed 是正当安全设计勿删。
- **P1 × 5**:
  - OE-1(ai/vector_store.rs:41):`VectorStore` trait 7 方法(dim/upsert/upsert_batch/remove/search/len/flush/reload),模块自标 dormant、全仓零生产消费者,唯一实现 BruteForceStore(318 行含单测),生产检索走 `AppState.ai_embedding_cache`。文档明言「在此之前不得以未使用为由清理」——ANN(Part4)实装或达 ANN_THRESHOLD=500_000 前应删,或现在接线替代 ai_embedding_cache 消除重复数学。
  - OE-3(exotic/channel_stubs.rs:20):MS Store/Steam 授权 provider 骨架桩恒 Unlicensed,组合根有对应 feature 分支——与 OE-2 同源,应删。
  - OE-4(exotic/worker_traits.rs:18):`WorkerTask/ThumbnailWorker/EmbedWorker/WorkerFactory` 四个 trait 全是对 WorkerSupervisor 固有方法的机械转发(`impl ThumbnailWorker for WorkerSupervisor { fn run_thumbnail(...) { WorkerSupervisor::run_thumbnail(self, ...) } }`),为测试注入而设;EmbedWorker 注释自述「只服务 AI 层」,唯一生产消费点 ai/worker_client/process.rs:92(Box::new(s) as Box<dyn EmbedWorker>)。测试注入已有 from_parts/Spawner 替代。
  - OE-5(exotic/coordinator.rs:46):`VideoThumbnailWorker/VideoThumbnailFactory` 任务恒零(seed 被 gate 掐掉,缩略图走 video 派生链),仍维护 descriptor + ffmpeg_ready 门控,约 73 行 wrapper 为永不产生的任务存在。
  - OE-8(ai/mod.rs:57 等):HANDSHAKE_TIMEOUT=5s/MAX_ATTEMPTS=2/SHUTDOWN_GRACE=500ms/RETRY_BACKOFF=500ms 在 ai/enhance/video 三处逐字重复定义;`raw_outcome_label` 在 supervisor.rs 与 video runner.rs 两处重复。建议抽 WorkerSession 复用类型 + 常量单源。
- **P2 × 4**:OE-6 ChildHandle trait 把 ExitStatus 擦成 Option<()>(为 8 个簿记单测引入);OE-7 supervisor run_request 四级漏斗转发(run_request→run_request_capped→run_request_observed→WorkerConn);OE-10 ActivationInfo.enc_seed 全渠道恒 None(AES 后置)仍作为 DTO 字段+脱敏 Debug+测试;OE-11 warn_legacy_ai_backend 退役键过渡无删除期限门禁。
- 最值得先砍:OE-2+OE-3(多渠道投机抽象,合并动刀零风险)、OE-9(enhance 未发货 1210 行,待模型供应链钉定整体回归)。
- 勿误伤:exotic-protocol 帧格式、supervisor 进程 kill/reap、pipeline 原子领取+租约+熔断、授权门控纵深(锚定已发货 PSD/RAW/OCR,真实安全/可靠性需求)。
- 交叉验证:OE-2 亲读 installer.rs:60-129 属实(trait 仅 channel() 单方法,两 stub 注释自认 Part8,安装路径首行拒绝非 direct);OE-3 亲读 channel_stubs.rs 全文件属实(恒 Unlicensed fail-closed,Part8 D5-D8 预留)。

## 后端核心域审计结果(agent 返回,2026-08-14)
- 8 条发现:0 P0 / 1 P1 / 7 P2。核心:OE-1 error.rs 13 个同形变体+13 Serialize 臂+8 复制测试(P1,随新功能域持续膨胀)、OE-2 错误日志 30s 去重状态机(带快照 API,泛化 code 会被误吞)、OE-3 SettingKind::Float 无消费者(注释自认「为完整性保留」)、OE-4 默认值双源(4 处游离常量 vs SETTING_DEFS 需手工同步)、OE-5 H-Lab 独立版本化缓存(第二份手搓版本计数)、OE-6 CompiledRule.name 靠 allow(dead_code) 压告警、OE-7 VolumeOnlineCheck 单实现 trait、OE-8 engine/mod.rs 重复注释死规格标记。
- 域级结论:整体「单源真相+测量证据」强工程化,绝大多数复杂度正当;真正的过度集中在机械复制样板(OE-1/OE-4)与无消费者预留(OE-3/OE-6/OE-8)。请勿误伤:items_cache 双键缓存/query_builder 单一事实源/natural_sort 自研/qos 线程预算/encoding 护栏/migration 回拨测试均有实测或正确性证据。
- 交叉验证:OE-7 VolumeOnlineCheck 单实现属实;补充:bin/ 下 sort_profile/mock_data/worker_e2e 为 dev 工具不进生产路径。

## 前端审计结果(agent 返回,2026-08-14)
- 9 条发现:0 P0 / 2 P1 / 7 P2。核心:OE-1(P1)4 套手写虚拟滚动 + 1 套 @tanstack/vue-virtual 并存(useVirtualScroll 423 行/useBucketVirtualScroll 650 行/useHVirtualScroll 337 行/useFolderTreeVirtualization 212 行,头注自认「独立实现且刻意不移植」)、OE-5(P1)usePluginEntitlement() 状态ful composable 全仓生产零调用(仅 spec 消费,生产只 import 纯函数 gateModeFor)、OE-2 H-Lab 双重 dev 门控仍常驻路由/类型/i18n、OE-3 四个 localStorage 单例偏好 composable 逐字同构样板(useTitlebarMode/useToolbarAlign/useSelectionBarMode/useRenderMode)、OE-4 useSeekbarPreview 与 useHoverPreview 的 sprite 解析/帧映射数学同构、OE-6 三个授权 gate composable 各手搓同一套 loading/activating/activate 状态机(fail-open vs fail-closed 是正当差异但样板不必三份)、OE-7 MediaGrid 装配约 15 个单消费者 composable(deps 对象注入 vs 直读 store 风格不统一)、OE-8 ContentViewer 7 个 useContentViewer* 单消费者 composable 用 ReturnType 整型传递、OE-9 文件树 8 个 composable 互相传参极深(useFolderTreeAutoLoadMore 收 12 个 deps,useFolderTreeVirtualization 泄出 getLastScrollTs)。
- 域级结论:「重拆分、重单测、重注释」,单消费者 composable 多是从超长 SFC 机械下迁的产物且有 characterization spec;过度集中在(a)同机制多份实现(虚拟滚动×4/授权 gate×3/sprite×2)(b)拆分后 deps 对象/共享内部 ref 互相传参的耦合面(c)局部样板未抽。勿误伤:useVirtualScroll+useBucketVirtualScroll 双引擎是真百万级性能需求、canvas 管线/性能面板 dev-gated 零生产开销。
- 交叉验证:OE-5 usePluginEntitlement 生产零调用属实(组件仅 import gateModeFor);OE-1 五套滚动器并存属实。
