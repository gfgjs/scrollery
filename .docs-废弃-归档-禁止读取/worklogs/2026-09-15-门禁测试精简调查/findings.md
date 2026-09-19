---
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-15
---

# 发现
- 私有常规CI无改动路径分流；多个job前置cargo check；release中vite重复构建。
- 两处确证重复：isDarkColor重新导出路径复测、gallery玻璃CSS重复约束。
- FFmpeg两个测试缺环境时return，不能将通过计数当视频实测。
- 七次轻门调用合计357ms，保留低成本而有实际约束的检查。
- 完整证据见审查报告。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 当前门禁测试盘点与精简建议 | docs/reviews/2026-09-15-门禁与测试精简调查.md |

