---
status: 快照
type: 工作记忆
line: UI-多主题系统
created: 2026-08-15
---

# 文档化工作发现

## 候选结论与落点

| 编号 | 结论 | 证据来源 | 永久文档落点 | 状态 |
| --- | --- | --- | --- | --- |
| F-001 | Kun-0.2.28 的清透感主要来自连续材质层、留白和克制的分组，不是单一颜色或 blur 参数 | Kun `base-shell.css`、`DESIGN.zh-CN.md`、memory UI 截图 | 对比分析第 4、6、8 节；永久落点为对比分析文档 | 已蒸馏 |
| F-002 | Scrollery 的 Moonlight 与 Kun 默认浅色的色相接近，主要差异是外框重复、表面更实、更密和材质作用范围更窄 | Scrollery `moonlight.css`、`AppShell.vue`、`SettingsView.styles.css`、theme-matrix 截图 | 对比分析第 5、6、8 节；永久落点为对比分析文档 | 已蒸馏 |
| F-003 | 应优先增加宿主级 material recipe，再收敛 Settings/sidebar 的重复卡片；不应先大幅改色或全局铺 blur | 两套项目的 token、布局与浮层样式 | 建议方案第 3、4、5 节；永久落点为建议方案文档 | 已蒸馏 |
| F-004 | 方案需要保留主题身份、媒体画布中性度和性能边界，并用六主题×关键场景做视觉回归 | Scrollery 六套主题、Xuan texture、现有布局 | 建议方案第 6、7、8 节；永久落点为建议方案文档 | 已蒸馏 |

## 耐久提升候选

| 候选 ID | 内容摘要 | 建议去向 |
| --- | --- | --- |
| F-001 | Kun-0.2.28 的清透感主要来自连续材质层、留白和克制的分组，不是单一颜色或 blur 参数。 | design |
| F-002 | Scrollery 的 Moonlight 与 Kun 默认浅色色相接近，主要差异是外框重复、表面更实更密和材质作用范围更窄。 | design |
| F-003 | 应优先增加宿主级 material recipe，再收敛 Settings/sidebar 的重复卡片，不先大幅改色或全局铺 blur。 | design |
| F-004 | 方案需保留主题身份、媒体画布中性度和性能边界，并用六主题×关键场景做视觉回归。 | design |

## 待验证事项

- [x] 文档中的 Kun 行号与 Scrollery 行号在落盘前再次确认。
- [x] 所有永久文档链接指向当前仓库内真实存在的文件。
- [x] 明确标注建议 token 名称为提案，不代表已经实现的 API。
- [x] 明确标注截图只能作为方向性证据，不能做像素级产品优劣结论。
