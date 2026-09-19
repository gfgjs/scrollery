---
status: 快照
type: 工作记忆
line: worklogs日期前缀修正
created: 2026-08-24
---

# 任务计划:worklogs日期前缀修正

## 目标
2026-08-14 收口时 worklogs 下 42 个任务目录被统一冠 `2026-08-14-` 前缀;核实每个任务的真实创建日期,把目录名、todo.md/completed.md 等全部引用、以及 worklog 文件内部自引用一并改回真实创建日期。

## 当前阶段
已完成(收口 2026-08-24,未 commit——待用户审后提交)

## 阶段

### 阶段 1:核实真实创建日期
- [x] 逐目录取证:task_plan.md frontmatter `created:` + git 历史(原 planning 目录名),44/44 双源一致
- [x] 产出「目录 → 真实日期」对照表(rename-map.tsv,42 改 2 保持)
- **状态:** done

### 阶段 2:重命名目录
- [x] `git mv` 42 个目录(37 直接改名;5 个含子目录的因沙箱「含子目录不可 rename」限制改用逐文件迁移 + rmdir 空壳,见 experience §52)
- [x] 事故恢复:日志能力重构/closeout.md 被探针误删,`git show HEAD:` 恢复并补齐暂存
- **状态:** done

### 阶段 3:更新全部引用(3 子代理并行)
- [x] docs 主文档 51 文件 131 处(todo 10 / completed 27 / spec 7 / status 10 / reviews 2 / designs 10 / lines 60 / 在施 planning 5)
- [x] worklogs 内部 44 文件 85 处(README 42 + 43 文件各 1;含两个保持目录内的对外引用)
- [x] 源码注释 15 文件 16 处(crates 2 / src-tauri 9 / src 前端 4)
- [x] 通配符样板 32 处(completed.md 1 + lines 31:「收口播种自 worklogs/2026-08-14-*/」改指收口波事件,不再依赖路径)
- [x] 根因修正:planning SKILL.md「日期换成收口日」→「日期用任务创建日」
- **状态:** done

### 阶段 4:验证与收口
- [x] 全仓 grep:剩余 `2026-08-14-` 仅 2 个保持目录的自引/互引、审查报告文件名、各类 frontmatter id(45 文件,逐类查明合法)
- [x] worklog-kit index 门通过(目录表 + worklogs 登记双向一致);check 门 48 处红均为 CI 既有红、与本次改动集零交集
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 目录名日期 = 任务创建日(非收口日) | 用户裁决;与 08-14 前既有实践一致;收口日命名在同日批量收口时全撞同一前缀 | D-001 |
| 2 个真 08-14 创建的目录保持原名(完成可收口任务核实与回写、过度工程化审查) | 双证据源均显示创建日=2026-08-14 | D-002 |
| README「已归档任务」收口日列(2026-08-14)保留,仅改目录名列 | 收口日是事实;目录名才是本次订正对象 | D-003 |
| lines 样板「收口播种自 worklogs/2026-08-14-*/」改指收口波事件 | 改名后 glob 失真;事件溯源不依赖具体路径 | D-004 |
| 根因行 SKILL.md 同改 | 裁决推翻规范文本,正文同批更新,防复发 | D-005 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 含子目录目录 git mv 全部 Permission denied | 停 fsmonitor(无效)、PowerShell Rename-Item(同败)、icacls 对比(无差异) | 探针复现=沙箱系统性限制;逐文件 git mv + rmdir 空壳 |
| 移动探针用了真实 closeout.md,恢复时以探针名回移,后续清理误删内容 | — | `git show HEAD:<旧路径>` 恢复 + fix_refs + 补暂存;教训入 experience §52 |
