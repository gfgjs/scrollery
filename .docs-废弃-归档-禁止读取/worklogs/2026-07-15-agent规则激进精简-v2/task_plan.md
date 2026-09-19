---
status: snapshot
type: working-memory
line: agent规则激进精简-v2
created: 2026-07-15
---

# 任务计划：agent 规则激进精简 v2

## 目标
保留核心原则与高风险技术红线，将其余规则改为按风险、按作用域触发，显著降低 CLAUDE.md 常驻 token 与执行负担，并同步英文规则、中文对照和可回滚备份。

## 当前阶段
任务完成并归档

## 阶段

### 阶段 1：负担审计与放宽边界
- [x] 读取 v1 英中规则与历史备份
- [x] 分类保留、合并、按需路由、放宽和删除项
- **状态：** completed

### 阶段 2：设计并写入 v2
- [x] 生成激进精简的英中对照
- [x] 备份用户级 v1，并利用 git 保留项目级 v1
- [x] 分段落盘并重读逻辑批次
- **状态：** completed

### 阶段 3：结构与风险验证
- [x] 核对核心原则、高风险红线、英中结构和回滚路径
- [x] 运行文件卫生、diff 与 docs gate
- **状态：** completed

### 阶段 4：token 量化
- [x] 检索现有 tokenizer；不可用时改用 UTF-8 bytes、words、行数与条目数
- [x] 报告可复现降幅与被放宽的执行义务，不伪造 token 数
- **状态：** completed

### 阶段 5：提交与收口
- [x] 显式 pathspec 提交本任务文件
- [x] 归档工作记忆并复跑文档门禁
- **状态：** completed

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| v2 从“规则全集”改为“常驻核心 + 条件路由” | 降低每轮输入 token 和无差别执行成本 | D-001 |
| 保留诚实、证据、完整交付、push 审批及高风险数据/安全红线 | 这些约束失守的代价高于 token 节省 | D-002 |
| 放宽逐编辑重读、全量测试、全面 orchestrator 复验 | 改为逻辑批次、风险相称验证和关键结论抽验 | D-003 |
| 不为量化临时安装 tokenizer | 避免给用户环境和仓库增加与规则精简无关的依赖 | D-004 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 两套 Python 均无 `tiktoken` | 系统 Python 与 bundled Python 分别 import | 停止该路径，不安装依赖 |
| Bundled Node runtime 无常见 tokenizer package | 检查 js-tiktoken、gpt-tokenizer、tiktoken、@dqbd/tiktoken、@anthropic-ai/tokenizer | 使用 UTF-8 bytes、words、行数和条目数作可复现代理，明确不声称 token 数 |
