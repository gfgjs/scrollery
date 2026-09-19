---
status: 施工中
type: 工作记忆
line: Kun-0.2.28与Scrollery-UI对比重做
created: 2026-08-14
---

# 进度日志:Kun-0.2.28与Scrollery-UI对比重做

## 会话:2026-08-14
- 做了:确认用户要求废弃旧参考项目，并建立独立的重做分析记录。
- 做了:从 Kun-0.2.28 重新读取 Electron/React 入口、Tailwind 语义 token、base-shell、设计文档、UI mode 和 memory/settings 截图；重新读取当前 Scrollery 的 CSS 入口、六个主题、布局/设置/工具栏样式和 Moonlight 截图。
- 验证:Kun-0.2.28 默认浅色 token 为浅蓝 canvas+半透明白面+渐变/haze/阴影；Scrollery 默认亮色主题为 Moonlight，基础色已接近 Kun，但组件 recipe 仍以实体卡片、边框和重复分组为主。
- 结论:本次差异的首要变量是 surface composition、全局材质范围和内容分组，不是基础色值；截图只作方向性佐证，业务页面不同需保留产品角色校正。
- 遗留:无实现任务；完成全新对比交付，并明确旧 Kun-master 结论废弃。

## 回顾(收口时填)
- 亮点:先用 token 对齐浅色基线，再用截图核对容器分组，避免把“清透”误判为某一个颜色或 blur 数值。
- 教训:不能把不同版本的同名项目目录当作稳定设计系统；每次必须重新确认入口、默认 mode 和实际 CSS 覆盖关系。
- 意外:正确版本的 Kun 与 Scrollery Moonlight 色板相当接近，视觉差距主要来自透明层级、渐变/glaze 和页面结构。
