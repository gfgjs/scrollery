---
status: 快照
type: working-memory
line: md阅读器巨型文档内存爆炸修复
created: 2026-07-17
---

# 进度日志:md 阅读器巨型文档内存爆炸修复

## 会话:2026-07-17
- 做了:根因调查收官(测量数据见 findings)+ 三件套立项;阶段 1-4 全部施工完毕:
  ① markdown.ts renderMarkdownBlocks + 断长跑(行 200/字符 6K 双上限,首版只有行上限,
     实测 240 字符长行日志产 50-80K 单块击穿 24K 预算 → 补字符上限对齐);
  ② syntheticBook.ts groupMarkdownBlocks(h1/h2 分片 + 24K 预算 + 单块不内切)+
     buildMarkdownSyntheticBook 多 section({html}→{blocks} 破坏性改签名,唯一生产调用方同步);
  ③ BookReader 逐块高亮 + forcedScrolled 护栏(单片 >256K)+ flow-forced 事件;
     DocumentViewer toast + 流选择器禁用;shiki 单块 >100K 跳过;i18n zh/en;
  ④ text_index.rs push_chapter_capped(规则命中章 >30K 按 10K 续切「原题 · N」);
  ⑤ 顺带修 encoding.rs chardetng 截断采样 bug(F-004/D-003)。
- 验证(证据):
  - vitest 全量 88 文件 1176 测试全绿(含新增 markdown 7 例/syntheticBook 12 例);
  - vue-tsc --noEmit 零错;eslint 触及 9 文件零告警;
  - cargo test reader:: 43/43 绿;clippy -D warnings 过(修 1 处 needless_range_loop);fmt --check 静默;
  - cargo test --lib 全量:614 过 / **3 失败全在 exotic 模块**(期望旧缩略图档位 480,
    b554aa5「档位重定 64-128-256-512-1024」的既有欠账,src-tauri 工作树除 reader 两文件外零改动,
    与本线无关,已报告用户、未越线代修);
  - **端到端复现对拍**(esbuild 打包生产管线 → 真 foliate + CDP 实测,同一 6.6MB log.md paginated):
    修复前 7.9GB 且 >3min 主线程阻塞永不就绪 → 修复后 **177MB、3 秒就绪、翻 12 页平稳**;
    分片统计:4283 块 / 377 片 / max 24,049 字符 / avg 20,342 / 管线 76ms。
- 遗留:GUI 真机手测(打开该 log.md 验证不再爆内存 + 超大围栏件走 scrolled 提示)——不自动化,
  标注 not automated;存量 md 阅读进度(单 section CFI)开卷回退章首属预期。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
