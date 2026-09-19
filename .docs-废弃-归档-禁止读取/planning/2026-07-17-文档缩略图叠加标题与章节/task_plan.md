---
status: active
type: working-memory
line: 文档缩略图叠加标题与章节
created: 2026-07-17
---

# 任务计划:文档缩略图叠加标题与章节

## 目标
让文档类缩略图(txt/md/office/pdf/epub)在格子上显示**文件名 / 内嵌标题 / 章节**,提取在后台静默进行,
不阻塞画廊、不拖慢扫描。终态:文本卡不再是「四条假线 + 扩展名」的哑卡,一眼能认出是哪本书。

> 本线由「批量15项问题清单」线(`docs/planning/2026-07-17-批量15项问题清单/`)的 #13 拆出。
> 用户 2026-07-17 裁决:#13 另起独立线,在新会话单独接续做。原设计稿正文已迁入本线 findings.md。

## 当前阶段
阶段 0:已建线、摸底已核实完毕(证据全在 findings.md)。**等用户拍板 D-C(一期做不做/视觉样式)后开工。**

## 阶段

### 阶段 0:建线与摸底核实
- [x] 从批量15项线拆出,三件套建立
- [x] 旧设计稿 6 条说法逐条核实(5 真 / 1 语义偏松),另揪出 3 处过度承诺(见 findings「设计稿更正」)
- [x] 补摸底:设置项点法、Canvas 文字绘制能力、viewportMeta 加字段的 7+3 个改动点
- **状态:** done

### 阶段 1:零期——文件名(零代码,但**非零操作**)
- [ ] 确认:设置 → 缩略图 → 「缩略图信息悬浮窗」(默认已开)→ 勾「文件名」即生效,DOM+Canvas 双路已通
- [ ] 待定:默认是否把 `filename` 预置进 `thumbInfoElements` 默认值(现为空数组 → 开箱看不到任何信息行)
- **状态:** pending(**等 D-C**)
- **注意**:不覆盖 compact 模式(`MediaThumb.vue:446` 直接 `return []`);只给文件名,不给标题

### 阶段 2:一期——文本卡画文件名(小)
- [ ] DOM 侧 `MediaThumb.vue:48-60` 文本卡分支:把四个空 `<span>` 假行换成真文件名(截断/多行)
- [ ] Canvas 侧 `MediaGridCanvas.vue:1165 drawTextCard`:同上,复用 `truncateToWidth`(:104)
- [ ] 两路视觉对齐(Canvas 与 DOM 的字号/行数/截断位须一致,否则切换渲染模式会跳)
- **状态:** pending(**等 D-C** 定视觉样式)
- **不依赖信息行开关**——这是与零期的关键区别:一期是卡面本体,零期是叠加层

### 阶段 3:二期——内嵌标题(大)
- [ ] 后端 epub:`derive/doc.rs` 加 `find_dc_title(&opf_xml)`,**必须上提到 :101 的 `?` 之前**(见 findings 陷阱①)
- [ ] 后端 pdf:`DocThumbRenderer.vue` 经 pdf.js `getMetadata()` 顺带取,随 `STORE_DOC_THUMBNAIL` 回传
- [ ] schema:`document_meta` 加 `title` 列(现仅 3 列)+ DAO 两处 SQL
- [ ] `get_media_meta_batch` 加 **LEFT JOIN document_meta**(本项最大实际改动量)+ MediaMeta 前后端加字段
- [ ] `META_DRIVEN_INFO_ELEMENTS` 加元素 + `buildThumbInfoLines` 加分支 + 设置面板选项 + i18n 双语
- [ ] Canvas `infoLineCache` 的 cache key 并入新字段(:1218),否则数据到达后不重绘
- **状态:** pending(**等 D-D** 定优先级)

### 阶段 4:三期——章节(大,性能敏感)
- [ ] 关键约束:`text_index` **不可直接复用**——`ensure_text_index` 走 `fs::read` 整读全文(见 findings 陷阱②)
- [ ] 需设计「浅层分章」:只读头部若干 KB 出前 N 章,或把 text_index 降级为后台 enrichment 尾段 + cap
- [ ] `document_meta` 加 `chapters`(JSON)列
- **状态:** pending(**等 D-D** 定性能预算)

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| #13 从「批量15项」线拆出独立立项 | 零/一/二/三期跨度大(零代码→schema+IPC+双路渲染),范围需用户逐期裁;挂在 15 项线里会让那条线无法收口 | |
| 叠加走**前端覆盖层**而非烘进图 | 覆盖层可随主题重着色、改文案不需重烘全库;烘进图则每次换主题/改字号都要重生成 50 万张 | |

## 待用户裁决(开工前置)
- **D-C(阻塞阶段 1/2)**:一期(文本卡画文件名)是否直接做?视觉样式?
  是否把 `filename` 预置进 `thumbInfoElements` 默认值(影响开箱观感)?
- **D-D(阻塞阶段 3/4)**:二期/三期优先级;后台提取的性能预算
  (50 万库全量取 epub title 的上限策略;txt 章节是否只做浅层)。

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
