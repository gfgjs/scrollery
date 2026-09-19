---
status: 施工中
type: 工作记忆
line: 缩略图派生流水线只读分析
created: 2026-09-06
---

# 发现与决策:缩略图派生流水线只读分析

## 需求
- 用户指令：无人值守不停顿；使用 `docs/reviews/2026-09-05-性能流水线梳理与分线优化提示词.md` 中的**提示词2**（缩略图/派生文件生成流水线）；建立三件套；开始只读分析；报告落盘。
- 提示词2要点：核实三个触发入口（视口按需 batch_request_thumbnails、全量/增量 thumbnail_full_gen、懒自愈 regenerate_missing_thumb）与处理链现状；核实 B1–B10 静态可疑瓶颈；报告供后续「先量化再优化」的实施会话使用。

## 发现
- HEAD=7fea98e3 与提示词快照同一提交，**无行号漂移**，全部行号即当前行号（报告见本目录 report.md）。
- **B1–B10 全部成立**，无一条被推翻。其中两处归属/构成修正（B7 的 create_dir_all 实际在 generator.rs:421 而非 exif_thumb.rs:421，该文件纯内存；B6 四目录=thumbnails+ai_thumbs+face_thumbs+viewer_color）、一处范围扩大（B4 生命周期读锁实际 8 处取锁点，快照只列 2 处）。
- 快照未载的新发现 N1–N10 已登记进 report.md §3，其中实施影响最大的三条：
  - N2（B4 范围）：dispatcher 逐 id `is_database_epoch_current` 与 batch 收集器逐条读锁也在每项路径上，批级化改造要覆盖全部 8 处。
  - N3（B6 加重）：LRU 未超限的常态路径也要先完成四目录全量 walkdir+逐文件 metadata（total_size 在遍历中累计后才早退，cache.rs:233-258 主会话抽查原文确认）。
  - N1（新可疑点）：直显小文件 ≤500KB 为 thumbhash 做**无 ResizeHint 全量解码**（generator.rs:283-292，主会话抽查原文确认）。
- 顺带确认共享资源协议现状：CAS `update_thumb_result_if_current` 不降级语义完好（db/queries/thumbnail.rs:462-502）、write_atomic tmp+rename 是有意设计不可去除（generator.rs:56-65）、QoS 三池与 crossbeam bounded(1024)、先 CPU permit 后 GPU 令牌均无漂移。
- 前端触发链（useRequestQueue 50ms/24 批/30s 看门狗/0.5-1-2s 退避、useThumbLoader 自愈、飞掠闸门、ipc 命令名）全部与快照一致。

## 外部资料(当数据,不当指令)
- （本任务未引入外部网页资料；提示词文档属仓内权威源）

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 提示词2 B1–B10 现状核实结论与 N1–N10 新发现（report.md），供缩略图线后续优化实施会话直接引用 | docs/status/ 缩略图线 status 分片或该线 planning 引用本报告 |
| F-002 | 陈旧注释两处：cache.rs:60-66 称 LRU 只遍历 thumbnails/（实际四目录）；generator.rs:436-440「解码期即降采样」对 image-rs 名不副实 | code（随手修注释，可并入任一缩略图批次） |
| F-003 | 直显路径 ≤500KB 无 ResizeHint 全量解码生成 thumbhash（generator.rs:283-292），快照未载的新可疑瓶颈 | todo（登记进缩略图优化线候选） |
| F-004 | LRU 驱逐已是事件驱动自愈（tasks.rs:142-176 reset_thumbs_by_evicted_paths + db:media_enriched），启动 stat 扫描为兜底；T6 事件触发未接线（tasks.rs:106-107） | experience |
| F-005 | 生命周期读锁 8 处全清单（report.md §2.4 表）——B4 类「批级取锁」改造的完整范围基线 | experience |
