---
status: 快照
type: 工作记忆
line: 画廊性能
created: 2026-07-19
---

# 进度日志:minimap 审查与渲染模式设置

## 会话:2026-07-19
- 做了:完成 minimap 全链审查;新增 `minimap_render_mode` 启动配置与设置页 select(仅色块/缩略图);缩略图选项显式标注加载延迟;修键盘 a11y、滚轮单位与同帧吞量、缓存目录身份、模式切换中止/资源释放、chunk 迟到回写绕过 LRU 五类确定性问题。
- 验证:定向 Vitest 14 项 minimap helpers + 2 项 i18n 完整性全绿;全量 Vitest 98 文件/1257 测全绿;`vue-tsc --noEmit`、全量 ESLint、Vite production build、`cargo check --workspace --locked`、`cargo test --workspace --locked --quiet`(831 过/0 败/7 ignore)、本任务 Rust 文件 rustfmt 全绿。全仓 rustfmt 被并行工作未格式化文件挡住;workspace clippy 被非本任务 `src-tauri/src/ipc/scan_commands.rs:96 manual_inspect` 挡住。
- 遗留:真机 GUI/大库性能验收。

## 会话:2026-07-19（裁决续作）
- 做了:按用户裁决开放日期/分组视图的时间轴/minimap 模式按钮；微图失败增加 400ms/1200ms 两次有限退避、重试账本上限与请求所有权 token，模式/目录切换和卸载会清除定时器与状态。
- 验证:定向 Vitest 15 项、全量 Vitest 98 文件/1258 项全绿；`vue-tsc --noEmit`、ESLint、Vite production build（含 bundle budget）、`git diff --check` 全通过。
- 遗留:真机 GUI/大库性能验收。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
