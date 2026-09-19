---
status: 施工中
type: 工作记忆
line: Kun-0.2.28与Scrollery-UI对比重做
created: 2026-08-14
---

# 发现与决策:Kun-0.2.28与Scrollery-UI对比重做

## 需求
- 用户更正参考项目为 `C:\workspace\Kun-0.2.28`。
- 之前以 `Kun-master` 为对象的对比结果全部废弃；本次不得引用其记忆、结论或证据。

## 发现
- F-001 比较范围已重新建立：Kun 只取 `C:\workspace\Kun-0.2.28` 的 `src/renderer/src`、设计文档和该目录内截图；Scrollery 取当前工作树。旧的 `Kun-master` 目录、旧结论和旧证据不参与本次分析。
- F-002 Kun-0.2.28 的真实浅色基线是材质化浅蓝：`--bg-app:#f3f5fc`、`--bg-sidebar:#eef2fa`、`--bg-canvas:#fafbff`，卡片/浮层是白色 `rgba(...,0.90/.98)`，并配合半透明边框、shell/panel/composer 阴影、topbar/sidebar 渐变和 body 全局 glaze。相关 token 集中在 `src/renderer/src/styles/base-shell.css` 顶部，入口由 `src/renderer/src/main.tsx` 统一导入。
- F-003 Kun 的 blur 不是唯一来源：`ds-frosted`、菜单、时间线预览、计划面板等浮起 surface 使用局部 blur；普通页面的清透感主要由“近色 canvas + 半透明白面 + 渐变/光泽 + 低饱和蓝色 accent”共同形成。默认 UI mode 是 `default`，iKun/Retroma 是额外 mode，不应把它们混为默认基线。
- F-004 Scrollery 的 Moonlight 浅色变量与 Kun 基线在色相/明度上已经接近：`--color-bg-primary:#f4f8f9`、`--color-bg-canvas:#f3f5f6`、surface 为纯白、elevated 为 `#eaf1f4`、accent 为 `#2177b8`。因此单纯重调主题色，预计不能复制 Kun 的清透感。
- F-005 表面组合是主要差异：Kun 的代码和截图更倾向于一个大的连续内容面，内部用细分隔线、少量统计卡和少量强调控件；Scrollery 的设置页、toolbar、sidebar tool card、popover 和输入控件更常各自拥有背景+边框+圆角，设置页又重复铺设多条 elevated accordion card，画面被切成更多块。
- F-006 全局材质范围不同：Kun 在 body、stage、topbar、sidebar、card、composer 级别定义透明度/渐变/haze/elevation token；Scrollery 的主题文件只覆写语义颜色变量，几何形状与容器 recipe 留在全局 `index.css`、页面样式和组件 scoped CSS 中。六个主题因此共享同一套卡片化结构，换主题不会换成 Kun 的表面组合。
- F-007 信息密度与产品角色会放大差异：Kun-0.2.28 的 memory/settings 截图以大标题、大块留白、单一主卡和少量操作色为主；Scrollery 的截图同时展示图库、筛选 chip、媒体徽章、工具卡、文件夹树和 statusbar。Scrollery 的业务密度不是纯样式问题，但当前容器边界数量让它显得更厚。
- F-008 排版不是首要差异。Kun 使用系统/CJK 优先字体栈并有 11–16px 的 UI 节奏与更大的 hero/title；Scrollery 使用打包的 Inter Variable、16px body 基线和 11–30px token 梯度。两边都有合理的字号 token，真正的感知差别主要来自留白、内容分组和表面连续性。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 以“连续材质 recipe”而非“再做一套主题色”作为 Scrollery 借鉴 Kun 的主线。 | 若后续实施，落到 `docs/designs/` 的 UI 设计决策；本次不改实现 |
| F-002 | 建立 canvas/chrome/surface/elevated 的透明度、渐变、边框和 elevation 契约，并限定 blur 只服务于真正浮起的层。 | 同上 |
| F-003 | 减少设置/工具区域的重复外卡，使用大容器+细分隔线承载同组内容；主题个性继续保留。 | 同上 |
| F-004 | Moonlight 现有色板已接近 Kun，优先验证 surface composition 和内容密度，不先改基础色。 | 同上 |
