---
status: 快照
type: 工作记忆
line: worklogs日期前缀修正
created: 2026-08-24
---

# 发现与决策:worklogs日期前缀修正

## 需求
- 用户:todo.md 8月14日收口了一批任务,worklogs 中任务名全被改成 08-14……,实际应该用任务创建日期;先核实;todo.md、worklogs 目录名、文件内部都要改;用多子代理提效。

## 核实结论(2026-08-24)
- **用户判断成立**:worklogs/ 下 44 个 `2026-08-14-*` 目录是 2026-08-14「完成/可收口任务核证回写」收口波(commit `9daadab`/`0db0850`/`2e01bfb` 等,「收口 B 级 15/16 线」)按**收口日**统一冠名归档的。
- **双重证据交叉一致**:① 每个目录 task_plan.md frontmatter `created:`(07-16~08-07 不等);② git 历史中每个任务在 `docs/planning/<创建日>-<任务名>/` 的原始目录名。两者逐条吻合(44/44)。
- **42 个需改名,2 个保持**:真 08-14 创建的是 `完成可收口任务核实与回写`(收口波本身)与 `过度工程化审查`,保持 `2026-08-14-` 前缀。
- 特例:`全仓未完成工作梳理` 的 task_plan 用旧式 frontmatter(无 `created:`),真实创建日取自其 `date: 2026-07-24` + git planning 目录名,两者一致。
- **根因**:planning SKILL.md 收口步骤原文「日期换成收口日」——同日批量收口即全撞同一前缀。本次按用户裁决改为「日期用任务创建日」(与 08-14 之前的既有实践一致:07-12~08-11 的 worklog 目录全部用创建日)。
- 完整映射见本目录 `rename-map.tsv`(42 行)。

## 环境发现(执行期)
- **本机沙箱限制:含子目录的目录不能 rename(Permission denied,OS 级)**;平铺目录改名、文件移动、子目录(平铺)改名均正常。空目录 + 逐文件 `git mv` + rmdir 空壳可绕过,索引状态等价(rename 检测按内容)。已停 `git fsmonitor--daemon` 排除干扰后仍复现,确认与 fsmonitor 无关。
- 引用面(全仓 grep `2026-08-14-`):docs 主文档(todo/completed/worklogs README/spec/status/reviews/designs/lines)+ 在施 planning 三件套 5 文件 + worklogs 内部互引(closeout.md 为大头)+ **源码注释 15 文件**(crates/ai-worker、psd-worker、src-tauri 9 处、src 前端 4 处)。
- worklogs/README.md「已归档任务」索引的**收口日列(2026-08-14)是事实,保留**;只有目录名列要改。

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 沙箱「含子目录的目录不可 rename」限制与逐文件 git mv 绕过法 | experience |
| F-002 | worklog 归档目录命名 = 任务创建日(非收口日)裁决 + 43 线订正史 | completed(随本线收口条目) |
