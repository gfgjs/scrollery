---
status: snapshot
type: working-memory
line: 缩略图流水线
created: 2026-09-12
---

# 发现：缩略图生成流水线性能调查

## 需求
- 调查缩略图生成流水线，分析性能瓶颈。

## 基线
- 工作区已有另一轮 Canvas 优化的暂存文档和 `.research-tmp/` 未跟踪目录，本轮保留。
- 后端主要入口为 `src-tauri/src/thumbnail/`、`src-tauri/src/ipc/thumbnail_full_gen.rs`、`src-tauri/src/ipc/thumbnail_commands.rs`。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 现有指标不足以分解生成瓶颈，需独立记录排队、解码、后处理、写盘、DB 和首次可见时间 | 本线状态与本轮报告 |
| F-002 | 核实批间串行、线程池生命周期、全量回退屏障、CPU全解码及像素队列容量，修正过强性能推断 | 本轮调查报告 |

## 主会话核实：测量边界
- `generator.rs:491–541` 的 ENCODE_OK elapsed_ms 覆盖可选 AI 缓存、resize、ICC、ThumbHash、WebP/JPEG、目录检查与原子写盘；不能读成纯编码耗时。
- `thumbnail_commands.rs:146` 有批量 IPC 总 span；`thumbnail_full_gen.rs:248,648` 有整轮计时。尚未发现阶段队列等待/解码耗时的完整分解。
- `src-tauri/benches/logging.rs:134–174` 是 256×256 PNG、CPU、30 次串行 generate_thumbnail 的日志成本实验，不能代表多 worker 全库吞吐。
- `scripts/bench/canvas-realapp-bench.mjs:19–25` 的 cold 是已生成缩略图的 LRU 冷加载，不能充当未生成图库基准。
- 7 月 A/B 文档的 7.3s vs 16–20s 是历史记录。本轮未复现，不能据此推导当前速度或阶段占比。
- 旧报告把 WIC scaler 表述成全图分配并不严谨：当前 wic_engine.rs:109–149 在 scaler 后按目标尺寸分配输出；codec 内部工作集不能从调用顺序直接证明。

## 终审整合
- 前端常态单批24项，批回齐后再等50ms；不能把普通滚动假定为无限多批并行。
- 16逻辑核 budget=12，单批实际30个阶段worker，另加dispatcher/blocking线程；跨全量与视口未共享预算。
- 只有encode队列携带像素；4:3/512档/1024项推算768MiB，不应乘两条队列；普通24项批不可能填满。
- CPU EXIF只尝试一次，GPU入口未试EXIF；常态主图resize只有一次，encode满足尺寸时复制。AI缓存放大同次解码目标，不是必然多一次源解码。
- 收口以 reviews/2026-09-12-缩略图生成流水线性能调查.md 的限定结论为准。
