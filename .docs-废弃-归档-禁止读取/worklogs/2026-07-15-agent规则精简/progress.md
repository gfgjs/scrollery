---
status: snapshot
type: working-memory
line: agent规则精简
created: 2026-07-15
---

# 进度日志：agent 规则精简

## 会话：2026-07-15
- 做了：读取 planning 与 caveman-compress 技能；核对 docs 文档契约、工作区状态、源文件元数据及相关记忆索引；逐行审计两份源规则，完成语义覆盖矩阵与双语精简稿。
- 验证：用户级 CLAUDE.md 为 34 行、SHA-256 `F45611380C528B117CFC898CBCAAAD0E4055ED942856A984FD3EC0A98D379A9D`；项目级 CLAUDE.md 为 67 行、SHA-256 `B69EAFF19DC9FCC5D096ADA9CB8BD7DBA955D35E1B9C327FDF1C12E721CDD650`。逐项复核了规范强度、命令、路径、数字、技术术语及跨层仲裁语义。
- 做了：用户批准后创建两份 `.bak.2026-07-15` 字节级备份，分节写入两份英文规则和两份中文对照，并在每次编辑后重读。
- 验证：备份 hash 与审核基线完全一致；新版英文 hash 分别为 `92558D5A0D94624B87AC94423D17ED7B20A06BB5E8ABEEBF233479E0F51208B1`、`D6D81817FB6F9D94478637C5E53E7840F3C3AB14134A93C8A2A9661424B2D4F0`。原 heading 与 inline code 全保留，英中结构计数一致；四份新版 UTF-8、尾随空白、NUL、末尾换行检查通过；`git diff --check -- CLAUDE.md` 退出码 0。
- 验证：归档前 `node tools/check_docs.mjs` 退出码 0（150 个文档、450 个代码/配置文件）；本任务没有对应的滚动产品工作线，无 `docs/todo.md` 状态需要改写。
- 遗留：无规则内容遗留；归档 commit 由收口步骤完成，不 push。

## 回顾（收口时填）
- 亮点：审核前锁定源 hash，审核后先做字节级备份，再分节落盘和逐次重读；英中结构计数与 inline code 覆盖可机械复核。
- 教训：不可把精确备份套入新版文件格式门；备份的唯一标准是与原源文件字节一致。
- 意外：两份原始 CLAUDE.md 均没有末尾换行，因此精确备份也必须保持该状态。
