---
status: snapshot
type: working-memory
line: 主页Canvas性能优化
created: 2026-07-13
---

# 发现与决策:主页Canvas性能优化

## 需求
- 检查主页画廊和主页右侧时间/文件夹轴从 DOM 迁移到 Canvas 后的代码实现，并尝试优化性能。
- 所有完成声明必须有测试、typecheck、build 或运行结果等可观察证据。
- 保持现有 Rust 脏改动 `src-tauri/src/db/queries.rs`、`src-tauri/src/exotic/installer.rs` 不受影响。

## 发现
- 2026-07-13 开工时当前分支已有两处与任务无关的 Rust 修改，前端范围可独立施工。
- 既往审计指出 `MediaGridCanvas.vue` 的缩略图中间 `ImageBitmap` 异常路径、pending-delete 重绘以及不可取消的在途加载值得复核；本次只以当前源码为准。
- 当前 `MediaGridCanvas.vue` 每帧先由 `visibleItems()` 线性扫描全部挂载行构造选中快照，再由 `draw()` 第二次线性扫描全部挂载行；`hitTestCellWithRow()` 的每次指针命中也从首行开始线性扫描。bucket 引擎挂载段扩大时，这些成本随挂载段增长，而不是只随视口增长。
- 当前 `TimelineScrubberCanvas.vue` 的 `drawBars()` 在每个 `currentY` 重绘帧遍历全部 `monthBuckets` 或 `separators`；folder 分组较多时，滚动帧仍有 `O(全部分组)` 的 Canvas path/fill 工作。
- `TimelineScrubberCanvas.vue` 已把非 bars 密度带色串、调色板和指针事件做了缓存/逐帧节流，适合继续把不随滚动变化的静态轴层缓存，而不是重复已有优化。
- `MediaGridCanvas.vue` 的 `loadBitmap()` 只在第二次 `createImageBitmap()` 成功后关闭中间 `full`；裁剪/缩放抛错会泄漏该 `ImageBitmap`，随后又走 `Image()` 回退并额外占用资源。
- 针对性基线：`canvasThumbState`、`mediaGridCanvas.helpers`、`timelineScrubber.helpers` 共 133 个测试全部通过。
- 浏览器 harness 已实证画廊与右侧轴同时运行 Canvas 路径：`.mgc-canvas`/`.tlc-canvas` 均为可见 `CANVAS`，画廊 `.media-card` 数量为 0；日期/文件夹模式切换、滚到底部、轴 hover 放大镜与浮层均可工作。
- harness 的 layout summary 固定为日期样例，无法生成数千文件夹 separator；切换 folder 时还会因未覆盖 `get_directory_ancestors` 触发既有 `useFolderTree.expandToNode` error。该缺口限制大规模文件夹联动验收，但未发现 Canvas 自身错误。
- 完整门禁：69 个 Vitest 文件、843 个测试通过；`vue-tsc --noEmit`、ESLint、Vite production build 均通过。build 仅报告仓库既有的 mixed dynamic/static import 与大 chunk 警告。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 有序布局行的可见窗口和命中应使用二分定位，使每帧遍历成本与可见窗口而非挂载段规模相关 | code/test |
| F-002 | 右侧 Canvas 轴应区分静态密度层与动态 current/hover 层，静态层仅在数据、尺寸、主题或模式变化时重建 | code/test |
