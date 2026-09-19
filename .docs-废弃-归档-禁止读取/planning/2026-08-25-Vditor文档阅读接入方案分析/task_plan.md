---
status: 施工中
type: 工作记忆
line: Vditor文档阅读接入方案分析
created: 2026-08-25
---

# 任务计划:Vditor文档阅读接入方案分析

## 目标
结合 Vditor 官方实现与 Scrollery 当前文档阅读链路，给出是否接入、接入边界、架构方案、性能风险和落地顺序。

## 当前阶段
阶段 4:Markdown Editor 平行 POC 实现与验证

## 阶段

### 阶段 1:现状与官方能力核对
- [x] 读取 Scrollery 文档查看器、Markdown 渲染和资源访问实现
- [x] 核对 Vditor 官方仓库、文档、版本和许可证
- **状态:** completed

### 阶段 2:方案设计与风险评估
- [x] 区分阅读、编辑、源码和搜索等能力边界
- [x] 评估 Tauri、超长 Markdown、图片/链接和移动端约束
- [x] 形成推荐方案与备选方案
- **状态:** completed

### 阶段 3:实现与验证
- [x] 接入固定版本 Vditor 依赖与本地运行时资源
- [x] 新增仅 Markdown 启用的薄模块和 route query 切换
- [x] 运行构建、局部 lint、法律材料和静态资源校验
- **状态:** completed

### 阶段 4:Markdown Editor 平行 POC
- [x] 接入固定版本 `@vscode/markdown-editor` 与 `@vscode/observables`
- [x] 新增独立实验模块，不改 `BookReader`、Markdown section 渲染和 Rust 保存链路
- [x] 增加标准阅读器 / Vditor / Markdown Editor 三选一入口
- [x] 完成源码变更事件、当前窗口草稿和 EditContext 不可用提示
- [x] 运行 typecheck、build、lint、全量 Vitest 和 diff 检查
- **状态:** completed（Windows WebView2 真机输入体验待手测）

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| Vditor 作为仅 md 的平行模块，通过 route query 显式切换，默认仍是现行阅读器 | 用户要求可切换且不大改原功能；新模块可独立回滚、独立测试，避免污染现有 ReaderApi/foliate 状态机 | D-001 |
| 新模块直接使用 `new Vditor` 默认配置，不做工具栏/预览/主题能力裁剪 | 用户要求最快接入且保留 Vditor 全部能力；包装层只负责挂载、内容和销毁 | D-002 |
| 只补本地资源路径，不默认依赖外网 CDN | 当前 Tauri CSP 不允许外部脚本，资源本地化是能运行的必要条件，不属于额外产品设计 | D-003 |
| 本次不接 Scrollery 版本、进度、书签和 TOC | 避免平行模块反向改造现有阅读状态机，先以 Vditor 独立能力为交付边界 | D-004 |
| Markdown Editor 作为第三个平行入口，固定使用 `engine=markdown-editor` | 先验证 VS Code 实验编辑器的独立挂载和源码输出，不让其进入默认阅读链路 | D-005 |
| POC 只保留当前窗口草稿，不接保存 | 先验证 EditContext、中文输入、块级渲染和源码稳定性；保存可复用现有版本 IPC，另开子任务 | D-006 |
| EditContext 不存在时只在实验模块内显示不可用提示 | macOS/Linux 或未开启实验能力时不影响默认阅读器与 Vditor | D-007 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 全项目 typecheck 失败 | `npm run typecheck` | 仅报并行工作区已有 `src/constants/mediaCategoryDescriptors.ts` 与 `TreeCategoryDescriptor` 的字段不匹配，本线不修改 |
| 全量测试失败 | `npm run test` | 139/140 文件通过；唯一失败来自并行工作区文件树分类功能缺失 8 个 locale 键，本线新增键树完整 |
