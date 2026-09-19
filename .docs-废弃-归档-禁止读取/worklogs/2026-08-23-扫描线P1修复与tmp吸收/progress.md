---
status: 快照
type: 工作记忆
line: 扫描线P1修复与tmp吸收
created: 2026-08-23
---

# 进度日志:扫描线P1修复与tmp吸收

## 会话:2026-08-23
- 做了:双线对比评审完成(两个审计 agent + 头对头比对),用户采纳处置建议;建三件套开工。
- 做了:阶段 1 完成——mark_missing.rs 按根分表(`_mm_seen_r{root}`)+ `SeenWriter` 预构建语句 + preloaded 缺表保守跳过 + 真实差集后 DROP;fast_scan.rs 四触点同步(导入/QuickDirPruner 加 seen 字段/init 带 root/批内插入走 SeenWriter)。
- 验证:`cargo test --lib mark_missing` 10 passed;`scanner::fast_scan` 19 passed;全量 `cargo test --lib` **1091 passed / 0 failed / 6 ignored**;clippy 无本线新增警告(仅基线 chunks_exact 与 build-script 信任根提示)。
- 做了:阶段 2 完成——enricher 不可 keyset 序(folder / *+filename)改 TEMP 队列表:首批判次一次性按视图序灌入(INSERT..SELECT 经 prepare 绑定 root_id),按 seq 游标续取,取批重查 is_deleted;表名带调用代次(`_enrich_q_r{root}_g{gen}`)+ QueueGuard RAII 收尾 DROP,根除对照线的固定表名 DROP 竞态与内存滞留;fallback 兜底轮语义保留(并发插入+失败重试)。
- 错误账(新增):①execute_batch 不绑定参数,建队 SQL 的 ?1 被当 NULL 静默灌空表——拆 CREATE(batch)+INSERT(prepare 绑定),注释与测试双锁;②手写期望序想当然写反树键序,以对拍结果为准修正。
- 验证:`scanner::` 62 passed;全量 lib **1094 passed / 0 failed / 6 ignored**;clippy 11 条警告全在基线未触碰文件,scanner 零新增。
- 做了:阶段 3 完成——walker 文件分支单次 `symlink_metadata` 取代「file_type() 预检 + metadata()」双调用(目录分派仍走 walkdir d_type 便宜路径;is_symlink 先判保持跳过语义;stat 失败仍只对已分类媒体计 errors);`contains_bytes` 改首字节定位 + 整串比对。
- 验证:`scanner::` 62 passed;全量 lib **1094 passed / 0 failed / 6 ignored**;clippy scanner 零警告。
- 做了:阶段 4 完成——vue-tsc 通过(exit 0);CI 基线核查:dev 自 2026-08-20 起既有红(runner build.rs 环境 exit 101、Linux job 排队 24h 超时、docs-governance 对 2026-08-15 既有文档报红),与本线无关,登记 F-006;todo.md 2026-08-23 增补(含 1077→1094 计数更正与 CI 余项)、experience §51、Spec02 §3.1.1 walker/seen/选批三处同步;三件套收口迁 worklogs。
- 验证:`npx vue-tsc --noEmit` exit 0;`worklog-kit check` 过闸(见收口 commit)。
- 遗留:无(本线收口)。跨线余项见 todo.md 2026-08-23 增补:真机大库 A/B、quick 账本持久化、live photo 流式立项、CI 基线修复立项。

## 回顾(收口时填)
- 亮点:双线对照评审先行(同一审查建议的两套实现互为镜像审计),把「谁好」拆成可落地差异清单再吸收,移植时扬长避短(保留本线正确的 keysetable 谓词、补上对照线的代次隔离与保守跳过、修掉其常驻内存与 DROP 竞态);对拍测试驱动 SQL 函数层,沿用本线既有测试风格零重构。
- 教训:`execute_batch` 对含参数 SQL 静默当 NULL(不报错、灌空表)——参数化 SQL 必须 prepare 绑定,已在代码注释与测试双锁。
- 意外:CI dev 早已三重既有红(runner 环境/排队超时/文档门禁),推翻了「本线提交会挂 clippy 门」的预设——远程根本没跑到 clippy;本地门禁证据仍是当前唯一有效验证面。
