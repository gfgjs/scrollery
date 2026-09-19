---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-12
---

# 执行日志

## 最终结果（2026-09-12收口）
- 七项P0/P1在报告覆盖范围内完成；前端1844、Rust1568、RAW独立3通过，Rust12项既有忽略。类型/lint/build/check/clippy/fmt及13项前端相关门禁通过。
- NSIS/MSI生成且载荷/法律资源检查通过；NSIS安装后boot7、ready11、chain36步均0失败；本轮新备份、改值恢复、清标记与卸载通过。
- 实际构建source=357e27f94a9556f87781b5ce178f888915276592dad121fc77cb954ad8303b3e；最终runner四文件digest=1212b9f5f7af032f3407be2eeaeb70bee98d11d3cbd48041f087971220720506。产品输入0差异；并行研究草稿和后续文档另分类，保留原构建基线。
- 最后runner只改失败清理假告警，正向boot与负向安装门覆盖；成功chain路径未改，未重复完整chain。最后语法、ESLint、三脚本selftest通过。
- 所有本轮代理已关闭，产品源码冻结；未提交/发布；用户原有两项删除及其他任务文件均保留。
- 限制：跨物理卷/断电未原生实测；模型推理未跑；PSD可选组件hash_mismatch；MSI未安装；滚动不宣称帧率提升；旧文档21违规/149豁免归P2。
- 永久结论见docs/reviews/2026-09-12-P0-P1实现与验收.md。下方为施工期原始记录，曾失败、等待和中间测试数只代表当时状态。

## 收口回顾
- 有效做法：故障注入结合原生安装验收；产品编写冻结后集中回归；不让UI成功、旧文件或脚本exit0代替真实完成。
- 教训：测试夹具也必须遵守实际接口；本轮修正了txt分类、分层目录API、Channel结束帧、缓存命中、导出finalDir、旧备份误命中、退出回包与端口释放时序。
- 意外：Tauri包内exe会有bundle-type补丁，不能与还原裸exe直接比hash；父进程RUST_LOG=warn过滤Ready info；并行调研导致全树持续变化，须分清产品输入与草稿。


## 会话：2026-09-12
- 用户授权开始P0和P1，接续完成的只读审查。
- 已读planning技能、报告验收项和状态，分派四个不重叠范围。
- 保留原始用户修改与前轮分析文档，不自动提交/发布。
- 第一批代理：Franklin(move P0-1)、Leibniz(engineering P0-2/P1-2准备)、Cicero(resume P1-1)、Feynman(scroll P1-5)，随后Herschel(scan P1-4)。模型仅两种获准供应商，max effort。
- 文件协调：P0-1拥有state/scan_commands/migration/error；scan不碰这些共享文件；i18n根locale由主会话集中接线；P1-3待resume完成后复用代理，避免aiStore并改。
- 为并行推进P1-3，另派Fermat只做ai/search.rs内核/测试；IPC与aiStore仍待Cicero释放后接线，避免共享改动。
- 主会话完成AnalysisBusy错误码最小共享接线（enum/Serialize/预期日志豁免），供P1-1消费；具体争用语义与测试由Cicero实现。
- 主会话为扫描UI接好sidebar.quickScan/fullScan/quickScanHint/fullScanHint四个中英文键；具体按钮/调用由Herschel接线。
- 主审工程验收脚本，要求启动前验证acceptance产物身份而不只拒绝用户安装路径；未验证身份前不启动exe。
- 主审P1-5初版采用锁内收集/阻塞探测/纯内存应用三段，保留512服务小格和404恢复；等待代理测量与完整调用接线结果。
- commandcode三路长时间重复探索且未交付代码批次，主会话先收敛执行要求后关闭，改由同属允许名单的opencode-go/deepseek-v4.1-flash接手：Dewey(move/max)、Sartre(resume/high)、Kant(scan/high)。后两项范围已明确，按用户High约定；不使用名单外模型。
- 当前代理ID由functions store impl_move/impl_resume/impl_scan/impl_search/impl_scroll/impl_engineering保存；旧三路shutdown已确认，避免双写。
- Leibniz完成P0-2：NOTICE重生、benchmark默认进程改scrollery，两门exit0。P1-2新增隔离配置、验收驱动、源码摘要、RAW独立验证脚本和CI job；3脚本selftest/语法/ESLint通过；RAW GNU独立3单测通过并产出worker。
- 验收安全边界复核通过：overlay无非法注释键，独立productName/identifier/MSI upgradeCode；spawn前验内嵌identity/PE productName/hash构建证明；普通exe反向测试在启动前拒绝。完整native/安装包尚未启动，等集成快照。
- 搜索内核阶段一：24个search测试与61个AI聚焦测试通过；主审退回两个边界（seq登记同锁、cache epoch检验/安装同锁），IPC尚未接。P1-3不算完成。
- P1-5初轮通过serve7/items19/layout146/thumbnail47/ipc2聚焦测试并产出受控测量；主审要求新await后校验layout版本，避免旧几何配新载荷，且去掉未接线的生产invalidate入口。暂待修订。
- P1-2脚本主审追加：cargo test的harness不等于普通raw-worker.exe，要求显式cargo build并冷产物验证；launchApp隐藏console。准备脚本验收不借暖缓存判绿。

## 回顾
- 阶段3前端完成：Leibniz严格记录GATE_EXIT及shell exit，13门均0（npm test/typecheck/lint/build、notice、rename、sync-version、path-hygiene、exotic-protocol selftest+gate、theme contrast/palette、channel bundle）；163文件1844测试，Vite7.76s。报告target/acceptance/phase3-frontend-gates.md/json；frontend-freeze-files.json为报告时刻576文件指纹，不冒充最终源码快照。没有修改前端源码、无cargo全套/原生打包。
- P0测试已不再挂起：新一轮25例中24通过，仅crash_after_publish_before_delete_source_converges失败；Dewey发现staging内部ownership标记污染payload摘要，正在修正标记位置。旧日志位于src-tauri/dirmove-test.log（测试生成）。
- 已放行Leibniz阶段3第一段：前端全量测试/类型/全仓lint/Vite build和门禁，前端代码已冻结；先不跑Rust全套、不固定最终摘要、不打包native。当前只Dewey改P0 Rust，等其通过再放行第二段。
- Fermat收尾P1-3：默认target精确过滤ai::search与ipc::ai_commands，29通过（search4/control23/teardown2），21.7s含增量编译；stores206通过、aiStore9条、类型/lint/fmt通过。restart/rebuild共用reset_embeddings_with_search_teardown，即使分批删除后UPDATE失败也吊销、失效、擦库，再传播原错；model配置+sync结果保存，model_switched共同收尾后再传播错误；mixed转普通查询也清旧语义。Fermat关闭，当前只等Dewey移动Rust聚焦失败/挂起收敛后GO工程。
- 挂起诊断：Dewey误以为等另一cargo锁，实际默认target的cargo test --lib ipc::dir_move子harness自07:16停滞；Fermat另temp target宽ai:: ipc::也匹配同一挂起。主会话核实两个PID的完整exe路径后仅停止自有测试harness32288/5736，让父cargo输出缓冲日志。已要求Dewey修挂起用例、使用实时日志；Fermat精确过滤搜索/AI IPC，停止复制target绕过问题。此前这两次不能记通过。
- Nash收尾P0前端：有限错误详情不丢恢复id/路径；恢复清单启动只读、单条显式重试、列表修订号拒旧读；初次move/undo/redo统一complete/pending判定，pending不进反向历史、不报成功，拖拽唯一调用方同步返回值；复制入库失败展示已复制待重扫及动作。163文件1844前端测试、vue-tsc、改文件ESLint/Prettier通过。主审核实唯一生产history.move调用方；sourceLeftover有值必须配recoveryId为后端不变量。Nash关闭。
- Leibniz完成ACL生成验证：cargo check -p scrollery exit0；registry与allow-app-commands均261条，双向差集空，3个新增IPC均覆盖；main/logs精确窗口、无remote。权限名改错的变异导致tauri-build拒绝，恢复后通过。工程待最终GO。
- 安装后验收脚本已备：带bundle的acceptance构建→产物身份/hash证明→NSIS currentUser安装到自有target/acceptance目录→安装目录再次验身份/hash→chain→卸载自有安装项；MSI只做构建及载荷解包，不装。当前只验证脚本与守卫，真实安装/native仍未运行。
- 新增Nash(opencode-go/deepseek-v4.1-flash/max)分担P0前端闭环：history/有限IPC错误详情/恢复列表与显式重试/复制后扫描未完成提示，Dewey仅保留Rust。agent id=01a092c2-1a43-7bc1-b203-4efd644a3483，functions store impl_move_ui；共享IPC常量只局部file_ops段，保留Fermat搜索段；ManagementSection保留Kant扫描入口。两locale后续目录恢复键归Nash。
- Kant二次收尾：生产扫描收尾只在walk_complete&&volume_online清dirty，新增finalize_walk_errors_preserve_dirty_and_force_next_quick_full、finalize_offline_volume_preserves_dirty_and_forces_next_quick_full，均生产收尾→实际quick扫描补回漏项。rustfmt/diff通过，测试被搜索在途warn导入阻断，已告Fermat；Kant再次关闭。
- Feynman收尾P1-5：最终生产lib build和三文件rustfmt通过；serve7/items19原有证据有效；ipc::layout_commands::tests::layout_swap_during_offloaded_io_is_rejected改写为调用生产apply_visible_rows的版本待集中test-cfg验证。数值及限制记录在滚动取行测量.md。Feynman关闭。
- Kant追加最后收尾修正：finalize缺失检测对walk_complete=false或volume_online=false返回Ok(0)，原无条件清dirty会假完整；已恢复Kant，仅修此守卫及生产收尾路径反例。
- 工程新增必要IPC权限接线：本地tauri2.11.4源码证明有app manifest后所有custom IPC进入ACL；以registry命令为唯一源构建权限，main/logs显式授予，避免只声明3个新命令导致旧API被拒。
- Kant收尾P1-4：ManagementSection直接调用既有scanStore，去掉仅为布尔传参新造的helper和镜像测试；前端31聚焦通过、类型/lint通过。新增scanner::fast_scan::tests::p1_4_freshness_tests四例：full_scan_detects_in_place_edit_and_rejects_stale_derived_writes、interrupted_scan_forces_next_quick_to_full、cancelled_scan_leaves_incomplete_marker、baseline_marker_only_clean_when_proven_clean。前三例曾通过；最后保守读修订被P0在途编译阻断，待集中验证。Kant关闭。
- Sartre收尾P1-1：19条聚焦测试与3类守卫变异验证通过；其时前端全量159文件1805测试、vue-tsc/ESLint/cargo check --lib通过。Rust all-targets遇尚在施工的dir_move测试错误，不作为最终回归结果。共享文件已释放给Fermat，Sartre关闭。
- P1-3已按文件内职责放行接线：Fermat改ai_commands搜索/模型失效段、aiStore搜索段、state缓存字段；Sartre保留分析start/restart/status与controller。Dewey保留state扫描gate和移动IPC，共享文件均局部patch。
- 工程第二次复核完成：CDP启动前独占绑定验空端口，attach后用get_log_dir/get_thumb_cache_dir/get_config_status.path三份只读身份验证后才允许写IPC；attach失败回收自有进程。守卫正反与变异测试通过，native尚待最终构建。
- RAW cold-proof确证cargo test不生成普通worker.exe，显式build重新产出9,283,450字节并清暂存；CI --expect-binary已固定build前置。
- 二次主审：补齐等待态中英文文案；恢复控制需守住maybeAutoResume状态查询期与restart被stop取代的顺序；扫描基线读取错误走保守全量；滚动新增await后的版本复验已在工作区，待测试回传。
- P0-1第一版未验收：已明确要求目标归属证明、删源内容校验/部分残留、目标离线保留、路径边界与短DB阶段；代理正在修订。P1-2仍等待代码稳定，不提前打包。
- 施工中，待完整验证后填写。
