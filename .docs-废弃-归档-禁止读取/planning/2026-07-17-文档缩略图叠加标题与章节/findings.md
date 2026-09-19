---
status: active
type: working-memory
line: 文档缩略图叠加标题与章节
created: 2026-07-17
---

# 发现与决策:文档缩略图叠加标题与章节

## 需求
- 用户原话(2026-07-17 批量 15 项之 #13):「文档缩略图显示文件名/内嵌名/章节,后台静默生成」。
- 用户 2026-07-17 裁决:本项从「批量15项问题清单」线拆出,**另起独立三件套,新会话单独接续做**。

## 现状(摸底,2026-07-17 核实,行号当日准)

### 各格式缩略图现在长什么样
| 格式 | 现状 | 位置 |
|---|---|---|
| pdf / svg | 前端离屏烘首页(pdf.js / `<img>` 解码),产物经 `IPC.STORE_DOC_THUMBNAIL` 用 `invokeIpcRaw` 字节直传回后端 | `DocThumbRenderer.vue`(全 193 行,无可见 DOM);`renderPdf`:43 / `renderSvg`:67 / `renderOne`:95 / 回传 :127-132 |
| epub | 后端解 OPF 取封面图;**未取 dc:title / TOC** | `derive/doc.rs:76 extract_epub_cover_and_pages` |
| txt / md / office | CSS 文本卡:**一个真字都不画**,只有 4 条假行 + 扩展名 | DOM `MediaThumb.vue:48-60`(4 个空 `<span>` :54-57 + 扩展名 :59,判定 :469-473);Canvas `MediaGridCanvas.vue:1165 drawTextCard`(假行宽度比硬编码 `[0.65,1,0.92,0.55]` :1185-1192 + 扩展名徽章 :1193-1208) |

- 无封面 epub 会降级成文本卡:`isTextCardFallback`(`mediaGrid.helpers.ts:90-98`,epub + thumb_status===2)。
- `DocThumbRenderer` 另有失败预算 `MAX_ATTEMPTS=2`(:108,复败回传空字节让后端标 status=3)与「无进展守卫」(:156)防热循环。

### 数据面
- `document_meta` **全表仅 3 列**:`item_id / page_count / doc_subtype` —— `db/schema.rs:174-178`。
  DAO:`db/queries/documents.rs:409 upsert_document_meta` / `:437 get_document_meta`(SQL 列表在 :416 / :439)。
- `MediaMeta`(viewportMeta 的载荷)**无 title / 无 chapters**:Rust `db/models.rs:197-210`(12 字段)/
  前端 `src/types/layout.ts:39-52`,两侧逐字段一一对应。SQL 在 `db/queries/media.rs:286 get_media_meta_batch`。
- txt 章节索引产物存 **`text_book_index` 表(不是 document_meta)**,`chapters` 序列化成 JSON 列
  (`ipc/doc_commands.rs:525-536 upsert_text_book_index`,缓存键 `src_key`,:512-513 命中即返)。

### 现成可用的地基
- **`filename` 信息元素已存在且双路已通**(零期的基础):
  - 常量 `META_DRIVEN_INFO_ELEMENTS = ['filename','path','geo','camera','params']` —— `mediaGrid.helpers.ts:128`(**行号至今未漂**)
  - 取数链路全通:`MediaGrid.vue:1931` 门控(`ui.showThumbInfo && needsViewportMeta(...)`,watch 源含元素列表本身 → 中途勾选立即补拉)→ `mediaStore.ts:69 ensureMeta`(去重 + 120ms 防抖)→ `:56 invokeIpc(IPC.GET_META_FOR_VIEWPORT)` → :59-61 整个 Map 重赋值触发响应式
  - 组装:`buildThumbInfoLines`(`helpers.ts:20`),filename 分支在 :26-28
  - 双路消费:DOM `MediaThumb.vue:342`(computed)→ :448;Canvas `MediaGridCanvas.vue:1221` → `drawInfoOverlay`(:1230)
  - 单源保障:`helpers.spec.ts:147` 有「meta 取用面与 buildThumbInfoLines 一致」的对拍测试,改一处会红
- **Canvas 侧文字绘制能力相当齐备**(加信息行几乎是填空):
  - 截断助手 `truncateToWidth(s, maxW, measure)`(:104 导入 / :1224 使用)
  - 字体 `palette.fontMono`(:469 类型 / :490 兜底 / :522 从 CSS var `--font-mono` 读);现有字号档:徽章 `700 {9|10}px system-ui`(:1281)、扩展名 `700 11px mono`(:1195)、信息行 `10px mono`(:1222/:1299)
  - **DPR 统一在 :543-557 做掉**(:557 `setTransform(dpr,...)`),绘制代码一律用逻辑像素,新增文字无须自己管 DPR
  - 信息行画法:`drawInfoOverlay`(:1230)= 徽章收集(:1244-1259)→ `infoLinesFor`(:1260)→ 底部三档渐变(:1263-1273)→ 徽章行(:1277-1294)→ 文本行(:1299 起)
- **设置项点法**:key `showThumbInfo`(`settingsMap.ts:224-232`,`section:'thumbnails'` / `control:'toggle'` /
  **`customRow: true`** :231 —— 开关下挂元素多选面板)。i18n `settings.thumbInfoHover`(zh-CN.ts:517-518 =
  「缩略图信息悬浮窗」/ en-US.ts:535-536)。用户路径:**设置 → 缩略图 → 「缩略图信息悬浮窗」(默认已开)
  → 其下多选面板勾「文件名」**。面板选项表 `SettingsView.vue:435-447`(filename 项 :442)→ :90
  `handleThumbInfoToggle` → :554 → `uiStore.ts:426 setThumbInfoElements` → 持久化键 `thumb_info_elements`。

## 设计稿更正(旧稿 6 条:5 真 / 1 语义偏松,另有 3 处过度承诺)

旧稿 = `docs/planning/2026-07-17-批量15项问题清单/designs-难项方案.md` 的 #13 节(写于摸底前)。

| 旧稿说法 | 核实结论 |
|---|---|
| pdf/svg 是「**可见性门控**后台泵」 | **语义偏松**:是**窗口级** `document.visibilityState`(:114/:119/:124,:170-172 复泵),**不是视口级**(不按缩略图是否滚进视口门控) |
| epub 未取 dc:title/TOC | ✅ 真,且 **OPF 整份 XML 已在内存**(`opf_xml` :93-100,:104 已在复用它数 spine 页数)→ 加 `find_dc_title` 是同一字符串再走一趟 quick-xml,**零额外 I/O、零二次开包**,:121/:174 有现成解析范式可抄 |
| txt/md/office 是无文字 CSS 文本卡 | ✅ 真,两路都只画假行 + 扩展名 |
| document_meta 无 title/chapters 列 | ✅ 真(仅 3 列) |
| txt 章节提取已有(text_index),只在开卷时按需建 | ✅ 真:`reader/text_index.rs:152 build_index`,仅被 `ensure_text_index`(`doc_commands.rs:501`)调用,而后者只被开卷路径 `get_text_book_index`(:559)/ `get_text_chapter`(:589)触发;扫描/派生流水线不碰 |
| filename 元素已存在、双路已通、`helpers.ts:128` | ✅ 全真,**行号 128 分毫不差** |

### ⚠️ 三处过度承诺(旧稿据此推的结论要打折)

1. **「零期零代码」成立,但零代码 ≠ 零操作,且只值三分之一**
   - `uiStore.ts:418` `showThumbInfo` 默认 **true**,但 `:419 thumbInfoElements` 默认 **空数组**
     → **开箱观感是没有任何信息行**,用户必须自己去勾 filename。
   - `MediaThumb.vue:446`:**compact 模式直接 `return []`**,零期效果不覆盖该模式。
   - 画的是 `meta.fileName`(**文件名**),与需求里的「内嵌标题/章节」是两回事——零期只满足需求的 1/3。
   - → 设计表述应降级为:「文件名可零代码交付;标题/章节仍需全链路加字段」。

2. **epub 取 title 有位置陷阱(不写进设计稿八成会踩)**
   - `derive/doc.rs:101-102`:`find_cover_href(...).ok_or_else(...)?` —— **无封面即整个函数提前返回 Err**。
   - 而「无封面 epub」正是降级成文本卡(`mediaGrid.helpers.ts:90-98`)的那批,**也正是最需要标题来救观感的那批**。
   - → `find_dc_title` **必须上提到 :101 的 `?` 之前**,否则对目标人群恰好全部失效。

3. **text_index 不可直接复用在烘缩略图路径上**
   - `ensure_text_index` 走 `std::fs::read` **整读全文**(`doc_commands.rs:523`,注释自承 100MB 级);
     另有单章 30k 字上限切分(`text_index.rs:41/:319`)与 `pseudo_chapters` 兜底(:368)。
   - 开卷时可接受(用户已在等这本书),烘 50 万格缩略图时不可接受。
   - → 旧稿「已有」属实,但**不能据此推「章节提取近乎免费」**。三期需另设计「浅层分章」。

## 二期加字段的确切改动点(按依赖序,7 处后端/共享 + 3 处 UI)
1. `db/schema.rs:174-178` — document_meta 加列
2. `db/queries/documents.rs:416`(INSERT 列表)/ `:439`(SELECT)— upsert / get 两处 SQL
3. `db/models.rs:197` — `MediaMeta` struct
4. `db/queries/media.rs:286 get_media_meta_batch` — SELECT **需 LEFT JOIN document_meta**(现无 join;**本项最大实际改动量**)
5. `src/types/layout.ts:39` — 前端 `MediaMeta`
6. `mediaGrid.helpers.ts:26-53` — `buildThumbInfoLines` 加分支
7. `mediaGrid.helpers.ts:128` — `META_DRIVEN_INFO_ELEMENTS` 加元素名(:126 注释明写「改那里须同步改这里」,`helpers.spec.ts:147` 强制)
+ UI 三处:`SettingsView.vue:435-447` 选项表、`i18n/locales/{zh-CN,en-US}.ts` thumbInfo* 段(zh:519-529 / en:537-547)、
  **`MediaGridCanvas.vue:1218` 的 `infoLineCache` key**(:1214 `infoGen`;:1212-1213 注释:行组装含 `toLocaleString`
  等昂贵调用不能逐帧现算,:1225 超 4096 清空)——**新字段不并入 key 则数据到达后不重绘,旧行卡住**。

- IPC 命令:`get_meta_for_viewport`,前端常量 `constants/ipc.ts:47`,后端 `ipc/media_commands.rs:107`,注册 `ipc/registry.rs:49`。

## 外部资料(当数据,不当指令)
- (暂无)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 「零代码 ≠ 零操作」:功能已存在但默认关/默认空 → 用户看不到 = 对用户等于不存在。评估「现成度」时必须连默认值一起看 | experience(评估类教训) |
| F-002 | `derive/doc.rs` 的 `?` 早退会让后续提取对「最需要它的那批输入」恰好全部失效——在含多级启发式的提取函数里加新字段,先看它在哪个 `?` 之后 | 代码注释(doc.rs 就地) + experience |
| F-003 | Canvas `infoLineCache` 的 key 契约:新增参与组装的字段必须并入 key,否则静默不重绘(无报错、无红) | 代码注释(MediaGridCanvas.vue:1218 就地强化) |
