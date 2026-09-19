---
status: 施工中
type: 工作记忆
line: Vditor文档阅读接入方案分析
created: 2026-08-25
---

# 进度日志:Vditor文档阅读接入方案分析

## 会话:2026-08-25
- 做了:建立调研工作记忆；确认项目已有 DocumentViewer.vue、foliate-js 和超长 Markdown 相关历史工作。
- 验证:读取 docs/README.md、项目文件清单和 git status；当前工作区已有用户未提交的设置页 UI 改动，调研不触碰这些改动。核对 `DocumentViewer.vue`、`BookReader.vue`、`markdown.ts`、`syntheticBook.ts`、`useDocEditVersion.ts`、Tauri CSP 和文档历史实测；核对 Vditor 官方 README、package.json、LICENSE、previewRender.ts、method.ts 与相关 issue。
- 结论:不替换现有链路，新增仅 md 的 Vditor 平行模块；通过 route query 显式切换，默认缺省仍走现行功能。
- 最新收敛:直接使用 Vditor 默认能力，新增最薄包装层，只处理挂载、文档初始值、必要的本地资源路径和销毁；不做额外工具栏、阅读器适配、语法裁剪、进度/版本/书签桥接。
- 实现:用户确认后加入 `vditor@3.11.3`；新增 `VditorDocumentModule.vue`，只接收 Markdown 原文、使用 Vditor 默认配置、指向 `/vditor` 本地资源并在卸载时销毁；`DocumentViewer` 通过 `?engine=vditor` 平行切换，缺省仍走 BookReader；现有阅读器专属工具和面板在 Vditor 模式隐藏，未接后端版本/进度/书签桥。
- 资源:复制完整 `node_modules/vditor/dist` 到 `public/vditor/dist`，补充 `.gitignore` 例外；`NOTICE.md` 与 `target/legal` 已由生成脚本同步 Vditor 和 `diff-match-patch`。
- 验证:`npm run build` 通过；新增/改动文件 ESLint 通过；`git diff --check` 通过；本地 Vditor 关键动态资源(Lute/i18n/KaTeX)存在。全项目 `typecheck` 仍被并行工作区已有 `mediaCategoryDescriptors.ts` 类型错误拦截；全量 Vitest 139/140 文件、1580/1581 测试通过，唯一失败为并行工作区新增文件树分类使用的 8 个缺失 locale 键。
- 遗留:无本线实现遗留；后续若要求把 Vditor 编辑内容写回 Scrollery 版本链，再单独增加保存桥接。

## 会话:2026-08-25（Markdown Editor 平行 POC）
- 做了:固定加入 `@vscode/markdown-editor@0.0.2-0` 与 `@vscode/observables@0.1.1-0`；新增 `MarkdownEditorDocumentModule.vue`，仅负责 EditorModel/View/Controller 挂载、源码变更事件、销毁和 EditContext 能力提示。
- 接线:`DocumentViewer` 增加 `engine=markdown-editor` 分支，并把 Markdown 引擎选择改为标准阅读器 / Vditor / Markdown Editor 三选一；Markdown Editor 草稿只保存在当前窗口。
- 边界:未修改 `BookReader.vue`、`src/utils/markdown.ts`、`src/utils/syntheticBook.ts`、Rust IPC、版本保存、进度、书签和 TOC；默认 route 仍走原生 BookReader。
- 验证:`npm run typecheck`、`npm run build`、`npm run lint`、`npm run test`（142 个文件 / 1595 个测试）和 `git diff --check` 均通过；`vendor:licenses` 与 `prepare:legal` 也通过，新编辑器产物为独立懒加载块。
- 环境检查:普通 Vite 浏览器页因缺少 Tauri 注入 API停在启动页，不能代替 Tauri Windows WebView2 验证；需在 Windows 桌面包中手测 EditContext、中文 IME、块级编辑和卸载重挂载。
- 遗留:POC 尚未接保存；若 Windows 手测通过，再单独增加“另存为新版本”桥接，不做全文重序列化。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
