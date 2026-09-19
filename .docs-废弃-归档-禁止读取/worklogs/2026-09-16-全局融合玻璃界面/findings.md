---
status: snapshot
type: working-memory
line: UI-多主题系统
created: 2026-09-16
---

# 发现：全局融合玻璃界面

## 需求
用户认可整面连续玻璃方向，并授权开始实施。覆盖侧栏、画廊、时间轴、顶栏和底栏。

## 发现
- src/assets/styles/glass.css 已按 data-glass 对接原生 Mica/Acrylic，存在侧栏卡片、标题遮罩及多个独立材质面。
- src/components/media/MediaGridCanvas.vue 已按玻璃开关建立 alpha 上下文，画廊支持透明画布，无需重做渲染管线。
- package.json 提供 typecheck、vitest；scripts/capture-theme-matrix.mjs 使用现有 UI harness 进行浏览器截图，不能证明原生 DWM 效果。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|

| F-001 | 原生 DWM、沉浸浮出与真实图库 GPU 仍需真机验收，不能由浏览器截图替代 | UI-多主题系统滚动状态 |

