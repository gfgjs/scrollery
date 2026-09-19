---
id: 2026-07-17-status-WAL膨胀根治与启动积压驯服
status: active
type: rolling-status
line: WAL膨胀根治与启动积压驯服
created: 2026-07-17
---

# WAL膨胀根治与启动积压驯服 · 滚动状态

> 状态板 2026-08-24 自 docs/todo.md「WAL 膨胀根治评审与启动积压驯服」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## 权威文档
> worklog:[docs/worklogs/2026-07-15-WAL膨胀根治与启动积压驯服/](../worklogs/2026-07-15-WAL膨胀根治与启动积压驯服/findings.md)。性质=对用户提交诊断稿(P1–P5)的**方案合理性评审**,非直接施工;探针实测数据推翻了原诊断的核心假设。

## 状态板
| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | P1 `journal_size_limit=64MiB` 卫生封顶 | 已落地(`b24b196`);标注为通用卫生改动,非「问题修复」——原诊断的病因假设已被证伪 |
| ✅ | P2/P3/P4 逐项评审裁定 | P2(手动周期 checkpoint)**不建**——SQLite 默认 `wal_autocheckpoint` 已覆盖,前提不成立;P3(读快照生命周期审查)降级为条件触发,现有探针(P5,`48ed643`)已是哨兵;P4(resume 出关键路径)前提部分不成立(派生线 `useDerivationAutoStart` 已 3s 延迟+让步),降级为测量任务 |
| ✅ | 「WAL 膨胀致开机变慢」假设证伪 + NVMe 硬件事故蒸馏 | 生产库探针实测(WAL≤10.4MB、checkpoint 0–3ms、boot→Ready 253–407ms)证伪原假设;经验沉淀 → [experience.md](../experience.md) §13(NVMe 瞬时掉线诊断,含新增 Get-Disk/Get-Volume 判据)、§15(评审「补齐周期机制」类提案先核实编译期默认行为) |
| ⏸ | 真实慢源测量(D 盘 NVMe I/O 退化头号嫌疑) | 用户 2026-07-15 裁决暂不施工("暂时不改了");需要时:分段计时(exe/DLL 加载〔D 盘〕→ Rust boot → Ready → 前端首帧 → 画廊可交互),优先验证 D 盘 I/O 退化对 dev 冷启动的影响 |
