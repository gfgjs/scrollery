---
status: 施工中
type: 工作记忆
line: 主画廊重复项浏览施工
created: 2026-09-02
---

# 进度日志:主画廊重复项浏览施工

## 会话:2026-09-02
- 做了:读方案全文，建 planning 三件套；三子代理并行调研（后端/前端/旧页面），发现落 findings.md。
- 做了:P0 施工——TS 子代理完成（types/view.ts + types/layout.ts 契约增量、utils/duplicateLensQuery.ts + 25 用例全绿、vue-tsc/eslint 通过）；Rust 子代理完成代码后超时（未跑验证），主会话终审 diff 确认契约与规格一致，接手验证：fmt ✓、clippy -D warnings ✓、cargo test --workspace 进行中。
- P0 落地明细:
  - Rust: models/view.rs（DuplicateLensMode/DUPLICATE_LENS_ORDERING_VERSION=1/DuplicateLensDescriptor.validate/GalleryFilter::is_empty/DuplicateBucket）、ViewDescriptor.duplicate_lens（serde default+skip，缺省 wire 逐字节不变）、view_to_sql fail-closed（先 validate 再 DuplicateLensUnsupported）、error.rs 两个稳定 code 变体、geometry.rs（GallerySeparatorKind + LayoutRowItem 4 字段 + HydratedRow::Separator 8 字段，全 Option+skip+default）、items_cache.rs hydrate_rows 填 None、bin/sort_profile.rs 连锁补字段。
  - Rust 测试: view_to_sql.rs +6（wire 逐字节锁/round-trip/legacy JSON/validate 合法非法/view_to_sql 拒绝）、geometry.rs +4（None 不出键/键名值锁）、view_layout_parity.rs 新文件 11 视图形态 parity（view_to_sql 与 query_layout_items 同集）、dedup.rs +5（非 ready 排除/quick-only/hash_version 漂移/missing 可用性/成员分页完整性）。
- 验证:fmt --check exit 0；clippy --workspace --locked -D warnings exit 0；cargo test --workspace --locked 待完成。
- 遗留:P0 test 全绿后 commit（含三件套），随后进入 P1a（后端 groups 镜头布局）。

## 会话:2026-09-03
- 做了:P0 全量验证通过（cargo test --workspace exit 0；parity 11 用例 + lens wire 10 用例确认执行）→ P0 已提交 33b0e5a7。
- 做了:P1a-1 子代理完成：AppState.dedup_view_epoch（DedupStore::on_run_completed trait 钩子，complete_current 成功唯一路径 bump、锁外调用）；push_eligible_predicates 谓词提取共享（组查询与镜头成员查询同边界）；list_duplicate_lens_members（窗口函数过滤组规模≥2，normalized_dir_path=r.path||rel_path 完整路径排序）+ 6 测试（含镜头 vs 分页组查询同边界一致性锁）。
- 做了:P1a-2 两子代理中断（一次 inactive 超时、两次 Provider 错误），主会话接手完成：布局重构收尾（stitch_group_rows/pack_justified_rows/pack_grid_rows/grid_metrics 提取 + 6 处测试 fixture 补 separator_kind）、lens.rs 纯函数模块（子代理完成主体：assemble_lens_groups/lens_group_label/build_lens_projection/compute_lens_layout_justified|grid）、cache.rs（LayoutCacheData.lens_projection + store_layout_with_lens + get_lens_projection）、items_cache.rs hydrate_rows 投影参数 + separator_kind 透传、layout_commands.rs（ComputeLayoutParams.duplicate_lens + build_gallery_gen_key/build_lens_gen_key 提取 + 镜头 fail-closed 校验 + compute_lens_blocking 两轮单槽竞态处理 + ensure_canonical_items + 3 出口命令传投影）。
- 测试:lens.rs +7（组序/组内四键 NOCASE/folder_count 与跳过/label 格式与 size 边界/投影/justified 端到端/grid 端到端）、items_cache hydrate 投影 +1、layout_commands gen_key 双锁 +2。修测试期望 3 处（组内时间 DESC 语义、projection fixture id 越界、justified 末行撑满 131 行高）。
- 验证:cargo fmt ✓；cargo clippy --workspace --locked -D warnings ✓；cargo test --workspace --locked 25 target 全 ok（含修 link.exe 1104 残留进程）。
- 教训:①子代理单任务超 10 分钟会 inactive 被杀——大任务必须拆到 10 分钟级；②Provider 不可用时主会话直接施工是可行退路（上下文已充分）；③heredoc 追加大块内容不可靠，用 Edit/python。
- 遗留:P1a commit 后进入 P1b 前端。

## 会话:2026-09-03（续）
- P1b-1（8b544786）：子代理完成 duplicateLensStore/URL 同步/chip 首位固定/已暂停筛选摘要/排列分段控件/computeLayout 镜头参数链/滚动键统一 galleryScrollKey；i18n 双 locale；1685 用例绿。偏差：lensPaused 进 descriptors（overflow 位次正确性）、普通 chips 镜头态整体暂停（§5.1 线框）。
- P1b-2（68307261）：子代理完成 duplicateGroup 组头（DOM h2 + Canvas 自绘双引擎）、M/N 角标、DuplicateLensStatusBar（§9 状态表纯函数 8 态单源）、dedupStore.completedAt、镜头 navContext（查看器 flat_ids 序）、scroll key lens 维度、镜头空态；1717 用例绿。偏差：Canvas 跟随现状自绘（无 DOM overlay 通道）、计数徽标省略（label 已含）。
- P2（217dec5e）：P2a 子代理完成 view_to_sql 镜头 lowering（normalized_dir_path_sql 共享化、SQL 序与 assemble 逐位 parity、SelectAll 自动生效、hidden_roots 冗余性锁）；P2b 子代理完成 browse-only gate 全链（resolveCardClickAction 纯函数/SelectionToolbar forceHidden/MediaThumb browseOnly/框选右键 Ctrl+A 阻断/enterLens 清选区）、焦点恢复（reflow→lens 焦点锚点→scrollCache 单一顺序）、系统返回清快照 bug 修复、returnAnchorItemId 冗余删除；1716 用例绿。
- 遗留:P3 folders 镜头（拆 P3a-1 分桶查询/P3a-2 簇与布局/P3b 前端）；P4 入口迁移；P5 收拢。

## 会话:2026-09-03（P5 收拢）
- P4（d0228ab6）:子代理完成入口迁移——旧 DuplicatesView/5 组件/dedupFolderStore/选择树辅助/spec/死类型/孤儿 i18n 删除(-3549 行);dedupStore 裁组侧死代码留运行态;侧栏「重复项」深链;/duplicates 重定向;docs/status 两文件+todo.md 回写。
- P5a:门禁链全绿(Rust fmt/clippy --all-targets/test 25 target;前端 typecheck/eslint/vitest 1719;build 通过)。
- P5a 修复①:bundle 入口超预算 4.29kB(镜头 UI 按方案 §4.1 属首屏契约不可懒化)→ 按仓内 2026-07-22 先例重定基线 708→805(+13% 惯例,注释出处)。
- P5a 修复②:clippy --all-targets 2 处(测试 type_complexity/get-then-check)。
- P5b 性能基准(§15,1M 合成域纯函数层,ignored 手动跑):
  - groups 1M(500K 组):assemble 238ms + projection 45ms + pack 83ms ≈ 370ms(预算 3s ✓ 8x 余量)
  - folders 1M(1000 目录/250K 组/三桶):assemble 566ms + pack 143ms ≈ 0.7s(预算 5s ✓)
  - **关键修复**:簇内贪心遍历原实现比较器内现场做组集交集,O(D²)×大组集=36.9s 爆表;改组→目录倒排表增量计数(shared[dir] 在组入展示集时对其余成员+1),65 倍提速。SQL 取数段未含本基准(真实库校准列手动项)。
- P5c 验收矩阵核对(方案 §14,15 行):13 行有自动化测试锁定(组序/簇连通/分量不交错/三桶开关/分析状态/revision 漂移/Live Photo/暂停筛选/清选区/修饰键阻断/重定向);2 行需真机手动(查看器往返滚动恢复细节、键盘读屏全程走查)+Canvas/DOM 一致的语义层已知限制(Canvas 无 h2/h3/aria——与 date/folder 分隔符现状一致)。
- 已知边界:①簇头/组头 label 为后端中文生成,英文 locale 仍显示中文(与 date 分隔符同一先例,若需本地化须后端 label 生成侧另立切片);②view_to_sql 对 folders 显式拒绝(二部图遍历无 SQL 等价 lowering,browse-only 下无消费);③基准不含 SQL 段;④iOS/Android 真机验收未做(方案 P5 手动项)。
- 遗留:真机验收清单(Windows 双引擎切换/离线卷/快速滚动;iOS/Android 触控/返回键/安全区)交用户执行;收口(三件套迁 worklogs)待确认后进行。
- 最终提交链:P0 33b0e5a7 → P1a 20c2a3bf → P1b-1 8b544786 → P1b-2 68307261 → P2 217dec5e → P3a 1b16a5a3 → P3b 849e953d → P4 d0228ab6 → P5 0c6ce671。全部门禁绿(clippy --all-targets 0 / cargo test 25 target / vitest 1719 / vue-tsc / build)。

## 会话:2026-09-03（中断后全链核实）
- 背景:用户要求对跨多次中断的施工做快速核实。双审查子代理（Rust/前端）+ 全门禁重跑。
- 门禁实测:clippy --workspace --all-targets ✓；cargo test --workspace ✓（25 target 全 ok，lib 1259+8 ignored，前台 23s）；vue-tsc/eslint/vitest 1719 ✓；build ✓。**cargo fmt --check ✗**:0c6ce671(P5b 基准代码)两处漂移(lens.rs:578/lens_folder.rs:928)，CI 第一道门(ci.yml:69)会红——P5b 追加基准后未复跑 fmt。
- 发现 P1:「导出当前视图」绕过 browse-only gate——GalleryViewControls.vue offset-3 无 lensActive 门控,镜头态导出走 buildCurrentViewDescriptor(不带 duplicateLens)按普通画廊语义解析,静默导出与画面不一致的集合(违反方案 §8.1 批量命令阻断)。根因:P2 只闭环了后端 lowering,前端 useViewDescriptor.ts:74 过期 TODO("归 P2")从未回头。
- 发现 P2:①duplicateLensStore.returnViewKey 死状态(删 returnAnchorItemId 时漏删同类);②normalizeLensInUrl 内联复制 normalizeDuplicateLensQuery(模块头自警勿复制);③dedupStore error/errorCode 恒 null 死字段;④folders 状态条 groupCount=文件夹头数,文案叫"重复组"口径偏差;⑤Rust 两侧无 P0/P1(镜头分支未镜像 is_hit_valid 经分析无实险;hidden_roots 入参冗余系设计)。
- 记录修正:P2 段"契约闭环"实为后端半环;"门禁链全绿"对 0c6ce671 不成立(fmt);returnViewKey 漏删未登记。
- 环境观察(非代码问题):cargo test 以后台方式执行两次进程级冻结(CPU 零增长、24 线程全阻塞),前台重跑 22.8s 全绿;冻结点测试来自链外 a458e9da,隔离 5/5 过,判执行环境伪影。
- 遗留修复清单(同会话已全部修完):①fmt 两处已修(fmt --check ✓);②「导出当前视图」镜头态整项不渲染(GalleryViewControls offset-3 v-if=!lensActive,导出位于折叠序列末位卸载不影响其余 offset)+新增 GalleryViewControls.spec.ts 契约测试(镜头态导出缺席/排列分段在位);③returnViewKey 死状态连同 galleryScrollKey/useViewStore 依赖一并删除,spec 断言同步清理,useViewDescriptor 过期 TODO 改写为「镜头态本函数不可达+未来加批量须先携带 duplicateLens」的不变量注记;④normalizeLensInUrl 改调共享 normalizeDuplicateLensQuery;⑤dedupStore error/errorCode 死字段删除,useDuplicateLensStatus.safeErrorCode 直读 status.errors;⑥状态判定输入 groupCount→separatorCount+lensMode,folders 模式改用新增 i18n 键 analyzedFoldersTitle(双 locale,文件夹口径,后端文件夹头恒只覆盖含重复/疑似成员的文件夹,与独有项开关无关);⑦本条目即导出项补登。
- 修复后门禁:cargo fmt --check ✓;clippy --workspace --all-targets -D warnings ✓;cargo test --lib 1259 ✓(Rust 改动仅 fmt 重排);vue-tsc/eslint ✓;vitest 152 文件 1722 用例 ✓(+1 文件 +3 用例)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:

## 会话:2026-09-04（审查整改）
- 做了:完成近期提交链与当前代码复核；确认发布隔离、跨语义旧布局、lens 全量 flat_ids、镜头搜索控件、状态恢复竞态等问题。
- 决策:用户明确废弃本方案全部读屏/无障碍设计；其它建议按 MVP 直接修复。
- 验证基线:`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib lens` 39 passed/2 ignored；相关前端 9 spec/89 tests；rustfmt、typecheck、聚焦 eslint 均通过；工作树基线干净。
- 完成:P6 全部整改。后端采用 working/published 双表并在事务内发布；前端按内容语义 key 隔离 SWR 与迟到响应；镜头查看器改为布局缓存单项邻居 IPC；镜头态隐藏搜索且退出保留原状态；去重状态恢复加入事件版本门；分组/簇标题改由前端 i18n 组合结构化字段。
- 方案裁定:已从当前设计与验收矩阵删除本功能新增的读屏/无障碍设计，不建设 Canvas 语义层；已有通用交互实现不做破坏性回退。
- 集中验证:前端 Vitest 154 文件/1734 用例、`vue-tsc`、ESLint、生产构建全部通过；Rust `dedup` 74、`layout::` 140 passed/4 ignored、`db::migration` 13 通过，`cargo fmt --check` 与全 workspace/all-targets Clippy `-D warnings` 通过。
- 全量 Rust 观察:`cargo test --lib` 执行至 `state::scan_run_tests::concurrent_lifecycle_writers_restore_active_after_both_finish` 长时间无进展后人工终止；本次改动覆盖的三个聚焦测试域均已通过，未把该既有执行器挂起计作本功能失败。
- 遗留:仅真机 GUI 烟测——Windows 双绘制引擎下验证分析中保留旧发布结果、完成刷新、groups/folders/独有项切换、查看器前后导航、英文标题与退出镜头后的搜索恢复；其它平台设备可用时补同路径验证。
