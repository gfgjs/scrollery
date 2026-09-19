---
status: snapshot
type: working-memory
line: agent规则激进精简-v2
created: 2026-07-15
---

# 进度日志：agent 规则激进精简 v2

## 会话：2026-07-15
- 做了：读取 planning、caveman-compress、caveman 技能；重读 v1 英中规则；确认 git 基线与工作区隔离。
- 验证：HEAD=`daf2fb1`；规则文件无未提交改动；原有未跟踪 `.agents/skills/planning/SKILL.md` 与 `AGENTS.md` 不在任务范围。
- 做了：为用户级 v1 英中规则建立 `*.bak.2026-07-15-v1` 字节级备份；项目级 v1 由 `daf2fb1` 保留；写入 v2 英中规则。
- 验证：全局/项目英文分别减少 71.5%/73.7% UTF-8 bytes；英中行数、heading、条目数一致；核心与高风险关键字覆盖检查无缺失；四份新版 UTF-8、尾随空白、NUL、末尾换行检查通过；`git diff --check` 退出码 0；`node tools/check_docs.mjs` 退出码 0（153 个文档、450 个代码/配置文件）。
- 验证：归档前 `node tools/check_docs.mjs` 再次通过（154 个文档、450 个代码/配置文件）；本任务无对应滚动产品工作线，不改 `docs/todo.md`。
- 遗留：无规则内容遗留；收口使用显式 pathspec commit，不 push。

## 回顾（收口时填）
- 亮点：从压句子改为重构加载模型，以 9 条全局规则和 17 条项目规则承载核心与红线，其余细节按领域读取。
- 教训：减少 token 不能只做同义改写；更有效的是降低常驻规则数量和高频义务强度。
- 意外：系统 Python、bundled Python 与 bundled Node 均无现成 tokenizer；未安装额外依赖。
