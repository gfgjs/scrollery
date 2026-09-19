---
status: 快照
type: working-memory
line: 根文件夹显隐
created: 2026-07-18
---

# 任务计划:设置页控制根文件夹显隐(库级排除)

## 目标
设置页新增每个扫描根(scan_roots)的显隐开关;隐藏后该根媒体从画廊「全部」/时间轴/搜索/统计/侧栏文件树/Ctrl+A 全选全部消失,取消即恢复,状态持久。

## 当前阶段
阶段 1+2 全落地全绿(后端 636 + 前端 1211 测,fmt/clippy/tsc/eslint clean);阶段 3 待:真机 GUI 验收 + 用户裁提交顺序(V21 依赖未提交 V20)。**未提交、未收口。**

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| D-001 用条件子查询排除,不反规范化 root_hidden 到 media_items | 隐藏罕见;无隐藏时谓词整条省略→canonical 免JOIN红线不触碰、S1缓存不受扰。反规范化有 scan/move/copy 3+ 同步点,泄漏隐患 | |
| D-002 排除注入 push_where_predicates(WHERE 唯一事实源) | 一处覆盖画廊+全选+所有排序变体;隐藏集在有 conn 的入口函数取,不经 MediaFilter(使全局 filename rank 也自动吃到) | |
| D-003 新增专用列 scan_roots.is_hidden,不复用死列 is_active | is_active 语义含糊(从未被消费);专列意图清晰,仿 persons.is_hidden | |
| D-004 隐藏范围=全库(用户已选) | 用户 AskUserQuestion 明确选「全库范围」 | |

## 阶段

### 阶段 1:后端
- [ ] SCHEMA_V21 + 迁移(CURRENT_VERSION→21,STEPS 追加,改回拨重放测试 v17/v19 drop 新列)
- [ ] models.rs ScanRoot += is_hidden;scan.rs mapper/SELECT/set_scan_root_hidden/hidden_root_ids
- [ ] layout.rs push_root_exclusion + 线程 hidden_roots 穿 push_where_predicates/push_query_body/canonical/item_ids/view_to_sql;list_library_formats
- [ ] media.rs get_app_stats;search.rs 文件名搜索
- [ ] scan_commands.rs set_scan_root_hidden(写库+bump_data_version+tree invalidate);media_commands.rs resolve/count_selection 注入;registry 注册
- [ ] cargo test(迁移+layout+stats+selection)
- **状态:** in_progress

### 阶段 2:前端
- [ ] types ScanRoot += isHidden;constants/ipc SET_SCAN_ROOT_HIDDEN;ipcFixtures
- [ ] scanStore setScanRootHidden + visibleScanRoots
- [ ] RootFolderVisibilitySection.vue(仿 KnownVolumesSection)+ 挂 SettingsView storage 区
- [ ] FoldersSection 侧栏树用 visibleScanRoots
- [ ] i18n zh/en
- [ ] vue-tsc + eslint + 关联前端测试
- **状态:** pending

### 阶段 3:收口门禁
- [ ] 全量 cargo test + vue-tsc + eslint
- [ ] 端到端真机验收(⏸GUI 待批)
- **状态:** pending

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|