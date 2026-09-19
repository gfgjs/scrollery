---
status: 快照
type: working-memory
line: 画廊重排滚动位置恢复
created: 2026-07-17
---

# 发现与决策:画廊重排滚动位置恢复

## 需求
- 用户原话:「优化画廊体验: 改变窗口大小/切换分组/切换排序等重排操作后重新滚到原浏览位置」

## 发现
- 现有滚动恢复四层(MediaGrid.vue):① scrollCache 模块级 Map(utils/scrollCache.ts,键=`dir-${id}`/`album-${name}`,bucket 存逻辑 y、方案 A 存物理 scrollTop);② 行高锚点 capture/restore(MediaGrid.vue:892-930,仅 `ui.gridRowHeight` watcher 触发);③ layoutVersion watcher(MediaGrid.vue:1890-1912)先试锚点、败走 scrollCache;④ onMounted 重挂载恢复(1092-1110)。
- **缺口**:分组/排序/宽度重排只走 ② 的兜底路径——锚点从未捕获,恢复的是旧 Y 坐标,重排后同一 Y 是完全不同内容。
- 重算链:useJustifiedLayout watch 数组(useJustifiedLayout.ts:189-221,含 groupBy/sortOrder/sortWithinGroup/seamlessGroups/effectiveLayoutMode,**post-flush**)→ requestCompute → media.computeLayout(串行 isComputingInternal + pendingComputeParams coalesce,mediaStore.ts:125-202)→ layoutVersion bump。行高捕获 watcher 是 pre-flush,先于重算跑——新触发面沿用同一时序保证。
- resize 链:ResizeObserver(MediaGrid.vue:1066-1075,>1px 才动)→ onResize 防抖 300ms(DEFAULTS.RESIZE_DEBOUNCE_MS)→ compute。捕获点须在 observer 回调内(防抖前)。
- 后端 `get_item_y_by_id`:layout/cache.rs:495,O(1)(id_to_flat→flat_rowcol→rows[ri].y,Part2 文档实证),读**当前** LayoutCache——恢复发生在 compute 完成后,天然拿新布局坐标;项不在新布局返回 null → 走兜底。
- **潜伏缺陷(大库)**:锚点 400ms 定时清除(scheduleAnchorClear)与 compute IPC 竞态——50万库 compute_layout 1-2s(useJustifiedLayout.ts:26 注),layoutVersion watcher 恢复时锚点已被清,静默退化走 scrollCache。现有行高锚点在大库同样受害。`media.isComputingLayout` 可作续命信号。
- mediaGrid.helpers.ts 是既定「纯函数抽出+单测」范式(头注明示),锚点拾取逻辑落此处可测。
- 双引擎恢复面已统一:bucket 走 `scrollToLogicalY`(B3 映射态自处理),方案 A 走 `logicalToPhysical`+scrollTop——restoreRowHeightAnchor 内两分支现成,无需改。
- 引擎切换(bucketActive watch,MediaGrid.vue:859-869)自带滚动位保持,与本任务正交。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 锚点定时清除 vs 长 IPC 竞态:凡「捕获→异步重算→恢复」型机制,过期策略必须以在途信号续命,不能裸靠壁钟 | experience |
