---
id: 2026-07-13-realtest-round4-2026-07-13
title: 第四轮真机验收清单（S5 阶段11「Gallery empty-state 重构」）
status: active
type: acceptance
created: 2026-07-13
acceptance: 待真机验收
snapshot_date: 2026-07-13
scope: UiEmptyState 原语 + resolveGalleryEmptyState + 修 filtered-empty 错 CTA
commits: e0a85ca
---

# 第四轮真机验收清单

> 🔀 **本清单已并入 `realtest-round10-2026-07-15-合并验收.md`（§3.1），验收以后者为准**（2026-07-15）。

> S5 阶段11 empty-state 重构收官:抽 **UiEmptyState 原语**（同构包裹全局 `.empty-state*`）+
> **resolveGalleryEmptyState 纯决策源**,迁 MediaGrid 内联空状态,并**修一个真实错 CTA**——筛选无匹配
> 时此前误引导「添加文件夹」。逻辑已过门禁（typecheck/lint/vitest 769/build），以下列门禁盲区的真机点。
> 起真机 `npm run tauri dev`。

## A. 🔴 filtered-empty 修错 CTA（本轮核心）

- [ ] 在**有照片**的视图（如「全部照片」）里,施加一个**匹配不到任何照片**的筛选（例:评分设为 5 星、
      或选一个没有照片的色标、或把日期范围设到很久以前）：网格应显示 **「没有符合筛选条件的照片」** 标题 +
      **「试试调整或清除当前的筛选条件。」** 说明,以及一个 **「清除筛选」** 按钮（**FilterX 图标**,非文件夹图标）。
- [ ] 点 **「清除筛选」**：筛选应被清空,照片**立即重新出现**（live 重算,无需刷新）。
- [ ] **stale filter 陷阱**：先施加一个筛选,再切到一个**本身有照片但全被该筛选挡掉**的文件夹/收藏夹：
      应显示 filtered-empty（「没有符合筛选条件的照片」+ 清除筛选）,**而非**「文件夹是空的」（此前的错误方向）。
- [ ] 语言切换 zh↔en：filtered 文案与「清除筛选 / Clear filters」按钮均正确翻译。

## B. 既有空状态回归（迁移后视觉/文案不变）

- [ ] **空库**（真正无照片的「全部照片」视图,无筛选/搜索/目录/人物）：**「这里空空如也」** + 说明 +
      **「添加文件夹」** 按钮（FolderPlus 图标）→ 点击应触发加目录流程（复用 FoldersSection,不重复实现）。
- [ ] **空文件夹**（选中一个空目录）：**「文件夹是空的」**,**无**动作按钮。
- [ ] **空收藏相册**：**「还没有收藏任何照片」** + 「点击照片右下角的心形图标即可收藏。」,无动作按钮。
- [ ] **最近添加 / 实况照片为空**：对应单行文案,无说明行、无动作按钮。
- [ ] **搜索无结果**：**「没有找到匹配 "xxx" 的内容」**（带查询词插值）,无动作按钮。

## C. 原语视觉 parity（同构包裹,应与迁移前逐像素一致）

- [ ] 空状态整体：图标（48px,opacity 0.4,居中）、标题（lg/600）、说明（base,max-width 320,行高 1.6）、
      动作按钮与说明的间距——应与本次改动前**看不出差别**（全局 `.empty-state*` CSS 未动,仅 DOM 由内联改为原语渲染）。
- [ ] 动作区容器 `.empty-state__actions`：单动作时居中；若未来出现双动作应横向并排（本轮各场景均单动作,主要验居中与上间距）。
- [ ] 骨架屏/加载态不受影响：切视图/切筛选重算时,旧布局 stale-while-revalidate 不闪骨架（此项为既有行为,顺带确认未回归）。

## 已知边界（非本轮门禁覆盖 / 非本轮范围）

- 上述均为门禁盲区的**真机 GUI/交互**验证（文案出现、按钮显隐与图标、清除筛选 live 生效、视觉 parity）；
  决策逻辑（precedence + 动作门控）已由 `resolveEmptyState.spec` 10 例穷举锁定,结构由 `UiEmptyState.spec` 5 例 SSR 锁定。
- **error 状态未做**：compute_layout 失败目前仍渲染为空库（无独立 error 分支）——需接后端 compute 失败管线,属独立较大改动,不在本轮。
- **filter a11y/popover 与 selection bar 改造未做**：前者需 UiPopover 原语（需发明定位抽象=决策）+ 真机；
  后者（浮动可拖胶囊→底部固定动作条）需产品决策 + 真机拖拽验证。二者是 S5 阶段11「Gallery filter/selection」剩余项,见 task_plan 阶段11。
- filtered-empty 给了 **FilterX** 图标以区别于空库的 ImageIcon（设计 §8.4「不同状态用不同图标」精神）；若视觉上更希望统一 ImageIcon,可随时改回（非契约冻结）。
