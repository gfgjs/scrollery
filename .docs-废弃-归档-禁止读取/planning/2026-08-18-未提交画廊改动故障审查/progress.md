---
status: 已完成
type: 工作记忆
line: 未提交画廊改动故障审查
created: 2026-08-18
---

# 进度日志:未提交画廊改动故障审查

## 会话:2026-08-18
- 做了:只读确认工作树、分支、已跟踪差异统计及现有规划文档；审查 App、AppShell、MediaGrid、Canvas、虚拟滚动、viewer image 与路由水合调用链。
- 验证:`git diff --check`、`npm run typecheck`、`npm run lint`、`npm test`（151 files/1645 tests）和 `npm run build` 均通过。隔离浏览器 UI harness 验证普通画廊、bucket、侧栏开合、查看器翻页和框选。
- 发现:真实 `ContentViewer` 是多根组件，新增 RouterView `Transition` 每次打开查看器都会产生命中 Vue 的 non-element-root 警告；关闭时未出现预期的 leaving 宿主，查看器关闭保护状态机未被真实组件集成验证。
- 遗留:如需修复，先以 HEAD 为基线按功能组回退或最小化修正查看器 Transition，再在真实 Windows WebView2 复验；本会话未修改产品代码、未暂存、未提交。

## 执行:2026-08-18（用户授权建议 1、2）
- 做了:生成 `backup/uncommitted-src-before-head-restore.patch`，包含 76 个 `src` 路径（23 个原未跟踪路径），并在隔离 HEAD 工作树通过可应用性验证。
- 做了:以该补丁反向精确应用恢复 `src`；恢复后 `git status --porcelain -- src` 为空，`src` 与 `HEAD 0020ef3` 完全一致。未触碰 `docs`、`.github` 或其他工作区内容。
- 验证:`npm run lint`、`npm run typecheck`、`npm test`（134 files/1535 tests）和 `npm run build` 均通过。浏览器 UI harness 的画廊→查看器→返回通过，未再记录 Transition 多根节点警告；本地 Vite 服务和测试标签页已关闭。

## 回顾
- 亮点:先验证补丁可应用、再反向精确应用，避免粗粒度清理未跟踪文件。
- 教训:源码字符串契约和假路由组件不能替代真实 App + Viewer 的浏览器集成测试。
- 意外:全部静态门禁在坏版本也能通过，说明运行时界面的测试缺口比预期大。
