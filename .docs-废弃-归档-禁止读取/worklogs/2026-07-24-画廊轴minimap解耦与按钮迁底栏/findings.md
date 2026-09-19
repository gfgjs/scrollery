---
status: 快照
type: working-memory
line: 画廊轴minimap解耦与按钮迁底栏
created: 2026-07-24
---

# 发现与决策:画廊轴minimap解耦与按钮迁底栏

## 需求
- minimap 与无缝模式解耦:非无缝(日期分组)也可选开 minimap;现状=日期分组只有日期轴、无缝才有 minimap
- 轴切换钮、轴中「图谱」等按钮全部移入底栏;底栏版本信息左移
- 右下去顶部/去底部按钮被轴遮住——修遮挡+常态低透明度、鼠标移入才醒目
- 顺手发现:简单的顺手修,拿不准列清单待裁决

## 发现(✓consumed——已在阶段 2-3 落地为方案/施工依据,详见 task_plan 关键决策表)
- ✓门控链 MediaGrid.vue:613-631:canShowScrubber(:613)=totalRows>0&&(monthBuckets||separators);activeAxis(:623)=canShowScrubber&&preferredAxis==='timeline'?'timeline':'minimap';timelineVisible(:626)含会话 ref showTimeline(:599);minimapVisible(:629)=activeAxis==='minimap'&&canShowMinimap(:622)&&ui.showSeamlessMinimap
- ✓无缝切断点:无缝分组后端不产分隔符,canShowScrubber=false,activeAxis 自动落 minimap;模板 v-if 链 :224-259,轴切换逻辑 :636-660
- ✓MinimapAxis.vue props(:55-70)全通用滚动度量(totalHeight/currentY/viewportHeight/containerWidth/active/cacheDir/renderMode),:112 自拉 fetchRowsByY——日期分组直接挂载零数据缺口
- ✓持久化 uiStore.ts:337-367:seamlessGroups(key seamless_groups)、showSeamlessMinimap(key seamless_minimap)、minimapRenderMode(key minimap_render_mode);hydrateFromStartupConfig :637-643;showTimeline/preferredAxis 均会话级(→施工后:showSeamlessMinimap 更名 axisVisible + 新增 axisMode,持久化 key 复用 seamless_minimap 语义扩为轴开合通用)
- ✓底栏 AppStatusBar.vue:版本渲染:140、左 info:210-216、selection-outlet:202-209(注入先例)、右容器:223-227;挂载 App.vue:78-80 + AppShell.vue:84-95(→轴按钮迁入此底栏)
- ✓去顶底 fab MediaGrid.vue:352-370(模板)/:1174-1182(函数)/:2686-2694(.scroll-fab absolute bottom:32 right:32 z-100);轴 wrapper z-101 且贴右缘=遮挡根因(→施工已修)
- git 基线:工作树脏文件 src/components/media/player/VideoSeekBar.vue(与本线无关,绕开不入提交)
- 密度带钮真身:TimelineScrubberCanvas.vue:221-241 内嵌 visualMode(cycleVisualMode 循环 VISUAL_MODES)+ coordMode(toggleCoordMode 切 time/item)两枚状态与切换函数,:958 `defineExpose({ cycleVisualMode, toggleCoordMode, visualMode, coordMode })` 暴露给父组件——即用户所指「图谱钮」;:945/:951 各自 watch 写 localStorage(coordMode 键 `timelineCoord`)持久化,与 uiStore/config schema 无关联的独立持久化通道;迁底栏方案采方案 B(父组件通过 template ref 调用 defineExpose 暴露的方法,不在底栏侧复制状态)

## 复核结论(2026-07-25 主线亲审 e1b1789,以当前 HEAD 复证)

**契约面成立**——axis_mode 有四道防线,不落入 2026-07-25 深审 F-04(「schema 只验基本类型」指的是
UInt 无 min/max,枚举键不在其列):

1. `schema.rs:334` `SettingKind::Enum(&["timeline","minimap"])` 声明值域;
2. `config/file.rs:139-152` 加载期按 options 白名单校验,越界报「取值不在允许范围内」而非静默灌入;
3. `config_commands.rs:65` `#[serde(rename_all = "camelCase")]` → 前端 `axisMode` 命名对得上;
4. `uiStore.ts:653` 水合二次守卫,陌生值保默认 `timeline`。

底栏 outlet 时序也稳:`AppShell.vue` 的 footer 沉浸态走 CSS `translateY` + `inert`,**不是 `v-if` 摘除**,
`#statusbar-axis-outlet` 的 DOM 恒在 → Teleport target 不会落空。方案 B 的响应性同样成立:`defineExpose`
的对象经 `proxyRefs` 暴露,父模板读 `timelineCanvasRef?.visualMode` 会 unwrap 并收集依赖,子组件内部改
visualMode 能触发父重渲染。

### 已修(本次)
- **R-1 schema 注释语义过期(P2)**:`seamless_minimap` 的 `comment_zh` 仍写「是否显示无缝 minimap 轴」,
  但键语义已扩为「轴整体开合(时间轴/minimap 通用)」。该注释会原样写进用户可见的 `config.toml`,
  等于向用户描述了一个不存在的行为。已改写为「轴开合 + 键名沿用历史 + 形态另见 axis_mode」。
- **R-2 axis 契约零测试(P2)**:`axisVisible`/`axisMode` 此前无任何测试(`uiStore` 整体亦无 spec,
  属既有缺口)。新建 `src/stores/uiStore.spec.ts` 钉三条易回归契约,8 测:枚举守卫(认可/陌生/缺位)、
  水合默认方向(缺位=开、仅显式 'false' 才隐)、**写盘键名**(`setAxisVisible` 必须仍写老键
  `seamless_minimap`——这是「语义扩展、零迁移」的前提,键名一旦被顺手改成 `axis_visible`,
  老用户的收起状态会静默丢失)。

### 待用户裁决
- **C-1 沉浸态下轴按钮不可达(设计权衡,非 bug)**:轴钮迁底栏后,沉浸模式(查看器沉浸 ∪ F11 全屏)
  下底栏被 `translateY` 移出视口且 `inert`,开合钮/形态钮/密度带钮随之全部不可点;迁移前这些钮长在
  画廊轴上,沉浸态仍可操作。需 F10 或贴底唤出底栏才能碰。**可选处置:**(a) 接受现状(迁底栏是本线
  明确需求,沉浸态本就以「隐藏一切 chrome」为目的);(b) 沉浸态给轴钮留一个画廊内的浮层回退。
  倾向 (a),等用户 GUI 实测手感后定。

## 裁决(2026-07-25):R-5 time 坐标滑窗回归——线性对齐方案(用户定)
- 背景:R-3/R-4 三轮 time 空间视窗几何(8167a3f/f974a5e/6fd137e)被用户要求整体回退(4a38ba2),
  三件套当时同步回退到复核收尾版,故此前记录不在本文件;其「三条死路系几何模型冲突」的分析
  经用户裁定**来自低级模型、不可信,不再作为设限依据**。
- 用户新要求(即新方案的规格):time 坐标(时间轴+谱+时)下滑窗必须显示;**轴内指针与画廊内
  指针(MediaScrollbar 标尺线/箭头)全程对齐,同步贴顶/贴底**。
- 可证事实(独立于旧分析):两个单调映射要**处处**重合只能是同一映射;画廊指针是
  `currentY/(totalHeight−trackH)` 线性比例式,故轴内视窗+指示线唯有同走线性式才能满足规格。
  时间行映射(`logicalYToTimeFrac`)与线性式只在端点重合,中段必然分离——二者取一,用户取线性。
- 落地:TimelineScrubberCanvas.vue 两处——视窗 `v-if` 去掉 `effectiveCoord !== 'time'` 排除
  (所有坐标共用 `thumbGeometry` 滚动比例几何,拖动仍走 `thumbTopToLogicalY` 精确互逆,天然跟手
  +两端可达);drawAxis 指示线删 `logicalYToTimeFrac` 分支,恒 `scrollFrac×cssH`,与
  MediaScrollbar `lineY` 同源同式。helpers 零改动(线性式已被 mediaScrollbar.helpers.spec.ts
  round-trip+两端贴合锁死),无新增测试面。
- 语义代价(刻意,勿当 bug 报):指针/视窗不再落在密度带的日历行位——点轨跳到某日期后指针停在
  线性滚动位而非点击处;日历定位由点轨跳转/放大镜/浮层承担(此三者仍走时间行映射,未动)。

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
