---
status: archived
type: review
line: 新立项视角产品与架构全面评审
created: 2026-07-13
last-verified: 2026-07-13
---

> 📦 **已归档（2026-07-13）**：本文是 ChatGPT 5.6 Sol 的单专家原始 Review，已由[多专家综合 Review](../reviews/2026-07-13-新立项视角产品与架构综合Review-chatgpt-5.6-sol.md)吸收、复核并取代。正文冻结，仅保留为证据与意见演进存档；后续决策不要单独以本文为准。

# Scrollery 新立项视角产品与架构全面 Review

> - 评审角色：新产品立项产品经理 + greenfield 全栈架构师
> - 评审日期：2026-07-13
> - 评审对象：当前仓库表现出的产品能力、用户表面、技术清单、数据模型、质量与交付体系
> - 评审边界：现有代码与文档只作为“现状证据”，不把项目内既定方案、完成标记或历史路线当作设计前提；不评价具体函数写法，不提出逐文件修补清单
> - 外部资料：只采用官方产品文档、官方项目文档或官方仓库，访问日期均为 2026-07-13

## 0. 执行结论

### 0.1 立项结论：有条件 GO，但必须先重做产品定义与数据地基

本项目有一个真实、清楚且可形成口碑的产品楔子：

> 面向拥有多年本地照片目录、十万到百万级资产的个人与家庭，提供无需搬动原片、无需上传云端、几秒可开始浏览的智能照片库；用户即使只记得画面、人物、地点或大致时间，也能快速找回，并且长期保有原文件与整理成果的控制权。

当前最强的资产不是“功能多”，而是大图库渐进可用、本地处理、桌面原生文件能力和 AI 检索的组合。README 的核心承诺也集中在“十万到百万张照片流畅浏览”与 AI 语义搜索（`README.md:3-7,16-21`）。这条赛道有明确用户痛点，也存在竞争空位：它可以比 digiKam 更容易上手，比 Immich 更少运维，比 PhotoPrism 更像桌面产品，比纯云照片服务更尊重本地文件。

但当前产品已经从这个楔子扩散为“照片/视频管理 + 音频播放器 + 文档阅读与版本管理 + AI 校对 + 网络存储 + 人脸识别 + 商业插件平台 + 多渠道分发”的技术集合。路由已经把插件商店、文档、音频与实验室暴露为一等页面（`src/router/index.ts:46-83`），数据库也承载文档替换、文档版本、阅读偏好与书签（`src-tauri/src/db/schema.rs:336-371,655-691`）。这不是单纯的功能丰富，而是同时服务多种互不相同的 JTBD。

因此，本报告的结论不是“沿当前功能表继续补齐”，也不是“全部推倒重写”，而是：

1. 先冻结新功能扩张，确定照片/视频 local-first 智能图库为唯一主产品。
2. 先解决 Asset Identity、统一 Job System、备份/可移植、隐私承诺四个地基问题。
3. 再用真实用户测试证明“导入—找回—整理—恢复—导出”闭环好用。
4. 文档、音频、插件市场、server、多用户、E2EE sync 等只在核心指标过门后再立独立业务线。

### 0.2 一句话产品判断

当前形态是一套技术能力很强的桌面原型，但还不是用户容易理解、愿意长期托付照片的完整产品。

它已经较好回答“如何让百万图库跑得动”，但尚未完整回答：

- 为什么普通用户每天或每周还要回来？
- 用户是否相信它不会上传、移动、泄漏或弄丢照片？
- 用户投入的收藏、评分、人脸命名在换机、数据库损坏后如何回来？
- 找到照片后如何可靠导出、分享与交付？
- 文件被重命名、移动、复制或磁盘重挂载后，它还是“同一张照片”吗？
- AI、缩略图、扫描、派生、插件任务失败时，用户能否理解并恢复？

### 0.3 新立项评分

评分代表“作为新产品立项是否成立”，不代表代码质量。

| 维度 | 评分 | 结论 |
|---|---:|---|
| 用户痛点与市场楔子 | 4/5 | 大型本地图库、隐私、找图困难是真问题，承诺具体 |
| 差异化潜力 | 4/5 | local-first + 大库性能 + 可解释 AI 的组合有空间 |
| 产品边界 | 1/5 | 照片、阅读、音频、插件、网络存储与商业渠道并行 |
| 首次激活 | 1/5 | onboarding 把 2/3 步骤给主题和语言，核心扫描闭环不可靠 |
| 浏览与性能体验 | 4/5 | 已围绕百万规模建立大量针对性能力，但需真实硬件基准 |
| 搜索与发现 | 3/5 | 技术能力强，用户心智被 normal/mixed/semantic 等模式分裂 |
| 整理与交付闭环 | 2/5 | 收藏、评分、移动、复制已有，导出/分享/迁移/长期恢复不足 |
| 数据可靠性 | 2/5 | 有事务迁移、WAL、离线卷，但稳定资产身份与可移植事实层不足 |
| 隐私与信任 | 1/5 | 有本地处理基础，缺用户可见的 consent、数据地图、联网说明 |
| 架构可演进性 | 2/5 | 技术栈适合单机，但 179 个 IPC、全局状态与分裂状态机已显复杂 |
| 测试与交付 | 3/5 | 单元/静态门较强；E2E、视觉、可访问性、恢复演练与公开 PR 门不足 |
| 商业清晰度 | 1/5 | 免费/付费、AI/人脸/插件边界与首发价值尚未形成用户可懂的包装 |

## 1. 评审方法、证据等级与限制

### 1.1 证据等级

- **A 级仓库事实**：当前 manifest、schema/migration、用户可见路由/文案、CI 与发布配置直接证明。
- **B 级外部事实**：竞品官方文档、官方仓库或官方支持文档直接证明。
- **C 级专业判断**：由 A/B 级事实推导出的产品或架构判断。
- **D 级待验证假设**：必须由用户访谈、可用性测试、性能基准、故障演练或市场实验验证。

报告中“当前存在/当前暴露/当前采用”主要是 A/B 级事实；“用户会困惑/会降低信任/应优先”属于 C 级判断；指标目标与用户偏好均属于 D 级假设，不应伪装成已验证市场事实。

### 1.2 本轮取证范围

- 产品承诺：`README.md`、`CHANGELOG.md`、`CONTRIBUTING.md`。
- 用户表面：router、onboarding、sidebar、toolbar、Settings、Collections、Persons、Viewer、i18n 文案。
- 技术与架构：根 workspace、前后端 manifest、Tauri 配置/capability、模块目录、AppState、IPC 注册面。
- 数据结构：schema v1-v18、migration runner、SQLite connection/WAL 策略。
- 质量与交付：frontend tests、Rust tests、CI、OSS gate、release workflow。
- 外部基准：digiKam、Immich、PhotoPrism、Ente Photos、Apple Photos、Google Photos、Lightroom。

### 1.3 明确限制

本轮没有把以下事项冒充为已验证：

- 没有目标用户访谈、市场规模、付费意愿或竞品迁移数据。
- 没有在真实 10 万、100 万图库上重跑性能 benchmark。
- 没有完成 Windows/macOS/Linux 全矩阵真机体验。
- 没有完成屏幕阅读器、键盘全旅程、触控板、高 DPI 与视觉回归测试。
- 没有执行数据库破坏、断电、升级回滚、换机、目录大规模移动与备份恢复演练。
- 没有把仓库中的“已完成”或既定设计直接当成产品正确性的证据。

## 2. 建议重写的产品定义

### 2.1 主要目标用户

第一目标用户不是“所有媒体用户”，而是：

1. 拥有 5 万到 100 万照片/短视频，散落在电脑、移动硬盘、NAS 挂载目录中的个人或家庭档案维护者。
2. 不愿把全部原片上传云端，重视隐私、文件自主权和离线可用的人。
3. 想获得现代搜索、人物、地点、回忆能力，但不愿维护 Docker/Postgres/Redis 的桌面用户。
4. 愿意使用桌面工具进行批量整理，但不想理解 GPU、batch、WIC、DOM/Canvas、bucket、model profile 等实现术语的“普通高级用户”。

### 2.2 明确非目标

MVP 不应同时服务：

- 专业团队协同 DAM 与审核流；
- Lightroom 级 RAW 调色与完整 non-destructive editor；
- Google Photos/Immich 级手机自动备份与家庭云；
- Calibre/阅读器/文档知识库；
- 音乐库与歌词播放器；
- 通用文件管理器；
- 第三方开发者插件生态；
- 企业多租户、权限与审计系统。

这些方向都可以成立，但每一个都足以成为独立产品。把它们同时放入 MVP 会让导航、数据模型、测试矩阵、发布承诺和商业包装呈乘法增长。

### 2.3 核心 JTBD

1. 当多年照片散落在多个目录和磁盘时，我想在不搬动、不改名、不上传原片的情况下，迅速得到一个统一图库。
2. 当我只记得画面、人物、地点或大致时间时，我想在十几秒内找回目标照片，不必记文件名和路径。
3. 当我整理大量照片时，我想清楚知道哪些操作只改 catalog，哪些会改真实磁盘，并且误操作可以恢复。
4. 当移动盘离线、路径变化、换机或数据库损坏时，我想保住相册、评分、标签、人脸命名等劳动成果。
5. 当我找到了照片时，我想可靠地导出原片或副本、保留或移除 metadata，并得到失败报告，而不是只停留在查看。

### 2.4 建议价值主张

> 选择现有照片目录，Scrollery 不搬原片、不改原片、默认不上传；几秒开始浏览，后台逐步补齐人物、地点与自然语言搜索。索引可以重建，整理结果可以备份和带走。

这个承诺比“高性能媒体资源管理器”更接近用户结果，同时给产品立下四条不可轻易破坏的信任契约：

- 原片属于用户，不属于 catalog。
- 默认只读；任何写原片或 sidecar 的行为必须显式授权。
- 派生物可丢弃、可重建；用户整理事实必须可备份、可导出。
- 本地处理、联网下载、远端传输的边界必须可见。

### 2.5 北极星体验

建议将北极星体验定义为：

> 用户添加一个多年旧图库后，在 15 分钟内成功找回一张一年以上、原本不知道文件名和目录的照片，并理解原片未被移动或上传。

“百万图库能滚动”是关键性能门槛，但不是完整用户价值；“找回重要照片且愿意继续托付整理成果”才是产品结果。

## 3. 当前产品做对了什么

### 3.1 技术投入围绕一个强结果：大库尽快可用

两阶段扫描、后端布局、可见行虚拟化、按需 metadata、bucket virtualization 与 resident embedding cache 都指向“不要让规模拖垮浏览”（`README.md:16-21`）。这是一条一致的工程主线，应该保留。

面向用户时，应把它翻译成：

- 不必等待全部扫描完才看照片；
- 扫描和 AI 在后台运行，前台仍可浏览；
- 移动盘暂时离线不会被误判成删除；
- 大图库搜索和滚动不会随规模线性恶化。

### 3.2 本地桌面架构与目标场景匹配

Tauri + Rust + Vue 对“桌面、本地文件、较重媒体处理、跨平台 UI”是合理组合。Rust 负责扫描、metadata、thumbnail、布局与本地数据库，Vue 负责交互，重型 AI 进入独立 worker；这比把所有处理放进 WebView 或一开始部署 server 更适合目标用户（`README.md:9-12`；`src-tauri/Cargo.toml:79-83,149-158`）。

### 3.3 已意识到长任务、离线卷与可恢复状态

数据库为扫描、thumbnail、AI、face、derivation、exotic 等处理保留状态；离线卷与用户回收站也被区分（`src-tauri/src/db/schema.rs:235-249,270-305,440-451,532-599`）。这证明项目已经意识到“处理不是同步请求”“离线不等于删除”。

问题不在于没有状态，而在于这些状态没有形成统一的领域模型与统一用户任务中心。

### 3.4 数据库事务与单机读写策略稳健

SQLite 采用 WAL、foreign keys、busy timeout、mmap、单写连接与只读池（`src-tauri/src/db/connection.rs:24-34,45-56,112-140`）；migration 把单版本 DDL 与 schema version 写入同一事务（`src-tauri/src/db/migration.rs:51-62`）。对 local-first 单用户 MVP，这是合理且节制的选择。

### 3.5 质量门禁广度高于普通原型

当前仓库约有 66 个 frontend spec 文件、580 个 Rust test attribute；CI 覆盖 fmt、check、clippy、workspace tests、typecheck、lint、Vitest、production build、文档与发布规则（`.github/workflows/ci.yml:58-71,153-224`）。这说明工程团队有质量意识，不应在 greenfield 方案中丢掉。

## 4. P0 产品问题：不解决就不应正式发布

### P0-1 产品边界失控，主用户和主导航不再一致

事实：README 仍把产品定义为大规模照片浏览与 AI 搜索（`README.md:3-7`），但正式路由已包含 Plugin Store、Document Viewer、Audio Player、统一媒体查看器与 H-Lab（`src/router/index.ts:46-83`）；schema 则继续扩展到文档替换、版本、阅读偏好与书签（`src-tauri/src/db/schema.rs:336-371,655-691`）。

判断：用户会难以回答“Scrollery 到底是照片库、通用媒体库、阅读器还是插件平台”。每条新增线又带来独立的导入、查看、搜索、metadata、派生、设置、错误与恢复语义，核心闭环反而没有优先补齐。

建议：

- MVP 只承诺照片与短视频。
- 文档编辑/版本/AI 校对、音频歌词、插件商店、H-Lab 退出主导航并冻结新增。
- 若未来恢复，必须按独立业务线重新做用户研究、成功指标与架构预算。

### P0-2 首次使用没有围绕“第一批照片出现”设计

事实：onboarding 的三步是选择目录、主题、语言（`src/components/common/OnboardingWizard.vue:38-98`）；选择目录只登记 scan root（`src/components/common/OnboardingWizard.vue:178-188`），而侧栏正常添加目录流程会继续显式启动扫描（`src/components/sidebar/sections/FoldersSection.vue:937-947`）。完成条件只是写入 `first_launch=false`（`src/components/common/OnboardingWizard.vue:207-216`）。

判断：产品可能把“完成引导”当作激活，而用户的真实激活是“看见自己的第一批照片”。主题和语言可以读取系统，不应占首次体验的 2/3。

建议的首次旅程：

1. 一句话价值与隐私承诺。
2. 多选目录或磁盘，默认只读。
3. 预检文件数、支持格式、目录重叠、预计缓存与可用空间。
4. 立即扫描，第一批 thumbnail 出现即进入图库。
5. 任务中心解释后台仍在补齐的 metadata、人物和语义索引。
6. 给出“试着搜索一张旧照片”的成功任务。

### P0-3 找到照片之后，交付与保全闭环不足

事实：当前用户表面突出收藏、评分、颜色、删除、移动、复制、设壁纸与复制位图；未形成一等的 catalog backup/restore、批量 export、metadata export、换机迁移与系统分享旅程。仓库搜索到的 backup 主要是插件安装或文档覆盖备份，不是用户 catalog 备份。

判断：资源管理产品如果不能让用户把原片/副本可靠交付，也不能保护用户整理成果，用户不会愿意长期投入人脸命名、相册、评分和标签。

MVP 必须补齐：

- Export Original 与 Export Copy 两条明确路径；
- 格式、尺寸、质量、metadata、命名与冲突策略；
- 大批量导出进入统一 Job Center，支持暂停、重试与逐项报告；
- catalog 定时备份、手动备份、恢复预览、版本兼容与恢复点；
- 明确“catalog backup 不包含原片”，并提供原片备份健康提示；
- 换机后重新关联磁盘/目录，不丢用户整理事实。

### P0-4 删除与真实文件操作的信任模型不完整

事实：图库删除、收藏夹删除主要依赖几秒钟的 toast undo；真实磁盘移动、复制、新建和删除与 catalog-only 的收藏、评分、相册操作出现在相近的操作面。数据层已为媒体与收藏夹建立 soft-delete，但产品面仍缺统一的历史与冲突恢复。

判断：照片是不可替代资产。用户必须在操作前就理解“只改 catalog”还是“会改磁盘”，且恢复能力不能只依赖瞬时 toast。

建议：

- 正式 Trash：Restore、Restore to、Delete Permanently、Empty Trash、保留期。
- Activity/Undo Journal：记录 move/copy/rename/delete/metadata writeback，展示逐项结果。
- 所有磁盘写操作先显示源、目标、数量、空间、同名冲突与可撤销性。
- 虚拟“相册”明确标注不会移动原文件；避免“收藏夹”同时指 Favorites、Album 与物理文件夹。

### P0-5 隐私是架构事实，但还不是用户契约

事实：产品会处理路径、EXIF/GPS、人脸 embedding、语义 embedding、日志、WebDAV 凭据，也会下载模型和插件；Tauri CSP 和系统 keyring 等技术基础存在（`src-tauri/tauri.conf.json:43-48`；`src-tauri/Cargo.toml:106-109`），但 onboarding 没有解释默认是否联网、哪些数据离开设备、敏感派生物在哪里、如何删除。

判断：“本地程序”不自动等于“隐私产品”。用户无法从技术实现推断网络、日志、模型、插件和凭据边界。

建议建立 Privacy Center：

- 默认承诺与真实行为一致：“默认仅本机索引，不上传照片”。
- 列出 original、thumbnail、embedding、face vector、日志、凭据的位置、大小、生命周期与删除入口。
- Network Activity 显示模型/插件下载、WebDAV、远程校对等出站目的与最近活动。
- 人脸识别显式 opt-in，解释用途、模型许可、敏感性、删除和重建。
- 模型/插件安装前展示来源、大小、hash/signature、许可、权限与联网范围。
- 日志默认脱敏绝对路径、用户输入、GPS、token 和 URL credentials。

### P0-6 稳定 Asset Identity 尚未成为数据模型地基

事实：`media_items` 由自增 id 标识，但记录与 `directory_id + file_name` 唯一约束绑定，`content_hash` 可空（`src-tauri/src/db/schema.rs:74-109`）；volume 与 relative path 是后续迁移加入的可用性信息（`src-tauri/src/db/schema.rs:564-599`）。当前模型没有把“逻辑资产”“同一内容的多个文件实例”“当前位置与历史位置”明确分开。

判断：路径变化、重命名、跨盘移动、重复副本、RAW+JPEG、Live Photo、编辑版本会不断冲击 album、rating、person、AI 与导出引用。只要身份仍偏向路径记录，用户 metadata 就有丢失或错误归属风险。

建议把 Asset Identity 作为 release blocker，目标模型见第 8 章。

### P0-7 后台处理有多套状态机，却没有一个用户可理解的 Job System

事实：处理状态分散在 `media_items.thumb_status/ai_status/face_status`、`media_derivations`、`exotic_tasks`、`face_coverage` 与内存 cancellation token 中（`src-tauri/src/db/schema.rs:91-93,235-240,270-284,440-443,494-522,721-734`；`src-tauri/src/state.rs:68-73,101-167`）。状态既有整数，也有文本；重试、租约、进度和错误字段并不统一。

判断：产品将很难提供一致的“正在做什么、完成多少、为什么失败、能否暂停、是否会自动重试、关机后怎样”的任务中心；架构也会重复解决调度、公平、取消、恢复和资源预算。

建议：建立统一持久 Job System，所有扫描、metadata、thumbnail、AI、face、OCR、duplicate、reconciliation、sidecar、export、backup 共用同一状态与资源模型，目标结构见第 9 章。

### P0-8 可访问性不能成为性能模式的交换条件

事实：产品设置与工具栏暴露 DOM/Canvas 与多种实验渲染选择，i18n 甚至直接出现“切换画廊渲染方式（DOM / Canvas 对比）”和 H-Lab（`src/i18n/locales/zh-CN.ts:150-190`）。Canvas 路径无法天然提供逐项 DOM semantics。

判断：实现策略不应成为普通用户设置；更不能让屏幕阅读器与完整键盘旅程因“性能模式”而消失。

建议：

- 正式产品只暴露结果级选择，例如“兼容 / 平衡 / 极致性能”。
- Labs 明确非正式支持，并显示 accessibility 限制。
- DOM 或等价 virtual accessibility tree 必须完成导入—搜索—打开—收藏—删除—恢复全旅程。
- E2E 验收包含键盘与屏幕阅读器，不以单元测试替代。

## 5. P1/P2 产品问题：决定产品是否“喜欢用”

### P1-1 搜索把技术分支暴露给了用户

当前文案和交互区分 normal、mixed、semantic，再细分 filename、folder、date、device、location、global（`src/i18n/locales/zh-CN.ts:115-202`）。这更像搜索引擎调试台，而不是“我想找照片”。

目标应是一个搜索框、两层心智：

- 默认层同时搜索 filename、date、place、person、metadata 与 semantic，按实体和结果类型组织。
- 高级层提供 camera、lens、rating、tag、resolution、format、folder、missing metadata、processing state 等精确 filter。
- AI 索引未完成时，普通搜索仍返回结果，并显示“语义结果覆盖图库的 35%”，而不是要求用户先切模式或启动全量任务。
- 搜索词、filter、sort、layout 与当前上下文进入 URL/route state，Back/Forward 可重现。
- 支持 Saved Search；它比再增加一种固定“智能相册”更具扩展性。

### P1-2 信息架构混合了内容、投影、维护任务与实现选项

侧栏“图库”同时放所有照片、Favorites、Live Photo、Recent、Trash、Collections、People 与 Plugin Store（`src/components/sidebar/sections/LibrarySection.vue:24-58,81-109`）；“工具”常驻 AI/face 全量分析；Settings 暴露 thumbnail 编码、bucket、GPU、Canvas、batch 等实现参数。

建议 IA：

- **Library**：All Photos、Recent、Favorites、Albums、People、Places、Folders、Trash。
- **顶部主操作**：Search、Add Source、Import、Export。
- **Job Center**：只有运行、失败或需要处理时显著出现，支持查看全部历史。
- **Manage**：Sources、Storage、Backup、Privacy、Models、Extensions。
- **Advanced/Labs**：实现实验、诊断、兼容开关；默认不进入普通用户路径。

Folder、Album、Person、Place、Moment、Stack、Saved Search 是不同 projection，不能因为都能“装照片”就压成同一种 collection 或一棵树。

### P1-3 设置页把产品责任转嫁给用户

普通用户不应负责选择 WIC、GPU、batch size、DOM/Canvas、thumbnail tier、时间轴几何和缓存内部策略。参数越多并不代表控制力越强，它也会让每个性能问题变成“是不是用户配错了”。

建议只暴露意图级 preset：

- 兼容优先；
- 平衡（推荐）；
- 速度优先；
- 节省磁盘。

应用根据硬件、图库规模和失败记录自动选择 backend 与并发。Expert 页面可以显示实际选择、原因、benchmark 结果和恢复自动设置按钮，但不应成为普通 onboarding 的一部分。

### P1-4 当前强项偏“管理”，长期留存理由不足

性能决定第一次印象，搜索决定找图效率，但它们不自动产生持续复访。家庭照片产品的留存通常来自三种循环：

- **Rediscovery**：On This Day、People、Trips、Moments、随机精选。
- **Maintenance**：duplicate/similar、模糊图、截图/票据、离线盘、未备份提醒。
- **Curation**：Saved Search、智能相册、批量整理、导出历史。

优先建议是 On This Day、duplicate/similar、Saved Search 与 Backup Health。它们比扩展阅读器或插件市场更贴近核心用户，也复用现有时间、hash、AI 和 catalog 能力。

### P1-5 产品包装与开源/付费边界不清

README 表述“图片浏览 + AI 语义搜索已实现”（`README.md:6-7`），CONTRIBUTING 又把 AI inference、face recognition 与 exotic plugins 列为闭源商业产品（`CONTRIBUTING.md:5-18`），CHANGELOG 则把 AI、人脸、插件平台都列在首版新增能力中（`CHANGELOG.md:19-28`）。

这会产生三个用户问题：

- 下载免费版后到底能用哪些能力？
- 付费购买的是模型、格式支持、性能还是持续更新？
- 用户的 catalog 是否被商业 feature 锁定？

建议商业原则：

- 核心 catalog、浏览、metadata 搜索、备份/恢复、导出必须免费且无锁定。
- 付费能力可以是高成本 AI model、专业格式、批量自动化或跨设备服务。
- 即使订阅到期，已有用户 metadata 与原文件仍可读、可导出；派生能力降级不能让 catalog 不可用。
- 产品页用 capability matrix 展示 Free/Pro 与平台差异，不让用户理解 Cargo feature 或渠道实现。

### P1-6 “跨平台”承诺缺少产品级支持矩阵

README 和 package 描述 cross-platform（`README.md:3-4`；`src-tauri/Cargo.toml:1-7`），但当前视频 backend 只有 Windows Media Foundation，非 Windows 路径会返回无 backend（`src-tauri/src/video/mod.rs:76-92`）；GPU、窗口行为、网络盘、壁纸和安装包在各 OS 上也不等价。CI 有 Windows 与 Linux compile/test，发布 workflow 只构建 Windows MSI/NSIS（`.github/workflows/ci.yml:101-137`；`.github/workflows/release.yml:35-39,84-97`）。

建议首发若现实上是 Windows-first，就明确写 Windows-first。真正 cross-platform 的验收必须是逐 OS capability matrix 与 runtime/E2E，不是“能 cargo check”。

### P2：可以后置但需要预留的数据语义

- RAW+JPEG、Live Photo、burst、edited copy、duplicate、stack 的关系模型。
- OCR、place reverse geocoding 与 sensitive content 的可选派生。
- 手机 device、ingestion source、remote identity、sync revision 与 tombstone。
- 共享时的 viewer/contributor、expiry、password、download、GPS/EXIF redaction。
- 多用户与 E2EE 只有在核心产品验证后实施，但不能把 path-based identity 固化到无法演进。

## 6. 竞品与优秀开源项目基准

### 6.1 市场范式

| 范式 | 代表 | 核心承诺 | 主要代价 |
|---|---|---|---|
| 专业桌面 DAM | digiKam、Lightroom | 文件、RAW、metadata、筛选、批处理与编辑深度 | 功能密度与学习成本高，移动/家庭闭环弱 |
| 家庭自托管照片云 | Immich | 手机备份、跨端时间线、AI、分享，服务器归用户 | Docker、数据库、队列、网络和备份运维 |
| 既有目录 Web 图库 | PhotoPrism | 不搬目录，在 NAS/浏览器获得现代图库 | 手机自动备份与消费级闭环依赖第三方 |
| 隐私优先加密云 | Ente Photos | E2EE 下仍有跨端备份、分享与本地 AI | 密钥恢复、客户端计算与同步一致性复杂 |
| 消费级生态照片库 | Apple Photos、Google Photos | 拍完即出现、无需整理也能找回与回忆 | 云/生态依赖，文件系统与专业 metadata 控制弱 |

### 6.2 digiKam：学习文件自主，不复制界面复杂度

digiKam 官方定位覆盖 camera import、album、tag、rating、label、EXIF/IPTC/XMP、RAW、视频、编辑和批处理，并强调数据与 AI 默认在本机、没有 telemetry 或云端上传（[digiKam About](https://www.digikam.org/about/)）。

值得借鉴：

- 原文件、XMP、metadata 与专业批处理是一等公民。
- 专业用户需要精确 filter、batch、颜色管理与 sidecar portability。
- 强写回能力必须有 preview、dry-run、冲突策略和 undo journal。

不应复制：

- 多侧栏、多工作空间、九类组织入口同时暴露给普通家庭用户。
- 把每一种专业能力都当作主导航。

### 6.3 Immich：学习完整 lifecycle 与 Job Center

Immich 不是单一图库，而是手机备份—时间线—搜索/人物—相册/分享—释放空间的连续产品。官方架构采用 client-server、Postgres、Redis/BullMQ、独立 ML service，并把 thumbnail、metadata、transcode、smart search、face、sidecar 与删除作为后台 job（[Immich Architecture](https://docs.immich.app/developer/architecture/)）。

官方也清楚区分数据库与原文件备份：数据库自动备份不包含照片，完整保护仍需备份原文件；恢复前会创建 restore point 并执行兼容性和健康检查（[Immich Backup and Restore](https://docs.immich.app/administration/backup-and-restore/)）。

值得借鉴：

- Timeline 是主心智，Folder 是控制和诊断投影。
- 统一 job queue 与管理页面让长任务可理解、可恢复。
- backup/restore 是用户产品面，不是运维脚本附录。
- ML service 与核心产品可关闭、可替换、可独立扩展。

不应复制：

- 在单机 MVP 中引入 Postgres + Redis + server 的运维成本。
- 让应用内部 storage layout 成为用户不能触碰的封闭目录。
- 把大量 model 名称和硬件 trade-off直接交给普通用户。

### 6.4 PhotoPrism：学习“零迁移接管目录”

PhotoPrism 官方区分 Index 与 Import：Index 直接索引 originals，不重命名、不移动、不改变目录结构；Import 则复制、去重并整理到 originals，官方建议已有图库优先 Index（[PhotoPrism Library](https://docs.photoprism.app/user-guide/library/)）。

值得借鉴：

- “不搬文件就得到现代图库”是清晰价值，而不是技术选项。
- Folders、Albums、People、Places、Moments、Calendar 与 Search 是不同投影。
- 派生 thumbnail/index 可重建，original 不由 catalog 生命周期支配。

不应复制：

- 把 Index/Import/WebDAV 的内部区别直接变成首启认知负担。
- 把 PWA 或 WebDAV 当成完整 mobile backup 体验。

### 6.5 Ente Photos：学习把隐私写入数据结构

Ente 的 master key、collection key、file key、分享密钥与 recovery key 形成端到端加密数据模型，而不是一个“启用加密”设置；文件与 metadata 在客户端加密，分享通过对接收者包装 collection key 完成（[Ente Architecture](https://ente.com/architecture/)）。

值得借鉴：

- 隐私必须覆盖存储、分享、恢复、搜索与诊断，而不是只说“本地运行”。
- 用户整理结果与 ML 派生结果都要有明确可迁移策略。
- 分享权限需要单独建模 GPS、EXIF、原图下载、上传与有效期。

不应复制：

- 在尚未验证本地核心体验时引入完整 E2EE sync。
- 低估密钥恢复、移动端电量/热量、跨设备索引一致性成本。

### 6.6 Apple/Google Photos 与 Lightroom：消费体验上限

Apple/Google Photos 的优势不是文件夹管理，而是人物、地点、自然语言、回忆、自动集合和低认知负担。Lightroom 的 Local 模式则证明本地目录可以无需导入或云同步直接编辑，non-destructive edit 可由参数与 XMP sidecar 承载（[Apple Photos User Guide](https://support.apple.com/guide/photos/welcome/mac)、[Google Photos Search](https://support.google.com/photos/answer/15235862)、[Lightroom Local Photos](https://helpx.adobe.com/lightroom/desktop/add-import-and-capture-photos/access-photos.html)）。

对本项目的启示：

- 先让用户看到“我关心的照片”，再要求整理。
- Search/People/Trips/Memories 是情感价值，不只是索引功能。
- Local 与 Cloud、Catalog 与 Original、Virtual Album 与 Physical Folder 必须用用户语言清楚区分。

### 6.7 竞争定位结论

Scrollery 最有机会的空位不是另一个 digiKam，也不是桌面版 Immich，而是：

> 一个真正面向普通人的 local-first 智能照片库：接受用户现有的杂乱目录，几乎无需配置即可获得时间线、人物、地点、搜索、回忆和整理建议，同时保持专业级的数据可解释性、文件可移植性与恢复能力。

它的壁垒不是某一个 AI model，而是三件事同时成立：

1. 大图库首次可用足够快、日常浏览足够顺。
2. 文件、metadata、身份、派生物与索引的关系长期可靠。
3. 自动化让用户感到被帮助，而不是失去控制。

## 7. 技术选型 Review

### 7.1 建议保留的选择

| 选择 | 结论 | 适用理由 | 约束 |
|---|---|---|---|
| Tauri 2 | 保留 | 桌面壳、系统 dialog、文件协议、安装包与较小体积适合本产品 | Shell 只承载 lifecycle/OS adapter，不成为业务层 |
| Rust | 保留 | 扫描、解码、hash、SQLite、并发与 worker supervisor 合适 | 避免业务全部堆入一个 crate/AppState |
| Vue 3 + TypeScript | 保留 | UI 迭代效率高，现有组件与测试资产可复用 | 建立 typed IPC 与 UI state/domain state 边界 |
| Pinia | 保留但收缩 | 适合 view/session state | 不把持久领域真相或跨窗口 job 真相放在 store |
| SQLite + WAL | 保留 | 单用户、local-first、可搬运单机 catalog 的正确默认 | 权威数据与可重建数据分域；备份、降级拒绝、integrity check 必须产品化 |
| Worker subprocess | 保留并强化 | 隔离 AI/不可信 decoder、控制崩溃与资源 | 进程隔离不是 sandbox；需权限、资源和网络边界 |
| ONNX Runtime | 保留为 adapter | 跨硬件推理生态成熟，可替换模型 | 模型身份/version/input fingerprint 必须入数据模型 |
| Vanilla CSS/token | 可保留 | 无需为换 UI 框架重写 | 先统一 primitives、a11y 与 visual regression |

### 7.2 建议停止或后置的选择

| 选择 | 结论 | 原因 |
|---|---|---|
| 过早 server 化 | MVP 不做 | 单机用户不应承担 Postgres/Redis/Docker；先守住本地体验 |
| 完整 E2EE sync | P2/P3 | 数据模型预留，实施必须独立立项与安全审计 |
| Plugin Marketplace 一级入口 | 后置 | 生态、签名、授权、sandbox、更新与客服成本远大于首发收益 |
| 文档/音频深功能 | 从主产品剥离 | JTBD 与照片找回/整理不一致，扩大 schema、UI 与测试矩阵 |
| 实验渲染参数用户化 | 移入 Labs | DOM/Canvas/GPU/batch 是内部策略，不是用户目标 |
| 以 path 作为实体中心 | 必须停止 | 无法可靠承载 rename/move/duplicate/版本/多设备 |

### 7.3 SQLite 是否够用

对单用户桌面 MVP，SQLite 完全够用，甚至比引入 client-server database 更合适。百万媒体并不自动要求 Postgres；真正的瓶颈通常是 query/index 设计、向量搜索、派生任务和文件 IO。

需要设定迁移门：

- 单 catalog 超过既定规模仍满足查询与写延迟 SLO；
- single writer backlog 可控，前台写不被后台派生饿死；
- 向量索引从 SQLite BLOB/full scan 分离；
- 多设备/多用户成为真实已验证需求时，再把 repository port 接到 server store。

不要为“未来也许要多端”提前牺牲桌面产品的简单性，但也不要让 domain model 依赖 Tauri command、绝对 path 或 SQLite row id。

### 7.4 当前架构规模信号

本轮静态统计约为：frontend 274 个相关文件/约 5.27 万行，`src-tauri/src` 115 个 Rust 文件/约 4.99 万行；前端有 179 个 IPC 常量，Tauri 注册约 181 个 command。`db/queries.rs` 约 9,395 行，`lib.rs` 约 965 行，`AppState` 同时持有 database、layout cache、AI worker、embedding cache、多套 token、catalog 与 resource limiter（`src-tauri/src/state.rs:23-183`）。

这些数字不是“代码太多”的罪证，但说明系统已越过可以继续靠全局 state、手工 command list 与单一 query 文件自然增长的阶段。greenfield 重构应优先改变边界和真相所有权，不是换语言或 UI framework。

## 8. Greenfield 目标领域架构

### 8.1 采用 modular monolith，而不是微服务

建议目标形态：一个桌面进程 + 多个受控 worker + 一个本地 catalog；代码内按 bounded context 切分，进程外只隔离高风险/高资源任务。

```text
Vue UI
  ↓ typed Application API
Tauri Shell / OS Adapters
  ↓
Application Services
  ├─ Library & Catalog
  ├─ Ingestion & Reconciliation
  ├─ Organization & User Metadata
  ├─ Discovery & Search
  ├─ Jobs & Artifacts
  ├─ Backup & Portability
  └─ Privacy & Diagnostics
  ↓ ports
SQLite Catalog | File System | Keyring | Worker Supervisor | Exporter
                                      ├─ Image/Video worker
                                      ├─ AI worker
                                      └─ Plugin decoder worker（后置）
```

### 8.2 bounded contexts

| Context | 权威真相 | 不应拥有 |
|---|---|---|
| Library/Catalog | Asset、File Location、Source、Volume、关系 | AI job 运行状态、UI scroll state |
| Ingestion/Reconciliation | scan cursor、filesystem observation、identity match decision | 用户 rating/album |
| Organization | Album、Favorite、Rating、Label、Tag、Person correction | 原始 EXIF 与 thumbnail path |
| Discovery/Search | query、Saved Search、可重建 index、result session | 用户资产身份 |
| Jobs/Artifacts | job、lease、progress、artifact manifest、failure | 业务 UI transient state |
| Backup/Portability | catalog snapshot、sidecar/import/export、restore plan | 原片备份已完成的虚假承诺 |
| Privacy/Diagnostics | consent、network ledger、redaction、diagnostic bundle | 业务数据的第二份影子真相 |
| Tauri Shell | window、dialog、protocol、lifecycle、OS capability | scan/AI/collection 业务状态机 |

### 8.3 API 边界

- Tauri command 只调用 Application Service，不直接拼业务 SQL 或操作任意 path。
- API 输入优先是 `asset_id/source_id/operation_id`，不是任意绝对路径。
- Rust DTO 与 TypeScript contract 由单一 schema/codegen 生成，错误码、版本与 deprecation 可测试。
- command 按领域拆 capability；gallery WebView 无权调用 plugin install、任意 shell open 或任意目录写。
- 长任务返回 `job_id`，进度通过统一 subscription/event stream，不为每条 pipeline 自造协议。

## 9. Greenfield 目标数据结构

### 9.1 先分清三类数据

| 数据域 | 示例 | 丢失后果 | 策略 |
|---|---|---|---|
| Original/User Asset | 原照片、原视频、sidecar | 不可替代 | 永不当缓存；默认不改；由用户备份 |
| User-authored Catalog Facts | album、rating、tag、favorite、person correction、caption | 用户劳动不可自动重建 | 强耐久、自动备份、可导出、迁移前 snapshot |
| Derived/Rebuildable | thumbnail、embedding、face detection、OCR、ANN index、layout cache | 可重算但有时间成本 | 可清理、可失效、记录 processor/version/input digest |

当前同一个 SQLite DB 同时保存上述后两类，还包含 AI BLOB、临时搜索结果与任务状态（`src-tauri/src/db/schema.rs:182-305,408-443`）。同库不是绝对错误，但备份、durability、migration 与清理必须按数据域区分；不能因“媒体可重扫”就误认为 catalog 可重建。

### 9.2 核心实体建议

#### `assets`

代表用户心智中的“这张照片/这段视频”，使用稳定 UUID/ULID，不使用 path 或 SQLite 自增 row id 作为跨导出身份。

建议字段：

- `asset_id`
- `media_kind`
- `capture_time_utc`、`capture_tz_offset`、`capture_time_source`
- `created_at`、`updated_at`
- `primary_file_instance_id`
- `stack_id`（可空）
- `tombstone_at`（catalog 级删除）

#### `file_instances`

代表一个具体文件副本或容器内成员。同一 Asset 可以有多个实例，例如 RAW+JPEG、重复副本、编辑导出、Live Photo pair。

建议字段：

- `file_instance_id`
- `asset_id`
- `source_id`、`volume_id`
- `normalized_relative_path`、`display_path`
- `filesystem_identity`（NTFS file id / inode 等，可空）
- `size_bytes`、`mtime_ns`
- `quick_fingerprint`、`content_hash`
- `availability`
- `first_seen_at`、`last_seen_at`、`missing_since`
- `format`、`role`（original/sidecar/paired_video/edited_copy）

约束：path 在同一 Source 内唯一，但绝不是 Asset identity；`content_hash` 也不应强行 UNIQUE，因为同一内容可以合法存在多份。

#### `sources` 与 `volumes`

- `source` 表示用户批准扫描或管理的逻辑来源，以及 metadata write policy、exclude rule、watch capability。
- `volume` 表示可插拔物理/网络卷的稳定身份、最近挂载点与在线状态。
- Source 通过 Volume + relative root 定位，不把盘符或绝对 path 当永久身份。

#### `asset_relations`

显式表达：

- `duplicate_of`
- `raw_jpeg_pair`
- `live_photo_pair`
- `burst_member`
- `edited_from`
- `stack_member`
- `sidecar_of`

不要继续把这些关系压进一个 `companion_of` 或 UI 临时分组。

#### `extracted_metadata`

按 namespace/provenance 保存 EXIF/IPTC/XMP/container probe 结果：

- source file instance；
- extractor 与 version；
- input digest；
- extracted at；
- normalized value 与必要的 raw value；
- conflict/provenance。

这能回答“拍摄时间来自 EXIF、sidecar 还是 filesystem”“新 sidecar 是否覆盖用户修改”。

#### `user_asset_metadata`

承载 rating、favorite、color label、caption、rotation correction 等用户事实；与 extracted metadata 分离。字段有明确 range/CHECK、更新时间与来源 device。若采用 event/journal，可从 operation log 恢复与同步。

#### `albums`、`album_members`、`tags`、`asset_tags`

- 系统 smart view 使用 stable code/query，不把“图片收藏”等中文展示名播种进 DB。当前 schema 在 migration 中写入中文系统名（`src-tauri/src/db/schema.rs:315-333`），会造成 i18n 与升级负担。
- 用户 Album 是虚拟多对多关系，删除默认 soft-delete，可在历史中恢复。
- Saved Search 保存 versioned query AST，不保存脆弱 SQL 字符串。

#### `persons`、`face_observations`、`face_assignments`

- `face_observation` 是模型派生物，带 model/version/input digest，可重建。
- `person` 与用户给出的名称、隐藏、合并、拒绝是权威用户事实。
- `face_assignment` 区分 algorithm suggestion、user confirmed、user rejected。
- 换模型不得覆盖用户确认事实，也不能把不同 embedding space 的 centroid 混用。

### 9.3 身份重链接流程

文件系统 reconciliation 建议按以下顺序：

1. 同 Source/path 命中：更新 observation。
2. OS filesystem identity 命中：识别 rename/move。
3. size + quick fingerprint 候选匹配。
4. full content hash 确认 exact duplicate 或移动。
5. EXIF、capture time、dimension、perceptual hash 只作为模糊候选，不自动合并用户事实。
6. 多候选或冲突进入“Review Changes”，由用户确认。

原则：宁可保留两个可合并候选，也不要错误地把两张不同照片合成一个 Asset。

### 9.4 数据约束与时间语义

当前 schema 中 rating 明示无值域约束，media type、availability、status、kind 等大量依赖自由 TEXT/INTEGER（`src-tauri/src/db/schema.rs:83,95-99,273-284,473-480,495-514,573-582`）。新 schema 应：

- 对 enum/range 加 CHECK；
- 所有时间统一单位与 UTC 语义，拍摄时间额外保留原时区/未知状态；
- 不用 `0` 同时表示“未知”“未处理”“没有”；
- 状态有 stable code 与未知值策略；
- JSON 扩展字段带 schema version；
- 跨表不变量通过事务 API、复合 FK 或必要 trigger 保证；
- 每个用户事实有 created/updated/provenance；每个派生事实有 input/processor fingerprint。

## 10. 统一 Job 与 Artifact 架构

### 10.1 为什么是核心域

大图库产品的大部分“功能”其实都是长任务：scan、reconcile、metadata、thumbnail、video probe、transcode、semantic embedding、face、OCR、duplicate、sidecar、export、backup。若它们各自管理 thread、token、status、retry 与 progress，用户和开发者都会面对多个互不一致的系统。

### 10.2 建议 `jobs` 结构

关键字段：

- `job_id`
- `job_type`
- `subject_type`、`subject_id`
- `input_digest`
- `processor_id`、`processor_version`、`profile_id`
- `state`: queued/running/paused/retry_wait/succeeded/failed/cancelled/superseded
- `priority_class`: interactive/visible/background/maintenance
- `attempts`、`max_attempts`、`next_retry_at`
- `lease_owner`、`lease_expires_at`、`generation`
- `progress_current`、`progress_total`、`progress_message_code`
- `stable_error_code`、`error_detail_redacted`
- `created_at`、`started_at`、`finished_at`、`updated_at`
- `operation_id`（与用户动作/批次关联）

不变量：

- `(job_type, subject_id, input_digest, processor_version)` 可幂等去重。
- worker 写结果必须 compare-and-set generation，旧 worker 的迟到结果不能覆盖新一轮。
- 进程崩溃后依 lease expiry 恢复，不把所有 running 无条件退回 pending。
- Cancel 是可观察状态，不只是一枚内存 token。

当前 exotic task 已有 attempts、next retry、lease、fingerprint 与 worker version，是值得保留的正确雏形（`src-tauri/src/db/schema.rs:494-522`）；目标是把这种纪律推广到全部 pipeline，而不是继续特判。

### 10.3 统一 `artifacts`

所有 thumbnail、preview、transcode、face crop、OCR、embedding/index segment 都通过 Artifact manifest 管理：

- `artifact_id`
- `asset_id/file_instance_id`
- `kind`、`profile`
- `source_digest`
- `generator_id/version`
- `storage_uri`
- `content_hash`、`size_bytes`
- `integrity_state`
- `created_at`、`last_accessed_at`

Job 成功时在同一 finalize transaction 写 artifact manifest 与 job result；业务表不再各自镜像 `thumb_path/payload_path/output_path` 多份真相。Catalog projection 可以缓存 primary artifact id，但必须可重建。

### 10.4 Writer Actor 与资源治理

SQLite 单 writer 适合本产品，但应该成为可观测的 Writer Actor/command queue：

- interactive 用户写优先；
- 后台批量写有 batch 上限和 cooperative yield；
- 事务保持短小；
- 暴露 queue depth、wait time、transaction duration、busy/retry 指标；
- 各 bounded context 通过 port 提交 command，不直接长期持锁；
- cache invalidation 由 revision/domain event 驱动，不依靠所有写路径手工 bump 一枚全局版本。

### 10.5 Single Instance 决策

桌面首版建议严格 single-instance：第二实例把 open intent 转发给主实例并聚焦窗口。当前只有 exotic 显式考虑跨进程 lease，而其他 pipeline 与全局临时状态没有统一多实例语义。与其给每个子系统补跨实例竞态，不如先在产品层做真实单实例；未来 server/multi-client 再统一采用 lease/fencing。

## 11. 关键技术与数据风险清单

### 11.1 P0：旧二进制必须拒绝写入更新 schema

当前 migration 对 `version < step` 执行升级；如果数据库版本高于当前 binary 支持版本，最终只 warning 并返回成功（`src-tauri/src/db/migration.rs:154-177`）。用户回滚或误装旧版本后，旧代码可能继续写新 schema。

目标：

- `db_version > binary_max_version` 必须 fail closed；
- 只允许进入只读诊断/导出，或引导安装兼容版本；
- migration 前自动 snapshot；
- migration history 记录 id、checksum、started/finished/dirty；
- CI 用历史 v1..vN fixture 升到 current，并测试 newer DB 拒绝路径。

### 11.2 P0：catalog backup/restore 与 integrity 是产品能力

当前连接层有 WAL checkpoint，但不等于备份；启动迁移前也没有形成用户可见 snapshot/restore 流程（`src-tauri/src/db/connection.rs:59-93`；`src-tauri/src/lib.rs:197-219`）。

目标：

- 自动轮换 backup，按版本和时间可见；
- 手动 export/import catalog bundle；
- restore 前创建 restore point；
- restore 后运行 schema compatibility、foreign key、integrity 与文件可达性检查；
- 数据库损坏时进入只读 recovery，不自动清库；
- 清楚说明 catalog backup 不含 original。

### 11.3 P0：DB 与文件系统之间需要 Saga/Operation Journal

真实文件 move/copy/trash 与 SQL 无法共享 ACID transaction。当前批量文件操作的实现注释明确记录前 N-1 项可能已经生效、第 N 项失败即返回的部分成功语义（`src-tauri/src/ipc/file_ops_commands.rs:20-26,99-106`）。正确目标不是持有一个超大 DB transaction，而是：

1. 写 `operation_intent` 与逐项计划；
2. 执行 filesystem action；
3. 记录每项结果并提交 catalog；
4. finalize operation；
5. 启动 reconciler 处理 crash 后的中间态；
6. 用户看到逐项成功/失败、重试和 undo 能力。

所有操作带 stable `operation_id`/idempotency key，重复调用不会再次移动或复制。

### 11.4 P1：语义搜索不能长期全量常驻、全量排序

当前 `ai_search_results` 以 file id 为唯一键，没有 search session/query id（`src-tauri/src/db/schema.rs:252-260`）；架构审视还发现当前路径会将 embedding 全量载入内存并全量打分/排序。百万 × 512 的 f16 约 1GB，仅缓存本身就会排除低内存设备。

建议分级：

- 小/中图库先用 SIMD/brute-force + fixed-size Top-K heap，避免 O(N log N) 全排序。
- 搜索结果直接返回或用 `search_session_id` 隔离，不用全局单例持久表互相覆盖。
- 达到规模/延迟阈值后采用可重建的 ANN/HNSW index，model/version 分区、支持增量更新。
- 保留 brute-force 作为精排、召回质量评估和故障降级。
- 用户看到质量/速度 preset 和索引覆盖度，不看到内部模型表。

### 11.5 P1：IPC 与 capability 必须最小权限

当前 capability 包含 shell default/allow-open，asset protocol 允许 `$APPDATA/**`，启动又为扫描根扩展 scope（`src-tauri/capabilities/default.json:5-19`；`src-tauri/tauri.conf.json:43-48`；`src-tauri/src/lib.rs:304-322`）。`create_physical_folder` 还会在确认路径是否属于既有扫描根之前先执行 `create_dir_all`（`src-tauri/src/ipc/file_ops_commands.rs:57-95`）。CSP 是良好基础，但 WebView compromised 后，过宽的 host capability 仍是风险。

目标：

- UI 只传 `asset_id/root_id`，host 从批准 source 解析 canonical path；
- 所有 path canonicalize 后验证在批准根或 staging/output grant 内；
- 媒体通过窄 custom protocol 按 asset id 提供，不 blanket expose 整个 scan root；
- shell/open/install/credential 分 capability 和窗口；
- plugin store/diagnostics 不与 gallery 共享默认权限。

### 11.6 P1：Worker subprocess 不等于安全 sandbox

签名、hash、timeout、子进程与 crash isolation 都值得保留，但恶意或被利用的 decoder 仍可能继承当前用户权限、环境与网络。

目标 sandbox：

- Windows restricted token/AppContainer/Job Object；
- macOS sandbox profile；
- Linux namespaces/seccomp/bubblewrap；
- 清理 environment，只授予单个 input handle 与 staging output；
- 默认禁网；
- CPU、RAM、wall time、output size、file count 硬限制；
- crash/timeout 熔断与用户可见 quarantine。

若做不到，就必须诚实定义 trust model：官方签名证明来源和完整性，不证明 decoder 没有漏洞。

### 11.7 P1：多份派生真相要收敛

当前 thumbnail、derivation、exotic output 分散在主表状态列和多张 task 表中，已有启动自愈/对账的复杂度信号。不要继续修每一处镜像写入；应以统一 Artifact manifest 为真相，业务 projection 只是可重建缓存。

### 11.8 P1：Durability policy 要区分用户事实与派生写

全连接统一 `synchronous=NORMAL` 对高吞吐派生合理，但 rating、person name、album membership 与 backup metadata 的价值不同。可选择：

- 权威用户事务使用更强 durability/显式 checkpoint；
- 派生写保持 NORMAL 和批量吞吐；
- 或拆 catalog DB 与 derived DB，分别设置恢复策略。

最终取舍要通过断电/kill test 和磁盘基准，不凭直觉。

### 11.9 P2：开源/闭源投影拓扑增加供应链风险

当前私有超集通过标记块与 Copybara 剥离 pro member，再用 OSS gate 做禁词、目录、lock 与编译检查（`Cargo.toml:12-36`；`.github/workflows/oss-gate.yml:46-129`）。现有 gate 很认真，但模式本身让一次漏规则同时成为源码泄漏、密钥泄漏与公开构建风险。

greenfield 优先顺序：

1. 公开 core 成为 canonical；
2. 私有产品依赖固定版本/commit 的公开 core；
3. 私有 overlay 独立 package/repo；
4. 若保留投影，必须有 hermetic projection fixture、secret scan、SBOM、签名与 provenance/attestation。

## 12. 安全、隐私与可观测性目标

### 12.1 Threat Model 最低覆盖

- 恶意/畸形媒体触发 decoder 漏洞或资源炸弹；
- 被篡改的模型、插件、registry 与更新包；
- WebView XSS 后滥用 host command；
- 日志/诊断包泄露路径、GPS、搜索词、token；
- 本机其他用户读取 catalog 中人脸与位置；
- 远程校对/WebDAV/未来 sync 上传超出用户预期；
- 数据库损坏、旧版本误写、新 migration 中断；
- 恶意 ZIP/EPUB、sidecar、超长 path、symlink/junction escape；
- 双实例、进程 crash、断电导致 DB/FS 分叉。

### 12.2 隐私设计

- face、semantic AI、remote AI 分别 consent，不能合并成一个“启用 AI”。
- 明确 retention 与删除：删除 model 不等于删除 embedding，必须给一键清除派生。
- 敏感 catalog 可选 at-rest encryption；密钥只进 keyring，不明文落 DB。
- 诊断包默认 hash/截断 path，移除 GPS、query text、credential、remote URL secrets。
- 任何 telemetry/crash upload 均 opt-in，上传前可预览；若产品不采集，也应明确承诺。
- Privacy Center 展示最近 network activity 与目的。

### 12.3 本地可观测性

不上传 telemetry 也必须可诊断。建议保留本地结构化 metrics ring buffer：

- startup to interactive；
- first thumbnail latency；
- scan files/s 与 error categories；
- writer queue depth/wait；
- job queue depth、retry、crash、timeout；
- thumbnail decode/encode latency by backend/format；
- search p50/p95、index coverage、recall evaluation sample；
- cache hit/eviction/churn；
- dropped frame/thumbnail settle time；
- backup age、last integrity check。

日志使用 rotation、size/retention cap、structured span 与 redaction；`run_id/job_id/operation_id` 可关联，但 item/path 默认用 hash，避免直接泄露用户目录。

## 13. 测试、CI、发布与跨平台 Review

### 13.1 值得保留的工程基线

- Rust workspace 有 fmt、check、clippy、test 硬门（`.github/workflows/ci.yml:58-71`）。
- Frontend 有 lint、typecheck、Vitest、production build 与多类静态治理门（`.github/workflows/ci.yml:153-224`）。
- Linux 有独立 compile/test 面，Windows tag/manual 有 build artifact startup smoke（`.github/workflows/ci.yml:101-137,226-271`）。
- OSS projection 有源码剥离、lock、workspace、gitleaks 与实际 compile/test 多层验证（`.github/workflows/oss-gate.yml:46-145`）。
- Release 会检查 tag/version、CHANGELOG、测试、安装包与 SHA-256（`.github/workflows/release.yml:62-123`）。

这些门说明项目不是“只写功能不验证”的原型。但它们主要证明代码能编译、单元逻辑通过和包能启动，不证明真实用户旅程、数据恢复与跨平台体验成立。

### 13.2 主要缺口

#### 真实 E2E

- 导入—首图—搜索—打开—收藏—删除—恢复—导出全旅程。
- Tauri 真进程、真实 asset protocol、真实 filesystem 与数据库，不只 browser fixture。
- 窄窗、高 DPI、多显示器、窗口恢复、拖放、系统 dialog。

#### 历史与故障

- v1..vN 每个历史 fixture 升级到 current。
- newer DB 被旧 binary 拒绝。
- migration 中途 kill、磁盘满、权限丢失、WAL/DB 损坏。
- move/copy/delete/export 每个阶段 fault injection，启动 reconciler 可恢复。
- 双实例、worker crash、timeout、GPU OOM、模型损坏。

#### 规模与性能

- 10k/100k/1M 三档合成库与至少一个脱敏真实库。
- 首图、scroll、filter、search、reconcile、backup、restore、full rebuild 基准。
- 性能 regression budget 挂 CI/nightly，而不是一次性人工描述。

#### 安全

- 恶意/畸形 image/video/EPUB/ZIP/sidecar corpus。
- parser/IPC fuzz、path traversal、symlink/junction、ZIP bomb、超大尺寸。
- dependency audit/deny、license、SBOM、artifact signing、provenance。
- capability negative tests：未批准 path、跨 root、任意 shell、token redaction。

#### 可访问性与视觉

- axe/static scan 只是起点；需 keyboard-only 与 screen reader journey。
- Windows/macOS/Linux 各至少一个真实 runtime visual baseline。
- 六主题不是优先级；先保证默认 light/dark、200% scaling、high contrast 与 reduced motion。

### 13.3 公开协作与 CI 事实需要一致

CONTRIBUTING 声称 CI 在每个 PR 跑 full suite（`CONTRIBUTING.md:28-33`），但私有 CI job 以私有仓名 guard，自托管 runner 也明确不服务公开仓；OSS gate 只在公开 `sync-staging` 运行（`.github/workflows/ci.yml:23-34,107-143`；`.github/workflows/oss-gate.yml:23-32`）。

如果希望外部贡献者参与 core，公开 PR 应至少有托管 runner 上的 fmt/check/test/lint/typecheck 与安全的 fixture suite。否则文档应诚实说明公开 PR 的验证流程和维护者会在哪个受控环境复跑。

### 13.4 平台支持矩阵

每个发布版本应公开矩阵：

| 能力 | Windows | macOS | Linux | 验收方式 |
|---|---|---|---|---|
| 安装/升级/卸载 | 明确版本 | 明确版本 | 明确发行版范围 | 真实安装包 E2E |
| Image formats | 格式与 backend | 格式与 backend | 格式与 backend | golden corpus |
| Video playback/thumbnail | backend/codec | backend/codec | backend/codec | codec matrix |
| Live Photo | 支持/限制 | 支持/限制 | 支持/限制 | paired corpus |
| GPU/AI | provider/fallback | provider/fallback | provider/fallback | runtime benchmark |
| Removable/NAS | 卷身份/重连 | 卷身份/重连 | mount/permission | unplug/remount test |
| File ops/Trash | OS semantics | OS semantics | desktop/env semantics | fault injection |
| Keyring | Credential Manager | Keychain | Secret Service | roundtrip test |
| Accessibility | Narrator | VoiceOver | Orca | journey test |

在三平台矩阵达到前，产品文案采用 Windows-first + 其他平台 preview，比笼统 cross-platform 更可信。

## 14. 目标产品信息架构与核心旅程

### 14.1 一级信息架构

```text
Photos
  All Photos
  Recent
  Favorites
  Memories
Discover
  People
  Places
  Duplicates & Similar
Organize
  Albums
  Saved Searches
Folders
Trash

全局：Search | Add Source | Import | Export | Job Center
Manage：Sources | Storage | Backup | Privacy | Models | Diagnostics | Labs
```

原则：主导航只表达用户对象与目的；scan、AI、thumbnail、plugin、render backend 是管理/任务/实现层，不是 Library 内容。

### 14.2 首次使用

**目标：不是“走完 wizard”，而是“看到照片并信任产品”。**

1. 欢迎：一句价值与“默认不移动、不修改、不上传”。
2. Add Source：选择多个目录/移动盘；解释 Index 与 Managed Copy 的用户含义，默认 Index。
3. Preflight：文件估算、格式覆盖、重叠目录、空间、权限、离线/网络来源提示。
4. Progressive Index：发现首批 asset 后立即进入 timeline，后台继续 metadata/thumbnail。
5. AI Choice：在基础图库已经可用后单独询问 semantic/face，分别 opt-in。
6. First Success：引导按时间/人物/自然语言找一张旧照片。

失败状态必须区分 Permission、Offline、Unsupported、Corrupt、Disk Full、Cancelled，并给出下一步；不能把错误渲染成空图库。

### 14.3 搜索

- 单框默认混合搜索；AI 未就绪不阻塞 metadata/filename 结果。
- 实体 suggestion：person、place、date、album、camera。
- 结果显示匹配原因与索引覆盖度。
- Advanced Filter 采用 chips/query builder，不要求用户记语法。
- Zero Result 提供清 filter、改写、等待索引、降低 semantic threshold、搜索文件名五类恢复。
- Saved Search 可命名、固定在侧栏、导出 query definition。

### 14.4 整理

在视觉与文案上严格区分：

- **Catalog-only**：Favorite、Rating、Label、Tag、Album、Person correction。
- **Original-affecting**：Move、Rename、Delete to OS Trash、Embedded metadata writeback。
- **New copy**：Copy、Export、Convert、Resize。

选择条按这三类分组。会写磁盘的动作显示影响范围、目标、冲突、剩余空间与 undo 能力；bulk operation 进入 Job Center。

### 14.5 Trash 与 Activity

- Trash 允许 Restore、Restore to、Permanent Delete、Empty Trash。
- 原目录离线/不存在时提示新的恢复目标。
- 默认保留期可配置，清空前给规模与不可逆说明。
- Activity 记录 catalog action 与 filesystem operation；能查看逐项失败、重试、undo 与诊断 id。

### 14.6 Export 与分享

Export Wizard：

1. Original / Copy / Contact Sheet / Metadata Manifest。
2. Format、quality、dimension、color profile。
3. Preserve/strip EXIF、GPS、caption、rating。
4. Naming template 与 folder structure。
5. Conflict policy：skip/rename/replace with backup。
6. 目标权限与空间 preflight。
7. Job Center 进度、逐项失败和可重跑 manifest。

首版分享可以是系统 share sheet 或导出目录，不必立即做 public link/server；但“复制位图”不能代替交付原文件。

### 14.7 Backup Health

页面同时展示三件不同的事：

- Original：产品是否知道用户有几个副本、最近是否离线；不假装已替用户备份。
- Catalog：最近自动备份、保存位置、版本、验证结果、恢复按钮。
- Derived Cache：大小、可清理、重建预计成本；明确无需备份。

## 15. 推荐实施路线：先产品验证，再架构收敛

以下是依赖顺序，不是未经团队估算的日历承诺。

### Stage 0：立项验证与停止线

产出：

- 10 至 15 名目标用户访谈，覆盖家庭档案、摄影爱好者、移动盘/NAS 用户。
- 5 个 find tasks 与 3 个整理/恢复 tasks 的竞品对比测试。
- Windows-first 或 cross-platform 的明确决策。
- Free/Pro capability matrix 与“永不锁用户数据”原则。
- 冻结文档、音频、Plugin Marketplace、H-Lab 新功能扩张。

退出门：至少 8/10 受访者认为“零迁移 + 本地智能找图”是现实痛点，并愿意用真实图库试用；否则重新定位，不进入大规模重构。

### Stage 1：数据与恢复地基

产出：

- Asset/File Instance/Source/Volume v2 model。
- 统一 Job/Artifact、single-instance、Writer Actor。
- schema downgrade fail-closed、migration snapshot、integrity check。
- Operation Journal + filesystem reconciler。
- Catalog backup/export/restore。
- typed IPC 与 path/capability 收窄。

退出门：历史 DB 升级矩阵、newer DB 拒绝、100 轮 fault injection、rename/move/replug 身份保持、catalog restore 全部通过。

### Stage 2：可爱的 local-first 核心

产出：

- 重做 onboarding，首图即 activation。
- Timeline/Folder/Album/Favorite/Trash。
- 单框 metadata/filename 搜索。
- Viewer 与完整 Export。
- Job Center、Activity、Backup Health、Privacy Center。
- 默认 light/dark 与正式 accessibility path。

退出门：目标用户完成导入、找图、整理、恢复、导出五旅程；没有依赖工程人员解释的阻断。

### Stage 3：智能差异化

产出：

- Semantic Search、People、可纠正 face workflow。
- Saved Search、Duplicate/Similar、Places、On This Day/Moments。
- AI coverage、model/version、删除/重建与资源 preset。
- ANN 达阈值后引入，不先行复杂化。

退出门：与 metadata-only baseline 相比，用户找图成功率和时间有统计显著改善；AI 误识别有清晰纠正路径且不破坏用户事实。

### Stage 4：扩展生态（需重新立项）

候选：

- Mobile companion 与 LAN access；
- optional E2EE sync/backup；
- shared/collect album；
- exotic format plugins；
- 专业 sidecar/writeback/batch；
- 文档或音频独立产品线。

进入条件：Stage 2/3 的 activation、trust、retention、restore 指标连续两个发布周期达标，且扩展方向有独立用户研究与资源预算。

### 15.1 对当前项目的迁移方式

不建议在 v1-v18 schema 上继续堆更多互相纠缠的列，也不建议一次性 big-bang rewrite。推荐 Strangler 路径：

1. 冻结旧 schema 业务扩张，只修数据安全问题。
2. 建 Catalog v2 与 typed Application Service，先并行只读 index。
3. 由 original files 重建 derived facts；从旧 DB 只迁 user-authored facts。
4. 生成迁移报告：matched/ambiguous/orphan/failed，用户可预览。
5. 双读验证一段时间，但用户写只进入单一权威，避免长期 dual-write。
6. 完成 restore drill 后切换，旧 catalog 只读保留一个版本周期。

## 16. 优先级总表

### P0：立项/发布阻断

| 项目 | 用户风险 | 架构动作 | 验证信号 |
|---|---|---|---|
| 收缩照片/视频主产品 | 用户不懂产品为何存在 | 冻结非核心 bounded context 扩张 | 导航与产品页一句话一致 |
| 首启扫描闭环 | 完成引导仍空库 | activation 以首图为准 | 空图库误完成率为 0 |
| Stable Asset Identity | move/rename 后整理成果丢失 | Asset/File Instance 分层 | 跨盘/外部改名保持事实 |
| Catalog backup/restore | 用户劳动不可恢复 | snapshot/export/restore/integrity | 恢复演练全通过 |
| Downgrade fail-closed | 旧版损坏新库 | schema compatibility gate | newer DB 只读拒绝写 |
| Operation Journal | DB/FS 部分成功分叉 | saga + reconciler + per-item result | kill 后自动对账 |
| 统一 Job/Artifact | 任务状态混乱、不可恢复 | lease/fencing/idempotency | crash/重试不重复产物 |
| Privacy/Consent | 用户不敢启用人脸/联网 | Privacy Center/network ledger | 用户能准确回答数据去向 |
| Trash/Export 闭环 | 误删难恢复、找到无法交付 | 正式 Trash + Export Job | 24h 后可恢复、导出有报告 |
| 平台承诺真实 | cross-platform 预期落差 | Windows-first 或真实矩阵 | 每个宣称平台 runtime E2E |

### P1：形成明显吸引力

- 单框搜索与 Saved Search。
- People/semantic 的覆盖度、可解释和可纠正。
- Duplicate/Similar、Places、On This Day。
- Job Center、Activity、Backup Health。
- typed IPC、Writer Actor、ANN 分级、worker sandbox。
- 真实 Tauri E2E、故障注入、大库 benchmark、accessibility journey。

### P2：验证后扩展

- Mobile/LAN/server。
- E2EE sync/backup。
- Shared/Collect Album。
- Exotic Plugin Marketplace。
- 专业 metadata writeback/RAW editing。
- 文档/音频独立业务线。

### 明确停止投入

- 在普通 Settings 继续增加 renderer/GPU/batch 微参数。
- 在 path-based `media_items` 上继续挂更多用户事实。
- 为每条 pipeline 新建一套 status/token/retry/progress。
- 以短 toast 作为持久 undo/history。
- 在没有恢复演练前继续增加不可重建 catalog 数据。
- 用“测试数量”替代真实用户旅程与故障恢复证据。

## 17. 成功指标与决策门

以下数字是首轮可测的 D 级目标假设，应由参考硬件、真实图库和用户研究校准，不是本轮已验证成绩。

### 17.1 Activation

- 选择本地 SSD source 后，首张可浏览 thumbnail：p50 ≤ 5 秒，p95 ≤ 10 秒。
- onboarding 完成后错误空图库率：0%。
- 首次 15 分钟内完成一次“找回一年以上旧照片”的用户比例：≥ 70%。

### 17.2 Findability

- 5 个预定义找图任务，目标用户 30 秒内成功率：≥ 80%。
- metadata search p95：100k 库 ≤ 200ms，1M 库 ≤ 500ms。
- semantic search p95：参考硬件上 100k ≤ 1 秒，1M ≤ 2 秒。
- AI index coverage 始终可见；未满 100% 时不阻塞普通结果。

### 17.3 Trust 与 Safety

- 80% 目标用户能准确回答：原片是否上传/移动、AI 数据在哪里、删除后去哪、catalog 如何备份。
- 删除 24 小时后仍可从 Trash 恢复；原路径不可用时能 Restore to。
- Catalog backup 恢复后，Album/Rating/Tag/Favorite/Person correction 一致率 100%。
- 外部 rename/move/replug 后，已确认 user facts 误丢失率 0；模糊候选不自动误合并。

### 17.4 Performance 与资源

- 已有 catalog 冷启动到可交互 p95 ≤ 3 秒（参考 SSD/硬件）。
- 30 秒连续滚动中 >50ms long frame 比例 <1%；停止滚动后可见清晰图 settle p95 ≤ 1 秒。
- 前台交互发生后后台重任务在 250ms 内让出资源；取消请求在 2 秒内进入可观察 cancelled/finishing 状态。
- 低内存 preset 不允许固定消耗约 1GB embedding cache；预算按设备能力有硬上限。

### 17.5 Reliability

- 所有历史 schema fixture 升级通过；newer schema 拒绝写通过。
- 关键 operation 每个阶段 100 轮 kill/fault injection 后无静默丢失，reconciler 给出确定结果。
- backup/restore 每个 release candidate 自动演练。
- 恶意 media corpus 不导致 host 进程退出或越权访问。

### 17.6 Retention

- 4 周内至少再次执行 search/rediscovery/curation/export 任一核心行为，而不是只打开设置。
- On This Day、Duplicate Review、Saved Search、Backup Health 分别测独立回访贡献。
- 不以扫描文件数、AI 处理数或设置调整数当作用户价值指标。

### 17.7 Kill Criteria

出现以下任一情况，应暂停架构扩张并重新评估：

- 目标用户并不把本地隐私/零迁移视为重要，且更期待手机云备份。
- 真实图库中找图成功率不显著优于系统文件搜索/现有工具。
- 百万级性能优势必须牺牲 accessibility、数据可靠性或普通硬件可用性。
- 用户不愿为任何差异化能力付费，且开源维护成本不可持续。
- 文档/音频/插件需求来自内部技术兴趣，而非独立用户证据。

## 18. 仍需产品负责人拍板的问题

1. 首发是 Windows-first，还是承诺三平台功能等价？
2. 核心用户是家庭照片维护者，还是专业摄影资产管理者？两者默认 IA 与 metadata policy 不同。
3. 默认完全只读，还是推荐 XMP sidecar？Embedded writeback 是否进入首版？
4. Free/Pro 的用户可懂边界是什么？哪些能力永不收费以避免数据锁定？
5. Original backup 只做健康提示/第三方集成，还是产品未来承担备份？
6. AI 默认 opt-in 还是在本地且明确说明后默认启用？Face 必须独立 consent。
7. 是否接受 single-instance 作为桌面首版契约？
8. 文档、音频与 Plugin Store 是永久剥离、独立插件，还是未来独立产品线？

上述问题不应由当前代码惯性代替决策。

## 19. 最终建议

### 应继续投资的核心

- local-first、零迁移接管既有目录；
- 百万级渐进浏览与资源调度；
- metadata + semantic + people 的统一找图体验；
- removable/NAS source 的离线与重链接；
- user facts 可备份、可移植、可恢复；
- 自动化可解释、可纠正、可关闭。

### 应立即降级的方向

- 通用文档阅读/编辑/AI 校对；
- 音频歌词与音乐库心智；
- Plugin Marketplace 一级导航；
- 面向普通用户的 renderer/GPU/batch 选项；
- 在核心恢复能力之前继续扩商业渠道和 exotic feature surface。

### 架构总原则

> 不换掉适合的技术栈，换掉错误的真相边界：Asset 不等于 Path，User Fact 不等于 Derived Cache，Job 不等于 Thread Token，Signed Worker 不等于 Sandbox，Index 不等于 Backup，Cross-platform 不等于 Compile Success。

- 如果只能优先做一件产品工作：重做“添加目录到首图出现”的首次旅程。
- 如果只能优先做一件数据工作：建立稳定 Asset/File Instance 身份。
- 如果只能优先做一件可靠性工作：让 catalog 自动备份、可恢复，并让旧 binary 拒绝新 schema。
- 如果只能优先做一件架构工作：统一 Job/Artifact 与 Operation Journal。
- 如果只能停止一件事：停止在核心闭环未成立前继续扩展媒体类型和插件生态。

## 附录 A：关键仓库证据索引

| 主题 | 证据 |
|---|---|
| 当前产品承诺 | `README.md:3-21` |
| 技术栈 | `README.md:9-12`、`package.json:24-54`、`src-tauri/Cargo.toml:16-185` |
| 产品路由与范围 | `src/router/index.ts:8-89` |
| 首次引导 | `src/components/common/OnboardingWizard.vue:38-119,178-216` |
| 主导航 | `src/components/sidebar/sections/LibrarySection.vue:24-58,81-109` |
| 用户文案/设置表面 | `src/i18n/locales/zh-CN.ts:2-203,291-375` |
| 当前核心媒体结构 | `src-tauri/src/db/schema.rs:40-215` |
| AI/派生/文档/人脸/插件/卷 | `src-tauri/src/db/schema.rs:217-615` |
| migration 策略 | `src-tauri/src/db/migration.rs:22-177` |
| SQLite/WAL | `src-tauri/src/db/connection.rs:24-140` |
| 全局运行时状态 | `src-tauri/src/state.rs:23-183` |
| IPC 能力面 | `src/constants/ipc.ts:5-230`、`src-tauri/src/lib.rs:700-926` |
| Tauri CSP/capability | `src-tauri/tauri.conf.json:43-48`、`src-tauri/capabilities/default.json:5-19` |
| CI | `.github/workflows/ci.yml:31-271` |
| OSS gate | `.github/workflows/oss-gate.yml:23-149` |
| Release | `.github/workflows/release.yml:35-123` |
| 开源/商业边界 | `CONTRIBUTING.md:5-18`、`CHANGELOG.md:19-28` |

## 附录 B：外部官方资料

- [digiKam About](https://www.digikam.org/about/)
- [digiKam Interface Layout](https://docs.digikam.org/en/main_window/interface_layout.html)
- [digiKam Database](https://docs.digikam.org/en/getting_started/database_intro.html)
- [Immich Architecture](https://docs.immich.app/developer/architecture/)
- [Immich Mobile Backup](https://docs.immich.app/features/mobile-backup/)
- [Immich Search](https://docs.immich.app/features/searching/)
- [Immich Backup and Restore](https://docs.immich.app/administration/backup-and-restore/)
- [Immich External Libraries](https://docs.immich.app/features/libraries/)
- [PhotoPrism Library](https://docs.photoprism.app/user-guide/library/)
- [PhotoPrism Features](https://docs.photoprism.app/)
- [PhotoPrism Mobile Devices](https://docs.photoprism.app/user-guide/sync/mobile-devices/)
- [Ente Architecture](https://ente.com/architecture/)
- [Ente Machine Learning](https://ente.io/blog/machine-learning)
- [Ente Sharing FAQ](https://help.ente.io/photos/faq/sharing-and-collaboration)
- [Apple Photos User Guide](https://support.apple.com/guide/photos/welcome/mac)
- [Apple Photos Search](https://support.apple.com/guide/photos/search-for-photos-and-videos-pht64de33e5a/mac)
- [Google Photos Search](https://support.google.com/photos/answer/15235862)
- [Google Photos Partner Sharing](https://support.google.com/photos/answer/7378858)
- [Lightroom Local Photos](https://helpx.adobe.com/lightroom/desktop/add-import-and-capture-photos/access-photos.html)
