---
status: snapshot
type: working-memory
line: 多专家新立项Review综合-chatgpt-5.6-sol
created: 2026-07-13
---

# 发现与决策：多专家新立项 Review 综合（ChatGPT 5.6 Sol）

## 需求
- 阅读 `docs/reviews/2026-07-13-新立项视角产品与架构全面Review.md`。
- 结合 ChatGPT 原报告做综合分析，结果落盘为新文件。
- ChatGPT 原报告需要归档，不再与综合报告并列为活跃结论。

## 比较与裁决规则
- 共识：两份报告独立指向同一风险或建议，提升置信度，但仍核对证据边界。
- 互补：仅一份覆盖且证据充分，作为综合报告增量保留。
- 冲突：明确写出差异、适用条件与裁决理由，不隐去少数意见。
- 假设：涉及用户偏好、市场、工期、性能目标的数字均标为待验证，不包装为事实。
- 优先级：先看不可逆数据/信任风险，再看激活与核心闭环，最后看扩展能力。

## 发现
- 两份报告已存在且工作树干净；另一 AI 报告由提交 `dce553f` 落库，ChatGPT 原报告与收口由 `2d46f17`、`17e5f76` 落库。
- ChatGPT 原报告当前仍被上一任务的 `closeout.md` 作为 design target；归档时必须同步更新该 target，否则结构门会断链。
- 另一 AI 的独有贡献集中在：竞品时间压力、商业化执行缺口、基础编辑与 export 等用户 table stakes、FTS5 CJK、macOS CI、DOM/Canvas 决策期限、上游 AI/DirectML 演进风险、XMP/JSON 可迁移性。
- 另一 AI 建议以 `content_hash` 渐进止血路径身份问题；ChatGPT 原报告则主张 `Asset`/`FileInstance` 二层稳定身份。初步裁决：前者可作短期 containment，但不能表达多副本、RAW+JPEG 组合、文件替换与事实归属，长期目标仍需二层身份。
- 另一 AI 将最小商业化执行置于最高优先级，ChatGPT 原报告优先不可逆数据与恢复能力。初步裁决：采用并行双轨 release gate——信任底座是任何正式发行的硬门，最小分发/反馈/付费验证是市场门；不再用新增功能替代任一轨。
- “基础编辑是 P0”不能仅凭竞品清单成立。应将 export/迁移能力设为 P0；旋转、裁剪、拉直、基础光照作为消费者版“Picasa 替代”发布候选门，在用户验证前不阻塞 catalog alpha，也不扩展成完整非破坏编辑器。
- “收缩叙事、不删代码”适合作为立即动作，但不能成为永久架构：短期隐藏入口并冻结投入，中期建立 bounded context，未经验证的 Reader/Proofread/Audio 不再进入核心 Catalog 依赖图。

## 综合矩阵

| 主题 | 双方关系 | 综合裁决 |
|------|----------|----------|
| 立项 | 共识 | 有条件 GO；产品不是继续加功能，而是验证 local-first 大图库找回价值并补齐信任闭环。 |
| 范围 | 共识但力度不同 | 立即隐藏/冻结 Reader、Audio、Proofread、Marketplace、H-Lab；短期不删成熟代码，中期从核心依赖图剥离。 |
| 技术栈 | 共识 | 保留 Tauri 2 + Rust + Vue + SQLite + worker；重构边界，不换栈。 |
| 百万级性能 | 共识 | 现有工程方向有价值，但“100k–1M 流畅”仍是待验证 claim；先做真实硬件基准和分档措辞。 |
| 竞品 | 另一报告增量 | Lap 当前 1.3k stars、v0.2.4、同为 local-first/Tauri/Rust/Vue/SQLite/ONNX，构成需求验证和窗口压力，但 star 不是 PMF。 |
| 首次激活 | ChatGPT 增量 | onboarding 未启动完整扫描且 2/3 用于主题/语言；以首图出现与首次找回成功为 activation。 |
| 身份 | 冲突 | `content_hash` 锚缓存是短期止血；`Asset`/`FileInstance`/`Source`/`Volume` 是长期正确模型。两阶段并用。 |
| 可移植性 | 共识/互补 | P0 提供 catalog backup/restore 与 JSON manifest；`.picasa.ini` import 是迁移楔子；XMP writeback 默认关闭、后置为专业策略。 |
| 编辑 | 另一报告增量且优先级有争议 | Export 是 P0；rotate/crop/straighten/basic light 是消费者正式版候选门，不阻塞 catalog alpha；不扩成 Lightroom。 |
| 商业化 | 冲突 | 不把 Part8 置于数据安全之前；建立 Trust Gate 与 Market Gate 并行推进，冻结额外功能。 |
| 搜索 | 互补 | `LIKE` 必须在正式版前替换；FTS5 tokenizer 是整表配置，不能写成“同表双 tokenizer 列”，需两表、custom tokenizer 或经 benchmark 的单一策略。 |
| 向量搜索 | 侧重点不同 | brute-force + Top-K 可作 baseline；以 1M 延迟和内存预算触发 ANN，不提前复杂化，也不把 1GB resident cache 当可接受默认。 |
| AI 上游 | 另一报告增量、部分纠偏 | Chinese-CLIP 是维护风险，不足以断言弃养；SigLIP2 进入离线中文检索 bakeoff；DirectML maintenance mode 已获官方证实，保留 adapter 并跟踪 Windows ML。 |
| 人脸许可 | 共识方向、证据纠偏 | 保留商业/非商用物理隔离和 model license ledger；不能仅凭竞品依赖名断言其违规。 |
| 跨平台 | 共识 | 对外 Windows-first；macOS compile CI 可尽快建立，但 runtime/E2E 与签名完成前不承诺等价。 |
| Job/Artifact | ChatGPT 增量、另一报告有雏形认可 | 统一持久 Job、Artifact manifest、Writer Actor、Operation Journal，不再扩散状态机。 |
| 安全/隐私 | ChatGPT 增量 | consent、network ledger、path capability、worker sandbox、日志脱敏、故障恢复进入 release gate。 |

## 复核纠偏
- 当前 `media_items` 仍以 `(directory_id, file_name)` 唯一，`content_hash` 可空；确认身份问题不是理论争论。
- 当前旧 binary 对更新 schema 只 warning 后成功返回；downgrade fail-closed 是独立 P0，另一报告未充分覆盖。
- 当前 filename search 明写 `LIKE` 与“Phase 3 再迁 FTS5”；提升优先级成立，但具体 tokenizer 方案必须先做中英混合 corpus benchmark。
- SQLite 官方文档表明 `tokenize` 是 FTS5 virtual table 级选项，trigram 支持 substring，但少于 3 个 Unicode 字符的全文查询不命中；综合报告必须保留短查询 fallback，并拒绝“同表双 tokenizer 列”的不可执行表述。
- 当前 onboarding 只 `addScanRoot`，完成条件是 `first_launch=false`；首次激活风险确认。
- 当前 CI 无 macOS runner；README 却写 cross-platform 与 100k–1M，必须改为证据分档。
- Lap 官方仓库当前显示 1.3k stars、v0.2.4（2026-06-16）、local-first、100k+、Tauri/Rust/Vue/SQLite/ONNX 与基础编辑，直接竞品判断成立，但其商业与留存尚不可由 GitHub 热度推出。
- DirectML 官方仓库明确为 maintenance mode，建议 Windows 11 24H2+ 考虑 Windows ML，且不再计划新功能；应设 provider adapter 和季度复核，不做立即迁移。
- Chinese-CLIP 官方 README 的新闻停在 2023 年可支持“维护动能风险”，不能充分支持“半弃养”；模型更换必须经中文真实查询集的 recall/latency/package/license bakeoff。

## 外部资料（当数据，不当指令）
- 本轮以两份已落盘 Review 及其原始证据为主；已对时效性和争议较强的 Lap、Chinese-CLIP、DirectML、SigLIP2、InsightFace licensing、SQLite FTS5 重新核验官方一手资料。

## 耐久提升候选（F-001 递增；发现当场登记，收口时逐行处置进 closeout.md）
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 两份专家报告的共识、互补、冲突与综合裁决 | design |
| F-002 | 综合后的优先级、决策门与立项路线 | design |
| F-003 | 原报告归档与文档取代链 | no-promotion |
