---
status: 施工中
type: 工作记忆
line: 首次AI语义搜索结果不显示
created: 2026-08-03
---

# 进度日志:首次AI语义搜索结果不显示

## 会话:2026-08-03
- 做了: 建立任务工作记忆，检查项目文档契约与工作区现有修改。
- 做了: 追踪 `aiStore → media.invalidateLayout → useJustifiedLayout → mediaStore.fetchBucketRows → useBucketVirtualScroll` 链路，并核对 2026-08-03 应用日志。
- 发现: 首次语义搜索会先计算空布局，再快速切换到多个 1000 项语义布局版本；布局代次交错时首段可能收到 `LayoutNotReady`。bucket 段把该错误粘成 `error`，同窗不重试，拖动滚动条触发离窗重建后才恢复。
- 验证: `npm test -- --run src/composables/useBucketVirtualScroll.spec.ts src/composables/useVirtualScroll.spec.ts` → 2 个文件、69 个测试全部通过；现有测试同时证明失败段同窗不重试。
- 验证: `git diff --check -- docs/planning/2026-08-03-首次AI语义搜索结果不显示` → 通过；未修改业务代码，保留工作区既有用户改动。
- 遗留: 当前请求按诊断范围收口；若用户确认修复，下一步应先补“LayoutNotReady 自动受控重试/最新代重取”回归测试，再改 bucket 与方案 A 的失败恢复路径。

## 回顾(收口时填)
- 亮点: 待补。
- 教训: 待补。
- 意外: 待补。
