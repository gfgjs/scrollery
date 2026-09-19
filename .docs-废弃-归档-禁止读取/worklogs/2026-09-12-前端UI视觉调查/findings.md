---
status: snapshot
type: working-memory
line: UI-多主题系统
created: 2026-09-12
---

# 调查发现

- 存在六套主题、基础 UI 组件及主题对比度/契约检查。
- scripts/capture-theme-matrix.mjs 支持 gallery/settings/viewer 三种 harness 场景，浏览器验证不覆盖原生窗口、WebView2 和 GPU 真机路径。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 信息与评分重叠、年份越界、引导flex收缩、恢复按钮穿透错误 | UI-V-A候选；报告V01/V04/V05/V08 |
| F-002 | 后端75与前端100漂移、实际默认档小文字偏淡、焦点口径不同 | UI-V-B候选；报告V02/V03/V07 |
| F-003 | 控件尺寸分叉及覆层/设置层级/宽度的设计优化 | UI-V-C候选；报告V06及设计建议 |

## 核实与排除
- Fresh Light tint60/text75：设置说明11px约3.97，侧栏标签14px约4.08，数量13px约2.86；由computed颜色计算。
- 默认harness text100，后端schema75；显式text75浏览器验证不代表真机已测。
- 最小800×560下主工具栏折叠正常；引导13/20px不超窗，只有内部导航重叠和图标被压缩。
- 不采纳「所有硬编码媒体黑色都是主题缺陷」「按物理DPI除最小逻辑尺寸」「minimap年份被裁」等错误归因；实际裁切的是时间轴。
- 完整结论落 reviews/2026-09-12-前端UI视觉问题调查.md；全部产品优化仍待实施。
