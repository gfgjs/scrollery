---
status: snapshot
type: working-memory
line: UI-多主题系统
created: 2026-09-16
---
# 发现：玻璃侧栏色差补修

- 用户截图：Mica/Acrylic分组横条与工具行色差明显。
- glass.css中acc-header常驻blur(12px)，工具行hover用30/34%主题表面染色；原生合成是否是白条直接原因尚未实测，不将推断记为事实。
- 工作区存在其他任务的Rust schema、useRenderMode等修改；本次只改授权UI文件。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|

