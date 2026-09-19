---
status: snapshot
type: working-memory
line: 文件树画廊定位唯一性修复
created: 2026-07-14
---

# 任务计划:文件树画廊定位唯一性修复

## 目标
修复大库中画廊滑动联动左侧文件树时因目录分组不唯一及异步展开乱序导致的错误定位，确保按稳定 `directory id` 唯一展开、选中并滚动到当前屏图片所属目录。

## 当前阶段
已收口

## 阶段

### 阶段 1:定位数据链与根因
- [x] 追踪画廊可见图片变化到文件树定位的事件、store 与组件调用链
- [x] 确认目录节点身份、路径规范化与滚动目标的当前契约
- **状态:** completed

### 阶段 2:设计并实现修复
- [x] 以 `(rel_path, directory_id)` 唯一目录序替代会跨根合并的 `rel_path` 单键序
- [x] 以 latest-wins 队列保证祖先展开、节点选中与滚动使用最后一个权威目标
- **状态:** completed

### 阶段 3:回归测试
- [x] 覆盖多级异步展开时旧目标晚完成、同路径同名目录与同名文件
- [x] 覆盖 SQL/内存排序等价、frontend 全量测试与 Rust 全量测试
- **状态:** completed

### 阶段 4:完整验证与提交
- [x] 运行相关测试、type-check、lint、build、格式与 docs 门禁
- [x] 复读改动并检查 diff
- [x] 使用精确 pathspec 分阶段提交并归档工作记忆
- **状态:** completed

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| folder layout 的目录顺序固定为 `(rel_path ASC, directory_id ASC)`，组内排序随后应用 | 保持既有路径顺序，同时用数据库主键保证跨扫描根同路径目录不交错 | D-001 |
| 画廊到文件树的异步定位采用串行 latest-wins，最多保留一个在途和一个最新候选 | 大库多级懒加载会乱序完成，必须禁止旧目标晚到覆盖新目标 | D-002 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| Windows sandbox ACL 注入拒绝只读命令 | 在默认 sandbox 中读取 skill 与仓库状态 | 经审批以只读 escalated 命令完成检查，写入仍限定工作区 |
| 原生 apply_patch 因同一 ACL 无法读取既有源码 | 对 helper 进行精确局部编辑 | 改用 Codex CLI 的 apply-patch 入口传递同一精确 patch，不做全文件覆写 |
| filename 排序回归测试首次报 no such collation sequence: NATURAL_CMP | 使用裸内存 SQLite 连接执行生产查询 | fixture 调用生产 register_custom_collations 后定向测试通过 |
| 新增竞态测试首次 lint 报 prefer-const | 队列变量先声明后赋值 | 改为 const 初始化；lint、type-check 与全量前端测试重跑通过 |
