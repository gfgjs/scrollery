---
status: 快照
type: 工作记忆
line: 画廊大图首次打开慢与闪烁修复
created: 2026-08-03
---

# 进度日志:画廊大图首次打开慢与闪烁修复

## 会话:2026-08-03
- 做了:建立任务计划；读取 `docs/README.md` 与 2026-07-23 大图无闪修复工作记忆；追踪画廊点击、`/view/:id` 路由、`ContentViewer` 挂载和 `GET_MEDIA_DETAIL` 时序。
- 做了:新增共享查看器路由加载器；画廊空闲帧和图片/视频点击路径预取查看器代码块；移除全局 `.content-viewer` `viewer-in` 透明度/缩放进入动画。
- 验证:`npm run lint`、`npm run typecheck`、`npm test -- --reporter=basic`（134 文件、1528 项）和 `npm run build` 全部通过；入口包 675.43 kB / 708 kB，`ContentViewer` 懒块 152.90 kB（gzip 46.66 kB）。浏览器 harness 验收 `animationName=none`、`animationDuration=0s`、图片 `complete=true`、`naturalWidth=960`。
- 遗留:真实 Tauri Windows/macOS 仍需人工确认首屏空闲预取在低速磁盘/网络下的体感；该项不由自动化 GUI 门禁覆盖。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
