---
id: 2026-07-24-status-RAW等格式照片支持
status: active
type: rolling-status
line: RAW等格式照片支持
created: 2026-07-24
---

# RAW等格式照片支持 · 滚动状态

> 状态板 2026-08-24 自 docs/todo.md「RAW 等格式照片支持线」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## 权威文档
> **进行中未登记(2026-07-24)**:**RAW 等格式照片支持线**——参考 PSD exotic 插件架构,为 CR2/CR3/NEF/ARW/DNG/RAF/ORF/RW2/PEF/SRW 等相机原始格式接线(builtin distribution + 桌面 sidecar worker + LibRaw CDDL 嵌入预览)。阶段 A(P0 过渡体验)/B(P1 raw-probe)/C(P2 raw-worker + 宿主接线,含 C-2b-prod tauri externalBin 打包链)**全部完成**,prod 打包链闭合(commit fb5062f+c954865)。剩真机/CI tail(CI gnu job/release 实跑取证/样张厂商实测/mac-Linux 交叉编译/GUI 真机验收)与阶段 D/E 二期未做。按 /planning 纪律追踪于 [docs/worklogs/2026-07-24-RAW等格式照片支持/](../worklogs/2026-07-24-RAW等格式照片支持/task_plan.md),收口后正式回写本文。

## 状态板
(本线为叙述式状态,无表格;现状全文见上「权威文档」;开放行:真机/CI tail + 阶段 D/E 二期,均随原文保留。)
