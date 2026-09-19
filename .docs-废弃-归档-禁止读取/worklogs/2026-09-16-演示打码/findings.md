---
status: snapshot
type: working-memory
line: 演示打码
created: 2026-09-16
---

# 发现
- Canvas 图片入口为 src/components/media/mediaGridCanvas.cellRenderer.ts，正常覆盖层在图片之后绘制。
- Canvas 悬停由 useCanvasHoverCard 驱动 DOM MediaThumb，异步预解码 token 必须在开启打码时失效。
- 信息浮窗在 mediaGridCanvas.infoOverlay.ts 按 id 缓存文本，切换需使缓存失效。
- 文件树普通行、粘性行与原生 title 都含敏感名称或路径；画廊目录分组头也需同步演示名称。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 模糊只在图片绘制阶段生效，需覆盖开关切换及悬停异步返回 | 状态分片与行为测试 |
