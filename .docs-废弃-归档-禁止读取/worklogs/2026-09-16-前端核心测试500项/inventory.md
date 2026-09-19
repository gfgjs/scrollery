---
status: snapshot
type: review
line: 仓库架构与流水线全面梳理
created: 2026-09-16
---

# 500 项预算下的核心测试清单

最终实跑：27 文件、487 独立展开用例、487 通过、0 失败、0 跳过。原始 1999 项，前一轮 1946 项；当前保留 487 项，预算余量 13 项。

| 核心测试文件 | 用例 |
|---|---:|
| [scripts/vite-plugin-bundle-budget.spec.mjs](../../../scripts/vite-plugin-bundle-budget.spec.mjs) | 14 |
| [src/commands/commandCatalog.core.spec.ts](../../../src/commands/commandCatalog.core.spec.ts) | 22 |
| [src/commands/commandDispatch.core.spec.ts](../../../src/commands/commandDispatch.core.spec.ts) | 19 |
| [src/components/media/canvasPipeline.core.spec.ts](../../../src/components/media/canvasPipeline.core.spec.ts) | 18 |
| [src/components/media/galleryGeometry.core.spec.ts](../../../src/components/media/galleryGeometry.core.spec.ts) | 46 |
| [src/components/sidebar/sections/folderTree.core.spec.ts](../../../src/components/sidebar/sections/folderTree.core.spec.ts) | 25 |
| [src/composables/selection/selectionCore.spec.ts](../../../src/composables/selection/selectionCore.spec.ts) | 26 |
| [src/composables/useAnalysisController.spec.ts](../../../src/composables/useAnalysisController.spec.ts) | 19 |
| [src/composables/useBucketVirtualScroll.spec.ts](../../../src/composables/useBucketVirtualScroll.spec.ts) | 46 |
| [src/composables/useCanvasThumbPipeline.spec.ts](../../../src/composables/useCanvasThumbPipeline.spec.ts) | 15 |
| [src/composables/useFolderTree.spec.ts](../../../src/composables/useFolderTree.spec.ts) | 9 |
| [src/composables/useRequestQueue.spec.ts](../../../src/composables/useRequestQueue.spec.ts) | 22 |
| [src/composables/useSettingsLifecycle.spec.ts](../../../src/composables/useSettingsLifecycle.spec.ts) | 9 |
| [src/composables/useTauriListen.spec.ts](../../../src/composables/useTauriListen.spec.ts) | 3 |
| [src/harness/ipcFixtures.spec.ts](../../../src/harness/ipcFixtures.spec.ts) | 6 |
| [src/i18n/core.spec.ts](../../../src/i18n/core.spec.ts) | 4 |
| [src/stores/aiStore.modelMissing.spec.ts](../../../src/stores/aiStore.modelMissing.spec.ts) | 6 |
| [src/stores/aiStore.spec.ts](../../../src/stores/aiStore.spec.ts) | 9 |
| [src/stores/backupStore.spec.ts](../../../src/stores/backupStore.spec.ts) | 6 |
| [src/stores/directoryOperations.spec.ts](../../../src/stores/directoryOperations.spec.ts) | 24 |
| [src/stores/mediaStore.spec.ts](../../../src/stores/mediaStore.spec.ts) | 17 |
| [src/stores/scanStore.spec.ts](../../../src/stores/scanStore.spec.ts) | 29 |
| [src/stores/settingsPersistence.spec.ts](../../../src/stores/settingsPersistence.spec.ts) | 30 |
| [src/themes/core.spec.ts](../../../src/themes/core.spec.ts) | 21 |
| [src/utils/contentRendering.core.spec.ts](../../../src/utils/contentRendering.core.spec.ts) | 26 |
| [src/utils/ipc.spec.ts](../../../src/utils/ipc.spec.ts) | 6 |
| [src/utils/paths.core.spec.ts](../../../src/utils/paths.core.spec.ts) | 10 |
