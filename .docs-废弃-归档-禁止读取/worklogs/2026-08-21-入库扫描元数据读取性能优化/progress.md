---
status: 快照
type: 工作记忆
line: 入库扫描元数据读取性能优化
created: 2026-08-21
---

# 进度日志:入库扫描元数据读取性能优化

## 会话:2026-08-21
- 做了:完成静态分析与简化 SQLite 建模,产出瓶颈清单;建立三件套。
- 验证:Python sqlite3 简模 100k 行选批对比 9.5s → 0.19s(仅说明数量级,非真机基准)。
- 遗留:阶段 1 开工(enrichment keyset + V24 索引)。

## 回顾(收口时填)
- 亮点:每个阶段独立可验、独立 commit,keyset 简模量化(9.5s→0.19s)让「重复排序」从猜测变成证据。
- 教训:rusqlite 无语句缓存;增量剪枝晚于 per-file stat 等于没省最大头;Xxh3 streaming 分段 update 与 one-shot 在本 crate 版本不一致,性能优化不能改 cache_key 字节语义。
- 意外:V24 删 `idx_media_type` 后既有 canonical 计划测试按旧索引名断言,必须同步更新计划锁定测试——隐式唯一索引名进入计划是合理的新事实。

### 阶段 1 完成(2026-08-21)
- 做了:V24 迁移新增 `idx_media_type_sort`/`idx_media_type_id`,删除被复合索引覆盖的 `idx_media_type`;图片富化默认 date+datetime 改 keyset,视频/音频队列改 id keyset;新增 5 个单测(keyset 分页、计划无 TEMP B-TREE、队列跳过已处理行、V24 迁移)。
- 验证:`cargo +1.97.0 test -p scrollery --lib` 全量 1070 passed/5 ignored/0 failed;`cargo +1.97.0 clippy -p scrollery --lib --tests` 仅 2 条既有 `items_after_test_module` 基线警告(非本阶段引入);`cargo +1.97.0 fmt --all -- --check` 通过。
- 遗留:阶段 2(DB 写热路径 + 冗余目录索引删除 + 快扫分配瘦身)。

### 阶段 2 完成(2026-08-21)
- 做了:V25 迁移删除冗余 `idx_media_directory`(唯一约束隐式索引覆盖);扫描/富化热路径 DAO 改用 `prepare_cached`;`compute_cache_key` 改 512B 栈缓冲优先拼接,长路径回退 Vec 且旧值逐字节等价;更新目录视图计划锁定测试到隐式索引。
- 验证:`cargo +1.97.0 test -p scrollery --lib` 全量 1071 passed/5 ignored/0 failed;`cargo +1.97.0 clippy -p scrollery --lib --tests` 仅 2 条既有基线警告;fmt/diff-check 通过。
- 错误账:`Xxh3::update` 分段 update 与 one-shot `xxh3_64` 在本 crate 版本下不一致(小输入实测),放弃 streaming 方案改栈缓冲拼接——cache_key 值不可漂移是硬约束。
- 遗留:阶段 3(quick 前端接线 + 目录级剪枝前置到 per-file stat 之前)。

### 阶段 3 完成(2026-08-21)
- 做了:`MediaWalker` 新增 `DirPruner` 目录级剪枝钩子,quick 判定从「拿到 WalkedFile 之后」前置到 walker 进入目录时;未变目录直接文件不再构造 WalkedFile/调 metadata(walkdir 的 `file_type` 零 syscall,故 stat 全免);前端 `startScan` 首扫全量、快扫完成后同根重扫默认 quick。
- 验证:Rust 全量 lib 1073 passed/5 ignored/0 failed;walker 剪枝测试 6 passed、quick_scan 8 passed;scanStore vitest 6 passed;vue-tsc --noEmit 通过;eslint/prettier 通过;clippy 仅 2 条既有基线警告。
- 遗留:阶段 4(mark_missing seen 流式化)。

### 阶段 4 完成(2026-08-21)
- 做了:seen 集从内存 HashSet 改为连接级 `_mm_seen` TEMP 表流式累积;`mark_missing` 保留旧签名兼容测试,新增 `init_seen_table`/`insert_seen_id`/`mark_missing_preloaded` 生产路径;`decide_dir_pruned_at` 的回填经泛型回调接入 TEMP 表。
- 验证:Rust 全量 lib 1074 passed/5 ignored/0 failed;mark_missing 8 passed(新增 preloaded 不清 seen/同连接复位语义);quick/finalize 回归通过;fmt/clippy 通过(仅 2 条既有基线警告)。
- 遗留:阶段 5(元数据读取常数项:fast_scan eager HeaderBuf、XMP 字节搜索、阶梯头读)。

### 阶段 5 完成(2026-08-21)
- 做了:fast_scan eager 尺寸读取非 TIFF 走 HeaderBuf(尺寸 + orientation 一次 open);enrichment 头读按扩展名阶梯(JPEG 128KB/TIFF·HEIC 系 256KB/其它 64KB);XMP Motion Photo 标记改字节滑窗,去掉每 JPEG 128KB lossy String 分配。
- 验证:Rust 全量 lib 1077 passed/5 ignored/0 failed;metadata 测试 11 passed;fmt/clippy 通过(仅 2 条既有基线警告)。
- 遗留:阶段 6(Spec02 同步、docs/todo、三件套收口)。

### 阶段 6 完成(2026-08-21)
- 做了:Spec01/02/04/15 同步 V24/V25、keyset、quick walker 前置与 seen TEMP 流式现状;docs/todo 登记 2026-08-21 增补;新建 lines/入库扫描元数据读取性能优化.md;experience 增补 §49/§50;closeout 处置 7 候选并迁 worklogs、登记 worklogs README。
- 验证:worklog-kit index 通过;worklog-kit check 仍为基线红 106 处存量强制违反(本线 closeout 已从红榜消失,零新增);Spec01/02/04/15 与 todo 均为目标文件修改。
- 遗留:真机大库导入/增量重扫 A/B(仅 SQLite 简模证据);quick 账本重启后首扫仍全量。
