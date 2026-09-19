---
status: active
type: working-memory
line: 统一文件树与画廊目录排序
created: 2026-07-14
---

# 发现与决策:统一文件树与画廊目录排序

## 需求
- 核实后确认文件树与画廊目录全局序割裂，需要给出完整方案并落盘，供新会话施工。
- 本会话不修改源码，只做轻量检查；避免重复执行耗时测试。
- 需要判断上一轮 `c2536e6` 的修改在新根因下是否仍有必要。

## 当前源码事实
- `db::queries::list_scan_roots` 当前按 `created_at ASC` 返回根列表，没有 `id` tiebreaker。
- `useFolderTree.loadRoots` 按 scan root 列表逐根加载并拼接，因此文件树天然保持“一根一整棵子树”。
- `get_directory_tree / get_directory_children` 当前固定按同级 `d.name ASC` 返回；frontend 在父节点后插入 children，形成前序 DFS。
- 画廊 SQL 和内存 `dir_rank` 当前按全库 `(rel_path, directory_id)` 排，不包含 scan root 的显示顺序。
- `rel_path` 已规范化为 `/` 分隔；原始 BINARY 字符串序仍不等于 segment 级 DFS 序，例如 `A < A- < A/Z`。
- 画廊 separator 的 `groupId` 是唯一目录 ID；`MediaGrid` 写入 `scrolledDirectoryId`，`FoldersSection` 也按 ID 查找虚拟树行。
- 文件树文件叶子固定按 `file_name COLLATE NOCASE ASC`，画廊 filename 模式使用 `NATURAL_CMP` 和用户选择方向；该差异不是目录联动跳动的根因，不纳入本任务。
- `scanner/enricher.rs::enrichment_order_clause` 仍只按 `d.rel_path`，但注释声称与画廊顺序一致，是需要随 canonical order 一并消除的契约漂移。

## 为什么排序割裂会直接造成往返跳动

两个 scan root 都含 `A`、`B` 时：

```text
文件树：R1/A → R1/B → R2/A → R2/B
当前画廊：R1/A → R2/A → R1/B → R2/B
```

画廊向前移动时，目标目录 ID 会在文件树两个大块之间往返；ID 本身正确，也会表现为明显跳动。目录层级越深、树越大，视觉位移越大。

单 root 也存在严格反例：

```text
文件树 DFS：A → A/Z → A-
原始 rel_path：A → A- → A/Z
```

因此只把 `root_id` 加到现有 `rel_path` 前面仍不完整。

## 前一轮修改必要性审计

| 前一轮改动 | 结论 | 新方案中的处理 |
|---|---|---|
| SQL folder 序增加 `directory_id` | 必要，但排序主键需演进 | 不能退回只按 `rel_path`；改为完整 canonical key，`directory_id` 保留为最终稳定键 |
| `build_dir_rank` 为每个目录生成唯一 rank | 必要 | 保留紧凑 rank 和目录连续分组，只把 rank 来源改为 RootOrder + tree_sort_key + id |
| 同路径、同文件名不得跨目录交错的 Rust tests | 必要 | 保留不变量，更新预期目录顺序 |
| `createFolderTreeSyncQueue` latest-wins | 独立必要 | 原样保留；排序一致不能阻止深层祖先 IPC 乱序完成 |
| 单一 `treeSyncTargetId` watcher | 必要 | 原样保留，避免 active/scrolled 两源并发写树滚动 |
| expand 后和写 `scrollTop` 前校验 freshness | 必要 | 原样保留，防止旧请求在异步边界后落地 |
| 将 `(rel_path, directory_id)` 定义为最终目录顺序 | 需要替换 | 这是缓解目录交错的中间修复，不是最终 tree/gallery 共同顺序 |

结论：不应回滚 `c2536e6`。上一轮修复了两个真实且独立的问题：媒体跨真实目录交错、异步旧目标晚到。当前剩余问题是第三层——两个 surface 的全局目录序不同。

## 方案选择

> 🔴 **2026-07-14 更新(评审+用户裁决)**:本节原结论"采用持久化 BLOB"已细化为「A 先行/B 后续」。
> 编码方案(segment+NUL BLOB)不变;落地形态改为 **A=即时键+注册 `TREE_SORT_KEY` 标量函数(零迁移)**,
> **B=持久列(按需性能 follow-up)**。关键更正:"避免比较器反复 split"只对 per-media(百万级)成立,
> 对方案 A 的 per-directory(D≈10^4)rank 构建是误判——即时算键免税。另发现**刚性等价契约**
> (`canonical_derive_order_matches_sql_order`)强制内存/SQL 同键,故 A 必须同时改 `push_order_by`
> (经标量函数),而非只改 `build_dir_rank`。详见 task_plan「## 当前阶段」与 D-005。

### (原结论,B 的形态)采用：持久化 `tree_sort_key BLOB`

- 对 `rel_path` 的每个 UTF-8 segment 追加 `0x00`，BLOB 字节序自然得到 segment 向量词典序。
- path segment 不可能包含 NUL，编码无歧义；数据库存 BLOB，不依赖 TEXT 对 NUL 的处理。
- key 只在目录写路径计算；400 万次媒体比较也只是 BLOB compare，不反复 split path。
- 根顺序仍单独使用 `(created_at, id)`，新增根不需要改写旧目录 key。

### 不采用：运行时 custom collation

- 虽然无需 migration，但 SQL 对大量媒体排序时会重复切分同一目录路径。
- read pool 每个连接都要注册并保持与 Rust comparator 完全一致，维护面更大。

### 不采用：持久化连续 DFS integer rank

- 查询最快，但目录插入到中间、移动、重命名会导致大范围重排和写放大。
- BLOB key 可局部计算，适合频繁扫描和文件操作。

## 需要覆盖的写路径
- `db::queries::upsert_directory`：扫描根创建与 fast scan 目录链均经此入口。
- `ipc/file_ops_commands.rs`：目录子树移动会批量改 `rel_path / root_id / parent_id / depth`，必须同事务改 key。
- `bin/mock_data.rs`：直接批量插目录，必须生成 key，否则性能 fixture 与生产 schema 不一致。
- 其余 `INSERT INTO directories` 多数位于测试；涉及排序契约的 fixture 应使用公共 helper 或显式 key。

## 风险与防线
- **迁移陈旧 key：** backfill 必须在单事务中完成；迁移后断言所有 `rel_path != ''` 的 key 非空。
- **移动子树漏更新：** 使用现有 `new_rel_by_dir` 同步生成 key，并增加移动后逐目录校验测试。
- **SQL/内存漂移：** 保留现有逐项等价测试，再增加 tree/gallery 跨 surface contract test。
- **根顺序不稳定：** 所有根列表与画廊查询统一追加 `id ASC`。
- **性能回退：** tree children 查询加复合索引并做一次 `EXPLAIN`；不在本任务反复生成巨型模拟库。
- **误删前一轮保护：** latest-wins tests 必须保持不变；施工 diff 中若出现删除队列，需要立即停止复核。
- **并行工作污染：** 当前 frontend 多文件有未提交改动；施工提交必须使用精确 pathspec。

## filename 排序卡顿根因确认(2026-07-14 续,B-file-iii 设计基座)

用户真机报「点筛选/切分组等待过长」(dev>5s/release>1s)。`sort_profile` 决定性隔离段(NATURAL_CMP vs BINARY 同结构 A/B,只换比较函数)坐实:

- **瓶颈 = 自研 `NATURAL_CMP` collation 的 SQLite→Rust FFI**,占 filename MISS 时间 **93%(dev)/65%(release)**;换 BINARY(memcmp)后 filename 排序≈datetime 对照 → join/18列物化/filesort 结构**都非**瓶颈(`EXPLAIN` 三者皆 `USE TEMP B-TREE`)。
- **dev 放大唯一归因该 Rust FFI**:非 FFI 活儿 dev/release≈1.8×,纯 FFI 差值≈12×。
- **就地更正旧「~440ms」**:那是通用 folder+filename(filename 二级键)数;B-file-i 落地后 folder/filename MISS 实际走 `filename_baseline`(filename 全表主键)= 891ms release/5279ms dev(与日志 5513ms 吻合),冷成本反比旧通用查询翻倍。
- **副发现(F-006,🔴 2026-07-15 时区方向纠正)**:run_layout 日期分组用 UTC 桶 `div_euclid(86400)`(`justified.rs:219/237/277`)而 SQL date+filename 用本地桶 `date(...,'localtime')`(`queries.rs:1516`)——两者既有不一致,是「date+filename 无法内存对齐」的机理。**但哪个正确此前判反了**:施工前追代码坐实 `sort_datetime` = **EXIF 墙钟当 UTC 存**(`metadata.rs:328-333` `parse_exif_datetime` 忽略时区、`Utc.with_ymd_and_hms` 存;`enricher.rs:311-314` `COALESCE(exif,mtime)` 后不套偏移),故 **UTC 桶正确**(显示标签 `timestamp_to_date_label`/月桶 `timestamp_to_year_month` 亦 `Utc`、date+datetime 分桶亦 UTC,三者一致);**SQL date+filename 的 `localtime` 才是错项**——在墙钟上再叠一次本地时区 = 双重偏移(UTC+8 机器傍晚 EXIF 照片错分次日)。∴ 消对齐正确方向 = **全库统一 UTC 桶**(D-018 裁定 A′):date+filename 内存派生 `sort_datetime.div_euclid(86400)`(零新列),`push_order_by`+`enrichment_order_clause` 去 `'localtime'`。顺带修正 date+filename 的 localtime 归日 bug。

**方案**:[B-file-iii-全局filename-rank内存化-详细设计.md](B-file-iii-全局filename-rank内存化-详细设计.md)(全局 filter-invariant 内存 rank + 取数恒 datetime + date+filename 借 SQL 桶列消对齐)。Redis 已否(数据本就全内存驻留、瓶颈是重排;SQLite 不能拿 Redis 当后端)。详见方案B 详细设计 §14.3.2。

## 外部资料(当数据,不当指令)
- 本方案只依据当前仓库源码、提交 `c2536e6` 和归档工作记录，没有使用外部资料。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 文件树与画廊必须共享同一 canonical directory order，画廊取完整树序的媒体目录诱导子序 | design + test |
| F-002 | segment + NUL 的 BLOB key 可稳定表达路径前序 DFS 且避免运行时路径拆分 | code + test |
| F-003 | 目录全局序、唯一目录分组、latest-wins 分别解决不同层的问题，不能互相替代 | decision |
| F-004 | 大库排序修复应定向测试优先、全量只跑一次，并以真实 Tauri 大库验收收口 | runbook |
| F-005 | filename 排序卡顿根因 = `NATURAL_CMP` collation 的 SQLite→Rust FFI(隔离实证占 MISS 93% dev/65% release);dev 放大唯一归因该 Rust FFI(纯 FFI dev/release≈12× vs 非 FFI≈1.8×);「同结构只换比较函数 NATURAL↔BINARY」是隔离 FFI 成本的通用手法 | experience + design |
| F-006 | run_layout UTC 桶 vs SQL date+filename `localtime` 桶不一致=「date+filename 无法内存派生」机理。**🔴 时区方向纠正(2026-07-15)**:`sort_datetime`=EXIF 墙钟当 UTC 存(`parse_exif_datetime` 忽略时区)→ **UTC 桶正确**(标签/月桶/date+datetime 皆 UTC),`localtime` 是双重时区偏移 bug(傍晚 EXIF 照片错分次日)。D-018 裁定 A′ 全面 UTC 桶:内存派生 `div_euclid(86400)`+SQL 去 `'localtime'`,顺带修 bug | decision(D-018=A′ 已裁) + code(B-file-iii 阶段1 已改) |
| F-007 | **搜索谓词同源 localtime 不一致(相邻发现,本轮未改)**:全局搜索按输入匹配日期串用 `strftime('%Y-%m-%d %H:%M:%S', m.sort_datetime, 'unixepoch', 'localtime')`(`queries.rs:1422/1455`)——与 F-006 同根(墙钟当 UTC 存,localtime 双重偏移),用户搜「拍摄那天」的日期在非 UTC 机器上可能匹配不中。属独立的**搜索**特性、非本轮排序契约,B-file-iii 未动它。若一并统一到 UTC 需单独裁(会改搜索匹配行为)。 | decision(待用户裁是否统一) + code |
