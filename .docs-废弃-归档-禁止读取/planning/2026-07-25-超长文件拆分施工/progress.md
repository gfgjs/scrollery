---
status: active
type: working-memory
line: 超长文件拆分施工
created: 2026-07-25
---

# 进度日志:超长文件拆分施工

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:全线收官——P1..P5 全落地(derivations.rs 按 D-454 明示跳过);P6 全量清算完成(A 段 7 门禁+B 段全角对拍),0 新增红,1 处折损已还原。dev 尖端 9047a12。
- 未解错误:无(既有红 2 项均非本线:cargo test error.rs flaky、vitest alignment-grid 契约既有红,均已核实与本线改动文件无关)
- 关键指针:D-452..454(task_plan);方案正文=方案线 analysis/;终态 commit——260ebd4(tierB-4)、9047a12(P4 尾件)、本轮 worker_log.rs 全角还原 commit(见下)。
- 遗留:derivations.rs 待用户裁归属;⏸GUI 手测清单(各批复核文档内);子组件抽取(MediaGrid 3 件+ContentViewer 4 件)留账待单独授权。

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-25
- 做了:建施工线三件套;D-452..454 拍板;波1 起跑(P1 CSS首刀 + P3 layout.rs/worker_client.rs)。
- 验证:暂无
- 遗留:P1..P6 全部

## 会话:2026-07-25(续,收口回写)
- 做了:P1/P2 CSS 外置落地并折叠;P3 layout.rs/faces.rs 进 dev,scan.rs/worker_client.rs 复核中,lib.rs 施工中;D-453 因 stale 基点事故修订为手建 worktree+基点硬门;方案线 layout-rs.md/faces-rs.md 两处复核发现回写更正。
- 验证:commit 指针——ba3abde(P1 CSS)、fa6e8e2(P2 CSS×5)、5bc6fba(layout 进 dev)、c2de682(layout doc 链接修)、33a4cd8(faces 进 dev)、63e22d5(faces 两注释项修,dev 尖端)。
- 遗留:scan.rs/worker_client.rs 复核收尾、lib.rs/P4/P5 施工推进、P6 全量清算

## 会话:2026-07-25(续二,回写)
- 做了:P3 lib.rs 深度复核处置(StartupFailure thiserror 化+删死字段+段 c/d 序采方案裁决)进 dev;P4 MediaGrid/ContentViewer 落地,DocumentViewer 施工中;P5 tierB-1 进 dev,tierB-2 施工中。
- 验证:commit 指针——b65e6d3(tierB-1)、e92e2b2(lib.rs 拆分,cherry-pick 冲突手解)、de95b78(lib.rs 复核修)、8cde88f(ContentViewer)、f72f71b(MediaGrid 轴测试 28 用例)。
- 遗留:DocumentViewer/FoldersSection/MediaGridCanvas/SettingsView(P4)、tierB-2/3/4(P5)、P6 全量清算、ContentViewer 特征测试 ×2 补写

## 会话:2026-07-25(续三,回写)
- 做了:P4 ContentViewer 特征测试补齐(7f54155)、DocumentViewer 拆分+复核修 3 项+方案 §5 五 spec 补齐、FoldersSection 拆分+复核抓 1 严重修复;P5 tierB-2/tierB-3 联合提交落地,tierB-4 施工完转复核中。
- 验证:commit 指针——7f54155(ContentViewer 特征测试)、3e07612(DocumentViewer 拆分)、9f02a5f(DocumentViewer spec 27 用例)、7430abe(tierB-2/3 联合)、2a73de5(FoldersSection 拆分+回归锁)。
- 遗留:MediaGridCanvas/SettingsView(P4)、tierB-4 复核收尾(P5)、P6 全量清算

## 会话:2026-07-25(续四,P6 全量清算收口)
- 做了:P4 尾件 MediaGridCanvas+SettingsView(9047a12)、P5 tierB-4(260ebd4)确认已落;跑 A 段全量门禁 7 项(cargo fmt/clippy/test、npm typecheck/vitest/build、eslint);B 段全角折损全量对拍(python FF01-FF5E 规整化+difflib,覆盖清单全部 13 组 commit/文件族)。
- 验证:A 段 7 门禁除两处既有红(cargo test error.rs 单测 flaky——`--lib` 单独重跑通过;vitest alignment-grid 契约既有红 1 例)外全绿,均核实与本线改动文件无关。B 段发现 2 处候选折损,1 处(scan/media_upsert.rs)此前 b9d666b 已修维持,1 处(exotic/worker_log.rs L187)本轮还原全角标点,还原后 fmt/build 复跑绿。
- 遗留:derivations.rs 待用户裁(D-454);⏸GUI 手测清单(各阶段复核文档);子组件抽取留账待单独授权。
