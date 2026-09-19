---
id: 2026-07-01-T20_T18-layout_布局策略接缝_合并设计
status: active
type: design
line: 布局策略 T20
created: 2026-07-01
---

# T20 + T18(布局切片)合并设计:布局策略接缝

> 研究结论文档。回答的问题:**T18(巨组件拆分)与 T20(Grid/Justified 布局切换)既然都改 MediaGrid,能否合并为一次重构?**
> 结论:**只有 T18 的"布局渲染切片"应与 T20 合并;T18 的其余拆分项与 T20 正交,应独立推进。**
> 性质:架构研究 + 分阶段计划。✅ **轨道 A(A1+A2)已落地并交付**(A1 `206fb88`、A2 后端 `9f5de07`+前端 `6ef5bfc`);§4 几何决策已按 (a) 后端 uniform-packing 拍板执行。轨道 B 部分完成,详见 §5/§6 回写。

## 1. 背景:两个任务的原始定义

- **T18(§3.10.1,P3 重构债)**:MediaGrid 1581 行、7+ 职责,纯重构拆成 `useMediaFolderDrop` / `useGridContextMenu` / `useBatchOps` / `TimelineSidebar` / `useDimPriority` 等,**零行为变更**。
- **T20(§3.11.1,P3)**:新增 **Grid(固定宫格)** 模式 vs 现 **Justified(等高行)**;AppToolbar 加切换按钮,复用已有 `gridRowHeight` 密度滑块;plan 原话"**优先前端 CSS grid**(object-fit cover 方图)"。

## 2. 关键架构发现(实测代码得出)

### 2.1 虚拟滚动已是布局无关的(决定性)

`useVirtualScroll`([src/composables/useVirtualScroll.ts](../../src/composables/useVirtualScroll.ts))只依赖一个抽象契约:

```
opts = {
  totalHeight: () => number          // 逻辑总高
  totalRows:   () => number          // 总行数
  fetchRowsByY: (topY, bottomY) => Promise<LayoutRow[]>   // 按逻辑 Y 区间取行
  containerRef, layerRef, rowHeight
}
// 每行只需 row.y / row.height（+ row.rowType / items）
```

它**不知道行从何而来**。所有难点——百万级坐标平移(物理 ≤ SAFE_MAX 10M px、逻辑可达 40M)、`renderAnchor` 锚定、`isTranslated` 平移模式、滚轮惯性补偿——**全部与布局模式无关**,只对 `row.y/height` 几何成立。

> 推论:Grid 模式**不需要**碰这套机器。它只需提供另一组 `{ totalHeight, totalRows, fetchRowsByY }`,几何换成均匀方格即可。

### 2.2 模式相关的部分很窄

整条管线里只有三处是 justified 专属:

| 处 | justified 现状 | grid 需要 |
|---|---|---|
| **行供给** `fetchRowsByY` | IPC `get_layout_rows_by_y`,后端按等高行 width-packing | 均匀行:每行固定列数、行高=方格边长 |
| **几何源** `totalHeight/totalRows` | 后端 `compute_layout` 产出并缓存 | `ceil(N/cols) * (cell+gap)` |
| **单元渲染**(MediaGrid 模板 ~88-145) | `item.w/item.h` 保宽高比 | 方格 + `object-fit: cover` 居中裁切 |

其余一切(选区、拖图、右键、批量、时间轴、缩略图队列、键盘)与布局模式**无关**。

### 2.3 因此 T18 的拆分项里,只有"布局"与 T20 耦合

plan §3.10.1 把 T18 拆成 5 块。逐一判耦合:

| T18 拆分项 | 与 T20(布局模式)耦合? |
|---|---|
| **布局消费/渲染**(useJustifiedLayout + 虚拟滚动接线 + 行模板) | **强耦合** —— T20 就是给它加第二实现 |
| useMediaFolderDrop(拖图入文件夹) | 正交 |
| useGridContextMenu(右键菜单) | 正交 |
| useBatchOps(批量收藏/删除/移动/设色) | 正交 |
| TimelineSidebar(时间轴边栏) | 正交(消费 monthBuckets,与单元布局无关) |
| useDimPriority(缩略图取图优先级) | 正交 |

## 3. 结论与建议:拆成两条轨道

### 轨道 A(**合并**):布局策略接缝 = "T18-布局切片" + T20

把当前 justified 逻辑抽到一个**布局策略接口**后,T20 = 加第二个策略。

```ts
// 设想接口（落地时按实测命名调整）
interface GalleryLayoutSource {
  totalHeight(): number
  totalRows(): number
  fetchRowsByY(topY: number, bottomY: number): Promise<LayoutRow[]>
  cellShape: 'aspect' | 'square'   // 驱动模板:保比 vs 方图裁切
  recompute(params): Promise<void> // 替换现 useJustifiedLayout.compute
}
```

- **策略 #1 `justifiedSource`**:把现 `useJustifiedLayout` + `mediaStore.computeLayout` + `get_layout_rows_by_y` 接线原样搬入,`cellShape='aspect'`。**零行为变更**,可独立验证。
- **策略 #2 `gridSource`**:`cellShape='square'`,几何均匀。
- MediaGrid 持有 `activeSource`,由 `uiStore.layoutMode`('justified'|'grid')切换;`useVirtualScroll` 绑 `activeSource` 的三函数,**模板按 `cellShape` 分支** item 尺寸/裁切。AppToolbar 加切换按钮。

**为何必须合并而非先后独立做:**
- 若先做 T18-布局**而不预见** T20:抽接缝时极易把 justified 假设(变宽高行、后端 width-packing)焊死进接口签名,T20 再来要返工拆第二次——违背重构初衷。
- 若先做 T20**而无接缝**:第二条布局路径只能硬塞进 1581 行巨组件,把"巨组件"问题做得更糟。
- 二者共用**同一个接缝**,co-design 一次成型最省。

### 轨道 B(**保持独立**):T18 其余拆分项

`useMediaFolderDrop` / `useGridContextMenu` / `useBatchOps` / `TimelineSidebar` / `useDimPriority` 与 T20 正交。各自一次"零行为变更"抽取,**可任意穿插、与 T20 无依赖**。不必为 T20 等待,也不应捆进 T20 的验证门。

## 4. 必须前置拍板的 T20 设计决策(唯一硬岔路)

**Grid 几何在哪算?** 两条都可行,影响后端是否动:

- **(a) 后端 grid-packing 模式**:`compute_layout` 加一个 uniform-grid 排版分支(固定列、方格、均匀行高),照样缓存 → `get_layout_rows_by_y` **完全复用、前端零改动取行**。代价:后端加一个布局模式(但均匀排版极廉价)。整套 SAFE_MAX/平移机器**原样生效**。
- **(b) 纯前端 uniform 行生成**:契合 plan"优先前端 CSS grid"原话,但前端要 ① 新增"按布局序 index 区间取 item 数据"的 IPC(现有 `get_layout_rows_by_y` 按 Y、`get_view_ids` 只给 id,**都不直接给区间内的完整 LayoutItem**);② 在前端自行复刻 totalHeight/row.y 几何并override 虚拟滚动的几何源。

**我的倾向:(a)。** plan 写"优先前端"时未计入 SAFE_MAX/坐标平移与后端布局缓存的耦合——纯前端 grid 得把这套精度/平移逻辑重新推导一遍,反而更重。后端 uniform-packing 让 grid 与 justified 共享**同一条取行+虚拟滚动+平移**通路,是当前架构下回归面最小的选择。

✅ **已拍板并落地(用户选 (a))**:`compute_layout` 加 `compute_grid_layout` 分支(固定列 `⌊(W+gap)/(target+gap)⌋`、方格撑满宽、复用 `group_key`/分隔符),`get_layout_rows_by_y` 前端零改动取行,SAFE_MAX/平移机器原样生效。原设想的 `cellShape` 前端分支因几何后端权威、MediaThumb 既有 `object-fit:cover` 自然裁方图而**撤掉**(避免未用抽象),MediaGrid 模板零改。

## 5. 分阶段执行计划(落地时)

| 阶段 | 内容 | 验证门 | 状态 |
|---|---|---|---|
| **A1** | 抽 `GalleryLayoutSource` 接缝,justified 作策略 #1,行为不变 | 视觉 + 滚动回归(你跑):大库滚动/平移模式/时间轴跳转/筛选重排全不变 | ✅ 已交付(`206fb88`),vue-tsc/eslint/vitest 全绿(仅本地) |
| **A2** | 加 `gridSource` 策略 #2(几何方案见 §4)+ AppToolbar 切换 + `uiStore.layoutMode` 持久化 | 两模式切换、grid 方图裁切、两模式下滚动/选区/批量均正常 | ✅ 已交付(后端 `9f5de07` + 前端 `6ef5bfc`);模板 cellShape 分支因方案 (a) 不需要而撤掉 |
| **B**(可穿插) | T18 其余拆分项独立抽取 | 各自零行为变更回归 | 🌓 部分完成,见 §6 |

**每阶段一个 commit、过验证再下一步**(全局纪律)。A1 是天然 checkpoint。

## 6. 待办边界(诚实标注,2026-07-02 回写)

- 🔴 本文档原「尚未动代码」表述已被推翻:轨道 A(A1+A2)已完整落地,§4 决策已拍板执行,详见 §3/§4/§5。
- 轨道 B 现状(逐项对照原 5 块拆分设想):
  - **useMediaFolderDrop** → 已交付,实为 `useMediaDragToFolder`(`5da1c7a`),整簇拖图状态机抽出,**已人工手测通过**(左键移动/Shift 复制/右键落点菜单/幽灵跟手/文件夹高亮均正常)。
  - **TimelineSidebar** → 已交付,实为 `TimelineScrubber.vue`(T14,`4536b63`),独立子组件 + 纯逻辑抽 `timelineScrubber.helpers.ts` 补 21 单测。
  - **useDimPriority** → 已交付,实为 `useViewportDimPriority`(`ccee891`)。
  - **useGridFlipReflow**(原设想外新识别的干净单元)→ 已交付(`101f875`),FLIP+淡出动画三件套,只依赖渲染层。
  - **useBatchOps / useGridContextMenu** → 🔴 **有意不抽**(重读实际代码后修正原拆分设想):`pendingDeleteIds` 被模板深度绑定(v-memo/卡片 class/角标)+ FLIP 图层 + 对话框 + history,硬抽属 coupling-relocation(把耦合搬家而非解耦),留在组件内更诚实。此裁决见 todo.md D 节「有意不抽」记忆背书,R2-3 已引用尊重此裁决。
- 上述抽取均按项目「未覆盖自动化验证需人工步骤」的降级 Done 标准如实标注,部分(如拖拽)已获人工手测确认,详见各自 commit 说明。
