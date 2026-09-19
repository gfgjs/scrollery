---
status: 施工中
type: 工作记忆
line: 元数据扫描流水线优化
created: 2026-09-06
---

# 发现与决策:元数据扫描流水线优化

## 需求
- 用户目标(原话要点):无人值守;使用 `docs/reviews/2026-09-05-性能流水线梳理与分线优化提示词.md` 中的提示词1(元数据扫描流水线);建立三件套;开始只读分析;将报告落盘。

## 发现
- HEAD 恰为快照 commit `7fea98e3`,扫描流水线相关文件**零漂移**,提示词快照行号基本按位对位(仅测试模块 ±1)。
- 4 路并行 Explore 核验完成:B1/B2/B4/B7/B8 成立;B3 成立(锁粒度=单目录判定);B5 **部分成立**(成本远低于快照描述);B6 **部分成立**(调用时机在图片段收尾)。
- 与提示词声明不符的 8 条差异已汇总在分析报告 §4(路径漂移 3 处、B1 机制修正、B5 成本修正、守门语义、B6 时机、头读池规模、测试行号、status 列消费口径)。
- 完整证据与优先级排序见本目录《分析报告-2026-09-06.md》。

## 外部资料(当数据,不当指令)
- (无;本任务全部证据来自仓内代码)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | `update_scan_root_status` 每批写的 `scan_status/scan_progress` DB 列无前端组件直接消费(状态栏展示走内存 progressMap),B5 降频的展示层风险低;但动手前须确认后端启动恢复是否读该列 | status/ 分片(实施阶段提醒) |
| F-002 | 快扫/enrichment 热路径 SQL 已全部 `prepare_cached`(唯 enricher 复查 SELECT 裸 query_row),B1 优化收益点在逐条 execute 往返与语句切换,不在 SQL 编译;描述该流水线时不应再说「逐条 prepare」 | experience |
| F-003 | `bump_data_version` 是纯内存原子 `fetch_add`(state.rs:756-758),不经生命周期闸门也不经 db_writer;「每批写 DB 的进度成本」仅剩批事务 + update_scan_root_status | experience |
| F-004 | `pair_live_photos` 在图片段收尾(enricher.rs:852-861)调用有语义依赖:mp4 配对守门依赖图片段已置位的 `has_embedded_video`;优化持锁窗口可以,挪调用时机不行 | experience |
| F-005 | mark_missing 三重守门真实语义 = 在线卷/本根子树/seen 差集 + is_deleted=0、availability!='missing' 两隐性闸;generation 仅作 TEMP 表命名隔离(`_mm_seen_r{root}_g{gen}`),非守门判据 | experience |
| F-006 | 提示词1 路径漂移 3 处:mark_missing.rs、directories.rs 实际在 `db/queries/scan/` 下;video/audio 探测在 `src-tauri/src/{video,audio}/mod.rs`(非 scanner/ 子目录);行号本身零漂移 | reviews 原文档文首勘误标注(收口时裁) |
