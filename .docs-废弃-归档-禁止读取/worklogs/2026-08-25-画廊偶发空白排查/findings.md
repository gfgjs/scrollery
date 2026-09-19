---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-25
---

# 发现与决策:画廊偶发空白排查

## 需求
- 画廊偶发不显示任何内容、滚轮滚动后恢复显示；问题无法稳定复现但已多次发生；需查明原因。

## 发现
- 受影响范围是“画廊渲染引擎（实验）= Canvas”。默认 DOM 模式不创建 `MediaGridCanvas`，因此不经过该 rAF 调度链。
- `MediaGridCanvas.vue` 的 `onMounted()` 会先调用 `scheduleDraw()`，排入 generation 0 的 rAF；同组件位于 KeepAlive 子树，Vue 实测生命周期顺序为 `mounted -> activated`。
- 紧随的 `onActivated()` 调用 `renderLifecycle.activate()`，generation 变为 1，再调用 `scheduleDraw()`；但 `rafId` 仍指向旧帧，新请求被合并掉。
- 旧 rAF 执行时，generation 0 已不再允许绘制，回调直接返回；没有任何当前 generation 的 rAF 留下，故 `draw()` 从未执行。滚动使 `currentY` 改变，watcher 再调用 `scheduleDraw()`，才恢复显示。
- 缩略图请求失败不是主因：Canvas 单元在无位图时仍绘制占位底色；而持续 0 尺寸也不符合“仅滚动就恢复”，因为滚动 watcher 不会重新测量。
- 现有 `mediaGridCanvas.helpers.spec.ts` 覆盖生命周期代次类，但未覆盖 rAF 合并与 mounted→activated 的组合；定向运行 62 项测试全部通过，不能发现本竞态。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Canvas 激活必须保证当前 generation 至少有一个有效 rAF；并用组件级回归测试覆盖 mounted→activated 后旧 rAF 作废的序列。 | todo |
