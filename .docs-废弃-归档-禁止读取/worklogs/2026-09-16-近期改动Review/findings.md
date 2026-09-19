---
id: 2026-09-16-近期改动Review
status: snapshot
type: working-memory
line: 仓库架构与流水线全面梳理
created: 2026-09-16
---

# 发现

## 范围
对功能性 diff 审查，纯换行和第三方大段删除按调用引用与交付资源闭包核对。

## 待复核与待确认
尚无。

## 耐久提升候选
收口时以审查报告为统一落点。

## 已确认并直修
- 安装包脚本实际 legalSources 漏 ADDITIONAL-PERMISSION.md；补一项映射。用当前源码/HEAD 对照执行内置 selftest 和缺项 fixture，原先 NSIS/MSI 漏报，修后两路命中。
- useSelectionBarMode 的停靠/对齐 setter 丢弃 rejected Promise；与相邻 setter 统一 catch，中央错误提示仍生效。21 测试通过，失败注入零 unhandledRejection。
- CloseConfirmDialog 常驻组件的 flushFailed 跨次打开残留；子代理添加打开时复位。主会话通过真实 SFC setup + Vue 响应性复现修前/修后行为。

## 待确认候选
- publish-oss.ps1 的无内容变化分支直接 return；相同公开 clone 先 dry-run 或 push 失败再以 -Push 重试时，已有本地提交不会推送。代码分支已核实，尚未模拟真实远端。应确认脚本是否需要支持复用 clone 的发布重试（当前 CI 每次重建 clone 不触发）。

## 排除的伪问题
- 重置时旧代次防抖批次 resolve 属已有取消契约，调用方没有把它当用户可见的保存成功，无需用户额外裁决。
- ContentViewer/ReaderSettingsSection 的通常失败回滚有中央乐观值与确认值恢复，未按缺陷上报。
## 最终审查结论
- 画廊组完成，11个spec共216项通过；其中19项重跑不重复累加。已确认打码开启后支持态消失会藏掉按钮，主会话把修复放到AppToolbar可见性条件，保留真实能力标志。其余滚动修改未确认新缺陷。
- 新增Q2：Canvas打码开启后切换/回退DOM会暴露真实画廊内容；现有范围排除DOM，按用户要求记录待取舍，不扩展渲染实现。

## 耐久提升候选表
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 31提交审查覆盖、4处修复、验证与限制 | reviews/2026-09-16-近期改动Review.md |
| F-002 | Q1公开快照原地重试与Q2打码回退决策 | 当前线状态分片；Q2转演示打码分片 |

