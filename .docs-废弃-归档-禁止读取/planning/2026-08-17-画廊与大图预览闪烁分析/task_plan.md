---
status: 施工中
type: 工作记忆
line: 画廊与大图预览闪烁分析
created: 2026-08-17
---

# 任务计划:画廊与大图预览闪烁分析

> **2026-08-18 状态更正**:阶段 6–12 中关于 gallery-ready 屏障、route-return 几何锁、Canvas 生命周期、查看器关闭 Transition 与冷启动 layout shell 的“已完成”实现均属于后来已备份并回退的未提交源码；它们只能作为历史取证和候选设计，不能作为当前 `HEAD 0020ef3` 的能力声明。不得从阶段 12 继续施工；真实 GUI 基线验收已全部通过、无源码再施工，记录见[恢复后画廊稳定化](../../worklogs/2026-08-18-恢复后画廊稳定化/task_plan.md)。

## 目标
定位大图关闭回画廊与大尺寸图片首次/换图后重进闪烁的具体时序和根因，给出不改代码的分层解决方案、取舍与验证标准。

## 当前阶段
阶段 12:冷启动布局反馈修复（自动门禁完成，待 Windows WebView2 GUI 验收）

## 阶段

### 阶段 1:并行取证
- [x] 还原画廊与大图预览的组件、状态和 DOM 生命周期
- [x] 还原关闭大图时的帧级可见性与布局时序
- [x] 还原大尺寸图片加载、解码、GPU 上传、ICC/色域处理和缓存时序
- **状态:** completed

### 阶段 2:根因裁决
- [x] 区分已证实事实、强推断和待实测假设
- [x] 判断三个现象是否共享根因
- **状态:** completed

### 阶段 3:方案设计
- [x] 给出最小修复、推荐方案和可选增强
- [x] 定义动画契约、资源交接和竞态处理
- **状态:** completed

### 阶段 4:验证设计
- [x] 给出自动化、性能标记和人工慢动作验收方法
- [x] 列出实施顺序、风险和回退点
- **状态:** completed

### 阶段 5:分析交付
- [x] 汇总为可直接进入实施阶段的方案
- **状态:** completed

### 阶段 6:画廊返回链路施工与验证
- [x] 锁住 route-return 期间画廊 wrapper 的实际几何宽度，并以 flex-basis/min/max 约束与帧级几何读数核验
- [x] 门控 bucket ResizeObserver/调度，保留失活段并在非零视口恢复；停止 Canvas 失活 RO/rAF 与 0×0 backing-store 写回
- [x] 定义 gallery-ready 四分片信号，接入视口稳定、滚动恢复、bucket 可绘段与 Canvas 首次有效 draw
- [x] 补 characterization/unit 测试并完成定向/全量验证
- **状态:** completed

### 阶段 7:关闭大图过渡施工与验证
- [x] 将查看器到非查看器的所有关闭入口收敛到同一导航守卫并阻断重复导航
- [x] 使用默认 simultaneous enter/leave 保留离场查看器层，gallery-ready 后淡出且支持 reduced-motion
- [x] 增加 transitionend、ready 超时、导航失败与焦点/pointer/inert 兜底测试/实现
- [x] 完成定向与全量前端验证，并保留真实 Tauri/WebView2 人工验收缺口
- **状态:** completed

### 阶段 8:终审竞态修复与门禁
- [x] 关闭导航 afterEach 仅收尾原始目标的真实 aborted/cancelled，补重复/旁路导航 characterization
- [x] 移除 ready timeout 伪造路径，改为保留旧查看器 shield 的安全失败；四分片绑定 close generation/view/layout token
- [x] 延后空布局/深链恢复 ready，Canvas 首绘只承认当前可绘行，图片交接补 transitionend 缺失兜底
- [x] 完成定向/全量 Vitest、typecheck、lint、build 与 diff-check；记录 npm reporter 转发缺口和 GUI 人工缺口
- **状态:** completed

### 阶段 9:最终行为竞态修复与全门禁
- [x] 关闭导航 attempt FIFO 所有权：同目标替代导航的旧 cancelled 不得提前释放当前 generation；补 createMemoryHistory 集成回归与旁路 guard 拒绝回归
- [x] Canvas ack 绑定 close/view/layout 成功 epoch/rows/session 五元 identity；旧 draw 被拒后当前 rows settle 可重试
- [x] mediaStore 建立 latest-request status/成功 epoch/watchdog；失败、取消、timeout 清空旧 summary，MediaGrid 不消费旧 rows
- [x] restoreGalleryAfterActivation 等待 updateVisible、nextTick/rAF 与当前 DOM/bucket paint；Canvas/bucket 不以非零尺寸或旧段提前 ready
- [x] ready timeout 采用持久 shield fallback：释放旧 viewer leave/导航，真实四分片到齐后撤 shield；补失败/恢复回归
- [x] 建立 activation generation/barrier：route hydration→layout success→scroll restore→当前 rows DOM/bucket/canvas paint ack，迟到 promise/fetch/callback 全部按身份丢弃
- [x] route collection/person 异步 hydration 进入 barrier；旧 view summary/rows 不得释放 shield；bucket 保留段只可复用、不可作为新激活 ready 证据
- [x] AppShell fallback 只在 afterEach 成功且 current route 符合、目标真正变化时清理；aborted/cancelled 保持 token/barrier/shield；fade 160ms 内 view/filter/layout 换代会撤 ready 并重新屏障
- [x] 完成定向 lifecycle/router 回归、全量 Vitest、typecheck、lint、build、diff-check，并记录真实 GUI 未覆盖项
- [x] P1 补强：restore failed/timeout 同 activation retry，bucket/Canvas 只接受当前 readyVersion/current fetch identity，route hydration failed/identity/retry 与 route-blocked 错误态，sidebar transition completion 门控
- [x] 新增 activation barrier、route entity resolution、retained bucket rows 回归；真实 SFC/Tauri/WebView2 GUI 仍未覆盖
- **状态:** completed

### 阶段 10:P1/P2 收口修复与全门禁
- [x] fallback shield 改为不透明、可访问、可操作的状态面板；Retry 通过当前关闭代次信号交给 MediaGrid activation barrier，成功仍只由四分片真实合取撤 shield
- [x] AppShell afterEach 接入 `isExpectedViewerCloseFailure` 与 attempt FIFO；无替代的原始 aborted/cancelled 恢复 viewer closing/token/inert/focus，same-target replacement 与旁路 guard 拒绝不提前清理
- [x] 侧栏 transition 增加 kind/generation 监听；route-return 每代锁当前 wrapper，Canvas/resize 在锁窗口明确门控，补 manual→route-return 回归
- [x] 完成定向/全量 Vitest、vue-tsc、lint、build、diff-check；记录 npm `EUNKNOWNCONFIG` 与 GUI 未自动覆盖
- [x] 最终残余：Canvas 锁监听 route-return generation，连续锁代次取消旧 RO/rAF，解锁只做一次最终 measure/重连/补绘；route-blocked 隐藏完整画廊可见/交互表面；fallback shield 改完全不透明黑并在成功收尾可靠恢复焦点
- [x] 为上述四项补 lifecycle 与渲染静态契约回归；本轮仍不伪造真实 GUI 结论
- [x] 最终 P1：非 bucket Canvas 当前 draw identity/新 rows proof；route-blocked 根级与 Teleport gate、失活 drag/menu 清理；AppToolbar 水合阻塞退场
- [x] 最终 P1/P2 命令与 bucket 收口：失活/激活刷新撤销 loading fetch ownership 并允许当前代重取；`galleryRouteBlocked` 贯通 command context、ContextualToolbar 与 AppShell F11/undo/redo
- [x] 目录滚动定位 P2 收口：为 `pendingScrollDirId` 增加单调请求代次；stale/失活/finally 仅清理仍归当前请求所有的 id+代次，覆盖同目录新请求与回到原身份重触发
- **状态:** completed（仅剩人工 GUI）

### 阶段 11:人工 GUI 验收
- [ ] 真实 Tauri/WebView2 中验证 fallback shield 的可见状态、Retry/返回、焦点与未就绪画廊不泄露
- [ ] 验证真实 guard/lazy-load aborted 后 viewer 可重试关闭，inert/focus 恢复
- [ ] 慢动作验证 manual→route-return generation、wrapper 几何锁与 Canvas backing store 不闪
- **状态:** pending

### 阶段 12:冷启动布局反馈回归审查
- [x] 复核 2026-08-17 至今工作树差异，确认 `computeLayout()` 发起时清空 summary 是相对 HEAD 的关键新增行为
- [x] 还原 `summary → totalRows → bucket/axis 外壳 → ResizeObserver → onResize → compute` 的无交互反馈环，并与实测症状交叉确认
- [x] 识别尺寸优先 resetKey 的次级自驱重算风险与扫描期的持续重算放大器
- [x] 在不泄露旧视图数据的前提下实施稳定外壳/当前布局状态分离；bucket/原生滚动条与轴槽在 pending 期不再随 `totalRows` 变形
- [x] 将尺寸优先 resetKey 收窄为 view/route/activation identity，移除 layoutVersion/successEpoch 自驱维度
- [x] 补 store、双引擎、轴槽与用户轴模式 pending 意图定向回归，并完成全量前端门禁
- [ ] 在真实 Windows WebView2 中验证 5 秒内布局请求与观测宽度收敛
- **状态:** pending（自动化完成，等待 GUI）

### 阶段 13:普通 view 换代陈旧表面核证（后续，非 P0 阻塞）
- [ ] 核实 view/filter identity 变更与 `useJustifiedLayout` post-flush compute 同一 tick 时，是否可能短暂显示旧 `visibleRows`
- [ ] 若可复现，以同步 layout-ready gate 同时约束行表面和 viewport metadata 调度，并补回归
- **状态:** pending

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 首轮分析阶段只出方案；后续按用户授权对确认残余做最小施工 | 初始需求先要求分析，后续验收明确授权修复 Canvas/route-blocked/shield/focus 残余 | |
| 并行排查交给 Luna（Max）子代理 | 用户明确指定其余工作使用 Luna（Max） | |
| 关闭闪先修交接时序，再加动画 | 动画只能遮蔽空帧，不能替代画廊宽度/Canvas 生命周期修复 | D-001 |
| 大图采用可见层双缓冲/占位保留，而非扩大常驻大图缓存 | 解决冷/热缓存差异造成的跨帧暴露，同时避免多张 46MiB 解码图常驻 | D-002 |
| 不恢复 `mode="out-in"` 路由过渡 | 该模式天然先退后进会制造空窗；历史版本另有 KeepAlive 黑屏回归 | D-003 |
| route-return 宽度锁挂在画廊 wrapper 并同时钉住 `flex:0 0`、`min/max-width` | 仅给内部滚动节点写 width 不能约束父级 flex 的实际主轴几何；wrapper 才是侧栏回流的有效边界 | D-004 |
| gallery-ready 只由四个真实分片合取，不以 onActivated 单点代替 | 视口、滚动恢复、bucket 段和 Canvas 首绘各自可能异步，合取才不会把空交接暴露给后续关闭动画 | D-005 |
| 关闭交接用路由守卫登记原始目标，App.vue 默认 simultaneous enter/leave 层消费 ready 后淡出 | ESC、按钮、命令、back、侧栏与深链都共享同一门控；不使用先退后进语义，也不让进入查看器重播动画 | D-006 |
| ready timeout 只保留旧查看器 shield，不把未完成四分片伪造成 ready | 超时无法证明 viewport/scroll/bucket/canvas 真正完成；安全失败应继续遮挡并等待真实 ready 或下一次重试 | D-007 |
| gallery-ready 回调同时校验 close token、route/view identity 与 layoutVersion | KeepAlive 旧段、旧 bucket readyVersion、旧 Canvas draw 可能迟到；单一布尔信号不足以区分本轮交接 | D-008 |
| 将“当前布局是否可消费”与“画廊外壳几何是否稳定”分离 | `layoutSummary=null` 用于阻止陈旧数据进入当前 ready，但 `totalRows` 同时驱动 bucket、原生滚动条和轴侧栏，不能在 pending 期直接充当外壳开关 | D-009 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
