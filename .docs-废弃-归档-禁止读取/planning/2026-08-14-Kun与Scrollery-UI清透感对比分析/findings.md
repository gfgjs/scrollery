---
status: 施工中
type: 工作记忆
line: Kun与Scrollery-UI清透感对比分析
created: 2026-08-14
---

# 发现与决策:Kun与Scrollery-UI清透感对比分析

## 需求
- 用户认为 Kun-master 的 UI 特别清透，整体优于 Scrollery 现有主题，希望分析并总结两者差异。

## 发现
- F-001 架构差异：Kun 把主窗口的表面、边框、层级阴影和主题模式集中在 `src/renderer/src/styles/base-shell.css` 与 `neutral-polish.css`，组件大量复用 `ds-*` 语义 token；Scrollery 的主题变量集中，但组件仍有大量局部 `<style scoped>`，同一类 surface、border、radius、shadow 在页面/组件层重复表达。
- F-002 清透感的第一来源是低噪声的表面关系，而不是单纯的透明度。Kun 默认 host 使用接近同色的 canvas/sidebar/surface，边框很轻，普通 shell/card 默认无阴影；Retroma 模式再通过低不透明度 surface、渐变和 haze 提供玻璃感。Scrollery 的 Moonlight/Porcelain 虽然浅色，但 canvas、surface、elevated、inset、border、shadow 的层级同时出现，许多区域被卡片化，视觉分段更多。
- F-003 颜色策略不同：Kun 的设计契约是“中性 chrome + 单一 accent 只服务于可操作状态”，当前默认也是低饱和灰纸色；Scrollery 的主题系统更强调各主题的身份色，Moonlight/Porcelain 仍带蓝灰底色、蓝/紫 accent 和多组状态色。前者让视线先读空间层级，后者让视线更快读出组件边界和状态。
- F-004 密度差异会放大样式差异：Kun 设计文档明确限制 UI 字号节奏、使用 4px 间距和较大留白；Scrollery 全局 body 是 16px/1.5，工具栏、侧栏工具卡、设置卡、徽章和筛选控件同时可见。Scrollery 作为资产管理器需要承载更多筛选、树、状态和网格信息，这个产品角色差异也会让它天然比 Kun 的设置/工作台截图更“满”。
- F-005 主题作用域不同：Scrollery 的设计明确规定“theme = semantic variable overrides”，主题主要换颜色与阴影，不改变组件形状和布局；Kun 的 UI mode 会整体切换 host 的 material recipe，Retroma 还改变 surface 透明度、渐变、haze 和 elevation。因此 Scrollery 即使换主题，也不会自动获得 Kun 那种“同一套表面组合方式”。
- F-006 形状与层级：Kun 遵循“柔和但不过分圆”，用有限的 control/card/pill 半径和三档 elevation，普通元素更依赖留白与分隔线；Scrollery 的 settings-card、popover、toolbar search、sidebar tool card 等都同时使用填充、边框、圆角、阴影，组件边界更强，容易产生“一个卡片套一个卡片”的感觉。
- F-007 证据截图方向一致：Kun 的 memory UI 截图呈现大块近色背景、单一主卡、少量强调色和较长的空白；Scrollery 的 Moonlight 设置/图库截图呈现更多白色容器、浅蓝块、边框、工具条芯片和状态徽章。截图所处业务页面不同，不能做像素级结论，但足以支持“层级数量与信息密度”是主要感知差异。

## 初步结论
- “清透”可拆成：低色相噪声 + 受控对比度 + 少量表面层级 + 足够留白 + 统一的排版/间距节奏。Kun 在这五项上都更一致；Scrollery 目前主要完成了颜色主题化，尚未把表面组合和密度收敛成一套清透 host recipe。
- 最值得借鉴的不是把 Scrollery 做成更多透明玻璃，而是先让 chrome 中性化、减少普通容器的边框/阴影、统一三档 surface/elevation，再把透明和 blur 限定给顶栏、弹层、编辑器等少数真正需要浮起的区域。
- Scrollery 的 canvas 中性原则仍应保留：资产内容需要不被主题色污染。因此应优先改 chrome、设置和工具容器，不应把强色渐变或纹理铺到图片展示画布。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Kun 的优势是 host-level surface recipe 与低噪声层级，不是某个单独的主题色；Scrollery 当前是 palette-first，需将“清透 host”从主题身份中抽离。 | 若后续实施，落到 `docs/designs/` 的 UI 设计决策；本次仅作分析，不直接修改实现 |
| F-002 | 减少普通容器的边框/阴影/填充叠加，建立 canvas/chrome/surface/elevated 三档统一契约。 | 同上 |
| F-003 | 保留各主题颜色个性，但把默认浅色 chrome 收敛到近色中性；accent 只出现在选中、主操作和状态上。 | 同上 |
