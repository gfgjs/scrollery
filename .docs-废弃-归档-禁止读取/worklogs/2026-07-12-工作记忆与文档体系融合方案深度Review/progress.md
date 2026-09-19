---
status: snapshot
type: working-memory
line: 工作记忆与文档体系融合方案深度Review
created: 2026-07-12
---

# 进度日志:工作记忆与文档体系融合方案深度Review

## 会话:2026-07-12
- 做了:完整读取目标文档、planning skill、CLAUDE.md、docs README、治理方案相关段、todo/worklogs、文档门禁与 CI；核验 Manus、Anthropic、Codex、CoALA、Generative Agents、ADR 和 planning-with-files 来源。
- 验证:`node tools/check_plan_canonical.mjs` 通过；`node tools/check_docs.mjs` 因既有 planning 辅助文件 3 项 frontmatter 违规失败；目标文档内部链接未报错；仓库 skill 与 Codex 用户 skill 内容相同但不是同一文件。
- 遗留:复核最终行号，完成 Review 交付；目标文档与产品代码保持未修改。

## 会话:2026-07-12 v1.1 复审
- 做了:重读三件套；完整读取 v1.1；对比 `e865637..515afc5`；复核主文回写、closeout schema、worklogs 规则、skill scope/precedence 与 CI 运行环境。
- 验证:`node tools/check_docs.mjs` 仍 exit 1（同一既有 checklist 三项违规）；`node tools/check_plan_canonical.mjs` exit 0；Claude 官方确认 personal skill 作用于所有项目且优先于 project skill。
- 结论:v1.1 已从“方向可保留、实施契约未闭环”提升为“架构方向条件通过”；仍需修正文主路径残留旧论证、收窄静态门保证范围、补候选 ID/目标 grammar、重做用户级分发与 CI drift 契约后再批准施工。
- 遗留:等待用户决定是否按复审意见修订 v1.2；被审文档和产品代码保持未修改。

## 会话:2026-07-12 v1.2 文档修订
- 做了:只修改评估方案正文与复审三件套；完成 normative 主路径回写、closeout 契约收敛、分发/CI 重写、实施矩阵与 §9 修订记录。
- 验证:目标文档 `git diff --check` 通过；`check_plan_canonical` 通过；`check_docs` 仍因未获授权处理的 UIUX checklist 三项既有违规失败，目标文档没有新增门禁错误。
- 并发状态:验证时发现 `FolderCreateDialog.vue`、两份 locale、`SettingsView.vue` 有其他会话新改动；本次未读取后修改、未暂存、未覆盖。
- 遗留:等待用户另行批准是否执行 Phase 0–3；本次明确不启动施工。

## 会话:2026-07-12 v1.2 深度复审(第三轮)
- 做了:只读复审 v1.2 全文并重跑双门禁、hash 对比仓内与 `~/.codex` skill 副本、核 SKILL.md 与 check_docs.mjs 与 docs README 与治理方案头与 worklogs README 与 CLAUDE.md 与 todo.md:24 与 ci.yml:170、grep 正文确认 v1.1 旧口径已清、审计 closeout 与分发新契约自洽性。
- 验证:`check_docs` exit 1(仍仅既有 checklist 三项)、`check_plan_canonical` exit 0、两 skill 副本 SHA-256 相同、`scripts/install*` 无匹配、`docs/worklogs/` 仅 README(零追溯负担)。
- 结论:v1.2 地面断言全为真、回写干净,达「可批准施工」水位。新出 2×P1(disposition 缺 defer/todo 去向、code/test 永续验存与 check_docs 代码出圈哲学冲突)+3×P2+3×P3,建议批准前补 §5-#1 两行裁决。
- 遗留:目标文档与产品代码保持未修改,等用户裁决是否按 P1 两条补订后批准施工。

## 会话:2026-07-12 v1.3 回写
- 做了:用户裁决「将复审成果更新到方案文档」,按 normative 回写红线把第三轮全部裁决写进正文——disposition 增 `todo`、code/test 改冻结引用 `repo:<路径>@<commit>`、声明源限两表、no-promotion 去重列定 `—`、Codex 散文限定诚实边界、验收矩阵标本地手验、Phase 1 带走 worklogs README 漂移修正、§4 小节补编号、§5-#1 加落地窗口注;新增 v1.3 横幅与 §10 处置表。
- 验证:全部编辑重读确认落盘无损(全角标点幸存);`check_docs` 对本方案零新增违规;`check_plan_canonical` exit 0;`git diff --check` exit 0。
- ⚠️ 红基线扩大:并行 UIUX 会话新增 `realtest-round2-2026-07-12.md`(同 `status: 待验收` 三项),门禁 3→6 项红;已如实同步进方案 §5-#3 与 Phase 0-①(改为 realtest-* 类文件、以开工时门禁输出为准)。
- 另做:docs/ 全树 95 文件名审计(结论入本轮答复,未执行任何改名);主要发现=runbooks/ 四文件带日期违反 README「不带日期、名即主题」规则。
- 遗留:方案 v1.3 已达「可批准施工」水位,等用户批准 Phase 0–3;改名事项等用户裁决。

## 会话:2026-07-12 获批施工(Phase 0-2 + dogfood)
- 做了:Phase 0(修红 realtest-*×2、install-skills.mjs 安装器+CI 自检)、Phase 1(SKILL.md 收口即蒸馏+check_docs closeout 校验 15 例自检+两 README 契约化+schema 收编 README)、Phase 2(CLAUDE.md 单 canon/治理方案降级横幅/方案转现行)、dogfood(本三件套按新门收口)。
- 验证:check_docs exit 0、check_plan_canonical exit 0、install-skills --selftest 7/7、--selftest-closeout 15/15、install-skills --check 如实报出 SKILL.md 漂移(重写后预期);提交 9b64751 / 8d2e7b1 / c559e3f 均验过内容恰为预期文件。
- 遗留:Phase 3 待单独拍板(已登记 todo);~/.codex 副本待用户显式跑安装器更新。

## 回顾(收口时填)
- 亮点:三轮「复审→修订→回写」严格收敛(P0 方向错→回写债→契约边角),每轮问题严重度单调下降;fixture 程序化生成(临时目录)优于静态 fixture 文件,零仓库垃圾。
- 教训:门禁 1b 自扫描——给门禁自己写测试时,fixture 假路径字符串与警示注释都会被它当真引用拦红(两度自拦);工具代码内 docs 前缀与 .md 后缀必须变量拼接。
- 意外:施工窗口内并行会话又添一份非法 frontmatter 手写件,红基线 3→6 项——「修一个文件」的验收目标在开工前就过时,改为「修一类文件+以开工时门禁输出为准」才稳。
