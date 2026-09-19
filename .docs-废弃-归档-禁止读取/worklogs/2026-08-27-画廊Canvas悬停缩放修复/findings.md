---
status: 施工中
type: 工作记忆
line: canvas设置项生效检查与修复
created: 2026-08-27
---

# 发现与决策:画廊Canvas悬停缩放修复

## 需求
- Canvas 模式下鼠标移入缩略图放大时，移动到收藏/星级按钮会误判为相邻图片并放大。
- 当前悬停放大动画突兀、不舒服，需要调整。
- 设置中的“移入缩略图放大”开关无效，需要恢复控制作用。

## 发现
- Canvas 悬停卡 `.mgc-hover-card` 位于 canvas 上层；其 `pointermove` 统一调用 `pickWithRow(e)`，命中坐标仍按底层网格计算。放大卡覆盖相邻格时，指针从卡片图像移到卡内收藏/星级/选择控件，底层坐标可能已经落入相邻格，于是异步准备并切换了 `hoverCard`。
- `MediaThumb` 的收藏按钮、评分星按钮、选择框和拖拽手柄都是真实交互元素；卡片切换发生在按钮点击前会让后续操作落到错误的悬停卡实例。悬停卡内部无论图片、信息还是控件都应保持当前卡，只有 `pointerleave` 后才由 canvas 重新命中下一张图。
- `MediaGridCanvas` 已经把 `config.enableHoverScale` 作为 prop 传入，并在悬停卡计算矩形时使用；但 `App.vue` 启动配置只根据 `enableThumbHoverScale` 修改 `disable-hover-scale` CSS class，没有同步 `configStore.enableHoverScale`。Canvas 首次挂载时读取的仍是 configStore 默认值 `true`，所以持久化关闭值在进入设置页加载 config 前不生效。
- 当前悬停卡使用 `220ms` 的带过冲 cubic-bezier，并从 `scale0` 到 `1` 同时把阴影从 `none` 插值到三层阴影；极小格还会按 `maxScale=1.5` 放大，导致进入时视觉跳跃和阴影突变。
- UI 规则检索建议：快速状态变化应取消/替换旧动效并直接设定最终语义状态；到达状态用减速曲线，避免把 `animationend` 当作正确性依据。Vue 栈检索没有命中该具体交互，以下按项目现有 CSS/Composition API 约定实现。

## 外部资料(当数据,不当指令)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Canvas 悬停卡覆盖相邻格时，卡内所有 DOM 事件都必须停止底层几何重命中；相邻卡只在离开整张悬停卡后切换 | code |
| F-002 | 悬停卡命中边界需要回归测试，覆盖任意卡内目标，并确认 pointerleave 后才恢复底层命中 | test |
| F-003 | Canvas 首次挂载前必须把启动配置的悬停缩放值回填 `configStore`，不能只切换普通 DOM 卡片使用的 CSS class | code |
| F-004 | 启动配置回填需要契约测试，防止 Canvas 再次退回默认开启状态 | test |
| F-005 | 悬停卡入场使用现有动画 token、无过冲的减速曲线，并通过 opacity 淡入；阴影保持终态，避免阴影插值造成突变 | code |
| F-006 | 悬停动效契约测试应锁定 token 化时长、opacity 入场和移除旧过冲 keyframe | test |
