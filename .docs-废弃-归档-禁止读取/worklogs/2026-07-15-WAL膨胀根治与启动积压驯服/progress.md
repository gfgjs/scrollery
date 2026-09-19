---
status: snapshot
type: working-memory
line: WAL膨胀根治与启动积压驯服
created: 2026-07-13
---

# 进度日志:WAL膨胀根治与启动积压驯服

## 会话:2026-07-13(23:10 起,路径:d:\photoapp\picasa-next,后证实为项目迁移前的旧路径)
- 做了:①核实诊断稿引用的全部代码事实(全属实)与两个关键反例(wal_autocheckpoint 默认在场;派生线 auto-start 已延迟+让步)②试写 docs/planning/ 时撞上 D 盘 NVMe 掉线,层层排除软件因素后定性硬件,三件套临时保全 C 盘 scratchpad③用户重启恢复后核实 D 盘写入/git 完好④取证生产库日志:当日开机 WAL≤10.4MB、checkpoint 0–3ms、boot→Ready 253–407ms → 证伪「WAL 膨胀致开机慢」⑤按用户澄清(先分析不施工)交付 P1–P5 逐项评审结论,三件套写入 d:\photoapp\picasa-next\docs\planning\。
- 验证:无代码改动,无门禁运行。git 只读检查 dev@81cef21 完好、工作区与会话开始一致、仅 1 未推送提交。
- 遗留(当时视角):等用户裁决——①P1 一行卫生改做不做②「真实慢源测量」立不立项。

## 会话:2026-07-15(真实日期;发现路径歧义并收口)
- 做了:用户回复「暂时不改了,收尾并 commit」。执行 git status 时发现 `d:\photoapp\picasa-next` 已不是 git 仓库(`.git` 空);经 Read/Glob 报错信息确认真实工作目录已是 `D:\workspace\scrollery`(项目已完成 picasa-next→scrollery 本地目录改名迁移,另有独立三件套 `docs/planning/2026-07-15-本地目录改名picasa-next至scrollery/` 追踪该迁移本身)。核对真实仓库发现:**并行会话已经独立走完了几乎相同的评审路径**——P1 已作为「通用零风险卫生改」直接提交(`b24b196`,未等待本任务的显式用户 go,但结论与裁定一致,予以承认)、NVMe 硬件事故已蒸馏进 `docs/experience.md` §13(commit `332e262`)。本会话据此收口:①task_plan.md 阶段 1 标记 completed 并记录该提交的落地方式;阶段 2 按用户「暂时不改了」标记 shelved,登记为 `docs/todo.md` 新增 R 线(字母登记表 README.md 同步取号,下一空闲字母推进至 S)②`docs/experience.md` 新增 §15(F-001「评审补齐周期机制类提案先查编译期默认行为」+ F-003「硬件退化机器上性能归因先排硬件」),§13 追加一句 Get-Disk 枚举缺失 vs Get-Volume/Get-PhysicalDisk 仍报常的判据(F-002,与已有内容合并,非全新落点)③本文件与 task_plan/findings 三件套按收口仪式定稿,新增 closeout.md,`git mv` 归档至 `docs/worklogs/2026-07-15-WAL膨胀根治与启动积压驯服/`。旧路径 `d:\photoapp\picasa-next\docs\planning\...` 下的重复文件未清理——该路径已脱离版本控制,清理归属迁移任务本身,不在本任务范围内。
- 验证:experience.md/README.md/todo.md 编辑后未跑 check_docs(将在最终 commit 前一并跑);无代码改动本身。
- 遗留:「真实慢源测量」(D 盘 I/O 退化嫌疑)留在 `docs/todo.md` R 线,用户后续需要时再启动;闭环见 closeout.md。

## 回顾(收口时填)
- 亮点:探针先行(P5 早于本次评审就已埋点)让「诊断稿听起来完全合理」的方案在动工前被真实数据推翻,避免了一次无效施工;两个独立会话(本任务 + 并行会话)在互不知情的情况下对同一诊断稿得出了一致的技术裁定,交叉印证了评审结论的稳健性。
- 教训:发现工具报错("not a git repository"、Read 报错里带出的 cwd)与既有假设(自己一直在正确的项目路径下工作)冲突时,要立刻把报错信息本身当第一手证据去核对路径,而不是重试同一命令;本任务因此浪费了一整轮在已脱离版本控制的旧路径上的读写。
- 意外:①「WAL 膨胀治理」评审进行中,被同一台机器的新硬件故障(D 盘 NVMe 掉线)现场打断——该故障恰好提供了体感慢的替代解释,反转了整个方案的前提;②收口时才发现整个任务在另一条并行会话里已经几乎重跑了一遍并已提交代码,本任务的价值收窄为「交叉验证 + 把两侧发现合并进同一份权威记录」。
