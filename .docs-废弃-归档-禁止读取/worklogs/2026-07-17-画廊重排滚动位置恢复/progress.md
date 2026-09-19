---
status: 快照
type: working-memory
line: 画廊重排滚动位置恢复
created: 2026-07-17
---

# 进度日志:画廊重排滚动位置恢复

## 会话:2026-07-17
- 做了:摸底完成(四层恢复机制+重算链+后端 IPC 实证),设计拍板(见 task_plan 决策表),施工完成——① mediaGrid.helpers 新增 `pickReflowAnchor` 纯函数(顶部特判 vTop≤0 不押锚)+ spec 4 例;② MediaGrid 锚点泛化:capture/restoreReflowAnchor 更名、锚点携带 viewKey(恢复时不匹配即弃)、清锚定时器遇 `media.isComputingLayout` 续命(修大库秒级 compute 竞态)、onBeforeUnmount 掐断定时器;③ 触发面:gridRowHeight/groupBy/sortWithinGroup/sortOrder/seamlessGroups/layoutMode 合一 pre-flush watcher + ResizeObserver 回调(防抖前)捕获;④ todo.md 增补登记。
- 验证:`npm run test` 88 文件/1188 全绿(含新增 4 测);`npm run typecheck`(vue-tsc --noEmit)零输出通过;`npx eslint` 改动三文件零告警。
- 遗留:⏸真机 GUI 验收(大库切排序/分组/拖窗宽钉原位;顶部切排序留顶;行高滑块回归)。类型坑一则:ReflowAnchorRowLike 的 items 必须可选——LayoutRow 联合的 separator 变体无 items 字段,非判别联合不narrowing。
- 提交:`872981e`(7 文件,显式 pathspec,未吸收在途 src-tauri 视频线改动)。
- docs 门禁:check_docs 本提交前后均 213 ✗(HEAD~1 临时 worktree 对拍实证,含 check_docs_index 的 README「下一空闲字母」缺失)——**基线即红,零新增违规**,按红线归收编线不代修。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
