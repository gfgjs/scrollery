---
status: 快照
type: working-memory
line: 文件树与顶栏格式筛选方案
created: 2026-07-16
---

# 进度日志:文件树与顶栏格式筛选方案

## 会话:2026-07-16(方案初稿)
- 做了:读取项目文档治理规则与 planning skill；核查扫描器、DB 目录树、格式分类/Catalog、顶栏 Priority+、URL codec、MediaFilter/ViewDescriptor 和两处 SQL builder；形成文件树三态与顶栏两级筛选完整方案。
- 验证:执行 `git status --short` 确认现有 EPUB/Markdown 未提交改动；本轮只新增本任务三件套，没有触碰业务代码或现有用户改动。方案事实均以当前源码行号回查。
- 遗留:等待用户确认四项推荐裁决；确认后再建立正式 S 线设计件并进入实施，不提前修改业务代码。

## 会话:2026-07-16(复核 + 实测 + 裁决)
- 做了:
  - 逐条回查方案断言，**就地更正 3 处事实错误**：①`schema.rs:116` `idx_media_format` 已在 `SCHEMA_V1`、存量库全有（原记「无」）；②`queries.rs:1326` 所在是共享 builder `push_where_predicates`（原记「canonical layout 专属」）；③`MediaFilter`/`GalleryFilter` 是下沉关系非两层独立契约（`models.rs:476` 注释自称单一事实源）。
  - **生产库只读实测**（`%APPDATA%/com.scrollery.app/scrollery.db`，251MB，543,449 项）：格式分布 29 个 + facet 四种查法耗时 + `EXPLAIN QUERY PLAN`。
  - 出 8 项详细对比裁决清单（每项含现状证据 / 选项对比表 / 推荐 / 影响批次）。
  - 用户 **采纳全部推荐** → 拍板 D-001~D-008，并把原方案 4 条推荐补编为 D-009~D-012。
  - 按裁决重写三件套：findings 补实测数据段 + 更正段标 ⚠️ + F 候选扩到 F-008；task_plan 重写 §3/§5/§6/§7 + 验收矩阵 + 决策表 12 行 + 错误账 2 行。
- 验证:
  - 实测为只读采样（`sqlite3` `mode=ro`），3 次取最小值，未写生产库；未触碰业务代码。
  - 全角标点经 Write 工具落盘后 `od -c` 验证原样（U+FF1A/U+FF0C/U+3002），无需 `\u` 转义 —— 记忆里那条「工具参数静默转半角」本轮未复现。
  - D 号取号经**来源文件反查**确认（非 grep 全局唯一值推断）。
- 遗留:阶段 5（取 S 号 + 建 `docs/designs/2026-07-16-文件树显示范围与格式筛选.md` + todo.md 加 S 分节）未开工；业务代码零改动。

## 会话:2026-07-16(独立复核修订 + exotic 扩展预留)
- 做了:
  - 复核修改稿与当前源码，发现并修正 6 类契约缺口：FS-only 目录身份、common/exotic parity、内部/外部格式描述符分层、reveal 权限及移动端边界、排序验收口径、facet 基础谓词与 RAW fixture。
  - 用户采纳全部修订，并追问未来增加其它冷门格式的扩展方式；方案确定为**内部 builtin registry + 动态 Catalog snapshot + IPC 投影**，不把当前 67 写成协议常量或测试常量。
  - 更新 D-001/D-002/D-004/D-005/D-007，新增 D-013；新增 F-009~F-011。方案、实施批次和验收矩阵已同步，不只在修订记录追加结论。
- 验证:
  - 生产库以 `sqlite3 mode=ro` 复测：543,449 行，基础谓词后 543,445 行；raw/base distinct 均为 29。`DISTINCT file_format` 最低约 0.014ms，带基础谓词约 0.018ms；`DISTINCT(file_format,media_type)` 中位约 103ms，带 counts 中位约 218ms，原性能决策成立。
  - 源码核对：`classify_media_type("psd") == None` 与 common-first Catalog 回退均有测试/实现证据；当前目录 UI 的路由、active、拖拽和移动/复制确实依赖数值 `DirNode.id`。
  - Tauri 官方文档核对：通用 reveal capability 无预配置 scope；Android/iOS 不支持 `revealItemInDir`。
- 遗留:仍停在阶段 5，待按用户后续指令创建 S 线正式设计件；业务代码零改动。

## 会话:2026-07-16(第三轮核实 + 阶段 5 落地 + 开工)
- 做了:
  - 独立核实其它会话的复核稿（用户要求「无误后开始施工」）：逐条回查源码与生产库，**实质性断言全部为真**。
  - **新发现一处不准并就地更正**：P0 写「Catalog loader 负责小写规范化、重复扩展名、common 冲突和 offering 间冲突校验」像是待建工作，实际 `catalog.rs:217-248` **已全部实现且有测试**（`CommonFormatConflict` `:223-224`/测试 `:426`；`DuplicateFormat` `:245-247`/测试 `:384`；`InvalidFormat` `:217-218`/测试 `:396`）→ P0 复用不新建。且「小写规范化」措辞反了：`is_valid_format`（`:323-328`）限 `[a-z0-9]{1,16}` 是**拒绝**非小写，`:226` `norm_formats.push(f.clone())` 原样克隆，变量名与 `:214` 注释均为误称（失败响优于静默转换，**不改**）。连带：P0 测试项②已由装载期结构性保证，降级为 characterization。
  - 阶段 5 全部落地：S 线设计件 + README 字母表 + todo.md S 分节。
- 验证:
  - 源码核实:`format.rs:122-127` `psd_is_no_longer_common`、`walker.rs:18-19` `classify_media_type(ext).or_else(catalog.media_kind)`、`media.ts:23` `id: number` 强制、`FoldersSection.vue` 数值 id 全链（`:key`/`data-dir-id`/`activeDirectoryId`/`dropId`/`dragId`/`navigateToFolder`/`parentId` 祖先遍历）、`queries.rs:1291-1295` 基础谓词含 `trashed_only` 分支 —— 全部为真。
  - 生产库复测（`mode=ro`）:总 543,449 / 基础谓词后 543,445（**差 4 行全是 `is_deleted=1` 的 jpg**）/ raw distinct == base distinct == 29、raw-only 为空 → **「29 相等是数据巧合」成立**，基础谓词是契约不是装饰。
  - 门禁:`check_docs` 绿；`check_docs_index` **首跑红**（「下一空闲字母」声明 S 未随登记推进）→ 改 S→T 后绿。该门禁按预期抓到了索引不变量。
- 遗留:P0 开工中；P1–P4 未动。

## 会话:2026-07-16(P0 施工收官,三提交)
- 做了:
  - **P0-a**（`b3321fd`）:`format.rs` 三个平行 match → `BUILTIN_FORMATS`(66 项)单一事实源;`RegisteredFormatDef` 带内部处理字段;热路径走 `OnceLock<HashMap>` O(1)(classify 每文件调一次,543k 库)。测试 17 项(net +11)。
  - **doc_subtype 接线**（`3fc96cf`）:用户「顺手做 doc_subtype()」查出它**不是无害死代码**——`doc_commands.rs:846` 把 `get_item_file_format()` 的返回值(file_format)当 doc_subtype 直接写库,变量名 `subtype` 掩盖了它。没炸是因为 `DOC_THUMB_FORMATS=[pdf,epub,svg]` 三者恰好 `doc_subtype(ext)==ext`(**巧合非设计**)。可达性:`store_doc_thumbnail` 的 `x-item-id` 来自请求头且不校验 file_format,拿 `.txt` 项 id 调用即写 `"txt"` 而非 `"text"`。已改为经 `doc_subtype()` 派生;`pipeline.rs` 字面量 `Some("epub")` 亦改派生;`derive/kind.rs` 新建 test mod 钉住「恒等是巧合」。
  - **P0-b**（`7b4c868`）:新建 `formats/` 顶层模块(选址:`utils::format` 引 `exotic::catalog` 成环,且全集语义不属于 exotic);`FormatDescriptor` UI 投影;删 `src/constants/formats.ts`。测试 6 项 + 双 offering fixture。
- 验证:
  - `cargo test --lib` **546/0/5** 绿;rustfmt + `clippy --lib --all-targets -D warnings` 绿;`vue-tsc --noEmit` 绿（本地非 CI）。
  - **变异验证测试本身有效**（experience §19「先测量测量本身」）:①表删 `jpeg` → 4 红含 golden 对拍;②给 `mp4` 错标 `phase1_image` → 2 红。漏项与值错两类都抓得住。
  - 过程踩坑:恢复变异用 `shutil.move` **保留了备份的旧 mtime** → cargo 判定无需重编,跑的是变异残留的旧二进制,磁盘源码明明正确却报 2 红。`touch` 刷新 mtime 后才是真结果。**反向同样成立**:改坏了却因 mtime 没变而报绿。
- 遗留:P1–P4 未开工。新增 F-012~F-014。

## 会话:2026-07-16(P1-a 后端收官,六提交,无人值守)
- 做了(用户「继续推进/无人值守直到需要决策」):
  - `cbc7527` **`resolve_within_root`**:三层防线(逐段拒 `..`/Windows 拒含 `:` 段 → canonicalize 两侧 → `starts_with` 边界断言)。10 测试全走真 FS(tempfile)。
  - `a686dae` **FS 数据源**:隐藏判定(点前缀 OR Windows `FILE_ATTRIBUTE_HIDDEN` OR macOS `UF_HIDDEN`)+ DB 等价比较器 + 惰性枚举 + 链接不跟随。11 测试含与真 SQLite(rusqlite in-memory)对拍两种 collation。
  - `f5ea0e6` **`reveal_tree_entry`**:核实上游源码后发现 `reveal_item_in_dir()` 是**自由函数**(不需 AppHandle)→ **插件完全不注册、capability 一条不授**,JS 侧够不着,ACL 面为零,比设计原定的「注册插件但不授 capability」更紧。错误按变体重写文案(上游 `NoParent(PathBuf)` 的 Display 会打出绝对路径)。
  - `79f911a` **目录快照缓存**:自研有界 LRU(需 `invalidate_root` 前缀失效,用 lru crate 也得自己遍历);**双维度封顶**(8 快照 / 20 万条目)+ 自逐出保底。12 测试。
  - `f3498dd` **`list_tree_entries`**:TreeEntry 双身份分离;**mediaId 谓词与 `list_directory_files` 一致**(软删项/companion 在磁盘上存在但 DB 模式不显示 → 不赋 mediaId,否则「共有项」定义糊掉);新增 D-002 端到端对拍(真 FS + 真 DB)。差点重复造 `find_directory_id`,编译器 E0428 拦下。
  - `c111e1a` 失效接线(扫描完成 `invalidate_root` + 显式刷新 IPC)。
- 验证:
  - `cargo test --lib` **583/0/5**(P0 后 546 → +37);rustfmt + `clippy --lib --all-targets -D warnings` 绿(本地非 CI)。
  - **四处变异验证,抓出 4 个假测试**(详见 F-016)。**这是本批最重要的产出** —— 若不做变异,四个洞都会以「全绿」形态留在仓库里:
    ①NOCASE 期望序手写写反(实现对、测试错);②断言测的是 std 恒真行为而非被测函数;③`Ä/ä` 与 fixture 目录 `Alpha/zulu` **无区分度**(两种实现输出相同);④symlink 守卫可自我跳过,而它是边界断言的**唯一**守卫(`..` 被第 1 层拦、绝对路径被 canonicalize 拦)。
  - 判据修正:折叠判据改用能改**首字节**的 `İ`/`Σ`/`ẞ`(经真 SQLite 实测,三例均 SQLite ≡ ASCII 折叠、≠ Unicode 折叠);fixture 目录改 `Zebra`/`apple`(BINARY 与 NOCASE 序**相反**);链接守卫改 Windows junction(`mklink /J` **无需**开发者模式,CI 亦可)+ 建不出即硬失败。
- 遗留:**P1-b 前端待裁 F-017**(`DirNode`/`DirFile` 改造 vs 新建类型);F-015(`show_in_explorer` 第二套 reveal 实现)待裁;P2–P4 未动。

## 会话:2026-07-16(用户裁 F-017=方案 A、F-015=统一到 opener;四提交)

**用户裁决**:F-017 选 **A(就地放宽既有类型)**;F-015 **统一到 opener**。

- `6e8d1e7` **F-015 reveal 统一**:两套实现合一(`ipc::reveal::reveal_path`)。修 Linux 分支只 `xdg-open` 父目录**不选中**(opener 走 D-Bus `FileManager1.ShowItems`,失败回退 XDG portal `OpenDirectory`)。
  - **顺带的行为变化必须一并处理**:旧 `.spawn()` 发射后不管、打开器起不来也返回 `Ok`;opener 校验路径存在(`absolute_and_check_exists`)且同步等结果 → **「文件已被外部删除」成了常见真实失败**。两个调用点原本都吃不下(`global.ts` 静默 `.catch(() => {})`、`ContentViewer.vue` 裸 `await`)→ 补 error toast + `contextMenu.showInExplorerFailed`(中英)。
  - **先查了调用点才动手**:`show_in_explorer` 有 2 个真实调用点,故 Linux 缺陷**是用户可见的**(曾一度因 Grep 的 cwd 被 Bash `cd` 污染而误判「零调用点」,见 F-018)。
- `9166d3d` **路径身份收敛为唯一实现**:原设想「前端 DB 适配器自己拼 `${rootId}:${relPath}`」**否决** —— 那是跨语言的第二份 key 实现,分歧后果极隐蔽(不报错,只是切模式时同一目录被当成两个节点)。改为 `crate::tree::{node_key, parent_key, child_rel_path}` 唯一产出,DB 模式三个命令 + FS 模式命令全从这里取键,**前端只消费不推导**;`tree_commands` 内联的那份 `node_key` 删除。`list_directory_files` 用独立一次 PK 查找取 `(root_id, rel_path)`,有意不 JOIN 进那条带 `COLLATE NOCASE` + `LIMIT` 的分页热查询。
- `b1b2227` **结构轴换 nodeKey**(DB 模式零行为变化)。身份分离划出两类代码各归其轴;**实体轴(拖拽/移动/复制/路由/滚动锚点)原样不动** —— 那些能力按 §4.1 本就不对 FS-only 开放,身份分离的价值也包括**划出哪些代码根本不需要改**。
- `e94ad70` **放宽 id 为可选 + 能力边界成具名类型谓词**:`vue-tsc` 精确列出 **12 处**,一处不多一处不少。`hasEntityIdentity`/`isOpenableInApp` 写成**类型谓词**而非返回 boolean —— 后者会逼调用方 `node.id!` 断言,等于把类型系统刚给的保护扔掉;谓词收窄后「先判后用」从约定变成编译期强制(本批零 `!` 断言)。

- 验证:`cargo test --lib` **589/0/5**(+6);`vitest` **1033/0**(+14);vue-tsc / ESLint / rustfmt / `clippy -D warnings` 全净(本地非 CI)。prettier 对涉及的 7 个前端文件**在 HEAD 即告警**(既有,非本批引入),按项目规则不做仓库级 format。
- **变异验证共 14 项,抓出 2 个真问题**:
  - Rust 路径身份 4 项全捕获(去 `:` 分隔符 / `child_rel_path` 根下拼前导 `/` / `parent_key` 剥首段 / 扫描根返回 `Some`)。
  - 前端换轴 6 项:5 捕获,**MF4「注入判重退回 id 轴」初版无人捕获(真·假测试)** —— F-016 同款:理由早写在实现注释里,却没有断言接住。**初版补测也没抓到**(判重是拿已在树中的节点建 `existing`,我造的两个撞 id 节点都是新来的,压根不进 `existing`),改用「子节点撞已在树中的父节点 id」的形状才捕获。
  - 能力边界谓词 5 项全捕获,各被**恰好一条**测试抓到(无冗余无空档)。
- **补了既有缺口**:`collapseNode`/`collapseAll`/`getDescendantKeys`/`toggleNode` 此前**零测试覆盖**,而本批改的正是它们 —— 项目规则要求改未测关键行为前先补网,补 6 条(characterization 3 + 轴契约 3)。
- **轴契约测试的构造要点**(值得复用):DB 模式下 id 与 nodeKey 一一对应,**任何只用正常 DB 节点的测试都区分不出两条轴**(改回 id 轴照样全绿)。必须专门造出二者分歧的形状:`parentId` 为 null 而 `parentKey` 有值(FS-only 目录父链形态)、子节点与已在树中的父节点实体 id 相同(FS-only 子树 id 全为空值的退化同构)。
- 文档更正:设计 §4.2 原写「Rust 侧**注册** `tauri-plugin-opener`…内部调用 `OpenerExt`」,已被 P1-a 实测推翻(自由函数 → 插件完全不注册),**正文就地更正**(非脚注),§7 P1 行同步。
- 遗留:**P1-b-3**(FS 适配器 + 三态显示菜单 + 本机持久化 + 双击 reveal 接线)未动;P2–P4 未动。新增 F-018/F-019。

## 会话:2026-07-16(施工全量审查 + 真机三问题定性;零代码改动)

> 注:P1-b 收官、P2、P3/P4 的逐批交付记录回填在 todo.md S 节与设计件 §9(`9bb3a6f`/`378cc4a`/`13a3192`),本三件套 progress 未逐会话回填——接续读 todo.md S 节即可。

- 做了:
  - 对 S 线全部施工提交(`b3321fd`~`73c44bb`,21 个代码提交)做契约审查:4 个并行域(P0+P3 后端 / P1-a 后端 / P1-b 前端 / P2+P3 前端)逐条核验设计件 §3–§8 + 测试有效性抽查;🔴 与决定性 🟠 主线程独立复核源码确认。
  - 真机三问题根因追踪(与域审查交叉印证):①隐藏项显示混淆 = **真 bug**(`FoldersSection.vue:120-127` 五处实体轴判等 `null === null` 恒真 + adapter 丢弃 `hidden` 字段);②隐藏目录下 .md 不能应用内打开 = **设计有意行为**(D-001/§4.4,机制链核验完好),给出 A(affordance)/B(path-based 打开,需立项)/C(否决)三案;③拖拽 = 文件行**从未实现**(`0c54d03` 起仅目录行可拖)+ 默认模式目录拖拽链核验完好 + FS 模式被 adapter `parentId` 恒 null **误伤**(连库内目录也禁拖,超出 §4.1 边界)。
  - 产出**审查快照** `docs/reviews/2026-07-16-S线文件树与格式筛选施工审查.md`:2 🔴(R-01 判等恒真 / R-02 writeUrl watch 漏 `fileFormats`)+ 5 🟠(R-03 FS 模式根行沿用 DB 轴致 FS-only 子目录整层丢失 / R-04 loadChildren 无代际守卫切模式竞态 / R-05 应用内 8 个物理写点不失效树快照 / R-06 hidden 断链 / R-07 FS 模式禁拖误伤)+ 14 🟡 + 修复批次建议。
  - findings 登记 F-020~F-023(判等漏网/枚举点复发/旧轴字段语义漂移/死守卫复活)。
- 验证:
  - R-01/R-02/R-03/R-04 均主线程重读源码逐行确认(watch 源列表、adapter 映射、loadRoots/toggleNode 判据、拖拽入口守卫),非转述 agent 结论;问题 3 的「默认模式完好」核到后端 `get_directory_children` 下发 `parent_id`。
  - 正面确认一条:既有 `shell:allow-open` 上游默认 scope 只放 URL,未重开 D-001 执行面。
  - 本轮**零代码改动**(用户指令「报告和解决方案落到文档」);修复批次待用户 go。
- 遗留:修复批 1~3(见审查快照 §5)待裁决排期;问题 2 方案 B 与「树内文件行拖拽」若要做须单独立项;P4 真机验收照旧。
- 库:543,449 项 / 29 个格式 / **当前**注册表快照 67 个（内置 66 + exotic PSD；不是协议常量）。
- **jpg 515,728 与 jpeg 2,530 并存** → 别名归一是实证问题（D-003）。
- **RAW + heic/heif/avif 全 0** → RAW 组按「只列存在的」规则隐身（D-004）。
- 文档 4,818（txt 4,615 为主）/ 音频 32 / psd 62。
- facet:`DISTINCT file_format` **0.0ms**（COVERING INDEX 跳扫）｜+ 基础谓词 **0.0ms**｜`DISTINCT (media_type,file_format)` **114.3ms**｜`GROUP BY + COUNT(*)` **216.0ms**（TEMP B-TREE）。

## 会话:2026-07-16(修复批 1~3 施工收官;四提交)

- 做了:
  - **开工前逐项核实**(用户提示报告后经历大规模重构):T 线(queries.rs 拆 14 个 DAO)+ U 线(ipc/exotic/thumbnail 拆分)落地后,后端行号全漂移(R-14 从 queries.rs:1424 迁至 `db/queries/layout.rs:363`、R-05 写点在 file_ops_commands 九命令、R-12 在 scan_commands),但 **21 项发现全部仍在**,零项已被重构顺带修掉。
  - **批1**(`2188bff`,真机问题①③根治+两🔴):R-01 五处判等收敛 `sameEntityId`(null 恒不等)+spec;R-02 watch 补 `fileFormats` + 新建 `useGalleryQuerySync.spec.ts` wiring 级对拍(`Record<keyof GalleryFilterSnapshot>` 类型完备门 + 逐字段变更→URL 必变);R-06 hidden 端到端透传+文字/图标 opacity 0.55(不动背景,与选中/拖拽通道正交);问题②方案A(文件行 `.selected` 可见选中态——「单击没反应」的根因是 `.kb-active` 被 `:focus-visible` 门在鼠标路径恒不显示;未入库 tooltip 明示;移动端不发起 reveal);R-07a 后端 `TreeEntry.parentDirectoryId` = 被列目录库行 id(`find_directory_id` 结果复用,零新增查询),adapter 透传恢复 FS 模式库内目录拖拽。
  - **批2**(`973709c`,数据正确性):R-03 loadRoots 按模式把根行 `hasChildren`/`mediaCount` 归一 null(F-022);R-04 loadChildren 代际守卫回归(F-023)+ loadRoots 换代清在途去重表 + finally 只删自己注册项;R-05 `InvalidateOnWrite::drop` 扩展清树快照(覆盖批量命令含中途失败路径)+ 五个无守卫命令显式清(**尝试即清不等成功**——部分落盘也失真);顺并 R-11(设计 §4.2 回填 kind 入参)与 R-12(remove_scan_root×2/clear_database);设计 §4.2 失效触发点清单补第 4 条「本应用物理写」。
  - **批3**(`13b727d`,🟡清偿):R-08 `AppError::Reveal{code,message}`(Exotic 同姿态),测试改钉序列化后 code 字段;R-09 三消费点(FoldersSection/global.showInExplorer/ContentViewer)按稳定码分流+移动端动作不出(`isMobilePlatform`);R-10 展开失败 collapseNode 回退+`onLoadError` 上报口→FoldersSection toast;R-14 rustdoc 归位;R-17 格式 chip 挪至收藏后(描述符+模板 DOM 序同步,spec 钉前七项);R-18 declare(tree/mod.rs 注释 + 本 task_plan §二划线更正);R-19 harness 三 fixture(四形态条目)。
  - **破窗**(`d293b53`):T 线遗留 `derivations.rs` 测试模块后置项致 clippy `--all-targets -D warnings` 红(既有,非本批引入),测试模块移文件尾。
- 验证:
  - 门禁:vitest 全量 **1146/1146**(新增 23:sameEntityId 5 / wiring 10 / R-03·04·10 各 2-3 / adapter·descriptors 补强)、vue-tsc / ESLint 全库净、`cargo test --lib` **608/0/5**、rustfmt + clippy `--all-targets -D warnings` 净、check_docs + check_docs_index 双绿(本地非 CI)。
  - **变异验证四处**(experience §19):R-02 撤 watch 源 → 2 红;R-03 关归一 → 2 红;R-04 撤守卫 → 恰 1 红;全部恢复后全绿。
  - 未吸收无关工作:工作树中他人 `docs/planning/2026-07-16-官网重设计/` 未入任何提交。
- 遗留:P4 真机验收(todo S 节已列修复批新增验收项);问题②方案 B 与「树内文件行拖拽」待单独立项;R-13/R-15/R-16/R-20/R-21 记录不动。

## 回顾(收口时填)
- 亮点:待收口补充。
- 教训:待收口补充。
- 意外:待收口补充。
