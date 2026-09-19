---
status: 快照
type: working-memory
line: asbuilt-spec
created: 2026-07-24
---

# 发现与决策:施工规格集

> 本文件把摸底阶段(`assess-canon-governance.md`/`map-docs.md`,均为会话临时 scratchpad、随会话消失)的关键结论落为持久规划记忆,供后续 16 篇写手会话直接读取,不必重新摸底。

## 四份 scratchpad 原料文件路径(临时区,阅后即会消失,结论已蒸馏入本文件下方各节)
- `C:\Users\gf\AppData\Local\Temp\claude\C--workspace-scrollery\62c3e560-97c2-4d54-b084-49729fe40d36\scratchpad\assess-canon-governance.md`(治理规则 + Part0-8 完备度/漂移评估)
- `C:\Users\gf\AppData\Local\Temp\claude\C--workspace-scrollery\62c3e560-97c2-4d54-b084-49729fe40d36\scratchpad\map-docs.md`(现有文档盘点)
- 本任务未产出第三、四份原料文件;上述两份是摸底阶段的全部产出(prompt 称「四份」,实际交付两份,如需另两份需向发起方确认,详见「需裁决」)

## 治理规则(持久摘录)
- **frontmatter 必填字段**:`id`/`status`/`type`/`line`/`created`,可选 `last-verified`。机器权威已迁至根 `.worklogrc.jsonc`(ASCII canonical),`docs/README.md` 的中文展示枚举已滞后,新文档一律用英文枚举值。
- **status 枚举**:`draft | active | snapshot | superseded | archived`(`superseded` 为死态,活区不得出现,须移 `archive/`)。
- **type 枚举**(`.worklogrc.jsonc` v2,带 `canBeAuthoritative`):`design(T) review(F) decision(T) runbook(T) canon(T) working-memory(F) closeout(F) index(F) line(F) experience(T) rolling-status(F) acceptance(F) plan(F) experiment(F)`。本规格集正文用 `canon`(可承载 authoritative)。
- **v5 新概念(D-003)**:每 `(line, authorityScope)` 至多一个 `active`+`authoritative` 文档,判据="是否某条线在某范围内的当前答案"。本规格集与 Part0-8 同属不同 `line`(`asbuilt-spec` vs `refactor_2026`),暂不冲突;但若规格集内容事实上取代 Part0-8 某章节成为权威,需在收口阶段显式处理 `authoritative` 标记,避免同 line 内两个 active+authoritative 冲突(本次未标注 `authoritative` 字段,留待封顶阶段裁决)。
- **目录登记**:新增顶层目录须同步两处——`.worklogrc.jsonc` `dirs` 数组(已本次登记)+ `docs/README.md`"目录职责"表(已本次登记)。`refactor_2026/` 本身即"新增顶层目录"先例,`docs/spec/` 依此先例新建,不挂靠 `designs/` 子目录。
- **门禁 check_docs.mjs 已不在本仓**:2026-07-21(裁决 U-13)整体移除,迁至外部 npm 包 `worklog-kit`。CI(`.github/workflows/docs-governance.yml`)在 push/PR/merge_group 跑 `npx --yes --package worklog-kit@0.1.0-alpha.4 worklog-kit check` + `worklog-kit index`(版本钉死 alpha.4,勿裸跑 `npx worklog`)。本仓无法本地复现门禁逻辑,只能靠遵循既有惯例规避红线;`planning/**` 下 `task_plan.md`/`findings.md`/`progress.md` 三件套豁免 frontmatter 与断链检查,但本目录内其余手写文件(如有)不豁免。
- **交叉引用规则**:移动文档须同一 commit 同步全部仓内引用;`archive/` 内正文冻结、活区→归档链接必须改指新路径;开新文件门槛(惯例)= 方案有歧义或正文预计 >50 行,本规格集显然远超此门槛。

## 漂移与"引用 vs 新写"判定表

| Part0-8 领域 | 判定 | 理由 |
|---|---|---|
| 命名/品牌(Picasa Next→Scrollery) | **新写** | Part0 §10.7 / Part8 全篇仍写候选名,已被 `docs/decisions/2026-07-06-R2-7-*` 系列拍板取代,续写等于把废弃候选名写进新文档 |
| Part1 数据层 schema 现状 | **新写** | 正文自认止于 SCHEMA_V10/V11 过渡,现值以 `schema.rs` 实测为准截面,不续计划口吻 |
| 04 图像与色彩管线(moxcms/ICC/色域) | **新写** | Part3 原文无对应章节(色彩管理是重构后新增子系统) |
| 05 视频与音频(DXVA 硬解/自研 headless 播放器) | **新写** | Part3 原文 §2.1/§3.1/§3.7 无对应位置 |
| 06 AI-人脸-OCR(第4插件OCR + MF 同步 ReadSample 死等根因) | **新写** | Part4"三插件"产品假设已被 OCR 第4插件突破;运行时故障模式原文无记录 |
| 02 扫描与画廊(轴/minimap 交互体系) | **新写** | 原文 §3.8"时间轴直方图"颗粒度远不及现有视窗拖拽/canvas微缩预览/无缝分组解耦体系 |
| 11 前端架构(UI 原语库/六主题) | **新写** | Part5 原文成文时未规划组件原语库与主题系统 |
| 00 打包/盈利/四层防护(Part0) | **引用+勘误段** | 决策理由未过时,只追加当前实际值 |
| 09 插件平台协议骨架+开闭源边界(Part6) | **引用+勘误段** | worker 隔离/Ed25519 留开源等理由稳定 |
| 13 CI矩阵与渠道cfg门控(Part7) | **引用+勘误段** | 设计理由稳定,自托管 runner 等为实施细节勘误 |
| 13 签发/加密打包/激活(Part8) | **引用+勘误段** | 微服务设计理由稳定 |

## 治理处置(顺手,非规格集内容)
- `Part6_3c_Copybara同步配置草稿.md` 前提已被 Part0 §10.4 内联标注"作废",但仍 `status: active` 未移 `archive/`、未加废弃横幅——治理动作滞后,应在合适批次顺手处置(非本次范围,记录待裁)。

## 需裁决
- prompt 称"四份 scratchpad 原料文件",摸底阶段实际只产出两份(`assess-canon-governance.md`、`map-docs.md`);未见第三、四份文件,已如实记录,不臆造路径。

## 文档层顺手发现(供主线终报,非本线范围内施工项)
撰写/复核 16 篇 as-built 规格集过程中,读代码交叉核对文档陈述时顺手揪出的代码/文档不一致或潜在缺口。**处置轮次 2026-07-24(主线接续 `修复<文档层顺手发现>`):逐条回码核实,安全零风险的真值修复已落地,其余按理由留置**——

**已修(零风险真值/注释修复,本次落地):**
- ✅ `src-tauri/src/db/connection.rs`:读池注释写 4,实际配置为 8(`lib.rs:314` 传 8,理由在 :312-313)——已改注释 4→8 并补 8 的理由;顺带 `min_idle` 处「4× open」注释改「至多 pool_size 次」。
- ✅ `image_meta` 表:`dominant_hue/sat/lum/hex`+`is_monochrome` 5 列有 schema+models+SELECT 读回,但**全仓无 INSERT/UPDATE 写入路径**(回码核实),实测恒 NULL——已在 `schema.rs` 加注标为「预留列,尚无写入路径」,防后续误当有效数据源(仿 video_meta 预建注释先例;不删列以免多余迁移)。
- ✅ `.worklogrc.jsonc`:`profile` 注释「MVP 仅实现 strict」但值为 `brownfield`;`index.mode` 注释「invariant=MVP 默认」但值为 `generated`(D-014 生成档已实证生效)——两处注释已改为如实反映生效档。
- ✅ `store_doc_thumbnail` 守卫(**用户 2026-07-24 采纳单独立项后施工完成**):x-item-id 来自请求头不可信,原命令不校验 item 是否前端可渲染文档,拿非 doc-thumb 项(.txt/.jpg/epub/mp4)的 id 调用会污染其派生态+document_meta。落地=`queries/derivations.rs` 新增单源白名单常量 `FRONTEND_DOC_THUMB_FORMATS=&["pdf","svg"]`+谓词 `is_frontend_doc_thumb_format`+可测守卫 `validate_frontend_doc_thumb_item(conn,id)`;`list_pending_doc_thumbs` 的 `IN(...)` 改由该常量插值单源生成(消除泵/守卫双向漂移);`doc_commands.rs` store_doc_thumbnail 在 spawn_blocking 首行早退校验(覆盖空 body 失败分支),成功分支复用已校验 file_format 免二次查库。测=derivations 新增 3 测(whitelist/accept-reject/missing→MediaNotFound),`cargo test --lib derivations` 11 绿(含既有 list_pending_doc_thumbs 覆盖测),clippy+fmt 净。

**已核实为误报(无需修):**
- ❌ 前端依赖 `@tanstack`:原记「react-table 系列、画廊未用」**误报**。实为 `@tanstack/vue-virtual`(非 react-table),且**已被使用**——`src/components/logwindow/LogVirtualList.vue:10 import { useVirtualizer }`(消费方是日志窗口虚拟列表,非画廊)。Spec11:134 的「待核实」亦可据此结案:MediaGrid 用自研 BucketVirtualScroll,`@tanstack/vue-virtual` 的消费点=日志窗口。

**核实准确但纯信息项(无代码可改):**
- ℹ️ Ed25519 签名验证位于 `crates/scrollery-exotic-trust/src/{crypto,license}.rs`——回码核实**准确**;crate 名「trust」实指此处,记录留档即可。

**留置(附理由,主线/对应线裁定):**
- ⏸ `restore.rs:335`「不可信 DB 迁移」TODO:**记忆红线明令 defer 勿轻补**(见 [[backup-review-fixes-2026-07-19]] #10「不可信DB迁移加固」已归备份测试硬化专项,解冻须专项内裁)。不动。
- ⏸ 生产 CSP 缺 `tauri:` scheme 分档:**硬约束**(CLAUDE.md「Production CSP 严格且平台条件化」,改动须先问),且 Spec00 §1.3/Spec13/Spec14 已如实记录为待关闭 gap。非新发现,不在此波改。
- ⏸ Part6(`docs/refactor_2026/Part6_*`)crate 名/协议描述漂移:属 refactor_2026 线 canon;规格集 Spec 篇已按当前代码实况新写覆盖。改 Part6 正文=编辑他线 canon,留该线归口(勘误段而非重写)。
- ⏸ `.github/workflows/release.yml` 命名:文件名 release、职责 oss(`name: OSS Release`)。文件**经 Copybara 同步进公开仓**(:5),重命名牵动 Copybara 路径映射 + 分支保护必需检查引用,风险 > 收益(header 已自标 oss-release);建议留置。
- ⏸ `Part6_3c_Copybara同步配置草稿.md`:`status: active` 但前提据称被 Part0 §10.4 作废,未移 archive/未加横幅。属 Part6 ③c 线治理决策 + 需「移动同步全部引用」多步动作,不擅自归档他线 canon,留主线/该线裁。
