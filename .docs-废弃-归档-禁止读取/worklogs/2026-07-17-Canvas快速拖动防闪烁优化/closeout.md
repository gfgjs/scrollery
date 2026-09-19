---
id: 2026-07-17-Canvas快速拖动防闪烁优化-closeout
status: snapshot
type: closeout
line: Canvas快速拖动防闪烁优化
created: 2026-07-17
---

# 收口处置:Canvas快速拖动防闪烁优化

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | code | repo:src/composables/useBucketVirtualScroll.ts@ec2bbff | `syncDesired` 最新目标旁路 / MediaGridCanvas `cancelAbortableThumbLoads` | new | — | yes |
| F-002 | code | repo:src/assets/styles/themes/porcelain.css@ec2bbff | `--color-bg-canvas-placeholder`(六主题同键) | new | — | yes |
| F-003 | code | repo:src/components/media/MediaGridCanvas.vue@ec2bbff | `GATED_PREFETCH_AHEAD_FACTOR` / `runDrawPrefetchSlice` | new | — | yes |
| F-004 | code | repo:scripts/bench/canvas-wave-bench.mjs@ec2bbff | 文件头用法注释 / `__scrolleryBench` DEV 桥 | new | — | yes |
| F-005 | code | repo:src/components/media/MediaGridCanvas.vue@ec2bbff | `canvasGap`(六主题 `--color-bg-canvas-gap`) | new | — | yes |
| F-006 | todo | repo:docs/status/Canvas快速拖动防闪烁优化.md | O 节防闪烁收口行 + N 节「60px 极密 Canvas 多档缩略图源」行(残留承接) | new | — | yes |
| D-001 | code | repo:src/composables/useBucketVirtualScroll.ts@ec2bbff | 常态单飞 + 在途全陈旧时最新目标旁路第 2 槽 | repo:src/composables/useBucketVirtualScroll.ts | — | yes |
| D-002 | code | repo:src/components/media/MediaGridCanvas.vue@ec2bbff | `ensurePrefetchPlan` gated 分支(关闸收缩仅前向而非停取) | repo:src/components/media/MediaGridCanvas.vue | — | yes |
| D-003 | code | repo:src/assets/styles/themes/porcelain.css@ec2bbff | `--color-bg-canvas-placeholder` 统一低饱和中灰 | repo:src/assets/styles/themes/porcelain.css | — | yes |
| D-004 | code | repo:src/components/media/mediaGridCanvas.helpers.ts@ec2bbff | `runPrefetchWalk`(预算按新启动数计)+ draw 尾分片 | new | — | yes |
| D-005 | code | repo:src/components/media/MediaGridCanvas.vue@ec2bbff | `canvasGap` 格缝/格面双 token 拆分 | repo:src/components/media/MediaGridCanvas.vue | — | yes |
| D-006 | todo | repo:docs/status/Canvas快速拖动防闪烁优化.md | N 节「60px 极密 Canvas 多档缩略图源」行(4K+全屏+64px 极速域残留承接) | repo:docs/status/Canvas快速拖动防闪烁优化.md | — | yes |
