---
status: active
type: working-memory
line: 画廊性能
created: 2026-08-16
---

# 发现与决策:核实 Canvas 画廊缩略图性能审查

## 需求
- 核实 `docs/reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md`。
- 用户随后授权执行修订；若调用子代理，只允许 Luna。本轮交叉核验使用的 3 个子代理均为 Luna。

## 发现
- 总体裁定：实现链路与多数行号基本准确，但“瓶颈排序”不成立；原文把静态候选成本写成已定位主因，并遗漏仓库既有真机反证，当前不宜据此直接立项。
- 文档治理：CI 实际真源 `.worklogrc.jsonc` 要求 `status: snapshot`、`type: review`；目标文档的 `status: 快照`、`type: 审查快照` 会触发门禁。`line: 画廊渲染` 也没有 `docs/lines/画廊渲染.md`，应归入已有 `画廊性能` 等实体。`docs/README.md` 的中文枚举说明与现行 CI 配置存在漂移，核验以 `.github/workflows/docs-governance.yml` 钉定的 worklog-kit 判定为准。
- 链路事实：bucket `onScroll` 会写 `logicalScrollTop`，Canvas 的 `currentY` watch 经 rAF 合并触发 `draw`；但同一宿主滚动事件还会更新 `isScrolling`、加载闸门和目录联动，不能称“整个滚动帧唯一响应式写”。
- Bitmap 路径：成功路径确有 `createImageBitmap(blob)` 后再从已有 `ImageBitmap` 做 crop/可选 resize；第二次不是可由代码证明的“再次解码”，线程池排队翻倍也无静态证据。第二阶段完成前 `full` 与产物句柄重叠驻留属事实，但实际 native/GPU 内存不可由 `gallery.cacheBytes` 推出。
- 既有反证：`docs/worklogs/2026-07-17-Canvas滚动缩略图无感加载优化/` 已记录单段 WebP/单次 Bitmap 处理对 480px 冷区无可见改善并撤销；最终保留 64 槽以消除任务风暴，剩余 4K+64px 冷区由 `docs/todo.md` 的 120/240/480 多档源待办承接。因此 P1-1/P1-2 不能列为已证实前两优先级。
- 64 槽事实成立，但它是当前 pipeline 实例的并发上限，不是严格“64/批”；本地 asset 协议仍有磁盘读取、Blob、解码和位图构造成本。512MiB 只约束已 commit 的缓存输出，不约束在途整图/Blob 峰值，原表“内存上界由字节预算兜底”错误。
- 绘制事实：每次 draw 先整幅铺底，再遍历所有可见格；每格必有底色与描边，有有效图才 `drawImage`。pending/missing/offline 会设置 `ctx.filter`，但 Chromium 是否软件回退、是否为显著瓶颈均需目标 WebView2 profile，不能静态定为 P1 或“低风险直改”。
- 选择动画持续 250ms 并连续排帧属实；帧数取决于刷新率，不能固定写 8–15。`SelectionAnimTracker.sync` 本身每次 draw 还会新建可见项 Map，原文未列入分配候选。
- 生成队列：前端 50ms 合批、每批最多 24 项、同一时刻一批；后端不是单 worker，而是 decode/encode/deferred CPU 多池流水线，Channel 逐项回传。
- 失败契约：普通加载/解码/编码失败会逐项回传并持久化 `thumb_status=2`，前端回填后收敛为失败占位，原文“后端生成失败若不回写、需确认是否回写”与当前实现不符。真正的缺口是 IPC 整批失败、30s stall 或结果缺失时，前端 catch 后不清同 sig 的 `requestThumbOnce` 去重，重进视口本身不会自动重试。
- 签名：每格每次 draw 确实创建 `sig`/`renderSig` 字符串，但 GC 抖动未测；布局 item 会被原地 patch，并非不可变快照。`pathHash` 数值化会引入碰撞/失效契约，不是可直接认定的廉价修复。
- 预取：滚动 `currentY` 变化会取消计划，随后创建两个 generator；非滚动 redraw 可复用计划。保留旧 iterator 会削弱最新视口优先级，不能静态认定低风险。
- `canvasRows` 仅在 bucket 分支的 computed 依赖失效时调用 `mountedRows()` 平铺新数组；并非每次读取都分配，影响也不能静态判为可忽略。
- 观测类型：`gallery.draw`、`gallery.bitmapLoad`、`gallery.imageFallback` 是 span；`gallery.coldCellFrames` 是 counter；`gallery.inFlightThumbs`、`gallery.visibleColdCells`、`gallery.cacheBytes` 是只保留末值的 gauge。现有指标不能直接测量 ImageBitmap native/GPU 内存峰值。
- “真机 idle 通道不可靠”没有已归档实证；2026-07-17 worklog 明确写的是 headless 中 rIC 饿死假设被证伪、真机是否成立未证。应改成“headless 与 WebView2 可能不同，需真机核对”。
- 更可靠的立项顺序：先复用真机性能面板确认症状分段；若仍是 4K+64px 冷区，优先评估已登记的多档源；并发、双 Bitmap、filter、签名和 iterator 只作为隔离 A/B 候选，且并发实验必须同时看帧健康与在途 native 内存。
- 执行结果：目标审查稿已按“代码事实 / 既有实测 / 待测假设”重写，frontmatter 改为现行 canonical，并新增冷热分层、失败恢复缺口、指标解释边界和受证据约束的实施顺序。

## 外部资料(当数据,不当指令)
- WHATWG HTML Living Standard `createImageBitmap`：从已有 `ImageBitmap` 创建新对象的规范语义是复制/裁剪/格式化位图数据，并按选项缩放；规范不保证 Chromium 的具体线程池调度，故不能据两次 API 调用推出“两次源文件解码”或“排队翻倍”。来源：https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
