---
status: 快照
type: working-memory
line: 前端动画重构
created: 2026-07-22
---

# 进度日志:前端动画重构

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:阶段 3 收口位,全量前端门禁已过;真机手测揪出 route-fade 黑屏回归并已修复(commit 1f3ea06)
- 未解错误:无
- 关键指针:findings.md + attachments/anim-inventory.md + attachments/architect-plan.md(§P5 GUI 手测清单,余 5 条,not automated)+ task_plan 决策 D-001/D-403/D-404/D-405 + 错误账(route-fade 黑屏根因)
- 遗留:剩余 GUI 真机手测(用户执行)+ 六 commit 待批 push

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:<什么做法值得复用>
- 教训:<什么坑值得预警>
- 意外:<什么假设被现实推翻>

## 会话:2026-07-22
- 做了:阶段 0 完成,三路摸底回执齐(动画清单/缺失点位/冲突区),无代码改动。
- 验证:<命令 + 结果证据>
- 遗留:<阶段号;内容定义在 task_plan,不复述>

## 会话:2026-07-22(阶段 1-2 收尾)
- 做了:阶段 1 architect 设计(attachments/architect-plan.md)+ 阶段 2 施工五批全落——
  - A=8bee8e4 动画 token 补档+画廊↔查看器 route-fade 路由过渡+spinner 统一
  - C=9f5ebb1 UiDialog 补关闭 leave 动画
  - D1/D2=a60acfc 硬编码动画时长收编 token 25 处(顺带清 7 文件 prettier 折行债)
  - B=ecea926 大图浏览器切图 opacity dip 过渡(ContentViewer 争用区最小改)
  - 复核:opus 深审(A/C 零发现,B 两 minor 已修+增量核验)+ cavecrew 机扫无发现
- 验证(阶段 3 全量前端门禁,本次执行):
  - `npm run lint` → exit 0
  - `npm run typecheck` → exit 0(`vue-tsc --noEmit`)
  - `npx vitest run --reporter=dot` → exit 0,Test Files 109 passed (109) / Tests 1345 passed (1345)
- 遗留:阶段 3;GUI 真机手测清单见 attachments/architect-plan.md §P5(6 条,not automated);五 commit 待批 push

## 会话:2026-07-23(真机回归修复)
- 做了:用户真机手测第 1 项即触发画廊进大图黑屏(`.content-viewer`/`.app-content` 均空,控件全失)。根因=App.vue `<Transition mode="out-in">` 包 KeepAlive 在 Vue 3.5.13 上 leave 完成后新组件不插入(模板与 vuejs/router#1655 逐字同构,相邻已知缺陷 vuejs/core#12465,3.5.13 早于其修复合并)。修复:撤路由级 Transition,查看器进入动画改纯 CSS 挂载 keyframe(`viewer-in`,挂 `.content-viewer` 根,组件每次全新实例自动重放)。
- 验证:`npm run typecheck` exit 0;`npx eslint src/App.vue` exit 0;`rg "route-fade" src` 零命中;真机复验大图正常显示+淡入缩放、返回画廊滚动位保持。
- commit:1f3ea06(fix)。
- 遗留:阶段 3;剩余 GUI 手测 §P5 5 条(连翻 dip/对话框关闭/reduce/万级帧率/深链);查看器关闭无淡出、其他路由无过渡属已知代价,defer 待升 Vue 版本重议路由级过渡方案;六 commit 待批 push。
