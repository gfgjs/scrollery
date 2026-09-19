---
status: 施工中
type: 工作记忆
line: canvas设置项生效检查与修复
created: 2026-08-27
---

# 进度日志:画廊Canvas悬停缩放修复

## 会话:2026-08-27
- 做了:读取 UI/UX 交互与动效规则；检查 MediaGridCanvas、useCanvasHoverCard、useCanvasHitTest、MediaThumb、configStore 和启动配置链路；核对了 `cb459c47` 的悬停放大开关修复。
- 验证:静态链路确认两个根因：悬停卡 pointermove 对交互控件仍按底层几何重命中；App 启动水合只切 CSS class，未回填 configStore 的 Canvas prop。现有 computeHoverRect 单测已覆盖关闭开关的纯几何分支，但缺少按钮命中和启动回填契约。
- 做了:为悬停卡控件增加语义标记；交互控件的 pointermove 不再重命中底层 Canvas，收藏/评分/选择控件的 pointerdown 不再进入卡片级拖拽/框选入口；启动配置回填 `configStore.enableHoverScale`；悬停卡改为 token 化、无过冲的 opacity+scale 入场。
- 验证:聚焦 Vitest 69/69；全量 Vitest 143 个文件、1606 个测试通过；`npm run typecheck`、`npm run lint`、`npm run build` 和 `git diff --check` 通过。生产构建仅报告既有动态导入提示。
- 复核修正:用户指出保护范围不应只覆盖控件；移除控件标记命中分支，改为悬停卡内部所有 pointermove 都保持当前图片，只有离开悬停卡后才恢复 canvas 命中。
- 验证:修正后聚焦 Vitest 68/68；全量 Vitest 143 个文件、1605 个测试通过；`npm run typecheck`、`npm run lint`、`npm run build` 和 `git diff --check` 通过。
- 遗留:真实 Windows WebView2/Tauri 运行时的鼠标观感仍需手动验收，自动化已覆盖命中边界、设置回填和动效契约。

## 回顾(收口时填)
- 亮点:把整张悬停卡作为当前图片的交互边界，卡内移动保持当前实例，离开后才恢复底层 Canvas 命中。
- 教训:Canvas 与 DOM 共用视觉组件时，用户配置必须同时同步到 Store/几何入口和 CSS class；只改其中一条链路会留下首屏时序缺口。
- 意外:问题不是单纯的按钮 click 冒泡；切换发生在 pointermove 的异步悬停准备阶段，因此仅加 `click.stop` 无法解决。
