---
id: 2026-07-13-新立项视角产品与架构综合Review-chatgpt-5.6-sol
status: snapshot
type: review
line: 多专家新立项产品与架构综合评审
created: 2026-07-13
last-verified: 2026-07-13
---

# Scrollery 新立项视角产品与架构综合 Review

> - 评审角色：新产品立项产品经理 + greenfield 全栈架构师
> - 综合日期：2026-07-13
> - 输入一：[另一 AI 专家 Review（已归档）](../archive/2026-07-13-新立项视角产品与架构全面Review.md)
> - 输入二：[ChatGPT 原 Review（已归档）](../archive/2026-07-13-新立项视角产品与架构全面Review-chatgpt-5.6-sol.md)
> - 评审边界：现有仓库用于证明现状，不把项目内既定路线、已完成标记或沉没成本当作产品正确性的前提；不评价具体函数写法
> - 本文定位：在两份独立 Review 之上完成证据复核、冲突裁决和优先级重排；不是摘要，也不是简单取并集

## 0. 最终裁决

### 0.1 立项结论：有条件 GO，停止功能扩张，进入“信任 × 市场”双轨验证

Scrollery 值得继续立项，但当前不能按“再补一些功能即可发布”的方式推进。

成立的产品楔子是：

> 面向拥有多年本地照片目录、移动硬盘或 NAS 挂载目录的个人与家庭，在不搬动、不修改、默认不上传原片的前提下，几秒开始浏览，并通过时间、人物、地点、metadata 与自然语言找回旧照片；用户的整理成果可备份、可恢复、可带走。

这个方向同时获得两份 Review 的支持，且直接竞品 Lap 的出现说明“local-first + 大图库 + 本地 AI”不是只有本项目在下注。现有 Tauri 2、Rust、Vue、SQLite、worker subprocess 与大库布局/调度投入也与目标场景匹配，不需要换技术栈。

但项目还不是可正式托付照片的消费产品。当前主要矛盾不是功能少，而是：

1. 产品范围扩散到照片、视频、音频、文档、AI 校对、插件市场与网络能力，主 JTBD 被稀释。
2. 首次体验没有以“第一批照片出现并成功找回”为完成条件。
3. 稳定资产身份、catalog backup/restore、DB/FS 操作恢复、schema downgrade 防护仍不完整。
4. 找到照片之后的 export、迁移与策展事实出口不足。
5. “cross-platform”“100k–1M 流畅”等公开承诺尚缺对应 runtime/E2E 与真实硬件证据。
6. 商业化执行、签名分发和反馈闭环落后于功能建设，但商业化不能先于不可逆数据安全门。

因此采用两条并行且互不替代的 release gate：

| Gate | 必须证明什么 | 不允许用什么替代 |
|------|----------------|------------------|
| Trust Gate | 原片不被意外改变；用户事实不会因移动、升级、损坏而静默丢失；可恢复、可导出、可解释联网 | 新功能数量、单元测试数量、代码完成标记 |
| Market Gate | 用户能快速理解价值、成功激活、愿意持续使用；分发可信；有明确 Free/Pro 或 supporter 验证 | GitHub stars、竞品存在、内部偏好、支付页面本身 |

任何正式收费发行都必须同时通过两门；alpha 可以先验证市场，但不得让真实用户把不可恢复的整理劳动托付给尚未通过 Trust Gate 的 catalog。

### 0.2 一句话判断

> Scrollery 已经具备“能做出好产品”的技术骨架，却仍是一套比用户闭环更成熟的工程原型；下一阶段的胜负不在继续扩功能，而在用可靠的数据地基把“大图库找回”做成第一次就懂、长期敢用、随时能走的产品。

### 0.3 综合评分

评分是“作为新立项产品是否成立”，不是代码质量评分。

| 维度 | 综合评分 | 综合判断 |
|------|---------:|----------|
| 用户痛点与定位楔子 | 4/5 | 本地大图库、隐私、找回与零迁移有真实价值；仍需目标用户验证 |
| 竞争窗口 | 3/5 | Lap 证明方向有人关注，也增加发布时间压力；尚不能据此证明付费市场 |
| 产品边界 | 1/5 | 多媒体与工具线并行，照片核心叙事被冲淡 |
| 首次激活 | 1/5 | wizard 完成不等于扫描启动、首图出现或找回成功 |
| 浏览与规模能力 | 4/5 | 工程主线强；百万级 claim、WebView2 内存与坐标上限仍需真机证明 |
| 搜索与发现 | 3/5 | AI 差异化强；基础 filename 搜索仍是 LIKE，用户心智也被模式分裂 |
| 整理、编辑与交付 | 2/5 | catalog 操作较多；export、迁移、持久旋转/裁剪等消费闭环不足 |
| 数据可靠性 | 2/5 | SQLite/migration 基础好；Asset Identity、downgrade、backup、operation journal 不完整 |
| 隐私与安全 | 2/5 | 本地与 worker 基础正确；consent、network ledger、sandbox、最小 capability 未产品化 |
| 架构可演进性 | 3/5 | 技术选型正确；全局状态、IPC、分裂 jobs/artifacts 与多业务域增加复杂度 |
| 测试与交付 | 3/5 | 静态/单元门广；真实 Tauri E2E、故障注入、macOS runtime、签名分发不足 |
| 商业清晰度 | 1/5 | 能力很多，但首发包装、Free/Pro、价格与转化实验尚未闭环 |

## 1. 综合方法与证据纪律

### 1.1 两份报告如何合并

本文把输入结论分为四类：

- **共识**：两份报告独立指向同一问题，提升置信度。
- **互补**：仅一份充分覆盖，复核后纳入综合基线。
- **冲突**：保留双方观点，按用户风险、不可逆性、仓库事实与验证成本裁决。
- **待验证假设**：用户偏好、性能数字、市场窗口、付费意愿均不伪装成事实。

### 1.2 证据等级

- **A：仓库事实**——当前 schema、路由、manifest、CI、用户文案、发布配置直接证明。
- **B：官方外部事实**——产品/项目官方文档、官方仓库、官方支持说明直接证明。
- **C：综合判断**——由 A/B 级事实推导出的产品或架构建议。
- **D：待验证假设**——需要访谈、可用性测试、benchmark、故障演练或市场实验。

### 1.3 本轮纠正的证据越界

1. Lap 当前官方仓库可确认约 1.3k stars、v0.2.4、local-first、100k+、基础编辑及相近技术栈；这能证明直接竞争与注意力，不能证明 PMF、留存或付费。
2. Chinese-CLIP 官方 README 的公开新闻停在 2023 年，可登记维护动能风险，不能直接定性“半弃养”。
3. DirectML maintenance mode 已由微软官方仓库明确，不再只是社区猜测；近期仍受支持，但无新功能计划。
4. 仅凭竞品使用 InsightFace 不能断言其许可违规；正确动作是逐模型、逐权重、逐用途建立 license ledger。
5. SQLite FTS5 的 tokenizer 是 virtual table 级配置，不支持“同一表不同列分别用 unicode61/trigram”的直接写法；必须选择两张索引表、custom tokenizer 或经基准验证的单一策略。

## 2. 产品定义：只做一种用户承诺

### 2.1 目标用户

首要目标用户是：

1. 有 5 万至 100 万照片/短视频，分散在电脑、移动硬盘、已有目录或挂载 NAS 的个人与家庭档案维护者。
2. 不想把所有原片上传云端，又希望获得现代搜索、人物、地点和回忆体验的人。
3. 不愿维护 Docker/Postgres/Redis，也不愿理解 GPU、batch、WIC、Canvas、model profile 等实现术语的桌面用户。
4. 愿意为“省时间、保隐私、保整理成果、一次买断或透明付费”付费的普通高级用户，而非专业协作 DAM 团队。

### 2.2 明确非目标

首个正式版本不同时服务：

- Lightroom 级 RAW 调色与完整 non-destructive editor；
- 手机自动备份与家庭云；
- 专业团队审核、多人权限和 enterprise DAM；
- 通用文档阅读器、版本管理和远程 AI 校对；
- 音乐库与歌词播放器；
- 第三方 Plugin Marketplace；
- 多租户 server、E2EE sync 和公开分享平台。

这些方向不是永远错误，而是每一个都需要独立用户研究、成功指标、资源与 threat model。

### 2.3 核心 JTBD

1. 不搬动、不改名、不上传原片，就把多个旧目录变成统一图库。
2. 只记得画面、人物、地点或时间时，能迅速找回照片。
3. 清楚知道某个操作只改 catalog、会改原片，还是会生成新副本。
4. 移动盘离线、目录移动、换机、升级或数据库损坏后，整理成果仍然回来。
5. 找到照片后能可靠 export、迁移或交付，并得到逐项失败报告。

### 2.4 产品信任契约

对外承诺必须长期满足：

- 原片属于用户，不属于 catalog。
- 默认只读；写原片或 sidecar 必须显式授权、预览并可追踪。
- thumbnail、embedding、face observation、OCR、ANN 等派生物可清理、可重建。
- album、rating、tag、favorite、person correction、caption 等用户事实可备份、可恢复、可导出。
- 默认不上传照片；任何模型下载、插件下载、WebDAV、远程 AI 或未来 sync 都有可见 consent 与 network activity。
- 订阅或授权到期不锁 catalog，用户仍可读、可导出自己的原片引用与整理事实。

### 2.5 北极星体验

> 用户选择一个多年旧图库后，在 15 分钟内找回一张一年以上、原本不知道文件名或目录的照片，并能准确说明原片是否被移动、修改或上传。

“百万图库能滚动”是性能能力；“快速找回并愿意继续托付”才是产品结果。

## 3. 市场与竞争：窗口存在，但不能被竞品牵着跑

### 3.1 直接竞品 Lap 的含义

[Lap 官方仓库](https://github.com/julyx10/lap)当前将自己定位为 macOS、Windows、Linux 的开源 local-first 照片管理器，公开能力包括 100k+ 图库、端侧 AI、人物、去重、基础编辑和广格式支持；技术栈也是 Tauri + Rust + Vue + SQLite + ONNX Runtime。2026-07-13 页面显示约 1.3k stars，最新 v0.2.4 发布于 2026-06-16。

这带来三个结论：

1. **需求验证**：相近判断能获得早期开发者与用户注意力。
2. **时间压力**：Scrollery 的纸面优势只有交付后才是优势。
3. **差异化升级**：不能只说“本地、AI、大图库”，因为这已成为品类描述。

但不应从 stars 推出留存、满意度、商业成功，也不应为追赶竞品功能表而扩大范围。真正可守的差异化应是：

- 中文与本地语境下的高质量找图；
- 移动盘/NAS 断连、重挂载、改名后的稳定身份；
- catalog 备份、迁移、恢复与无锁定；
- 百万级性能有公开可复现证据；
- 安装包签名、清晰隐私承诺、许可透明和可靠升级；
- 比专业 DAM 更低认知负担。

### 3.2 竞品学习原则

| 参照 | 应学习 | 不应照搬 |
|------|--------|----------|
| digiKam | XMP/IPTC/EXIF、批处理、文件自主、专业 filter | 高密度导航与全部专业能力前置 |
| Immich | 完整 lifecycle、统一 jobs、backup/restore 解释、人物/搜索旅程 | 单机 MVP 引入 Postgres/Redis/server 运维 |
| PhotoPrism | 既有目录零迁移 index、Original 与派生物分离 | 把内部 Index/Import 概念当首启负担 |
| Apple/Google Photos | 单框找回、People/Places/Memories、低认知负担 | 云和生态锁定 |
| Lightroom Local | 本地目录、参数化非破坏编辑、XMP 能力 | 首版扩为完整 RAW editor |
| Lap | 同类功能取舍、签名和基础编辑的用户预期 | 逐项追赶、以 stars 代替用户验证 |

### 3.3 立项前必须回答的市场问题

- 目标用户是家庭档案维护者，还是专业摄影爱好者？两者对编辑、metadata、sidecar 的默认值不同。
- “不上传、零迁移、中文 AI、大图库”中，哪个是第一购买理由？
- 用户愿意为买断 Pro、supporter key、年度升级还是服务订阅付费？
- 用户现在如何解决：Explorer/系统 Photos、digiKam、Lightroom、NAS、自托管还是放弃整理？
- Picasa 遗产用户是否足够集中，值得优先做 `.picasa.ini` 导入桥？

这些问题需要 10–15 名目标用户访谈、竞品任务测试和 landing/pricing 实验，不能由仓库功能数量回答。

## 4. 范围裁决：短期隐藏，长期解耦

### 4.1 双方共识

两份 Review 都认定照片/视频以外的 Reader、Audio、Proofread、Plugin Marketplace 与 H-Lab 已稀释主产品。当前正式路由确实把 plugin、document、audio、viewer 与实验画廊暴露为一等页面（`src/router/index.ts:46-82`）。

### 4.2 “不删代码”与“架构剥离”的综合裁决

另一报告主张“收缩叙事，不收缩代码”，避免浪费已完成能力；原报告主张冻结并按 bounded context 剥离。二者适用于不同时间尺度：

**立即：**

- 从默认导航、onboarding、营销页和首发 capability matrix 隐藏 Reader、Audio、Proofread、Marketplace、H-Lab。
- 冻结这些线的新 feature、schema 扩张、专属设置与发布承诺。
- 保留成熟代码，不做情绪化删除，不让范围治理演变成重写项目。

**进入 Catalog v2 时：**

- Reader/Proofread/Audio 不再向核心 `Asset`、Job 状态、全局 Settings 与 IPC surface 注入专属语义。
- 保留为独立 package/bounded context、实验 build 或未来独立产品候选。
- Proofread 的远程 AI 网络能力必须与“默认离线”承诺隔离，默认关闭并单独 consent。

**重新进入主产品的条件：**

- 有独立用户研究证明与核心照片用户高度重叠；
- 有单独 activation/retention/revenue 指标；
- 不破坏照片主线的性能、信任与交付 SLO；
- 有明确维护人和 release budget。

## 5. 用户体验：从“功能可用”改为“旅程闭环”

### 5.1 首次激活是当前最高产品风险

当前 onboarding 选择目录后只登记 scan root，主题与语言占三步中的两步，完成条件只是写入 `first_launch=false`（`src/components/common/OnboardingWizard.vue:178-216`）。这无法证明扫描已启动、首图出现、权限正确或用户理解隐私。

建议首次旅程：

1. 一句话价值与“默认不移动、不修改、不上传”。
2. 支持多选目录/卷，默认 Index Existing Folders。
3. Preflight：文件数估算、支持格式、目录重叠、权限、缓存空间、网络盘与离线风险。
4. 启动渐进扫描，第一批 thumbnail 出现即进入 timeline。
5. 基础图库可用后，分别询问 semantic 与 face，不能合并成一个 AI 开关。
6. Job Center 解释还在后台补齐什么、预计多久、失败如何处理。
7. 引导用户完成第一次“找一张旧照片”。

主题和语言默认读取系统，可在普通设置中修改，不应占用首次成功路径。

### 5.2 信息架构

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

原则：主导航只放用户对象和目的；scan、AI、thumbnail、plugin、renderer、GPU、batch 属于任务、管理或实现层。

### 5.3 搜索心智

- 默认只有一个搜索框，混合 filename、folder、date、person、place、camera、tag 与 semantic。
- AI 未完成时仍返回 metadata/filename 结果，并显示语义索引覆盖度。
- 高级筛选用 filter chips/query builder，不让普通用户切 normal/mixed/semantic 模式。
- 结果显示匹配原因；零结果提供清筛选、改写、等待索引、搜索文件名等恢复动作。
- 搜索词、filter、sort、layout 写入 route state，Back/Forward 可重现。
- Saved Search 保存 versioned query AST，不保存脆弱 SQL 字符串。

### 5.4 整理动作必须分三类

| 类型 | 例子 | 用户承诺 |
|------|------|----------|
| Catalog-only | Favorite、Rating、Tag、Album、Person correction | 不移动、不修改原文件；可备份和撤销 |
| Original-affecting | Move、Rename、OS Trash、embedded metadata writeback | 操作前预览影响、冲突和可撤销性；进入 Activity |
| New copy | Copy、Export、Convert、Resize | 原文件不变；明确目标、格式、metadata 与空间 |

选择条和菜单按这三类分组，避免“收藏夹”同时表示 Favorites、Album 与物理文件夹。

### 5.5 Export 是 P0，基础编辑是有条件发布门

两份报告都认可 export/可移植性不足。完整 Export Wizard 至少支持：

1. Original / Copy / Metadata Manifest。
2. format、quality、dimension、color profile。
3. preserve/strip EXIF、GPS、caption、rating。
4. naming template 与 folder structure。
5. skip/rename/replace-with-backup 冲突策略。
6. 目标权限、空间 preflight 与逐项失败 manifest。
7. Job Center 中暂停、重试和重跑。

对“基础编辑是 P0”的分歧裁决如下：

- Catalog alpha：不以编辑为 gate，先验证导入、浏览、找回、整理、恢复、export。
- 面向普通消费者宣称“Picasa 替代”的正式版：持久 rotate、crop/straighten、基础光照调整进入候选 release gate，应先做用户测试。
- 一律采用参数化 non-destructive recipe、sidecar 或 Export Copy；不在首版做完整 RAW 调色、图层或 destructive overwrite。
- slideshow 是低成本 P1，不应压过身份、恢复与 export。

### 5.6 可移植性三层

1. **P0 Catalog Bundle**：数据库一致性 snapshot + manifest + schema/version + restore preview；明确不包含 originals。
2. **P0 JSON/CSV Export**：album、rating、label、tag、person name、caption 等稳定契约；用相对路径 + content hash + stable asset id 多锚点。
3. **P1/P2 Interop**：`.picasa.ini` import 作为迁移楔子；XMP sidecar 读入优先，写回默认关闭，经过 preview、conflict policy、backup 与 operation journal。

## 6. 商业化裁决：先完成可售闭环，不以收费掩盖产品风险

### 6.1 双轨而非串行

另一报告认为 Part8 商业化执行应排最前；原报告认为身份、backup、downgrade 与 operation journal 的不可逆风险优先。综合结论不是二选一：

**市场轨可以立即做：**

- 一页清楚的产品定位和 Windows-first 支持矩阵；
- 签名安装包、版本说明、反馈入口和 crash/diagnostic opt-in；
- Free/Pro capability matrix、价格假设、supporter key 或付费意愿实验；
- 受控 alpha 招募与 10–15 名目标用户任务测试；
- 商标、第三方 model/codec/plugin 许可与 FTO 清单。

**但正式收费前 Trust Gate 必须完成：**

- stable identity containment；
- catalog backup/restore；
- downgrade fail-closed；
- export 与用户事实出口；
- destructive operation journal/Trash；
- privacy/consent 与数据删除路径。

支付、授权和官网可以验证购买意愿，但它们不是用户数据安全的替代品。

### 6.2 建议商业边界

- 免费核心：catalog、浏览、metadata search、backup/restore、基础 export、用户事实迁移。
- 可收费：高成本/高质量 AI model、专业格式、批量自动化、高级去重/策展、可选服务。
- 许可到期：已有 catalog 和用户事实保持可读可导出；不会因派生能力失效锁库。
- 对用户展示 capability，不让用户理解 Cargo feature、worker channel 或模型内部名称。

### 6.3 不应过早承诺

- 不以“一次买断”作为未验证的永久商业承诺；需测算 model、签名、客服和跨平台维护成本。
- 不在付费边界未清晰时把 AI、face、exotic 全部既写成“已实现”又定义为闭源能力。
- 不以插件市场承担早期变现；生态治理、签名、sandbox、更新和客服成本过高。

## 7. 技术选型：保留栈，重做边界

### 7.1 保留项

| 选择 | 裁决 | 约束 |
|------|------|------|
| Tauri 2 | 保留 | 只做 shell/lifecycle/OS adapter，不拥有业务真相 |
| Rust | 保留 | 适合 scan、hash、decode、SQLite、调度与 worker supervisor；避免全部堆入 AppState |
| Vue 3 + TypeScript | 保留 | 采用 typed IPC；UI state 与 domain state 分离 |
| Pinia | 保留但收缩 | 管 view/session state，不做跨窗口 Job 或持久 catalog 真相 |
| SQLite + WAL | 保留 | 单用户 local-first 的正确默认；backup、integrity、downgrade 必须产品化 |
| Worker subprocess | 保留并强化 | 隔离 crash/资源；进程隔离不等于 sandbox |
| ONNX Runtime | 保留为 adapter | model/provider/version/input fingerprint 入数据模型；不绑定单一 EP |
| Vanilla CSS/tokens | 保留 | 冻结 primitive 扩张，先补 a11y 与 visual regression |

### 7.2 停止或后置项

- 不为“未来可能多端”先引入 server/Postgres/Redis。
- mobile、LAN、E2EE sync 在桌面核心验证前 defer；只保持 domain core 不依赖 Tauri command/absolute path。
- 不预先拍板 Flutter/UniFFI；如果 mobile 成为真实需求，再比较 native、Flutter、WASM/shared Rust core 的产品成本。
- Plugin Marketplace、Reader、Audio、远程 Proofread 不再扩充核心状态和主导航。
- renderer/GPU/batch/model profile 移入 Labs/Expert，不让普通用户为内部策略负责。
- 不以 path 或 SQLite row id 作为跨备份、跨迁移和跨设备身份。

## 8. Greenfield 架构：modular monolith + 受控 worker

### 8.1 目标形态

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
                                      ├─ Image/Video Worker
                                      ├─ AI Worker
                                      └─ Untrusted Decoder Worker（后置）
```

采用一个桌面主进程、一个本地 catalog 和若干受控 worker。进程边界只用于 crash、资源与权限隔离，不把每个业务域微服务化。

### 8.2 bounded context 与真相所有权

| Context | 权威真相 | 不应拥有 |
|---------|----------|----------|
| Library/Catalog | Asset、FileInstance、Source、Volume、关系 | AI job 状态、UI scroll state |
| Ingestion/Reconciliation | filesystem observation、scan cursor、identity match decision | 用户 rating/album |
| Organization | Album、Rating、Tag、Person correction、edit recipe | 原始 EXIF、thumbnail path |
| Discovery/Search | query、Saved Search、可重建 FTS/ANN index | Asset identity、用户事实 |
| Jobs/Artifacts | job lease、progress、artifact manifest、failure | UI transient state |
| Backup/Portability | catalog snapshot、restore plan、import/export contract | “原片已备份”的虚假结论 |
| Privacy/Diagnostics | consent、network ledger、redaction、diagnostic bundle | 业务数据的影子副本 |
| Tauri Shell | window、dialog、protocol、lifecycle、OS capability | scan/AI/album 业务状态机 |

### 8.3 API 约束

- Tauri command 只调用 Application Service，不直接拼业务 SQL 或操作任意 path。
- 输入优先使用 `asset_id/source_id/job_id/operation_id`，绝对路径只出现在受控 adapter 内。
- Rust DTO 与 TypeScript contract 从单一 schema/codegen 生成，错误码和版本可测试。
- 长任务统一返回 `job_id`，通过 subscription/event stream 报进度。
- capability 按窗口和领域拆分；Gallery 无权安装 plugin、读取任意 `$APPDATA` 或执行任意 shell open。
- 首版严格 single-instance，第二实例只转发 open intent；避免每个 pipeline 各自补多实例竞态。

### 8.4 统一 Job、Artifact 与 Writer

当前扫描、thumbnail、AI、face、derivation、exotic 各有状态、token 和重试语义。目标是一个持久 Job System：

`jobs` 最低字段：

- `job_id`、`job_type`、`subject_type`、`subject_id`；
- `input_digest`、`processor_id/version/profile_id`；
- `state`：queued/running/paused/retry_wait/succeeded/failed/cancelled/superseded；
- `priority_class`：interactive/visible/background/maintenance；
- `attempts/max_attempts/next_retry_at`；
- `lease_owner/lease_expires_at/generation`；
- `progress_current/total/message_code`；
- `stable_error_code/error_detail_redacted`；
- `operation_id` 与时间字段。

关键不变量：

- `(job_type, subject_id, input_digest, processor_version)` 幂等去重。
- 结果 finalize 使用 generation compare-and-set，旧 worker 迟到结果不能覆盖新结果。
- crash 后依 lease expiry 恢复，不把所有 running 粗暴退回 pending。
- cancel 是持久可观察状态，不只是一枚内存 token。

`artifacts` 统一管理 thumbnail、preview、transcode、face crop、OCR、embedding 和 ANN segment，记录 source digest、generator/version、storage URI、content hash、size 与 integrity。Job 成功时在同一 finalize transaction 写 artifact manifest 与结果。

SQLite single writer 保留，但成为可观测 Writer Actor：interactive 写优先、后台 batch 有上限和 cooperative yield，暴露 queue depth、wait time、transaction duration 与 retry。

### 8.5 DB 与文件系统之间使用 Operation Journal

真实 move/copy/rename/trash/export 与 SQL 无法共享 ACID transaction。正确流程是：

1. 持久化 `operation_intent` 和逐项计划。
2. 执行 filesystem action。
3. 逐项记录成功、失败和外部观察。
4. 更新 catalog projection。
5. finalize operation。
6. 启动 reconciler 处理 crash 中间态。
7. 在 Activity 中提供逐项结果、retry 与可行的 undo。

每个操作有 stable `operation_id`/idempotency key，重复调用不会再次移动或复制。

## 9. 数据结构：先止血，再建立正确身份

### 9.1 当前事实

`media_items` 当前以自增 `id` 标识，以 `(directory_id, file_name)` 唯一；`content_hash` 可空（`src-tauri/src/db/schema.rs:74-109`）。这不能稳定表达外部改名、跨盘移动、同内容多副本、RAW+JPEG、Live Photo、编辑副本与 sidecar。

两份报告都认定 path identity 是最深数据债，但提出不同解法：

- 渐进方案：让 cache key 锚定 `content_hash`，全量回填并建立去重基础。
- 目标方案：把用户心智中的 Asset 与具体 FileInstance 分开。

综合裁决是两阶段，而不是二选一。

### 9.2 Horizon A：立即 containment

目标是停止继续放大损失，且不立刻 big-bang 改全库：

1. 确定 content hash 算法、版本和 upgrade policy；先 benchmark BLAKE3/SHA-256，不凭偏好拍板。
2. 对受支持图片/视频全量或渐进回填 `content_hash`，建立完整度和失败状态，不把 NULL 当“无重复”。
3. 派生 cache/artifact key 改为 `source_digest + processor/version/profile`，与 path/mtime 解耦。
4. 扫描端先以 filesystem identity、size/quick fingerprint、full hash 识别可能的 rename/move。
5. `content_hash` 不设全局 UNIQUE；合法多副本必须保留。
6. 新增用户事实不再只绑定脆弱 path key；至少携带 exportable stable id 与 hash/path 多锚点。

这一步能减少改名后全部重算与“删除+新增”，但它仍不能解决 Asset 与多个文件实例的语义。

### 9.3 Horizon B：Catalog v2 目标模型

#### `assets`

代表用户心智中的一张照片或一段视频，使用 UUID/ULID 等 stable id：

- `asset_id`、`media_kind`；
- `capture_time_utc/tz_offset/source`；
- `primary_file_instance_id`；
- `stack_id`、`tombstone_at`；
- `created_at/updated_at`。

#### `file_instances`

代表一个具体文件副本或容器成员：

- `file_instance_id`、`asset_id`；
- `source_id`、`volume_id`；
- `normalized_relative_path`、`display_path`；
- `filesystem_identity`；
- `size_bytes`、`mtime_ns`、`quick_fingerprint`、`content_hash`；
- `availability`、`first_seen_at`、`last_seen_at`、`missing_since`；
- `format`、`role`：original/sidecar/paired_video/edited_copy。

path 只在同一 Source 内唯一，content hash 也不全局唯一。

#### `sources` 与 `volumes`

- Source 是用户批准的逻辑扫描根，包含 exclude、watch 与 metadata write policy。
- Volume 是可插拔物理/网络卷的稳定身份与最近挂载状态。
- Source 通过 Volume + relative root 定位，不把盘符或绝对 path 当永久身份。

#### `asset_relations`

显式表达 `duplicate_of`、`raw_jpeg_pair`、`live_photo_pair`、`burst_member`、`edited_from`、`stack_member`、`sidecar_of`，不继续压进一个 `companion_of`。

### 9.4 用户事实、提取事实与派生事实分域

| 数据域 | 示例 | 策略 |
|--------|------|------|
| Original/User Asset | 原照片、原视频、sidecar | 永不当 cache；默认不改；由用户备份 |
| User-authored Facts | album、rating、tag、favorite、person correction、caption、edit recipe | 强 durability；自动备份；可导出；migration 前 snapshot |
| Extracted Facts | EXIF/IPTC/XMP/container probe | 保存来源、extractor/version、input digest 与冲突 provenance |
| Derived/Rebuildable | thumbnail、embedding、face observation、OCR、ANN、layout | 可清理和重建；记录 processor/version/input digest |

Face 模型升级时，`face_observation` 可重建，用户确认的 `person`、assignment/rejection 不能被覆盖；不同 embedding space 的 centroid 不混用。

### 9.5 身份重链接顺序

1. 同 Source/path 命中，更新 observation。
2. OS filesystem identity 命中，识别 rename/move。
3. size + quick fingerprint 产生候选。
4. full content hash 确认 exact duplicate 或移动。
5. EXIF、capture time、dimension、perceptual hash 只做模糊候选。
6. 多候选进入 Review Changes，由用户确认。

原则：宁可保留两个待合并候选，也不要错误合并两个不同 Asset。

### 9.6 数据约束与迁移

- enum/range 使用 CHECK；rating 不再无值域。
- 时间统一单位与 UTC 语义，拍摄时间保留原时区/未知状态。
- 不用 `0` 同时表示未知、未处理和没有。
- JSON 字段带 schema version；stable code 有未知值策略。
- 用户事实有 created/updated/provenance；派生事实有 input/processor fingerprint。
- `db_version > binary_max_version` 必须 fail closed，只允许只读诊断/export 或升级引导。当前实现只 warning 后返回成功（`src-tauri/src/db/migration.rs:154-177`）。
- migration 前自动 snapshot；migration history 记录 checksum、started/finished/dirty。
- Catalog v2 采用 Strangler：旧库冻结扩张、v2 并行只读验证、只迁用户事实、生成 ambiguous/orphan report、写入只保留单一权威。

## 10. 搜索、AI 与媒体技术裁决

### 10.1 基础全文搜索：FTS5 前移，但先做正确设计

当前 filename search 明确使用 `LIKE`，注释计划 Phase 3 再迁 FTS5（`src-tauri/src/ipc/search_commands.rs:1-17`）。对以“找回”为核心的产品，基础搜索不能长期全表扫，应在正式版前进入 P0/P1 交界的地基工作。

SQLite 官方 [FTS5 文档](https://www.sqlite.org/fts5.html)说明：

- tokenizer 在 FTS virtual table 级配置，不是 column 级；
- trigram 支持 substring 和可索引 LIKE/GLOB；
- 少于 3 个 Unicode 字符的全文 query 不匹配，某些 LIKE/GLOB 情况会回退全表扫；
- external content FTS 需要严格维护一致性并提供 rebuild/integrity 路径。

因此不直接采用“同表西文 unicode61 列 + CJK trigram 列”的表述。候选方案为：

1. 两张外部内容 FTS 表：token search 与 trigram substring 分离，再合并 ranking；
2. 一个 custom multilingual tokenizer；
3. 单一 trigram + 精确结构化 filter；
4. 对 1–2 字 CJK、短文件名和精确 ID 保留专门 fallback。

用中英混合真实 corpus 比较 index size、build time、incremental write、1/2/3 字 query、substring、prefix、ranking 与 100k/1M p95 后再定案。

### 10.2 向量搜索：exact baseline + 阈值触发 ANN

两份报告的分歧主要在何时引入 ANN。综合策略：

- 小/中图库用 SIMD/brute-force + fixed-size Top-K heap，避免全量排序。
- 查询结果直接返回或使用 `search_session_id`，不让全局 `ai_search_results` 相互覆盖。
- embedding cache 有明确 RAM budget、chunking/mmap/quantization 策略，不把 1M × 512 f16 约 1GB resident memory 当默认可接受。
- 以参考硬件上 1M p95、冷启动、内存上限和增量更新成本为门；过门失败再引入 sqlite-vec/HNSW/其他 ANN adapter。
- brute-force 始终保留作精排、recall baseline 和故障降级。

### 10.3 模型与 Execution Provider

**Chinese-CLIP：**保留当前 adapter，但登记维护风险。建立包含中文口语、时间、地点、物体、事件与 OCR 场景的脱敏 query set，对 Chinese-CLIP、[SigLIP2](https://huggingface.co/blog/siglip2)及其他可商用候选做 recall@K、延迟、内存、包体、量化损失和许可 bakeoff。没有证据前不直接替换。

**DirectML：**微软 [DirectML 官方仓库](https://github.com/microsoft/DirectML)已明确 maintenance mode：继续支持和安全/合规修复，但不计划新功能，并建议 Windows 11 24H2+ 评估 Windows ML。当前路径近期可留；EP 必须是 adapter，季度跟踪 Windows ML/ORT/provider maturity，并在真实模型上测 CPU、DirectML、未来 NPU 路径。

**人脸：**保留商业与 noncommercial feature 的物理隔离、符号/依赖 gate 与用户 opt-in。每个 detector、recognizer、weight、训练数据说明、再分发方式和商业用途都进入 machine-readable license ledger；不能只审代码 license。

### 10.4 媒体格式

- Windows WIC/Media Foundation 作为首发 native backend 是节制选择，但 capability matrix 必须说明 codec/平台差异。
- 不为格式数量无止境引入专利、动态库和安装复杂度；exotic decoder 走隔离 worker、签名、资源限制与默认禁网。
- JPEG XL 可作为较低法律负担的机会项，仍以真实用户 corpus 与解码性能决定。
- RAW 浏览可放 v1.x，用嵌入 preview/thumbnail 先满足“看见与筛选”；完整 RAW 编辑不进入核心 MVP。
- 去重采用 exact content hash + perceptual hash 分层；“哪张更值得留”的质量模型是 P1 差异化，不先于稳定身份。

### 10.5 布局与 DOM/Canvas

Rust 全集物化布局是当前简单且高性能的资产，不应在无 benchmark 时为理论扩展性改成 keyset 混合架构。正确动作是：

- 10k/100k/1M 合成库和至少一个脱敏真实库测 cold start、layout time、RAM、WebView2 peak、scroll long frame 与 settle time；
- 修复或证明 >约 25 万项坐标/physical spacer 的上限；
- 对外措辞在证据前写“十万级实测目标、百万级设计目标”或更保守分档；
- 达到内存/启动阈值后再评估分段 materialization/keyset，不提前推翻模型。

DOM 与 Canvas 不能无限双线发展。设 decision deadline：以 100k/1M 性能、a11y、交互一致性、维护成本和 visual regression 选正式路径；在此之前冻结两边新增 feature。若 Canvas 胜出，仍需等价 virtual accessibility tree 或 DOM companion，不能以性能交换键盘/屏幕阅读器。

## 11. 可靠性、安全与隐私：正式发行的 Trust Gate

### 11.1 Catalog backup/restore

WAL checkpoint 不是 backup。正式产品必须提供：

- 自动轮换 snapshot，显示时间、版本、位置、大小与验证结果；
- 手动 export/import Catalog Bundle；
- migration/restore 前自动 restore point；
- restore preview 与 schema compatibility、foreign key、integrity、文件可达性检查；
- DB 损坏时进入只读 recovery，不自动清库；
- 清楚说明 catalog backup 不含 originals，并单独展示 Original Backup Health。

Original、Catalog、Derived Cache 必须在 UI 中分开：原片由用户/第三方备份，catalog 由产品保护，cache 可清理重建。

### 11.2 Trash 与 Activity

- Trash 支持 Restore、Restore to、Permanent Delete、Empty Trash 和保留期。
- 原目录离线或不存在时提供新的恢复目标。
- Activity 同时记录 catalog action 和 filesystem operation，展示逐项失败、retry、undo 与 diagnostic id。
- 几秒 toast undo 只能作为便利，不是持久恢复模型。

### 11.3 Privacy Center

- 明确默认行为：“只在本机索引，不上传照片”。
- 展示 original、thumbnail、embedding、face vector、日志与凭据的位置、大小、生命周期、清理入口。
- semantic、face、remote AI 分别 consent；face 必须独立 opt-in。
- Network Activity 展示模型/插件下载、WebDAV、远程 AI、未来 sync 的目的和最近活动。
- 删除 model 不等于删除 embedding；提供按模型清除/重建派生物。
- telemetry/crash upload 默认 opt-in，上传前可预览；若完全不采集，也明确承诺。
- 日志和诊断包默认脱敏 absolute path、GPS、query、token、credential 和 remote URL secret。

### 11.4 Capability 与 worker sandbox

UI 只传 stable id/root grant，host canonicalize path 并验证在批准 Source、staging 或 output grant 内。媒体通过窄 custom protocol 按 asset id 提供，不 blanket expose 整个扫描根或 `$APPDATA`。

Worker subprocess 只提供 crash isolation，不自动提供安全 sandbox。对不可信 decoder/plugin 至少需要：

- 清理 environment，传单个 input handle 和 staging output；
- 默认禁网；
- CPU、RAM、wall time、output size、file count 硬限制；
- Windows restricted token/AppContainer/Job Object；macOS sandbox profile；Linux namespaces/seccomp/bubblewrap；
- crash/timeout circuit breaker 与 quarantine；
- package/model/plugin hash、signature、来源和 license manifest。

若平台上做不到强 sandbox，必须在 trust model 中明确“签名证明来源与完整性，不证明 decoder 没有漏洞”。

### 11.5 Durability 与本地可观测性

用户事实与派生写有不同价值。可选择同库分事务策略，或拆 catalog DB/derived DB；最终由 kill/断电测试决定。至少确保 rating、person name、album membership、backup metadata 不因追求派生吞吐而静默丢失。

本地 metrics ring buffer 建议记录：startup-to-interactive、first thumbnail、scan files/s、writer wait、job retry/crash、decode latency、search p50/p95、index coverage、cache churn、long frame、backup age 与 integrity。全部有 rotation/retention cap 和 redaction。

## 12. 平台、测试、CI 与发布

### 12.1 平台裁决：Windows-first，其他平台按证据升级

README 当前同时宣称 cross-platform 与 100k–1M 流畅（`README.md:3-4`），CI 当前没有 macOS workflow，视频、GPU、Trash、keyring、安装与 codec 也不等价。综合建议：

- v0.x 对外明确 Windows 10/11 first，列出硬件、codec、AI provider 与已知限制。
- macOS compile CI 可作为低成本 architecture smoke 尽快加入，但不等于用户支持。
- macOS 只有通过 signed/notarized package、runtime E2E、codec、Keychain、removable volume、VoiceOver 后才升级为正式支持。
- Linux 根据发行版/desktop/codec/Secret Service 范围明确 preview 或 community-supported，不笼统承诺等价。

每次发布公开 capability matrix：安装/升级、image/video formats、AI provider、Live Photo、removable/NAS、file ops/Trash、keyring、accessibility。

### 12.2 真实 E2E

- Add Source → first thumbnail → search → open → favorite/rating → Trash → restore → export。
- 真实 Tauri process、asset protocol、filesystem、SQLite、worker，不以 browser fixture 代替。
- 窄窗、高 DPI、多显示器、窗口恢复、drag/drop、system dialog。
- keyboard-only、Narrator；正式支持 macOS 后加 VoiceOver，Linux 后加 Orca。

### 12.3 历史与故障

- v1..vN 每个历史 DB fixture 升级 current；newer DB 被旧 binary 拒绝写。
- migration 中途 kill、磁盘满、权限丢失、WAL/DB 损坏。
- move/copy/rename/trash/export 每个阶段 fault injection，reconciler 给出确定结果。
- backup/restore 每个 release candidate 自动演练。
- 双实例、worker crash、timeout、GPU OOM、模型损坏与网络断开。

### 12.4 规模与性能

- 10k/100k/1M 合成库 + 脱敏真实库。
- 首图、cold start、layout、scroll、filter、FTS、semantic、reconcile、backup/restore、full rebuild。
- 参考低配/主流/高配 Windows 硬件；记录 CPU、RAM、VRAM、WebView2 peak、disk type。
- regression budget 进入 nightly/RC，不以一次人工测量证明永久 claim。

### 12.5 安全与供应链

- 畸形 image/video/EPUB/ZIP/sidecar corpus，parser/IPC fuzz，path traversal、junction/symlink、ZIP bomb、超大尺寸。
- capability negative tests：未批准 path、跨 root、任意 shell、token redaction。
- dependency audit/deny、license ledger、secret scan、SBOM、artifact signing、provenance/attestation。
- 开源 core 若继续由私有超集投影，漏规则同时构成源码/密钥/构建风险；长期优先公开 core canonical、私有产品依赖固定 core revision。

### 12.6 发布可信度

- 安装包签名与稳定 upgrade/uninstall 是产品功能，不是发布末尾杂务。
- release notes 明确 DB migration、回滚限制、模型变化、平台变化和 backup 建议。
- 公开 PR 的实际 CI 能力与 CONTRIBUTING 说法一致；不能声称 full suite 而只在私有/self-hosted 环境运行。
- 公开性能和隐私 claims 都有可复现验证方法与最后验证日期。

## 13. 综合路线图：依赖顺序，不是虚假工期

### Stage 0：停止线与立项验证

产出：

- 冻结非照片主线，隐藏 Reader/Audio/Proofread/Marketplace/H-Lab。
- 10–15 名目标用户访谈，5 个找图任务，3 个整理/恢复/export 任务。
- Windows-first 决策、Free/Pro capability matrix、许可/FTO 台账。
- Lap/digiKam/Immich/系统 Photos 的任务对比与 landing/pricing 实验。
- README 把百万级和跨平台 claim 改为当前证据分档。

退出门：目标用户确认零迁移 + 本地找回是现实痛点，并愿意拿真实图库参加 alpha；否则重新定位，不启动大规模 Catalog v2。

### Stage 1A：Trust containment

产出：

- content hash/version/backfill 与派生 cache key 解耦；
- downgrade fail-closed、migration snapshot、integrity check；
- Catalog Bundle backup/restore 和 JSON/CSV user-facts export；
- Operation Journal、正式 Trash、Activity；
- Privacy Center 最小版、path capability 收窄、single-instance；
- signed Windows alpha package。

退出门：newer DB 拒绝写、backup restore、外部 rename/move/replug、关键 file operation fault injection 全部有确定结果，无静默用户事实丢失。

### Stage 1B：Market alpha（与 1A 并行）

产出：

- 重做 onboarding，以 first thumbnail 和 first find 为 activation；
- 单框基础搜索与 FTS5 方案 benchmark；
- 完整 Export Original/Copy/Manifest；
- 反馈/diagnostic opt-in、support matrix、定价假设；
- 受控 alpha cohort 与周度访谈。

退出门：用户无需工程人员解释即可完成 Add Source、找回、整理、Trash/restore、export，并理解原片/联网边界。

### Stage 2：Catalog v2 与架构收敛

产出：

- Asset/FileInstance/Source/Volume 与 relation model；
- 统一 Job/Artifact、Writer Actor、typed IPC；
- user/extracted/derived facts 分域；
- 旧 catalog 迁移 preview、ambiguous/orphan report、只读保留；
- FTS5 正式实现和中英混合检索基准；
- DOM/Canvas 正式路径决策。

退出门：历史升级矩阵、身份 relink、双副本/RAW+JPEG、恢复演练、100k/1M 资源预算全部通过。

### Stage 3：智能差异化与消费者完整度

产出：

- semantic + metadata 混合搜索、People 可纠正 workflow；
- Chinese-CLIP/SigLIP2 模型 bakeoff 与 provider adapter；
- Saved Search、Duplicate/Similar、Places、On This Day；
- 持久 rotate、crop/straighten、基础光照（若用户测试支持）；
- `.picasa.ini` import，XMP read；可选 writeback 另立安全门；
- ANN 只在 exact baseline 过不了 SLO 时引入。

退出门：智能能力相对 metadata-only baseline 显著提高找图成功率，纠错不覆盖用户事实；消费者测试确认基础编辑取舍。

### Stage 4：扩展方向重新立项

候选 mobile/LAN、E2EE sync/backup、shared album、exotic marketplace、专业 RAW/writeback、Reader/Audio 独立产品。进入条件是 Stage 1–3 的 activation、trust、retention、restore、revenue 指标连续达标，且有独立研究与资源预算。

## 14. 优先级总表

### P0：正式发行阻断

| 项目 | 为什么阻断 | 最小验证信号 |
|------|------------|--------------|
| 照片/视频范围收敛 | 用户不懂产品为何存在 | 导航、产品页、pricing 一句话一致 |
| 首图/首次找回 onboarding | wizard 完成不等于激活 | 空图库误完成为 0，首图/首找可测 |
| Stable Identity containment | 改名/移动后事实和派生物断裂 | rename/move/replug 不静默丢事实 |
| Catalog backup/restore | 用户劳动不可恢复 | RC 自动恢复演练通过 |
| Downgrade fail-closed | 旧版可能写坏新 schema | newer DB 只读拒绝写 |
| Operation Journal + Trash | DB/FS 部分成功不可追踪 | kill 后 reconciler 确定收敛 |
| Export + user-facts manifest | 找到却无法交付/带走 | 大批量逐项报告、可重跑 |
| Privacy/Consent | “本地”不等于用户理解 | 用户能回答数据去向与删除方法 |
| Signed Windows package | SmartScreen/来源破坏信任 | 签名、升级、卸载真实 E2E |
| Claim 分档 | 宣传超出证据 | 支持矩阵与 benchmark 可复现 |

### P1：形成可爱、可留存的核心

- Catalog v2 Asset/FileInstance、统一 Job/Artifact、typed IPC、Writer Actor。
- FTS5 中英混合搜索、Saved Search、semantic coverage 与可解释匹配。
- Job Center、Activity、Backup Health、Privacy Center 完整版。
- Duplicate/Similar、Places、On This Day/Memories。
- 持久 rotate/crop/basic light 的用户验证与最小实现。
- macOS compile CI；若商业决策支持，再投入 runtime/notarization/E2E。
- worker sandbox、恶意 corpus、a11y/visual journey。

### P2：验证后扩展

- XMP writeback 与专业 batch metadata。
- RAW 浏览深化、JPEG XL、更多 exotic format。
- ANN、NPU/Windows ML 优化。
- mobile/LAN/server、E2EE sync、分享。
- Plugin Marketplace、Reader、Audio 独立业务线。

### 明确停止投入

- 继续为非照片主线新增 route/schema/setting。
- 在 path-based `media_items` 上继续挂用户事实。
- 每条 pipeline 自建 status/token/retry/progress。
- 在 Settings 增加 renderer/GPU/batch/model 微参数。
- 用 toast 代替 Trash/Activity/undo history。
- 未完成 benchmark 就强化“百万级”“跨平台”等措辞。
- 以 stars、测试数量或 commit 数代替用户价值和可靠性证据。

## 15. 成功指标与 Kill Criteria

下列数字是 D 级首轮目标，必须经参考硬件和用户研究校准。

### Activation / Findability

- 本地 SSD 首张可浏览 thumbnail：p50 ≤ 5 秒，p95 ≤ 10 秒。
- onboarding 错误空图库完成率：0%。
- 首次 15 分钟完成“找回一年以上旧照片”：≥ 70%。
- 五个预定义找图任务 30 秒内成功率：≥ 80%。
- metadata/FTS search p95：100k ≤ 200ms，1M ≤ 500ms。
- semantic search p95：参考主流硬件 100k ≤ 1 秒，1M ≤ 2 秒；内存不超 preset budget。

### Trust / Reliability

- 80% 目标用户准确回答原片是否上传/移动、AI 数据位置、Trash、catalog backup。
- Catalog restore 后 Album/Rating/Tag/Favorite/Person correction 一致率 100%。
- 外部 rename/move/replug 后，confirmed user facts 误丢失率 0；模糊候选不自动误合并。
- 关键 operation 每个阶段 100 轮 kill/fault injection 后无静默丢失。
- 所有历史 schema fixture 升级通过，newer schema 拒绝写。

### Performance / Retention / Revenue

- 已有 catalog 冷启动到可交互 p95 ≤ 3 秒（参考 SSD/硬件）。
- 30 秒连续滚动中 >50ms long frame 比例 <1%，停止后清晰图 settle p95 ≤ 1 秒。
- 前台交互后后台重任务在 250ms 内让出资源；取消在 2 秒内可观察。
- 四周内用户再次完成 search/rediscovery/curation/export，而非只打开设置。
- 单独测定 pricing page → download → activation → continued use → pay/support intent，不用下载量充当收入信号。

### Kill Criteria

- 目标用户更需要手机自动备份而非桌面零迁移，且不愿维护本地目录。
- 找图成功率不显著优于系统 Photos/Explorer/现有工具。
- 百万级性能必须牺牲数据可靠性、a11y 或普通硬件可用性。
- 用户不愿为差异化能力付费，开源/supporter 模式也不可持续。
- Reader/Audio/Plugin 需求只来自内部技术兴趣，无独立用户证据。

## 16. 产品负责人必须拍板

1. 第一用户是家庭档案维护者还是摄影爱好者？
2. 首发明确 Windows-first，还是为 macOS fast-follow 分配真实签名/runtime/E2E 预算？
3. 默认永远只读，还是允许 opt-in XMP sidecar？embedded writeback 是否永久后置？
4. Free/Pro 永不锁定的数据边界与价格实验是什么？
5. Catalog backup 由产品负责；Original backup 是健康提示、第三方集成还是未来服务？
6. AI 默认关闭、分步 opt-in，还是本地 semantic 在清楚告知后默认启用？Face 必须独立 consent。
7. 是否接受 single-instance 作为首版契约？
8. 持久 rotate/crop/basic light 是否是正式消费者版 release gate？由哪轮用户测试裁决？
9. Reader、Audio、Proofread、Marketplace 是独立 build、独立产品候选，还是永久停止？
10. Catalog v2 的迁移窗口和旧 catalog 只读保留周期是多少？

## 17. 最终建议

### 继续投资

- local-first、零迁移接管既有目录；
- 大图库渐进可用与可证明的资源治理；
- metadata + semantic + people 的统一找图；
- removable/NAS source 的离线与重链接；
- user facts 可备份、可恢复、可导出；
- 自动化可解释、可纠正、可关闭；
- 签名分发、许可透明和诚实 support matrix。

### 立即降级

- Reader、Audio、Proofread、Plugin Marketplace 主产品叙事；
- server/mobile/E2EE 的提前建设；
- 普通用户可见的 renderer/GPU/batch/model 微参数；
- 未经验证的 million/cross-platform 宣传；
- 在 Trust Gate 前继续堆不可重建用户事实。

### 综合架构原则

> 保留适合的技术栈，重做真相与责任边界：Asset 不等于 Path，Content Hash 不等于 Asset，User Fact 不等于 Derived Cache，Job 不等于 Thread Token，Signed Worker 不等于 Sandbox，Index 不等于 Backup，Compile Success 不等于 Platform Support，Payment Flow 不等于 Product-Market Fit。

如果只能先做：

- 一项产品工作：重做 Add Source → first thumbnail → first find。
- 一项数据工作：content-hash containment，同时确定 Asset/FileInstance v2。
- 一项可靠性工作：catalog backup/restore + downgrade fail-closed。
- 一项架构工作：Operation Journal 与统一 Job/Artifact。
- 一项市场工作：签名 Windows alpha + 真实用户任务/定价实验。
- 停止一件事：停止非照片主线的新增投资。

## 附录 A：两份报告的共识、互补与冲突

| 主题 | 关系 | 综合处置 |
|------|------|----------|
| 有条件 GO | 共识 | 保留，进入双轨 gate |
| 聚焦照片/视频 | 共识、力度不同 | 短期隐藏冻结，Catalog v2 中期解耦 |
| Tauri/Rust/Vue/SQLite | 共识 | 保留，重构边界 |
| 百万级方向正确但未证 | 共识 | benchmark 先于 claim |
| Lap 直接竞品 | 另一报告独有 | 纳入窗口压力，不当 PMF |
| 首次激活断裂 | 原报告独有 | 纳入 P0 |
| `content_hash` vs Asset/FileInstance | 冲突 | containment + target model 两阶段 |
| export/XMP/Picasa interop | 互补 | export P0，Picasa import P1，XMP writeback P2 |
| 基础编辑优先级 | 冲突 | alpha 非 gate，消费者正式版候选 gate |
| 商业化优先于地基 | 冲突 | Trust/Market 双轨并行，正式收费双门都过 |
| FTS5 CJK | 另一报告独有、实现需纠偏 | 前移，但 tokenizer 方案经官方能力与 benchmark 决定 |
| exact vs ANN | 侧重点不同 | exact Top-K baseline，SLO 触发 ANN |
| Chinese-CLIP/DirectML | 另一报告独有、证据纠偏 | 前者登记风险，后者 maintenance 已确认 |
| Job/Artifact/Operation Journal | 原报告独有深化 | 纳入目标架构与 Trust Gate |
| backup/downgrade/privacy/sandbox | 原报告独有 | 纳入正式发行阻断 |
| Windows-first/macOS | 共识、节奏不同 | Windows-first；mac compile CI 不冒充正式支持 |

## 附录 B：关键仓库证据

| 主题 | 当前证据 |
|------|----------|
| 产品承诺 | `README.md:3-21` |
| 产品范围 | `src/router/index.ts:46-82` |
| 首次引导 | `src/components/common/OnboardingWizard.vue:178-216` |
| Path identity | `src-tauri/src/db/schema.rs:74-109` |
| Schema downgrade | `src-tauri/src/db/migration.rs:154-177` |
| Filename LIKE | `src-tauri/src/ipc/search_commands.rs:1-17` |
| SQLite/WAL | `src-tauri/src/db/connection.rs:24-140` |
| 分裂任务状态 | `src-tauri/src/db/schema.rs:91-93,235-284,440-443,494-522,721-734` |
| 全局运行时状态 | `src-tauri/src/state.rs:23-183` |
| IPC/capability | `src/constants/ipc.ts:5-230`、`src-tauri/src/lib.rs:700-926`、`src-tauri/capabilities/default.json:5-19` |
| CI/Release | `.github/workflows/ci.yml:31-271`、`.github/workflows/release.yml:35-123` |
| 商业边界 | `CONTRIBUTING.md:5-18`、`CHANGELOG.md:19-28` |

## 附录 C：本轮重新核验的官方资料

- [Lap 官方仓库](https://github.com/julyx10/lap)
- [SQLite FTS5 官方文档](https://www.sqlite.org/fts5.html)
- [Chinese-CLIP 官方仓库](https://github.com/OFA-Sys/Chinese-CLIP)
- [DirectML 官方仓库](https://github.com/microsoft/DirectML)
- [SigLIP2 介绍](https://huggingface.co/blog/siglip2)
- [InsightFace 商业授权页面](https://www.insightface.ai/solutions/face-recognition-licensing)
- [digiKam About](https://www.digikam.org/about/)
- [Immich Architecture](https://docs.immich.app/developer/architecture/)
- [Immich Backup and Restore](https://docs.immich.app/administration/backup-and-restore/)
- [PhotoPrism Library](https://docs.photoprism.app/user-guide/library/)
- [Ente Architecture](https://ente.com/architecture/)
- [Apple Photos User Guide](https://support.apple.com/guide/photos/welcome/mac)
- [Google Photos Search](https://support.google.com/photos/answer/15235862)
- [Lightroom Local Photos](https://helpx.adobe.com/lightroom/desktop/add-import-and-capture-photos/access-photos.html)
