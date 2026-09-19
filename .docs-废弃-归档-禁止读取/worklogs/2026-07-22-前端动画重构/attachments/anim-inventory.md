---
id: 2026-07-22-anim-inventory
status: snapshot
type: plan
created: 2026-07-22
line: 前端动画重构
---

# Scrollery 前端动画/过渡代码清单

## Vue Transition 组件

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/components/common/ToastContainer.vue:6 | TransitionGroup | 通知消息进出场 (name="toast") |
| src/components/media/ContentViewer.vue:262 | Transition | 内容面板滑入滑出 (name="slide") |
| src/components/layout/AppShell.vue:101 | Transition | 全屏提示气泡淡入淡出 (name="fs-hint") |
| src/components/ui/UiPopover.vue:97 | Transition | 弹出层淡入淡出 (name="ui-popover-fade") |
| src/components/layout/AppToolbar.vue:176 | Transition | 工具栏下拉菜单淡入淡出 (name="dropdown-fade") |
| src/components/media/SelectionToolbar.vue:3 | Transition | 选择工具栏向上滑入 (name="slide-up") |
| src/components/media/SemanticSearchPanel.vue:4 | Transition | 语义搜索面板 (name="semantic-panel") |
| src/components/sidebar/AccordionSection.vue:50 | Transition | 折叠框高度/透明度过渡 (name="acc-collapse") |

## CSS transition 属性（样式类别）

### 颜色/背景过渡
| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/views/SettingsView.vue:830 | transition | opacity 0.2s |
| src/views/SettingsView.vue:908-911 | transition | background/color/border-color (transition-fast) |
| src/views/SettingsView.vue:929-932 | transition | background/color/border-color (transition-fast) |
| src/views/SettingsView.vue:972-974 | transition | background-color/color (transition-fast) |
| src/views/SettingsView.vue:1024-1026 | transition | text-decoration-color/color (transition-fast) |
| src/views/PersonsView.vue:501-504 | transition | background/border-color/transform (transition-fast) |
| src/views/PersonsView.vue:602-605 | transition | opacity/color/background (transition-fast) |
| src/views/DocumentViewer.vue:1600-1602 | transition | opacity/color (transition-fast) |
| src/views/DocumentViewer.vue:1667-1669 | transition | background/color (transition-fast) |
| src/views/CollectionsView.vue:452-454 | transition | background/border-color (transition-fast) |
| src/views/CollectionsView.vue:479-482 | transition | background/border-color/transform (transition-fast) |
| src/views/CollectionsView.vue:550-553 | transition | opacity/color/background (transition-fast) |
| src/views/AudioPlayer.vue:584 | transition | 多属性 (transition-fast) |
| src/components/common/StarRating.vue:88 | transition | color (transition-fast) |
| src/components/common/OnboardingWizard.vue:256-259 | transition | width/border-radius/background-color (transition-fast) |
| src/components/common/OnboardingWizard.vue:381-384 | transition | border-color/background-color/color (transition-fast) |
| src/components/layout/AppShell.vue:268-272 | transition | left/opacity/background-color/color (transition-normal/fast) |
| src/components/layout/WindowChrome.vue:179-181 | transition | background-color/color (transition-fast) |
| src/components/common/ColorLabelPicker.vue:70-73 | transition | opacity/transform/border-color (transition-fast) |
| src/components/layout/FormatFilterPopover.vue:215-218 | transition | background/color/border-color (transition-fast) |
| src/components/ui/UiDialog.vue:134-135 | transition | background-color/color (transition-fast) |
| src/components/settings/CollapsibleCard.vue:74-77 | transition | background/color/border-color (transition-fast) |
| src/components/settings/DynamicSettingControl.vue:500-502 | transition | background-color/color (transition-fast) |
| src/components/settings/FaceModelLibrary.vue:208-210 | transition | background/color (transition-fast) |
| src/components/settings/SettingRow.vue:65-67 | transition | background-color/color (transition-fast) |
| src/components/settings/ThemePicker.vue:132-134 | transition | background/color (transition-fast) |
| src/components/media/MediaGrid.vue:2213-2215 | transition | background/color (transition-fast) |
| src/components/media/SemanticResultCard.vue:115-119 | transition | background-color/border-color/transform/box-shadow (transition-fast) |
| src/components/sidebar/sections/LibrarySection.vue:157-159 | transition | background-color/color (transition-fast) |
| src/components/media/SelectionActions.vue:354-356 | transition | background-color/color (transition-fast) |
| src/components/sidebar/sections/ManagementSection.vue:259 | transition | width 100ms linear |
| src/components/sidebar/sections/ToolsSection.vue:804-806 | transition | background-color/color (transition-fast) |

### 尺寸/进度过渡
| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/views/SettingsView.vue:1088 | transition | width 100ms linear |
| src/views/PluginStoreView.vue:572 | transition | width 200ms linear |
| src/components/doc/ReaderSettingsGroup.vue:104 | transition | grid-template-rows 0.26s cubic-bezier(0.4, 0, 0.2, 1) |
| src/components/settings/CollapsibleCard.vue:112 | transition | grid-template-rows 0.26s cubic-bezier(0.4, 0, 0.2, 1) |
| src/components/settings/ModelLibrary.vue:510 | transition | width 200ms linear |
| src/components/settings/ModelLibrary.vue:424 | transition | grid-template-rows 0.26s cubic-bezier(0.4, 0, 0.2, 1) |
| src/components/media/ContentViewer.vue:1349 | transition | padding-right (transition-normal) |

### 变换/不透明度过渡
| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/views/CollectionsView.vue:419 | transition | transform (transition-fast) |
| src/views/DocumentViewer.vue:1736 | transition | transform 0.28s ease |
| src/App.vue:375 | transition | transform 0.18s ease |
| src/components/common/ToastContainer.vue:136 | transition | opacity 0.2s |
| src/components/common/ToastContainer.vue:173-174 | transition | opacity/transform (compose) |
| src/components/layout/AppShell.vue:296 | transition | margin-left (transition-normal) |
| src/components/layout/AppShell.vue:308 | transition | background (transition-fast) |
| src/components/layout/AppShell.vue:382 | transition | transform 0.18s ease |
| src/components/layout/AppShell.vue:397 | transition | transform 0.18s ease |
| src/components/layout/GalleryViewControls.vue:297 | transition | transform (transition-fast) |
| src/components/layout/GalleryViewControls.vue:350 | transition | transform 0.28s ease |
| src/components/layout/GalleryFilterChips.vue:362 | transition | transform 0.28s ease |
| src/components/doc/BookReader.vue:644-647 | transition | opacity/background/color (transition-fast) |
| src/components/doc/FolderTreeSelectorDialog.vue:239 | transition | transform 0.2s |
| src/components/doc/ReaderSettingsGroup.vue:91 | transition | transform 0.2s |
| src/components/doc/BookmarkPanel.vue:161 | transition | opacity (transition-fast) |
| src/components/media/ContentViewer.vue:1373-1376 | transition | opacity/background/color (transition-fast) |
| src/components/media/ContentViewer.vue:1407 | transition | transform 0.2s cubic-bezier(0.25, 0.46, 0.45, 0.94) |
| src/components/media/ContentViewer.vue:1692 | transition | transform (transition-normal) |
| src/components/media/FaceApprovalPanel.vue:331-333 | transition | background-color/color (transition-fast) |
| src/components/media/MediaThumb.vue:524 | transition | 多属性 |
| src/components/media/MediaThumb.vue:656 | transition | opacity 0.18s ease |
| src/components/media/MediaThumb.vue:768-770 | transition | opacity/transform (transition-fast) |
| src/components/media/MediaThumb.vue:859-861 | transition | opacity/background (transition-fast) |
| src/components/media/SemanticResultCard.vue:146 | transition | 多属性 |
| src/components/media/SelectionActions.vue:307 | transition | transform 0.28s ease |
| src/components/media/SelectionActions.vue:291 | transition | 多属性 |
| src/components/media/SelectionToolbar.vue:233-235 | transition | background-color/color (transition-fast) |
| src/components/media/TimelineScrubberCanvas.vue:1024 | transition | background (transition-fast) |
| src/components/media/TimelineScrubber.vue:459-461 | transition | background/height (transition-fast) |
| src/components/sidebar/sections/FoldersSection.vue:1517-1520 | transition | background/color/border-color (transition-fast) |
| src/components/sidebar/sections/FoldersSection.vue:1771 | transition | transform (transition-fast) |

### z-index 和其他
| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/components/media/MediaGrid.vue:2308 | transition | z-index 220ms linear |
| src/components/layout/AppToolbar.vue:665 | transition | border-color (transition-fast) |
| src/components/layout/AppToolbar.vue:685 | transition | color (transition-fast) |
| src/components/layout/AppToolbar.vue:795-798 | transition | background-color/border-color/color (transition-fast) |
| src/components/layout/AppToolbar.vue:854-856 | transition | background/color (transition-fast) |
| src/components/layout/AppToolbar.vue:902-904 | transition | opacity/transform (transition-fast) |
| src/components/layout/BackgroundFileJobIndicator.vue:213-215 | transition | background-color/color (transition-fast) |
| src/components/layout/GalleryViewControls.vue:266 | transition | border-color (transition-fast) |
| src/components/layout/GalleryViewControls.vue:334 | transition | 多属性 (transition-fast) |
| src/components/layout/GalleryFilterChips.vue:346 | transition | 多属性 |
| src/components/layout/GalleryFilterChips.vue:459-462 | transition | border-color/color/background (transition-fast) |
| src/components/layout/StatusBarFileInfo.vue:191 | transition | color (transition-fast) |
| src/components/layout/StatusBarFileInfo.vue:215 | transition | color (transition-fast) |
| src/components/media/ContentViewer.vue:1562 | transition | color 0.3s ease |
| src/components/media/ContentViewer.vue:1659 | transition | color (transition-fast) |
| src/components/media/ContentViewer.vue:1675 | transition | color (transition-fast) |
| src/components/media/MediaGrid.vue:2380-2383 | transition | right/color/background (transition-fast) |
| src/components/media/MediaGrid.vue:2406-2409 | transition | color/background/border-color (transition-fast) |
| src/components/media/MediaGrid.vue:2444-2446 | transition | color/background (transition-fast) |
| src/components/media/MediaGrid.vue:2478-2480 | transition | color/background (transition-fast) |
| src/components/media/MediaGrid.vue:2531 | transition | 多属性 |
| src/components/media/MediaGrid.vue:2542 | transition | 多属性 |
| src/components/media/MediaScrollbar.vue:186-188 | transition | opacity/background (transition-fast) |
| src/components/media/SemanticSearchPanel.vue:225 | transition | 多属性 |
| src/components/media/SemanticSearchPanel.vue:289 | transition | width 0.4s ease |
| src/components/media/SemanticSearchPanel.vue:320-323 | transition | background-color/border-color/color (transition-fast) |
| src/components/media/MinimapAxis.vue:501 | transition | opacity (transition-fast) |
| src/components/settings/ReaderSettingsSection.vue:124-126 | transition | border-color/box-shadow (transition-fast) |
| src/components/settings/ThemePicker.vue:185-187 | transition | border-color/box-shadow (transition-fast) |
| src/components/sidebar/sections/ToolsSection.vue:666 | transition | opacity (transition-fast) |
| src/components/sidebar/sections/ToolsSection.vue:830 | transition | width 100ms linear |

## CSS @keyframes 和 animation 属性

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/views/SettingsView.vue:1098 | animation | shimmer 1.5s ease-in-out infinite |
| src/views/SettingsView.vue:1172 | @keyframes | shimmer 动画定义 |
| src/views/PluginStoreView.vue:885 | animation | spin 1s linear infinite |
| src/views/PluginStoreView.vue:887 | @keyframes | spin 动画定义 |
| src/views/PersonsView.vue:450 | animation | persons-spin 1s linear infinite |
| src/views/PersonsView.vue:452 | @keyframes | persons-spin 动画定义 |
| src/components/exotic/PluginGate.vue:186 | animation | gate-spin 0.7s linear infinite |
| src/components/exotic/PluginGate.vue:188 | @keyframes | gate-spin 动画定义 |
| src/components/doc/BookReader.vue:696 | animation | book-curl-fade 320ms ease-out |
| src/components/doc/BookReader.vue:699 | animation | book-curl-next 320ms ease-out |
| src/components/doc/BookReader.vue:702 | animation | book-curl-prev 320ms ease-out |
| src/components/doc/BookReader.vue:704 | @keyframes | book-curl-fade 动画定义 |
| src/components/doc/BookReader.vue:714 | @keyframes | book-curl-next 动画定义 |
| src/components/doc/BookReader.vue:722 | @keyframes | book-curl-prev 动画定义 |
| src/components/ui/UiButton.vue:49 | animation | ui-btn-spin 0.6s linear infinite |
| src/components/ui/UiButton.vue:52 | @keyframes | ui-btn-spin 动画定义 |
| src/components/ui/UiButton.vue:60 | animation-duration | 1.5s (调整) |
| src/components/sidebar/sections/ToolsSection.vue:834 | animation | spin 1s linear infinite |
| src/components/sidebar/sections/ManagementSection.vue:263 | animation | breathe 1.5s ease-in-out infinite |
| src/components/sidebar/sections/ManagementSection.vue:265 | @keyframes | breathe 动画定义 |
| src/components/sidebar/sections/FoldersSection.vue:1761 | animation | spin 1s linear infinite (全局 animations.css) |
| src/components/media/MediaGridCanvas.vue:1793 | animation | mgc-hover-pop 220ms cubic-bezier(0.34, 1.18, 0.64, 1) both |
| src/components/media/MediaGridCanvas.vue:1795 | @keyframes | mgc-hover-pop 动画定义 |
| src/components/media/MediaGridCanvas.vue:1817 | animation | mgc-delayed-reveal 0s 150ms both |
| src/components/media/MediaGridCanvas.vue:1819 | @keyframes | mgc-delayed-reveal 动画定义 |
| src/components/media/SemanticSearchPanel.vue:349 | animation | spin 0.6s linear infinite |
| src/components/media/SemanticSearchPanel.vue:351 | @keyframes | spin 动画定义 |
| src/components/media/SemanticSearchPanel.vue:401 | animation | spin 0.8s linear infinite |
| src/components/layout/AppToolbar.vue:919 | animation | toolbar-spin 0.6s linear infinite |
| src/components/layout/AppToolbar.vue:922 | @keyframes | toolbar-spin 动画定义 |

## will-change 属性

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/components/sidebar/sections/FoldersSection.vue:1620 | will-change | transform (虚拟滚动优化) |
| src/components/media/MediaGridCanvas.vue:1793 | 注释 | 提及 will-change (性能优化) |
| src/components/media/MediaGrid.vue:175 | 注释 | will-change 用于平移模式 |
| src/components/media/MediaGrid.vue:182 | 属性绑定 | :row-will-change (条件启用) |
| src/components/media/MediaGrid.vue:2572 | 注释 | 禁用 will-change/transition/hover-scale |
| src/components/media/MediaGrid.vue:2575 | will-change | auto (重置) |
| src/components/media/MediaGrid.vue:2672 | will-change | transform |
| src/components/media/MinimapAxis.vue:500 | will-change | transform |
| src/components/media/MediaScrollbar.vue:189 | will-change | transform |

## requestAnimationFrame 用法

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/perf/performanceRecorder.ts:151 | requestAnimationFrame | 性能监测帧回调 |
| src/perf/performanceRecorder.ts:191 | requestAnimationFrame | 性能监测帧回调 |
| src/composables/useAdjustPreview.ts:39-40 | requestAnimationFrame | 预览调整抖动聚合 |
| src/composables/useGridFlipReflow.ts:68 | requestAnimationFrame | 网格 FLIP 动画重排 |
| src/composables/useGalleryPerfProbe.ts:80 | requestAnimationFrame | 相册性能探针 tick |
| src/composables/useGalleryPerfProbe.ts:84 | requestAnimationFrame | 相册性能探针 tick |
| src/composables/useGalleryPerfProbe.ts:113-114 | requestAnimationFrame | 性能探针延迟回调 |
| src/composables/usePerformanceMonitor.ts:39 | requestAnimationFrame | 性能监测延迟 |
| src/composables/usePerformanceMonitor.ts:135 | requestAnimationFrame | 进度动画步进 |
| src/composables/usePerformanceMonitor.ts:138 | requestAnimationFrame | 进度动画步进启动 |
| src/composables/useHVirtualScroll.ts:79 | requestAnimationFrame | 虚拟滚动 raf 依赖注入 |
| src/composables/useHVirtualScroll.ts:217 | requestAnimationFrame | 虚拟滚动异步更新 |
| src/composables/useMediaDragToFolder.ts:136 | requestAnimationFrame | 拖拽绘制循环 |
| src/composables/useToolbarOverflow.ts:231 | requestAnimationFrame | 工具栏溢出检测 |
| src/composables/useWindowMode.ts:194 | requestAnimationFrame | 窗口模式调整 |
| src/composables/useVirtualScroll.ts:266 | requestAnimationFrame | 虚拟滚动变换更新 |
| src/composables/useVirtualScroll.ts:411 | requestAnimationFrame | 虚拟滚动位置同步 |

## scrollIntoView 和 scroll-behavior

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/views/AudioPlayer.vue:294 | scrollIntoView | behavior: 'smooth' 居中滚入 |
| src/views/SettingsView.vue:525 | 注释 | scroll-behavior: smooth 容器配置 |
| src/views/SettingsView.vue:994 | CSS scroll-behavior | smooth (设置容器平滑滚动) |
| src/assets/styles/animations.css:188 | CSS scroll-behavior | auto (禁用减速模式) |
| src/components/sidebar/accordion.helpers.ts:9 | 注释 | scrollIntoView block:'nearest' 语义说明 |
| src/components/sidebar/sections/FoldersSection.vue:522 | 注释 | scrollIntoView 虚拟化后失效,改索引→scrollTop |

## prefers-reduced-motion 支持

| 路径:行号 | 类型 | 动画对象/用途 |
|----------|------|-------------|
| src/components/doc/BookReader.vue:731 | @media | prefers-reduced-motion: reduce 动画禁用 |
| src/assets/styles/animations.css:181 | @media | prefers-reduced-motion: reduce 动画禁用 |
| src/components/ui/UiButton.vue:58 | @media | prefers-reduced-motion: reduce 动画禁用 |
| src/composables/useGridFlipReflow.ts:12 | 注释 | 尊重 prefers-reduced-motion 减少动效 |
| src/composables/useGridFlipReflow.ts:37-40 | 代码 | matchMedia 检测 prefers-reduced-motion 直接跳过过渡 |
| src/composables/useGridFlipReflow.ts:89 | 代码 | matchMedia 检测 prefers-reduced-motion 跳过动画 |

---

## 统计摘要

- **Vue Transition 组件**：8 个
- **CSS transition 属性**：119 个（超过 270 行源代码）
- **CSS @keyframes 定义**：14 个，对应 18 个 animation 实例
- **will-change 属性**：9 个
- **requestAnimationFrame 调用**：14 个文件，18+ 个调用点
- **scrollIntoView/scroll-behavior**：6 个引用
- **prefers-reduced-motion 支持**：6 个引用（3 个 CSS 媒体查询，3 个 TS 代码）

**动画库依赖**：无（package.json 未引入 motion/gsap/animejs 等）

**按目录分布**：
- src/views/：7 个
- src/components/：约 100+ 个
- src/composables/：约 25 个
- src/assets/：约 3 个
- src/perf/：2 个