---
id: 2026-07-13-全局性能面板-closeout
status: snapshot
type: closeout
line: 全局性能面板
created: 2026-07-13
---

# 收口处置:全局性能面板

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | code | repo:src/composables/usePerformanceMonitor.ts@81cef21 | 静默录制隐藏面板；实时模式单独刷新并明确有额外开销 | new | — | yes |
| F-002 | code | repo:src/components/devtools/PerformancePanel.vue@81cef21 | 面板内 GPU caveat 明示 draw 仅为 JavaScript 提交耗时 | new | — | yes |
| D-001 | code | repo:src/composables/usePerformanceMonitor.ts@81cef21 | startManualRecording 与 runGalleryRoundTrip 的静默生命周期 | new | — | yes |
| D-002 | code | repo:src/perf/performanceRecorder.ts@81cef21 | 非 Vue 单例、TypedArray 固定容量环形缓冲与停止后摘要 | new | — | yes |
| D-003 | code | repo:src/App.vue@81cef21 | 单一 App WebView 全局挂载 PerformancePanel | new | — | yes |
| D-004 | code | repo:src/components/devtools/PerformancePanel.vue@81cef21 | 静默模式/Release/关闭 DevTools 的面板说明 | new | — | yes |
