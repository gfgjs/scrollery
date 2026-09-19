---
type: 工作记忆
status: 快照
created: 2026-08-25
line: Canvas缩略图加载渲染性能优化
---

# Findings

## 已知原因

`MediaGridCanvas` 首次挂载时先排入 generation 0 的 requestAnimationFrame；由于组件位于 KeepAlive 树中，随后执行 `onActivated` 并切换到 generation 1。旧帧仍占用 `rafId`，新代次的 `scheduleDraw` 被合并返回；旧帧执行时因代次失效而跳过 `draw()`，直到滚动触发新的绘制。

## 修复方向

让待执行的 rAF 记录其 generation。若已有帧属于旧代次，应取消并替换为当前代次；只有同代次帧才继续合并。

## 约束

- 保持现有绘制节流与生命周期失效保护。
- 不改变默认 DOM 画廊路径。
- 遵守当前工作树已有用户改动，不做破坏性回退。

## 已落地

- `CanvasRafScheduler` 记录帧句柄与 generation；旧代次待帧被当前代次替换。
- 旧回调即使晚到也不会清掉当前帧状态，也不会调用 `draw()`。
- 同代次重复请求仍只保留一帧，停止 Canvas 工作时同时清理句柄和 generation。
- `mediaGridCanvas.helpers.spec.ts` 覆盖上述时序；真实 WebView2 手动复测仍需在可稳定触发场景下补做。
