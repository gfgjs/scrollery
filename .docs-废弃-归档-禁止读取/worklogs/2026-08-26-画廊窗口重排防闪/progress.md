---
status: 快照
type: 工作记忆
line: 画廊窗口重排防闪
created: 2026-08-26
---

# 进度日志:画廊窗口重排防闪

## 会话:2026-08-26
- 做了:完成当前代码链路审查，定位布局 IPC 中间结果提交、bucket 清空、Canvas backing store 重置和 DOM row key 重挂载四类闪烁源。
- 做了:提交 `c9768661`，只涉及媒体布局提交、bucket 保帧、Canvas resize 调度、DOM row key 和对应回归测试；保留其它工作区改动未暂存。
- 验证:聚焦 Vitest 46/46；全量 Vitest 142 个文件、1600/1600；`npm run typecheck`、改动文件 ESLint、`npm run build` 全部通过。构建仅报告既有动态导入提示。
- 遗留:无代码遗留；真实 Windows WebView2 拖动窗口的观感仍需手动验收。

## 回顾(收口时填)
- 亮点:用最新结果提交和旧段首帧种子同时切断“中间布局”和“空段表”两条闪烁链，改动集中且可单测。
- 教训:Canvas 的 props watch 可能绕过 ResizeObserver 防抖，必须增加 resize settling 闸门而不只延迟 observer 回调。
- 意外:默认 DOM+bucket 路径比 Canvas 更容易触发空帧；Canvas 方案仍需保留以覆盖实验渲染模式。
