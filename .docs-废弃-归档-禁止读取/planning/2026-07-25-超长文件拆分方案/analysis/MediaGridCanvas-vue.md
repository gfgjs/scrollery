---
id: 2026-07-25-MediaGridCanvas-vue
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# MediaGridCanvas.vue 拆分方案

> 目标文件:`src/components/media/MediaGridCanvas.vue`(1837 行,约 79KB)。
> 锚点原则:并行会话正在精简本文件注释,行号会漂移——本文档以**符号名**为主锚点,行号区间仅作现状快照(基于 2026-07-25 04:10 版本),施工时须以符号名重新定位。
> 本文档只做只读分析,不改任何代码。

## 0. 背景与约束复述

- Canvas 渲染是全库列表的性能热路径(零逐格 DOM + 算术命中 + 预取自适应,近期刚做完预取边距/进度条等收尾)。
- 拆分必须是**纯结构移动**:不得新增 `ref`/`computed`/`watch`,不得改变任何时序、命中判定、绘制顺序或缓存策略。
- `MediaGridCanvas.vue` 与宿主 `MediaGrid.vue` 之间的 props/emits 契约(见 §3)必须逐字节保留。

## 1. 现状结构图

文件三段:`<template>` 1-77、`<script setup>` 79-1750、`<style scoped>` 1752-1837。

### 1.1 Template(77 行)

单一职责:`.mgc-wrap`(高占位)→ 悬停卡层(单例 DOM,§T12)→ `<canvas>`(唯一交互元素,click/contextmenu/pointerdown/pointermove/pointerleave 五个原生事件)。体量小,不是本文件超长的成因,不建议拆(见 §3.5)。

### 1.2 Script setup 逻辑域(职责 + 约行数 + 符号锚点)

| # | 逻辑域 | 约行数 | 行区间 | 关键符号 |
|---|---|---|---|---|
| A | 依赖导入 + props/emits/根 refs | 84 | 80-180 | `defineProps`、`defineEmits`、`wrapRef`、`canvasRef` |
| B | 缩略图加载管线(命令式 LRU + 解码期预缩放 + 失败收口) | 248 | 182-429 | `thumbState`(`createCanvasThumbState`)、`AbortableThumbLoad`、`thumbLoads`、`getImage`、`loadBitmap`/`loadBitmapImpl`、`cancelAbortableThumbLoads`、`prioritizeVisibleThumbLoads`、`loadViaImage`、`onLoadFailed` |
| C | 调色板(挂载/换主题时读一次 CSS 变量) | 94 | 431-524 | `Palette` 接口、模块级 `let palette`、`readPalette()` |
| D | 视口测量 + DPR 适配 | 36 | 526-561 | `measure()`、`fitCanvas()` |
| E | 绘制调度 + 视口外预取(idle/draw 尾双通道) | 298 | 563-860 | `scheduleDraw`、`visibleItems*`、`selAnim`(`SelectionAnimTracker`)、`PrefetchPlan`、`ensurePrefetchPlan`/`walkPlan`/`runPrefetchSlice`/`runDrawPrefetchSlice`/`schedulePrefetchSlice`、`scrollDirection`、`draw()`(主循环,810-860) |
| F | 单格绘制原语(占位/图/描边/角标/星级/红心/checkbox/拖拽手柄) | 245 | 863-1073, 1122-1358 中的非overlay部分 | `drawCell()`(863-1014,复合入口)、`drawCheckbox`、`HANDLE_INSET`/`HANDLE_SIZE`、`drawHandle`、`drawPlayIcon`、`drawDuration`、`drawAvailBadge`、`drawExtText`、`drawTextCard`、`drawStars`、`drawHeart` |
| G | 信息浮窗(徽章行 + 信息行 + 逐帧缓存) | 101 | 1211-1311 | `infoGen`、`infoLineCache`、`infoLinesFor()`、`drawInfoOverlay()` |
| H | 分隔行绘制(日历/文件夹图标 + 药丸底 + folder 分组行内 sticky) | 73 | 1360-1432 | `FOLDER_ICON_PATH`、`folderIconPath2D`、`drawSeparatorIcon()`、`drawSeparator()` |
| I | 命中测试(算术二分,零逐格 DOM) | 62 | 1436-1492 | `idAtClient()`、`pick()`、`onClick`/`onContextMenu`、`hitHandleAt()`、`onPointerDown()` |
| J | 悬停跟踪 + 单例悬停卡(T1/T12) | 172 | 1494-1647 | `HoverCard`、`hoverLayerRef`、`hoverCard`(`shallowRef`)、`hoverSelected`/`hoverCardBare`(`computed`)、`prepareHoverCard`、`handleHoverTracking`、`pickWithRow`、`onPointerMove`/`onHoverCardPointerMove`/`onCanvasPointerLeave`/`onHoverCardPointerLeave`/`onHoverCardClick`/`onHoverCardContextMenu`/`onHoverCardPointerDown` |
| K | 生命周期 + 重绘触发源(watchers) | 102 | 1648-1749 | `onMounted`/`onBeforeUnmount`、10 个 `watch()`(currentY/rows/patchTick/selectionVersion/isSelectionMode/viewportMeta 组合/groupBy/showDragHandle/scrolling/compactCells/spacerHeight/themeToken/useThumbLoadGate） |

已下沉的既有纯模块(本次不动,作为拆分的依赖目标):`mediaGridCanvas.helpers.ts`(483 行:`hitTestCell`/`hitTestCellWithRow`/`coverRect`/`bitmapBucketH`/`bitmapPrepParams`/`SelectionAnimTracker`/`visibleRowRange`/`canvasPrefetchBudgets`/`canvasPrefetchItems`/`runPrefetchWalk`/`computeHoverRect`/`starPoints`/`truncateToWidth`/`clampStickyLabelY`,自带 `.spec.ts`)、`canvasThumbState.ts`(193 行,LRU 状态机)、`mediaGrid.helpers.ts`(324 行,`buildThumbInfoLines`/`docBadgeKind`/`typeBadgeOf`/`isTextCardFormat`/`isTextCardFallback`,自带 `.spec.ts`)。这三个文件已经是"逻辑下沉"的既有产物,本方案只处理仍留在 `.vue` 里的 script setup 部分。

### 1.3 Style(86 行)

`.mgc-wrap`/`.mgc-canvas`/`.mgc-hover-layer`/`.mgc-hover-card` 及其 keyframes。体量小,且悬停卡 CSS 与其 `<template>` 片段强耦合,不建议拆(见 §3.5)。

## 2. 拆分方案

### 2.1 总体思路

体量集中在 script(1670/1837 行)。拆分对象是**纯 TS 模块**(无 Vue 响应式,函数签名显式接参,零新增开销)与**composable 工厂**(把已有的 `ref`/`computed`/闭包状态原样搬进工厂函数,宿主里只调用一次,不在热路径内重新构造)。命名沿用仓库既有约定:与 `.vue` 同域的纯模块延续 `mediaGridCanvas.<domain>.ts` 前缀(现有 `mediaGridCanvas.helpers.ts` 已如此);有状态生命周期的部分放 `src/composables/useCanvasXxx.ts`(与 `useBucketVirtualScroll`/`useJustifiedLayout`/`useGridFlipReflow` 同构,均为"工厂返回命令式 API"风格,非典型 hook)。

### 2.2 下沉清单

| 新文件 | 职责 | 迁移符号(来自 §1.2) | 对外接口 | 依赖方向 |
|---|---|---|---|---|
| `src/components/media/mediaGridCanvas.palette.ts` | 调色板类型 + 默认值 + 读取 | 逻辑域 C 全部 | `interface Palette`;`defaultPalette: Palette`;`readPalette(el: HTMLElement): Palette`(**签名调整**:纯函数返回新对象,替代原地重写模块级 `let palette`——见 §3.2) | 无(仅 DOM `getComputedStyle` + `parseColorToRgb`) |
| `src/components/media/mediaGridCanvas.painters.ts` | 单格绘制原语 + 分隔行绘制 | 逻辑域 F(除 `drawCell` 本体)+ 逻辑域 H | `drawCheckbox`/`drawHandle`/`drawPlayIcon`/`drawDuration`/`drawAvailBadge`/`drawExtText`/`drawTextCard`/`drawStars`/`drawHeart`/`drawSeparatorIcon`/`drawSeparator`,均改为显式接收 `palette: Palette`(和 `drawSeparator` 额外接收 `groupBy: string`)而非闭包捕获 props/模块变量;导出 `HANDLE_INSET`/`HANDLE_SIZE`(命中测试模块要复用,见下) | 依赖 `mediaGridCanvas.palette.ts`(仅类型)、`colorLabels`/`format` 工具、`mediaGrid.helpers.ts`(`docBadgeKind`)、`mediaGridCanvas.helpers.ts`(`starPoints`/`clampStickyLabelY`) |
| `src/components/media/mediaGridCanvas.infoOverlay.ts` | 信息浮窗组装 + 逐帧缓存 | 逻辑域 G | 工厂 `createInfoOverlayCache()` 返回 `{ drawInfoOverlay(ctx, item, x, y, w, h, loaded, ctxArgs), invalidate() }`;`ctxArgs` 显式传入 `{ palette, viewportMeta, thumbInfoElements, showThumbInfo }`,`invalidate()` 对应原 `infoGen++` + `infoLineCache.clear()`,供宿主 watch 调用 | 依赖 `mediaGrid.helpers.ts`(`buildThumbInfoLines`/`typeBadgeOf`)、`mediaGridCanvas.helpers.ts`(`truncateToWidth`)、`utils/format`(`formatFileSize`) |
| `src/components/media/mediaGridCanvas.cellRenderer.ts` | `drawCell` 复合入口(选中动画几何 + 占位/图 + 覆盖层编排) | 逻辑域 F 中 `drawCell()` 本体 | 工厂 `createCellRenderer(deps): (ctx, item, sy, now) => boolean`,`deps` 一次性注入:`{ getPalette: () => Palette, selAnim: SelectionAnimTracker, getImage: (item) => CachedImage|null, isPendingDelete, isSelected, compactCells: () => boolean, showThumbInfo: () => boolean, thumbInfoElements: () => readonly string[], showDragHandle: () => boolean, isSelectionMode: () => boolean, drawInfoOverlay }` | 依赖 `mediaGridCanvas.painters.ts`、`mediaGridCanvas.infoOverlay.ts`、`mediaGridCanvas.palette.ts`(类型)、`mediaGridCanvas.helpers.ts`(`coverRect`)、`utils/format`(`formatDuration`)、`colorLabels` |
| `src/composables/useCanvasThumbPipeline.ts` | 图像缓存/加载/失败收口 + 视口外预取(idle+draw 尾双通道) | 逻辑域 B 全部 + 逻辑域 E 中预取相关部分(`PREFETCH_*`、`PrefetchPlan`、`prefetchPlan`/`prefetchIdleId`/`prefetchTimerId`、`cancelScheduledPrefetch`/`cancelPrefetchPlan`/`samePrefetchPlan`/`nextPrefetchItem`/`walkPlan`/`runPrefetchSlice`/`runDrawPrefetchSlice`/`schedulePrefetchSlice`/`ensurePrefetchPlan`、`scrollDirection`) | 工厂 `useCanvasThumbPipeline(opts: { cacheDir: () => string; onRequestThumb: (id: number) => void; onRegenerateThumb: (id: number) => void; scheduleDraw: () => void; viewport: () => { w: number; h: number } })` 返回 `{ getImage, prioritizeVisibleThumbLoads, ensurePrefetchPlan, runDrawPrefetchSlice, cancelPrefetchPlan, setScrollDirection(dir), dispose(): void, thumbState (供 draw() 读 size()/bytes()/loadingCount() 打点), getPrefetchProgress(): number }` | 依赖既有 `canvasThumbState.ts`、`useThumbLoadGate`/`useThumbLoader`(`isThumbLoadDeferred`/`buildThumbUrl`)、`mediaGridCanvas.helpers.ts`(`bitmapBucketH`/`bitmapPrepParams`/`visibleRowRange`/`canvasPrefetchBudgets`/`canvasPrefetchItems`/`runPrefetchWalk`)、`performanceRecorder` |
| `src/composables/useCanvasHitTest.ts` | 算术命中(点击/右键/拖拽手柄) | 逻辑域 I 全部 | 工厂 `useCanvasHitTest(deps: { canvasRef, rows: () => LayoutRow[], currentY: () => number, isSelected, showDragHandle: () => boolean })` 返回 `{ idAtClient, pick, pickWithRow, hitHandleAt }`;`HANDLE_INSET`/`HANDLE_SIZE` 从 `mediaGridCanvas.painters.ts` 导入(单一来源,见 §3.3) | 依赖 `mediaGridCanvas.helpers.ts`(`hitTestCell`/`hitTestCellWithRow`)、`mediaGridCanvas.painters.ts`(仅两个常量) |
| `src/composables/useCanvasHoverCard.ts` | 悬停跟踪 + 单例悬停卡状态机 | 逻辑域 J 全部 | 工厂 `useCanvasHoverCard(deps: { props 相关只读访问器、emit、hitTest: Pick<ReturnType<typeof useCanvasHitTest>, 'pickWithRow' \| 'hitHandleAt'>, viewport: () => {w,h} })` 返回 `{ hoverLayerRef, hoverCard, hoverSelected, hoverCardBare, clearHover, onPointerMove, onHoverCardPointerMove, onCanvasPointerLeave, onHoverCardPointerLeave, onHoverCardClick, onHoverCardContextMenu, onHoverCardPointerDown }`(与当前 template 绑定的名字逐一对应,模板零改动) | 依赖 `useCanvasHitTest.ts`(`pickWithRow`/`hitHandleAt`)、`useThumbLoader`(`buildThumbUrl`)、`mediaGridCanvas.helpers.ts`(`computeHoverRect`) |

### 2.3 宿主 `MediaGridCanvas.vue` 拆后残留(script 部分,估约 300-350 行)

- 逻辑域 A(props/emits/根 refs)—— `defineProps`/`defineEmits` 是编译宏,**必须**留在 SFC 顶层,不能下沉。
- 逻辑域 D(`measure`/`fitCanvas`)—— 体量小(36 行),且被 `draw()`/`onMounted`/`ResizeObserver`/`spacerHeight` watch 共用,建议保留在宿主,不单独拆(收益低于拆分成本,见 §4)。
- 逻辑域 E 中的 `scheduleDraw`/`visibleItems*`/`selAnim` 实例化/`draw()` 主循环 —— `draw()` 是编排根:调用 `pipeline.getImage/prioritizeVisibleThumbLoads/ensurePrefetchPlan/runDrawPrefetchSlice`、`cellRenderer(ctx, item, sy, now)`、`drawSeparator`(从 painters 模块导入),因此 `draw()` 留在宿主作为唯一的"每帧入口",不下沉。
- 逻辑域 K(生命周期 + watchers)—— 保留在宿主,作为跨模块编排层:`onMounted`/`onBeforeUnmount` 显式调用各工厂返回的 `dispose()`(见 §3.4 的顺序不变量);watch 块里调用 `hoverCard.clearHover()`/`pipeline.cancelPrefetchPlan()`/`pipeline.setScrollDirection()`/`infoOverlay.invalidate()` 等已下沉模块的导出函数。

### 2.4 依赖方向(单向,无环)

```
MediaGridCanvas.vue (宿主,组合根)
 ├─ mediaGridCanvas.palette.ts            (叶子)
 ├─ useCanvasThumbPipeline.ts             → canvasThumbState.ts / useThumbLoadGate / useThumbLoader / mediaGridCanvas.helpers.ts / performanceRecorder
 ├─ mediaGridCanvas.painters.ts           → mediaGridCanvas.palette.ts(仅类型) / colorLabels / mediaGrid.helpers.ts / mediaGridCanvas.helpers.ts
 ├─ mediaGridCanvas.infoOverlay.ts        → mediaGrid.helpers.ts / mediaGridCanvas.helpers.ts / utils/format
 ├─ mediaGridCanvas.cellRenderer.ts       → mediaGridCanvas.painters.ts / mediaGridCanvas.infoOverlay.ts / mediaGridCanvas.palette.ts(类型)
 ├─ useCanvasHitTest.ts                   → mediaGridCanvas.helpers.ts / mediaGridCanvas.painters.ts(仅 2 个常量)
 └─ useCanvasHoverCard.ts                 → useCanvasHitTest.ts / useThumbLoader / mediaGridCanvas.helpers.ts
```

无模块回引宿主 `.vue`(避免循环 import);跨新模块之间只有 `useCanvasHoverCard → useCanvasHitTest` 与 `cellRenderer → painters/infoOverlay` 两条内部边,其余均为叶子对既有 helpers 的依赖。

## 3. 风险与不变量

### 3.1 热路径零新增开销(最高优先级红线)

所有工厂函数(`createInfoOverlayCache`/`createCellRenderer`/`useCanvasThumbPipeline`/`useCanvasHitTest`/`useCanvasHoverCard`)必须在 `<script setup>` 顶层**只调用一次**(等价当前一次性的模块级初始化)。`draw()`/`drawCell` 内部只能调用已绑定好的函数引用,**不得**在 `requestAnimationFrame` 回调或逐格循环内重新构造闭包、对象字面量或再次调用工厂——否则会在热路径引入按帧/按格分配,直接违反"不得引入额外响应式开销"的红线,且会抵消近期刚落地的预取自适应收益(9cd4bfb/15db479 等)。

### 3.2 Palette 传参方式变更(唯一必要的签名调整,非行为改动)

现状:模块级 `let palette: Palette`,`readPalette()` 原地重写,所有绘制函数隐式闭包读取。拆分后 `readPalette(el)` 改为纯函数返回新对象,宿主保留 `let palette = defaultPalette` 并在 `readPalette` 调用点重新赋值,再作为**显式参数**逐层传入 `cellRenderer`/`painters`/`infoOverlay`。这是"闭包捕获"→"显式参数"的调用约定调整,**不改变任何取值时序或结果**(仍是"挂载/换尺寸/换主题时读一次,之后每帧引用同一个对象引用"),按值传对象引用零额外成本。施工时需对拍:主题切换(`themeToken` watch)后下一帧所有绘制点读到的必须是新 palette,不能有一帧新旧混用。

### 3.3 拖拽手柄几何单一来源(既有不变量,拆分后更易破坏)

原文件 1048 行注释已明确:`HANDLE_INSET`/`HANDLE_SIZE` 是 `drawHandle`(绘制)与 `hitHandleAt`(命中)的共享几何,"防两处漂移"。拆分后二者分处 `mediaGridCanvas.painters.ts` 与 `useCanvasHitTest.ts` 两个文件,**必须**从同一处 `export`(方案定为 painters 模块导出、hitTest 模块 import),不得在 hitTest 模块里复制一份常量——这是本次拆分中最容易被无意打破的既有不变量,施工 PR 应显式 diff 检查。

### 3.4 生命周期清理顺序(不变量,建议宿主集中编排而非各模块自注册)

现状 `onBeforeUnmount` 顺序:`setPointerIdResolver(null)` → `cancelHoverPrep()` → `ro?.disconnect()` → `cancelAnimationFrame(rafId)` → `cancelPrefetchPlan()` → `cancelAbortableThumbLoads()` → `thumbState.clear()`。拆分后建议**不**让 `useCanvasThumbPipeline`/`useCanvasHoverCard` 各自注册自己的 `onBeforeUnmount`(那样清理顺序会退化为"按 composable 调用顺序",与现状可能不完全一致且难以审查);而是让宿主的 `onMounted`/`onBeforeUnmount` 保留为**唯一编排点**,显式调用每个工厂暴露的 `init()`/`dispose()`,严格复刻现状顺序。这是本方案对"不得改变行为"最保守的落地方式。

### 3.5 与 MediaGrid.vue 的契约(不可变)

宿主 `src/components/media/MediaGrid.vue:73-105` 以具名 props(`rows`/`current-y`/`spacer-height`/`cache-dir`/`compact-cells`/`is-selected`/`is-pending-delete`/`pending-delete-label`/`avail-missing-label`/`avail-offline-label`/`grid-aria-label`/`selection-version`/`is-selection-mode`/`scrolling`/`show-thumb-info`/`show-drag-handle`/`patch-tick`/`thumb-info-elements`/`viewport-meta`/`group-by`/`theme-token`)与具名事件(`cell-click`/`cell-contextmenu`/`cell-pointerdown`/`request-thumb`/`cancel-thumb`/`regenerate-thumb`/`cell-favorite`/`cell-rate`/`cell-select`)绑定 `MediaGridCanvas`。`defineProps`/`defineEmits` 的类型声明必须逐字保留在 SFC 内,拆分只改变这些值在 script 内部如何被消费,不改变对外接口。

### 3.6 Template/Style 保持原位

Template 中悬停卡是刻意的"单例 DOM,非逐格镜像"(T12 裁决,原文件头部注释已强调),若为了"拆分"把悬停卡抽成子组件,会引入一次额外组件实例的 props/emit 响应式管线(哪怕薄),与"不得引入额外响应式开销"冲突,且体量(77 行模板 + 86 行样式)远低于本次超长文件的成因,**不建议拆**。

## 4. 收益与优先级

### 4.1 拆后预估大小

| 文件 | 预估行数 | 预估大小 |
|---|---|---|
| `MediaGridCanvas.vue`(宿主残留) | ~500-560(script ~340 + template 77 + style 86) | ~22-25KB |
| `useCanvasThumbPipeline.ts` | ~630 | ~27KB |
| `mediaGridCanvas.painters.ts` | ~320 | ~13KB |
| `useCanvasHoverCard.ts` | ~200 | ~9KB |
| `mediaGridCanvas.cellRenderer.ts` | ~155 | ~7KB |
| `mediaGridCanvas.infoOverlay.ts` | ~105 | ~5KB |
| `mediaGridCanvas.palette.ts` | ~100 | ~4KB |
| `useCanvasHitTest.ts` | ~90 | ~4KB |

宿主从 79KB 降到约 22-25KB(降幅 ~70%),且每个新文件都落在本次全仓扫描"观察名单"(40-50KB)以下,不会制造新的超长文件。

### 4.2 施工顺序建议(按"风险从低到高、可独立验证"排序)

1. **`mediaGridCanvas.palette.ts`**——纯数据 + 一个 DOM 读取函数,零依赖,先行验证"闭包→显式参数"模式跑通。
2. **`mediaGridCanvas.painters.ts` + `mediaGridCanvas.infoOverlay.ts`**——纯绘制原语,输入输出均为显式参数,最适合先补 characterization 测试再搬(见 §5)。
3. **`mediaGridCanvas.cellRenderer.ts`**——依赖上一步产物,验证 `drawCell` 复合入口在工厂化后逐像素/逐分支行为不变(可用现有 GUI 手测截图或后续可加的 canvas 快照测试对拍)。
4. **`useCanvasHitTest.ts`**——体量最小,验证 `HANDLE_INSET`/`HANDLE_SIZE` 单一来源(§3.3)落地正确。
5. **`useCanvasHoverCard.ts`**——依赖第 4 步,验证悬停卡状态机(token 竞态/换格清除/pointerleave 边界)全部行为不变。
6. **`useCanvasThumbPipeline.ts`**——体量最大、内部状态最复杂(LRU + 背压 + 预取双通道),放最后,施工前**强烈建议**先给 `getImage`/`loadBitmapImpl`/`cancelAbortableThumbLoads`/`prioritizeVisibleThumbLoads`/`onLoadFailed` 补 characterization 测试(现状零直接单测覆盖,只能靠挂载整个 SFC 间接触达),再做纯移动,避免在参数化/工厂化过程中意外改变时序。

每步都可独立编译 + `vue-tsc` + `eslint` 通过后再进行下一步,允许分批提交而不破坏中间态可运行性。

## 5. 验证策略

### 5.1 静态检查

- `vue-tsc --noEmit`(或项目既有的 TS 严格检查命令):确认新文件类型闭合、宿主残留的 `<script setup>` 无隐式 `any`(strict mode 硬约束)。
- ESLint(`npm run lint:fix` 仅用于本次改动涉及的文件,不做仓库级 `npm run format`)。
- 交叉检查 `mediaGridCanvas.painters.ts` 导出的 `HANDLE_INSET`/`HANDLE_SIZE` 与 `useCanvasHitTest.ts` 的 import 是同一处符号(非重复声明)——可用一次性 grep 核对,不依赖人工记忆。

### 5.2 Vitest(涉面单测)

- 既有 `mediaGridCanvas.helpers.spec.ts`/`mediaGrid.helpers.spec.ts` 保持全绿(未改动,回归基线)。
- 新增(建议,按 §4.2 顺序):
  - `mediaGridCanvas.painters.spec.ts`:对纯绘制函数用 mock `CanvasRenderingContext2D`(现有仓库若已有 canvas mock 工具复用,否则记录方法调用序列)做特征化快照,锁定 `drawHandle`/`drawCheckbox`/`drawTextCard` 等的绘制指令序列不因参数化而变。
  - `useCanvasThumbPipeline.spec.ts`:核心目标是**先于搬动**锁定现状——`getImage` 的两级失效(数据签名 vs 渲染规格)、`MAX_IN_FLIGHT_THUMBS` 背压、`cancelAbortableThumbLoads` 的 abortable 判定、`prioritizeVisibleThumbLoads` 的可见优先逻辑,这些在现状文件里完全没有直接单测覆盖,属于"改动前先补特征化测试"的项目红线(CLAUDE.md:"add characterization tests before changing untested critical behavior")。
  - `useCanvasHoverCard.spec.ts`:悬停卡 token 竞态(`hoverPrepToken`)、`handleHoverTracking` 的 `fromCard`/`scrolling`/`isPendingDelete` 分支、`onHoverCardPointerLeave` 的"钳位后仍命中同格保持"逻辑。

### 5.3 GUI 手测点(不可自动化,需真机核对)

- 快速甩滚(飞掠)下缩略图持续换入、无换图波抖动(对应近期 D-003/阶段 6 的既有裁决,不能回退)。
- 极密模式(<100px)大量格子同屏滚动,帧率与冷格数(`gallery.coldCellFrames` 探针)与拆分前打点对拍。
- 选中态过渡动画(`SelectionAnimTracker`)在框选/单选切换时缩放+圆角+遮罩过渡不闪烁。
- 悬停卡:普通格放大、选择模式下原位不放大、快速划过邻格换卡不残留、卡外扩悬空隙时不误清除。
- 拖拽手柄:开关(`showDragHandle`)关闭后既不绘制也不可命中(`hitHandleAt` 与 `drawHandle` 视觉/命中一致,验证 §3.3)。
- 主题切换(亮/暗/自定义)后下一帧调色板立即生效,无残留旧色。
- 信息浮窗设置(`showThumbInfo`/`thumbInfoElements`)切换后徽章/信息行立即刷新,无残留旧缓存(验证 `infoOverlay.invalidate()`)。
- 窗口 resize / DPR 变化下画布不糊、不错位。

## 顺手发现

`src/components/media/MediaGridCanvas.vue:1436-1452,1537-1544` — `idAtClient()`/`pick()`/`pickWithRow()` 三处"canvas 视口坐标 → 逻辑坐标"换算逻辑逐字重复(`rect.left/top` → `x`/`logicalY`),仅返回值形态不同 — 建议拆分 `useCanvasHitTest.ts` 时顺手抽一个内部 `clientToLogical(e)` 共享,消除三处潜在漂移点(本次不改代码,留待施工阶段处理)。

## 附注:CSS 外置(D-451,2026-07-25 补录)
- 本组件 `<style scoped>` 可整块外置为同目录 `MediaGridCanvas.styles.css`,SFC 留 `<style scoped src="./MediaGridCanvas.styles.css"></style>`;外置文件仍编译为宿主组件 style 块,scope id 归属不变,`:deep()` 穿透语义不变。
- 前提(2026-07-25 核实):全仓 .vue 样式零 `v-bind()`;本文件为单一 `<style scoped>` 块。
- 定位:可选先行批、全场风险最低的行数削减刀;不替代 script 拆分主刀。
- 施工顺序:首刀拿最小文件实测 Vite 构建链 + HMR,通过后铺开。
