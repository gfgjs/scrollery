---
status: 施工中
type: 工作记忆
line: 主画廊重复项浏览施工
created: 2026-09-02
---

# 任务计划:主画廊重复项浏览施工

## 目标
按 `docs/designs/2026-09-02-主画廊重复项浏览方案.md` 完成 P0–P5 六个切片：在主画廊落地 duplicateLens（groups/folders）浏览镜头，替换独立 /duplicates 页入口。

## 当前阶段
阶段 P6:审查整改完成，待真机验收

## 阶段

### 阶段 0:代码库现状调研
- [x] 去重后端现状（dedup_index、精确摘要、Live Photo、source_revision、当前有效组查询）
- [x] 主画廊前端现状（MediaGrid、ViewDescriptor、layout、filterStore/uiStore、URL 路由）
- [x] 旧 /duplicates 页面现状（DuplicatesView、DedupGallery、文件夹树、清理草案）
- **状态:** done

### 阶段 P0:契约与表征测试
- [x] 冻结 DuplicateLensDescriptor、separator/item 最小投影、URL 规范化
- [x] 为普通 gallery 的 descriptor/layout/flat_ids parity 补表征测试
- [x] 固化 exact-ready、全成员、Live Photo、source revision、错误状态分类
- [x] 不改入口，旧 /duplicates 可继续使用
- **状态:** done

### 阶段 P1:按重复组只读镜头
- [x] 后端 groups item source → 标准 LayoutRow、separator、flat_ids
- [x] 前端镜头 store、URL 同步、固定可见 chip、排列控件
- [x] DOM/Canvas 组头、查看器顺序、滚动恢复、空/加载/失败状态
- **状态:** done

### 阶段 P2:查看器与主画廊契约闭环
- [x] groups 的 useViewDescriptor/view_to_sql 与布局同一 lowering；查看器改走布局缓存单项邻居
- [x] browse-only gate（阻断 SelectionToolbar、范围选择、Ctrl+A、右键、批量命令）
- [x] 切换排列/查看器往返/焦点恢复/退出返回快照验证
- **状态:** done

### 阶段 P3:关联文件夹模式
- [x] folder relation streaming query、Union-Find、确定性簇内遍历
- [x] folder/component rank、三类统计、文件夹 separator
- [x] 组徽标、显示独有项开关、文件夹轴、Canvas 分组绘制
- **状态:** done

### 阶段 P4:入口迁移与旧体验退场
- [x] 侧栏改名"重复项"进主画廊镜头；/duplicates 重定向
- [x] 删旧 DuplicatesView、DedupGallery、文件夹树、前端清理草案状态
- [x] 状态文档、i18n、帮助文案更新
- **状态:** done

### 阶段 P5:收拢与验收
- [x] Rust 测试、Vitest、vue-tsc、eslint、生产构建（历史批次已完成；P6 另行重验）
- [x] 验收矩阵逐项核对（方案 §14；读屏/无障碍项已按用户裁定删除）
- **状态:** done

### 阶段 P6:审查整改
- [x] 以已发布 generation 隔离分析中间结果，停止/失败保留上一版
- [x] 跨普通/groups/folders 语义切换时不展示旧 rows/summary
- [x] 镜头查看器改为布局缓存邻居查询，不再传完整 flat_ids
- [x] 镜头期间暂停搜索控件，退出后恢复原搜索状态
- [x] 废弃本方案全部读屏/无障碍设计与验收项
- [x] 修复 dedup 状态恢复竞态、清理过期注释并集中验证
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 废弃本方案全部读屏/无障碍设计，不继续建设 Canvas 语义层 | 用户明确裁定，集中交付当前重复项浏览主路径 | D-001 |
| 发布隔离采用最小 generation/staging 方案，其余修复沿现有数据流直接落地 | 避免过度抽象，优先修正已证实的正确性和性能问题 | D-002 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
