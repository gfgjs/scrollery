---
id: 2026-07-22-architect-plan
status: snapshot
type: plan
created: 2026-07-22
line: 前端动画重构
---

# architect 施工计划(阶段 1 产出,主线代写落盘)

行号基于 2026-07-22 dev 工作区。主线裁决:spinner 统一 800ms 先落;P3#19 只加注释;B 批独立 commit 先行;A/B/C 并发(C、B 实际不依赖 A),D1/D2 等 A。

## P1 动画 token 体系

现状:token 已存在且覆盖率高——`src/assets/styles/variables.css:39-51` 已有 `--ease-out(0.16,1,0.3,1)`、`--ease-in-out(0.4,0,0.2,1)`、`--ease-spring(0.34,1.56,0.64,1)`、`--duration-fast:120ms / normal:200ms / slow:350ms` 及复合 `--transition-fast/normal/slow`;119 个 transition 点约 90 个已用 token。本线不是新建体系,是补档 + 收编约 25 个硬编码离群点。

1. 补两档 duration(落点 variables.css:46 后,中文注释说明档位语义):`--duration-moderate: 280ms`(抽屉/面板/折叠滑动档,收编 0.26s/0.28s/0.3s 簇)、`--duration-spin: 800ms`(spinner 循环统一档,收编 0.6s/0.7s/0.8s/1s)。
2. 不加复合 `--transition-moderate`(两簇缓动不同,调用点自行组合)。
3. 不加 `--ease-spring-soft`:MediaGridCanvas cubic-bezier(0.34,1.18,0.64,1) 单消费点且系性能调参,defer。
4. 迁移策略:新增动画一律 token;P3 一次收编 25 离群点;其余不动。无 stylelint 门(引新依赖,defer),rg 审计兜底(P5)。

## P2 缺失动画逐点方案

### P2-1 画廊 ↔ 查看器路由过渡(含全部页面级路由切换)
- 挂点 `src/App.vue:68-72`:RouterView v-slot 内包 `<Transition name="route-fade" mode="out-in">`,结构 Transition > KeepAlive(现有)> component :is。**禁**给 component 加 `:key`——同组件路由('/'↔'/folder/:id' 均 MediaGrid)靠组件同一性天然不触发过渡,须保留。
- CSS 落 `src/assets/styles/animations.css` 新段:enter = opacity 0→1 `var(--duration-normal) var(--ease-out)`;leave = opacity 1→0 `var(--duration-fast) var(--ease-out)`。查看器特化:`.route-fade-enter-*.content-viewer` 叠加 scale(0.985)→1(利用 ContentViewer 根类,ContentViewer.vue:6,零状态方向感)。单个全屏根元素一次性 transform,过渡结束无残留,不违 HGalleryLabView.vue:459 逐格禁 transform 纪律。
- 隔离:ContentViewer 零改动;查看器内 prev/next 走 router.replace(ContentViewer.vue:1071)同组件不触发过渡。依据 router/index.ts:84-87、src/utils/mediaRoute.ts:53-56。

### P2-2 大图上一张/下一张切换(争用区批 B,独立 commit)
- 方案:opacity dip(切换瞬间降不透明度,新图 load 后淡回)。弃双 img 交叉淡化(复制节点属结构性改动)与方向 slide(transform 通道被缩放/平移状态占用,ContentViewer.vue:54)。
- 改动(全在 ContentViewer.vue,4 处极小):
  - :48-58 img:class 加 `'is-switching': imgSwitching`;`@load="updateZoomRatio"` 改 `@load="onImgLoad"`(内部调 updateZoomRatio + 清 flag)。
  - :1067-1072 navigate():入口 `imgSwitching=true` + 250ms 安全定时器兜底清 flag(load 不触发/切视频项防卡淡出态)。
  - script 新增:1 ref<boolean> + 1 timer 句柄 + onImgLoad(≈8 行);onMediaError(:57)路径也清 flag;卸载清定时器。
  - :1400-1411 CSS:`.detail-viewer__img` transition 加 `opacity var(--duration-normal) var(--ease-out)`(同行 transform 0.2s 时长 token 化为 var(--duration-normal),缓动保留原 cubic-bezier(0.25,0.46,0.45,0.94) 缩放手感);新增 `.detail-viewer__img.is-switching { opacity: 0.25; }`。
- 视频/音频切换不做动画(视频播放器线领地);EditOverlay 激活时 navigate 已禁(:1068),flag 不误挂。方向 slide defer。

### P2-3 UiDialog 关闭动画(全库 7+ 模态一点收益)
- 现状:enter 有(src/assets/styles/index.css:463-507 dialog-fade-in/dialog-slide-up 挂载即放),leave 无——open=false 时 v-if 瞬移(UiDialog.vue:53)。
- 改法:UiDialog.vue:51-101 Teleport 内包 `<Transition name="ui-dialog">`(Teleport > Transition > overlay div 合法);enter 保持现有 mount keyframe(避免双动画),仅补 leave:index.css 同区加 `.ui-dialog-leave-active { pointer-events: none; animation: dialog-fade-in var(--duration-fast) ease-in reverse; }` + `.ui-dialog-leave-active .dialog-content { animation: dialog-slide-up var(--duration-fast) ease-in reverse; }`。
- 隔离:仅 UiDialog.vue + index.css,消费方零改动。

### Defer
缩略图→查看器 hero/FLIP(跨组件 rect 协调,双双触碰争用区)、prev/next 方向 slide(transform 占用)、沉浸模式进出淡化(已有 0.18s 联动,收益低)。

## P3 现存动画调优清单(锚点+问题+改法)

1. src/App.vue:375 — transform 0.18s ease → transform var(--duration-normal) var(--ease-out)
2. src/components/layout/AppShell.vue:382 — 0.18s ease → 同上
3. src/components/layout/AppShell.vue:397 — 0.18s ease → 同上
4. src/views/DocumentViewer.vue:1736 — transform 0.28s ease → var(--duration-moderate) var(--ease-out)
5. src/components/layout/GalleryViewControls.vue:350 — 0.28s ease → 同 4
6. src/components/layout/GalleryFilterChips.vue:362 — 0.28s ease → 同 4
7. src/components/media/SelectionActions.vue:307 — 0.28s ease → 同 4
8. src/components/doc/ReaderSettingsGroup.vue:104 — grid-template-rows 0.26s cubic-bezier(0.4,0,0.2,1) → var(--duration-moderate) var(--ease-in-out)
9. src/components/settings/CollapsibleCard.vue:112 — 同 8
10. src/components/settings/ModelLibrary.vue:424 — 同 8
11. src/components/doc/FolderTreeSelectorDialog.vue:239 — transform 0.2s(隐式 ease)→ transform var(--transition-normal)
12. src/components/doc/ReaderSettingsGroup.vue:91 — transform 0.2s → 同 11
13. src/components/common/ToastContainer.vue:136 — opacity 0.2s → opacity var(--transition-normal)
14. src/views/SettingsView.vue:830 — opacity 0.2s → 同 13
15. src/components/media/SemanticSearchPanel.vue:289 — width 0.4s ease → width var(--duration-slow) var(--ease-out)
16. 进度条 width 双转速:SettingsView.vue:1088(100ms)、ManagementSection.vue:259(100ms)、ToolsSection.vue:830(100ms)、PluginStoreView.vue:572(200ms)、ModelLibrary.vue:510(200ms)→ 统一 width var(--duration-fast) linear
17. spinner 四转速(0.6/0.7/0.8/1s)→ animation-duration 统一 var(--duration-spin):animations.css:127 + :193 豁免块(var(--duration-spin) !important)、PluginStoreView.vue:885、PersonsView.vue:450、PluginGate.vue:186、UiButton.vue:49、ToolsSection.vue:834、SemanticSearchPanel.vue:349,401、AppToolbar.vue:919、FoldersSection.vue:1761
18. src/views/AudioPlayer.vue:294 — scrollIntoView({behavior:'smooth'}) 不受 CSS reduce 块压制 — 加 matchMedia('(prefers-reduced-motion: reduce)') 判断,reduce 时传 'auto'
19. src/components/media/MediaGrid.vue:2308 — z-index 220ms linear 疑刻意(hover 层级延迟复位)— 只核实+补一行中文注释,不改行为(主线已裁)
20. animations.css:93-98(.toast-enter/.toast-leave)与 :172-174(.overlay-enter)疑死码(ToastContainer 用 scoped toast-enter-from/-active,ToastContainer.vue:162,170)— rg 'toast-enter|toast-leave|overlay-enter' src 确认后删

Defer:ContentViewer.vue:1562 color 0.3s(争用区非必经行);BookReader.vue:696-729 curl(阅读器线,自带守卫);SettingsView.vue:1098-1172 shimmer 与全局重复(结构改动);MediaGridCanvas.vue:1793 弹性曲线(perf 调参);useGridFlipReflow.ts:16-17 TS 硬编码(已有守卫)。

## P4 施工分批表(主线修正版)

| 批 | 文件集 | 内容 | 依赖 | 并发 |
|---|---|---|---|---|
| A | variables.css、animations.css、App.vue | P1 两 token + P2-1 + P3#1 + P3#17(animations.css 两处)+ P3#20 | 无 | 与 B/C 并发 |
| B | ContentViewer.vue(仅此) | P2-2 + :1407 顺手 token 化 | 无(所用 token 已存在) | 独立 commit |
| C | UiDialog.vue、index.css | P2-3 | 无 | 与 A/B 并发 |
| D1 | SettingsView、PersonsView、PluginStoreView、DocumentViewer、AudioPlayer、ModelLibrary、CollapsibleCard、ReaderSettingsGroup、FolderTreeSelectorDialog | P3#4,8,9,10,11,12,14,16 部分,17 部分,18 | A(新 token) | 与 D2 并发 |
| D2 | AppShell、AppToolbar、GalleryViewControls、GalleryFilterChips、SelectionActions、SemanticSearchPanel、MediaGrid、ToastContainer、ManagementSection、ToolsSection、FoldersSection、PluginGate、UiButton | P3#2,3,5,6,7,13,15,16 部分,17 部分,19 | A | 与 D1 并发 |

五批文件域两两不相交(UiButton∈D2 与 UiDialog∈C 不同文件;ContentViewer 只在 B;App.vue/animations.css 只在 A)。

## 边界情况

1. Transition+KeepAlive 时序:MediaGrid deactivated 推迟到 leave 结束;activated 滚动位恢复(App.vue:64-67 保活契约)须手测;坏则退路 = RouterView v-slot 动态 :name,仅 /view|/doc|/audio 进出启用过渡(行为变更需主线确认)。
2. Transition 要求子组件单根。已核单根:MediaGrid(:2)、ContentViewer(:6)、CollectionsView(:3);A 批须 grep 其余路由组件(SettingsView/PersonsView/PluginStoreView/DocumentViewer/AudioPlayer/HGalleryLabView)确认,多根者上报不改。
3. mode="out-in" 空窗:leave 120ms 视图短暂空白,背景由 .app-content 承接,dev 确认无白闪。
4. P2-2 flag 卡死路径:navigate 到视频/音频项 img 卸载 load 不触发 → 250ms 定时器无条件清 flag;组件卸载清定时器。
5. P2-3 leave 期间 overlay 残留 ~120ms 必须 pointer-events:none;若 vitest 断言「close 后 DOM 立即消失」挂,允许改断言为 await 过渡完成。
6. reduced-motion:新增 CSS 自动被 animations.css:181-189 全局 0.01ms 压制(Transition 事件照发不卡 leave);新增代码不得用 animation:none 退化(卡 Vue transitionend,见 animations.css:177-180 注释)。
7. 性能纪律:route-fade scale 只作用单个全屏根元素且结束即无 transform;禁格/行级 will-change 或常驻 transform;不引 TransitionGroup。
8. 排除区:TimelineScrubberCanvas.vue(dirty 硬禁)、src-tauri/src/video/、src/utils/assetUrl.ts、src/composables/useMediaDetail.ts。

## P5 验证方案

施工中(implementer 每批):npm run typecheck;npx eslint <涉改文件>;npx prettier --check <涉改 .css/.vue>;B/C 批 npx vitest related --run <涉改文件>。A 批附加审计:`rg '\d+m?s (ease|linear|cubic-bezier)' src --glob '*.vue' --glob '*.css'` 应只剩 defer 点位。
批末门禁(phase-closer):npm run lint + npm run typecheck + npm test 全量。
GUI 手测清单(not automated,收口真机):
1. 画廊点图开查看器淡入+微缩放;Esc/返回淡出;往返后画廊滚动位保持、无重挂白闪(KeepAlive 契约)。
2. 查看器滚轮/方向键连翻:dip 淡切,快速连翻不卡淡出态;翻到视频项无残留;损坏图 error 路径正常。
3. 对话框关闭淡出+下滑,期间点击穿透无效。
4. 系统「减少动态效果」:全部瞬时退化,spinner 仍旋转。
5. 万级画廊滚动帧率无肉眼差异;进出查看器无合成层爆炸(DevTools Layers 抽查)。
6. /view/:id 深链直开关闭回根路由正常。

## P6 风险与裁决点(主线裁决已注)

- spinner 800ms 观感 — 已裁先落,用户嫌快慢改 token 一处。
- out-in ~120ms 空窗 — 真机验收拍板,嫌顿降 leave 80ms 或仅查看器路由启用。
- B 批时序 — 已裁独立 commit 先落,抢在视频播放器线动 ContentViewer 前。
- KeepAlive 滚动恢复若被破坏 — 退路动态 :name 备好,行为变更需再裁。
- P3#19 — 已裁默认只加注释。

## 已弃方案
新引动画库(违零依赖约束)、hero/FLIP 过渡(争用区)、双 img 交叉淡化/方向 slide、component :key="route.path"(破坏 KeepAlive 语义)、UiDialog enter 迁 Vue 类(双动画冲突)、stylelint 门(新依赖)、FLIP_MS 读 CSS var(不值当)。