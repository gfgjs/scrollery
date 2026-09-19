---
status: 快照
type: working-memory
line: 缩略图进度刷新丢失与视频派生独立控制
created: 2026-07-18
---

# 任务计划:缩略图进度刷新丢失与视频派生独立控制

## 目标
① 缩略图全量/增量生成中刷新页面(webview reload)后,进度条与运行态可恢复显示(后端本就继续跑);
② 视频封面+关键帧提取获得独立控制面(全量提取/增量提取/停止),且「提取关键帧」勾选决定是否连带关键帧,不勾只提封面。

## 当前阶段
阶段 5:完成(余真机 GUI 验收)

## 阶段

### 阶段 1:后端——缩略图进度改事件广播 + 状态快照
- [x] AppState 增 `thumb_gen_progress: Mutex<Option<FullThumbProgressPayload>>` 快照
- [x] `run_thumbnail_generation` 进度发送改 `publish_thumb_progress`(快照先行 + `app.emit("thumb:gen_progress")`),六处 send 全换,弃 Channel 参数
- [x] 新命令 `full_thumb_gen_status`:快照 + token 真相;快照 running 而 token 不在 → 归一 cancelled
- [x] registry.rs 注册新命令
- **状态:** done

### 阶段 2:后端——派生流水线多 kind 过滤 + 全量重置 + 分 kind 计数
- [x] `kind_filter` 全链 `Option<Vec<DerivationKind>>`;IPC `start_derivation(kinds, reset)`(旧单数 `kind` 参数前端零调用方,直接换)
- [x] 显式 kind_filter 覆盖 disabled_kinds(pipeline.rs retain)
- [x] `reset=true` 先 `reset_derivations_by_kinds`(2/3→0)再启动;reset 无 kinds → 拒绝(防全 kind 误清)
- [x] `count_derivations_by_status_for_kinds` + `derivation_status(kinds)` 限定计数;新增 2 条 DAO 单测
- **状态:** done

### 阶段 3:前端——scanStore 改事件订阅 + 启动恢复
- [x] scanStore:Channel → `listen(EVENTS.THUMB_GEN_PROGRESS)`(共享注册 Promise 防并发窗口)
- [x] `restoreThumbGenProgress`:App.vue onMounted 调,先订阅后查快照;completed/cancelled 回填不触发 invalidateLayout
- [x] scanStore.spec.ts:+2 恢复用例(running 回填+事件接续 / completed 只显示)
- **状态:** done

### 阶段 4:前端——视频派生控制卡
- [x] settingsMap `videoDeriveGen`(video/custom)+ i18n 双语 6 键
- [x] 新 derivationStore:kinds 随勾选、运行中 1s 轮询、start/stop/fetch
- [x] ToolsSection 特殊卡:增量/全量/停止 + 关键帧勾选(绑 config.setEnableVideoKeyframes,运行中禁改)+ 进度/失败面
- [x] SettingsView 特例行(增量/全量/停止 + 进度;未提交的 thumbSize 改动原样保留、分 hunk 暂存隔离)
- **状态:** done

### 阶段 5:验证与收尾
- [x] cargo test --lib 645 绿 + clippy 零警告 + rustfmt
- [x] vue-tsc 零错 + eslint 改动面零警告 + vitest 全量 1213 绿
- [ ] 真机 GUI:刷新恢复进度条、视频卡三按钮、勾选切换 —— 不可自动化,待用户验收
- **状态:** done(余 GUI)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 缩略图进度 Channel → Tauri 事件 + AppState 快照 | Channel 绑死单次 invoke,webview reload 即断;事件天然广播给当前 webview,快照供重载后查询恢复 | D-001 |
| 手动视频提取显式 kinds 覆盖 disabled_kinds | 用户点「全量/增量提取」是明确意图;背景 enable_video_cover 开关只管自动流水线 | D-002 |
| 关键帧勾选 = 既有 `enable_video_keyframes` 单一事实源 | 已有配置+流水线 gate,卡内勾选直接绑 configStore,不另立状态 | D-003 |
| 全量提取只重置派生行(2/3→0),不动 media_items 缩略图镜像 | 新封面落地时同路径覆盖+writer 回填,旧封面过渡期仍可显示,UX 更好 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
