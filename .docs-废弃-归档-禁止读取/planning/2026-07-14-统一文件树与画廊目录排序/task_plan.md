---
status: active
type: working-memory
line: 统一文件树与画廊目录排序
created: 2026-07-14
---

# 任务计划:统一文件树与画廊目录排序

## 目标
建立唯一、稳定且适合大库的 canonical directory order，使画廊 `folder` 分组的目录序严格等于左侧文件树前序 DFS 中“当前视图有媒体目录”的诱导子序，从根本上消除向前拖动画廊滑块时文件树跨 scan root 或跨子树往返跳动。

## 当前阶段

> 🔴 **2026-07-14 用户裁决（覆盖原 B 单方案）**：走「**方案 A 先行、方案 B 列后续**」。
> 下方「## 阶段」已按方案 A 重排；原持久列方案 B 降级为「## 后续:方案 B（持久列性能优化）」。
>
> ✅ **方案 A 代码全落地并本地验证通过**（阶段 1–6 done）：`cargo test` 501 passed / 0 failed、
> `cargo fmt --check`、`cargo clippy --lib`、`docs 门禁` 全绿（**本地非 CI**）。**仅剩阶段 7 真机大库验收**
> （CI 不能覆盖 GUI）。
>
> ✅ **方案 B-dir（目录序持久列 V19）已施工落地并本地验证**（2026-07-14，用户显式 go）：三提交
> `7d97b01`（迁移地基）/ `d649414`（写路径）/ `f4a85c7`（消费者切列），全量 lib **515 passed / 0 failed
> / 5 ignored** + fmt + clippy + EXPLAIN（计划无回归、filesort 仍在）全绿（**本地非 CI**）。施工前验证
> 修正 3 处设计漂移（`scrollery_lib` 导入名 / enricher 无需 JOIN / functions feature 已启用）。**遗留 =
> 阶段 7 真机 40 万库 profile**（§2.4 决策门：证明常数因子收益值得）。**B-file 家族仍勿自动开工**（先测
> filename 使用频率）。详见 [方案B 详细设计](方案B-持久化排序键列-详细设计.md) 与 progress.md 末节。

**方案 A（本轮施工）= 注册 SQLite 标量函数 + 内存即时键，零迁移、零 schema 列。**
评审时核对代码发现:canonical 内存 `derive_order` 与 SQL `push_order_by` 之间存在**刚性等价契约**
（`canonical_derive_order_matches_sql_order` 对拍锁定,防 SelectAll 选区漂移）。因此方案 A **不能**只改
`build_dir_rank`,必须让内存侧与 SQL 侧用**同一份 DFS 键逻辑**。做法:把 `encode_tree_sort_key(rel_path)`
注册为 SQLite 标量函数 `TREE_SORT_KEY`,SQL 的 folder 分支 `ORDER BY TREE_SORT_KEY(d.rel_path)`,内存
`build_dir_rank` 调同一 Rust 函数。因 `push_order_by` 三个 folder 分支都用上该键,方案 A **一并修复了
datetime / filename / similarity 全部 folder 排序模式**（不止最初报告的 datetime 滑块跳动）。

**方案 B（后续,勿自动开工）= 持久化 `tree_sort_key BLOB` 列**,把 SQL 侧逐行 `TREE_SORT_KEY(rel_path)`
函数调用换成索引列比较,仅优化 filename/similarity-folder 这类**较少用**的 SQL 排序路径性能。与 A 干净衔接
（替换 ORDER BY 键即可）。触发条件:实测该类模式在大库下排序过慢时再上。

## 范围边界

### 本任务包含
- 统一 scan root 顺序、文件树同级目录顺序、画廊 folder 分组顺序。
- 保持每个真实 `directory_id` 的媒体连续成组。
- 保持 SQL 排序、内存 `dir_rank` 派生序与文件树懒加载序逐项等价。
- 覆盖数据库迁移、扫描新增目录、目录移动/重命名后的排序键维护。
- 保留并复核现有 `directory_id` 定位链与 frontend latest-wins 异步队列。

### 本任务不包含
- 不要求文件树的文件叶子顺序等于画廊组内媒体顺序。文件树可继续固定文件管理器顺序；画廊组内仍由 `datetime / filename / similarity + asc / desc` 控制。
- 不改变画廊 separator 的 `groupId = directory_id` 契约。
- 不重写文件树虚拟化、sticky header、懒加载或画廊布局算法。
- 本方案会话不修改任何源码；所有代码施工留给新会话。

## Canonical directory order 契约

### 1. Scan root 顺序

```text
RootOrder = (scan_roots.created_at ASC, scan_roots.id ASC)
```

- `created_at` 保留当前产品行为。
- `id` 是必须的稳定 tiebreaker，解决同一秒添加多个根时顺序未定义的问题。

### 2. Root 内目录顺序

目录顺序定义为文件树的前序 DFS：父目录先于全部后代；一个目录的完整子树先于其后续同级目录。

新增持久化列：

```sql
directories.tree_sort_key BLOB NOT NULL DEFAULT X''
```

`tree_sort_key` 由已规范化为 `/` 分隔的 `rel_path` 生成：每个 UTF-8 path segment 后追加一个 `0x00` 终止字节，再把全部 segment 串接为 BLOB。根目录空路径的 key 为 `X''`。

示例：

```text
rel_path  tree_sort_key（十六进制）
A         41 00
A/Z       41 00 5A 00
A-        41 2D 00
```

SQLite BLOB 字节序得到 `A < A/Z < A-`，与前序 DFS 一致；不会再出现原始字符串排序中 `A < A- < A/Z` 的割裂。

完整目录键：

```text
DirectoryOrder = (
  root.created_at ASC,
  root.id ASC,
  directory.tree_sort_key ASC,
  directory.id ASC
)
```

### 3. 画廊 folder 分组顺序

画廊只展示当前过滤条件下含媒体的目录，因此它的目录序应是完整文件树 DirectoryOrder 过滤掉无媒体目录后的诱导子序：

```text
GalleryDirectoryOrder = filter_has_media(FullTreeDirectoryOrder)
```

组内媒体排序继续位于目录键之后；切换组内升降序不能改变目录组顺序。

### 4. 唯一身份与异步一致性

- separator 继续使用唯一 `directory_id`，不使用 basename、display path 或数组位置作为身份。
- `createFolderTreeSyncQueue` 继续采用“一个在途 + 一个最新候选”的 latest-wins 语义。
- 排序统一只解决目标序列在树中的往返；latest-wins 继续解决深层祖先异步加载的旧请求晚到问题，两者不可互相替代。

## 推荐实现

> 🔴 **已被上方裁决覆盖**：本节原主张「一步到位持久化 BLOB」。现拆为 A（即时键，本轮）+ B（持久列，后续）。
> 下列替代方案分析仍然有效,但**其中一条对方案 A 是误判,已更正**（见下）。

编码方案 `segment + 0x00 终止字节的 BLOB 键` 不变(数学可证,见「## Canonical directory order 契约」)。落地形态分两步:

- **不继续使用原始 `rel_path`**：它无法严格表达 path segment 的 DFS 顺序（正确,A/B 都不用）。
- **不持久化连续整数 DFS rank**：目录插入、移动或重命名会迫使大量目录重新编号（正确,A/B 都不用）。
- **不只使用 `(root_id, rel_path)`**：不能解决单 root 下 `A / A- / A/Z` 多级边界（正确,已在代码验证 `build_dir_rank` 现按 rel_path BINARY 排会得 `A < A- < A/Z`）。
- ⚠️ **~~不在 40 万媒体排序比较器中反复 `split('/')`~~ —— 对方案 A 是误判,已更正**：该顾虑只对
  **per-media**（百万级）比较器成立;而方案 A 的即时键只在 **`build_dir_rank` 构建目录 rank 时按目录数（D≈10^4）算一次**,
  媒体排序仍用压好的 `u32` rank。D 级即时算键 <10ms,完全免税。持久列（方案 B）的真正收益只落在
  **SQL 侧** filename/similarity-folder 排序（SQL 无法调 Rust,只能落列或注册函数逐行算）。

方案 A 落地:`encode_tree_sort_key` 既在 `build_dir_rank`(Rust 内存)调用,又经 `TREE_SORT_KEY` 标量函数在
`push_order_by`(SQL)调用,一份逻辑两处消费,天然满足刚性等价契约。SQLite BLOB memcmp = Rust `Vec<u8>::cmp`,
两侧同构。

## 阶段（方案 A）

### 阶段 1:锁定排序契约(A/B 共用)
- [ ] 在 `src-tauri/src/utils/path.rs` 增加纯函数 `encode_tree_sort_key(rel_path) -> Vec<u8>`，明确输入须为 `normalize_db_path` 后的路径;每段 UTF-8 后追加 `0x00` 终止字节。
- [x] 纯函数单测：空路径(→`[]`)、单段、多级、祖先优先、`A / A- / A/Z`、大小写(BINARY)、Unicode、双斜杠/空段防御。
- **状态:** done(8 单测全绿)

### 阶段 2:注册 SQLite 标量函数(A 核心)
- [x] `db/mod.rs::register_custom_collations` 中用 `create_scalar_function` 注册 `TREE_SORT_KEY(rel_path)->BLOB`(委托 `encode_tree_sort_key`,`SQLITE_DETERMINISTIC`)。写连接与读池连接同一入口注册,全连接可用。
- [x] `Cargo.toml`:rusqlite 加 `functions` feature(启用 `create_scalar_function`)。
- **状态:** done

### 阶段 3:内存路径改 DFS 键(A)
- [ ] `DirLabel` 增 `root_created_at / root_id`(**不加 tree_sort_key,那是方案 B**);`query_dir_labels` SELECT 补 `r.created_at, r.id`(scan_roots 已 JOIN)。
- [x] `items_cache::build_dir_rank` 比较键改为 `(root_created_at, root_id, encode_tree_sort_key(rel_path), dir_id)`;保留紧凑 `u32` rank 与目录连续分组。同步 `dl()` 测试助手与 `justified.rs` 基准里的 `DirLabel` 字面量。
- **状态:** done

### 阶段 4:SQL 路径改 DFS 键 + 统一(A)
- [ ] `push_order_by` 三个 folder 分支的目录序前缀改为 `r.created_at ASC, r.id ASC, TREE_SORT_KEY(d.rel_path) ASC, d.id ASC`,后接原组内键;尾部统一 `, m.id {order_dir}` 机制不动。
- [ ] `list_scan_roots` 改 `ORDER BY created_at ASC, id ASC`(与画廊 root 序对齐,防文件树/画廊 root 顺序分歧)。
- [ ] 更新对拍测试 `canonical_derive_order_matches_sql_order` 期望向量为 DFS 序(desc `[7,1,2,5,3,6,4]` / asc `[1,7,2,3,5,6,4]`);该更新本身即多根交错修复的实证。
- [x] `scanner/enricher.rs::enrichment_order_clause`:folder 仍走 rel_path(**背景富化序,不在刚性契约内**),仅订正其"与画廊一致"的过度声称注释;精确 DFS 对齐留给方案 B。
- **状态:** done

### 阶段 5:保留前一轮有效修复(A/B 共用)
- [ ] 保留 separator `groupId = directory_id`、`scrolledDirectoryId` 与文件树按 ID 查行的定位链。
- [ ] 保留 `createFolderTreeSyncQueue`、单一权威 target watcher、展开后与写 `scrollTop` 前的 freshness 校验。
- [x] 现有"同 rel_path 由 id 裁决/媒体连续成组"不变量测试保持通过(方案 A 只强化不削弱)。
- **状态:** done(全量 501 passed 内含这些不变量测试)

### 阶段 6:轻量优先验证(A)
- [ ] 定向 Rust:`encode_tree_sort_key` 单测、items_cache folder 序测试、`canonical_derive_order_matches_sql_order`、`folder_filename_sort_keeps...`、migration 测试(不应受影响)。
- [ ] 相同失败连续两次即停止,重读源码与错误全文,不盲跑。
- [ ] 定向绿后运行一次 `cargo fmt --all -- --check`;docs 门禁一次。
- [x] 未改 TS/Vue,不跑全量 frontend;`folderTree.helpers.spec.ts` 未运行(本任务零 frontend 改动,latest-wins 逻辑未触碰)。
- **状态:** done(test 501/0/5ignored + fmt + clippy --lib + docs 门禁,本地非 CI)

### 阶段 7:真实大库验收与收口(A)
- [ ] 用户 40 万图库 Tauri 应用真实验收(非 browser hosting):向前慢拖/快拖滑块,文件树目标沿树顺序向后,不跨 root/子树往返。
- [ ] 覆盖两 scan root 同 rel_path、同名目录/文件、多级目录、空父目录;记录 separator `groupId` 与文件树 active `directory_id` 始终相等。
- [ ] 验收通过后更新三件套、归档 `docs/worklogs/`(closeout 处置 F-001..F-004)、提交。
- **状态:** pending(⬅ **方案 A 唯一剩余项**;CI 不能覆盖 GUI,须用户真机大库跑)

## 后续:方案 B 家族(排序键预计算,勿自动开工)

> 📄 **完整详细设计见 [方案B-持久化排序键列-详细设计.md](方案B-持久化排序键列-详细设计.md)(v1.1;本节仅索引,以该文件为准)。**
> **v1.1 家族化**:方案 B = 「预计算键 / 内存基准序替代每查询重排」的家族 —— **B-dir**(目录序持久列,原 v1.0,下方要点速览)
> + **B-file**(媒体项 filename 序,v1.1 新增:**B-file-i** 运行时内存 filename 基准序 + 整数 filename_rank 推荐先行 /
> **B-file-ii** 持久 filename 键列后手;详见设计 §0、Part II、D-011..D-014)。三成员都不消除分组叠加时的跨表 filesort,
> 都受「先 profile / 先测 filename 使用频率」决策门约束。
> 触发条件:实测 filename/similarity-folder 排序在大库下过慢**且 profile 证明** `TREE_SORT_KEY` 逐行 FFI 是可观成本
>(详细设计 §2 诚实边界:B 是常数因子优化、**不消除 filesort**,先测再做)。与方案 A 干净衔接——把 SQL 侧
> `TREE_SORT_KEY(d.rel_path)` 换成 `d.tree_sort_key` 列,内存路径不动。

**B-dir**(目录序持久列)要点速览 —— ✅ **已施工落地本地验证(2026-07-14)**,逐项实现见提交
`7d97b01`/`d649414`/`f4a85c7`(展开与代码级细节见详细设计 Part I):
- [x] `SCHEMA_V19`:`directories` 增 `tree_sort_key BLOB NOT NULL DEFAULT X''` + 回填 `UPDATE ... = TREE_SORT_KEY(rel_path)`;`CURRENT_VERSION`→19。**(提交1)**
- [x] **回填用已注册标量函数 + `run_migrations` 顶部自注册**(纯 SQL 建不出 NUL BLOB;自注册一行修好全部裸连接迁移测试,优于扩 runner 支持 Rust 闭包)。迁移后不变量:`rel_path!='' ⟹ key!=X''`(测试 `v19_backfills`)。**(提交1)**
- [x] 写路径**生产仅 2 处**(均 funnel through)+ dev 1 处,Rust 算键绑 blob:`upsert_directory`(覆盖 `scan_commands`/`fast_scan` 全部扫描)、`move_directory`(唯一改 rel_path 入口)、`bin/mock_data.rs`(fixture,`use scrollery_lib::` **非** `scrollery::`)。**(提交2)**
- [x] 消费者切列:`push_order_by` 三 folder 分支 + `enricher` 改读 `d.tree_sort_key`(enricher 顺带由「近似」升「**根内**精确 DFS」——per-root 查询无需 scan_roots JOIN,修正设计 worst-case 假设);`build_dir_rank` **不动**(内存算键免税)。**(提交3)**
- [x] 测试面(**隐藏成本**):裸 INSERT fixture 取 `DEFAULT X''` 会破 folder-SQL 排序断言 → 全仓仅 **2 处** folder 断言测试(canonical 对拍 + folder_filename_sort)插完补 `UPDATE ... TREE_SORT_KEY(rel_path)`;加**列驱动契约测试** `folder_sql_order_driven_by_stored_tree_sort_key_column`(反写键证明读存列非函数)。**(提交3)**
- [x] 索引:**refine——不预先加**(EXPLAIN 证实多表 ORDER BY filesort 不可消解,`USE TEMP B-TREE FOR ORDER BY` 切列前后都在,单表键索引不被用)。
- [x] ⚠️ `DEFAULT X''` 把"忘写 key"从编译错误降级为静默错序;靠写路径集中(生产 2 处)+ 不变量测试 + 列驱动契约测试兜底。
- [ ] **(未做,正交,按需)** `parent_id/depth` 一致性修复与本序目标正交,**拆独立提交**,勿混入本序目标。
- [ ] **(遗留)** 阶段 7 真机 40 万库 profile filename/similarity-folder 切列前后时延(§2.4 决策门;CI/GUI 不覆盖)。

## 新增待审:B-file-iii(全局 filename rank 内存化,勿自动开工)

> 📄 **完整详细设计见 [B-file-iii-全局filename-rank内存化-详细设计.md](B-file-iii-全局filename-rank内存化-详细设计.md)(v1.0;本节仅索引)。须用户显式 go 后施工。**

**触发**:用户 2026-07-14 真机报「点筛选/切分组等待过长」(dev>5s/release>1s)。`sort_profile` 决定性隔离段(提交 `b203262`)坐实根因 = `NATURAL_CMP` collation 的 SQLite→Rust FFI(占 filename MISS 93% dev/65% release);详见 findings「filename 排序卡顿根因确认」+ 方案B §14.3.2。

**方案要旨**(三支柱):①全局 filter-invariant 内存 `filename_rank`(全库一份、开机后台建一次,filename 序是全序 → 服务所有筛选子集,D-017);②取数基准恒 datetime(canonical 无 collation)、删 `filename_baseline` MISS 分支 → 单槽缓存对任意 group×sort 皆 HIT、thrash 根治(D-020);③date+filename 借「SQL 把本地日期桶作为一列带回」内存派生,消灭 UTC/本地对齐风险(D-018,**待用户裁** A 全面本地桶 / B 暂留 SQL)。

**收益**:filename 轴点筛选/切分组从 release ~900ms / dev ~5s 降到 ~16–27ms 内存派生;一次性全局 rank 构建 546ms(release)/4651ms(dev)藏开机后台。**风险低**(复用现成 `natural_cmp`、零迁移、无字节键编码)。

**施工阶段**(详见设计 §6):阶段1 全局 rank 结构+构建+生命周期 / 阶段2 取数恒 datetime + derive_order 读全局 rank / 阶段3(gated on D-018)date+filename 派生 / 阶段4 真机验收。

**与 B-file-ii 关系**:iii 先行不挡 ii;ii 仍独立待 go(D-013),仅作 iii 之上的可选加速(把一次性构建从 NATURAL_CMP 546ms 改读字节键 BINARY 201ms)。

## 建议施工提交(方案 A)

1. `feat(db): 增加目录前序 DFS 排序键与 TREE_SORT_KEY 标量函数`(path.rs 纯函数 + 单测 + db/mod.rs 注册)
2. `fix(gallery): 内存与 SQL folder 序统一为前序 DFS`(DirLabel/query_dir_labels/build_dir_rank/push_order_by/list_scan_roots + 对拍期望更新)
3. `docs: 归档目录排序统一(方案 A)施工记录`

每个提交只暂存本任务路径;当前工作区存在并行 frontend 改动,须用精确 pathspec,不得混入。

## 验收标准

- 同一 fixture 中，画廊 unique directory ID 序列严格等于文件树 DFS 过滤无媒体目录后的序列。
- 多 scan root 不再按相同 `rel_path` 交错。
- 父目录始终早于全部后代，完整子树始终早于后续同级目录。
- 同路径、同目录名、同文件名仍按真实 `directory_id` 连续分组。
- 组内 `asc / desc` 只反转媒体次键，不改变目录组序。
- (方案 A)目录移动/重命名后 key 自动正确:键由 rel_path 即时算,`move_directory` 改完 rel_path 即无陈旧 key 之虞。(方案 B 才需显式维护持久 key。)
- latest-wins 回归测试保持通过，真实大库拖动不再出现反向往返。

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 以文件树前序 DFS 作为唯一 canonical directory order | 树结构要求子树连续，画廊可以无损采用其诱导子序；反向让树服从扁平字符串序不可行 | D-001 |
| segment + `0x00` 编码的 DFS BLOB 键 | 精确表达 DFS，避免连续整数 rank 的全局重排；SQLite memcmp = Rust `Vec<u8>::cmp` 两侧同构 | D-002 |
| 保留前一轮唯一目录分组与 latest-wins | 它们分别修复跨目录媒体交错和异步旧请求晚到，与全局顺序割裂是三个独立层次 | D-003 |
| 验证采用定向优先、全量一次 | 满足 stop-loss，减少大库任务中无信息增益的重复重型测试 | D-004 |
| **A 先行(即时键+标量函数)、B 后续(持久列)** | 刚性等价契约强制内存/SQL 同键;即时键在 D≈10^4 目录级算,免税;持久列仅优化 SQL filename/similarity-folder,须 Rust 回填,故降级为按需 follow-up | D-005 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 暂无施工错误 | 本会话只完成只读审计与方案落盘 | 新会话施工时追加 |
