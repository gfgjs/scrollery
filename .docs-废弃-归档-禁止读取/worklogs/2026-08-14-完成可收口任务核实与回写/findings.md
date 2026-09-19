---
status: 快照
type: 工作记忆
line: 完成可收口任务核实与回写
created: 2026-08-14
---

# 发现:完成可收口任务核实与回写

## 核证方法
- 文档自述不采信;每线以 git commit 存在性 + 代码符号/文件存在性双证据核证
- 辅助:08-10 盘点报告(165 条)作交叉参照,但每条仍独立复核

## 最终结论(2026-08-14)

### A 类:已完成未登记 → 已新登记 todo.md(25 条)
见 todo.md「2026-08-14 核证表」。全部有 commit 或符号证据:
- git 实测 commit:cb459c4/2940600/ac07f9f/475cb5a/324453a/9cd4bfb/e1b1789/6d92779/f931db5/5018c76/da83688/835fd73/1f3ea06/b216dcc/5b0b925/03cd68c..cc88f8c/c6cc2c6/f0f5654/e8ebdf8/141cd32/8bee8e4/9f5ebb1/a60acfc/ecea926/61d125f/efec238/0d1234a/305d4b6/8b0220f
- 符号/文件:StatusBarFileInfo(3)/full_thumb_gen_status(5)/route-fade 零残留/useViewerImageSource(3)/auto_hide_chrome_windowed(2)/axis_mode(2)/is_hidden(10)/video/d3d.rs/backup/{core,restore,swap,dbread,manifest}.rs/docs/spec 17 篇/Spec15 891 行

### B 类:todo.md 断言过期 → 已划线订正(3 处)
1. L43 降噪超分「未施工等 J-1..J-8」→ J 已定案 + P0 施工批1-3 落地(enhance-worker 已建,host service.rs 1210 行),残余 F-02 PENDING 占位
2. L35 UIUX「S7 未开始」→ S7 已收官(07-15 round9),S1 原语全建成
3. U-P1-b「lib.rs 832 行重测 churn」→ lib.rs 实测 368 行,e92e2b2 已 boot 化拆分,前提失效

### 文档残留修正(2 处)
- docs/status/asbuilt-spec.md:11 计数 16→17(Spec00–15),「待主线提交」→ 已提交 dev
- Spec15 三件套 task_plan 阶段 2 勾选 complete(475cb5a)

### C 类:确认未完成(维持未登记,与文档一致)
- 2026-08-03-首次AI语义搜索结果不显示:仅诊断;useBucketVirtualScroll.ts:358 仍 error 粘滞复证
- 2026-07-23-OCR文字提取/自定义ICC与色域切换:施工在飞
- 2026-07-17-文档缩略图叠加标题与章节:等 D-C/D-D
- 2026-07-21-备份测试硬化专项:立项≠开工
