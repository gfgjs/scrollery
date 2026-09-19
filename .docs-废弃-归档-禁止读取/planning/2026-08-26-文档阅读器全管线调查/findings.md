---
status: 施工中
type: 工作记忆
line: 文档阅读器全管线调查
created: 2026-08-26
---

# 发现与决策:文档阅读器全管线调查

## 需求
- 调查分析本项目文档阅读器整个管线。

## 发现
- 项目为 Vue 3 + TypeScript 前端、Tauri/Rust 后端；文档阅读相关入口至少包含 `src/views/DocumentViewer.vue`、`src/components/doc/` 和 `src/vendor/foliate-js/`。
- 工作区存在未提交改动，包含 `MarkdownEditorDocumentModule.vue`、`DocumentViewer.vue`/样式及 Vditor 依赖，调查必须以当前工作树为准并避免覆盖这些改动。
- 项目已有历史长任务 `docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/`，可作为大文档管线的变更与验证背景。

## 当前支持边界

扫描层注册的文档格式比阅读页宽：`pdf`、`epub`、`txt`、`md`、`rtf`、Office 系列和 `svg` 都可以作为 document 入库，但 `DocumentViewer` 真正内置渲染的只有 PDF、EPUB、TXT、MD；Office/RTF 等在阅读页显示不支持并提供外部打开，SVG 主要走文档缩略图路径。历史 worklog 中出现的 mobi/fb2/cbz 等格式不应当当作当前能力。

主入口为 `/doc/:id`。`DocumentViewer` 先取媒体详情、阅读进度、有效替换规则、文本全文/当前版本及书籍偏好，再按格式分派：PDF→`PdfReader`，EPUB/TXT/MD 默认→`BookReader`，MD 另有 query 旁路 `engine=vditor` 和当前未提交的 `engine=markdown-editor`。`BookReader` 使用 vendored foliate-view；EPUB 直接打开资源，TXT/MD 构造 synthetic book。

## 端到端管线

```text
扫描/分类 → media_items + path + document_meta
        → /doc/:id → GET_MEDIA_DETAIL / progress / replacements / prefs
        → PDF.js | foliate EPUB | Rust TXT index/chapter + foliate | Markdown parser + foliate
        → relocate/page events → debounced reading_progress
        → TOC/search/bookmark/settings/edit/version 等 IPC 回路
        → unmount 时 detach、close、revoke Blob URL、flush progress
```

### 各格式责任

| 格式 | 解析/渲染 | 定位与交互 | 缩略图 |
|---|---|---|---|
| PDF | 前端 pdf.js，IntersectionObserver 附近页懒渲染 canvas | `page:N` 进度、翻页；当前没有 foliate 的目录/搜索/书签面板 | 前端离屏首屏渲染 |
| EPUB | foliate 读取本地资源；打开前拒绝外链脚本资源 | CFI、目录、搜索、书签、排版/主题；进度写回 | Rust zip 读取 OPF/封面，页数为 spine 近似值 |
| TXT | Rust 编码检测/章节索引；章节按需 IPC，synthetic book 生成 XHTML | CFI、目录、搜索、书签、编码/重排/中文转换 | 无专门文档缩略图 |
| MD | Rust 取生效全文；轻量 Markdown parser 分块，代码块可经 Shiki 高亮，再建 synthetic book | 同 TXT 的 foliate 能力；默认 reader，Vditor/Markdown Editor 为并行实验路由 | 无专门文档缩略图 |
| Office/RTF/SVG | 目前不进入内置阅读渲染 | 外部打开或缩略图侧能力 | SVG 属前端文档缩略图白名单；Office/RTF 无 |

## 后端、缓存与持久化

- `get_document_text`、版本读写、TXT 索引/章节、中文转换都通过 `spawn_blocking` 隔离文件/CPU 工作；Rust 领域读写通过绑定参数的 rusqlite 查询。
- TXT 的索引缓存由源文件 mtime（秒）+ size + 编码覆盖或版本 ID 组成 `src_key`。索引只向前端返回标题/字符数，章节内容由 `GET_TEXT_CHAPTER` 按 byte range 内读取并解码；前端无需一次装入全部 TXT。
- 阅读进度按 item 保存：foliate 使用 `cfi:`，PDF 使用 `page:`，兼容旧文本 `scroll:`；书籍偏好、替换规则、版本链、书签分别落库。
- synthetic book 对章节/分片使用 Blob URL，`unload/destroy` 会撤销；BookReader 卸载时还会关闭 foliate view。PDF 销毁 pdf document、移除 observer/listener，但 page wrapper 仍按总页数一次性建出。

## 关键风险与证据

1. **高优先级：TXT 入口仍提前读全文，MD 还会重复读全文。** 虽然 TXT 的 `BookReader` 路径已经是 index + chapter 懒加载，`DocumentViewer.load()` 对所有 TXT/MD 仍先调用一次 `GET_DOCUMENT_TEXT`。因此大 TXT 会在挂载 reader 前先经过 Rust 解码、IPC 和前端字符串内存，懒加载收益被入口层部分抵消；默认 MD 随后又在 `BookReader.resolveOpenTarget()` 再取一次全文。MD 需要全文解析，这是合理的，但应避免父视图为编辑态预取与渲染器重复取数；TXT 则应与编辑/版本数据解耦。
2. **高优先级：Markdown Editor 当前 WIP 的语义不一致。** 父视图注释/已有 Vditor 方案将其描述为仅窗口内草稿，但未提交组件实际每秒自动调用 `SAVE_VERSION(target:'overwrite')` 写回源文件，并允许 Ctrl/Cmd+S；没有真实 WebView2/IME 交互测试，也没有卸载时取消 in-flight IPC。未明确产品语义前不应把该旁路视为可发布编辑器。
3. **中高优先级：不支持格式的 viewerStore 分类漂移。** `viewerKind` 注释要求 DOCX/RTF 在进入 viewerStore 前拦截，但当前 `/doc/:id` 仍由通用文档入口打开，`useDocActiveViewerSync` 对未知 document fallback 为 `text`。这可能使不支持格式出现文本阅读上下文命令，虽然主体最终显示外部打开。
4. **中优先级：PDF 不是虚拟页列表。** canvas 是附近懒渲染，但每页 wrapper 都进入 DOM；超大页数 PDF 的布局、滚动和内存成本仍随页数增长，render/getPage 也没有显式取消。
5. **中优先级：索引失效粒度为 mtime 秒。** 同一秒内同大小修改 TXT 可能命中旧 `text_book_index`；概率低但属于可修复的缓存指纹边界。
6. **测试缺口。** 当前静态/单元测试很健康，但没有覆盖真实 Tauri WebView2 中 foliate iframe、PDF IntersectionObserver、缩略图 pump、Vditor/Markdown Editor 输入法与长文档实测的组件/E2E 表征测试。

## 安全与可靠性结论

EPUB 在打开后、初始化定位前安装脚本资源拒绝器；CSP 的 `script-src` 没有 `unsafe-inline/unsafe-eval`，blob iframe 由父页策略约束。PDF.js 明确关闭 eval；本地资源通过 Tauri asset protocol scope 和运行时扫描根授权，并由 `convertFileSrc` 生成 URL。媒体缩略图回传还校验 item 格式和原始 PNG 载荷。generation/itemId guard 能避免切换文档后旧请求提交状态，但 Markdown Editor 的写入请求尚未具备相同的取消/路由生命周期约束。

## 验证证据

- 前端：142 个测试文件、1598 个测试全绿；阅读器相关 11 个文件/99 个测试全绿；`npm run typecheck`、相关 ESLint、`npm run build` 通过。
- Rust：reader 44 个测试、文档缩略图 5 个测试、版本落盘 2 个测试通过；`cargo fmt --all -- --check` 与 `cargo check -p scrollery --locked` 通过。
- 构建仍有既有动态/静态 import 提示；Markdown Editor、Vditor、PDF reader 都被拆为懒加载 chunk。以上是当前工作树的自动验证，不代替真实 GUI、WebView2 和超大文件基准。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
