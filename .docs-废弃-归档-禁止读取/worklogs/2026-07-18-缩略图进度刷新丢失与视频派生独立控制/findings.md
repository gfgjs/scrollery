---
status: 快照
type: working-memory
line: 缩略图进度刷新丢失与视频派生独立控制
created: 2026-07-18
---

# 发现与决策:缩略图进度刷新丢失与视频派生独立控制

## 需求
- 「缩略图流水线,开始生成后刷新页面会丢失进度,但生成还在跑」→ 进度显示要能在 reload 后恢复。
- 「给提取视频封面和关键帧单独加 全量提取/增量提取/停止;勾选了关键帧才提取关键帧,不勾选则只提取封面」。

## 发现
- 进度丢失根因:`scanStore.runThumbnailGeneration`(src/stores/scanStore.ts:319)用 Tauri `Channel` 收进度;Channel 生命周期绑定发起它的 webview,reload 后前端无任何状态查询/重订阅路径。后端 `run_thumbnail_generation`(thumbnail_commands.rs:497)spawn_blocking 脱离 invoke 继续跑,`on_progress.send` 失败被 `let _` 吞,故「生成还在跑」。
- 后端运行态真相已有:`state.thumb_gen_token`(state.rs:85)存在即运行;缺的只是进度快照 + 可查询命令 + 广播传输。
- 派生流水线控制面已有 start/pause/stop/status 四命令(derive_commands.rs),但 `kind_filter` 只支持单 kind(`Option<DerivationKind>`);视频线需要 cover+keyframes 两 kind 同跑。
- `enable_video_cover`/`enable_video_keyframes` 配置已存在且流水线已尊重(pipeline.rs:125 disabled_kinds:不 backfill、生产者排除、非破坏暂停);前端 configStore + DynamicSettingControl 已有开关 UI。
- `reset_derivations_by_kinds`(derivations.rs:186)现成:status 2/3→0 清 payload,正是「全量提取」所需重置;有测试锚定语义。
- 工具卡模式:settingsMap 注册 key(control:'custom')→ SettingsView 特例行 + ToolsSection 置顶特殊卡(`fullThumbGen` 为样板,ToolsSection.vue:27);置顶列表 ui.pinnedSettings 持久化于 app_config `pinned_settings`。
- `derivation_status` 计数(count_derivations_by_status)是全 kind 聚合,视频卡需分 kind 计数——需新查询带 kinds IN 过滤。
- 派生流水线无进度事件,前端 aiStore 模式 = 运行中轮询 status;视频卡沿用轮询即可,无需给 writer 加发事件。
- 工作树已有未提交改动:SettingsView.vue、FaceModelLibrary.vue(他人/前序工作)——施工须保留,提交用显式 pathspec。
- scanStore 已有测试 `src/stores/scanStore.spec.ts`,改传输层须同步。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Tauri Channel 进度传输在 webview reload 后即断且不可重订阅;长任务进度必须「事件广播 + 后端快照可查询」双件套,Channel 只适合与 invoke 同生命周期的短任务 | experience |
