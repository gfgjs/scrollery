---
title: 全仓未完成工作梳理 · findings
date: 2026-07-24
---

# findings

## §1 摸底方法与证据源

- `git status --short` / `git diff --stat` / `git diff -- <file>`(主线亲见)
- `git log --oneline`:HEAD = `61d125f`(IPC 敏感错误定向脱敏)
- `docs/todo.md` 全量 + 六条近期活跃 planning 线 progress/findings(scout + Explore 摸底)
- MEMORY 索引(review-round2 / 施工规格集 等已收官线状态)
- 完整原始扫描存 scratchpad:`survey-unfinished.md`(含 Explore 生成的 58 项表,**含陈旧项,已在 §3 核实反正**)

## §2 关键发现:工作树正被并行会话实时编辑(勿动)

会话开始 `git status` = 8 个 M 文件;进行中 `git diff --stat` = 14 个 tracked 文件;再查 = 16 项。**数量随会话持续增长**,证并行会话(RAW 线或相邻前端线)正在实时改工作树。

14 处改动是成体系的活跃 feature 工作,非可收口的残件:

| 文件 | 改动意图(diff 亲见) | 归属判定 |
|------|--------------------|---------|
| src/components/media/player/VideoSeekBar.vue | 新增 `pendingRatio` 消 seek 松手「跳→弹回→跳回」闪烁 | 活跃 WIP |
| src/composables/useBucketVirtualScroll.ts | 速度自适应预取边距(`VELOCITY_MARGIN_FACTOR`/`MAX_PRELOAD_MARGIN_PX`)治 canvas 段空洞 | 活跃 WIP |
| src/components/settings/ReaderSettingsSection.vue | 阅读设置卡布局重构(内边距/分组) | 活跃 WIP |
| src/components/media/MediaGrid.vue + MediaThumb.vue + mediaGrid.helpers.ts/.spec.ts | 画廊网格一批(FAB 迁 wrapper + helper + 测试 + i18n) | 活跃 WIP |
| src/components/media/ContentViewer.vue | 小改 | 活跃 WIP |
| src/i18n/locales/{en-US,zh-CN}.ts | 配套文案 | 活跃 WIP |
| src-tauri/src/db/queries/derivations.rs + ipc/doc_commands.rs | store_doc_thumbnail 前端驱动守卫(白名单常量+SQL过滤+早退校验,cargo test 11 绿) | 施工规格集续波 WIP |
| docs/planning/.../施工规格集/{findings,progress}.md | 上述守卫的决策/进度回写 | 施工规格集续波 WIP |

**裁决 D-1/D-2**:全部不碰。提交任一即污染在飞工作或跨会话抢 git 状态。

## §3 未完成工作清单(已核实反正)

### 【确定项 · 可安全顺手做】= 0

无。所有 actionable 项落入下列不可动/待裁/待验收桶。Explore 原表列的「确定项」经核实全为**已完成**:

- ~~error.rs 定向脱敏(A2)~~ → 已落 `61d125f`(HEAD)
- ~~C1-C5(锁中毒/trash边界/删死码/移anyhow/worklog)~~ → 已 commit 且已 push(MEMORY review-round2)
- ~~文档层注释修订(A3-A5)、docs门欠账~~ → 已落 `f0709ff`/`25fe262`/`0eba7c8`;余「他线欠账勿代修」红线
- store_doc_thumbnail 守卫 → 见 §2,活跃 WIP,不由本线收口

### 【待裁项 · 需用户决策】

| ID | 事项 | 决策点 |
|----|------|--------|
| Q1 | 工作树 14 处未提交 WIP 归属与提交时机 | 确认哪条会话/哪条线负责收口;本线不代提交 |
| Q2 | R2-7 商标 FTO + 占位域名 + CI 冒烟(todo G3) | 全局最高杠杆,前置 Part7/8/官网/上架;须用户侧委托 FTO |
| Q3 | 签发机与真实生产公钥 ceremony(todo B1)| 用户离线机操作,手册可查 |
| Q4 | Part7 发布工程(mac 矩阵 + 证书链 T13-T16,现状~15%)| 依赖链见 Part7 §3.6.5 |
| Q5 | Part8 变现全 15 项(收款/签发/上架 D-C/D-3..D-14)| D10/D15 上架硬门 |
| Q6 | Part1 T9/T11(simsimd/ANN 实装)| 按 R2-2 dormant,触发条件待评 |
| Q7 | 应用配置重构待裁清单(progress task_plan 末 5 条)| 见该线 task_plan |
| Q8 | 前端动画重构:query 模式/预留集/D-403..405 | 见该线决策表 |
| Q9 | USE_PIPELINE 运行时化 A/B 开关(todo N)| 待真机实测定线上方案 |
| Q10 | filename 排序 B-file-ii(D-013)| 独立工序待 go,与 B-file-iii 不同 |
| Q11 | 顶栏 Phase 6 / 阅读器 R6 后置池 | 按需拉起,方案待论证 |

### 【⏸GUI 真机 · 待批 push】

代码就位、仅待真机验收或用户批 push 的积压线(不可主线施工),主要有:施工规格集正文波、全仓深度review二轮(已push,余⏸GUI)、画廊轴 minimap 解耦、前端动画重构、应用配置重构、阅读器 R5 竖排、图片编辑 v2、OCR 线、窗口化沉浸、自定义 ICC、日志/span 线、AI/face 根治、缩略图/导出/备份线等。完整 28 项明细见 scratchpad `survey-unfinished.md` §【⏸GUI/push】表。

> 注:该桶多数条目 MEMORY 各单文件已记录「余=⏸GUI真机+待批push」,本线不重复展开,真机批量验收另行组织。

## §4 裁决点清单(一眼可答)

1. **工作树 14 处 WIP 谁收口?** 本线不提交,请确认归属会话。
2. **是否要本会话推进任一待裁项(Q2-Q11)?** 默认全部挂起等用户点名。
3. **是否需要把 ⏸ 桶的待批 push 集中处理?** push 须显式批准。
