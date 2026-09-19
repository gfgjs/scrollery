---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-15
---

# 进度：全仓换行统一

## 2026-09-15
- 用户授权：先提交当前修复，再执行全仓统一方案。
- 已提交 f629ac43，内容仅 .gitattributes 的局部 LF 规则。
- 已委派白名单模型完成换行配置、工作区迁移和机械验证；主会话审查配置与实际差异，负责文档记录与最终提交。
- 共享配置：.gitattributes 默认 text=auto/eol=lf，bat/cmd 使用 CRLF；.editorconfig 与 Prettier 显式对齐。
- 本地配置：未跟踪的 .vscode/settings.json 设 files.eol 为 LF，保留原有设置，不强制入库。
- 验证：101 个二进制 hash 不变，文本规范化内容不变；21 个固定 hash 通过；git diff --check 通过；CARGO_NET_OFFLINE=true npm run prepare:legal 退出 0，环境已还原。
- 检出验证：临时索引与隔离目录确认 core.autocrlf=true 下普通文本 LF、批处理 CRLF；未运行构建或安装依赖。
- 文档门禁 worklog-kit 未安装，未运行完整门禁；本次只核对新增收口候选、元数据、链接与 diff。
- 另一个任务的新建规划目录不属于本次提交，保持原样。

## 回顾
- 亮点：先核对固定 hash 再迁移，字节差异与真实内容差异分别验收。
- 教训：core.autocrlf 会改变工作区字节；默认仓库规则应同时约束入库、检出与编辑器。
- 意外：索引本来全部为 LF，因此工作区迁移覆盖大量文件，但源码正文不需要产生提交差异。
