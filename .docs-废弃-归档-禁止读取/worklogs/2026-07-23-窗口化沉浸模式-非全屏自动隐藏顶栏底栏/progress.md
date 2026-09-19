---
status: 快照
type: working-memory
line: 窗口化沉浸模式-非全屏自动隐藏顶栏底栏
created: 2026-07-23
---

# 进度日志:窗口化沉浸模式-非全屏自动隐藏顶栏底栏

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:全阶段完成,commit 324453a(13 文件 278+/4-);剩 ⏸GUI 真机验收 + 待批 push
- 未解错误:无
- 关键指针:本文件;决策表 D-422..D-426 见 task_plan.md(D-423 已更新为按窗口态分档)

## 会话:2026-07-23
- 做了:摸底三路并发(Explore+scout×2,haiku)+设计四裁决(详见 task_plan 决策表 D-422..D-426);施工 implementer(sonnet)28 轮,改动 9 文件:uiStore(StartupConfig 字段/ref+setter 92d8396 式/hydrate/return)、settingsMap 注册、i18n zh+en、useChromeReveal(第三来源+activeRevealEdgePx 8/4 分档)、spec 两用例+mock、ipcFixtures 补字段、后端 schema.rs 注册 auto_hide_chrome_windowed(Bool,default false)+config_commands.rs StartupConfig 三处
- 验证:施工段——vitest 涉改 20/20、vue-tsc、eslint 7 文件、cargo check+clippy 全 exit 0(implementer 回执)。本次批末全量门禁(phase-closer):
  - `npx vitest run` exit 0 — Test Files 117 passed(117),Tests 1416 passed(1416)
  - `npx vue-tsc --noEmit` exit 0 — 无输出
  - `cargo test -p scrollery --lib` exit 0 — test result: ok. 955 passed; 0 failed; 6 ignored
- 复核批(reviewer opus 深审 13 轮):1严重(DynamicSettingControl toggleBindings 缺绑定,开关 UI 不可达)+1警告(activeRevealEdgePx 零测试绑定)+1存疑(分档按来源与死区立论矛盾)。裁决:采纳存疑,判据改按窗口态 `isFullscreen ? 4 : 8`(D-423 立论更新,窗口化查看器沉浸同吃死区、8px 系顺手修既有缺口)。修复批(原 implementer 续 8 轮,3 文件)+增量核验(原 reviewer 4 轮,五点全过+独立复跑 vitest 23/23/vue-tsc/eslint 绿)
- commit 324453a:10 源文件+三件套,显式路径 stage;并行会话 dirty(ContentViewer/VideoControlBar 等)已核 grep 零命中本线关键词,绕开未 stage
- 遗留:⏸GUI 真机验证——窗口化开关后顶/底栏隐藏、鼠标移边缘唤出、8px 带真机 wry 环境是否够宽、与 F11 全屏沉浸往返无残留、设置页开关行可见可切且重启后持久;待批 push
