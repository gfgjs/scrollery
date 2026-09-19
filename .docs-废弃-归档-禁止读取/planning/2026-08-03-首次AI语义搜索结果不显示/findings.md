---
status: 施工中
type: 工作记忆
line: 首次AI语义搜索结果不显示
created: 2026-08-03
---

# 发现与决策:首次AI语义搜索结果不显示

## 需求
- 用户反馈: 首次 AI 语义搜索出结果后，内容不显示，必须拖动下滚动条才显示图片列表。

## 发现
- `src/stores/aiStore.ts` 的 `runSemanticSearch` 在后端写完 `ai_search_results` 后只调用 `media.invalidateLayout()`；语义模式刚切入时，`useJustifiedLayout` 会先按空结果计算一次布局，随后搜索完成再触发结果布局，首轮存在多个布局代次交错。
- 当前日志 `scrollery.2026-08-03.log` 的现场顺序为: 23:43:16.242 / 23:43:16.695 先产出 `0 items, 0 rows` 的 v10/v11，搜索完成后 23:43:18.670 起产出 `1000 items` 的 v12 及后续多个语义布局版本；这证明搜索结果数据已写入，问题不在 AI 检索为空。
- `src/stores/mediaStore.ts` 的 `fetchBucketRows` 每次把当时的 `layoutSummary.layoutVersion` 传给后端；后端 `get_bucket_rows` 在缓存代次不一致或布局尚未落地时返回 `LayoutNotReady`。
- `src/composables/useBucketVirtualScroll.ts` 的 `fetchSegment` 捕获取行失败后把段设为 `error`，该状态不会在同一愿望窗口自动重试；注释明确写的是“错误粘滞至离窗重进或布局换代”。
- 因此首个语义结果的布局换代期间若首段请求撞上 `LayoutNotReady`，首屏会保持空白；拖动滚动条改变愿望段后，旧段离窗、新段重新创建并再次取行，图片列表才出现。默认 `bucketSegmentedScroll=true`，与用户描述一致。
- `useBucketVirtualScroll.spec.ts` 当前把“失败后同窗不重试、换代才恢复”锁为既有行为；`useVirtualScroll` 虽在失败时回滚取数窗口，但仍依赖下一次 `updateVisible`（通常由滚动事件）才会重试。
- 本地日志还记录过 `fetchBucketRows(0, 2560) FAILED` / `LayoutNotReady`（21:29 与 23:41），说明该竞态并非只存在于语义搜索，但语义搜索的多代布局切换显著放大了触发概率。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | `LayoutNotReady` 属于布局换代期间的可恢复取行失败；虚拟滚动段不应把它永久粘成 error，首屏应有一次受控重试或按最新 layoutVersion 重取 | code/test |
