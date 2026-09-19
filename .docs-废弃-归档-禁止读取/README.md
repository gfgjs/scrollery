---
id: 2026-07-10-README
status: active
type: index
line: 文档治理
created: 2026-07-10
last-verified: 2026-07-13
---

# docs 文档索引与治理规则

前端核心测试（500 项预算）：[当前27个测试文件清单](worklogs/2026-09-16-前端核心测试500项/inventory.md)；[取舍与集中依据](worklogs/2026-09-16-前端核心测试500项/findings.md)。

AI 首次分析模型下载引导：[实现与验证边界](status/AI-人脸流水线深审.md#2026-09-16-首次-ai-分析模型下载引导)；[实施记录](worklogs/2026-09-16-AI模型缺失引导/progress.md)。

近期代码审查：[2026-09-16 Review 报告与待确认项](reviews/2026-09-16-近期改动Review.md)。

融合玻璃界面：[当前视觉约定与验证状态](status/UI-多主题系统.md)；[实施归档](worklogs/2026-09-16-全局融合玻璃界面/closeout.md)；[侧栏色差补修](worklogs/2026-09-16-玻璃侧栏色差补修/closeout.md)。

主题配色：[当前配色模型](designs/2026-09-16-主题配色全新设计.md)；[分批实施与验证](worklogs/2026-09-16-主题配色重构/progress.md)；[收口处置](worklogs/2026-09-16-主题配色重构/closeout.md)。

演示打码：[功能与验证状态](status/演示打码.md)；[实施归档](worklogs/2026-09-16-演示打码/closeout.md)。

用户设置：[集中保存与完整重置方案](designs/2026-09-16-用户设置集中保存与完整重置.md)；[功能与验证状态](status/应用配置重构-外置配置文件与热应用.md)；[实施归档](worklogs/2026-09-16-设置集中保存实施/closeout.md)。

打包体积调查：[现有产物基线与优化候选](worklogs/2026-09-16-打包体积调查/findings.md)。

代码消融：[第一轮分析](reviews/2026-09-15-全仓代码消融/README.md)；[实施报告与量化结果](reviews/2026-09-15-全仓代码消融/实施报告.md)；[滚动状态](status/全仓代码消融.md)；[实施归档](worklogs/2026-09-15-全仓代码消融实施/closeout.md)。

门禁与测试调查：[现状盘点与精简建议](reviews/2026-09-15-门禁与测试精简调查.md)；[调查过程归档](worklogs/2026-09-15-门禁测试精简调查/closeout.md)；[精简实施记录](worklogs/2026-09-15-门禁测试精简实施/closeout.md)。

> 本目录是 Scrollery 全部内部工程文档之家(私有:公开快照规则以 `docs/**` 整体排除公开镜像——**因此一切文档不得移出本目录**)。目录 2026-07-11 由 `plan-docs/` 更名而来(`b0fced0`,历史文档正文中的旧名不回改)。
> 本文是目录 taxonomy 与 frontmatter schema 的**当前唯一真源**,兼工作线登记表;治理规则的原始论证与迁移史见 [plan-docs 文档体系整理方案](designs/2026-07-10-plan-docs文档体系整理方案.md)(2026-07-10 背景件,schema 以本文为准)。
>
> **本文定位**:纯入口/索引。**进度、状态、施工沿革一律不写在此**——线现状唯一权威是 [status/](status/) 分片(D-016);[todo.md](todo.md) 是全局线索引+跨线账本;已完成详注在 [completed.md](completed.md)。

## 新会话导读(人与 AI 通用)

1. 先读 [todo.md](todo.md) 线索引表,按需读对应 [status/](status/) 分片——「现在做到哪了」。
2. 回本索引按类型/主题线定位权威文档。
3. 已完成项详注与沿革 → [completed.md](completed.md);技术踩坑病历 → [experience.md](experience.md)。

专项验证入口：[Canvas密集缩略图实施与真机验证](reviews/2026-09-12-Canvas密集缩略图优化实施与真机验证.md)。

前端视觉调查：[UI视觉问题与优化建议](reviews/2026-09-12-前端UI视觉问题调查.md)。

生成性能调查入口：[缩略图生成流水线性能调查](reviews/2026-09-12-缩略图生成流水线性能调查.md)。

扫描性能调查：[文件夹入库元数据流水线](reviews/2026-09-12-文件夹入库元数据流水线性能调查.md)。

扫描优化验收：[入库元数据流水线实施与验证](reviews/2026-09-12-入库元数据流水线性能优化实施与验证.md)。

## 目录职责

开源前检查与商业取舍：[2026-09-14最终检查报告](reviews/2026-09-14-开源前最终检查与收费取舍.md)。

当前第一方许可：[AGPL-3.0-only 与单独商业授权决策 brief](decisions/2026-09-14-决策brief-AGPL-3.0-only与单独商业授权.md)（2026-09-14 决议，取代此前 MPL-2.0 结论）。

许可修订记录：[AGPL与商业双重授权](worklogs/2026-09-14-AGPL与商业双重授权/progress.md)。

源码发布记录：[开源发布与main同步](worklogs/2026-09-14-开源发布与main同步/progress.md)。

仓库换行约定：[现行规则](status/仓库架构与流水线全面梳理.md#换行约定)；[迁移记录](worklogs/2026-09-15-全仓换行统一/progress.md)。

| 目录 | 放什么 | 判据(读者拿它做什么) |
|---|---|---|
| `refactor_2026/` | 架构正典 Part0–8 + 卫星设计件 | 照着建(全局架构) |
| `spec/` | as-built 重建级施工规格集:按当前代码,为人类+低能力 LLM 提供从零重建依据 | 照着建(全局架构) |
| `designs/` | 单点设计方案,`YYYY-MM-DD-主题.md`;多部头线开 `designs/<线名>/` 子目录 | 照着建(单线契约) |
| `reviews/` | 审查/复核快照;多文件轮次一目录(README 索引 + 分册) | 查「当时查出了什么」 |
| `decisions/` | 决策 brief 存证,定案后不可变 | 查「当时为什么这么定」 |
| `runbooks/` | 操作手册,不带日期、名即主题 | 照着操作 |
| `planning/` | **施工中**长任务的工作记忆三件套(自研 `/planning` skill 产出;入库随 git 同步,支持换机续作) | 查「这条长任务此刻做到哪」(活文件,非正典) |
| `worklogs/` | **已收口**长任务的工作记忆三件套,`worklogs/<日期>-<任务名>/` | 查「当时这个任务怎么做的、发现了什么」 |
| `archive/` | 已归档(📦 完成使命)+ 已废弃(🔴 被推翻),文首横幅写明取代链 | 不该再照着用 |
| `lines/` | 工作线实体,`<slug>.md` 一句话使命;文档 frontmatter `line` 引用其文件名(开线 = 新建实体) |
| `status/` | 工作线滚动状态分片,`<slug>.md` 每线一文件(D-016),承载该线滚动状态板(2026-08-24 自 todo.md 分片化迁入);closeout 的 todo 处置按任务 line 落此(D-014) |

## 工作线字母登记表(已退役,新立项不再取号)

> 📦 本表已退役(2026-07-17):v4 起工作线 = `lines/<slug>.md` 实体,开线即建文件,无须取号。历史行已由 `worklog upgrade` 归并进对应线实体;失配项(死号/野号/表外线)见当次 upgrade 输出。

## 维护规则

2026-09-15：文档治理退出 CI；以下为人工维护约定，worklog-kit 仅按需手动运行，不再作为合并门禁。

- **任何改变文档状态/位置的 commit 须同步更新本索引**(与 todo.md 红线同款)。
- 新文档:入类型目录 + 文首 YAML frontmatter(id/status/type/line/created,可选 last-verified)。id创建后保持稳定。**机器枚举与 `.worklogrc.jsonc` 一致，中文仅作正文展示标签**:
  - `status: draft | active | snapshot | superseded | archived`，对应施工中、现行、快照、已废弃、已归档；superseded文档必须移入 archive/。
  - `type: design | review | decision | runbook | canon | working-memory | closeout | index | line | experience | rolling-status | acceptance | plan | experiment`。例如设计方案填design、工作记忆收口填closeout、实验报告填experiment，不把中文标签或括注写进机器字段。
- **收口即处置**:施工计划执行完毕的收口 commit 同步「移 archive/ + 文首横幅 + 修活区链接 + 更新本索引」,不允许「先收口、改日归档」。
- **归档不变量**:archive/ 内正文冻结,仅文首横幅可追加;archive 内部旧互链**不修**(存证原样);活区指向归档件的链接必须改指新路径。
- **长任务工作记忆**:自研轻量 skill `/planning`(`.agents/skills/planning/SKILL.md`,零 hook 零 token 税、中文任务名可用)——施工期三件套居 `docs/planning/<日期>-<任务名>/`(入库,支持换机续作);收口按「收口即处置」迁 [worklogs/](worklogs/README.md) + **逐候选蒸馏 + `closeout.md` 入库结构门**(候选全覆盖/disposition 枚举/target 验存或冻结引用,按需手动使用 worklog-kit 检查,契约详见 worklogs README)+ 回写 todo;手动检查对两处三件套过程件豁免 frontmatter 与断链,**目录内手写辅助文件与 closeout.md 不豁免**(须 status/type/created 轻 frontmatter)。前身 planning-with-files 插件工作流已弃用,参考件见 [归档手册](archive/planning-with-files-使用说明.md)。
- 开新文档门槛:方案有歧义/需备选对比、且正文预计 >50 行;否则在对应线 status/ 分片记一行并在 todo.md 线索引表登记。
- **不在本索引记进度/状态**:登记表只登记「字母→工作线→权威文档」;完成/收官/施工进度写入 todo.md(现状)或 completed.md(详注),不回灌本文。
- **双水位线(2026-08-24 立,与 todo.md 维护规则⑦一致)**:todo.md >20 KB 触发收拢(线索引表膨胀=该收口/该归档的线没处理);单分片 >30 KB 触发该线向 completed.md 归档瘦身;度量看字节不看 `wc -l`(超长单行会让行数失真)。
