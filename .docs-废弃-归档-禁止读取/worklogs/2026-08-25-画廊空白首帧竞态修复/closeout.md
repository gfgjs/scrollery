---
id: 2026-08-25-画廊空白首帧竞态修复-closeout
status: snapshot
type: closeout
line: Canvas缩略图加载渲染性能优化
created: 2026-08-25
---

# 收口处置:画廊空白首帧竞态修复

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| D-001 | code | repo:src/components/media/mediaGridCanvas.helpers.ts@029fbe6 | `CanvasRafScheduler` 按 generation 合并同代帧、替换跨代帧 | new | — | yes |
| D-002 | code | repo:src/components/media/MediaGridCanvas.vue@029fbe6 | Canvas 绘制调度与 `stopCanvasWork()` 接入代次感知 scheduler | new | — | yes |
| D-003 | test | repo:src/components/media/mediaGridCanvas.helpers.spec.ts@029fbe6 | generation 0→activate generation 1、旧回调晚到、同代合并回归 | new | — | yes |
