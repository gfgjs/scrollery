---
status: 快照
type: working-memory
line: 无人值守清账波
created: 2026-07-21
---

# 任务计划:无人值守清账波

## 目标
清掉 docs/todo.md + MEMORY.md + docs/planning/ 各线遗留中**无需人拍板**的项;验收 = 全库 cargo test + vitest 绿,且每个新增施工项有独立 commit。触红线/需拍板/GUI 真机项一律转「未裁」,输出施工报告 + 决策清单两份产出。

## 当前阶段
阶段 5:接续(U-* 采纳落地),complete

## 阶段

### 阶段 1:盘点与基线
- [x] 直读索引面:todo.md 全文 + MEMORY.md + git log/status
- [x] 基线门禁:cargo test --workspace 835 过 / vitest 1336 过,**基线红名单 = 空**(仅两验收门;clippy 有已知 pre-existing 红 scan_commands.rs:96 manual_inspect,文档在案)
- [x] git 基线 dirty:三件套(2026-07-17-未完成工作梳理线)M ×3 + .bak/docs/status 未跟踪若干 → 这些路径本波不可动
- **状态:** complete

### 阶段 2:候选核实与分诊(四路侦察)
四路侦察+池核实完,可施工 6 项/未裁池见 findings
- **状态:** complete

### 阶段 3:分批施工(每批:委派 implementer → 复核 → 门禁 → commit → 回写)
批1(#01-#03)+批2(#06-#08)六 commit,两档复核全过零发现(唯 #02 深审 1 警告 1 存疑,已修/已裁)
- **状态:** complete

### 阶段 4:最终产出(施工报告 + 决策清单,同写 findings.md)
报告 A/B 已写入 findings.md「最终报告」节;总账 22 项=施工 6/裁决 2/未裁 10/阻塞 2/no-go 2
- **状态:** complete

### 阶段 5:接续(U-* 采纳落地,同日第二次会话)
决策清单 U-1..U-10 中 6 项(U-1/U-2/U-3/U-5/U-6/U-7)按阶段 4 推荐方向采纳施工,六独立 commit(cb696dd/5a97bd4/e3fe2b4/00f45bb/78a611b/473311c);U-4/U-8/U-9/U-10 维持挂起未变。用户裁决:docs 门 9 处 frontmatter 基线红转两线(图片编辑线/日志重构线)各挂一条移交自办。文档回写(task_plan/findings/progress/todo)+ docs 门实测(worklog-kit check/index)完。
- **状态:** complete

## 分诊闸门(自动施工须全满足)
① 方向唯一 ② 不动公开契约(IPC/DB schema/磁盘格式/用户可见默认/权限)③ 可验证 ④ 单提交可 revert、无迁移 ⑤ ≤2 文件域、≤300 行(同事项累计 ~500 行封顶)。踩 MEMORY 红线(勿翻案/勿回退/defer 勿自动开工)即停。

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 基线红名单=空(两验收门全绿) | phase-closer 实跑 835+1336 全过 | |
| #02 展示形态不动,迁移收窄为变更同步+epoch | 无人值守不改用户可见形态 | |
| 池类维持既有 Boy Scout 随线并入裁决,仅清 chore 档确证项 | 避免与既有 defer 裁决冲突 | |
| U-1..U-3/U-5..U-7 六项全部采纳原推荐方向施工,U-4/U-8/U-9/U-10 维持挂起 | 深审复核后确认可自动化,无需额外拍板;后四项方向已定但需并入他线/专项/DB 波,不属本波范围 | |
| U-5(MediaFilter 双定义)考据为死码非有意双型,直接删 | ui.ts 版 2026-06-02 d71789d 引入即零消费,07-10 审查 C36 已判死码在先 | |
| docs 门 9 处 frontmatter 基线红转两线挂账,不代修 | 用户裁决(2026-07-21);铁律「docs 门禁基线红属收编线勿代修」 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|