---
id: 2026-07-13-SelectionBar合并分离一键切换方案
status: active
type: design
line: UI/UX 第二阶段整合重构
created: 2026-07-13
---

# SelectionBar 合并/分离一键切换方案(含收窄折叠与拖拽位置持久化)

> **定位**:UIUX 重构线(S0–S7)阶段11「selection bar 改造」的施工蓝本。承接裁决链:
> ① 2026-07-13 用户裁决**保留拖拽**——可拖正是解决「胶囊遮挡内容」的核心能力,覆盖主设计 §7.1 原「默认不可拖、拖拽降级高级选项」(已按规范文本红线写回,c21d315);
> ② 同日(会话续18)用户批准本方案全部推荐项(三个已批决策点存证见 §6),指示落盘供新会话施工。
> **接续导读**:施工会话直接读本文 + `docs/planning/2026-07-11-uiux-refactor/` 三件套阶段11 条;主设计文档(`2026-07-11-前端UIUX深度审计与重构方案.md`)§7.1 的 Selection bullet 已改指本文,本文是单一真源。

> **🟢 施工状态(2026-07-13,会话续19)**:C1–C5 全部落地(提交 9b893e5/0156db8/3974dfc/7cac029 + 本 docs),门禁本地全绿(typecheck 0/lint 0/vitest 801/build)。施工记录见 **§7**;真机验收清单见 `docs/planning/2026-07-11-uiux-refactor/realtest-round6-2026-07-13-selectionbar.md`(折叠视觉/docked 28px/Teleport+KeepAlive/拖拽 clamp/读屏均门禁盲区,待真机)。施工相对本蓝本有一处**加固**:AppStatusBar info 让位由「两单例」加第三条件 `hostActive`(见 §7)。

## 0. 结论摘要(TL;DR)

- **核心架构裁决:docked 形态用 Teleport 实现,推翻会话续16 评估的「必须先抽 useBatchActions」使能前提**——Teleport 只搬 DOM 不搬组件层级,9 个批量动作 handler 原地留在 MediaGrid,改造面缩小约一半、回归面显著收窄。
- 状态源照 useTitlebarMode 范式建 `useSelectionBarMode`(module 级 localStorage ref 单例);**分离(默认)**=浮动可拖胶囊(拖拽保留+位置持久化),**合并**=选区动作条 Teleport 进 AppStatusBar(docked 不可拖,替换式共存,28px 恒高)。
- **收窄可折叠**复用既有 useToolbarOverflow(Priority+ 引擎,AppToolbar 同款),两形态各持一实例;溢出项进「⋯」菜单,菜单**直接消费已建成的 UiPopover**(placement='top-end' 上弹,焦点陷阱/backdrop/Esc/autoUpdate 免费获得)。
- 前置重构:SelectionToolbar 从 9-emit 改为**数据驱动 commands prop**——溢出菜单必须能以数据渲染同一组动作(图标+文字标签行),这是折叠的结构前提而非风格偏好。
- 4 个代码提交 + 1 个 docs 提交,中等工作量;折叠/拖拽/弹层/28px 命中区属门禁盲区,全部列真机清单。零新增依赖。

## 1. 代码测绘(2026-07-13 快照;施工前对关键锚点 re-verify,行号可能漂移)

| 事实 | 位置 | 对方案的影响 |
|---|---|---|
| 选区**状态**是 module 级单例,全局可读 | `src/composables/useSelection.ts`(state/isSelectionMode/selectedCount 定义于 useSelection() 函数体外) | AppStatusBar 可直接读 isSelectionMode/selectedCount,无需 props |
| 9 个批量**动作** handler 深度纠缠网格局部状态 | `src/components/media/MediaGrid.vue`(SelectionToolbar 消费点约 295-305 行);handler 依赖 patchVisibleSelected/pendingDeleteIds 暂存置灰/FLIP 重排/compute()+updateVisible() | 抽共享 composable 须向网格回调注入,高成本高风险 → Teleport 方案绕开(§3.2) |
| 胶囊定位上下文=`.media-grid-layout`(position:relative,充满 .app-content) | `SelectionToolbar.vue`(.selection-toolbar-wrapper 为 absolute bottom:32px)+`MediaGrid.vue` | bottom:32px 是「内容区下缘上方 32px」——状态栏在 .app-content **之外**,物理上不可能重叠;32px 是审美间距非 safe-area,**不改数值只补注释钉语义**(§3.6) |
| 拖拽 offset 退出选区即复位,不持久 | `SelectionToolbar.vue`(watch isSelectionMode → 复位 offsetX/Y) | 按裁决改持久化,须删该 watch(有意行为变更,写入提交信息) |
| 状态栏=AppShell footer 槽,28px 恒高 | `AppShell.vue`(.app-statusbar)+`variables.css`(--statusbar-height:28px);内容 `AppStatusBar.vue`=.statusbar__info(flex:1,overflow:hidden)+.statusbar__right(版本/布局计算指示) | docked 共存策略见 §3.7 |
| useTitlebarMode 全链范式 | `src/composables/useTitlebarMode.ts` + `settingsMap.ts`(titlebarMerged 条) + `DynamicSettingControl.vue`(toggleBindings) + `App.vue` 消费 | §3.1/§3.8 逐文件镜像 |
| 折叠基建=useToolbarOverflow(纯核 computeOverflowSplit 已单测)+data-toolbar-item 测量+ResizeObserver | `src/composables/useToolbarOverflow.ts`;消费范本 `AppToolbar.vue`(foldableRef/remeasureKey/overflowButtonWidth) | 引擎原样复用;注意 observer 只在 onMounted 绑一次容器 → §3.4「双实例」理由 |
| **UiPopover 已建成**(@floating-ui 引擎;Teleport+useFocusTrap+backdrop/Esc dismiss+autoUpdate;props: open/anchor/placement/offsetPx/labelledBy/role/trapFocus) | `src/components/ui/UiPopover.vue`(4b02272/7c7289f,会话续17) | 溢出菜单直接消费(§3.5),不再手写弹层 |
| MediaGrid 被 KeepAlive 保活 | `App.vue`(KeepAlive include=['MediaGrid']) | Teleport 内容在 deactivated 时的摘除属框架边界行为,须显式守卫(§3.2) |

**🔴 推翻一处前评估前提**:会话续16 评估认为「批量动作须从 MediaGrid 抽共享 composable(useBatchActions)为使能前提」。实际读码后结论相反——该前提隐含假设「docked 条由 AppShell/AppStatusBar 渲染」。改用 **Teleport 由 SelectionToolbar(MediaGrid 子组件)自己渲染 docked 形态、只把 DOM 投送进状态栏**:组件层级不变、事件链与依赖注入不变、9 个 handler 一行不动。仓内先例=UiDialog 恒 Teleport 到 body 而消费方逻辑不动,同一原理在停靠上的应用。

**🟡 落盘前修订(re-verify before restating 红线的兑现)**:本方案初稿(会话续18 对话中)曾拟为溢出菜单「照 AppToolbar 范式手写第三个弹层、不发明 UiPopover」;落盘核对地面真相时发现并行会话续17 已交付 UiPopover 且全库手写弹层定位归零(positionMenu 已删)——溢出菜单改为直接消费 UiPopover,顺带免费获得焦点陷阱与 autoUpdate(初稿的「溢出菜单 a11y 债归 UiPopover 线」注记随之消解)。

## 2. 目标形态

```
分离(floating,默认,现状+增强)                合并(docked)
┌─ .app-content ───────────────────┐   ┌─ .app-content ───────────────────┐
│                                  │   │                                  │
│   ┌╌ 可拖胶囊(位置持久化) ╌╌╌╌╌┐  │   │        (内容零遮挡)              │
│   ┆⠿ 已选3项 ▢◧♥♡📁●● ⊘🗑→⧉ ⋯ ⇓ ✕┆ │   │                                  │
│   └╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┘  │   │                                  │
├─ 状态栏 28px ────────────────────┤   ├─ 状态栏 28px(info 区被替换) ─────┤
│ 3,204 项 · 3,100 图 …      v0.1.0│   │ 已选3项 ▢◧♥♡📁●●⊘🗑→⧉ ⋯ ⇑ ✕ v0.1.0│
└──────────────────────────────────┘   └──────────────────────────────────┘
  ⇓=停靠到状态栏(一键)                    ⇑=浮起为胶囊(一键)  不可拖·无手柄
  收窄→动作自尾部折入 ⋯(UiPopover 上弹)    收窄→同一套折叠引擎,更早触发
```

两形态**同一组 commands、同一折叠引擎、同一 i18n**,只有壳不同(胶囊壳=拖拽手柄/圆角/毛玻璃;docked 壳=28px 紧凑行)。

## 3. 设计分解

### 3.1 状态源:useSelectionBarMode(新文件,镜像 useTitlebarMode)

```
src/composables/useSelectionBarMode.ts
  docked: ref<boolean>            localStorage 'selection_bar_docked';缺省/非法一律回落 false(分离=现状)
  setDocked(v)                    写 ref + 持久化(try/catch 静默降级,同 useTitlebarMode)
  offset: ref<{x,y}>              localStorage 'selection_bar_offset',JSON;解析失败回落 {x:0,y:0}
  setOffset({x,y})                拖拽结束时写入
  clampOffset(offset, capsuleRect, boundsRect): {x,y}   ← 导出纯函数,spec 穷举
```

三个消费者共享同一单例:SelectionToolbar(形态分支+拖拽)、AppStatusBar(info 让位判断)、DynamicSettingControl(设置页开关)。

**默认值=分离(已批,§6-1)**:titlebar 线默认合并,是因为「合并才是被 S3 推翻的用户原设计」;selection bar 恰相反——浮动可拖是现状且被裁决为核心能力,默认分离=零惊讶,docked 是新增选项。两线默认值不对称是有意的。

### 3.2 架构:Teleport 停靠(已批) vs 抽取 useBatchActions(弃)

| | A. Teleport(已批) | B. 抽 useBatchActions(弃) |
|---|---|---|
| 动作 handler | 原地不动(MediaGrid) | 迁出,patchVisibleSelected/pendingDeleteIds/FLIP/compute 须以回调注入回网格 |
| 改动面 | SelectionToolbar+AppStatusBar+MediaGrid 模板 | 上述 + MediaGrid 脚本大手术(2200+ 行高危区) |
| 回归面 | 渲染位置 | 批量删除的暂存/撤销/重排整条链 |
| 备注 | 若未来查看器也要批量动作,届时再抽不迟 | 当下无第二消费者,属为不存在的消费者付重构税 |

**两个已知边界及对策**:

1. **挂载顺序**:AppShell 模板中 `<main>`(默认槽,含 MediaGrid)先于 `<footer>`(statusbar 槽)渲染;若 Teleport 在挂载期即求值会找不到目标。实际安全——整个 bar 在 `v-if="isSelectionMode"` 内,挂载时选区必为空,用户首次进入选区时状态栏早已就位。**代码留注释钉死此依赖**(勿改成挂载期常驻渲染)。
2. **KeepAlive 残留**:MediaGrid 被 deactivate(进查看器)时,Teleport 出去的 DOM 是否随之摘除属 Vue 已知边界(历史上有 Teleport+KeepAlive 残留 issue)。不赌版本行为:SelectionToolbar 内用 onActivated/onDeactivated(KeepAlive 子树任意深度组件均生效,初始化 true 兼容非 KA 上下文)维护 `kaActive` ref,渲染守卫= `v-if="selection.isSelectionMode.value && kaActive"`。(现状里切视图 watcher 会 clearSelection,多数路径选区本就清空——此守卫是防御层。)

### 3.3 前置重构:9-emit → 数据驱动 commands(parity)

折叠后溢出项要在 ⋯ 菜单里以「图标+文字标签」行再渲染一遍 → 动作必须是数据而非写死的模板按钮。

```ts
interface SelectionCommand {
  key: string                 // 'favorite' | 'delete' | ...
  icon: Component             // lucide 组件
  labelKey: string            // i18n key(tooltip 与菜单行共用)
  danger?: boolean            // 删除 → selection-action--danger / 菜单红行
  kind?: 'button' | 'colors'  // colors = ColorLabelPicker 整体作为一个折叠单元
  groupStart?: boolean        // 组首 → 其前渲染 divider,并与之同一折叠单元(divider 不独立成测量项)
  run: (value?: number) => void   // colors 单元传色值(0=清除)
}
```

MediaGrid 组装数组(handler=既有函数引用,零迁移),顺序保持现状 parity:
selectAll → invert → favorite → unfavorite → addToCollection → colors → clearColor → delete → move(groupStart) → copy。
以 `:commands` 单 prop 传入 SelectionToolbar,删除 9 条 `@` 事件绑定。折叠自尾部(copy→move→delete→…);若真机后认为 delete 应更晚折,属一行 reorder 的产品微调,不搭本线的车。
「✕ 取消选择」「⇓/⇑ 停靠切换」「拖拽手柄」「计数」是壳的固定件,**不进 commands、不参与折叠**(逃生口恒可达)。

收益:tooltip 文案/aria-label/图标从模板三处内联收敛单源;菜单行免费获得可见文字标签(兑现主设计 §7.1「可见标签」a11y 改进)。

### 3.4 折叠(收窄窗口可折叠)

- **引擎**:useToolbarOverflow 原样复用;`data-toolbar-item` 打在每个折叠单元;`remeasureKey = locale`(commands 集静态;docked/floating 切换靠各自实例 mount 时 measure)。
- **结构**:抽子组件 `SelectionActions.vue`(props: `commands` / `variant: 'floating'|'docked'`),浮动壳与 docked 壳各放一个实例。**两实例而非一实例换容器**——useToolbarOverflow 的 ResizeObserver 只在 onMounted 绑一次容器,v-if 换壳会让 observer 钉在死元素上;每壳独立实例 mount 即 measure+observe,天然正确,且不需要动共享 composable。
- **浮动壳可用宽**:胶囊现为 width:max-content(永不「装不下」,引擎测不出溢出)→ 改 `.selection-toolbar { max-width: calc(100% - 24px) }`(wrapper 已 left:0;right:0 全宽),动作容器 `flex:1 1 auto; min-width:0` → clientWidth 成为真实预算。
- **docked 壳可用宽**:替换进状态栏 info 区(flex:1,min-width:0),天然正确。
- **测量帧防闪**:AppToolbar 用容器 overflow:hidden 兜底测量帧的全渲染;但选区条 tooltip(data-tooltip 绝对定位于按钮上方、条外)会被 overflow:hidden 裁掉 → 改**测量帧容器 visibility:hidden**(offsetWidth 在 visibility:hidden 下仍可测,display:none 才归零),既无闪帧也不裁 tooltip。这是对既有范式的有意偏离,代码注释说明理由。
- **折叠渲染**:`v-show="measuring || i < visibleCount"` 简式(选区条是瞬态 UI,不做 GalleryFilterChips 的 G7 1fr→0fr 折叠动画);divider 与组首命令同一折叠单元。

### 3.5 溢出菜单 = UiPopover(已建成原语,直接消费)

- ⋯ 触发钮为壳固定件(引擎经 overflowButtonWidth 预留其宽,仅 hasOverflow 时显示);anchor 取其根 DOM(原生 button 直接 ref;若用 UiIconButton 则 `.value?.el`,defineExpose({el}) 已具备)。
- **disclosure 语义(对齐并行会话 f7684b0 确立的 de-facto 范式)**:⋯ 触发钮加 `aria-haspopup="true"` + `:aria-expanded="menuOpen"`——与 AppToolbar filter/view ⋯ 钮同款(会话续17 续,f7684b0 落地)。若用 UiIconButton,二者作为普通属性经**单根 fallthrough** 落根 button(UiIconButton.vue:8/54 已证,无需给原语加 prop);若用原生 button 则直接绑。这是本次 Review 因并行 a11y 工作**新增的方案要点**(初稿遗漏)。
- `<UiPopover v-model:open="menuOpen" :anchor="moreBtnEl" placement="top-end">`——条在底部,top-end 向上弹;近视口边缘的翻转/滑移由 flip/shift 中间件自动处理,无需手算「向上定位」。焦点陷阱(trapFocus 默认 true)、backdrop 点击/Esc dismiss、autoUpdate 滚动跟随全部免费。
- 菜单行=图标+文字标签(labelKey 同源);colors 单元渲染整行 ColorLabelPicker;danger 行红色。菜单**表面视觉**按 UiPopover 契约由消费方 slot 自带(参照 AppToolbar filter ⋯ 菜单迁移后的样式)。
- 容器变宽项回流时,若 menuOpen 且溢出集清空 → watch 置 menuOpen=false(空菜单自关)。
- **commands 动作钮不加 aria-pressed**:与 f7684b0 一致——选区动作全是瞬时命令(收藏/删除/移动…非 toggle),瞬时钮不带 pressed 语义;SelectionCommand 无 toggle 字段即此意。dock/float 切换钮亦为动作钮(label 随态切换「停靠到状态栏」↔「浮起为胶囊」已传达状态),不加 pressed。

### 3.6 拖拽位置持久化(分离形态)

保留现有 pointer capture 拖拽实现(已含 pointercancel 处理,质量良好),仅三处变更:

1. onDragEnd 末尾 `setOffset({x,y})` 持久化;
2. **删除**「退出选区即复位 offset」的 watch——持久化语义与 reset-on-exit 互斥(有意行为变更,写入提交信息);
3. **clamp**:恢复时/拖拽结束时/窗口 resize 时(bar 可见期间挂 resize 监听)用 clampOffset 纯函数把 offset 钳到「胶囊矩形与 wrapper 矩形交集非空且拖拽手柄可达」范围,防「大窗口拖到角落 → 小窗口打开后条在屏外失踪」。offset 相对居中基线存储(现状模型),对窗口尺寸变化天然稳健。

`bottom:32px` 魔数:测绘证明它是「内容区内的审美间距」而非 safe-area 缺失(状态栏在定位上下文之外,物理不可能重叠)——**不改数值,补注释钉住语义**,消除「魔数不知状态栏」的误解。

### 3.7 状态栏共存:替换式(已批,§6-2)

`docked && isSelectionMode` 时:AppStatusBar 的 .statusbar__info 整块 v-if 让位(含扫描/缩略图/AI 进度),outlet 接管左侧;.statusbar__right(版本/布局计算指示)保留。高度恒 28px,**零布局位移**(网格不 reflow);「已选择 N 项」语义上顶替「N 项」总计数。代价=选区期间扫描进度不可见——两者皆瞬态,可接受;真机若不适,回退成本低(改 v-if 条件保留 spinner 单元即可)。

弃选项存证:B 增高 36px(每次进出选区整个内容区 reflow+虚拟滚动重算,且破坏「状态栏恒定」心智);C 挤占共存(28px 行宽度装不下两组内容,排除)。

**docked 紧凑规格**:按钮 24×24(WCAG 2.2 AA 目标尺寸下限)、图标 14px、计数 --font-size-xs;tooltip 沿用 data-tooltip 上方弹出(容器不设 overflow:hidden,见 §3.4)。

**实现**:AppStatusBar 模板加恒存在的 `<div id="statusbar-selection-outlet" class="statusbar__selection-outlet">`(空时零宽);让位条件读 useSelection + useSelectionBarMode 两单例。沉浸模式 footer 整体 v-show 隐藏,docked 条随之隐藏,行为自洽(沉浸=查看器语境,kaActive 守卫本就不渲染)。

### 3.8 一键切换入口(双入口,同写一个单例)

1. **条上按钮(主入口)**:浮动壳尾部「⇓ 停靠到状态栏」,docked 壳尾部「⇑ 浮起为胶囊」(lucide 图标施工时按 @lucide/vue 实际可用集选,如 PanelBottom / PictureInPicture2)。选区是瞬态场景,去设置页切换动线太长,条上直切是「一键」的字面兑现。切换时选区不丢(状态在 useSelection 单例,壳换血不动状态)。
2. **设置页(发现性入口)**:settingsMap 加 `selectionBarDocked`(section:'general',紧邻 titlebarMerged)+ DynamicSettingControl toggleBindings 一条。与 titlebar 线完全同构,注册表驱动一处接入。

### 3.9 i18n 增量(zh/en 对称,localeIntegrity 门禁验平衡)

`selection.more`(更多动作)/ `selection.dockBar`(停靠到状态栏)/ `selection.floatBar`(浮起为胶囊)/ `settings.selectionBarDocked` / `settings.selectionBarDockedDesc`。

## 4. 施工计划(4 代码提交 + 1 docs,各带门禁)

| # | 内容 | 本地门禁 | 门禁盲区 → 真机 |
|---|---|---|---|
| C1 | commands 数据驱动化(parity):SelectionCommand 类型 + MediaGrid 组装 + SelectionToolbar 改渲染 commands、删 9 emit。外观行为不变 | typecheck/lint/build + **新增 SelectionToolbar SSR 契约 spec**(按钮数/顺序/aria-label 源自 labelKey/danger 类/✕ 恒在;@vue/server-renderer 范式) | 视觉 parity 抽查 |
| C2 | 折叠(浮动形态):抽 SelectionActions.vue + useToolbarOverflow + max-width 链 + 测量帧 visibility 方案 + ⋯ 菜单(UiPopover placement='top-end',触发钮 aria-haspopup/`:aria-expanded`) | 同上 + SSR 验 data-toolbar-item 结构、⋯ 钮 disclosure 属性、菜单 teleports(context.teleports 范式);computeOverflowSplit 纯核已有单测不重复 | 收窄逐项折入/无振荡/上弹定位与近边缘 flip/菜单内色签行/焦点陷阱/⋯ 钮读屏 disclosure 播报 |
| C3 | docked 形态+切换:useSelectionBarMode(含 spec:默认值/持久化/非法值回落/clampOffset 穷举)+ Teleport 停靠 + AppStatusBar 让位 + kaActive 守卫 + 条上切换钮 + settingsMap/binding/i18n | 同上 + composable spec + 对比度门禁(24px 钮落 bg-secondary,六主题) | 28px 命中区手感/tooltip 不被裁/替换让位观感/切换保选区/进出查看器无残留 |
| C4 | 拖拽位置持久化:setOffset + 删 reset watch + clamp 三时机 | 同上(clampOffset 已在 C3 spec 覆盖) | 跨重启位置恢复/小窗口 clamp 不失踪/拖拽仍顺滑 |
| C5 | docs 回写:本文标记进度 / task_plan 阶段11 勾选 / progress·findings 会话记录 / 新建 realtest-roundN 清单(含 §5 全部真机项) | check_docs / check_docs_index | — |

纪律:每提交 `git commit -- <显式路径>`(并行会话防收割);「全绿」为本地口径,CI 以 push 后为准;三大既存非-standalone-prettier-clean 的 .vue(App/MediaGrid/SettingsView)照 findings 会话续14 判据处理,**绝不 `prettier --write`**。

## 5. 风险与真机验收草案

- **Teleport+KeepAlive** 是本方案唯一的框架边界赌注,kaActive 守卫已兜底为确定性行为;真机专列「选中 → 进查看器 → 返回」用例。
- **折叠全链是 DOM 测量**,SSR 门禁只能验结构(data-toolbar-item/顺序);visibleCount 动态、弹层定位、clamp 的实际视觉全在盲区——不声称门禁绿=交互已验证。
- **替换式让位**使选区期间扫描进度不可见,属可逆产品取舍,真机确认。
- 真机清单(C5 落 realtest-roundN):① 浮动:拖拽顺滑/位置跨重启恢复/小窗口 clamp 不失踪;② 折叠:收窄逐项折入 ⋯/无振荡/UiPopover 上弹与近边缘 flip/菜单内色签行/焦点陷阱;③ docked:28px 命中区/tooltip 不裁/让位切换观感/版本区保留;④ 切换:条上钮与设置开关同源联动/选区不丢/localStorage 持久;⑤ KeepAlive:选中态进出查看器无 docked 残留;⑥ 六主题对比度目验(docked 钮落 bg-secondary);⑦ **a11y 读屏(NVDA/VoiceOver,对齐 f7684b0 §E 范式)**:⋯ 触发钮播报「有弹出·已折叠/已展开」(aria-haspopup+aria-expanded 随 menuOpen 更新)、菜单行播报可见文字标签、瞬时动作钮不误报 pressed。

## 6. 已批决策点存证(2026-07-13,用户批复「批准建议」)

1. **默认形态=分离(浮动)**——保现状零惊讶;与 titlebar 默认合并的不对称是有意的(各自被推翻/被保留的「原设计」不同)。
2. **状态栏共存=替换式**——28px 恒高、info 区让位、版本区保留。
3. **SelectionToolbar 契约变更:9-emit → commands prop**——折叠(溢出菜单以数据渲染同组动作)的结构前提。

## 7. 施工记录(2026-07-13,会话续19)

C1–C5 按蓝本分提交落地,每提交独立过本地门禁(typecheck/lint/vitest/build)。

| 提交 | 阶段 | 落地要点 | 门禁 |
|---|---|---|---|
| 9b893e5 | C1 动作数据驱动化 | `SelectionCommand` 类型 + MediaGrid 组装(handler 零迁移) + SelectionToolbar 迭代 commands、删 9 emit;新增 SelectionToolbar SSR 契约 spec | vitest 782 |
| 0156db8 | C2 收窄折叠 | 抽 `SelectionActions.vue`(自持 useToolbarOverflow,双实例各绑自己容器);宽度约束链(胶囊 max-width + flow flex:0 1 auto);测量帧 visibility:hidden;⋯ 菜单消费 UiPopover(top-end)+ disclosure 语义;spec 拆分 SelectionActions.spec(8)/SelectionToolbar.spec(4) | vitest 787 |
| 3974dfc | C3 docked 形态 + 切换 | `useSelectionBarMode`(默认分离);docked 用 Teleport(9 handler 原地留 MediaGrid);AppStatusBar outlet + 替换式让位;条上切换钮;settingsMap/binding/i18n | vitest 801 |
| 7cac029 | C4 拖拽持久化 | offset 走 useSelectionBarMode(localStorage);删 reset-on-exit watch;clampOffset 三时机(恢复/拖终/resize) | vitest 801 |
| (本 docs) | C5 回写 | 本文 §7 + 顶部状态banner / task_plan 阶段11 / progress·findings / realtest-round6 | check_docs |

**相对蓝本的一处加固(诚实披露)**:蓝本 §3.7 写「AppStatusBar 让位条件读 useSelection + useSelectionBarMode **两单例**」。施工时发现一个具体空白态:若选区**残留进查看器**(MediaGrid 被 KeepAlive deactivate、图片查看器的沉浸是**用户手动 toggle** 而非进入即隐藏 footer),则 docked 的 Teleport 已随 `hostActive=false` 摘除,但 info 仍按「两单例」让位 → outlet 空白。修法=在 `useSelectionBarMode` 加**共享信号 `hostActive`**(SelectionToolbar 于 onActivated/onDeactivated 写入),docked Teleport gate 与 AppStatusBar info 让位**同读此第三条件**,二者同步。这是对蓝本的加固(不改已批三决策),已在 §3.2/§3.7 语义内。

**门禁盲区(不声称门禁绿=交互已验证)**:折叠 visibleCount 动态切分、⋯ 菜单开阖/上弹/近边缘 flip、docked 28px 命中区与 tooltip 不裁、Teleport+KeepAlive 进出查看器无残留、拖拽 clamp 跨重启恢复、读屏 disclosure/可见标签——全在 SSR/单测盲区,列 `realtest-round6-2026-07-13-selectionbar.md` 待真机。
