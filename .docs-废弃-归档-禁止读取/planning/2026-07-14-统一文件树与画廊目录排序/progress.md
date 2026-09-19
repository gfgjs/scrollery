---
status: active
type: working-memory
line: 统一文件树与画廊目录排序
created: 2026-07-14
---

# 进度日志:统一文件树与画廊目录排序

## 会话:2026-07-14
- 做了:只读核对文件树根顺序、同级目录 SQL、frontend 懒加载拼接、画廊 SQL/内存 rank、separator ID 链和文件名次级排序。
- 做了:只读审计提交 `c2536e6` 与归档 `docs/worklogs/2026-07-14-文件树画廊定位唯一性修复/`，区分应保留的唯一分组/latest-wins 与应替换的 `(rel_path, directory_id)` 最终顺序。
- 做了:确定 canonical contract 与持久化 `tree_sort_key BLOB` 方案，列出 migration、写路径、排序消费者、测试与真实大库验收步骤。
- 约束:本会话不修改源码，不运行全量 frontend/Rust 测试；只对方案文档做轻量校验。
- 工作区:观察到与本任务无关的 frontend 未提交改动及其他 planning 目录；本任务只允许写入 `docs/planning/2026-07-14-统一文件树与画廊目录排序/`。
- 验证:- 验证:三文件已逐项复读且内容完整；限定新方案目录的 `git diff --check` 通过；`node tools/check_docs.mjs` 通过（140 个文档、442 个代码/配置文件）。按用户要求未运行 Rust/frontend 全量测试。
- 遗留:新会话先读本目录三件套，从 `task_plan.md` 阶段 1 开始；不得回滚 `c2536e6`，不得先跑重型全量测试。

## 会话:2026-07-14(评审+施工 A)
- 评审:核对代码验证方案对 `list_scan_roots`/`push_order_by`/`build_dir_rank`/`get_directory_*`/`move_directory` 断言全为真;`directories.name` 为 BINARY(无 COLLATE)→ DFS 字节键复现当前树序、不重排(方案未点破的隐含前提)。
- 关键发现:①迁移回填无法纯 SQL 表达(`||` 强转 TEXT、无 blob 拼接、char(0) 截断)→ 持久列须 Rust 回填;②**刚性等价契约** `canonical_derive_order_matches_sql_order` 强制内存/SQL folder 序逐项一致(防 SelectAll 选区漂移)→ 只改 `build_dir_rank` 会撞破;③"避免比较器 split"顾虑对 per-directory rank 构建是误判。
- 用户裁决(AskUserQuestion):**A 先行、B 列后续**(D-005)。
- 施工方向:方案 A = `encode_tree_sort_key` 纯函数 + 注册 `TREE_SORT_KEY` 标量函数,内存 `build_dir_rank` 与 SQL `push_order_by` 共用同一键逻辑,零迁移;一并修复 datetime/filename/similarity 全部 folder 模式。
- 三件套写回:task_plan「当前阶段/推荐实现/阶段/提交/验收/决策」全部按 A 重排,B 降为「## 后续」;findings「方案选择」加时效横幅。
- 做了(代码,方案 A 全落地):
  - `utils/path.rs`:新增纯函数 `encode_tree_sort_key`(segment+NUL BLOB) + 8 单测(空/单段/多级/`A A- A/Z`/大小写/Unicode/双斜杠防御/basename 归约)。
  - `db/mod.rs`:`register_custom_collations` 加注册 `TREE_SORT_KEY(rel_path)->BLOB` 标量函数(DETERMINISTIC);`Cargo.toml` rusqlite 加 `functions` feature。
  - `db/models.rs`:`DirLabel` 增 `root_created_at/root_id`;`db/queries.rs::query_dir_labels` SELECT 补 `r.created_at, r.id`。
  - `layout/items_cache.rs::build_dir_rank`:比较键 → `(root_created_at, root_id, encode_tree_sort_key(rel_path), dir_id)`;同步 `dl()` 助手、`layout/justified.rs` 基准的 DirLabel 字面量;更新 dir_rank/derive_order 文档注释。
  - `db/queries.rs::push_order_by`:三 folder 分支目录序前缀 → `r.created_at ASC, r.id ASC, TREE_SORT_KEY(d.rel_path) ASC, d.id ASC`(尾部 m.id tiebreaker 不动);`list_scan_roots` 加 `id ASC`。
  - `db/queries.rs`:对拍测试 `canonical_derive_order_matches_sql_order` 期望向量 → DFS 序(desc `[7,1,2,5,3,6,4]`/asc `[1,7,2,3,5,6,4]`),补 `register_custom_collations` 注册;`scanner/enricher.rs` folder 注释纠偏(仍 rel_path,DFS 对齐留 B)。
- 验证(本地非 CI):`cargo test`(scrollery lib)**501 passed / 0 failed / 5 ignored**;`cargo fmt --all -- --check` 通过;`cargo clippy --lib` 无 lint 告警;`node tools/check_docs.mjs` 通过。对拍测试通过=内存/SQL folder 序逐项等价(刚性契约)且多根交错已修(dir20 移本根末尾)。
- 隔离:仅提交本任务 Rust + 三件套路径;工作区并行 frontend 改动(AppShell/MediaGrid*/MediaThumb*)不属本任务,用精确 pathspec 排除。
- 遗留(方案 A):阶段 7 真机大库验收(40 万库拖滑块,文件树不跨 root/子树往返;separator groupId==文件树 active dir_id)未做——CI 不能覆盖 GUI,须真机;方案 B(持久列)按需 follow-up。

## 会话:2026-07-14(方案 B 详细设计,不施工)
- 需求:用户要求「方案 B 出个详细方案但不要施工」。
- 只读核对(为设计接地,非施工):迁移框架(`migration.rs` `STEPS: &[(u32,&str,&str)]` 纯 SQL + `execute_batch`,`CURRENT_VERSION=18`,`migrate_step` 单事务)、`register_custom_collations`(`db/mod.rs`)、写连接注册时机(`connection.rs:57` 建连即注册 `TREE_SORT_KEY`,`lib.rs:204→215` 随后同连接 `run_migrations`)、写路径(`upsert_directory` queries.rs:223 / `move_directory` file_ops_commands.rs:735 / `mock_data.rs:276/303`)、`push_order_by` folder 三分支(queries.rs:1422)、全部 `INSERT INTO directories` 站点归类(生产 2 处均 funnel through `upsert_directory`,余为 `#[cfg(test)]` fixture)。
- 关键设计判断(写入详细设计):①**性能诚实边界**——B 是常数因子优化,**不消除 filesort**(folder ORDER BY 跨三表);默认 datetime-folder 走内存 canonical 缓存本就免费,B 只碰较少用的 filename/similarity-folder SQL 排序 + enricher 批量重排 → 先 profile 再做。②**迁移回填连接依赖**——生产写连接已注册函数,但十余个裸连接迁移测试没注册,V19 回填 SQL 会全线红 → 推荐 `run_migrations` 顶部自注册(一行幂等,一处修全部测试),优于扩 runner 支持 Rust 闭包。③稳态写路径 **Rust 算键绑 blob**(自足),迁移回填**用 SQL 函数**(纯 SQL 步无法跑 Rust),分工清晰。④**测试面隐藏成本**——~15 处裸 INSERT fixture 取 DEFAULT X'' 会破 folder-SQL 排序断言(现有 B 清单低估),解法=断言 folder 序的少数测试补一行回填 UPDATE。⑤**索引 refine**——不预先加(单表键索引不被多表 filesort 用);⑥否决 generated column / 触发器(app-defined 函数对连接注册时机极脆)。
- 产出:`方案B-持久化排序键列-详细设计.md`(§1-12:目标/诚实边界/schema/迁移/写路径/消费者/测试面/索引/施工步/验收/决策 D-006..D-010/与 A 接口);`task_plan.md` 的「## 后续:方案 B」改为指向详细设计的索引(避免双源漂移)。
- 约束:本会话**零源码改动**,只写规划文档;方案 B 仍**勿自动开工**,须单次显式 go。
- 验证:待跑 `node tools/check_docs.mjs`(确认新文档链接解析、门禁绿)。

## 会话:2026-07-14(方案 B 升 v1.1:并入 filename 序分析)
- 需求:用户要求「将 filename 排序分析结合方案 B,新出 v1.1」。
- 核实(接地,非施工;亲读):`query_layout_items`(组装 push_order_by 后直接 collect,**无 LIMIT/OFFSET/游标**)、`canonical_layout_sql`(默认 datetime 走顺序全表扫 + 内存排,一元 `+` 压制 idx_media_sort;注释载明索引序遍历 1M 实测 6.6s)、`sort_canonical`(1M ~100ms)、`items_cache` 模块头(轴切换「5s 级 SQL 字符串排序」→亚秒内存派生)、`is_hit_valid`(`Canonical => sort_within=="datetime"`,filename 走 `Sql` 精确三元组)、`derive_order`(folder 次键**写死** `(sort_datetime,id)` 整数元组、date/none 用 `.rev()`/恒等**假设 datetime-desc 基准**、date 轴对 datetime 是 no-op)、`layout_commands` 分派(datetime→canonical 免 JOIN;余→push_order_by)。
- 核实结论:filename「每次序变全表 NATURAL_CMP filesort、无内存复用、比 datetime 慢一量级」**成立且有代码内实测撑腰**;两处收紧=「每次」应为每次序变/数据写(纯滚动/缩放同序仍命中缓存不重排)、「543k」=默认全库视图量级(过滤视图排子集)。
- 关键设计判断:①方案 B 升「排序键预计算家族」= **B-dir**(目录列)+ **B-file**(filename 序);②B-file 两档——**B-file-i** 运行时内存 filename 基准序 + 整数 `filename_rank`(让 filename 借道 datetime 的整数派生引擎,零迁移,**推荐先行**)、**B-file-ii** 持久 filename 键列(真·B-dir 同源但难一量级:自然序字节键=lexicmp 溢出坑近亲 + 10⁶ 行 + 分组叠加 filesort 不消除,后手);③**B-file-i 须小幅泛化 `derive_order`**(次键 + 基准方向参数化 + date 轴对 filename 走 keyed 排序),非「免费复用」——这是读 derive_order 本体才敢下的判断。
- 产出:详细设计升 **v1.1**——v1.0 全文降为 Part I(B-dir,§1–§12/D-006..D-010 不变)+ 新增 §0 家族总览 + Part II(B-file §13–§16 / D-011..D-014)+ 修订记录;`task_plan.md` B 节改「方案 B 家族」+ 家族 note;记忆 [[dir-sort-unification-2026-07-14]]/[[lexicmp-natural-cmp-overflow-2026-07-14]] 同步。
- 约束:本会话**零源码改动**(`git status` 确认 src-tauri 无变更);方案 B 全家族仍**勿自动开工**,须单次显式 go。
- 验证:`node tools/check_docs.mjs` 通过(EXIT=0,141 文档);Part II 全部代码位引用均亲读核实。

## 会话:2026-07-14(方案 B-dir 施工:持久列落地)
- 需求:用户显式 go「按方案B-持久化排序键列-详细设计.md 开始施工」,并提醒「出方案期间部分代码有改动,施工前先验证」。定位=Part I(B-dir 目录序持久列 V19),Part II(B-file)仍受「先测 filename 使用频率」门约束、本次不开工。
- 施工前验证(亲读核对,修正 3 处设计漂移):
  - **`use scrollery::` → `use scrollery_lib::`**:lib crate 实名 `scrollery_lib`(`[lib] name`),设计 §5(c) 导入路径写错;已确认 `pub mod utils`→`pub mod path`→`pub fn encode_tree_sort_key` 全公开可达。
  - **enricher 无需补 scan_roots JOIN**:enricher 查询 per-root(`WHERE d.root_id=?1`),单根内 rel_path 唯一 → tree_sort_key 唯一,`ORDER BY d.tree_sort_key ASC` 即精确 DFS;设计 §6/§10 的「须补 JOIN」worst-case 假设不成立。且 enricher 原用 `d.rel_path`(无函数调用),对它收益是**纯正确性**(字符串序→DFS 字节序)非「去函数开销」。
  - **`functions` feature 已启用**(Cargo.toml:37,方案 A 落地),无需改。
  - 另核实:`list_scan_roots` 已有 `id ASC`(方案 A 落地);`v17_backfills` 回拨重放跨越 V19,须补 `DROP COLUMN tree_sort_key`;全仓仅 **2 处** folder-断言测试(canonical 对拍 + folder_filename_sort);mock_data 打开 app 已迁移库、不经 lib 建 schema。
- 做了(代码,B-dir 全落地,三提交):
  - **提交1 `7d97b01` 迁移地基**:`SCHEMA_V19`(directories 加 tree_sort_key BLOB NOT NULL DEFAULT X'' + 回填 UPDATE=TREE_SORT_KEY(rel_path) WHERE rel_path<>'');`CURRENT_VERSION` 18→19;`STEPS` 追加 V19;`run_migrations` 顶部自注册 `register_custom_collations`(V19 回填依赖标量函数,一行幂等修好全部裸连接迁移测试);测试=`migrates_fresh` 断言 V19+列存在、新增 `v19_backfills`(非根键非空/根键空/逐位==encode/BLOB 序=前序 DFS 反例 A<A/Z<A-)、`v17_backfills` 补 DROP 列。
  - **提交2 `d649414` 稳态写路径**:`upsert_directory`(覆盖全部生产扫描插入,INSERT 首次写键;ON CONFLICT 不写=键稳定)、`move_directory`(唯一改 rel_path 入口,子树每目录 new_key 同事务写两条 UPDATE)、`bin/mock_data.rs`(ensure_sub_directories 补键,否则 perf 库键全空测不出特性)。三处均 Rust 算键绑 blob(自足)。
  - **提交3 `f4a85c7` 消费者切列**:`push_order_by` 三 folder 分支 `TREE_SORT_KEY(d.rel_path)` → `d.tree_sort_key`;`enricher` folder `d.rel_path` → `d.tree_sort_key`(升根内精确 DFS);两 folder-断言测试 fixture 补回填 UPDATE;新增 `folder_sql_order_driven_by_stored_tree_sort_key_column`(反写键证明消费者读**存列**非函数)。
- 验证(本地非 CI):全量 lib **515 passed / 0 failed / 5 ignored**;`cargo fmt --all -- --check` 绿;`cargo clippy --lib` 无告警;**EXPLAIN QUERY PLAN 对比**(临时 harness 跑完删):BEFORE(函数)与 AFTER(读列)计划**逐行相同**(`SCAN m USING idx_media_sort` / `SEARCH d,r BY PK` / **`USE TEMP B-TREE FOR ORDER BY` 两版都在**)= §2.2 诚实基线证实(B 不消除 filesort、无意外全表扫)。
- 隔离:三提交均精确 pathspec(`git commit -- <本任务 Rust 文件>`);工作区并行 frontend 改动 + 期间并入的 `361ad30`(标题栏 refactor,他会话)未被收割。
- 遗留(B-dir):**阶段 7 真机大库验收**(40 万库 profile filename/similarity-folder 切列前后时延,证明常数因子收益值得;§2.4 决策门——CI/GUI 不覆盖);profile 若不显著则如实记录可搁置。**B-file 家族仍勿自动开工**(先测 filename 使用频率)。

## 会话:2026-07-14(阶段 7 真机 profile:B-dir 切列收益量化,§2.4 决策门)
- 需求:用户「在合适位置增加计时点位量化耗时,然后在真机大库上跑 filename/similarity-folder 切列前后 profile」。
- 关键判断(接地):`d.tree_sort_key` 仅出现在 `push_order_by` 的 `group_by=="folder"` 分支;而 folder+datetime 命中 canonical 缓存、走 `derive_order` 内存派生**绕过本 SQL** → 真正下发目录序 SQL 排序的生产配置只剩 **folder+filename**(从不命中 canonical)与 **folder+similarity**(需 ai_search)。切列 A/B delta 完全落在共享的 `dir_order` 前缀,与二级键正交。
- 做了(代码,提交 `62955fb`,精确 pathspec):
  - **生产计时点位**:`compute_layout` MISS 路径把「布局排序 SQL 本体」从随后的 `query_dir_labels`(10³ 级小查询)分离计时,MISS 日志新增 `sql`/`dirlabels` 分项 + `axis=group/within`。
  - **bench `bin/sort_profile.rs`**:经生产 `create_read_pool`(只读 + 同款 PRAGMA + 注册 collation/函数)只读打开真机库,`view_to_sql` 取生产真身 SQL、字符串替换派生「切列前(`TREE_SORT_KEY(rel_path)`)vs 切列后(`d.tree_sort_key`)」,对 folder+filename / dir-only 跑 A/B(warmup + N runs,报 min/median/mean);similarity 因 `ai_search_results` 瞬态小结果集无法代表全库→诚实跳过,以 dir-only 作等价证据。
- 验证(本地非 CI):`cargo check`/`clippy --bin -- -D warnings`/`rustfmt --check` 均绿;release 构建 OK。
- **实测(真机 543,449 行 · 暖缓存 release · 两轮复现一致)**:
  - folder+filename:切列前 444ms → 切列后 378ms,**省 ~65ms(~15%)**
  - dir-only(隔离纯切列 delta):300ms → 239ms,**省 ~62ms(~20%)**
  - 参考:完整生产查询 `query_layout_items`(18 列物化)702ms,SQL 排序占 ~54%
  - 诊断:schema_version=19、tree_sort_key 回填 9430/9432(2 空=根目录)、ai_search_results=0。
- 裁决(回写 §2.4 + 顶部 banner):切列是**常数因子优化**(每行省 `TREE_SORT_KEY` FFI+alloc),稳定 ~60-65ms,**非阶跃**;B-dir 已 landed 保留(~15% 提速 + 零常态成本 + enricher 纯正确性收益),无理由回滚。**副产物推翻旧前提**:folder+filename SQL 排序实测已**亚秒(~440ms)非 5s**(§13/§14 已加更正 banner)→ 弱化 B-file 紧迫性,B-file 维持「先测 filename 使用频率」门下搁置。
- 遗留:**阶段 7 GUI 真机验收**(拖滑块文件树不跨 root/子树往返;separator groupId==active dir_id)仍未做——CI/GUI 不覆盖,须真机交互;与本次 SQL profile 正交。

## 会话:2026-07-14(B-file-i filename 序内存派生施工 + 埋点,决策门 D-014 通过)
- 需求:用户亲证 **filename 是核心用例**(漫画/连续剧必须按 name 排序)→「先测 filename 使用频率」门通过,B-file 立项;并要求异步埋点、确认与并行顶栏会话无冲突后直接开工。
- 冲突确认:并行会话仅动 `useWindowDrag.ts` 等前端顶栏文件(其间提交 `523cd0c`);B-file-i 纯后端(queries/items_cache/layout_commands)+ 埋点在 `uiStore.ts`(非顶栏组件),零重叠;全程精确 pathspec 提交。
- 施工前验证(设计漂移 2 处 refine):①`LayoutItem` **无 file_name**(重列早移出)→ rank 改由 SQL 赋(基准查询 `ORDER BY file_name NATURAL_CMP ASC, id ASC`,行位置即 rank),非设计原文的 Rust 排序;②date+filename 需本地 epoch_day 分桶,内存无法与 SQL `date('localtime')` 对齐 → 留 SQL,v1 只派生 none/folder+filename。
- 做了(代码,精确 pathspec):
  - **埋点 `0a47b70`**:`uiStore.setSortWithinGroup` 的 `persist===true`(唯一真实用户动作)累加 localStorage `sortModeUsage`;requestIdleCallback 异步写 + try/catch 静默,不阻塞交互。typecheck + eslint 绿。
  - **B-file-i 后端 `779df5a`**:`query_layout_items_filename_baseline`(SQL 赋 rank)+ `CachedOrder::CanonicalFilename` + `derive_order` 泛化(次键 filename 下标/datetime 值、基准方向参数化,datetime 逐值等价)+ `is_hit_valid` + compute_layout 分派(filename+none/folder 走派生,date+filename 走 Sql)。新增刚性对拍 `filename_derive_order_matches_sql_order`(img2<img10<img100 锁 NATURAL_CMP)。
  - **bench 扩展 `d73be88`**:sort_profile 加 B-file-i 派生提速测量。
- 验证(本地非 CI):全量 lib **516 passed/0 failed/5 ignored**;两个对拍(filename 派生==SQL / datetime 不回归)绿;fmt + clippy 绿;`cargo check` match 审计无 `_` 通配遗漏(CanonicalFilename 全显式处理)。**真机 543,449 行实测**:进入 filename 视图付一次基准 SQL 排序 ~862ms;此后 folder/none 轴与 asc/desc 方向切换 = **~26ms 内存派生(memo miss)** / 0.66ms(memo hit),取代改动前每次 ~700ms 全表 SQL NATURAL_CMP filesort ≈ **27×**,与 datetime 平权。
- 回写:顶部 banner 收窄红线至仅 B-file-ii;§14.1 加施工 banner;§15 决策门加已通过 banner;§16 加落地状态(D-011/D-012 landed、D-014 通过、D-013 待 go);修订记录 v1.3。
- 遗留:**B-file-ii(持久 filename 键)仍勿自动开工**(须 go;前置=自然序字节键编码单独设计验证);date+filename 未加速(有意,留 SQL);GUI 真机验收(拖滑块 + 现在可试 filename 轴/方向切换手感)仍待真机。

## 会话:2026-07-14(filename 慢因根因确认 + B-file-iii 全局 rank 设计落盘)
- 需求:用户真机报「顶栏点筛选(视频/收藏/星级)、切分组模式等待过长」(dev>5s/release>1s),附日志三条 `compute_layout MISS`(axis=folder/filename↔date/filename,全库 543,449 项)sql 5227–5541ms。要求分析原因给方案待审;并猜测或与大库/近期 DB 改动有关,提出全量内存缓存/Redis 备选。
- 只读核实(代码地面真相):`compute_layout` MISS 中 `item_count`=全库 543449(filter 不在此层缩集)、`rows`=布局行 43054;folder/filename 走 `filename_baseline`(filename 全表主键 NATURAL_CMP)、date/filename 走通用 `push_order_by`(date 主键+filename 次键);筛选列(media_type/is_favorited/rating)**均有索引**→「点筛选卡」非缺索引。
- 用户裁(AskUserQuestion):**先做 dev profile 确认**再定方案。
- 做了(决定性隔离,提交 `b203262`):给 `sort_profile` 加「NATURAL_CMP vs BINARY」同结构 A/B(只换比较函数)+ `EXPLAIN` + dev/release 双跑真机 543k(exit 0)。**结论**:filename MISS 时间 **93%(dev)/65%(release)是 NATURAL_CMP 的 SQLite→Rust FFI**;换 BINARY 后 filename 排序(341/201ms)≈datetime 对照(304/158ms)→ join/物化/filesort 结构非瓶颈(EXPLAIN 三者皆 USE TEMP B-TREE);非 FFI 活儿 dev/release≈1.8× 而纯 FFI 差值≈12×→dev 5s 放大唯一归因该 Rust FFI。profile 精确复现日志(filename_baseline 5279ms dev≈日志 5513ms)。
- 回写(方案B 文档,同提交):新增 §14.3.2(隔离实证表 + 三铁证 + 两方案量化);就地更正 §2.4/§14.1「~440ms」框定(那是通用 folder+filename 二级键数;真实 MISS 走 filename_baseline 主键=891ms release/5279ms dev,B-file-i 冷成本反翻倍);修订记录 v1.6。fmt+clippy `-D warnings` 绿。
- 否掉备选:Redis 错工具(数据本就全内存驻留 items_cache、瓶颈是重排非取数;SQLite 不能拿 Redis 当后端、Redis 不做自然序 collation);「全量内存缓存」已是现状。
- 用户裁:出**方案 1 详细设计并落盘三件套备审计**。
- 做了(设计落盘):核实 date+filename 物理不对称到代码——`push_order_by` date+datetime 仅按 `sort_datetime` 排序(:1520)、date+filename 按 SQL 本地日期桶 `date(...,'localtime')`+filename(:1516);run_layout 日期分组用 **UTC 桶** `div_euclid(86400)`(justified.rs:219/237/277)。**副发现**:两者本就 UTC vs 本地不一致,是「date+filename 无法内存对齐」真正根因(F-006)。新建 [B-file-iii-全局filename-rank内存化-详细设计.md](B-file-iii-全局filename-rank内存化-详细设计.md)(v1.0,三支柱:全局 filter-invariant rank D-017 / 取数恒 datetime 删 filename_baseline D-020 / date+filename 借 SQL 桶列消对齐风险 D-018 待裁 A/B;施工 4 阶段;回退 D-019 不坏序)。
- 验证(本地非 CI):sort_profile fmt+clippy 绿、dev+release 双跑 exit 0;设计文档 3 增量写入完整无占位。**未**跑 Rust/前端全量(本会话只加只读 bench + 文档,零生产代码改动)。
- 遗留:**B-file-iii 待用户审核后施工**(须显式 go);**D-018 待裁**(date+filename:全面本地桶 vs 暂留 SQL);**B-file-ii(D-013)仍独立待 go**;阶段 7 GUI 真机手感验收仍 pending。

## 会话:2026-07-15(D-018 裁定 A′ + B-file-iii 三阶段施工落地)
- 用户裁(AskUserQuestion):**选 A,开始施工**——但施工前先核实推翻了原前提。
- **决定方向的硬发现(核实后向用户报,反转 D-018)**:追代码坐实 `sort_datetime` = **EXIF 墙钟当 UTC 存**(`metadata.rs:328-333` `parse_exif_datetime` 忽略时区、`Utc.with_ymd_and_hms` 存;`enricher.rs:311` `COALESCE(exif,mtime)` 后不套偏移)。∴ UTC 桶(现 date+datetime/`timestamp_to_date_label`/月桶皆用)**才正确**;SQL date+filename 的 `date(...,'localtime')` 在墙钟上再叠本地时区=**双重偏移 bug**(UTC+8 傍晚照片错分次日)。原方案 A「全面本地桶」会把正确视图挪错、EXIF 照片显示错日 → 反是错向。**用户改选 A′「全面 UTC 桶」**(AskUserQuestion 二次确认):零新列、不改正确视图、顺带修 bug。
- 做了(三阶段,精确 pathspec 逐段提交,本地验证非 CI):
  - **阶段1(`d928271`)** date+filename UTC 内存派生:`items_cache` `can_derive_axis` 放行 date+filename、`derive_order` 加独立 `(div_euclid(86400) 日桶, filename 位次, id)` 分支;`queries.rs::push_order_by` + `enricher.rs::enrichment_order_clause` date+filename 去 `'localtime'` 改 UTC 日界;`layout_commands` `is_filename_derivable` 扩 `none|folder|date`。对拍 `date_filename_derive_matches_sql_order`(跨 3 UTC 日 fixture、双基准、手钉向量)+ 扩两旧对拍 date 分支 + 翻转 `is_hit_valid` date+filename 断言。**根治换轴 thrash(Pain 2「文件夹下切排序 2-3s」)**。
  - **阶段2(`55bd4a4`)** 全局 filter-invariant rank 基建:`GlobalFilenameRank{data_version,id_to_rank:FxHashMap}` + `build_global_filename_rank`(全库 id-only NATURAL_CMP);`AppState` 加 `global_filename_rank` 槽 + `global_rank_building` 标志 + `try_global_filename_ranks`(dv+全 id 覆盖校验)/`spawn_global_filename_rank_build`(幂等、`spawn_blocking`、构建期 dv 变则弃写);`lib.rs` 开机延迟 15s 后台预建。对拍 `global_rank_matches_filename_baseline` + `global_rank_restricts_to_filtered_order`(证 filter-invariance)。
  - **阶段3(`aeed4e4`)** MISS 取数恒 canonical:删 `filename_baseline` 取数分支,cacheable 非 ai 一律 `query_layout_items_canonical`、`CachedOrder` 恒 `Canonical`;新增 `resolve_filename_ranks`(优先全局免 DB、兜底 per-filter id-only 查询+触发后台构建);`ensure_filename_rank_for_hit` 加全局命中快路径、签名改 `&Arc<AppState>`;`GlobalFilenameRank::ranks_for` 提纯方法+单测。**根治 filename 筛选 891ms release/5279ms dev(Pain 1「点筛选 filename 模式>5s」)**。
- 验证(本地非 CI):`cargo test --workspace` 主 lib **522 passed/0 failed/5 ignored**、全 workspace 无失败;`rustfmt --edition 2021 --check` 6 改动文件净;`cargo clippy --workspace --locked -- -D warnings` 绿。**真机 GUI 手感未验(阶段4,须用户环境)**。
- 文档写回(规范红线,同步 A′):详细设计顶部 + §2.3 + §3.3 + §3.4 + §6 阶段3 + §7 D-018 全改写为 A′,修订记录 v2.0;findings F-006 纠正时区方向 + 新增 F-007(搜索谓词 `queries.rs:1422/1455` 亦 localtime、同源不一致,本轮未改)。
- 隔离:全程精确 pathspec;并行前端会话(顶栏 `b8e9090` 等)与本轮后端零重叠;未跟踪 WAL planning 目录非本任务、未动。
- 遗留:**阶段4 真机 GUI 验收**(点筛选/切分组/跨轴切换 filename↔datetime 手感、开机预建无卡顿、重扫重建不坏序)——GUI/CI 不覆盖;**F-007 搜索谓词是否统一 UTC 待用户裁**(改搜索匹配行为);**B-file-ii(D-013)仍独立待 go**。

## 回顾(收口时填)
- 亮点:把“目录身份唯一”“异步只认最新目标”“两个 surface 全局序一致”拆成三个独立不变量，避免因发现新根因而误删旧保护。
- 教训:扁平路径字符串排序不是树的前序 DFS；跨 root 加 tiebreaker 只能保证分组唯一，不能保证 UI 顺序一致。
- 意外:即使只有一个 scan root，标点目录名也能构造 `rel_path` 与 DFS 不一致的严格反例，因此不能只修多 root 主键。
