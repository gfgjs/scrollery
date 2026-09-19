---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-15
---

# 发现：全仓换行统一

- core.autocrlf=true 将 18 份许可证与 3 个第三方 JS 转换成 CRLF，使 prepare:legal 固定 hash 校验失败。
- 恢复 LF 后 21 个文件匹配清单，prepare:legal 实测通过（931 package license files、7 SPDX、unresolved 0）。
- 局部修复已独立提交 f629ac43，统一方案从此提交开始。
- Git 已跟踪 2615 个文件，索引无 CRLF 或混合换行文本，差异主要来自 Windows 工作区检出转换，预期业务源码无需产生内容提交。
- .vscode/settings.json 未跟踪，仅作本地设置；共享规则通过 .gitattributes、.editorconfig 与 .prettierrc.json 持久化。
- worklog-kit 未安装，按任务边界不安装；文档只做局部结构和链接验证。
- 迁移后，2615 个既有跟踪文件中：2465 个 LF 文本、1 个 CRLF 批处理、48 个无换行文件、101 个二进制文件；索引原本已是 LF，业务源码无内容差异。
- 子代理重写 1495 个工作区文件；主会话另外负责 4 份文档的换行与收口记录。新建 .editorconfig 不计入原有 2615 个文件。
- 二进制 before/after hash 全部一致；文本规范化内容全部一致；18 份许可证与 3 个第三方 JS 的固定 hash 全部通过。
- core.autocrlf=true 的临时索引隔离检出验证新规则生效；2614/2615 文件与工作区相同，唯一差异是临时索引保留了 .prettierrc.json 修改前内容。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 固定 hash 的第三方文件必须保持清单对应字节，换行迁移后重新校验 | 仓库换行规则注释 |
