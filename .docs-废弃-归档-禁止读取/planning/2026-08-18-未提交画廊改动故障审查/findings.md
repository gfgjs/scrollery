---
status: 已完成
type: 工作记忆
line: 未提交画廊改动故障审查
created: 2026-08-18
---

# 发现与决策:未提交画廊改动故障审查

## 需求
- 审查工作区未提交代码，说明改动内容并定位程序不可正常使用、Bug 很多的原因。

## 发现
- 当前分支为 `main`，无暂存改动；已跟踪未提交文件 54 个，另有多组未跟踪代码、测试和文档。
- 已跟踪差异约为 6,202 行新增、630 行删除；主要聚集在画廊、Canvas 缩略图、虚拟滚动、大图预览、选择/键盘/拖放和应用路由状态。
- 初始审查只读进行；用户后续授权后，仅对 `src` 执行了经过备份验证的恢复。
- 当前前端门禁均通过：`npm run typecheck`、`npm run lint`、`npm test`（151 个测试文件、1,645 项断言）和 `npm run build`。因此问题不在 TypeScript 或打包层，而在运行时集成和真实 WebView/Tauri 时序。
- 已在隔离 UI harness 复现：打开实际 `ContentViewer` 时 Vue 报“Component inside <Transition> renders non-element root node that cannot be animated”。`App.vue` 新增的 RouterView `Transition` 包住 `KeepAlive`，但 `ContentViewer.vue` 仍有 `content-viewer`、ContextMenu、多个 Dialog 等多个根节点。
- 同一复现中，点击返回画廊后 50ms 内不存在 `data-viewer-close-leaving` 元素，说明新加的 `onViewerRouteLeave` 保护/淡出路径没有获得可动画的真实元素宿主。
- 现有 `viewerCloseTransition.router.spec.ts` 用单一 `<div />` 假组件验证路由状态机；`gallerySafety.contract.spec.ts` 主要对源码字符串做 contains 断言，二者无法覆盖上述真实组件根节点和浏览器警告。
- UI harness 的普通画廊、bucket 分段滚动、侧栏开合、图像查看器翻页和框选能够完成基本操作；它不等同于真实 Tauri/WebView2，且其缺失 IPC fixture 会产生与本次改动无关的 harness 警告。

## 处置结果
- 备份已保存为 `backup/uncommitted-src-before-head-restore.patch`：76 个 `src` 路径、510,879 bytes、SHA-256 `7EEB2092B474258B46C7D9FBD37F1EF969FA970B17A539B1BA8C0D65ABAFAE7C`。
- 补丁已在隔离的 `HEAD 0020ef3` 工作树通过 `git apply --check --binary` 验证；再以反向精确应用恢复当前 `src`，避免波及 `docs`、`.github` 或其他路径。
- 恢复后 `src` 与 HEAD 完全一致；`npm run lint`、`npm run typecheck`、`npm test`（134 文件 / 1,535 项）和 `npm run build` 通过。
- 恢复后的本地 UI harness 可走通画廊→查看器→返回；此前的 Transition 多根节点警告未再出现。真实 Tauri/WebView2 仍需在目标设备复验。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | Vue Transition 的子组件必须提供单一元素根；路由动画需用实际 App + Viewer 挂载测试捕获 fragment 警告。当前已通过源回退消除，若重新引入功能则必须落地该测试 | deferred |
