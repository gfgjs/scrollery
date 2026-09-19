---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-16
---

# 进度日志:Canvas缩略图加载渲染性能优化

## 会话:2026-08-16
- 做了:完成 Canvas 画廊缩略图链路审查并落盘报告;核实用户修订版(全部实质声明过检);按修订版 §5/§6 建本三件套,产出施工计划(task_plan.md)。
- 验证:审查核实基于源码逐条比对(useCanvasThumbPipeline/canvasThumbState/useRequestQueue/thumbnail_commands.rs 等)+ 既有 worklog 与 todo 交叉印证;无代码改动,无测试运行。
- 遗留:等用户确认施工计划后,从阶段 0(测量脚手架)开工;阶段 3(恢复缺口)是否先行待用户拍板。

## 会话:2026-08-16(阶段 3 施工)
- 做了:用户确认开工(阶段 3 提前、本机即目标机)。`useRequestQueue.ts` 实现队列内有界退避重试:拒绝原因类型化(cancelled 不重试;stalled/incomplete 重试),0.5s→1s→2s × 3 次,cancel 掐断退避等待期;`canvasThumbState`/`MediaGrid` 零改动。spec 改 4 个既有用例适配重试时序 + 新增 3 个重试语义用例。
- 验证:`npx vitest run src/composables/useRequestQueue.spec.ts` → 23 过;`npm test` 全量 → 134 文件/1535 用例全过;`npm run typecheck`(vue-tsc)无错;ESLint(两文件)无告警。CI 前端 job 已含 `npm test`,测试线已接。
- 遗留:阶段 0(测量脚手架)待开工;阶段 4a 的既有反证(单段 WebP/8 槽)已记 findings,勿无数据重试该方向。

## 会话:2026-08-16(阶段 0 施工)
- 做了:bench 五场景(--scenario=cold|warm|ungenned|offline|missing|selectanim)+ 录制期 250ms gauge/counters/heap 时序采样(recorder.currentCounters + 桥 sampleGauges)+ Node 侧 500ms 进程工作集采样(PowerShell,--pid 可指外部进程)+ --out 结构化 JSON + CDP 页面异常捕获;harness 补 &thumbStatus/&avail/&availRatio 参数与批量生成模拟(30ms/项,先送达后 resolve);main.ts 装 harness 最小 __TAURI_INTERNALS__ 桩(修 F-004 两处既有缺陷);selectanim 点击坐标从 canvas 视口矩形反推(固定坐标点进侧栏)。
- 验证:vitest 全量 1535 过;vue-tsc 无错;ESLint(6 文件)无告警;headless 冒烟六场景 × constant + cold × burst 全通(warm 冷格帧 ≈ cold 1/5、ungenned 最差、selectanim 选中态 true、时序 12 点/工作集 11-19 样本)。dev server 冒烟后已停。
- 遗留:阶段 1(真机基线)待开工——本机即目标机,需先定发布构建 + WebView2 采集方案(可行路径:bench --pid 采 app 进程;页面内指标经 __scrolleryBench 需 dev 模式或桥的发布态替代,见 task_plan 阶段 1 要点)。

## 会话:2026-08-16(阶段 1 施工)
- 做了:用户拍板 dev 构建方案。盘点真实库(65,594 项,已生成 99.8%,grid_row_height=64)。写真机 rig `canvas-realapp-bench.mjs`(WebView2 CDP attach + 同源驱动/时序/内存/异常)。排障链:画廊空白 → 日志「JustifiedLayout width<100 giving up」→ 窗口屏外 (-21333,-21333) 158×26 → CDP setWindowBounds 位置被钳 → Win32 MoveWindow 移正成功。跑 10 run 基线(冷4/暖2/全选态2/快滚1/复测1),数据入 findings 与 baseline-realapp/。
- 验证:每 run 产出结构化 JSON(session 计数/gauge/时序/进程内存/探针);warm≈零 jank 120fps、全选态 jank 44–60% 两区可复现。tauri dev 与 dev server 均已停。
- 遗留:阶段 2(多档源 A/B)待开工;窗口状态损坏是独立产品问题,待告知用户/立项;4K 视口模拟未跑(本机 1920 物理窗口已够出分层结论,4K 档可在阶段 2 A/B 中补)。

## 会话:2026-08-16(阶段 2 施工)
- 做了:2a 服务选档入库(19cad9b:serve.rs + hydrate 接线 + cache_key + DPR 原子上报);修生成档位×DPR 脱节(前端 targetSize 乘 dpr)。2b 真机 A/B:R1 重置→再生 655 项 128 档(DPR1.5 生效证据)→同区前后测 + 页内双档三段微基准 + 全库档位体积抽样。排障:app 视图排序与 SQL 序不一致致选区散布(y 轴扫描定位主簇重做);ORDER BY id 小样本选样偏置。
- 验证:cargo test --workspace 25 套件过;vitest 1535 过;vue-tsc/ESLint 干净;A/B 数据入 ab-stage2/(before/after/control/microbench/pairs/R1v2 备份)。
- 遗留:阶段 4c(全选态绘制,jank 44-60% 真机实锤)为下一优先;4b 扩槽可测候选;4a 维持反证门控。库现状:655 项以 128 档路径服务(512 文件在盘,机制自洽)。

## 会话:2026-08-16(阶段 4c 施工 → 数据否决回退)
- 做了:E 系列逐项开关(运行时 __e4cSkip 免 HMR 切换)在降级实例上测得 mask+ring≈0.9ms、clip≈0.7ms、手柄≈0;实现帧级批处理(selBatch.ts:联合 clip + 巨路径单次 fill/stroke + selectedCellGeom 单一来源,1539 用例全绿);重启制交错 A/B(每臂全新 app)裁决:**批处理回退被否决**——batched 43fps/98.9% jank(draw span 反而最快 2.4ms → 帧光栅化恶化)vs legacy 118-120fps/jank 0-1.4%;单实例 2×2 归因:联合 clip 是灾难级(6.7fps/150ms 帧),巨路径 mask/ring 也回退(40→59fps)。**legacy 逐格绘制在健康实例贴满 vsync,原 44-60% 基线为降级实例伪影**。全部代码回退至 a510195。
- 验证:回退后 vue-tsc 干净、helpers spec 61 用例过、git status 无本线残留;A/B 与归因数据存 rig 临时目录(e-series/、ab4c2-*.json),结论已录 findings。
- 遗留:4b 扩槽 A/B 进行中(冷区 6 区域配对);实例级渲染降级机制未明(F-011);空视图缺陷待立项(F-009)。

## 会话:2026-08-16(阶段 4b 施工 → 不采纳)
- 做了:MAX_IN_FLIGHT_THUMBS 64→128 冷区 A/B(6 区域配对,每 run 全新 app:64@0.35/0.45/0.62 vs 128@0.40/0.55/0.68,scenario=cold 8s)。结果:coldFrames 被区域差异主导(132↔24,699)无法配对,loadP50 无方向;无回退信号(draw/内存/fps 两臂无差)。
- 验证:裁决不采纳,维持 64;const 复位后 git diff 干净(仅三件套文档变更);app 进程已清。
- 遗留:进入阶段 5 收口:审查报告实测节回写(含修正「最重场景」结论)、todo.md 回写、蒸馏候选处置、三件套迁 docs/worklogs/。

## 会话:2026-08-16(阶段 5 收口)
- 做了:审查报告追加 §9 实测结果(含修正「最重场景」结论与文首指引);experience.md 增 §46-48(测量效度/巨路径反噬/rig 踩坑);todo.md 多档行翻 ✅ 定案 + 顶部增补收官注记 + 2 项立项候选(空视图死态/窗口状态损坏);写 closeout.md(F-001~F-011、D-001~D-005 逐行处置);三件套迁 worklogs/ + frontmatter 翻快照 + worklogs README 登记。
- 验证:check_docs 门禁本地复跑;git 显式 pathspec 提交。
- 遗留:无(本线收官)。立项候选与待查项已在 todo.md 登记。

## 回顾(收口时填)
- 亮点:①「降级实例上测出的单项收益」险些误导批处理上线——重启制交错 A/B + 单实例 2×2 归因两步把「draw span 变快」与「帧率崩了」拆开,避免了带病合入;② E 系列运行时开关(免 HMR)与探针(CDP dump)的轻量实验基建一天内反复复用;③ 4c 全程「先实现全量验证再上真机」,否决时零代码残留、干净回退。
- 教训:① 绝对值跨 app 实例不可比(43/60/118fps 三档摆动),真机 A/B 协议必须同实例切换或每臂重启交错;② canvas2d 的 draw-call 合并直觉在 WebView2 上不成立(联合 clip/巨 stroke 掉出快路径),「减少调用数」须先过同实例帧率 A/B;③ Vite HMR 会清 Canvas 位图 LRU,暖区测量前必须重跑热身趟;④ 区域差异(132↔24,699 冷格帧)使跨区域配对 A/B 失效,4b 因此只能裁「不证明」而非「证伪」。
- 意外:① 阶段 1 的「全选态 jank 44-60% 最重场景」被证伪为实例伪影——健康实例逐格绘制贴满 vsync;② 阶段 2 多档源上线后,原「最重场景」在暖区自然消解(小文件快装载),阶段 4 的主目标随前置阶段落地而消失了大半;③ 空视图死态(F-009)与窗口状态损坏(F-007)两个独立产品缺陷在测量过程中反复现身。
