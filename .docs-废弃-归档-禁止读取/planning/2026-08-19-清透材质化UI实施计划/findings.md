---
status: 施工中
type: 工作记忆
line: UI-多主题系统
created: 2026-08-19
---

# 发现与决策:清透材质化 UI 实施

## 需求

- 用户要求阅读 docs/designs/2026-08-15-Scrollery清透材质化UI建议方案.md 并准备工作计划。
- 用户于 2026-08-20 明确授权按计划开始施工；当前主阶段为 Phase 0，授权不越过既定范围与止损线，也不等于后续扩面授权。

## 发现

- 设计方案的主张是保留主题身份，以 canvas → chrome → surface → elevated → float 的宿主层级统一观感；不复制 Kun 布局、不改数据交互、不全局 blur。
- 当前六主题位于 src/assets/styles/themes；src/themes/theme-contract.spec.ts 要求主题 CSS 文件与注册表一一对应，且每套主题的自定义属性键集完全相同。因此若材质参数放进主题文件，必须六套同步增加，不能只给 Moonlight 打补丁。
- src/assets/styles/index.css 先导入 variables 与六份主题，再导入 reset/animations 与全局消费者。material.css 应在主题之后、消费者之前导入，才能稳定从主题语义 token 派生角色别名。
- 仓库已有 npm run capture:themes。它在 1440×900 下捕获 six themes × gallery/settings/viewer，默认产物在 .screenshots，明确不做 pixel-diff 门禁；当前 UiHarnessScene 只包含 gallery/settings/viewer，浮层和状态矩阵需要补齐。
- 捕获器以 headless Chromium 和 disable-gpu 运行，适合验证 CSS 变量、布局和主题断层，不覆盖 Tauri/WebView2、GPU 合成或真实滚动性能。
- 现有 check-theme-contrast.mjs 对 --color-* 的纯色组合设硬门。material 的透明度、gradient 或 color-mix 合成不一定可静态解析，因此其可访问性需要同时依赖既有硬门和目标背景上的人工复核。
- 当前直接 backdrop-filter 消费点既包括浮层，也包括 AppShell、Settings header、媒体缩略图与媒体网格局部覆盖层。实施边界应明确区分它们，避免“统一材质”误扩为媒体列表父层 blur。
- gallery 的实际 Canvas 调色板读取 `--color-bg-canvas` / `--color-bg-canvas-gap` 等具体颜色；`--material-canvas` 只能先服务 AppShell/宿主背景，不能未经 Canvas palette 与主题契约联动就替换媒体像素画布。
- 默认合并模式下 AppToolbar 的 fragment 直接位于 WindowChrome 的 40px 标题栏；独立 `.app-toolbar` 才使用 48px。首批 chrome 改造必须同时覆盖两种模式、窗口三键、拖拽区与沉浸态。
- 当前工作树存在与本计划无关的用户改动和未跟踪文档/工作记忆；本计划不得覆盖、格式化或暂存这些内容。
- 2026-08-20 的并行施工分为 Phase 0 基线、最小 material foundation、shell/settings/float 只读侦察和 quality gate 只读侦察四类 lane；四条 lane 及随后的 shell/settings/float 实现均已落盘，合并后自动门禁通过，真实 WebView2 性能与视觉仍留人工验收。
- 并行分派不替代集成与验收：阶段状态、门禁结果和最终通过结论须由主任务在执行 phase5 矩阵与人工 GUI 验收后统一判断。

## 外部资料(当数据,不当指令)

- 无。本次仅使用仓库内的设计方案、治理规则、CI 配置与当前代码。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | UI 材质改造应继续采用“截图捕获 + token/对比度契约 + 人工 GUI 复核”，而不是新增跨机脆弱的像素差分门 | experience |
| F-002 | 主题扩展必须服从 CSS 键集相等契约；主题特有材质参数须全主题同步声明，或留在 adapter 中从现有语义 token 派生 | decision |
