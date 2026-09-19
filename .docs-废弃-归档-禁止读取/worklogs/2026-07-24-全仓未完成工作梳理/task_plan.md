---
title: 全仓未完成工作梳理
date: 2026-07-24
status: 快照
owner: 主会话(orchestrate)
---

# 全仓未完成工作梳理 · task_plan

## 目标

梳理全仓所有未完成工作,分「确定项 / 待裁项 / ⏸GUI·push」三类:确定且可安全施工的顺手做,拿不准的列清单交用户裁决。

## 硬约束(本线红线)

1. **避开 RAW 支持线**:`docs/worklogs/2026-07-24-RAW等格式照片支持/` 由并行会话在做,整条线不碰。
2. **工作树活跃勿动**:开工时 `git diff --stat` 显示 14 个 tracked 文件在改,且会话进行中数量持续增长(8→14→16),证并行会话正实时编辑。这些未提交改动**一律不 commit、不 stage、不修改**——碰即污染在飞工作。详见 findings §2。
3. 只 stage 显式路径;禁 `git add -A`;禁 push。

## 范围

- 覆盖:`docs/todo.md` 全量 + `docs/planning/*/progress.md` 各线余项 + `git status`/`git log` 工作树与提交态。
- 排除:RAW 线;他线 docs 门欠账(「余18他线欠账勿代修」,见 review-round2 记忆)。

## 依赖 DAG

```
摸底(git WIP · scout)  ┐
摸底(todo+近期线 · Explore)┤→ 主线核实·反陈旧 → 建三件套 → 输出清单+裁决点
```
纯文档任务,无代码施工阶段(工作树锁定,见约束2)。

## 决策表

| ID | 决策 | 结论 |
|----|------|------|
| D-1 | 工作树 14 处 WIP 是否顺手提交 | **否**。并行会话实时编辑,归属不清,提交即毁在飞工作。列为「待用户确认归属」。 |
| D-2 | 确定项是否有可安全施工者 | **无代码可动**。actionable 项全落入 WIP-live / ⏸GUI / 待裁 / 待push 四桶,无一可主线安全顺手做。 |
| D-3 | Explore 摸底清单陈旧项处置 | 主线按记忆索引+git log 核实反正:C1-C5 已 push、H 脱敏已落 61d125f,均从确定项剔除。 |

## 交付

1. 本三件套(task_plan / findings / progress)。
2. 未完成工作清单(findings §3)+ 裁决点清单(findings §4)。
3. 主线答复:说明无安全顺手做项及原因。
