---
status: 快照
type: working-memory
line: 缩略图进度刷新丢失与视频派生独立控制
created: 2026-07-18
---

# 进度日志:缩略图进度刷新丢失与视频派生独立控制

## 会话:2026-07-18
- 做了:摸底+方案定型后五阶段一次施工完:①缩略图进度 Channel→事件+快照+`full_thumb_gen_status`;②派生流水线多 kind 过滤+`start_derivation(kinds,reset)`+分 kind 计数;③scanStore 事件订阅+App.vue 启动恢复;④derivationStore+ToolsSection 视频卡+SettingsView 特例行+i18n。
- 验证:`cargo test --lib` 645 passed / clippy 零警告 / rustfmt 过;`vue-tsc` 零错;eslint 改动面零输出;`vitest run` 全量 91 文件 1213 passed(含新增 2 恢复用例+2 DAO 用例)。
- 施工纪律:工作树预存 FaceModelLibrary.vue+SettingsView.vue 未提交改动(thumbSize 行布局线,非本任务)——FaceModelLibrary 未触碰;SettingsView 用过滤 patch `git apply --cached` 只暂存本任务 4 个 hunk,thumbSize 两 hunk 留在工作树。
- 遗留:真机 GUI 验收(见下),不可自动化。

## 真机 GUI 验收步骤(not automated)
1. 启动全量/增量缩略图生成 → F5 刷新页面 → 侧栏工具卡进度条应立即恢复(数字续走,非归零重置);生成完成后刷新 → 显示「已完成」终态。
2. 设置页→视频:新行「提取视频封面/关键帧」三按钮;侧栏工具区把它置顶后出现卡片。
3. 不勾「提取关键帧」点增量 → 只补封面(日志 filter=["video_cover"]);勾选后点全量 → cover+keyframes 全部重做(日志含 reset 行数)。
4. 关闭「提取视频封面」开关后手动点增量 → 封面照提(显式覆盖开关,D-002)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
