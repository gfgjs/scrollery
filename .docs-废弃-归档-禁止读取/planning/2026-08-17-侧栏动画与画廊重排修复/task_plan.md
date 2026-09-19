---
status: 施工中
type: 工作记忆
line: 侧栏动画与画廊重排修复
created: 2026-08-17
---

# 任务计划:侧栏动画与画廊重排修复

> **2026-08-18 状态更正**:本计划中 route-return 宽度锁、Canvas backing-store 门控及其“已完成”复选项描述的是后来已备份并回退的未提交源码，不是当前 `HEAD 0020ef3` 的能力声明。不得从本计划的阶段 3 直接继续施工；真实 GUI 基线验收已全部通过、无源码再施工，记录见[恢复后画廊稳定化](../../worklogs/2026-08-18-恢复后画廊稳定化/task_plan.md)。

## 目标
保留侧栏滑出动画，同时避免进出大图时 KeepAlive 画廊在中间宽度连续重排而闪帧。

## 当前阶段
阶段 3:待人工 GUI 验收

## 阶段

### 阶段 1:接线与时序实现
- [x] 增加侧栏动画状态及完成信号
- [x] 路由返回期间锁住画廊实际视口宽度，结束后单次同步
- [x] 处理动画中断与无 transition 兜底
- **状态:** completed

### 阶段 2:针对性验证
- [x] 检查 TypeScript 类型与 ESLint
- [x] 核对 diff，确认手动侧栏仅冻结数据测量、路由返回才锁实际视口
- [x] 核对 route-return lock generation 与 Canvas 连续锁代次；补锁窗拒绘、旧代回调丢弃、解锁单次恢复回归
- [x] 核对 route-blocked 的外层滚动条/时间轴/minimap/辅助控制隐藏契约，错误/Retry 保持可操作
- [x] 补 route-blocked 根级/Teleport 控件 gate 与失活 drag/menu/hostActive 清理；AppToolbar 水合阻塞时退场
- [x] 收口目录滚动 stale ownership：pending id 由 FoldersSection 经 uiStore setter 登记单调代次，旧身份/finally 只清 id+代次仍匹配的请求
- **状态:** completed

### 阶段 3:人工 GUI 验收
- [ ] 在真实 Tauri/WebView2 中连续触发 manual→route-return，确认每次 route-return 都锁当前 wrapper 实际宽度
- [ ] 慢动作核对 Canvas 不按侧栏过渡中间宽度重置 backing store，解锁后只同步一次最终几何
- [ ] 记录 reduced-motion、transitioncancel/transitionend 缺失与 0×0 视口场景
- **状态:** pending（自动门禁已过，等待人工 GUI）

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 保留 `margin-left` 动画，仅冻结 MediaGrid 的宽度采样 | 用户确认保留动画；问题发生在 KeepAlive 画廊激活期间的连续重排 | D-001 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| 路由切换时直接禁用侧栏过渡 | 会让进出大图变生硬，用户体验下降 | 撤回，改为动画期间冻结画廊布局 |
