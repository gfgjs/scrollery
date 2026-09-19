---
status: 快照
type: working-memory
line: md阅读器巨型文档内存爆炸修复
created: 2026-07-17
---

# 任务计划:md 阅读器巨型文档内存爆炸修复

## 目标
巨型 md(实证件:6.6MB log.md)在阅读器中打开不再触发 GB 级内存与分钟级主线程阻塞:md 分片多 section(治本)+ 超大文档强制 scrolled 护栏 + renderMarkdown 断长跑 + txt 规则命中章大小 cap,全部带测试落地。

## 当前阶段
阶段 5:全量验证 + 收口

## 阶段

### 阶段 1:renderMarkdownBlocks + 断长跑(markdown.ts)
- [x] 提炼 `renderMarkdownBlocks(src): string[]`(块级 HTML 数组);`renderMarkdown` = join,零回归(列表收敛为单块,join 字节一致有测试钉住)
- [x] flushPara 断长跑:连续行每 200 行(MAX_PARA_LINES)拆新 `<p>`
- [x] markdown.spec.ts:join 等价性 + 断长跑 200/200/50 + 围栏不受影响(15/15 绿)
- **状态:** complete

### 阶段 2:md 分片多 section(syntheticBook.ts + BookReader)
- [x] `groupMarkdownBlocks`:h1/h2 起新片 + MD_SECTION_BUDGET_CHARS=24K 预算切片 + 单块超预算独占一片不内切
- [x] TOC:heading(h1-h3)起头的片产目录项(href=片号);无 heading → toc=[]
- [x] BookReader md 分支:逐块高亮(全篇无围栏则零开销跳过)→ blocks 进多 section 书;每片 blob URL 独立 load/unload/destroy
- [x] syntheticBook.spec.ts:分组规则 5 例 + 多 section 契约 7 例(47/47 绿含 markdown)
- **状态:** complete

### 阶段 3:超大文档护栏(强制 scrolled + 提示)
- [x] 护栏改为**单片超限**判定(MD_FORCE_SCROLLED_SECTION_CHARS=256K,见决策更新):BookReader 检出超限片 → forcedScrolled + emit('flow-forced')
- [x] DocumentViewer:toast 提示一次 + 流选择器 disabled + title 说明;换文档复位
- [x] shiki 护栏:单块代码 >100K 字符跳过高亮回退纯文本(MAX_HIGHLIGHT_CHARS)
- [x] i18n zh/en:doc.flowForcedScrolled
- **状态:** complete

### 阶段 4:txt 规则命中章 cap(text_index.rs)
- [x] push_chapter_capped:>MAX_RULE_CHAPTER_CHARS(30K)按 PSEUDO_CHARS 行边界续切「原题 · N」;前言块同 cap
- [x] Rust 单测:巨章切分/子章有界/区间无缝/常规章不受影响(reader 模块 43/43 绿)
- [x] **顺带修出独立真 bug(F-004)**:chardetng 截断采样 last=true 致 >64KB 无 BOM UTF-8 静默误判 windows-1252;修 feed(last=!truncated) + 回归测试
- **状态:** complete

### 阶段 5:全量验证 + 收口
- [ ] 前端全量:vitest + vue-tsc + eslint(quiet)
- [ ] Rust:cargo test(text_index)+ fmt + clippy(触及面)
- [ ] 真机手测步骤给出(GUI 不自动化,标注)
- [ ] 三件套收口迁 worklogs + todo 回写
- **状态:** pending

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 分片预算 ~24K HTML 字符,块边界分组,单块超预算独占一片不再内切 | 对齐 txt PSEUDO_CHARS 量级;实测 1.9M 字符 section 已 2.5GB,24K 远离危险区;内切块破坏 `<pre>` 等语义 | D-001 |
| TOC v1 只做「heading 起头的 section」级目录,不做片内锚点 | R2-4b 全量锚点是独立增强;href=index 与现有 resolveHref/splitTOCHref 零改动兼容 | |
| 护栏判定从「全文 >1M 字符」收窄为「存在单片 >256K」(实施更新) | 分片后总量不再是风险维度(每片有界、懒加载);唯一残余风险=单块超预算独占的片(巨型围栏),按片判精准不误伤大而正常的 md | D-002 |
| chardetng 截断采样改 feed(last=false)(顺带修 F-004) | 64KB 切点落多字节序列中间时 last=true 使 UTF-8 候选被误杀,>64KB 无 BOM UTF-8 整册静默 mojibake;流式语义下不完整尾序列合法 | D-003 |
| 存量 md 阅读进度(单 section CFI)不做迁移,靠 BookReader init 失败回退开卷 | 既有 catch 路径已优雅回退;迁移映射成本与收益不成比 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 分组预算测试 fixture 少算 `<p></p>` 7 字节开销,期望 3 片实得 4 | 复核 groupMarkdownBlocks 语义确认代码正确 | 改 fixture 块长 7007(3 块 21021 ≤ 24000) |
| txt cap 测试首跑全军覆没:titles 全是 mojibake | 断言注入 enc= 探针 → 实证 chardetng 误判 windows-1252 | 根因非本修复而是编码检测截断 bug(F-004),先修它测试即绿 |
