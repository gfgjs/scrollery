---
status: 施工中
type: 工作记忆
line: 文档阅读器全管线调查
created: 2026-08-26
---

# 进度日志:文档阅读器全管线调查

## 会话:2026-08-26
- 做了:读取项目级 AGENTS.md、docs/README.md 与 planning 技能；确认项目允许在 `docs/planning/` 建立本次调查工作记忆；盘点工作区并发现已有未提交改动与 Vditor 相关施工记录。
- 做了:沿 `/doc/:id`、`DocumentViewer`、`BookReader`/`PdfReader`、synthetic book、Rust reader/doc IPC、扫描/缩略图和 Tauri 资源授权逐层追踪，并核对 Vditor/Markdown Editor 并行分支。
- 做了:对照 `docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/`、Vditor 方案工作记忆和当前代码，区分了已落地行为、历史结论与未提交 WIP。
- 验证:前端全量 Vitest 142 文件/1598 测试通过；阅读器目标测试 99 个通过；`vue-tsc`、相关 ESLint、生产构建通过；Rust reader 44、doc thumb 5、版本落盘 2 个测试通过，fmt/check 通过。
- 结论:主阅读管线已闭环支持 PDF/EPUB/TXT/MD；核心待处理风险是 TXT 入口全文预读、Markdown Editor 自动覆盖源文件语义、不支持格式 active viewer 漂移、PDF wrapper 非虚拟化及真实 GUI/E2E 缺口。
- 遗留:本次只调查和记录，没有修改业务代码；超大文件、真实 WebView2/IME、PDF 超多页和缩略图泵仍需手动/设备验证。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
