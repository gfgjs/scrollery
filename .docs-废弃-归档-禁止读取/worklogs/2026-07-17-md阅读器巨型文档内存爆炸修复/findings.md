---
status: 快照
type: working-memory
line: md阅读器巨型文档内存爆炸修复
created: 2026-07-17
---

# 发现与决策:md 阅读器巨型文档内存爆炸修复

## 需求
- 用户真机:打开 `C:\More\0\executor\log.md`(6.6MB Python 日志改名件)→ WebView2 renderer 6.3GB;要求查明原因并修复/预防。
- 用户采纳全部四项建议:分片治本 + scrolled 护栏 + renderMarkdown 断长跑 + txt 章 cap。

## 发现(2026-07-17 会话实测,Edge 1600×900,同 WebView2 引擎)
- 实证件:37,984 行 / 空行 3,306 / 最长连续无空行 9,238 行;renderMarkdown 产出 3,307 个 `<p>`、8.8MB HTML;单 `<p>` 内 9k+ `<br>`。
- 测量阶梯(renderer private bytes):
  - 裸 Chromium multicol 同 HTML:442MB,~5s 布局完成,滚动/翻页零增长。
  - 忠实 foliate paginated 全量:**7.9GB 仍在爬,主线程阻塞 >3min,view.init 永不完成**(用户 6.3GB 截图=同曲线中途)。
  - foliate paginated 1/4 量(1.9MB):2.5GB,>5min 未就绪 → 时间超线性,内存 ~GB/MB 级放大。
  - foliate **scrolled** 全量:**379MB,3s 就绪**,翻页正常。
- 结论:凶手=「巨型单 section × CSS multicol × foliate 开卷期全文档几何查询」交集(paginator getVisibleRange 对 ~37k 文本节点逐个 Range+rect、expand 的整文 contentRange rect、锚点二分),非稳态泄漏——关文档即回收。
- `_` 下划线与本案无关:markdown.ts 斜体只认 `*`。
- txt 路径近似免疫:text_index.rs pseudo_chapters ~10K 字符兜底;但 split_by_rule 命中路径无章大小上限(稀疏命中可产多 MB 章)= 残余缺口。
- md 路径零上限:syntheticBook.ts buildMarkdownSyntheticBook 整篇单 section,注释自认 R2-4b 分章「待做」。
- 复现件:scratchpad `foliate-repro.html` + `cdp-measure*.mjs`(会话级临时目录,不入库;方法=CDP 驱动真 vendored foliate + 逐进程 PrivateMemory 采样)。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 「内存泄漏」表象需先分流:稳态泄漏 vs 无界进行中工作(布局永不完成)——排查手段完全不同;本案实测阶梯法(裸引擎→忠实管线→变量对照)一次定位 | experience |
| F-002 | CSS multicol 是乘积型放大器:整文档 rect 查询 × fragmentainer 数;任何「单容器塞全文再分栏」的阅读器管线都有此雷,epub 靠一章一 iframe 天然拆弹 | experience |
| F-003 | 阅读器 section 大小是硬约束(md 24K HTML 预算 / txt 10K 字符),新增文档格式接入 foliate 必须带分片 | design/代码注释 |
| F-004 | **顺带挖出真产品 bug**:chardetng 采样 64KB + `feed(last=true)`,切点落多字节序列中间 → >64KB 无 BOM UTF-8 中文书整册静默误判 windows-1252(1252 全字节可映射,替换率恒 0,Lossy 告警不响)。测试 fixture(135KB 纯 CJK UTF-8)意外必现。修=截断采样 feed(last=false)。教训:采样窗切多字节编码必须按流式语义喂检测器;"告警不响"≠"没病"——窄检测器的兜底告警要审「哪类错根本不触发它」 | experience |
