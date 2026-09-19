---
id: 2026-08-27-画廊Canvas悬停缩放修复-closeout
status: snapshot
type: closeout
line: canvas设置项生效检查与修复
created: 2026-08-27
---

# 收口处置:画廊Canvas悬停缩放修复

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | code | repo:src/composables/useCanvasHoverCard.ts@fc7f0efa | `handleHoverTracking` 对整张悬停卡的 pointermove 边界保护 | new | — | yes |
| F-002 | test | repo:src/components/media/MediaGridCanvas.hover.spec.ts@fc7f0efa | 卡内任意目标不重命中底层 `pickWithRow` 的回归 | new | — | yes |
| F-003 | code | repo:src/App.vue@fc7f0efa | 启动配置回填 `config.enableHoverScale` 并同步 DOM class | new | — | yes |
| F-004 | test | repo:src/components/media/MediaGridCanvas.hover.spec.ts@fc7f0efa | 启动悬停缩放值回填契约 | new | — | yes |
| F-005 | code | repo:src/components/media/MediaGridCanvas.styles.css@fc7f0efa | token 化、无过冲的悬停卡 opacity+scale 入场 | new | — | yes |
| F-006 | test | repo:src/components/media/MediaGridCanvas.hover.spec.ts@fc7f0efa | 悬停动效 token、opacity 与旧过冲 keyframe 契约 | new | — | yes |
