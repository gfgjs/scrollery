---
status: 快照
type: working-memory
line: 根文件夹显隐
created: 2026-07-18
---

# 进度日志:根文件夹显隐

## 会话:2026-07-18
- 做了:探索定架构(scan_roots/directories/media_items 链、push_where_predicates 唯一 WHERE 源、canonical 免JOIN红线、is_active 死列);AskUserQuestion 定隐藏范围=全库;定方案 D-001 条件子查询;起三件套;写计划 compiled-noodling-whale.md。
- **后端全落地**:SCHEMA_V21(scan_roots.is_hidden)+ 迁移(CURRENT_VERSION 21,v17/v19 回拨测试补 drop is_hidden);models.ScanRoot.is_hidden;scan.rs mapper/两SELECT/set_scan_root_hidden/hidden_root_ids;layout.rs push_root_exclusion + 线程 hidden_roots 穿 push_query_body/canonical_layout_sql/view_to_sql/query_item_ids + list_library_formats;media.rs get_app_stats;search.rs;scan_commands.set_scan_root_hidden(write_blocking+bump_data_version)+ registry;resolve/count_selection 注入;sort_profile bin 3 处补参。新增 7 个后端排除测试。
- **前端全落地**:types.ScanRoot.isHidden;IPC.SET_SCAN_ROOT_HIDDEN;ipcFixtures(fixture+handler);scanStore.visibleScanRoots+setScanRootHidden;RootFolderVisibilitySection.vue(新,仿 KnownVolumesSection);SettingsView storage 区挂载+搜索语料;FoldersSection 侧栏树改用 visibleScanRoots(4 处);i18n zh/en rootVis*;新增 scanStore.spec(3 测)。
- 验证(全绿):
  - 后端 `cargo test --lib` = **636 passed / 0 failed**;`cargo fmt --check` clean;`cargo clippy --tests` clean。
  - 前端 `npm run typecheck` exit 0;`eslint` 改动面 exit 0;`vitest run` 全量 = **1211 passed / 91 files**(含 localeIntegrity 键平衡)。
  - 端到端真机 ⏸待批(GUI)。
- 遗留/红线:V20/V21 曾在同批文件交织且 V21 迁移链式依赖 V20。**已按用户指示拆两本地提交**(stash 纯 V21 文件 + scratch 备份 8 个混合文件的 V21 态 → Edit 回退至 V20 → 提交 V20 → cp 恢复 V21 态 + stash pop → 提交 V21;每步 cargo check/test 验证):
  - `f931db5` feat(viewer):V20 内容页(14 文件,**无** Claude co-author——他线工作,仅代提交)
  - `6d92779` feat(settings):V21 根显隐(23 文件,含 co-author)
  - 拆后终检:V21 树 `cargo test --lib` = 636 passed;fmt clean;工作树净(仅余无关的 2026-07-17 未跟踪目录)。
  - **均为本地提交,未 push**(push 需用户另行批准)。收口未发起(需用户明示)。

## 会话:2026-07-18(续:生成侧流水线排除)
- **三流水线排除**(用户裁决:排除+补跑,前端 scanStore 触发):scan.rs 新增静态谓词常量 `EXCLUDE_HIDDEN_ROOTS`(裸 directory_id)/`EXCLUDE_HIDDEN_ROOTS_M`(m. 别名,仿 exotic 双变体);缩略图 3 查询、AI/人脸 get_pending+count 配对注入(枚举与 count 同口径,进度总数不漂)。unhide 补跑:缩略图走画廊 on-demand(显式全库 incremental 会连带扫别根积压,不做);AI/人脸走 maybeAutoResume(analysisActive 门控)。新增后端测试 hidden_root_pipeline_tests(枚举+count+unhide 复原)+ scanStore.spec 两断言(hide 不触发/unhide 触发)。
- 验证(全绿):`cargo test --lib` = **637 passed / 0**;fmt/clippy clean;`vue-tsc` exit 0;eslint 改动面 clean;`vitest run` 全量 = **1211 passed / 91 files**。真机 ⏸。
- **派生封面流水线排除**(用户追加裁决:第 4 条 video_cover/audio_cover/doc_thumb 也排,否则隐藏根封面生成不结束像故障):排除放两消费口 `get_pending_derivations`(+`EXCLUDE_HIDDEN_ROOTS_M`)与 `list_pending_doc_thumbs`(前端 pdf/svg 泵);backfill 不排(INSERT OR IGNORE 行躺表无害,unhide 后 status=0 行直接续跑=exclude_kinds 同款非破坏暂停语义);count_derivations_by_status 不排(前端只消费 isRunning)。unhide 踢:scanStore 查 DERIVATION_STATUS 未运行则 START_DERIVATION(同 useDerivationAutoStart.kick 守卫);**否决** db:media_enriched 方案——scanStore 监听器会置幽灵 enriching+10min 看门狗。pdf/svg 泵边缘(纯文档根 unhide 后无封面落地事件)靠下次 app 启动/可见性/任意 media_enriched 复跑,已注释。

## 会话:2026-07-18(续2:全线 review + 相邻面缺口全修)
- **Review 结论**:三提交核心实现无正确性 bug(热路径逐字节不变/参数续接不变量/`+` 抑制次序/迁移回拨/真库测试/S1 bump/语义搜索与回收站口径均核实为对);发现 4 处相邻面缺口——内部列表走 layout 单一事实源自动吃到排除,但「墙面」卡片的封面/计数是独立聚合 SQL,须逐个点名。
- **全修落地**(用户裁决「全修」):
  1. collections.rs:`list_collections` 系统夹封面/计数 + 用户夹计数注入 `EXCLUDE_HIDDEN_ROOTS_M`;显式封面 `a.cover_item_id` 不能裸走 COALESCE(短路绕过排除)改经 media_items 校验、隐藏时回退最新可见成员;`list_deleted_collections`/`recent_collections` 同口径。冷路径无条件注入(空集全通过)。
  2. faces.rs:`list_persons_by_ignored` 按隐藏集分支——空集走原 SQL 逐字节不变(反规范化 face_count 零开销);非空切变体:face_count 可见活算、封面 JOIN 加排除(cover_item_id 投影改 `m.id`,f.item_id 在 m 被排除后仍存活会漏 id)、EXISTS 守卫隐去零可见脸人物。
  3. exotic.rs:scan.rs 新增第三变体 `EXCLUDE_HIDDEN_ROOT_ITEMS`(item_id 形,任务表无 media JOIN),注入 `claim_exotic_tasks` 内层 SELECT;set_scan_root_hidden unhide 分支 `wake_exotic(ConfigChanged)` 踢 Coordinator 重领(hide 侧不踢,claim 谓词即时生效在途自然收尾)。
  4. media.rs:`get_trash`/`get_trash_keyset` 死路径(IPC 注册但前端零调用)注入 `EXCLUDE_HIDDEN_ROOTS` 对齐口径,防将来接线泄漏。
- 留档不改(有意偏差):AI/人脸设置页总数(ai.rs 205/216/226,用户已裁 count_analyzed 不动);storage 卷计数含隐藏根(管理面口径「隐藏≠不可管理」)。
- 新增测试 5:hidden_root_collection_tests×2(封面回退/计数/chips 置空/软删夹)、hidden_root_person_wall_tests(计数活算/封面不漏 id 与路径/空簇隐去/unhide 回归)、claim_skips_hidden_root_and_unhide_reclaims、hidden_root_trash_tests。
- 验证(全绿):`cargo test` 全量 = **643 passed / 0 failed**(638+5);fmt/clippy clean;前端零改动(纯后端查询层),vitest/vue-tsc 未重跑。真机 ⏸。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
