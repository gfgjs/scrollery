---
id: 2026-07-15-本地目录改名picasa-next至scrollery-closeout
status: snapshot
type: closeout
line: 本地目录改名picasa-next至scrollery
created: 2026-07-15
---

# 收口处置:本地目录改名 picasa-next → scrollery

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | experience | repo:docs/experience.md | §16. 判断"能否安全改名/搬迁项目目录"要查内部自证点,不要搜文件夹名字符串 | new | — | yes |
| F-002 | experience | repo:docs/experience.md | §17. Claude Code 项目数据按工作目录绝对路径键控,改名/搬迁前须手动迁移 | new | — | yes |
| F-003 | no-promotion | — | — | — | 设计已在 `scripts/check-rename-gate.mjs` 头部注释里自文档化(三层设计 + 与 Copybara FORBIDDEN 的分工),重复记录进 experience.md 增量价值有限 | yes |
