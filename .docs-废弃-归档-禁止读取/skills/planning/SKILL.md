---
name: planning
description: "为多阶段、≥5 步或跨会话/上下文压缩任务维护工作记忆，支持建档、接续及授权收口。自动启用须存在 docs/planning/ 或 docs/README.md 声明工作记忆契约；用户明确要求时启用。"
user-invocable: true
---

# 工作记忆

## 建档

1. 读取 `docs/README.md` 的相关约定；字段与分类规则见 `docs/runbooks/closeout.md`「文档契约」。
2. 优先挂靠既有工作线。`line` 取 `docs/lines/<线名>.md` 的文件名，不含 `.md`；确需新线时先建实体，任务名不自动成为线名。
3. 创建 `docs/planning/<创建日 YYYY-MM-DD>-<任务名>/`。任务名转 NFC，移除 `\ / : * ? " < > |`，空白换 `-`，去掉末尾点；重名追加 `-2`、`-3`。接续已有任务则复用原目录。
4. 按 `.worklog/templates/` 建档，填实目标、阶段和下一步，告知目录。使用 `worklog-kit start <任务>` 后，将生成文件的 `line` 改为选定工作线，补齐未展开字段。

多阶段或 ≥5 步任务使用三件套；小型跨会话任务可用 `checkpoint.md`（CLI：`worklog-kit start <任务> --mode lite`）。任务扩大、需要正式登记候选或准备收口时，在原目录展开为三件套，迁入有效内容与已有编号，移除被替代的 checkpoint。中途达到触发条件时补建并回填进度。

## 维护与接续

| 文件 | 内容 |
| --- | --- |
| `task_plan.md` | 目标、验收条件、阶段状态、关键决策表 |
| `findings.md` | 需求约束、带来源的发现、长期保留候选表 |
| `progress.md` | 前情、进度、验证命令与结果、错误及修法、阻塞和下一步 |

- 重要进展发生时更新对应文件，同一信息保留一处。发现写入 `## 发现`，决策和候选直接写入对应表格；CLI `note` 不替代候选登记。
- 候选使用 `F-NNN`／`D-NNN`，两类各自全仓递增：用 `next-id`，或扫描 `docs/**` 中全部 `F-\d{3,}`／`D-\d{3,}`（含围栏）取各自最大值加一，至少三位。
- 交接或上下文压缩前更新 `progress.md` 顶部不超过 10 行的“前情”。日志过长时保留最近两段，旧段迁入带完整元数据的 `progress-archive.md`。
- 接续先读计划、发现全文及 progress 前情和最近两段；lite 读 checkpoint 全文。核对工作区，确认当前阶段、阻塞和下一步。
- 未指定任务时列出 `docs/planning/`；多个任务无法确定时询问用户。CLI 可用 `list`、`resume`、`checkpoint` 替代对应操作。

## 收口

仅在用户明确要求“收口／归档任务／closeout”时执行；更新文档或完成阶段不构成收口授权。

1. 按 `docs/runbooks/closeout.md` 补齐候选并实际完成处置，填写 `closeout.md`；每个候选恰好一行，核验后才填 `verified: yes`。
2. 三件套改为 `status: snapshot`，保留 `type: working-memory` 和原 `created`。迁入 `docs/worklogs/<原目录名>/`，保留创建日期与重名后缀，不覆盖已有目录。
3. 在 `docs/worklogs/README.md` 登记真实收口摘要，同步本线状态及待办索引。
4. 按手册自检；有 CLI 时运行 `worklog-kit check`。记录实际执行的验证，不把手工核对或 dry-run 写成门禁通过。

CLI 收口仅在 `--dry-run` 的目标目录符合上述命名时使用，并传入 `--summary "真实摘要"`；否则手工迁移。CLI 已完成的状态切换、迁移和登记不重复执行。

归档后三件套正文及 closeout 的候选 ID、disposition、理由冻结；文档目标迁移时同步更新 target。提交、push 按用户授权执行，收口不自动授权提交。
