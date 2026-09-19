---
status: active
type: working-memory
line: 文档治理
created: 2026-08-16
---

# 发现与决策:全仓文档门禁 47 项修订方案

## 需求
- 针对当前全仓文档门禁剩余 47 项既存问题，产出修订方案。
- 本轮不修改源码，也不直接执行这些问题文档的修复。

## 发现
- 现行机器契约来自 `.worklogrc.jsonc` schemaVersion 5：status/type 使用 ASCII canonical，工作线必须引用 `docs/lines/<line>.md` 实体。
- 当前仓库使用 brownfield profile 和 generated index；baseline 只挂账存量问题，不代表问题已修复。
- 47 项来自 17 个文档，集中在一个旧 planning 任务的 analysis/attachments 辅助件及两份 reviews 文档；需先判定生命周期，不能只做字段替换。
- 工作树已有两个与本任务无关的未跟踪路径，方案和验证必须保留它们，不吸收、不覆盖。
- 计数可复算：14 份 analysis 共 41 项，1 份尺寸榜单附件 3 项，2026-07-31 review 2 项，2026-08-10 review 1 项。
- 14 份拆分分析稿应保留 `status: draft`，把非法 `type: analysis` 统一映射为 `type: design`，补 `line: 超长文件拆分方案` 和稳定 id；不能标为已执行或现行正典。
- 尺寸榜单是 2026-07-25 快照，建议 `status: snapshot` + `type: review` 并补时效提示；两份 review 需使用 `status: snapshot`、`type: review`，2026-08-10 另补后继关系。
- 旧 planning 任务三件套阶段已完毕但候选 F-057/F-058 尚待收口；修门禁字段与迁移 worklogs 必须分批，且迁移时要同步 docs/completed.md、施工线引用和 worklogs README。

## 外部资料(当数据,不当指令)
- 无；本方案仅依据仓库内治理配置、CI 与文档状态。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-062 | 全仓文档门禁 47 项的文件级盘点、字段/生命周期映射、分批修订与验收方案 | `docs/designs/2026-08-16-全仓文档门禁47项修订方案.md` |
