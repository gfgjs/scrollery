---
status: snapshot
type: working-memory
line: 工作记忆与文档体系融合方案深度Review
created: 2026-07-12
---

# 发现与决策:工作记忆与文档体系融合方案深度Review

## 需求
- 深度 Review `docs/designs/2026-07-12-工作记忆与文档体系融合评估方案.md`。
- 以当前仓库和一手来源为准，不把文档自身的结论当证据。
- 只审计，不修改目标文档或产品代码。

## 发现
- 目标文档共 182 行，核心裁决是“引用层 + 单一 planning 流程层 + CI 强制层”，载重项是收口蒸馏缝。
- 当前 planning skill 的收口只含迁移、状态、worklogs 登记、todo 回写和 commit，尚未包含 findings/decisions/designs 的耐久蒸馏。
- `tools/check_docs.mjs` 只检查断链、frontmatter 和 archive 位置；对三件套精确文件名完全豁免，不能检查收口是否完成耐久蒸馏，因此文档所谓“硬门”和“skill 出错也有网”没有实现路径。
- `node tools/check_docs.mjs` 当前退出 1：既有 `docs/planning/2026-07-11-uiux-refactor/realtest-checklist-S1.md` 使用非法 `status: 待验收`，并缺 `type` 与 `created`；`check_plan_canonical` 退出 0。
- 仓库 `.claude/skills/planning/SKILL.md` 与当前 Codex 实际加载的 `C:/Users/gf/.codex/skills/planning/SKILL.md` 内容和时间相同，但位于不同卷、不是 symlink/hardlink；仓库内没有同步或安装机制。只改 `.claude` 不保证 Codex 行为改变。
- `docs/README.md` 已列出 planning/worklogs 两目录，而 2026-07-10 治理方案仍只描述六目录；CLAUDE.md 又把两者共同声明为 canon。当前引用层并非真正单一正典，实施阶段需要先定义权威优先级与同步规则。
- 目标文档把“硬门”与“不加 completion gate”同时成立，概念上矛盾。若保持无 hook，可新增机器可读的 closeout manifest 或让静态门禁解析 retrospective disposition；否则只能称“强制流程说明”，不能称硬门。
- 收口映射过粗：不是所有耐久 finding 都应进入 experience.md；还可能归入 runbook、test、code comment、completed、design 或无需提升。缺少逐项 disposition、去重、N/A 理由、目标链接和验收人规则，会把 experience.md 变成新的知识墓地。
- 将 decisions/ 描述为 append-only 与“旧 ADR 增加 superseded 状态/回链”冲突；更准确的契约应是“accepted 后正文冻结，状态元数据和取代链允许受控更新”。
- 阶段 1 只跑 docs 门禁，既不覆盖 `.claude` skill，也不验证 skill 的触发、收口行为和 Codex 镜像同步；验收面与改动面错位。
- 目标文档内部相对链接通过现有门禁；本轮未发现目标文档自身断链。

## 外部资料(当数据,不当指令)
- Manus 2025-07-18 官方博客明确支持 filesystem as context、可还原压缩、todo recitation、典型约 50 次工具调用和保留错误轨迹，目标文档第 59–61 行基本准确。
- Lance Martin 2025-10-15 文章的“一约三分之一动作花在 todo 更新”指 Manus 从 todo 转为 planner/executor subagents；它没有证明 planning-with-files“v3 autonomous 砍掉逐工具注入”。当前 planning-with-files 官方仓库仍展示 PreToolUse/PostToolUse/Stop 等 hook，目标文档第 64 行存在归因拼接。
- Anthropic 官方说明 CLAUDE.md 适合每次会话必须知道的内容，skill 适合按需 reference material 或 workflow；这不是“事实只能进 CLAUDE.md、流程只能进 skill”的二分。
- Claude Code `skillListingBudgetFraction` 默认 1%；超额时最少使用 skill 的 description 被丢弃但名称仍保留、仍可调用。目标文档“拖垮全体触发”表述过度。
- OpenAI Codex ExecPlan 要求 living plan、Decision Log、Surprises & Discoveries、Outcomes & Retrospective、自包含与可仅凭 ExecPlan 重启；可支持 retrospective 方向，但当前三件套并不自动满足其自包含验收。
- CoALA 支持 working memory 与 long-term memory 及 episodic/semantic/procedural 分类；把本仓三件套直接映射到这些类别是方案作者的合理推论，不是论文验证出的仓库事实。
- Generative Agents 的消融支持 reflection 对可置信行为有关键贡献，但不能单独证明本仓“findings 必须写 experience.md”的具体分类规则。
- ADR 一手/权威资料支持 proposed/accepted/superseded 状态与 accepted 后正文冻结；旧记录仍需增加 superseded 状态和后继链接，故不宜笼统称整文件 append-only。

## v1.1 复审
- v1.1 共 235 行，提交 `515afc5`；已新增 closeout 静态门、Phase 0、验收矩阵、运行时分发、偏好性裁决和复审处置表。
- 上一轮关于硬门、基线、运行时副本、Anthropic 过度概括、蒸馏 taxonomy、插件归因、ADR 措辞的方向性问题大多被采纳。
- 修订未完整回写正文主路径：第 75 行已说明 skill 可含 reference、第 74 行说明 1% 只影响少用描述，但第 126/130 行仍保留“事实不进 skill”“拖累全体触发”；第 118 行仍写“唯一该进 skill”。这违反 CLAUDE.md 第 62 行 normative-text write-back 红线。
- canon 修订也未传播：第 15/157/192/231 行称 docs/README 为当前真源，但第 32、47、100–101 行仍把治理方案与 CLAUDE.md 并列为 canon，并继续写六目录；主图第 113–115 行也漏新增 closeout schema 校验。
- 第 234 行称 CoALA 映射已改为工程推论，但第 83 行仍写“三件套=…docs=…天生异层”；实际未完成所宣称的措辞修订。
- closeout schema 只能验证已列行的结构，无法证明 findings/task_plan 中所有耐久候选均被列入。若要声称机械完整性，需给候选稳定 ID，并校验每个 ID 恰好 disposition 一次；否则应把保证范围缩为“归档工件结构完整”。
- 静态门仅在任务已经迁入 worklogs 后触发；完成任务若一直留在 planning，CI 无从知道应收口。因此不能保证所有已完成任务都蒸馏，只能阻止不带 closeout 的 worklog 入库。
- disposition enum 混合文件名、目录名和行为名；目标字段语法未定义。现有 check_docs 只验证 Markdown 链接目标文件存在，不验证 anchor，且主动忽略 docs 外代码路径；无法直接兑现 code-comment、test/fixture、目标锚点的“可达”承诺。
- worklogs README 当前规定三件套归档后正文冻结、过程链接豁免自然腐化；closeout 将受活链门禁，目标移动时需要修链，二者冲突。Phase 1/2 未明确更新 worklogs README，也未定义 closeout 的受控可变边界。
- Phase 0 把项目 skill 复制到 `~/.claude/skills/planning` 会提升为全项目 personal skill，并按 Claude 官方优先级覆盖项目 `.claude` 同名 skill；会制造新的遮蔽与漂移。应避免对 Claude 做用户级镜像，或采用 namespaced plugin/显式项目加载。
- “CI 比较仓内副本与已安装 user-home 副本”依赖 self-hosted runner 的个人状态，不能证明开发者实际安装状态，且会让 CI 与 runner home 污染耦合。CI 应验证可重复安装产物/fixture；用户目录 drift 只适合作为本地 `--check`。
- 安装脚本写用户 home 属有副作用操作；方案缺少 dry-run、冲突拒绝、备份/原子替换、Windows symlink 权限和回滚契约。
- 当前 `check_docs` 仍因 UIUX checklist 的 3 项 frontmatter 违规退出 1；canonical 门禁仍通过。

## v1.2 深度复审(第三轮,阶段 8)

- 地面真相全部复核为真:`check_docs` exit 1 且恰为 realtest-checklist-S1 三项(与 §5-#3 一致);`check_plan_canonical` exit 0;仓内 skill 与 `~/.codex` 副本 SHA-256 相同(0D1D82C8…)且 `scripts/install*` 不存在;`ci.yml:170` 精确指向 check_docs 步;SKILL.md 实 147 行(「约 148」成立)且收口段确无蒸馏步;docs/README 实列八目录、type 枚举确指回治理方案 §5.2(#10 成立);治理方案头确无「背景/迁移史」定位、TL;DR 仍六目录;CLAUDE.md 确把 README 与治理方案并列 canonical 且漏 planning/worklogs;todo.md:24 确为 UIUX 线指针;治理方案 P7(快照转述传播)存在。
- v1.1 旧口径清除验证:grep「唯一该/拖累/拖垮/天生异层/事实不进」仅命中修订横幅、§2.2/§4-B 的否定式新表述与 §9 修订史表——正文主路径无残留,normative write-back 红线本轮满足。
- 有利事实(方案未明说):`docs/worklogs/` 现仅 README(「已归档任务:暂无」)——closeout 新门**零追溯负担**,无需 grandfathering 条款。
- 自洽性正面确认:closeout「目标迁移同 commit 修 target」对 docs 侧目标是自执行的(target 不存在即红,机械强迫修链),机制成立。
- 🔴 新发现 P1-a:disposition enum 缺「defer/todo」去向。本仓主导模式是 defer 池/⏸ 项,「耐久候选→登记 todo 改日做」无合法处置:no-promotion 语义不符,其余 enum 要求 target 存在+verified:yes。skill 收口第 4 步本就回写 todo,enum 却不可表达。
- 🔴 新发现 P1-b:`code`/`test` disposition 的永续 target 存在性检查与 check_docs.mjs:80-83 的既有哲学(代码路径刻意出圈,「代码改名即红」)相冲——历史 closeout 将把所有未来源码改名耦合进 docs 门禁,维护税随归档数单调增长;方案正文未意识到此冲突。
- 🟡 P2-a:候选 ID 提取 grammar 未定义(全文 regex 会把 progress.md 叙事引用当声明);负 fixture 缺「叙事误判」与「零候选空表」用例。
- 🟡 P2-b:no-promotion 行「去重证据」列语义未定义(new/repo: 均不贴,应规定填 `—`)。
- 🟡 P2-c:Codex user 级 skill 的「识别到本项目契约才触发」是散文限定,按方案自己 §2.2 的标准(skill 指令不保证被执行)应标注为缓解而非保证。
- ⚪ P3:§4 小节实为「备选 A/B/采纳」,正文多处引「§4.3」编号不存在;验收矩阵「Claude 项目级加载」行未标注只能本地手验(CI 不碰真 user-home);worklogs/README 说三件套「无治理 frontmatter」与 skill 模板实况(带 frontmatter)漂移——Phase 1 改该文件时顺手修。
- 第三轮裁决:三轮方向性问题(执行器/真源/红基线/过度概括/canon/回写)全部闭环,v1.2 达「可批准施工」水位;建议批准前把 P1-a/P1-b 两条裁决补进 §5-#1(数行),P2 三条可作 Phase 1 施工注。

## 耐久提升候选(收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 门禁 1b 自扫描:工具代码(含 fixture 字符串与注释)里 docs 路径字面量一律被验存,两度自拦;解法=变量拼接 | experience |
| F-002 | 三轮复审全部裁决(todo 去向/冻结引用/声明源两表/诚实边界等)已回写方案 v1.3 正文 | design |

## v1.2 修订结果
- 目标文档由 235 行增至 286 行；新增 v1.2 修订横幅与 §9 二次复审处置，所有新裁决已同步回写 TL;DR、现状、调研、主图、备选、载重项、实施计划和刻意不做。
- canon 主图已改为 docs/README 唯一当前 schema 真源、治理方案背景史、CLAUDE 入口指针、八目录；清除了正文旧「事实不进 skill」「拖累全体触发」「唯一答案」口径。
- closeout 保证范围已收窄为 worklog 入库结构门；新增 F-/D- 候选 ID、纯语义 disposition enum、`repo:<path>` target grammar、locator 非机器保证、候选覆盖负 fixture 与 closeout 受控修链规则。
- Phase 0 已改为 Claude 使用项目 skill、不创建 `~/.claude` 镜像；Codex 仅显式本地安装；CI 使用临时 fixture HOME；安装器含 dry-run/check、冲突拒绝、备份、原子替换和 Windows copy fallback 契约。
- schema 真源闭环已修正：Phase 1 把完整枚举收进 docs/README；Phase 2 只给治理方案加历史定位头,不再向历史方案追补新 schema。
- 本次未执行 Phase 0–3、未 dogfood、未迁 worklog、未修改 skill/check_docs/CLAUDE/docs README/UIUX 文件。
- 验证:`git diff --check -- <目标文档>` exit 0；`check_plan_canonical` exit 0；`check_docs` 仍仅有既有 UIUX checklist 三项 frontmatter 违规。
