---
status: 快照
type: working-memory
line: 全仓深度review与直修
created: 2026-07-24
---

# 任务计划:全仓深度review(第二轮)

## 目标
在 [[全仓深度review与直修]] 线(2026-07-23 首轮 9 commit 03cd68c..cc88f8c 收官)之后,针对基线新增变更面(9493e07 起,含 c20af3f 施工规格集/bff70ac onboarding/e1b1789+8b95854 gallery 轴重构等)再做一轮全仓深度 review:无分叉直修 + 拿不准项列裁决,委派 orchestrate 编排执行。

## 当前阶段
收官:6 维度并发复核 + 4 项直修全部落地(C1-C4),门禁六面绿。余仅 GUI 真机验收 + 待批 push + H 项(error.rs 定向脱敏)独立立项未开工。

## 范围
- Rust 后端:`src-tauri`(183 文件/26 模块)
- Vue 前端:`src`(439 文件/18 目录)
- 深扫面:变更面(9493e07 起)差量复核 + 全仓架构轮扫

## 复核维度(6 路并发)
1. Rust async/并发
2. DB/SQL
3. 路径安全/IPC/CSP
4. Vue delta(近期变更面)
5. Vue 架构/TS
6. Rust 错误处理/架构

## 委派编排
scout×1 + reviewer(opus×4/sonnet×2)+ 增量核验 resume×3 + implementer×3 + phase-closer×3(首发翻车 1 次重派)。
