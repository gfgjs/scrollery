---
status: 快照
type: working-memory
line: 画廊重排滚动位置恢复
created: 2026-07-17
---

# 任务计划:画廊重排滚动位置恢复

## 目标
窗口/侧栏改宽、切换分组、切换排序、切换布局模式等整体重排后,画廊自动滚回用户原浏览位置(钉住重排前视口顶部的项),不再落回旧 scrollTop 看到无关内容。

## 当前阶段
阶段 3:验证(自动化已过,⏸真机 GUI)

## 阶段

### 阶段 1:摸底
- [x] 定位现有滚动恢复四层机制(scrollCache / 行高锚点 / layoutVersion watcher / onMounted 恢复)
- [x] 确认后端 `get_item_y_by_id` O(1) 且读当前布局缓存(layout/cache.rs:495)
- [x] 确认重算触发链:useJustifiedLayout watch(post-flush) → computeLayout(串行+coalesce) → layoutVersion bump → MediaGrid watcher 恢复
- **状态:** complete

### 阶段 2:施工
- [x] helpers 抽纯函数 `pickReflowAnchor(rows, vTop)`(含顶部特判)+ 单测 4 例
- [x] MediaGrid:锚点泛化(rename capture/restoreReflowAnchor + viewKey 守卫 + isComputingLayout 续命 + 卸载清定时器)
- [x] 新触发面:gridRowHeight/groupBy/sortOrder/sortWithinGroup/seamlessGroups/layoutMode 合一 pre-flush watcher + ResizeObserver 回调捕获(防抖前)
- **状态:** complete

### 阶段 3:验证
- [x] vitest 全量:88 文件 / 1188 全绿(含新增 pickReflowAnchor 4 测)
- [x] vue-tsc --noEmit 零错;eslint 改动三文件零告警
- [ ] ⏸真机 GUI:大库切排序/分组/拖窗宽,视口顶部项钉原位;列表顶部切排序留在顶部;行高滑块回归不劣化
- **状态:** in_progress(仅剩真机)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 复用行高锚点机制扩触发面,不另建 | capture(pre-flush)→GET_ITEM_Y_BY_ID→scrollToLogicalY 三件套现成,双引擎/映射态已处理 | |
| 锚点语义=钉重排前视口顶部首个可视项 | 排序翻转后位置坐标无意义,唯一可解释的「原浏览位置」是同一内容项 | |
| 顶部(vTop≤0)不捕获锚点 | 排序翻转时钉首项会把顶部浏览者甩到列表另一端;顶部用户期望留在顶部 | D-001 |
| 400ms 清锚定时器遇 isComputingLayout 续命 | 大库 compute_layout 秒级,锚点会在恢复前被清——现有行高锚点在大库同样受害,一并修 | D-002 |
| 锚点携带 viewKey,恢复时不匹配即弃用 | 持锚期间切目录/相册,旧视图锚点会污染新视图滚动位(现有机制同样暴露) | D-003 |
| 触发面=groupBy/sortOrder/sortWithinGroup/seamlessGroups/layoutMode/容器宽度;filter/search 不入 | 前者项集不变仅几何/顺序变,锚必存活;后者项可能消失,维持现有 scrollCache 兜底,不扩 scope | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
