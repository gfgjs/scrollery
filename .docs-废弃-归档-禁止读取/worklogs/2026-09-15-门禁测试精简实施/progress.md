---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-15
---

# 实施与回顾

删除文档治理工作流、旧名门和旧文档脚本，撤回新增 CI 分流与 NOTICE 测试框架。删除纯源码/CSS 字符串测试，保留真实交互及几何边界测试。CI 去重 cargo check，发布去重前端构建。

验证：3 个前端文件共 80 项测试通过。局部 ESLint、三份工作流 YAML 解析、NOTICE/SBOM 实际生成及 diff 检查通过（NOTICE 无改动，单独 --check 不写 SBOM）。FFmpeg 测试仅做格式检查，未执行专项依赖或全量构建。

回顾：前期把精简设计得过于复杂，用户明确纠正后收窄为直接删除；不将未实施的分流方案登记为待办。
