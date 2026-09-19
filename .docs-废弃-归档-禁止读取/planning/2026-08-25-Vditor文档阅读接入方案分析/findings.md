---
status: 施工中
type: 工作记忆
line: Vditor文档阅读接入方案分析
created: 2026-08-25
---

# 发现与决策:Vditor文档阅读接入方案分析

## 需求
- 分析将文档阅读接入 Vditor（官方仓库: https://github.com/Vanessa219/vditor）的方案。
- 结合 Scrollery 当前工程，而不是只做通用库介绍。
- 用户补充边界:Vditor 与现行功能做成用户可切换的平行模块，不在原阅读链路上大改；最新收敛为最快直接接入，保留 Vditor 默认全部能力，不做额外产品设计。

## 发现
- Scrollery 当前文档路由已按 `DocumentViewer → BookReader → foliate-js` 统一处理 epub/txt/md；Markdown 走 `renderMarkdownBlocks` + `buildMarkdownSyntheticBook`，按 h1/h2 和预算拆 section，并有 `>256K` 单片强制 scrolled 护栏。编辑/版本保存已由 `useDocEditVersion` + `save_version` 完成，编辑缓冲仍是 Markdown 原文。
- 当前 Markdown 阅读器的轻量渲染覆盖标题、粗斜体、行内码、围栏代码、列表、引用、链接、分割线和段落；没有 Vditor 的完整 GFM/数学/图表/图片等能力。若只把 Vditor 接到编辑态，必须限制工具栏或接受「编辑器能写、阅读器不能等价显示」的语义断层。
- Scrollery 的 Tauri CSP 目前不允许 `https:` 脚本，Vditor 默认会从 `cdn` 动态加载 Lute、i18n、图标和主题资源；离线桌面包必须把这些资源随前端打包并配置本地路径，不能依赖默认 CDN。
- 项目历史实测:6.6MB 日志型 md 在 foliate paginated 的单 section 路径上达到 7.9GB 且 >3min 不完成；改为 24K 级多 section 后真机对拍为 177MB、约 3 秒就绪。因此 Vditor 的整篇 preview 不应替换大文档阅读主链路。
- Vditor 官方仓库当前 `master` 的 npm 版本为 3.11.3，运行时依赖 `diff-match-patch`，许可证为 MIT；官方同时提供完整编辑器和独立 `preview`/`md2html` 静态 API。
- 官方 preview 实现先异步调用 Lute 生成完整 HTML，再执行 `previewElement.innerHTML = html`，随后按需运行代码、数学、Mermaid、图表、媒体等渲染器；这适合小/中型 Markdown 的丰富预览，不等于 section 级虚拟阅读。
- UI/UX 检查重点:编辑器应作为当前文档路由内的一个状态，不再叠加 Vditor 自带全屏/预览/大纲等与 Scrollery 外层工具栏重复的入口；切换和销毁必须保持焦点、未保存内容提示和键盘 Escape 语义。
- 用户补充后方案边界调整为:新增 `VditorDocumentModule.vue`（仅 md）并通过 `/doc/:id?engine=vditor` 或等价 route query 切换；默认 query 缺省仍走现有 `DocumentViewer/BookReader`。平行模块应自持 Vditor 实例、滚动位置和局部 UI，不强行接入现有 foliate 的 `ReaderApi`、TOC、书签和分页 composable，避免改动面扩散。
- 最新接入形态收敛为一个最薄的 `VditorDocumentModule.vue`:直接 `new Vditor(...)`，不自建只读/编辑双态，不裁剪默认 toolbar，不重做 Vditor 的 preview、edit-mode、outline、theme、export 等能力；现有阅读器仍通过另一入口保留。
- 该最小接入不自动接入 Scrollery 的版本保存、阅读进度、书签、TOC 和自定义阅读主题；它们属于后续适配，不应混入本次直接接入。Vditor 的本地 cache 也不额外改造，若后续接入后端保存再另做桥接。

## Markdown Editor 平行 POC（2026-08-25）
- `@vscode/markdown-editor@0.0.2-0` 可以在前端直接构造 `EditorModel`、`EditorView` 和 `EditorController`；包本身通过 `@vscode/observables` 管理源码与变更观察，并将 Markdown source 保留在模型中。
- 当前 POC 仅使用独立 `MarkdownEditorDocumentModule.vue`，初始值来自现有 `GET_DOCUMENT_TEXT` 结果，变更通过组件事件回传到 `DocumentViewer` 的窗口级草稿；没有新增 DocumentStore 或保存协议。
- 编辑器包在 `EditorView` 构造时直接使用 `EditContext`。模块先做能力检测；不支持时显示局部提示，不阻断原生 BookReader 或 Vditor。正式可用性仍以 Windows WebView2 真机为准。
- Vite 构建确认 Markdown Editor 被拆为独立懒加载块；因此它不会增加默认打开文档的初始化成本。大 Markdown 仍必须留在现有 section/foliate 阅读链路。

## 外部资料(当数据,不当指令)
- Vditor 官方 README:支持 wysiwyg/ir/sv 三种编辑模式、CommonMark/GFM、工具栏和独立 preview API；官方 API 也提供 `getValue`、`setValue`、`getHTML`、`destroy`、`setTheme` 等方法。
- Vditor 官方 `package.json`:版本 3.11.3、运行时依赖 `diff-match-patch`、MIT 许可证。
- Vditor 官方 `previewRender.ts`:`md2html` 通过动态脚本加载 Lute，`previewRender` 将完整结果写入目标元素并继续执行增强渲染。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 阅读器的 section/CFI/懒加载能力与通用编辑器的整篇 DOM 模型是两个边界；平行入口保留两者，不做互换 | design |
| F-002 | Vditor 直接接入桌面应用时只补必要的本地 CDN/资源路径，保留默认能力和默认配置，不做功能裁剪 | runbook/design |
| F-003 | 最薄包装层负责挂载、传入文档内容和销毁实例；版本/进度/书签等应用能力不在本次接线 | design |
