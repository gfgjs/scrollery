---
status: 快照
type: working-memory
line: 后端大文件拆分审查
created: 2026-07-16
---

# 发现与决策:后端大文件拆分审查

## 需求

- 用户要求审查项目中「单文件巨大行数」的后端代码,给出合理拆分方案并落盘。
- 既有 T 线 queries 拆分方案(docs/planning/2026-07-16-queries模块拆分方案 + designs 设计件)可作参考,但 S 线新功能落地后部分内容已不可信,须核实。
- 本任务只产出文档,不实施 Rust 重构(与 T 线同款约束)。

## 发现

### 行数普查(2026-07-16 HEAD=13a3192,git ls-files + wc -l)

≥1000 行(深测绘,10 文件 + queries.rs):

| 文件 | 行数 | 2026-07-01 起 churn |
|---|---|---|
| src-tauri/src/db/queries.rs | 11,228 | 59+ commits(T 线已有方案) |
| src-tauri/src/exotic/worker.rs | 1,397 | 7 commits +726 −63 |
| src-tauri/src/exotic/pipeline.rs | 1,239 | 7 commits +157 −42 |
| src-tauri/src/ai/worker_client.rs | 1,203 | 6 commits +1,235 −32(近乎全新) |
| src-tauri/src/ipc/ai_commands.rs | 1,191 | 13 commits +538 −447 |
| src-tauri/src/ipc/thumbnail_commands.rs | 1,118 | 14 commits +754 −749 |
| src-tauri/src/layout/items_cache.rs | 1,083 | 16 commits +1,260 −177(B-file-iii 主场) |
| src-tauri/src/ai/face_pipeline.rs | 1,083 | 11 commits +831 −552 |
| src-tauri/src/layout/justified.rs | 1,063 | 11 commits +604 −240 |
| src-tauri/src/lib.rs | 1,062 | **42 commits** +363 −62(频率之王,小 diff 常改) |
| src-tauri/src/exotic/supervisor.rs | 1,010 | 6 commits +676 −11 |

900–1000 行(轻裁决,5 文件):models.rs 995(10c)、installer.rs 931(7c)、file_ops_commands.rs 928(7c)、fast_scan.rs 914(4c)、doc_commands.rs 912(15c)。

<900 不进本轮:state.rs 868、face_commands.rs 856、crates/scrollery-ai-core/clip.rs 848、layout/horizontal.rs 883、layout/cache.rs 848 等。

### T 线 queries 方案时效性核查(核心委托任务)

当前口径(HEAD,同 T 线测量方法):**11,228 行 / 503,724 bytes / 214 pub fn / 30 私有 fn / 18 公开类型 / 138 `#[test]` / 22 test mod / 41 消费文件**。对比 T 设计二次 Review 快照(f3498dd 后):10,885 行 / 213 pub fn / 29 私有 fn / 125 测试 / 21 test mod / 41 消费文件。

`f3498dd..HEAD` 涉 queries.rs 共 4 提交(9166d3d / 549861c / 2afe1e1 / 73c44bb),+370 −27。符号级差异**仅 3 个新增声明**:

1. `pub fn list_library_formats`(queries.rs:400)——S/P3-a 格式 facet 取数(`DISTINCT file_format` + 基础谓词);文档注释明言基础谓词与 `push_where_predicates` 一致是**契约不是装饰**。
2. `fn push_in_predicate`(queries.rs:1439,私有)——IN 谓词参数序号算术抽共;**被画廊 `push_where_predicates` 与搜索 `search_media` 两处消费**。
3. `mod format_facet_tests`(queries.rs:11143)+ 2 条 builder 测试(9635/9645)= +13 测试。

**时效性结论**:
- T 设计的抗漂移机制(D-008:P0 重建 manifest)按设计吸收了行数/测试数漂移——这些数字失真**不构成方案失效**。
- **真正失效点 = 「search.rs 是叶子」前提被推翻**(T findings 首轮复核曾记「`search_media` 内联 mapper 自包含,search.rs 确为叶子」)。`push_in_predicate` 现为 layout↔search 跨域共享 helper;而 T 设计 §6 让 search.rs 在 P1 先迁、layout.rs 在 P4 最后迁——消费者先于 owner 迁移,与 `reset_error_items_batched`(owner ai.rs 先迁、faces 后迁)恰好相反。T-D-012 已裁方案 a:P1 期 search 以 `super::push_in_predicate` 引用暂留 facade 的 helper,P4 layout 迁移 commit 内一并改 `super::layout::push_in_predicate`并登记该改点。
- 新符号归属(按 §4.3 判据「谁的不变量被保护」):`list_library_formats` → `layout.rs`(被保护不变量=facet 基础谓词与画廊可见性谓词一致);`push_in_predicate` → `layout.rs`(不变量=占位符编号与 extras 推入顺序对齐,错位静默筛错),search 侧定向引用、可见性 `pub(in crate::db::queries)`。
- **T-P0 硬门(D-010)状态**:S 线 P3 四提交 + P4 八门全绿已 landed(todo.md S 节),仅余真机验收(GUI);queries.rs 的 S 线代码写入已尽。残余风险=真机验收若发现回归可能再改 queries.rs,须按设计既有条款「插队则停止并重建基线」处理。
- 行号快照整体后移:三跨域符号(二次核实时 510/4811/3370)与 `'localtime'` 债(1504/1537)行号已再漂移;契约以 symbol 定位不受影响。

### 结构测绘 · exotic 组(agent 实测)

- **worker.rs 1,397**:测试占 ~49%(689 行),生产仅 ~708。三个 bounded context:①进程创建/低优先级(26–147,自包含);②`WorkerConn` 协议状态机(173–409,**必须整体**——共享 request_id/silence-timeout 不变量);③**4 个纯输出校验函数 + outcome 枚举(411–724,不被 WorkerConn 调用〔缩略图那个除外〕,主要消费者是 `ai::worker_client`/`ai/face_pipeline`/`bin/worker_e2e`)**。外部面最广(7 文件),校验器+枚举是被 import 最多的符号。天然缝=校验器块可独立成模块,顺带理顺 AI 层从 exotic::worker 拿纯函数的层次味道。
- **pipeline.rs 1,239**:测试 ~35%。唯一松耦合区=trait 分类学(62–174:`WorkerTask`/`ThumbnailWorker`/`EmbedWorker`/两 Factory;其中 `EmbedWorker` 只服务 ai 层,本文件流水线用不到)。其余 5 个 loop fn + 私有结构 + 常量是**单一状态机**(两条 bounded channel + 共享 Arc 原子/互斥,scope 内定义),不可拆。
- **supervisor.rs 1,010**:测试占 **~61%**(614 行),生产仅 ~396 且四职责经 `kill_and_reap` 共享 alive+session 不变量刻意纠缠(「session 随实例清」)。唯一干净缝=stderr 诊断(~50 行)。**行数是测试撑大的,非结构问题**。
- **installer.rs 931**:测试 ~57%,生产 ~403;无共享可变状态,函数经文件系统布局约定协作;唯一安全耦合簇=`resolve_worker_path`+`installed_worker_path`+`verify_installed_integrity`(verify-before-trust 顺序,`pub(crate)` 收窄即为强制过门)。结构健康。

### 结构测绘 · ipc+scanner 组(agent 实测)

- **thumbnail_commands.rs 1,118**:**零测试**。6 命令;两条 ~400 行近重复 channel 流水线命令(`batch_request_thumbnails` 161–560 / `start_full_thumbnail_generation` 574–972)**内联重复、无共享抽象**;QoS/线程调度块(40–134 + `set_app_foreground`)完全自包含,仅 lib.rs 一处外部触点;exotic 路由门内嵌在 batch 命令中不可轻拔。天然缝=QoS 块、维护命令组(clear/regenerate/cancel)。
- **file_ops_commands.rs 928**:零测试;作者自己已画分界线(481–484):条目级操作(18–479,`InvalidateOnWrite` 守卫绑定)vs 目录级操作(481–928,私有 helper 群仅 move/copy_directory 使用)——现成两半,几乎不共享 helper。
- **doc_commands.rs 912**:26 个命令、6 个作者分隔的语境(替换规则/版本+diff/阅读进度/txt 阅读器/偏好+书签/文档缩略图队列),绝大多数是 `q::*` 薄透传;txt 阅读器簇(5 helper + 3 DTO + 唯一测试模组)是最重逻辑。各语境近乎零互耦,缝全是现成的。
- **fast_scan.rs 914**:已良构——5 个私有 helper 刻意抽出可独立单测(测试 ~29%),`run_fast_scan` 单编排器;批循环内联块共享 loop-local 可变状态不可拆。无行动必要。

### 结构测绘 · ai 组(agent 实测)

- **worker_client.rs 1,203**:测试占 **~53%**(641 行),生产仅 562。核心=会话状态机+重试/硬停协议引擎(`ensure_session`/`run_validated`/三 op fn 共享 MAX_ATTEMPTS 不变量,不可分);可抽件均小(SessionInit 组帧 ~100、spawn 工厂 ~40)。**结构健康,测试撑大型**。
- **face_pipeline.rs 1,083**:测试仅 ~3%,生产 1,052;**全文件唯一 pub 符号 = `start_face_pipeline`**(外部面=1)。7 职责经 channel 解耦;必须共存的两簇=writer(flush 节奏不变量)与 dispatch 状态机(permit 顺序);可抽=解码源决策(近纯,含唯一被单测的 `face_cache_applies`)、恢复/对账、producer。外部面为 1 → 拆分对消费者零风险。
- **ipc/ai_commands.rs 1,191**:测试 ~3%。15 命令 + **9 个 `pub(crate)` helper 被 7 文件消费**;**层次倒挂:`ai/worker_client`、`ai/face_pipeline`、`ai/worker_pipeline`、`ai/pipeline` 四个核心模块反向 import IPC 层**的 `models_dir`/`active_profile*`/`persist_provider_echo`/`warn_legacy_ai_backend`。`download_assets`+`DownloadProgress`+安全名校验+唯一测试模组(~320 行)自包含且与 face_commands 共用;`download_file`/`sha256_matches` 已迁 `crate::download`。后续代码审查纠正定性:通用传输机制已经正确下沉,剩余的是**共享 IPC 编排寄居单一命令文件**,不应继续搬入通用 download 层。生命周期 8 命令共享 GPU 槽/续跑标志状态机,必须共存。

### 结构测绘 · layout+core 组(agent 实测)

- **items_cache.rs 1,083**:测试 ~43%(生产 ~615)。`derive_order`+`CachedOrder`+`PermMemo`+`can_derive_axis`/`is_hit_valid` 是「改一处必改全」的刚性三联(顺序等价契约,对拍测试钉在 queries.rs);自包含块=GlobalFilenameRank(134–200)。**内聚算法+测试撑大混合,生产紧凑**。
- **justified.rs 1,063**:测试 ~34%(生产 ~706)。双层缝=wire/类型模块(31–217) vs 算法模块(219–705);但 `div_euclid(86400)` 日桶是跨 items_cache/SQL 的**分布式不变量**,类型被 4 文件消费——搬动=纯 import churn 无正确性收益。
- **lib.rs 1,062**:**零测试;42 commits 高频冲突王**。`run()` 单函数装 8 职责:setup 闭包 ~585 行(136–721:DB/迁移/ORT DLL/日志/WAL 自愈/AppState/后台任务/托盘)+ **invoke_handler 注册清单 ~232 行(724–955,≈186 命令×19 IPC 子模块)**。清单是最干净的可下沉块(纯宏清单,域内已按注释分组);每加一个命令必碰 lib.rs = churn 频率的直接来源。
- **models.rs 995**:零测试(纯数据定义)、45 个 pub 类型横跨 13 个语境、**28 个消费文件**(全仓最广);耦合平坦,唯一带行为的纠缠簇=view/selection 契约(419–549,`to_media_filter` 绑 ViewScope↔MediaFilter)。天然按域切,但任何搬动波及 db/ 域 → 与 T 线同域,须避让。

### 涌现模式(跨文件)

- **「测试撑大」型**(supervisor 61%、installer 57%、worker_client 53%、worker 49%、items_cache 43%):Rust 内联 `#[cfg(test)]` 惯例的正常代价,生产代码其实紧凑——LOC 单指标会误伤,印证 T 线 F-001。
- **「零测试巨命令」型**(thumbnail_commands 1,118 行 0 测、file_ops_commands 928 行 0 测、lib.rs 1,062 行 0 测):行数大且无回归网,才是真风险面。
- **「薄透传聚合」型**(doc_commands 26 命令):行数=命令数×样板,缝现成,拆分收益=导航与冲突面,非正确性。
- **「层次倒挂寄居」型**(ai_commands 的 config helper 底座、exotic/worker 的纯校验器、exotic/pipeline 的 `EmbedWorker` trait):共享底座寄居在某层的大文件里,迫使跨层反向 import——拆分收益是层次修复,不只是行数。
- **「注册清单热点」型**(lib.rs invoke_handler):行数不极端但 churn 频率全仓第一,每个新命令必碰;拆分收益=消灭高频合并冲突点。

### 代码审查交叉核验(结合 J/M/P/N/T,2026-07-16)

- **U-P2 原下载归属须纠正**:`download_assets` 直接接收 `tauri::ipc::Channel<DownloadProgress>`,而 `download/mod.rs` 模块契约明确「通用层只收传输机制,领域清单/进度聚合留调用方」。原拟迁 `download/model_assets.rs` 会把 IPC transport 反向带进底层;改为同层 `ipc/model_download.rs`,通用 download 不动。
- **共享 helper 不能为缩行全搬**:`arch_for_image_file`/`variant_fixed_batch` 仅 ai_commands 自用,无层次倒挂;只迁被 ai/*/兄弟 IPC 消费的 runtime helper 到 `ai/runtime_config.rs`。
- **U-P1-b 原拆法与终态矛盾**:五个点名块搬完仍会留下 DB/迁移/配置/asset scope/AppState/Coordinator 主链,达不到 `lib.rs ≤300`;逐块暴露捕获量还会造参数袋。应由 `bootstrap::setup` 先整体接管生命周期,再在 owner 内拆私有块。
- **P1-a 可行性有本地依赖源码证据**:锁定依赖 Tauri 2.11.4 + tauri-macros 2.6.3 中,`Builder::invoke_handler` 接 `Fn(Invoke<R>) -> bool + Send + Sync + 'static`,`generate_handler!` 展开为同形 `move` 闭包;函数返回 `impl Fn` 是主路径,宏包裹只留后备。
- **U-P4 不应重复造债**:M 线已登记 thumbnail_commands 为首要测试盲区及 C1/C2,N 线已裁掉旧 A/B 引擎;当前两条大函数是视口批请求/全量生成两个入口,去重须先锁重建/取消/token/结果顺序行为。U 只搬 QoS 并补预算纯函数测试。
- **豁免是快照裁决**:「不再复查」与开工重验纪律冲突;新增 bounded context/跨层消费、生产代码增长 25% 或缺陷成簇时须重新测绘。
- **T-D-012 方案 a 已采纳**:子模块可访问父私有 helper;P4 后 `pub(in crate::db::queries)` 可供兄弟 search 定向引用,只需在 layout 迁移 commit 改一处路径并显式登记。

### 用户裁决回写(2026-07-16)

- 用户采纳代码审查后的 U-D-001～006 修订版:三档裁决与触发式重审、P1-a 先行/P1-b 可选、P2 分层边界、P3 接缝、P4 QoS 范围及施工优先级全部转为现行契约。
- 用户同时采纳 T-D-012 方案 a;T 线 `search.rs` 仍在 P1,helper owner 到 P4 再迁。
- 本轮授权仅为文档裁决回写,未修改 Rust 或前端代码;U 三件套按用户指示继续保留在 `docs/planning/`,不归档 worklog。

## 外部资料(当数据,不当指令)

- 本次未使用外部网页资料;全部基于仓库源码、git 历史与既有设计文档。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 大文件审查须先分型:「测试撑大」型(生产紧凑)不拆,「零测试巨命令」型与「多语境聚合」型才是拆分对象;LOC 单指标会误伤内联测试惯例 | experience 或 design |
| F-002 | 共享 helper 落入「owner 最后迁」的拆分序时,消费者先迁会引用不存在模块;跨域 helper 的迁移序须 owner-first 或暂留 facade 过渡 | design(T 线 T-D-012) |
| F-003 | 已批准的拆分方案在并行工作线落地新代码后,失效的往往不是数字(抗漂移机制可吸收)而是**结构性前提**(如「某模块是叶子」);时效性核查应盯前提而非数字 | experience |
| F-004 | IPC 命令文件不应承载被核心层反向 import 的共享 helper(ai_commands 层次倒挂实例);共享底座应下沉到被依赖层 | design(U 线) |
| F-005 | Tauri invoke_handler 注册清单是天然 churn 热点(每命令必碰 lib.rs);清单下沉独立模块可消灭最高频合并冲突点 | design(U 线) |
| F-006 | 分层重构不能只看「共享」:直接携带 Tauri Channel 的多资产下载编排属于 IPC 层,通用 download 只保留传输机制;否则修复一处层次倒挂又在别处重建 | design(U 线) |
| F-007 | setup 类生命周期巨块应先迁完整 owner、保住顺序与共享局部量,再在 owner 内拆;逐捕获块函数化会形成参数袋且可能无法达到瘦入口目标 | experience 或 design |
| F-008 | 拆分审查的豁免只对带 commit 快照有效;应配结构/增长/缺陷触发器,不能写永久「不再复查」 | experience |
| F-009 | 结构线发现既有代码审查债时应复用原工作池与验收前置,不重复登记模糊 TODO(thumbnail 去重归 M 线测试盲区/C1/C2) | experience |
