---
status: snapshot
type: working-memory
line: AI-人脸流水线深审
created: 2026-09-16
---

# 发现：AI模型缺失引导

- 用户日志指向 worker_pipeline.rs 的派发失败；worker_client/session.rs 在构造模型描述时检查文件。
- aiStore 已对 start/restart IPC 拒绝展示 toast，但后台失败发生在 IPC 成功之后，无法触发此处理。
- 侧栏和 SemanticSearchPanel 共用 aiStore 分析入口，可集中处理下载引导。
- 使用现有 variant_installed 判断五项必需资产；前端按 AiModelNotLoaded 分流，确认后跳转 /settings/ai。
- active_profile 已迁 ConfigManager；ai_commands 的旧注释仍提读池 SQL，本次依据当前实现判断。

## 耐久提升候选
本次根因和方案以 D-001 一并记录，无独立候选。
