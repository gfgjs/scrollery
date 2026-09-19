---
id: 2026-07-14-方案B-持久化排序键列-详细设计
status: active
type: design
line: 统一文件树与画廊目录排序
created: 2026-07-14
---

# 方案 B(排序键预计算)详细设计 v1.1:目录序持久列 + filename 序缓存 / 持久键

> 🟢 **施工状态(2026-07-14 更新)**:
> - **Part I(B-dir 目录序持久列 V19)已施工落地 + 本地验证**——三提交 `7d97b01`/`d649414`/`f4a85c7`,
>   全量 lib 515 test + fmt + clippy + EXPLAIN 全绿(本地非 CI)。**§2.4 真机 profile 已完成**(提交
>   `62955fb`):切列稳定省 ~60-65ms(~15%)常数因子、非阶跃,B-dir 保留;副产物推翻「5s 级」前提
>   (现 SQL 排序已亚秒),详见 §2.4 裁决。施工偏离设计的 3 处修正见文末「修订记录 v1.2」。
> - **Part II 的 B-file-i(运行时 filename 基准序内存派生)已施工落地 + 本地验证 + 真机实测**——用户
>   亲证 filename 是核心用例(漫画/连续剧按名排序),决策门 D-014 通过;提交 `779df5a`(后端)/`d73be88`
>   (bench),全量 lib 516 test + 新增 filename 对拍 + fmt + clippy 全绿(本地非 CI)。**真机 543k 实测**:
>   filename 轴/方向切换从每次 ~700ms 全表 SQL NATURAL_CMP filesort → **~26ms 内存派生(memo miss)**
>   / 0.66ms(memo hit),~27×,与 datetime 平权。施工期 2 处 refine(SQL 赋 rank、date+filename 留 SQL)
>   见 §14.1 施工 banner + 修订记录 v1.3。
> - **双键统一缓存(§14.3 / D-015)已施工落地 + 本地验证**——收尾 B-file-i,闭合 **datetime↔filename 跨轴
>   切换**的 MISS(用户 Request 4「文件夹下切排序仍 2-3s」根因 = 单槽缓存换轴挤掉另一基准全表重查)。同一缓存
>   同时服务两轴:每项自带 sort_datetime(派 datetime 免费)、datetime 基准惰性补 `filename_rank`(一次 id-only
>   查询)→ 跨轴切换从换基准全表重查(~319ms→datetime / ~893ms→filename)降为内存派生 HIT。**真机 543k 实测
>   (`sort_profile`,release,5-run median)**:filename→datetime **17.6 / 28.1ms**(none/folder 轴)、
>   datetime→filename **15.7 / 25.0ms**(rank 就绪),首次 datetime→filename 付一次性 `filename_rank` 构建 **530ms**
>   (id-only 查询,非记忆里估算的 ~378ms)。提交 `4ea7b68`,lib **518 test**(+双向跨键对拍)+ fmt +
>   clippy `-D warnings` 绿(本地非 CI);真机 GUI 手感验收 pending。
>
> 🔴 **红线**:仅剩 **B-file-ii(持久 filename 键列)** 是设计、**勿自动开工**(须一次显式 "go")。
> 触发条件 = B-file-i 已上后 profile 仍证明「那次基准 NATURAL_CMP 排序」(而非重复重排)是瓶颈、
> 且纯 filename 无分组视图确为热点(见 §14.2 / D-013);其自然序字节键编码须作前置子任务单独设计验证。
> 方案 A 已 landed(`24ab039`/`e935b41`)。不得回滚 `c2536e6`。

---

## 0. 方案 B 家族总览与排期（v1.1 新增）

> **v1.1 范围**：v1.0 全文原样降为 **Part I（B-dir 目录序持久列）**；本节与 **Part II（B-file 媒体项 filename
> 序）** 为 v1.1 新增，并入 2026-07-14 对 filename 排序路径的代码核实结论（详见文末修订记录）。

方案 B 的本质一句话：**用预计算的排序键（或内存基准序）替代「每次查询 / 每次交互都重算的昂贵排序」**。核实
代码后，它不是单个改动，而是一个**家族** —— 两个正交成员、三档解法：

| 成员 | 治理对象 | 形态 | 迁移 | 定位 |
|------|---------|------|------|------|
| **B-dir** | 目录序（folder 分组的目录顺序） | `directories.tree_sort_key BLOB` 持久列 | V19 | Part I（原 v1.0） |
| **B-file-i** | 媒体项 filename 序 | **运行时内存 filename 基准序 + 整数 rank**（仿 datetime canonical） | **无** | Part II · **推荐先行** |
| **B-file-ii** | 媒体项 filename 序 | `media_items.file_name_sort_key BLOB` 持久列（+可选索引） | V20 | Part II · 后手、重 |

**三条统一的诚实边界**（各成员章节展开）：
1. **都不消除「分组叠加时的跨表 filesort」** —— folder/date 分组的 `ORDER BY` 跨 `scan_roots` / `directories` /
   `media_items` 三表，单表键 / 索引不被规划器用上（§2.2 对 B-dir、§14.2 对 B-file-ii）。预计算键只让**取键**或
   **轴 / 方向切换**更便宜，不改排序的算法复杂度。
2. **默认 datetime 路径已免费** —— 走 S1 canonical 内存缓存（§2.3），与本家族无关。家族只治理 datetime 基准
   给不出的 filename / similarity 组内次序。
3. **都受同一决策门约束 —— 先测再做**：B-dir 先 profile `TREE_SORT_KEY` FFI 占比（§2.4）；B-file 先测 **filename
   实际使用频率** + profile 重排时延（§15）。

**推荐排期**（三者正交，可独立 go，但都过决策门）：B-dir 与 B-file 各按自己的 profile 结果决定；B-file **内部**先
**B-file-i**（运行时、零迁移、直接闭合「canonical 只服务 datetime」的复用缺口）→ 仅当其一次性基准排序仍是瓶颈、
且「纯 filename 无分组」视图是热点，再上 **B-file-ii**（持久列，须先解决自然序字节键编码）。

---

# Part I：B-dir —— 目录序持久化 `directories.tree_sort_key BLOB` 列

> 本部分为 v1.0 原文，内容不变；§1–§12 与决策 D-006..D-010 均属 B-dir。

## 1. 目标与非目标

### 目标
- 把画廊 `push_order_by` 与 `enricher::enrichment_order_clause` 的 folder 目录序键,从「每条媒体行调
  `TREE_SORT_KEY(d.rel_path)` 标量函数」换成「读预存 `d.tree_sort_key` 列」,消除大库 folder-SQL 排序
  的逐行 SQLite→Rust FFI + `Vec<u8>` 分配开销。
- 顺带把 `enricher` 的 folder 序从方案 A 遗留的**近似**(仍按 `rel_path`)升为**精确前序 DFS**(与画廊
  完全一致),并消除其每批全表重排时的逐行函数开销(见 [[lexicmp-natural-cmp-overflow-2026-07-14]] 记录的
  enricher 全表重排痛点)。

### 非目标(明确不做)
- **不改变任何排序结果**——纯性能优化,folder 各模式排序输出与方案 A 逐项一致。
- **不消除 filesort**——见 §2 诚实边界;B 是常数因子优化,不是算法级。
- **不动内存 canonical 路径**(`items_cache::build_dir_rank` / `derive_order`):它在 D≈10⁴ 目录级即时算键,
  本就免税(方案 A 评审已更正「避免比较器 split」对 per-directory 是误判)。
- **不动前端**(latest-wins 队列、separator `groupId=directory_id`、文件树懒加载全部不变)。
- **不动文件树同级查询**(`get_directory_tree/children` 按 `d.name ASC`,单段序等价于 `tree_sort_key`)。

---

## 2. 性能诚实边界(**先读本节再决定要不要做**)

**关键结论:B 是常数因子优化,不是算法级优化。**

### 2.1 B 优化了什么
folder 分组的 SQL 排序里,排序键 `TREE_SORT_KEY(d.rel_path)` 现在按**每条媒体行**求值一次:`d.rel_path`
是列、随行变化,`SQLITE_DETERMINISTIC` 只允许在**参数跨行不变**时折叠,列参数不满足 → 逐行求值。一个 N 行
视图 = N 次 SQLite→Rust FFI 跨界 + N 次 `Vec<u8>` 分配。换成 `d.tree_sort_key` 后是 N 次 BLOB 列读取
(memcpy),省掉 FFI 与分配。

### 2.2 B **没有**优化什么
folder 的 `ORDER BY` 跨 `scan_roots` + `directories` + `media_items` **三表**:主键来自 `scan_roots`
(`r.created_at, r.id`)、次键 `directories`(`tree_sort_key, d.id`)、末键 `media_items`(datetime/filename/
similarity + `m.id`)。跨多表且主键在被驱动表上的排序,**无法由单表索引消解 → SQLite 无论如何都要 filesort**。
B 只让每次**取键**更便宜,**不减少比较次数、不消除排序**。所以 B 的收益上限 = 「N 次 FFI+alloc」相对「整个
filesort」的占比。该函数体极轻(仅 `split('/')` + `push`),这个占比**未必大**——这正是「先 profile 再做」的原因。

### 2.3 为什么默认场景(最初的拖滑块 bug)**不经过** B
默认画廊路径走 **S1 canonical 缓存**(`push_order_by` 注释已载明:缓存路径**不走本函数**,恒 `(sort_datetime, id)`
基准序,分组/方向由 `items_cache::derive_order` 内存派生)。**folder + datetime-within** 可完全从
datetime-sorted 基准 + `build_dir_rank` 内存派生 → 已经免费。只有 **folder + filename** 或 **folder + similarity**
需要 datetime 基准给不出的组内次序,才回退到 SQL `push_order_by` 路径。**故 B 只碰这两个较少用的模式** + enricher
批量重排。最初 datetime-folder 的往返 bug 由方案 A 的内存路径修复,与 B 无关。

### 2.4 决策门
上 B 前**必须** profile:确认 `TREE_SORT_KEY` FFI 占 filename/similarity-folder 排序时延的可观比例。若不显著,
**记录并搁置**——这是完全可接受的结论,符合「按需 follow-up」定位。

> ✅ **已实测(2026-07-14 · 真机 543,449 行库 · 暖缓存 release · bin `sort_profile`)** —— B-dir 已 landed,此门转为**验收**(两轮复现一致):
> - **folder+filename**(生产配置 #1):切列前 `TREE_SORT_KEY(rel_path)` 444ms → 切列后 `d.tree_sort_key` 378ms,**省 ~65ms(~15%)**
> - **dir-only**(去掉 filename 二级键,隔离纯切列 delta):切列前 300ms → 切列后 239ms,**省 ~62ms(~20%)**
> - 参考:完整生产查询 `query_layout_items`(18 列物化 + 排序)702ms —— SQL 排序占 ~54%,行物化占 ~46%(后者为任何排序键优化的下界)
> - **similarity-folder**:`ai_search_results` 实测 **0 行**(每次搜索的瞬态小结果集,JOIN 后无法代表全库)→ 诚实跳过,以 dir-only 作等价证据(切列 delta 完全落在共享的 `dir_order` 前缀,与 similarity 二级键正交,故按构造相等)
>
> **裁决**:切列是**常数因子优化**(每行省 `TREE_SORT_KEY` 的 SQLite→Rust FFI + `Vec<u8>` 分配),稳定 **~60-65ms**,**非阶跃**。B-dir 已 landed **保留**——~15% 排序提速 + 零常态成本(键随写路径一次性维护)+ enricher 从近似 rel_path 串序升为精确 DFS 字节序的**纯正确性**收益;**无理由回滚**。
>
> **重要副产物(推翻旧前提)**:folder+filename 的 SQL 目录序排序实测已**亚秒(~440ms)**,**推翻**本文件 §13/§14 多处引用的「5s 级 SQL 字符串排序」前提(见 §13 已加更正 banner)。这**弱化 B-file 的紧迫性**:B-file-i 仍可把 filename 轴/方向切换从 ~440ms SQL filesort 降为亚秒内存派生,但收益倍数远小于原估(约 4-5× 而非一个量级),B-file 维持「先测 filename 使用频率」门下**搁置**。
>
> 🔴 **更正(2026-07-14,§14.3.2)**:此处 ~440ms 是**通用 folder+filename 查询**(filename 作**二级键**)的数,**不代表** B-file-i 落地后 folder/filename MISS 的真实成本——后者走 `filename_baseline`(filename 全表**主键**)= **891ms release / 5279ms dev**(与用户真机日志 5513ms 吻合)。且隔离实证证明该成本 **93%(dev)/65%(release)是 `NATURAL_CMP` FFI**、dev 5s 的放大唯一归因于该 Rust FFI。详见 §14.3.2。
>
> **产物**:生产计时点位(`compute_layout` MISS 日志的 `sql` 分项,隔离布局排序 SQL 本体)+ 可复用 bench `sort_profile`(只读打开真机库跑切列前后 A/B),提交 `62955fb`。

---

## 3. Schema 变更(`SCHEMA_V19`)

```rust
/// v19(目录排序统一 方案 B):directories 持久化前序 DFS 排序键列 + 回填。
/// 键编码与方案 A 的 encode_tree_sort_key / TREE_SORT_KEY 标量函数**完全同一**
/// (segment + 0x00 终止字节),BLOB memcmp = Rust Vec<u8>::cmp,故与内存 build_dir_rank 逐位同构。
pub const SCHEMA_V19: &str = "
ALTER TABLE directories ADD COLUMN tree_sort_key BLOB NOT NULL DEFAULT X'';
UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path) WHERE rel_path <> '';
";
```

要点:
- `NOT NULL` 的 `ADD COLUMN` 在**有行表**上 SQLite 强制要求 `DEFAULT`;`X''`(空 blob)既满足该约束,又恰是根
  目录(`rel_path=''`)的**合法**键值(根目录键本就是空)。
- `ADD COLUMN` 带常量 `DEFAULT` 是 **O(1) 元数据操作**(不重写行);既有行逻辑上读 `X''`,随后 `UPDATE` 覆写。
- `UPDATE ... WHERE rel_path <> ''`:根目录保持 `X''`,其余按函数回填。D≈10⁴ 目录,逐行调函数 <100ms,一次性。

---

## 4. 迁移回填(核心难点 + 连接注册依赖)

### 4.1 为什么回填**不能**纯 SQL 表达键
NUL 终止 BLOB 无法用纯 SQL 拼出:`||` 会把操作数**强转 TEXT**(丢 blob 语义)、SQLite **无 blob 拼接运算符**、
`char(0)` 在 TEXT 语境遇 NUL **截断**。因此逐段拼「segment + 0x00」的 BLOB 只能靠 Rust 编码函数。

### 4.2 解法:调用**已注册**的 `TREE_SORT_KEY` 标量函数(A 已建)
方案 A 已把 `encode_tree_sort_key` 注册为 SQLite 标量函数 `TREE_SORT_KEY`(`db/mod.rs::register_custom_collations`)。
回填 SQL 直接调它即得正确 BLOB(§3 的 `UPDATE`),**无需扩展迁移 runner 支持 Rust 闭包步**。

### 4.3 **必须解决的连接注册依赖**(本方案最易踩的坑)
- **生产可用**:写连接在 `create_write_connection` 建立时即注册 `TREE_SORT_KEY`([connection.rs:57](../../../src-tauri/src/db/connection.rs#L57)),
  `run_migrations` 随后在**同一写连接**上跑([lib.rs:204→215](../../../src-tauri/src/lib.rs#L204));故生产迁移时函数已就绪。
- **测试会全线红**:十余处测试在**裸 `Connection::open_in_memory()`** 上直接 `run_migrations`(`migration.rs` 多个
  `#[test]`、`exotic/coordinator.rs:506` 等),这些连接**没注册** → V19 的 `UPDATE ... TREE_SORT_KEY(rel_path)`
  会 `no such function: TREE_SORT_KEY` 全线失败。(方案 A 施工时,`canonical_derive_order_matches_sql_order` 对拍
  测试正是因此报错,当时逐个补了注册——B 的迁移把这个问题**放大到所有裸连接迁移测试**。)
- **推荐解法**:在 `run_migrations` **顶部**加一行自注册:
  ```rust
  pub fn run_migrations(conn: &Connection) -> Result<()> {
      // V19 回填 SQL 依赖 TREE_SORT_KEY 标量函数。此处自注册使 run_migrations 自足——生产写连接虽已在
      // create_write_connection 注册,但众多测试在裸连接上直接调本函数。幂等:同名 create_scalar_function
      // 即替换,写连接二次注册无害;顺带让「调用方须先注册」的隐式契约消失。
      crate::db::register_custom_collations(conn).map_err(crate::error::AppError::from)?;
      let version = read_version(conn);
      // ……以下不变……
  }
  ```
  优点:①一行、幂等、无害;②**一处改、全部裸连接测试绿**,优于逐个测试补注册;③`run_migrations` 自足声明其迁移
  SQL 的函数依赖,消除脆弱的隐式顺序契约。
- **备选(更重,不推荐)**:把 `STEPS` 从 `&[(u32, &str, &str)]` 扩成可含 Rust 闭包步,V19 用闭包逐行
  `encode_tree_sort_key` 回填。改动波及 `migrate_step` 事务边界与 runner 结构,收益不抵成本;仅当**其它**迁移也
  需要任意 Rust 逻辑时才值得。

### 4.4 迁移后不变量(放在 V19 迁移**测试**里,不在迁移 SQL 里 assert)
```sql
-- 非根目录若仍是空键 = 回填漏项 = bug;根目录(rel_path='')保持 X'' 合法。
SELECT COUNT(*) FROM directories WHERE rel_path <> '' AND tree_sort_key = X'';  -- 必为 0
```
`migrate_step` 只 `execute_batch` 不便 assert-and-fail,故不变量断言放测试:一是全新库到 V19 后该 count=0,二是
构造「V18 老库有目录数据」回拨重放 V19,断言回填正确(仿 `v17_backfills_coverage_from_faces_and_zero_face_done`
的回拨重放范式;注意回拨前须撤销所跨越的后续非幂等 DDL)。

### 4.5 版本号与 runner 登记
- `CURRENT_VERSION` 18 → 19;`STEPS` 末尾追加 `(19, SCHEMA_V19, "v19 (directories.tree_sort_key 持久 DFS 键 + 回填) | v19(目录持久化前序 DFS 排序键列)")`;`schema.rs` 的 `SCHEMA_V19` 导出并在 `migration.rs` 的 `use` 补入。
- `migrates_fresh_db_to_current_version_with_exotic_tables` 里 `assert_eq!(CURRENT_VERSION, 18)` → `19`,并补 `directories.tree_sort_key` 列存在断言。

---

## 5. 稳态写路径维护(生产 **3 处**;推荐 Rust 计算并绑定 blob)

**一致原则**:稳态写路径在 **Rust 侧**算 `encode_tree_sort_key(rel_path)` 并绑定 `Vec<u8>`——**自足**(裸连接
测试也能用,不依赖连接注册函数)、与 `move_directory` 现有「先在 Rust 算 `new_rel`」的风格一致。**迁移回填是唯一
用 SQL 函数处**(纯 SQL 步无法跑 Rust),二者分工清晰。

> 关键事实:**生产目录创建全部funnel through `upsert_directory`**——两个真实入口 `scan_commands.rs:97`(根目录)
> 与 `fast_scan.rs:199`(扫描子目录)都调它。其余 `INSERT INTO directories`(`exotic/pipeline.rs`、`fast_scan.rs`
> / `live_photo.rs` 的 `mod *_tests`)全是 `#[cfg(test)]` fixture(归 §7 处理)。故生产维护点只有 (a)(b),加 dev
> 工具 (c)。

### (a) `upsert_directory`([queries.rs:223](../../../src-tauri/src/db/queries.rs#L223))——覆盖所有生产扫描插入
```rust
// 自足:Rust 算键,不依赖连接注册 TREE_SORT_KEY。
let tree_key = crate::utils::path::encode_tree_sort_key(rel_path);
conn.execute(
    "INSERT INTO directories (root_id, parent_id, rel_path, name, depth, mtime, tree_sort_key)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
     ON CONFLICT(root_id, rel_path) DO UPDATE SET
         name=excluded.name, mtime=excluded.mtime",
    params![root_id, parent_id, rel_path, name, depth, mtime, tree_key],
)?;
```
- **`ON CONFLICT DO UPDATE` 分支不需改**:冲突键是 `(root_id, rel_path)`,命中即 `rel_path` 与传入值相等 → 键
  稳定,不必重写。仅 INSERT 首次写键。

### (b) `move_directory`([file_ops_commands.rs:735-751](../../../src-tauri/src/ipc/file_ops_commands.rs#L735))——唯一运行时改 `rel_path` 的入口
子树重写循环里 `new_rel` 已在手,加算键并在两条 UPDATE 各写列:
```rust
for d in &dirs {
    let new_rel = remap_rel(&d.rel_path, &old_prefix, &new_prefix);
    let new_depth = path_depth(&new_rel);
    let new_key = crate::utils::path::encode_tree_sort_key(&new_rel); // 新增
    if d.id == source_dir_id {
        tx.execute(
            "UPDATE directories SET rel_path=?1, depth=?2, root_id=?3, parent_id=?4, tree_sort_key=?5 WHERE id=?6",
            rusqlite::params![new_rel, new_depth, new_root_id, target_dir_id, new_key, d.id],
        ).map_err(AppError::Db)?;
    } else {
        tx.execute(
            "UPDATE directories SET rel_path=?1, depth=?2, root_id=?3, tree_sort_key=?4 WHERE id=?5",
            rusqlite::params![new_rel, new_depth, new_root_id, new_key, d.id],
        ).map_err(AppError::Db)?;
    }
    new_rel_by_dir.insert(d.id, new_rel);
}
```
- `copy_directory` **不需**改:它物理复制后由前端**重扫**经 `upsert_directory` 引入新行,(a) 已覆盖。

### (c) `bin/mock_data.rs`(dev fixture,非生产,优先级低)
两处裸 INSERT:
- `ensure_root_directory`([:276](../../../src-tauri/src/bin/mock_data.rs#L276)):`rel_path=''` → 键为空,可显式绑 `&[]` 或依赖列 DEFAULT。
- `ensure_sub_directories`([:303](../../../src-tauri/src/bin/mock_data.rs#L303)):`rel_path=name`(单段)→ 列表加 `tree_sort_key`、绑 `encode_tree_sort_key(&name)`(需 `use scrollery::utils::path::encode_tree_sort_key`)。
- 理由:否则 perf 库上所有 folder 键全空,恰好**测不出**本特性(排序退化按 `d.id`)。但它是 dev 工具,不阻塞生产。

### DEFAULT `X''` 的隐患
`DEFAULT X''` 把「忘写键」从**编译错误**降级为**静默错序**(空键 = 根目录合法值,不可区分)。防线:①稳态写路径
集中且少(生产仅 2 处)②§4.4 迁移后不变量测试 ③§7 跨面契约测试兜底。

---

## 6. 消费者切换(方案 A 已就位,仅换键**来源**)

- **`push_order_by` 三个 folder 分支**([queries.rs:1422](../../../src-tauri/src/db/queries.rs#L1422)):
  `TREE_SORT_KEY(d.rel_path)` → `d.tree_sort_key`,即
  `dir_order = "r.created_at ASC, r.id ASC, d.tree_sort_key ASC, d.id ASC"`。三分支(similarity/filename/datetime)
  共用该前缀,末尾 `, m.id {order_dir}` tiebreaker 不动。更新其 doc 注释(不再提标量函数)。
- **`enricher::enrichment_order_clause`**:folder 分支 `rel_path`(方案 A 的「近似」)→ `d.tree_sort_key`,升为**精确
  DFS** 且去逐行函数开销;同步订正方案 A 遗留的「近似」注释为「精确」。
- **`build_dir_rank`(内存)不动**:D≈10⁴ 即时算 `encode_tree_sort_key`,读列零收益,且保持内存路径独立于列。刚性
  契约仍成立——两侧都等于 `encode_tree_sort_key(rel_path)`。
  - **(可选,默认不做)** 若要让内存侧也读列:`query_dir_labels` 的 SELECT 加 `d.tree_sort_key`、`DirLabel` 加
    `tree_sort_key: Vec<u8>` 字段、`build_dir_rank` 用列而非算。**收益为零**(算键本就免费),仅在「想彻底删掉内存侧
    的 `encode` 调用以只留一处逻辑」时才考虑;会牵动 `DirLabel` 全部构造点(含 `justified.rs` 基准、`dl()` 助手),
    得不偿失。默认**保持算**。

---

## 7. 测试面(**B 的隐藏成本**——现有清单低估处,诚实标注)

「换 `ORDER BY` 一行」的表面下,真正的成本在**测试 fixture**:

- **现状**:约 15 处测试用裸 `INSERT INTO directories (... rel_path ...)`(`queries.rs` 多处、`migration.rs`、
  `fast_scan.rs`、`live_photo.rs`),这些**不填** `tree_sort_key` → 取 `DEFAULT X''`。一旦 `push_order_by` 改读
  `d.tree_sort_key`,凡**断言 folder 顺序**的 SQL 测试会因「所有 dir 键都空 → 退化按 `d.id`」而错序。
- **刚性契约测试首当其冲**:`canonical_derive_order_matches_sql_order` 内存侧算真键、SQL 侧读空列 → 对拍不等 → 红。
- **解法(择一,推荐 ①)**:
  - **① 局部回填 helper**:仅在**断言 folder-SQL 顺序**的测试(canonical 对拍 + folder-sort SQL 测试),fixture
    插完后加一行 `conn.execute_batch("UPDATE directories SET tree_sort_key = TREE_SORT_KEY(rel_path)")`。这些测试已
    (或应)注册 `TREE_SORT_KEY`,数量**个位数**且集中,**非每个 INSERT** 都改。
  - **② 触发器自维护**:`CREATE TRIGGER ... AFTER INSERT ON directories ... SET tree_sort_key=TREE_SORT_KEY(NEW.rel_path)`。
    裸 INSERT 自动填,免逐测试改;**但**要求所有写连接(含 `mock_data` 自建连接)注册函数,且给**热扫描写路径**加
    每行触发器开销 + 隐藏控制流。**不推荐**(与项目「显式维护」风格相悖)。
- **新增跨面契约测试**:同一 fixture 下,画廊 folder unique-dir 序 == 文件树 DFS 诱导子序,且 SQL 侧由**列**驱动
  (而非函数),锁住 stored-key 与 computed-key 一致。
- **考虑过但否决的自动化**:
  - **generated column**(`tree_sort_key BLOB GENERATED ALWAYS AS (TREE_SORT_KEY(rel_path)) STORED`):SQLite 生成列
    表达式引用 **application-defined 函数极脆**——schema 在**每次连接 open 时解析**,函数此时尚未注册即报错;`sqlite3`
    CLI / 只读池 / 任何未注册连接打开该库都会崩。**否决**。
  - **触发器**:见 ② ,同因不推荐。

---

## 8. 索引(**refine 现有 B 清单**:先 EXPLAIN,大概率不需要)

现有 B 清单列了「新增 `(parent_id, tree_sort_key, id)` 与 `(root_id, tree_sort_key, id)` 索引」。核实后**修正**:
- 画廊 folder 查询的 `ORDER BY` 跨三表,filesort 不可消解(§2.2),**单表 `tree_sort_key` 索引不会被该排序用上**——
  加了是死索引 + 写放大。
- 文件树同级查询按 `d.name`(单段,序等价于 `tree_sort_key`),不需改、不需索引。
- **结论:不预先加索引**。仅当将来出现「单独按 DFS 序扫描/过滤 `directories`」的**具体**查询,再据
  `EXPLAIN QUERY PLAN` 决定。本条从「施工步」**降级**为「按需、须 EXPLAIN 证据」。

---

## 9. 施工步骤(**仅当 go**)与建议提交

1. **迁移地基** — `SCHEMA_V19`(schema.rs)+ `STEPS` 追加 + `CURRENT_VERSION`→19 + `run_migrations` 顶部自注册
   + 全新/幂等/回填不变量迁移测试。
   → 提交1 `feat(db): 持久化目录前序 DFS 排序键列 + 回填(V19)`
2. **稳态写路径** — (a) `upsert_directory` + (b) `move_directory` + (c) `mock_data.rs`;若选 §6 可选项则连 `DirLabel`。
   → 提交2 `feat(db): 目录写路径维护 tree_sort_key`
3. **消费者切列** — `push_order_by` + `enricher` 改读 `d.tree_sort_key` + §7 测试 fixture 回填 helper + 跨面契约测试。
   → 提交3 `perf(gallery): folder 目录序改读持久键列`
4. **(独立、正交)** `parent_id/depth` 一致性修复若要做,**单独提交**,勿混入本序目标。

每提交只暂存本任务路径(精确 pathspec——工作区存在并行 frontend 改动,须排除)。

---

## 10. 验收标准

- **全量 `cargo test` 绿**(新迁移测试 + 跨面契约 + 既有 501)+ `cargo fmt --all -- --check` + `cargo clippy --lib`
  + `node tools/check_docs.mjs`(本地非 CI,项目 DoD 校准)。
- **`EXPLAIN QUERY PLAN` 对比**切列前后 folder 查询计划:证明未引入意外全表扫;**确认 filesort 仍在** = §2.2 诚实
  基线(B 不消除 filesort)。
- **大库 profile**:切列前后 filename-folder / similarity-folder 排序时延对比,**证明 B 的收益真实且值得**;若不显著
  则如实记录并可搁置(§2.4 决策门)。
- **真机**:folder 各模式排序结果与方案 A **完全一致**(纯性能,零行为变更);拖滑块行为不回归。

---

## 11. 关键决策

| 决策 | 理由 | ID |
|------|------|----|
| 迁移回填用**已注册标量函数** + `run_migrations` 顶部自注册,**不**扩 runner 支持 Rust 闭包 | 一行注册幂等,且**一处改修好全部裸连接迁移测试**;扩 runner 波及 `migrate_step` 事务边界与结构,收益不抵成本 | D-006 |
| 稳态写路径**Rust 计算并绑 blob**,不用 SQL 函数 | 自足、不依赖连接注册,裸连接测试可用;与 `move_directory` 现有「先算 `new_rel`」风格一致 | D-007 |
| **不预先加** `tree_sort_key` 索引 | 多表 `ORDER BY` filesort 不可消解,单表索引不被用;按需 + `EXPLAIN` 证据再定 | D-008 |
| 否决 **generated column / 触发器**自维护 | app-defined 函数在 schema/trigger 层对「连接注册时机」极脆(open 即解析);项目重显式维护 | D-009 |
| `build_dir_rank` **保持内存算键**、不读列 | D≈10⁴ 算键免税,读列零收益且保内存路径独立;刚性契约仍两侧同 = `encode_tree_sort_key` | D-010 |

---

## 12. 与方案 A 的接口(衔接点清单)

方案 A 已交付、B 依赖但**不改**的部分:
- `utils/path.rs::encode_tree_sort_key`(纯函数,B 的写路径与 `build_dir_rank` 共用)。
- `db/mod.rs::register_custom_collations` 里的 `TREE_SORT_KEY` 标量函数(B 的迁移回填复用;B 只在 `run_migrations`
  顶部**新增一次调用**,不改函数本身)。
- 刚性等价契约测试 `canonical_derive_order_matches_sql_order`(B 保持其绿,fixture 侧按 §7 回填)。

B **改动**的部分(全部在 SQL 键**来源**层,不触碰编码语义):
- `+ SCHEMA_V19` / `CURRENT_VERSION` / `STEPS` / `run_migrations` 顶部自注册。
- `directories` 表结构(+1 列)。
- `upsert_directory` / `move_directory` / `mock_data.rs` 写键。
- `push_order_by` / `enrichment_order_clause` 读键改列。
- 测试 fixture 回填 helper + 跨面契约测试。

同工作线记忆:[[dir-sort-unification-2026-07-14]]。

---

# Part II:B-file —— 媒体项 filename 序的缓存 / 持久键(v1.1 新增)

> 并入 2026-07-14 对 filename 排序路径的代码核实 —— 亲读 `query_layout_items` / `canonical_layout_sql` /
> `items_cache::{is_hit_valid, derive_order}` / `layout_commands` 分派。与 Part I(B-dir)**正交**:B-dir 治目录
> 顺序(`directories` 表),B-file 治媒体项组内的文件名次序(`media_items` 表)。

## 13. 问题定性(filename 序为何比 datetime 贵一个量级)

逐条有代码位:

- **filename 无内存复用,datetime 有**。S1 canonical 内存缓存的复用守卫只对 datetime 放行:`is_hit_valid` 的
  `CachedOrder::Canonical => sort_within == "datetime"`([items_cache.rs:126](../../../src-tauri/src/layout/items_cache.rs#L126));
  `derive_order` 只对 `Canonical` 派生轴 / 方向,`Sql`(filename/similarity)在分派点**原样返回**
  ([layout_commands.rs:247-248](../../../src-tauri/src/ipc/layout_commands.rs#L247))。模块头明写「filename/similarity
  特殊排序按 SQL 序原样缓存……只加速滑块 / 窗宽等序不变交互」([items_cache.rs:12-14](../../../src-tauri/src/layout/items_cache.rs#L12))。
- **每次序变 = 全表无 LIMIT 的 NATURAL_CMP filesort**。filename 走 `query_layout_items`,组装完 `push_order_by`
  直接 `query_map().collect()`,**无 LIMIT / OFFSET / 游标**([queries.rs:1000-1040](../../../src-tauri/src/db/queries.rs#L1000));
  排序键 `m.file_name COLLATE NATURAL_CMP`(三个 group_by 分支皆是,如 [queries.rs:1453](../../../src-tauri/src/db/queries.rs#L1453)),
  自定义 collation 无对应索引 → temp B-tree filesort,逐比较调 `natural_cmp` 回调。
- **🔴 量级差前提已被真机 profile 更正(2026-07-14)**:原据 [items_cache.rs:12](../../../src-tauri/src/layout/items_cache.rs#L12)
  「5s 级 SQL 字符串排序」+ [queries.rs:1108](../../../src-tauri/src/db/queries.rs#L1108)「datetime 内存 ~100ms」推
  「filename 慢一个量级」。**B-dir profile 顺带实测**(bin `sort_profile`,真机 543k 库暖缓存):folder+filename 的 SQL
  NATURAL_CMP filesort 仅 **~440ms(亚秒)**,**非 5s**——「5s」或系更早 schema / 冷缓存 / 1M mock 库的测值,或与
  [items_cache.rs:190](../../../src-tauri/src/layout/items_cache.rs#L190) 的**内存**排 ~200B 结构体 5-7s 混淆。故现实是
  filename SQL 排序 ~440ms vs datetime 内存派生 ~100ms = **约 4-5×**,而非「一个量级」。B-file-i 把 filename 轴/方向切换
  从 ~440ms SQL filesort 降为亚秒内存派生**仍成立、仍有价值,但收益倍数远小于原估**,且只作用于**重复重排**(首次基准
  NATURAL_CMP 排序不可免)。详见 §2.4 已实测裁决。
- **enricher 同病**:每批(`ENRICHMENT_BATCH`)的 filename 分支同样 `file_name COLLATE NATURAL_CMP`,对剩余未富化集
  从零 filesort,重复 ≈ 总数 / 批大小 次(见 [[lexicmp-natural-cmp-overflow-2026-07-14]] 记录的 enricher 全表重排痛点)。

**两处措辞收紧**(免规范文档失真):
- **「每次」≠ 每次交互**:同一 filename 视图纯滚动 / 改窗宽仍命中缓存**不重排**;真正触发全表重排的是**进入
  filename / 改方向(asc↔desc)/ 换分组轴 / `data_version` bump / 换过滤器**。痛点恰在:datetime 这些轴 / 方向切换走
  内存派生(免费),filename **每一种排列都是一次全新 SQL filesort**。
- **「543k」= 默认全库视图的量级**(`WHERE is_deleted=0 AND companion_of IS NULL`);过滤视图只排子集。

## 14. 两档解法(关键:第一档不是列,是运行时基准缓存)

### 14.1 B-file-i:运行时内存 filename 基准序 + 整数 rank(推荐先行,零迁移)

> 🟢 **已施工落地 + 本地验证 + 真机实测(2026-07-14,提交 `779df5a` 后端 / `d73be88` bench)**。
> 全量 lib 516 test(含新增刚性对拍 `filename_derive_order_matches_sql_order`,文件名取 img2<img10<img100
> 锁 NATURAL_CMP)+ fmt + clippy 绿(本地非 CI)。**真机 543,449 行实测**:进入 filename 视图付一次基准
> SQL 排序(~862ms,materialized);此后 filename 轴/方向切换 = **~26ms 内存派生(memo miss)** / 0.66ms
> (memo hit,滑块/窗宽),取代改动前每次 ~700ms 全表 SQL NATURAL_CMP filesort ≈ **27×**,与 datetime 平权。
>
> **施工期 2 处设计 refine(见修订记录 v1.3)**:
> 1. **rank 由 SQL 赋,不在 Rust**:`LayoutItem` **无 `file_name` 字段**(重列早移出布局查询),内存无从
>    跑 `natural_cmp`。改由新增查询 `query_layout_items_filename_baseline` 下发 `ORDER BY file_name
>    COLLATE NATURAL_CMP ASC, id ASC`,**行位置即 `filename_rank`**——比设计原文的「Rust 排序」更省(不
>    塞回 file_name、不重复一份 natural_cmp),且那一次排序恰是不可免的基准。故不需单独 rank 数组:items
>    存基准序,`derive_order` 的 enumerate 下标即 rank(新 `CachedOrder::CanonicalFilename` 变体)。
> 2. **date+filename 留在 SQL,不进内存派生**:date 轴需按**本地 epoch_day** 分桶,而本地时区 epoch_day
>    在内存侧无法与 SQL `date(…, 'localtime')` 逐值对齐(午夜边界)→ 会撞破对拍契约。故 B-file-i v1 只覆盖
>    **none/folder + filename**(用户核心用例:漫画/连续剧的文件夹 + 文件名序);date+filename 仍走
>    `query_layout_items` 精确 SQL 序(正确,只是不加速),`is_hit_valid` / compute_layout 分派据此放行。

**核心手法**:让 filename **加入 datetime 家族的廉价派生路径**。缓存填充时对视图做**一次** NATURAL_CMP 自然序
排序,给每个 item 赋一个**整数** `filename_rank`(它在自然序里的稠密位次 0..N-1);此后 filename 的 folder / date /
asc / desc 全部由 `derive_order` 在**整数元组**上内存派生,与 datetime 用 `(dir_rank, sort_datetime, id)` 完全对称 ——
只把次键从 `sort_datetime` 换成 `filename_rank`。

**为什么必须动 `derive_order`(诚实:它现在是 datetime 硬编码)**:核实
[derive_order](../../../src-tauri/src/layout/items_cache.rs#L194) —— folder 轴把次键**写死**为整数 `(sort_datetime, id)`
(紧凑元组 `(u32,i64,i64,u32)` 才有亚秒速度);date / none 轴用 `.rev()` / 恒等,**假设基准序恒为 `sort_datetime DESC`**,
且 datetime 下 **date 轴与 none 同序**(基准已按天单调,date 分组零重排)。filename 不同:
- date 轴对 filename **不再是 no-op** —— SQL 是 `ORDER BY date_expr, file_name NATURAL_CMP`(按天分桶 + 桶内文件名),
  故 filename 的 **folder 与 date 两轴都要 keyed 整数排序** `(bucket, filename_rank, id)`(bucket = dir_rank 或
  epoch_day),只有 none 轴才是基准 / 反转;
- 次键 `sort_datetime` → 传入的 per-item 整数 rank(datetime 传 `sort_datetime`,filename 传 `filename_rank`);
- 基准方向参数化(datetime 基准 DESC,filename 基准 ASC,`.rev()` / 恒等的选择随之翻转);
- `CachedOrder` 从二元(`Canonical` / `Sql`)扩出**可派生的 filename 基准**形态(携 rank 数组),`is_hit_valid` 放行
  filename 家族的轴 / 方向派生,而非现在的精确三元组匹配。

**收益(精确)**:filename 的**轴 / 方向切换**从「每次全表 SQL NATURAL_CMP filesort(真机 543k 实测 **~440ms**,非早前
估计的 ~5s——见 §13 更正)」变为「内存整数派生(亚秒)」—— 与 datetime 平权;一次性基准排序**每 filter / data_version
只做一次**。**不消除**首次基准排序本身(那次 NATURAL_CMP 排序不可免),消除的是**重复重排**。收益倍数(~440ms→亚秒
的重复重排消除)较原估温和,B-file 是否值得由「先测 filename 使用频率」门(§15)裁,不因本次 profile 而提前 go。

> 🔴 **更正(2026-07-14,§14.3.2)**:此段 ~440ms 是通用 folder+filename(filename 二级键)的数;B-file-i 实际走
> `filename_baseline`(filename 全表主键)= **891ms release / 5279ms dev**(冷 MISS 反比旧通用查询贵约 1 倍,换取暖
> 切换 ~27ms)。用户真机日志(dev 5.5s)证明「重复全表重排」并未如本段设想被摊薄——folder↔date 切分组 / 换 filter
> 每次都打穿单槽缓存重付基准排序。隔离实证:该基准成本 93%(dev)是 `NATURAL_CMP` FFI。根治见 §14.3.2 方案 1。

**契约**:新增刚性对拍 —— `derive_order` 的 filename 派生序 == SQL `push_order_by` filename 序。同构前提是
`filename_rank` **由同一 `natural_cmp` 赋秩**(稠密 rank ⇔ collation 序),与方案 A「标量函数 ↔ 内存函数同一逻辑」
的同构论证同型。

**成本 / 风险**:纯运行时,无 schema、无迁移;改动集中在 `items_cache`(`CachedOrder` / `derive_order` /
`is_hit_valid` / 缓存签名)+ 一处赋 rank。风险点是泛化 `derive_order` 别踩坏 datetime 既有对拍(datetime 分支须逐值
等价保持 —— 同方案 A「新默认与旧行为逐值等价」的安全泛化判据)。

### 14.2 B-file-ii:持久化 filename 自然序键列(真·B-dir 同源,重,后手)

**机制**:`media_items` 加 `file_name_sort_key BLOB`(字节可比的自然序编码),SQL `ORDER BY` 该列(BINARY,免自定义
collation),**纯 filename 无分组**视图可建索引消 filesort。

**与 B-dir 同源、但难一个量级**:
- **同源**:预计算键避免每查询重算(B-dir 存目录 DFS 键,B-file-ii 存文件名自然序键)。
- **差异①·编码难**:自然序字节键要把**数字段**编成「定宽 / 长度前缀」才能让 `memcmp` == 自然序(否则 `2` > `10`)——
  正是 [[lexicmp-natural-cmp-overflow-2026-07-14]] 那个数字溢出坑的近亲,还要处理大小写折叠 / Unicode / 前导零怪癖;
  比 B-dir 的 path DFS 键(段 + `0x00`,无数字语义)难得多,须**单独设计 + parity 验证**(仿 A 的 natural_cmp 3000 组对拍)。
- **差异②·规模**:`media_items` 是 **10⁶** 行(B-dir 的 `directories` 仅 10⁴),回填、写放大、索引体积全放大百倍。
- **差异③·filesort 仍在**:folder / date 分组叠加时 `ORDER BY` 仍跨三表(§2.2 边界原样适用),单列 filename 索引
  **只在「纯 filename 无分组」视图**才可能被用上。
- **差异④·写触点多**:每次文件名变更(rename / move / 重扫)都要重算键,触点比 `directories` 广。

**结论**:B-file-ii **只在** B-file-i 已上、profile 仍证明「那次基准 NATURAL_CMP 排序」(而非重复重排)是瓶颈、且纯
filename 无分组视图确为热点时才考虑;其自然序键编码须作为**前置子任务**单独设计验证。

### 14.3 双键统一缓存(B-file-i 收尾:闭合 datetime↔filename **跨轴切换**)

> 🟢 **已施工落地 + 本地验证(2026-07-14,提交 `4ea7b68`)**。全量 lib **518 test**(含新增双向刚性对拍
> `dual_key_cross_axis_derive_matches_sql_order` + 纯逻辑单测 `datetime_baseline_derives_filename_via_rank`)+
> fmt + clippy `-D warnings` 绿(本地非 CI)。**真机 GUI 拖滑块/轴切换手感验收仍 pending**(与阶段 7 同,GUI/CI 不覆盖)。

**触发(用户 Request 4 诊断)**:B-file-i 上线后,用户反馈「分组(date↔folder)切换快,但**文件夹下再切换
日期/文件名排序**仍明显慢,体感 2-3s」。查明根因 = **取数缓存是单槽**(`RwLock<Option<ItemsCacheData>>`,只存
**一个基准**):datetime 用 `Canonical`、filename 用 `CanonicalFilename`,**切换组内排序轴会把另一基准挤掉** →
全表 SQL 重查(~700ms-1s MISS);而分组/方向切换不换基准走内存派生(~15ms HIT)。逻辑证明「慢的是后端 MISS
非前端渲染」:date→folder 分组切换也渲染同一个「几千分隔符的文件夹视图」却快 ⇒ 文件夹渲染本身不慢 ⇒
folder+datetime→folder+filename 渲染同一视图却慢的增量只可能是后端重查。**B-file-i 只提速 filename 轴内部
切换(none↔folder、asc↔desc),未解 datetime↔filename 跨轴切换**——那在新旧代码里都是 MISS。

**核心不对称(修法据以成立)**:**每个 `LayoutItem` 都自带 `sort_datetime`**(布局查询列),但**没有 `file_name`**
(重列早移出)。故:
- **任一基准派 datetime = 免费**:items 各取 `sort_datetime` 跨键实排 / 反转,无需额外数据。
- **派 filename 需整数位次**:filename 基准 items 本按自然序存(**下标即位次**);datetime 基准无位次 → 需一个
  `filename_rank` 数组。

**机制**:让同一份缓存**同时服务两轴**。
- **单一事实源** `can_derive_axis(group_by, sort_within)`:datetime 任意 group / filename 仅 none/folder(date+filename
  仍 §14.1 留 SQL)。`is_hit_valid` 与 compute_layout 的 HIT `order_ok` **共用**,杜绝判据漂移。
- **`derive_order` 按请求轴 `sort_within` 参数化**(不再从 `data.order` 反推):**同键**(请求轴 == 基准物理序)走
  反转 / 恒等快捷(逐值等价旧行为);**跨键**实排 `(次键, id)`(次键 = filename 的下标 or `filename_rank[i]` /
  datetime 的 `sort_datetime`)。folder 轴恒实排,跨键仅换次键来源。`PermMemo` 键加 `sort_within`(同缓存两轴置换不同)。
- **datetime 基准的 `filename_rank` 惰性补**(`OnceLock<Vec<u32>>`):用户**首次**从 datetime 切到 filename 时,
  compute_layout 的预填助手 `ensure_filename_rank_for_hit` 在 **items 读锁外**跑一次 **id-only** NATURAL_CMP 查询
  (`query_item_ids_filename_order`,~取整行查询之半),把「全局位次 → id」映射回缓存 items 建 rank,经 `&self` 在读锁下
  `set`;算得的 rank 依 `(filter, data_version)` 定,查询期间缓存换代则重取锁校验丢弃白算、绝不误填。此后
  datetime↔filename 互切走内存派生 HIT(真机实测 ~16-28ms,见 §14.3.1)。**best-effort**:预填失败仅记日志,
  `order_ok` 见 `filename_rank` 未就绪即退化本次 MISS 全量重查一次(不以 `i64::MAX` 坏序命中)。

**收益**:datetime↔filename 跨轴切换从「换基准 → 全表 SQL MISS 重查」降为「内存派生 HIT」。两方向**不对称**:
**filename→datetime 从第一次即零额外成本**(sort_datetime 现成,直接跨键实排);**datetime→filename** 首次付一次
id-only NATURAL_CMP 查询建 rank(真机 **530ms**,约整行 filename 基准重查的六成),此后永久 HIT。

#### 14.3.1 真机实测(2026-07-14,`sort_profile` on 543,449 项生产库,release,5-run median)

`cargo run --release --bin sort_profile`;跨键派生走生产同一 `derive_order`(datetime 基准经 `query_layout_items_canonical`
建、`filename_rank` 经 `query_item_ids_filename_order` + `ensure_filename_rank_for_hit` 同构逻辑填):

| 切换方向 · 轴 | 改动前(MISS 重查该轴基准 SQL) | 改动后(纯内存派生) | 省 |
|---|---|---|---|
| filename→datetime · none | ~319ms(`canonical` 顺序扫 + 内存补序) | **17.6ms** 跨键实排 | ~301ms |
| filename→datetime · folder | ~319ms | **28.1ms** 派生(memo miss) | ~291ms |
| datetime→filename · none(rank 就绪) | ~893ms(filename 基准 NATURAL_CMP filesort) | **15.7ms** 跨键实排 | ~877ms |
| datetime→filename · folder(rank 就绪) | ~893ms | **25.0ms** 派生(memo miss) | ~868ms |
| 一次性 `filename_rank` 构建(仅首次 datetime→filename) | —(改动前无此路径) | **530ms** id-only NATURAL_CMP 查询 | 一次性 |

**一次性入口成本(两版皆付,不消除)**:进入 filename 视图基准排序 **893ms**(NATURAL_CMP filesort,§14.1);进入
datetime 视图基准 **319ms**(顺序全表扫 + 内存 `sort_canonical`,无 collation FFI——这正是「派 datetime 免费、派
filename 需 rank」这条不对称的物理根源)。

**诚实边界**:① 首次 datetime→filename 切换**实际付** `530ms(rank) + 25ms(派生) ≈ 555ms`(仍略优于改动前 893ms
全表重查),**第二次起**才是 ~25ms;filename→datetime 则从第一次就 ~18-28ms。② profile 测的是**纯后端派生**,不含
IPC 序列化 + Vue 渲染——用户体感的「2-3s」还叠这两层,后端消除的是其中最大那一项(全表重查),故**真机 GUI 手感
验收仍 pending**。③ 记忆/早前文本里 rank 构建的 ~378ms 估算已被本次实测(530ms)更正。

**契约**:新增双向刚性对拍 `dual_key_cross_axis_derive_matches_sql_order` —— datetime 基准派 filename(none/folder×
asc/desc)、filename 基准派 datetime(none/date/folder×asc/desc)均与 SQL `push_order_by` 精确序逐项一致(错位即
SelectAll↔flat_ids 选区漂移)。`is_hit_valid` 守卫测试同步改为对称语义。

**成本 / 边界**:datetime 缓存首次切 filename 后多驻留 ~2MB(543k 的 u32 rank 数组);date+filename 仍走 SQL 精确序
(§14.1 边界不变,epoch_day 内存无法对齐);首次基准 NATURAL_CMP 排序本身不消除(那是不可免的一次)。**诚实**:
本次消除的是「跨轴切换的重复全表重排」,不是「首次进入某轴的基准排序」。

#### 14.3.2 collation FFI 隔离实证(2026-07-14,root cause 确认,dev + release 对拍)

**触发**:用户真机报「顶栏点筛选 / 切分组等待过长」——dev >5s、release >1s。日志三条 `compute_layout MISS`
(axis=`folder/filename` ↔ `date/filename`,全库 543,449 项)`sql` 分项 5227–5541ms(dev)。为定位「是我们自研
`NATURAL_CMP` collation 的 SQLite→Rust FFI 开销,还是 join / 18 列物化 / filesort 结构本身」,给 `sort_profile`
加**决定性隔离段**(提交见下):同一批行、同一条 SQL、同一 filesort 结构,**只把比较函数从 `NATURAL_CMP`(FFI)
换成内建 `BINARY`(memcmp,无 FFI)**,两者耗时差 = 纯 collation FFI 成本(BINARY 改变次序但不改行数 / filesort
结构,只比耗时);`none/datetime`(走 `idx_media_sort` 索引扫、免 collation)作对照下限。id-only 排除物化。

| id-only,543,449 行 | **dev** NATURAL / BINARY / 纯 FFI | **release** NATURAL / BINARY / 纯 FFI |
|---|---|---|
| none/filename(filename 全表主键) | 4670 / 341 / **4329ms(93%)** | 570 / 201 / **369ms(65%)** |
| date/filename(date 主键 + filename 次键) | 4008 / 508 / **3500ms(87%)** | 581 / 308 / **272ms(47%)** |
| none/datetime(对照,免 collation) | **304ms** | **158ms** |

`EXPLAIN QUERY PLAN` 三者结构一致(`SCAN d USING COVERING INDEX idx_dir_root` → `SEARCH r/m` → `USE TEMP
B-TREE FOR ORDER BY`)——filesort 皆在场,差异只在比较函数。

**三个铁证**:
1. **纯 NATURAL_CMP FFI 占 MISS 时间 93%(dev)/ 65%(release)**;换 BINARY 后 filename 排序(341ms dev / 201ms
   release)≈ datetime 对照(304 / 158ms)——join / 物化 / filesort 结构**都不是瓶颈**。
2. **归属证据**:非 FFI 的活儿(BINARY 排序、datetime 索引扫)dev/release ≈ 1.7–1.9×(未优化代码正常倍率);而
   `NATURAL_CMP` **纯 FFI 差值 dev/release ≈ 12×**(4329/369、3500/272)。1.8× vs 12× 的鸿沟把「dev 5s vs
   release 1s」**唯一地**钉在我们那段 `natural_cmp` Rust FFI 上(dev 未优化 + overflow-checks)——非 SQLite。
3. **profile 精确复现日志**:`filename_baseline` 5279ms(dev)≈ 日志 folder/filename 5513ms;date/filename
   4008ms(dev id-only)+ 物化 ≈ 日志 5240ms。地面真相对齐。

**🔴 更正 §2.4 / §14.1 的「~440ms」框定**:那个 ~440ms 是**通用 `query_layout_items(folder+filename)`**(filename
作**二级键**,仅组内比较、collation 调用少)= release 391ms。但 **B-file-i 落地后 folder/filename MISS 实际走
`query_layout_items_filename_baseline`**(filename 作**全表主键**,collation 调用最多)= **891ms release /
5279ms dev**(见 §14.3.1 的 893ms)。即 filename 序真实冷 MISS 成本比旧框定高约 1 倍;且 B-file-i 用主键 baseline
换取暖切换 ~27ms,**冷 MISS 反比旧通用查询(release 391 / dev 2643ms)贵约 1 倍**——这是「切分组卡」的一部分诱因。

**对两个候选方案的量化裁决**(收益均落在消除这段 FFI):
- **方案 1(全局 filter-invariant 内存 rank)**:一次性构建 = 真机 **546ms(release)/ 4651ms(dev)** id-only
  NATURAL_CMP(开机后台藏);之后**所有** filename 轴 / 方向 / **跨轴** / 换 filter 切换 = ~16–27ms(release)内存
  派生。顺带把 `date+filename` 做成内存可派生(Rust 侧算 `epoch_day` 对齐 SQL `date('localtime')`),根治
  folder↔date thrash。风险低(复用现成 `natural_cmp`,无新编码),正确性用现有对拍范式钉。**推荐主攻**。
- **方案 2(B-file-ii 持久字节键)**:效果 = 上表 BINARY 列 = **201ms(release)/ 341ms(dev)**,**持久、永不重建**。
  但相对方案 1 只是把「一次性 546ms 构建」再压到 201ms(**非阶跃**),却要背字节键编码风险(lexicmp 溢出坑近亲)+
  10⁶ 行 V20 迁移 + 写触点。**维持 D-013「后手、须显式 go」**,仅当方案 1 的一次性构建被证明无法接受、且纯 filename
  视图确为热点时才启。

**产物**:`sort_profile` 新增「决定性隔离」段(NATURAL vs BINARY + `EXPLAIN` + dev/release 双跑),fmt + clippy
`-D warnings` 绿(本地非 CI)。

## 15. B-file 的决策门与排期

> ✅ **前置门已通过(2026-07-14)**:用户**亲证 filename 是核心用例**——「过去经常用 filename 排序,漫画/
> 连续剧之类必须按 name 排序」。故 B-file **立项成立**,B-file-i 已施工落地(见 §14.1 banner)。埋点(异步
> localStorage `sortModeUsage` 计数,提交 `0a47b70`)仍已上线作**持续观测**用途(非门禁,决策已由用户亲证下定)。
> 叠加 §2.4 profile,filename SQL 排序虽已亚秒但轴/方向切换每次全表重排是真实卡顿源,B-file-i 消之(实测 27×)。

- **前置门(最硬)**:先测 **filename 实际使用频率**。datetime 是默认排序、已拿走全部缓存投资;filename / similarity
  是次级排序,频率未知。按项目「测试 / 投资跟随风险」原则**不为冷门排序过度投资** —— 若埋点证明 filename 罕用,整个
  B-file **搁置**(完全可接受的结论)。**〔已裁:用户亲证核心用例,门通过。〕**
- **顺序**:B-file-i(便宜、直闭复用缺口)→ 仅当仍不足再 B-file-ii(重、需键编码)。
- **验收**:B-file-i = filename 轴 / 方向切换时延对比(证明从 SQL filesort 降为内存派生)+ 新增 filename 对拍绿 +
  datetime 既有对拍不回归;B-file-ii = 键编码 parity 测 + `EXPLAIN` 证明纯 filename 视图用上索引 + 大库回填时延可接受。

## 16. 新增决策(扩 §11 的 D-006..D-010)

| 决策 | 理由 | ID |
|------|------|----|
| filename 序治理**先上运行时内存基准 + 整数 rank(B-file-i)**,不是持久列 | 最便宜、零迁移,直接闭合「canonical 只服务 datetime」的复用缺口;让 filename 加入 datetime 的廉价整数派生路径 | D-011 |
| B-file-i **须小幅泛化 `derive_order`**(次键参数化 + 基准方向参数化 + date 轴对 filename 走 keyed 排序),而非「免费复用」 | 核实 `derive_order` 现把次键写死 `(sort_datetime,id)`、`.rev()` 假设 datetime-desc 基准、且 date 轴对 datetime 是 no-op 对 filename 不是;泛化须保 datetime 既有对拍逐值等价 | D-012 |
| **B-file-ii(持久 filename 键)是后手**,前置 = 自然序字节键编码单独设计验证 | 数字定宽 / 长度前缀编码是 lexicmp 溢出坑近亲,难于 B-dir 的 path 键;且分组叠加时 filesort 不消除、仅纯 filename 视图受益 | D-013 |
| 整个 B-file 受**「先测 filename 使用频率」**前置门约束 | datetime 默认、filename 次级;投资跟随风险,不为冷门排序过度投资 | D-014 |
| **双键统一缓存**收尾 B-file-i:同一缓存同时服务两轴,闭合 datetime↔filename 跨轴切换的 MISS(用户 Request 4「文件夹下切排序 2-3s」) | 单槽缓存换轴即挤掉另一基准 → 全表重查;而每项自带 sort_datetime(派 datetime 免费)、filename 位次可惰性补(datetime 基准一次 id-only 查询) → 跨轴切换降为内存派生 HIT | D-015 |

**落地状态(2026-07-14)**:**D-011 / D-012 已 landed**(B-file-i,提交 `779df5a`);**D-014 门已通过**
(用户亲证核心用例);**D-015 双键统一缓存已 landed**(提交 `4ea7b68`,lib 518 test + fmt + clippy `-D warnings`
绿,本地非 CI)。**D-012 施工 refine**:泛化实为「次键参数化(filename 用基准下标 / datetime 用
sort_datetime 值)+ 基准方向参数化(filename 基准 ASC / datetime 基准 DESC)」;原列的「date 轴对 filename
走 keyed 排序」**未实现**——date+filename 因本地 epoch_day 分桶无法与 SQL `date('localtime')` 内存对齐,
**留在 SQL 精确序**(见 §14.1 施工 banner)。**D-015 施工要点**:`derive_order` 进一步由「基准反推轴」改为「按
请求轴 `sort_within` 参数化」(同键反转快捷 / 跨键实排),新增 `can_derive_axis` 单一事实源 + `filename_rank`
惰性 `OnceLock` + `query_item_ids_filename_order`(id-only)+ 预填助手(best-effort,详见 §14.3)。**D-013
(B-file-ii)仍待 go**。

---

## 修订记录

- **v1.6(2026-07-14)** —— **collation FFI 隔离实证 + root cause 确认(新增 §14.3.2)**。触发 = 用户真机报「点筛选 /
  切分组等待过长」(dev >5s / release >1s,日志 `compute_layout MISS` sql 5227–5541ms)。给 `sort_profile` 加**决定性
  隔离段**(同 SQL 同 filesort,只换比较函数 `NATURAL_CMP`↔`BINARY`,差值即纯 FFI 成本;`none/datetime` 对照;附
  `EXPLAIN`),dev + release 双跑真机 543k 库(exit 0)。**结论**:filename MISS 时间 **93%(dev)/65%(release)是
  `NATURAL_CMP` FFI**(换 BINARY 后 ≈ datetime 对照,证明 join / 物化 / filesort 结构非瓶颈);非 FFI 活儿 dev/release
  ≈1.8× 而纯 FFI 差值 ≈12×,**把 dev 5s 的放大唯一归因于该 Rust FFI**。**就地更正** §2.4 / §14.1 的「~440ms」框定(那是
  通用 folder+filename 二级键数;真实 MISS 走 `filename_baseline` 主键 = 891ms release / 5279ms dev)。量化两方案:方案 1
  全局内存 rank 一次性 546ms(release)/后续 ~27ms,推荐主攻;方案 2 B-file-ii = BINARY 效果 201ms 持久但非阶跃,维持
  D-013 待 go。fmt + clippy `-D warnings` 绿(本地非 CI)。**待用户裁**是否进入方案 1 详细施工设计。
- **v1.5(2026-07-14)** —— **双键统一缓存真机 profile 补测 + 估算更正(§14.3.1)**。给 `sort_profile` 加**反方向**
  (datetime 基准 → 惰性建 `filename_rank` → 派生 filename)实测段,合成完整**双向**证据;真机 543k 库跑通(exit 0)。
  - **实测**(release,5-run median):filename→datetime 派生 **17.6 / 28.1ms**(none/folder),datetime→filename
    派生 **15.7 / 25.0ms**(rank 就绪);一次性 `filename_rank` 构建 **530ms**;入口基准 filename **893ms** / datetime **319ms**。
  - **更正**:v1.4 及记忆里 rank 构建的 **~378ms 估算**被实测 **530ms** 更正;并写清**首次** datetime→filename
    实付 `530ms(rank)+25ms ≈ 555ms`(仍优于改动前 893ms),**第二次起** ~25ms;filename→datetime 从第一次即 ~18-28ms。
  - **诚实**:profile 为纯后端派生,不含 IPC + Vue 渲染,真机 GUI 手感验收仍 pending。仅改 `sort_profile.rs` + 本文档。
- **v1.4(2026-07-14)** —— **双键统一缓存(§14.3 / D-015)施工落地**,收尾 B-file-i、闭合 datetime↔filename
  跨轴切换的 MISS。
  - **触发**:B-file-i 上线后用户 Request 4 反馈「分组切换快、文件夹下切排序仍 2-3s」;查明根因 = 单槽取数缓存
    换轴即挤掉另一基准 → 全表 SQL 重查(MISS ~700ms-1s),分组/方向切换才是 HIT。B-file-i 只提速 filename 轴内部
    切换,未解跨轴。
  - **修法**:同一缓存同时服务两轴。核心不对称 = 每项自带 `sort_datetime`(派 datetime 免费)、filename 需整数
    位次(filename 基准用下标 / datetime 基准惰性补 `filename_rank`,一次 id-only NATURAL_CMP 查询)。
  - **改动**(提交 `4ea7b68`,5 文件 +499/-88):`can_derive_axis` 单一事实源;`derive_order` 由「基准反推轴」改
    「请求轴 `sort_within` 参数化」(同键反转 / 跨键实排)+ `PermMemo` 键加 `sort_within`;`ItemsCacheData` 加
    `filename_rank: OnceLock<Vec<u32>>`;新增 `query_item_ids_filename_order`(id-only,复用 canonical 同源 WHERE);
    compute_layout 加 `ensure_filename_rank_for_hit` 预填(best-effort,读锁外查、竞态丢弃)+ order_ok 对称化;
    新增双向对拍 `dual_key_cross_axis_derive_matches_sql_order` + 单测 `datetime_baseline_derives_filename_via_rank`,
    is_hit_valid 守卫测试改对称语义;sort_profile 加跨键 datetime 派生实测点;顺手修 natural_sort fixture 的
    `manual_is_multiple_of` clippy 告警。
  - **验证**:lib **518 test** + fmt + clippy `-D warnings` 绿(本地非 CI)。**真机 GUI 手感验收仍 pending**。
  - **边界不变**:date+filename 仍 SQL(epoch_day 内存无法对齐);首次进入某轴的基准 NATURAL_CMP 排序不消除
    (那次不可免),消除的是跨轴切换的**重复全表重排**。
- **v1.3(2026-07-14)** —— **Part II 的 B-file-i 施工落地 + 真机实测 + 决策门通过 + 2 处施工 refine**。
  - **状态**:用户亲证 filename 是核心用例(漫画/连续剧按名排序)→ D-014 门通过;B-file-i 按 §14.1 施工,
    提交 `779df5a`(后端:`query_layout_items_filename_baseline` + `CachedOrder::CanonicalFilename` +
    `derive_order` 泛化 + `is_hit_valid` / compute_layout 分派 + 新增 filename 对拍)/ `d73be88`(bench 提速测量)/
    `0a47b70`(异步频率埋点)。全量 lib 516 test + fmt + clippy 绿(本地非 CI)。真机 543k 实测:轴/方向切换
    ~700ms SQL filesort → ~26ms 内存派生(~27×),memo hit 0.66ms。顶部 banner 收窄红线至仅 B-file-ii。
  - **施工 refine 2 处**(设计出稿后核实代码得):
    1. **rank 由 SQL 赋而非 Rust**:`LayoutItem` 无 `file_name`,内存跑不了 natural_cmp;改由基准查询下发
       `ORDER BY file_name COLLATE NATURAL_CMP ASC, id ASC`,行位置即 rank(items 存基准序、`derive_order`
       enumerate 下标即 rank,无需单独 rank 数组)。
    2. **date+filename 不派生、留 SQL**:date 轴需本地 epoch_day 分桶,内存与 SQL `date('localtime')` 无法
       逐值对齐 → 会撞破对拍;v1 只覆盖 none/folder+filename(核心用例),date+filename 走精确 SQL 序。
- **v1.2(2026-07-14)** —— **Part I(B-dir)施工落地 + 施工前验证的设计偏离修正**。
  - **状态**:B-dir 按本设计 §9 施工步骤落地,三提交 `7d97b01`/`d649414`/`f4a85c7`,全量 lib 515 test +
    fmt + clippy + EXPLAIN 全绿(本地非 CI)。顶部加施工状态银 banner;红线收窄为仅约束 Part II。
  - **施工前验证修正 3 处**(设计出稿后代码有演进,亲读核对得):
    1. **§5(c) 导入名**:`use scrollery::utils::path::...` → **`use scrollery_lib::utils::path::...`**
       (lib crate 实名 `scrollery_lib`,`[lib] name`;package 名才是 `scrollery`)。
    2. **§6 / §10 enricher「须补 scan_roots JOIN」worst-case 假设不成立**:enricher 查询 per-root
       (`WHERE d.root_id=?1`),单根内 rel_path 唯一 → tree_sort_key 唯一,`ORDER BY d.tree_sort_key ASC`
       即精确 DFS,**无需**根序前缀 / JOIN。且 enricher 原用 `d.rel_path`(无函数调用),对它收益是**纯
       正确性**(字符串序→DFS 字节序)而非「去逐行函数开销」。
    3. **§2/§9 rusqlite `functions` feature 已启用**(方案 A 落地时加,Cargo.toml),本次无需再改。
  - **验证补强**:§10 的 EXPLAIN 对比已实测 —— 切列前后 folder 查询计划**逐行相同**(`SCAN m USING
    idx_media_sort` / `SEARCH d,r BY PK` / **`USE TEMP B-TREE FOR ORDER BY` 两版都在**),证实 §2.2
    诚实基线:B 不消除 filesort、无意外全表扫;常数因子收益(省每行 FFI+alloc)待阶段 7 真机 profile 量化。
  - **§7 测试面实测**:折 fixture 破坏面为 **2 处** folder 断言测试(canonical 对拍 + folder_filename_sort),
    与「个位数」估计吻合;`v17_backfills` 回拨重放跨越 V19,已补 `DROP COLUMN tree_sort_key`。
- **v1.1(2026-07-14)** —— 并入 filename 排序核实结论。
  - **动机**:核实「filename 大库每次全表 NATURAL_CMP 重排、无内存复用、比 datetime 慢一个量级」的独立隐忧
    (原记于 [[lexicmp-natural-cmp-overflow-2026-07-14]] / [[dir-sort-unification-2026-07-14]]),确认属实并给出治理设计。
  - **结构**:v1.0 全文降为 **Part I(B-dir)**,内容与决策 D-006..D-010 **不变**;新增 **Part II(B-file)** §13–§16 与
    决策 D-011..D-014;新增 §0 家族总览。
  - **关键澄清**:v1.0 未点破「给 filename 加序缓存」与「方案 B 持久列」不是一回事 —— v1.1 拆两档:**第一档是运行时
    内存基准缓存(B-file-i,非列)**,持久列(B-file-ii)是更重的后手,二者决策门不同。
  - **诚实边界统一**:三成员都不消除分组叠加时的跨表 filesort,都受「先 profile / 先测使用频率」决策门约束。
- **v1.0(2026-07-14)** —— 初版,B-dir 目录序持久列(`directories.tree_sort_key`,V19)详细设计,§1–§12 + D-006..D-010。
