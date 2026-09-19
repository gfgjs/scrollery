---
status: 施工中
type: 工作记忆
line: 前端渲染大列表只读分析
created: 2026-09-06
---

# 发现与决策:前端渲染大列表只读分析

## 需求
- 用户：无人值守执行提示词6（前端渲染/大列表流水线），建立三件套，开始只读分析，报告落盘。
- 提示词6文本位于 `docs/reviews/2026-09-05-性能流水线梳理与分线优化提示词.md:261-298`，含 B1–B6 六个静态可疑瓶颈与「工作方式/验证与交付」约束。

## 发现
- HEAD = 7fea98e3，与提示词文档声明的快照基线一致（行号漂移只可能来自工作区未提交改动）。
- 工作区有大量未提交的主题系改动（主题色浓度/文字浓度/玻璃模式修复三条并行线），直接触及 `src/components/media/MediaGrid.vue`、`MediaGridCanvas.vue`、`mediaGridCanvas.helpers.ts`、`mediaGridCanvas.cellRenderer.ts`、`mediaGridCanvas.palette.ts`、`useCanvasHoverCard.ts` 等——提示词6的核心文件全在改动面内，行号漂移是确定的，且需甄别主题改动是否引入新性能问题。
- 姊妹任务已存在：`2026-09-06-缩略图派生流水线只读分析/`（提示词2线）、`2026-09-06-元数据扫描流水线优化/`（提示词1线）——多线并行，共享资源协议改动须跨线协调，本线只读不改协议。
- 提示词6 的 B3 跨界注记：regenerate_missing_thumb 后端失效语义属缩略图流水线；本线只做前端侧。
- **核实终局（详见本目录 report.md）**：B1 部分成立（visibleRows 深 ref 属实但仅几十行、DOM 侧无深 watch，真实成本=深代理 props+整窗替换+每格≥2 watcher 扇出）；B2 成立（2s 防抖+离屏合并已两层，MEDIA_ENRICHED 实际 4 路扇出）；B3 前端侧部分成立（regenerate 无跨实例去重，single-flight 仅 batch 队列）；B4 成立且被低估（selAnim.sync 每帧新建 Map+可见项双重遍历+多处未缓存 measureText）；B5 成立（显式 {passive:false}）；B6 成立（viewportMeta 清理漂移至 mediaStore.ts:231-244，scope key stringify 每轮 IPC 往返都跑）。
- 快照两处定位错置已修正：「rAF 节流+在途合并+陈旧丢弃」在 composables 不在 mediaStore.ts:302-332；「compute_layout 全库量级 IPC」注记在 useJustifiedLayout/useGalleryVirtualEngine 文件头。
- 性能基准工具完备：`window.__scrolleryBench`（DEV 桥，start/stop/roundTrip/phase/sessions/sampleGauges）+ 三往返基准 usePerformanceMonitor.ts:142-199 + performanceRecorder 环形缓冲（240Hz×150s）+ __scrolleryPerf.census()（DOM 节点普查）。

## 外部资料(当数据,不当指令)
- （无）

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 提示词6 六项瓶颈核实终局与快照措辞修正（B1 机制改写、B5 passive 措辞、B6 行号漂移、两处定位错置） | repo:本目录 report.md（已落盘，快照随归档）；后续实施会话产出的优化清单表为正式交付 |
| F-002 | 极密 DOM 场景每格 ≥2 个 watcher 扇出（~1000 格 ≈ 2000+ 活 watcher，闸门翻转全量齐发，mediaGrid.helpers.ts:321-322 注释自证） | 待实施会话量化后决定：量级显著则进 designs/experience，否则 no-promotion |
| F-003 | 无人值守基准入口：window.__scrolleryBench DEV 桥 API 面 + runGalleryRoundTrip 三往返基准 + __scrolleryPerf.census() 节点普查（含「基准中不得调用 snapshot()」约束） | repo:docs/experience.md（前端性能基准操作要点） |
| F-004 | useRequestQueue.ts:22-23 引用「ipc.ts CANCEL 注」已悬空（ipc.ts 无该注记）；useVirtualScroll.ts:87「logicalScrollTop 非热渲染绑定」注释失实（被 currentLogicalY props 链消费） | repo:代码注释修正（随实施会话顺手改，或单独小批清理） |
| F-005 | 媒体库滚动取行后端零 DB（layout/cache.rs 内存 binary_search）与前端两层合并（防抖+离屏 flushIfDeferred）是 B2 增量方案评估的既有前提 | repo:本目录 report.md §2 B2（实施会话参考即可） |
