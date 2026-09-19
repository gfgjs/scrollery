---
status: snapshot
type: working-memory
line: 主页Canvas性能优化
created: 2026-07-13
---

# 进度日志:主页Canvas性能优化

## 会话:2026-07-13
- 做了:确认工作树现状，读取 planning 规范和既往 Canvas 画廊审计线索，建立任务三件套；完成两个 Canvas 模块的热路径分析并确定二分可见窗口、静态轴层缓存和中间位图释放三个优化点。
- 做了:画廊可见行/命中改为二分定位；修复异常路径中间 `ImageBitmap` 释放；右侧轴拆为离屏静态层与逐帧动态层；视窗拖拽 pointermove 合并到 rAF 并阻断重复冒泡；代码提交为 `a5d8e99`。
- 验证:`npm.cmd test` 通过 69 个文件、843 个测试；`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run build` 全部通过；浏览器 harness 验证双 Canvas 挂载、folder 切换、滚动及 hover/loupe，无 Canvas 相关控制台错误。
- 遗留:产品级性能结论仍需在真实大图库以现有 perfProbe 对比 Canvas/DOM 的 FPS、帧耗时和 `selectEnterMs`；本次没有宣称小型 harness 能替代该真机基准。

## 回顾(收口时填)
- 亮点:把复杂度验证写成 8192 行 Proxy 访问计数测试，避免依赖易波动的时间基准；轴缓存明确分离静态和动态失效源，滚动帧成本不再随分组总数增长。
- 教训:Canvas 消除 DOM 节点不等于自动消除 CPU 工作；全量 path 重放、线性命中和异常路径 bitmap 生命周期仍需单独审计。
- 意外:browser harness 的 folder 模式仍消费固定日期 summary，并暴露未覆盖 `get_directory_ancestors` 的测试设施缺口；应避免把该样例误当真实大文件夹轴基准。
