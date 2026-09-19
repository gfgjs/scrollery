---
id: 2026-07-17-closeout
status: active
type: runbook
line: 文档治理
created: 2026-07-17
---

# 收口手册(无 AI 也能走通 start→closeout)

> 本手册把 `/planning` skill 的收口仪式**手册化**,让不用 AI agent 的贡献者照样能完成
> 「建三件套 → 施工 → 蒸馏归档」的完整长任务生命周期(R2-M2)。门禁(`worklog-kit check`)
> 是唯一的机械强制点,不依赖任何 agent。

## 一、开一条长任务(start)

1. 建目录 `docs/planning/<今天 YYYY-MM-DD>-<任务名>/`(中文任务名可用,剔除 `\ / : * ? " < > |`)。
2. 从 `.worklog/templates/` 拷 `task_plan.md`、`findings.md`、`progress.md` 进去,填实 frontmatter 与内容(该目录由 `worklog-kit init` stamp 进本仓;worklog-kit **包内**的 `templates/` 在本仓不存在)。
3. 任务属**新工作线**的,同批建线实体 `docs/lines/<线名>.md`(一句话使命,`type: line`、`line: <自身文件名>`)——**开线 = 新建实体**,收口文档的 `line` 引用要靠它解析(引用门验存)。
   **关线 = 实体改 `status: archived` 留在原地**(墓碑),正文加横幅指去向;**不要**删除或移走该文件——老 snapshot 文档的 `line` 引用与撞名保护都靠它。并线可加 `supersededBy: <新线实体 id>`(新线实体对应声明 `supersedes`,门禁验成对)。现役(draft/active)文档不得再引用已关的线——同 commit 把它们的 `line` 迁到去向线。
4. commit(显式 pathspec)。`ls docs/planning/` 即索引,无需登记。

## 二、施工中(work)

- 阶段翻转 → 更新 `task_plan.md` 阶段状态与决策表。
- 有耐久价值的发现/决策 → **当场**在 `findings.md` 候选表编 `F-NNN`、在 `task_plan.md` 决策表编 `D-NNN`,宁多勿漏。
- 跑了验证 → 结果与证据记 `progress.md`,错误记错误账。验证行引用门禁末行时**先跑门后回填**:门须在内容全部落盘后才跑得出,先占位、跑门、回填真实末行,改动了再复跑(F-005)。

## 三、收口(closeout,一次做完)

> **收口时机由任务发起人拍板**:施工期回写三件套(§二)不是收口;确认本任务不再接续后,才走本节。
> AI 代跑同规:**仅限用户明示指令**(「收口」「归档任务」「closeout」),不得从「回写文档/更新三件套」推断——AI 至多提议收口,执行须经批准(`/planning` skill §收口同一契约)。

1. **补全候选账**:把该提升的项编好 `F-NNN`/`D-NNN`,`progress.md` 填「回顾」。
2. **逐候选处置并当场执行**:每个候选决定去向——
   - 写进 `docs/experience.md`(经验)/ `docs/decisions/`(决策)/ `docs/designs/`(设计)/ `docs/runbooks/`(手册)/ `docs/completed.md`(完成详注);
   - 登记滚动状态源(fixed 档 = `docs/todo.md`;generated 档 = 本线 `docs/status/<slug>.md` 分片,靶点按任务 `line` 求解,D-014——以 `.worklogrc.jsonc` 的 `todo` 处置为准);
   - 落代码注释或测试(用冻结引用 `repo:<路径>@<commit>`);
   - 确无价值 → 裁 `no-promotion` 并写 N/A 理由。
3. **写 `closeout.md`**:拷 `.worklog/templates/closeout.md`,每个已声明候选**恰好一行**(D-012;列序固定,见模板)。
4. `git mv` 任务目录到 `docs/worklogs/<收口日>-<任务名>/`。
5. 三件套 frontmatter `status` → `snapshot`。
6. 在 `docs/worklogs/README.md`「已归档任务」登记一行(含 `` `目录名/` ``)。
7. 回写滚动状态源的本线状态(fixed 档 = `docs/todo.md`;generated 档 = 本线 `docs/status/<slug>.md` 分片)。
8. `worklog-kit check` 应全绿(候选全覆盖 / disposition 合法 / target 验存或冻结引用 / verified=yes)。
9. commit(显式 pathspec)。

## 四、门禁会挡什么(你不做就红)

- 归档任务**三件套不齐**或缺 `closeout.md`;声明候选漏处置 / 重复处置 / 处置了未声明的候选。
- 处置表**列名/列数/顺序**不符固定 schema(按位置解构,列漂移会静默错位取值)。
- disposition 不在枚举;docs 类 target 不存在;`repo:` 路径**越出仓根**(`../`、绝对路径);code/test 未用冻结引用;no-promotion 缺 N/A 理由;`verified≠yes`(**no-promotion 也要 yes**)。
- 归档件缺文首状态横幅;活区文档缺 frontmatter 或 status/type 非法。
- 文档 `line` 无对应 `lines/<slug>.md` 实体(slug=剥 `(X)` 字母尾+剔非法字符+空格→`-`+NFC);存量值批量播种跑 `worklog-kit upgrade`。
- **图不变量**(不可 baseline 豁免):`id` 重号;`supersedes`/`supersededBy` 悬垂、自环或不成对(双向须互指);同一 `(line, authorityScope)` 出现两个 active+authoritative(双权威);声明了 `supersededBy` 却仍 draft/active(双活);现役文档引用已关线(见 §一.3 关线)。`status: superseded` 缺 `supersededBy` 也红(此条存量债可 baseline)。
- `worklogs/` 存在却缺 `README.md`;`.worklogrc.jsonc` 的 `dirs` 与目录职责表、实际目录三者不一致。

## 五、误收口回滚(归档只是 git mv,回滚便宜)

1. `git mv` 任务目录从 `docs/worklogs/` 迁回 `docs/planning/<原日期>-<任务名>/`。
2. 三件套 frontmatter `status` 翻回在施值;删除 `docs/worklogs/README.md` 里对应登记行;滚动状态源(`docs/todo.md` 或本线 `docs/status/<slug>.md`)的本线状态改回。
3. `closeout.md` 建议直接删(重写便宜,半旧的处置账才贵);留的话其 frontmatter 须自行合法(活区受 frontmatter 门全检)。
4. 已执行的蒸馏写入(experience/decisions 等)**内容仍然成立就留**——蒸馏错的才回退,回滚归档不等于回滚知识。
5. commit(显式 pathspec)。`worklog-kit check` 应全绿。
