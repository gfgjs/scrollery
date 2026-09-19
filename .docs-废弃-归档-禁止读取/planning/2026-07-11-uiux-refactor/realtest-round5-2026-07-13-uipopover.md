---
id: 2026-07-13-realtest-round5-2026-07-13-uipopover
title: 第五轮真机验收清单（S5/S3「UiPopover 原语 + 迁 3 弹层」）
status: active
type: acceptance
created: 2026-07-13
acceptance: 待真机验收
snapshot_date: 2026-07-13
scope: UiPopover 原语（@floating-ui 引擎）+ 迁 date/filter ⋯/view ⋯ 三弹层
commits: 4b02272, 7c7289f
---

# 第五轮真机验收清单

> 🔀 **本清单已并入 `realtest-round10-2026-07-15-合并验收.md`，验收以后者为准**（2026-07-15）。
> ⚠️ **§B 首项的「右缘对齐 / placement=bottom-end」已被 round7 推翻**（改为居中），下方正文已就地更正。

> S5/S3 弹层定位前置：新增 **UiPopover 原语**（委托 @floating-ui 做 flip/shift/autoUpdate 定位数学，
> 原语持有 Teleport + useFocusTrap 焦点陷阱 + backdrop/Esc dismiss 契约），全库 3 处手写弹层（date +
> filter ⋯ + view ⋯）已全部迁入，positionMenu 式手算定位归零。逻辑已过门禁（typecheck/lint/vitest 775/
> build），以下列**门禁盲区**的真机点——定位视觉/flip/焦点陷阱/嵌套均 SSR 不可验，须真机。
> 起真机 `npm run tauri dev`。

## A. date 弹层（GalleryFilterChips，阶段1 4b02272）

- [ ] 点日期 chip → 弹层在 chip 下方左对齐弹出（placement=bottom-start），淡入过渡正常。
- [ ] **修 bug 验证**：把窗口拉窄 / 让日期 chip 靠近屏幕右缘再打开 → 弹层**不越界出屏**（shift 中间件把它推回视口内；旧实现漏水平钳制会溢出）。
- [ ] **flip 验证**：让 chip 靠近视口**底部**（窄高窗口 / 滚到底）再打开 → 弹层自动翻到 chip **上方**（放不下就 flip），不被视口底裁切。
- [ ] **autoUpdate 跟随**：弹层开着时滚动画廊 / 拉伸窗口 → 弹层**跟随** chip 移动而非错位悬挂（旧实现 resize 会错位，本项要求跟随）。
- [ ] **焦点陷阱（新增）**：打开后焦点落「起始日期」输入；Tab/Shift+Tab 在两输入 + 清除/完成键间循环，不逃逸到背景；关闭后焦点归还日期 chip。
- [ ] Esc 关闭；点弹层外（透明 backdrop）关闭；两日期填好后 chip 显示区间标签、active 高亮正常。
- [ ] 表面视觉（bg/border/圆角/阴影/内边距）与迁移前**逐像素一致**（表面 CSS 未动，只搬进 slot）。

## B. filter ⋯ / view ⋯ 菜单（AppToolbar，阶段2 7c7289f）

- [ ] 窗口收窄触发溢出 → 点「筛选 ⋯」/「视图 ⋯」→ 弹层在按钮下方**居中**弹出（`placement="bottom"`）。
      🔴 **本项原文已过期并更正**：本轮（round5）落地时为 `bottom-end` 右缘对齐（等价旧 positionMenu 的
      `rect.right-width`），**已被 round7「卡片弹出 + 与被点 ⋯ 按钮居中对齐」需求推翻**（`AppToolbar.vue:232/245`
      现为 `placement="bottom"`）。照原文「右缘对齐」测会报出假 bug。
- [ ] filter 菜单内 3 列等宽网格布局、view 菜单内「标签 : 控件」两端对齐 —— `:deep()` 规则仍命中（Teleport 产物带父 data-v），与迁移前一致。
- [ ] **窗口三键仍可点**：弹层开启时，标题栏右上角最小化/最大化/关闭三键**一次点击即响应**（backdrop inset 让出 --titlebar-height，未覆盖标题栏）。
- [ ] **行为变更验证**：弹层开着时拉伸窗口 → 弹层**跟随重定位**（autoUpdate），**不再**像旧实现那样直接关闭。确认这是期望行为（若更想「resize 即关」请反馈）。
- [ ] 焦点陷阱：打开后焦点入菜单，Tab 在菜单内 chip/控件循环；Esc 关闭；点 backdrop 关闭。
- [ ] 菜单内 chip/控件的实际操作（切筛选、切视图布局等）与内联实例行为一致（经 store 同步）。

## C. 🟡 嵌套弹层（filter ⋯ 菜单内的 date 弹层）

- [ ] 打开「筛选 ⋯」菜单 → 菜单内点日期 chip → date 弹层在其上层弹出，两层都在。
- [ ] Esc 只关**最上层** date 弹层、filter 菜单仍开（date 弹层 @keydown.esc.stop 阻冒泡，不误关外层）。
- [ ] date 弹层关闭后焦点归还菜单内的日期 chip，filter 菜单焦点陷阱仍有效。
- [ ] **已知 nested-dismiss 差异**：date 弹层开着时，点 filter 菜单**本体区域**是否关闭 date 弹层？当前 z-index（两层 backdrop 均 300、floating 均 301）下，date backdrop(300) 低于 filter floating(301)，**预期不关**（旧实现 date backdrop 300 > filter ~200 会关）。若此差异影响使用，反馈 → 改动态 z-index（按打开序递增）。

## D. 跨主题抽查

- [ ] 6 主题（墨/素/宣/玄/黛/月白）各抽 1-2 个，确认弹层表面 bg/border/阴影跟随主题、文字对比度正常（表面走 `--color-bg-surface`/`--color-border`/`--shadow-lg` 变量，理应自动跟随）。

## E. a11y 语义（读屏；本轮新增 aria-pressed / aria-expanded）

- [ ] 读屏（NVDA / VoiceOver）聚焦「图片/视频/Live/收藏」筛选 chip → 播报「切换按钮 · 已按下/未按下」，点击切换后 pressed 状态随之更新。
- [ ] 读屏聚焦 date chip / 「筛选 ⋯」/「视图 ⋯」钮 → 播报「有弹出 · 已折叠」，打开弹层后变「已展开」（aria-expanded + aria-haspopup）。
- [ ] 评分 / 颜色 chip **不**误报 pressed（它们是 StarRating / ColorLabelPicker 的容器 div，非 toggle button，各控件自持语义）。

## 结论

- [ ] 全部通过 → UiPopover 定位/交互契约在真机确立；filter a11y 只剩 chip aria-pressed 语义（另起）。
- [ ] 若有问题：记录症状 + 复现步骤，回写本清单对应项 + findings。
