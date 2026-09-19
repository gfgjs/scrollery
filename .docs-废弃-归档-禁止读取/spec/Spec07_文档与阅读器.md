---
id: 2026-07-24-Spec07_文档与阅读器
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec07-文档与阅读器

> 一句话:本篇讲 Scrollery 的文档(txt/epub/md/pdf)纵向全链——后端编码检测/分章/简繁/版本/书签 + 前端统一渲染器(foliate-js/pdf.js/shiki)。先读 [Spec01 数据层](./Spec01_数据层.md)(表全貌)与 [Spec11 前端架构](./Spec11_前端架构.md)(路由/store/原语通论),再读本篇。

## 1. 概览

文档/阅读器子系统覆盖四类文件的浏览、编辑与版本化:`txt`(智能分章/编码检测)、`epub`(foliate-js 统一渲染)、`md`(Markdown 渲染 + shiki 代码高亮)、`pdf`(pdf.js 独立渲染)。核心目标(见 `docs/designs/2026-07-07-阅读器完善方案.md` §1,下称「方案」):把此前各自为战的阅读/编辑/版本/AI 校对四条链路,统一到一套编码 seam(§3)之上。

**代码位置**:

| 层 | 主目录/文件 | 职责 |
|---|---|---|
| 后端领域逻辑 | `src-tauri/src/reader/` | 编码检测(`encoding.rs`)、分章(`text_index.rs`)、分段(`paragraph.rs`)、简繁(`zh_convert.rs`) |
| 后端 IPC | `src-tauri/src/ipc/doc_commands.rs`(1024 行,26 条命令) | 替换规则/版本/阅读进度/分章分段/简繁/书签/文档缩略图回环 |
| 后端派生 | `src-tauri/src/derive/doc.rs` | epub 封面 + spine 页数提取(zip + quick-xml) |
| 前端页面 | `src/views/DocumentViewer.vue`(1878 行) | 路由 `/doc/:id`;工具栏、TOC/搜索/书签/版本/替换/校对面板宿主 |
| 前端渲染器 | `src/components/doc/BookReader.vue`(txt/epub/md 统一,739 行)、`PdfReader.vue`(pdf 独立,201 行) | 分别包 foliate-js `<foliate-view>` 与 pdf.js |
| 前端合成书模型 | `src/utils/syntheticBook.ts` | 把 txt/md 适配成 foliate 的 `FoliateBook` 契约(章=section) |
| 前端代码高亮 | `src/utils/shikiHighlight.ts` | shiki JS 引擎(非 WASM)高亮 md 代码块 |

**在整机中的位置**:

```
画廊/搜索/收藏 (MediaGrid 等)
        │ 点击文档项 → router.push('/doc/:id')
        ▼
DocumentViewer.vue ── invokeIpc ──▶ doc_commands.rs(26 命令)
        │                                │
        │ isMarkdown/txt: textSource     │ spawn_blocking
        │ epub: url(convertFileSrc)      ▼
        │ pdf: url(convertFileSrc)   reader/{encoding,text_index,paragraph,zh_convert}.rs
        ▼                                │
  BookReader.vue(txt/epub/md)            ▼
   └─ syntheticBook.ts(txt/md 適配)  db: document_versions / doc_replacements /
   └─ foliate-js <foliate-view>          text_book_index / reader_book_prefs /
  PdfReader.vue(pdf)                     reader_bookmarks / document_meta
   └─ pdfjs-dist
```

上游:画廊/收藏/搜索任一处点开文档项即路由到此;下游:文件系统(源文件 + `<appData>/documents/<item_id>/` 版本快照)、DB(见 §2)、远程校对(`ipc/proofread_commands.rs`,详见 [Spec06 §校对](./Spec06_AI人脸OCR.md))。

## 2. 数据模型与状态

全表定义权威在 [Spec01 数据层](./Spec01_数据层.md);此处只列本子系统专有表的关键列。

| 表 | 引入版本 | 关键列 | 用途 |
|---|---|---|---|
| `document_meta` | v1(`src-tauri/src/db/schema.rs:174`,`SCHEMA_V1`) | `item_id`(PK)、`page_count`、`doc_subtype` | 文档元数据;`page_count` 由 epub spine 计数或 pdf.js `numPages` 回填(`doc_commands.rs:820`) |
| `doc_replacements` | v6(`schema.rs:343`) | `scope_kind`('item'\|'group'\|'global')、`scope_id`、`find`/`replace`、`is_regex`、`enabled`、`sort_order` | 展示层查找替换(角色扮演/人名映射),**不改源文件** |
| `document_versions` | v6(`schema.rs:358`) | `item_id`、`parent_id`(成树)、`storage`('appdata'\|'external')、`abs_path`、`source`('user'\|'ai-local'\|'ai-remote')、`content_hash`、`is_current` | 类 git 全量快照;版本**不进画廊** |
| `text_book_index` | v13(`schema.rs:655`) | `item_id`(PK)、`src_key`、`encoding`、`confidence`、`chapters`(JSON `[{t,s,e,n}]`) | txt 分章结果缓存,`src_key` 变化即失效重建 |
| `reader_book_prefs` | v14(`schema.rs:670`) | `item_id`(PK)、`prefs`(版本化 JSON diff) | 每书阅读偏好(R1 期仅 `encoding` 手动覆盖;R3 起扩充竖排/主题/字号) |
| `reader_bookmarks` | v15(`schema.rs:682`) | `item_id`、`locator`(`"cfi:<epubcfi>"`)、`label`、`fraction`(0..1)、`UNIQUE(item_id, locator)` | 书签;同位置幂等刷新 |
| `reading_progress` | v4(`schema.rs:301`) | `item_id`(PK)、`position` | 阅读位置(txt/epub 为 `"cfi:..."` 或旧 `"scroll:<ratio>"`;pdf 为 `"page:N"`) |

**状态归属**:

- **源文件字节** 是唯一 canonical 基线(除非有 `is_current` 版本)。
- **章节索引**(`text_book_index.chapters`)是**派生缓存**,`src_key = "src:<mtime>:<size>[:<override>]"` 或 `"ver:<version_id>[:<override>]"`(`doc_commands.rs:494-511`);源文件被改 / 换当前版本 / 改手动编码覆盖,指纹自动变,下次读取即重建。
- **每书偏好**(`reader_book_prefs.prefs`)只存与全局默认的 diff,读时前端 merge。
- **前端 store**:`DocumentViewer.vue` 内部用大量局部 `ref`(未接入独立 Pinia store);全局仅通过 `viewerStore` 挂 `ViewerApi`(TOC/搜索/书签/排版命令,供标题栏上下文工具栏调用,`DocumentViewer.vue:1519`)。

## 3. 关键流程与算法

### 3.1 txt 编码检测(`reader/encoding.rs`,方案 §5.1)

**为什么**:旧 `std::fs::read_to_string` 遇非 UTF-8(GBK/Big5/Shift_JIS 等)直接 `InvalidData` 报错,阅读/编辑/版本 diff/AI 校对四链路全废;前端 `fetch().text()` 兜底又强制按 UTF-8 解出乱码。本模块是「一次修四链路」的 seam。

检测顺序(`decode_bytes`,`encoding.rs:69-91`,**顺序本身是正确性关键**):

1. **手动覆盖**(每书 `reader_book_prefs.prefs.encoding`)—— `Encoding::for_label` 按 WHATWG label 查表,容错大小写/别名;无效标签静默回落自动检测,不 panic。
2. **BOM**(UTF-8/UTF-16LE/UTF-16BE)—— 命中即定,零歧义。
3. **无 BOM 的 UTF-16 零字节启发**(`sniff_utf16_no_bom`,`encoding.rs:144`)—— chardetng **有意不测 UTF-16**,须自补:采样前 4KB,按奇偶下标零字节占比强信号判定(一侧 ≥0.30、另一侧 <0.05),弱信号回落 chardetng。
4. **chardetng**(`sniff_chardetng`,`encoding.rs:126`)—— 采样前 64KB,`allow_utf8=true`(桌面 app 无浏览器「禁把网页误判 UTF-8」顾虑)。

**已修复的静默 mojibake 坑**(`encoding.rs:130-134`,2026-07-17 实证修复):`last` 参数(告知 chardetng 样本是否为流末尾)必须传 `!truncated`。64KB 切点大概率落在多字节序列中间;若仍传 `last=true`,chardetng 会把截断的合法 UTF-8 尾判为非法,进而把 >64KB 纯 CJK 无 BOM 的 UTF-8 整册误判 `windows-1252`——而 1252 每字节皆可映射,替换率恒 0,`Lossy` 告警完全不响,乱码零提示地流入阅读/编辑/校对全链。回归测试:`large_utf8_cjk_beyond_sniff_window_detects_utf8`(`encoding.rs:314`)。

**置信度**(`Confidence` 枚举,落库 `text_book_index.confidence`):`bom` > `detected` > `manual`,但 **`Lossy`(替换率超 0.5% 阈值)覆盖一切来源**——即便是 BOM/Manual 命中,只要 U+FFFD 占比超阈,UI 仍提示「编码可能误判」。

### 3.2 txt 智能分章(`reader/text_index.rs`,方案 §5.2)

机制仿 legado(GPL,**只借鉴机制,规则自写**,规则数据在 `txt_toc_rules.json`):

```
build_index(bytes, override_label):
  1. decode_bytes(bytes, override_label) → decoded text + encoding + confidence
  2. source_line_starts(bytes, enc)      # 源字节每行行首偏移(§3.2.1)
  3. select_rule(rules, ...)              # 选规则:前 512KB 逐规则统计命中(相邻 <1000 字符只算一次)
  4. 命中数最多者当选(平手按 priority);命中 < MIN_HITS_TRUST(2) → None
  5. selected 有值 → split_by_rule 全文切章;否则 → pseudo_chapters 伪章兜底(~10K 字符/段,行边界)
  6. 单章超 MAX_RULE_CHAPTER_CHARS(30_000) → push_chapter_capped 续切「原题 · N」子章
  7. 兜底:chapters 为空 → 整文当一章「正文」
```

**3.2.1 字节偏移对齐**(正确性核心,`text_index.rs:194-222`):章界永在行首。ASCII 兼容编码(GBK/Big5/SJIS)的尾字节范围不含 `0x0A`,故 `0x0A` 是无歧义行分隔,可直接扫源字节得行首偏移;UTF-16 按端序 2 字节对齐扫换行码元。`decoded.split('\n')` 第 i 行与源第 i 个行首**天然对齐**——一次全量解码 + 一次字节扫描即得可安全切片的源字节偏移,`get_text_chapter` 据此按需 `seek` 单章,不整读大文件。

**规则否决**(`deny_suffix`,`text_index.rs:104-119`):Rust `regex` 不支持环视(lookaround),用「匹配结束处紧邻字符若在 deny_suffix 集合内则否决」模拟负向断言,如「第三节课」不应被判为「第三节」章标题。

### 3.3 txt 智能分段(`reader/paragraph.rs`,方案 §5.2/R1-4)

两级设计:

- **一级(默认,保守)**:非空行 = 一段;空行分隔;行首缩进空白 `trim` 剥离(缩进由渲染层 CSS `text-indent` 施加,不改文本)。
- **二级「智能重排」(每书 opt-in,默认关)**:行尾非句末标点(`SENTENCE_ENDERS` 集,含中英文标点及配对括号/引号)则与下行黏合成一段,幂等(`reflow_is_idempotent` 测试)。**会改变文本 → 进度定位须走重锚**,故独立开关,不默认开。

标题剥离(`strip_title_line`,`paragraph.rs:88`)必须在 `segment()` **之前**按行精确匹配剥离:若在分段后再比对首段,reflow 模式下标题行(常无句末标点)会先与下一行黏合,导致标题重复渲染又污染正文首段(2026-07-10 审查 B2 修复)。只做精确匹配,不模糊/前缀匹配——超长标题被截断后不剥离,宁可重复不误删正文。

### 3.4 简繁转换(`reader/zh_convert.rs`,方案 §5.10)

`ferrous-opencc`(纯 Rust,零 C++ 依赖,14 内置 config 词典编译进库)。`convert_batch` 每次调用构建一次 converter 转整批(前端按章调用,每章一次构建,毫秒级)。**只作用于渲染层文本,不改 canonical 底层文本与章内偏移**——与进度/书签定位互不干扰。前端在**应用替换规则之后**按章调用(§4「替换 → 简繁 → 渲染」顺序,方案 §5.13)。

### 3.5 前端统一渲染管线(foliate-js,方案 R2)

txt / epub / md 三格式共用同一个 `BookReader.vue` 包 vendored `foliate-js` 的 `<foliate-view>` 自定义元素;pdf 独立走 `PdfReader.vue`。

**打开流程**(`BookReader.vue::resolveOpenTarget`,行 452-503):

```
textSource 存在(txt/md)?
  ├─ isMarkdown → GET_DOCUMENT_TEXT 取全文 → renderMarkdownBlocks 分块
  │              → 含代码块则逐块 highlightMarkdownHtml(shiki)
  │              → buildMarkdownSyntheticBook({blocks}) → groupMarkdownBlocks 按块分片
  └─ txt        → GET_TEXT_BOOK_INDEX 取章索引 → buildTextSyntheticBook({index, loadChapter})
                    (loadChapter 按需调 GET_TEXT_CHAPTER,章级懒加载)
  两分支均检查 hasOversizedSection(sections) → 超限则 forcedScrolled=true + emit('flow-forced')
否则(epub/pdf) → 直接用 url(convertFileSrc),交 foliate makeBook 自动探测(epub)
```

`SyntheticBook`(`src/utils/syntheticBook.ts`)把 txt/md 适配成 foliate 的 `FoliateBook` 契约:`sections[]` 每项 `{id, size(进度权重代理=charLen/htmlLen), load()→blob URL, unload(), createDocument()}`;`splitTOCHref`/`getTOCFragment`/`resolveHref` 等钩子用数字章号(`String(i)`)作 href。章内容按需经 `loadChapter` 依赖注入拉取(生产=IPC,测试=stub),使纯逻辑可在 `vitest environment=node` 单测。

**md 分片规则**(`groupMarkdownBlocks`,`syntheticBook.ts:228`):① `h1`/`h2` 块起新片(天然章界,派生 TOC);② 预算超限(`MD_SECTION_BUDGET_CHARS=24_000`)起新片;③ 单块超预算独占一片,不内切(切开 `<pre>`/`<ul>` 破坏块语义)。

**内存爆炸修复**(2026-07-17,详见 [experience.md](../experience.md) 与 `docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md`):foliate 的 `paginated` 流对整个 section 施加 CSS multicol,开卷期全文档几何查询(`getVisibleRange` 逐文本节点 Range+rect)随 fragmentainer 数乘积放大——实测单 section 1.9M 字符即 2.5GB 内存 + 主线程阻塞 5 分钟未完成,8.8MB 达 7.9GB。修复两层:

1. **分片**:md 按 `MD_SECTION_BUDGET_CHARS=24_000` 预算切片;txt 天然已按 ~10K 字符伪章/30K 上限续切(§3.2)。
2. **护栏**:`hasOversizedSection(sections, limit=FORCE_SCROLLED_SECTION_CHARS=256_000)`(`syntheticBook.ts:190`)——分片后仍可能残留超限单片(md 巨型代码围栏不内切;txt 分章只在**行边界**续切,单行巨串如 minified JSON/base64 日志整行独占一章,30K 上限对它失效)。命中即由 `BookReader` 在 `open()` 前置位强制 `scrolled` 流(同内容 `scrolled` 完全健康),并 `emit('flow-forced')` 通知父层禁用流切换、提示一次。

### 3.6 PDF 渲染(`PdfReader.vue`,方案 §5.1)

pdf.js 独立渲染(不入 foliate 统一管线,因固定分页布局与 foliate 的 reflowable 模型不匹配)。按页懒渲染:`IntersectionObserver`(`rootMargin: 600px`)命中才 `page.render()`;画布长边限 `MAX_CANVAS_EDGE=2200` 避免超大 canvas 爆内存;`isEvalSupported: false` 关闭 pdf.js 字型渲染的 eval 优化路径,配合生产 CSP 删除 `unsafe-eval`(P1-23)。位置格式 `"page:N"`。

### 3.7 shiki 代码高亮(`utils/shikiHighlight.ts`,方案 R4)

**约束**(用户 2026-07-07 定方向):必须用 `createJavaScriptRegexEngine`(TextMate 语法编译为原生 RegExp),不用 shiki 默认 WASM(oniguruma)引擎——WASM 需 CSP `wasm-unsafe-eval`,而生产 CSP 已硬化删除 `unsafe-eval`。全部经动态 `import` 懒加载(核心/引擎/每语法/主题各自独立 chunk),仅在打开含代码块的 md 时按需拉取。单块超 `MAX_HIGHLIGHT_CHARS=100_000` 回退纯文本(TextMate 逐行正则扫描,巨型日志块烧 CPU 属另一处「内存爆炸」同族风险,阈值独立设定,详见常量注释)。

### 3.8 书签 CRUD(方案 R4,§6.2)

`locator` 存位置串(现 `"cfi:<epubcfi>"`,与 `reading_progress` 同源;未来 loc1 落地后可存 `"loc1:<json>"`,列不变)。`add_reader_bookmark`(`doc_commands.rs:723`)靠 `UNIQUE(item_id, locator)` 令同位置书签幂等——重复添加即刷新标签/进度/时间,不产生重复行。前端 `getCurrentLocation()`(`BookReader.vue:442`)读最近一次 `relocate` 事件缓存的 `lastCfi`/`lastFraction`/`lastLabel` 作快照。

### 3.9 版本树(方案 §5.3)

`save_version`(`doc_commands.rs:239`)两种 `target`:

- `"version"`(默认):写入 appData 新快照,**不进画廊**。
- `"overwrite"`:先把旧源内容自动备份为一个版本(读失败除「源不存在」外一律中止覆盖,`doc_commands.rs:288`),再原子覆盖源文件(`write_atomic`,同卷 tmp→rename)。

**单事务写版本**(`write_version_tx`,`doc_commands.rs:202-230`,方案 B §3.1):插行(拿 id)→ 派生 `{id}.{ext}` 路径 → 写原子文件 → 回填路径 → commit,**文件写在 commit 之前**。已提交的行必有文件在盘;文件写失败则整事务回滚、行不落库。取代旧「插行→写文件→回填」两步式——旧式在①插行 commit 后、③写文件前崩溃会残留「committed 行 + 空 abs_path + 无文件」,备份据此产生假完整包。回归测试:`write_version_tx_rollback_leaves_no_row_on_file_failure`(`doc_commands.rs:979`)。

`document_storage_guard`(`doc_commands.rs:256`,`AppState` 字段)在版本写入/删除期间持 **read guard**,与数据备份的 write guard 互斥,防止备份捕获保存中间态;guard 只在 `spawn_blocking` 闭包内持有,不跨 `.await`。

### 3.10 文档缩略图派生(`derive/doc.rs`)

`epub` 走后端(`run_thumb`):`zip` 解析容器 → `META-INF/container.xml` 找 OPF → OPF 解析封面 href(优先级:EPUB3 `properties="cover-image"` → EPUB2 `<meta name="cover">` → 启发式 id/href 含 "cover") → 读封面字节 + 顺带计 `<spine><itemref>` 数作页数近似 → 解码 RGBA → 复用 `encode_media_step`(与视频封面同构)。`pdf`/`svg` 由前端离屏渲染截图,经 `store_doc_thumbnail`(raw body IPC,§4)回传;`txt`/`md`/`office` 无派生行,前端用 CSS「文本卡」呈现。

### 3.11 远程校对入口

`ipc/proofread_commands.rs` 提供 AI 校对配置/密钥/校对块命令(详情见 [Spec06 AI人脸OCR §校对](./Spec06_AI人脸OCR.md))。本篇只记接口面:接受后的校对结果经 `save_version(source="ai-remote"|"ai-local")` 存为新版本(§3.9),track-changes 预览走 `diff_texts`(§4,纯计算 diff,原文 vs AI 修订文)。

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令一览(`doc_commands.rs`,26 条)

| 分组 | 命令 | 要点 |
|---|---|---|
| 替换规则 | `list_replacements`、`get_effective_replacements`、`upsert_replacement`、`delete_replacement` | `get_effective_replacements` = 启用的 global + item 作用域,按序合并 |
| 版本 | `list_versions`、`get_current_version`、`get_document_text`、`get_version_content`、`save_version`、`set_current_version`、`delete_version`、`diff_versions`、`diff_texts` | `get_document_text` = 当前版本(若设)否则源文件;§3.9 |
| 阅读进度 | `get_reading_progress`、`set_reading_progress` | 查看器去抖调用 |
| txt 分章/分段/简繁 | `get_text_book_index`、`get_text_chapter`、`convert_chinese` | §3.2/3.3/3.4 |
| 每书偏好 | `get_reader_book_prefs`、`set_reader_book_prefs` | prefs JSON diff |
| 书签 | `list_reader_bookmarks`、`add_reader_bookmark`、`delete_reader_bookmark` | §3.8 |
| 文档缩略图 | `ensure_doc_thumb_queue`、`list_pending_doc_thumbs`、`store_doc_thumbnail` | pdf/svg 前端驱动回环,§3.10 |

错误码/`AppError` 全量权威见 [Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md);本子系统命令一律走该稳定错误类型,不泄漏内部字符串。

`store_doc_thumbnail` 走 **raw body**(`doc_commands.rs:800-828`):PNG 字节若走 JSON 数字数组序列化,每字节膨胀 ~4 字符(300KB PNG → 1-2MB JSON 串);Tauri v2 `tauri::ipc::Request` 的 raw `InvokeBody` 直传字节零膨胀。元数据(`item_id`/`page_count`)因此只能走小写自定义 request header(`x-item-id`/`x-page-count`),不能进 body。**空 body = 渲染失败**约定:标记派生 `status=3`(不再重试)+ `media_items.thumb_status=2`(无缩略图)。

### 4.2 不变量清单

| 不变量 | 为什么违反会怎样 | 出处 |
|---|---|---|
| 新格式接入 foliate 渲染**必须分片**(单 section 有预算上限) | 整篇单 section 在 paginated multicol 下有实测 GB 级内存放大 + 主线程阻塞分钟级 | `syntheticBook.ts:169-187`,md 内存爆炸修复(experience.md) |
| chardetng 截断采样必须传 `last=!truncated`,不可恒 `true` | 会把 >64KB 无 BOM UTF-8 静默误判 windows-1252,替换率恒 0、Lossy 告警不响,mojibake 完全无提示 | `encoding.rs:130-137`,回归测试 `large_utf8_cjk_beyond_sniff_window_detects_utf8` |
| `text_book_index`/`get_text_chapter` 恒按**字节区间 seek**,绝不整读全文取一章 | 100MB 级 txt 若每章都整读全文解码,IO/CPU 随章数线性放大;R1 设计的核心性能保证即「IPC 载荷恒为单章级」 | `doc_commands.rs:589-599`(`read_byte_range`),`text_index.rs` 模块头注 |
| txt 分章续切/伪章切分**只在行边界**,不做行内字节切分 | 行内切分需 decoded-char→源字节映射,在非 UTF-8 编码下是新的偏移正确性风险面;后端故意不做,内存放大改由前端护栏(§3.5 hasOversizedSection)兜底 | `text_index.rs:638-643`,characterization 测试 `single_giant_line_stays_one_chapter` |
| 简繁转换只作用于渲染层,不改 canonical 文本/章内偏移 | 若简繁改了 canonical,进度/书签定位(基于 CFI/字节偏移)会与文本错位 | `zh_convert.rs` 模块头注 |
| `write_version_tx` 文件写必须发生在 DB commit **之前** | 反过来会在崩溃窗口留下「committed 行 + 空路径 + 无文件」,备份据此产出假完整包 | `doc_commands.rs:193-230`,测试 `write_version_tx_rollback_leaves_no_row_on_file_failure` |
| `document_storage_guard` 只在 `spawn_blocking` 闭包内持有,不跨 `.await` | 项目硬约束(见项目 AGENTS.md);跨 await 持锁可致死锁/优先级反转 | `doc_commands.rs:256` |

## 5. 边界情况与失败模式

| 场景 | 处理 | 出处 |
|---|---|---|
| 编码检测误判(替换率超 0.5%) | `confidence` 降级为 `Lossy`,UI 提示「编码可能误判」,用户可在每书偏好手动覆盖编码 | `encoding.rs:19,99-104` |
| 无任何章节标记的长文本 | 伪章兜底,~10K 字符/段,行边界切分 | `text_index.rs:367-395` |
| 单行巨串(minified JSON/base64/单行日志) | 无法在行内续切,保持单章上报真实 `char_len`;前端护栏(§3.5)检出超限强制 `scrolled` | `text_index.rs:645-658` |
| 无封面 epub | `find_cover_href` 三级回退全部失败 → `AppError::Internal("epub cover not found")`;该 item 缩略图缺失,不阻断浏览 | `derive/doc.rs:103-104` |
| epub 容器损坏(缺 `container.xml`/OPF) | 逐步返回 `AppError::Internal`,派生任务标记失败(status=3),不影响其余文档 | `derive/doc.rs:85-97` |
| 覆盖源文件前读源失败(非 NotFound) | 中止覆盖(不产生无备份的覆盖),仅「源不存在」视为合法空备份 | `doc_commands.rs:288-293` |
| 陈旧/损坏的阅读进度 CFI(换版本/切 reflow 后旧 CFI 失配) | `restorePosition` 捕获 `view.init({lastLocation:cfi})` 异常,回退无位置初始化(首章起),不让整本书打不开 | `BookReader.vue:509-522` |
| 前端渲染文档缩略图失败(pdf/svg) | 空 body 视为失败信号,标记派生 error 状态,不无限重试 | `doc_commands.rs:788-789,833-848` |
| `store_doc_thumbnail` 误用他格式 item_id | 已知缺陷(非 P0):命令不校验 `x-item-id` 对应 item 的 `file_format` 是否 pdf/svg,理论上可用 `.txt` 项 id 调用写入错误 `doc_subtype`;当前依赖前端调用纪律,未加后端校验 | `doc_commands.rs:900-904` 代码注释自述 |

## 6. 重建指引(从零实现)

**依赖顺序**(建议施工序,对齐方案 R1→R6 分期):

1. **编码 seam**(`reader/encoding.rs`)——一切下游的地基,先做且须充分单测(BOM/UTF-16 无 BOM/chardetng/替换率/手动覆盖 5 类路径全覆盖)。
2. **分章 + 分段**(`reader/text_index.rs` + `paragraph.rs`)——依赖 §1 的解码结果;字节偏移对齐测试(roundtrip_check)是正确性生命线。
3. **doc_commands.rs 基础命令**:`get_document_text`/`get_text_book_index`/`get_text_chapter`,先打通 txt 读路径。
4. **版本管理**(`document_versions` 表 + `save_version`/`list_versions`/`diff_versions`):单事务写版本模式务必先写崩溃语义单测(`write_version_tx_*`)再接前端。
5. **前端合成书模型**(`utils/syntheticBook.ts`)+ **BookReader.vue** 包 foliate-js——先接 txt(最简单,单章一 section),再接 md(分片逻辑),epub 走 foliate 原生 `makeBook` 探测。
6. **PdfReader.vue**(pdf.js,独立管线,不依赖 foliate)。
7. **简繁**(`zh_convert.rs`)、**书签**(`reader_bookmarks`)、**shiki 高亮**——增量特性,互相独立可并行。
8. **文档缩略图**(`derive/doc.rs` + `store_doc_thumbnail` raw body 回环)。

**外部依赖**:

| 层 | 包 | 版本 | 用途 |
|---|---|---|---|
| 后端 | `chardetng` | 0.1 | txt 编码启发式检测 |
| 后端 | `encoding_rs` | (随 chardetng 生态) | 编码解码/BOM 识别/label 查表 |
| 后端 | `ferrous-opencc` | 0.4 | 简繁转换(纯 Rust,零 C++) |
| 后端 | `regex` | (workspace 版) | 分章规则正则 |
| 后端 | `zip` | 2 | EPUB 容器解析 |
| 后端 | `quick-xml` | 0.36 | OPF/container.xml 解析 |
| 后端 | `similar` | 2 | 版本 diff(行级) |
| 后端 | `xxhash-rust`(xxh3) | (随 workspace) | 版本内容指纹(`content_hash`) |
| 前端 | `pdfjs-dist` | 4.10.38 | PDF 渲染 |
| 前端 | foliate-js(vendored,`src/vendor/foliate-js/`) | — | epub/txt/md 统一渲染管线(**GPL,仅借鉴机制,规则/适配层自写**) |
| 前端 | `shiki` | 4.3.1 | 代码高亮(JS 正则引擎变体,非 WASM) |

**坑与教训**(详情勿重复,链接):

- md/txt 巨型文档内存爆炸修复的完整测量过程见 `docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md`。
- chardetng `last` 参数踩坑(§4.2 第二条)是本子系统唯一「测量当证据前先测量测量本身」类教训,写分章/编码相关扫描器前建议先读项目 `docs/experience.md §18/§19`。

**验收**:

- 后端单测:`cargo test -p scrollery`(或对应包名)覆盖 `src-tauri/src/reader/*.rs`(encoding 12 测试 + paragraph 12 测试 + text_index 16 测试 + zh_convert 6 测试)与 `ipc/doc_commands.rs` 内嵌测试(版本写崩溃语义 + prefs 解析)。
- 前端核心测试(vitest)：`src/utils/contentRendering.core.spec.ts` 集中 Markdown、EPUB 脚本处理和字幕转换。2026-09-16 按全仓500项预算移除 syntheticBook 专项；旧 shikiHighlight 入口此前已删除，当前自动化范围以核心套件为准。
- Gate 命令:`.github/workflows/ci.yml` 中 Rust `cargo test`/`cargo clippy` 与前端 `vitest`/`vue-tsc` 任务(具体 job 名见该文件,未在本篇复述)。
- GUI 手测(未自动化,需真机):txt 大文件(>10MB)打开流畅度、epub 竖排/主题切换、pdf 长文档滚动、md 代码块高亮、书签跨重启恢复、编码手动切换生效。

## 7. 关联

- 设计权威:`docs/designs/2026-07-07-阅读器完善方案.md`(R1-R6 分期、§5 逐项设计与技术裁决、§8 风险与开放问题)——本篇一切「为什么」优先链接此文档对应 § 号,不复制其论证。
- 内存爆炸修复过程:`docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md`。
- 前端通用架构(路由/store/原语/IPC 调用规范):[Spec11 前端架构](./Spec11_前端架构.md)。
- 全表定义权威:[Spec01 数据层](./Spec01_数据层.md)。
- IPC/错误码全量权威:[Spec10 IPC与错误契约](./Spec10_IPC与错误契约.md)。
- AI 远程校对推理与 OCR:[Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md)。
- 前端体验重构总纲(与阅读器不直接重叠,但同属前端主题/CSS 变量体系):`docs/refactor_2026/Part5_前端体验重构.md`。
