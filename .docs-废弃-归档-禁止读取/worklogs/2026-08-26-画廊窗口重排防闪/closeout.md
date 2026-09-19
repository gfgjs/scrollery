---
id: 2026-08-26-画廊窗口重排防闪-closeout
status: snapshot
type: closeout
line: 画廊窗口重排防闪
created: 2026-08-26
---

# 收口处置:画廊窗口重排防闪

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| D-001 | code | repo:src/composables/useBucketVirtualScroll.ts@c9768661 | bucket 换代以旧 rows 作为首帧并等待新代原子替换 | new | — | yes |
| D-002 | code | repo:src/stores/mediaStore.ts@c9768661 | 连续 resize 只提交最后一个布局摘要 | new | — | yes |
| D-003 | code | repo:src/components/media/MediaGridCanvas.vue@c9768661 | resize settling 期间只改 CSS 尺寸，停稳后重建 backing store | new | — | yes |
| F-001 | test | repo:src/stores/mediaStore.spec.ts@c9768661 | 旧布局在最新布局结果完成前保持可见 | new | — | yes |
| F-002 | code | repo:src/components/media/MediaGridCanvas.vue@c9768661 | Canvas backing store 不在连续 resize 回调中反复重置 | repo:src/components/media/MediaGridCanvas.vue | — | yes |
