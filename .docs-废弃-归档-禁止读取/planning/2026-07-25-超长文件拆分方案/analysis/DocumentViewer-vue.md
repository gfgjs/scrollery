---
id: 2026-07-25-DocumentViewer-vue
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
---

# DocumentViewer.vue 拆分方案

> 目标文件:`src/views/DocumentViewer.vue`(现 1878 行 / 77KB)。本方案为**纯结构移动**,不改变任何运行时行为、不新增/删除功能;并行会话正在精简本文件注释,行号会漂移——以下锚点以**符号名**为主、行号区间为辅(标注时的行号)。
> 接线对象 `src/vendor/foliate-js/**` 为 vendored 第三方(GPL 红线),本方案不触碰其内部,只讨论 `BookReader.vue` 对它的 props/emits 封装契约。

## 1. 现状结构图

文件三段:`<template>` 1–609 行、`<script setup>` 611–1571 行、`<style scoped>` 1573–1878 行。

### 1.1 Template(609 行)

| 区块 | 行号(约) | 职责 |
|------|---------|------|
| 工具栏外壳 + 侧栏开关 + 返回/标题合成控件 + 页码/foliate 进度页脚 | 4–49 | 静态展示 + 两个跳转/开关动作 |
| 折叠簇 `.doc-viewer__foldable`(`useToolbarOverflow` 消费方) | 50–328 | 13 个 `fold-item`(pagerMode/readerFlow/styleMode 三个 select + search/toc/bookmarks/readerSettings/autoScroll/immersive/edit/versions/proofread/replace/external 十个按钮)+ `⋯` 溢出按钮 |
| `⋯` 溢出菜单(`UiPopover`) | 329–440 | 与折叠簇**逐项镜像重复**的菜单态(select 保留原控件,按钮态转竖排图标+文字行) |
| 沉浸退出浮动按钮 | 443–452 | 单按钮,`v-if="immersive"` |
| 渲染区分发(编辑态 textarea / PdfReader / BookReader / 不支持提示) | 454–523 | `readerRef` 承接,kind 分发 |
| 七个侧栏面板(Replacement/Version/Proofread/ReaderSettings/Toc/Search/Bookmark) | 525–606 | **已是独立 SFC**,本层只做 `v-if` + props + `@事件` 转发 |

七个面板组件均已在 `src/components/doc/*.vue` 独立存在,不在本次拆分范围内,只是消费方。

### 1.2 Script setup 逻辑域(960 行,原文件已用 `// ──` 注释横幅分段,以下按该分段整理)

| # | 逻辑域 | 符号锚点 | 约行数 | 职责 |
|---|--------|---------|--------|------|
| S1 | 导入 + 懒加载子组件 + `ReaderApi` 接口 | 611–691 | 80 | 9 个 `defineAsyncComponent`;`ReaderApi` 是 `readerRef` 的统一方法面(`next/prev/getScrollEl/goToHref/searchBook/clearSearch/getCurrentLocation/goToLocator`) |
| S2 | 路由/详情核心态 | `id`/`detail`/`error`/`initialPos`/`pageInfo`/`readerRef`/`pagerMode` | 12 | 路由驱动的基础状态 |
| S3 | 阅读流/排版模式 | `readerFlow`/`flowForced`/`styleMode` | 6 | txt/epub 翻页↔滚动 + epub 自带/智能样式 |
| S4 | 排版全参数 | `readerFontSize`…`readerParagraphSpacing`/`readerPageTurn`/`readerVertical` + `readerTypography` computed | 12+11 | 传给 `BookReader :typography` 的 10 项字段之源 |
| S5 | 阅读主题 + app 暗色判定 | `readerThemeLight`/`readerThemeDark`/`bookReaderTheme`/`appIsDark` | 9 | 日/夜槽 + 每书覆盖 |
| S6 | 自动翻页开关 | `autoScroll`/`readerAutoScrollSec`/`showReaderSettings` | 4 | 会话瞬态 + 持久化速率 |
| S7 | 替换规则 | `replacer`/`showRepl`/`reloadToken` | 4 | §5.2;`reloadToken` 是**全局 remount 令牌**(见§3 不变量) |
| S8 | 编辑/版本态 | `textContent`/`editing`/`editBuffer`/`editLabel`/`currentVersionId`/`showVersions`/`showProofread` | 8 | §5.3,仅文本 |
| S9 | 派生 computed | `url`/`title`/`readerKey`/`supportsReplace`/`supportsEdit` | 8 | |
| S10 | 格式→渲染器类别 | `TEXT_FORMATS`/`kind`/`isMarkdown`/`textSource`/`isFoliateKind`/`isTxt` | 26 | 入库权威门收敛后的判定 |
| S11 | 工具栏折叠胶水 | `foldableEl`/`moreBtnEl`/`showMoreMenu`/`toolbarItemKeys`/`useToolbarOverflow(...)`/`folded()`/`menuAct()`/`watch(hasOverflow)` | 46 | 消费既有 composable `useToolbarOverflow`,项键序须与模板 DOM 顺序逐项对齐 |
| S12 | 每书设置(编码/重排/简繁) | `bookEncoding`/`bookReflow`/`bookZhConvert` | 5 | 与 S16 `onBookPrefsChange` 成对 |
| S13 | 导航/TOC 数据 | `readerFraction`/`readerTocLabel`/`readerTocHref`/`readerToc`/`showToc` | 7 | |
| S14 | 书内搜索状态 | `showSearch`/`searchResults`/`searching`/`searchProgress`/`searchGen` | 7 | `searchGen` 是非响应式 `let` 代次令牌 |
| S15 | 书签状态 | `showBookmarks`/`bookmarks` | 3 | |
| S16 | 沉浸态代理 | `immersive` computed(读写代理 `viewer.isImmersive`) | 9 | P5 已并入 `viewerStore` |
| S17 | `usePager` 接线 | `pager = usePager(...)`/`onReady` | 12 | 既有 composable,消费方不动 |
| S18 | 阅读进度去抖保存 | `lastProgress`/`saveTimer`/`onProgress`/`flushProgress` | 22 | 2026-07-10 B12 修复:捕获 `(itemId,pos)` 配对,防路由切换串档(**红线**) |
| S19 | `onInfo` | `onInfo` | 3 | pdf 页码回调 |
| S20 | 导航事件处理 | `onLocate`/`onToc`/`onFlowForced`/`onTocNavigate` | 19 | `onFlowForced` 的"只提示一次"判据 |
| S21 | 面板互斥切换(部分) | `toggleToc`/`toggleReaderSettings`/`toggleSearch` | 27 | 三者互相 `= false` 对方 |
| S22 | 书内搜索逻辑 | `onSearch`(async generator 消费)/`onSearchNavigate`/`closeSearch` | 44 | 代次令牌校验贯穿整段 |
| S23 | 书签逻辑 | `loadBookmarks`/`toggleBookmarks`/`onAddBookmark`/`onBookmarkNavigate`/`onDeleteBookmark` | 52 | |
| S24 | 面板互斥总控 | `panelFlags`/`anyPanelOpen`/`closeAllPanels` | 14 | **新增面板必须登记于此**(原注释原话) |
| S25 | 沉浸 + Esc 分层退出 | `enterImmersive`/`onGlobalKeydown` | 27 | 四层:全屏守卫让行→沉浸→面板→返回 |
| S26 | 文档加载编排 | `load()` | 70 | 复位全部瞬态 + 拉取 detail/进度/文本/版本/每书 prefs |
| S27 | 编辑/版本动作 | `startEdit`/`cancelEdit`/`saveNewVersion`/`overwriteSource`/`refreshText` | 63 | P1-16 补的失败提示 |
| S28 | 替换规则加载 | `loadReplacer`/`onReplChanged` | 13 | |
| S29 | 返回/外部打开 | `goBack`/`openExternal` | 10 | |
| S30 | 小设置持久化 | `savePagerMode`/`saveReaderFlow`/`saveStyleMode` | 11 | |
| S31 | 排版变更处理 | `captureCurrentPosition`/`onTypographyChange` | 100 | 含 14 项 diff-then-write 表 + `verticalChanged` remount 判据(**红线**) |
| S32 | 每书设置变更处理 | `onBookPrefsChange` | 34 | `needsRemount` 判据 + remount 前位置捕获(**红线**) |
| S33 | 启动引导:持久化设置批量拉取 | 14 个 `invokeIpc(IPC.GET_APP_CONFIG,...)` 链 | 97 | 与 S30/S31/S12 一一对应 |
| S34 | activeViewer 单源接线 | `viewerApi`/`readerSnapshot`/`watch(detail)` | 41 | **外部契约**:`src/commands/builtins/viewer-reader.ts` 按名调用 `ctx.activeViewer.api.toggleToc/toggleSearch/toggleBookmarks/toggleSettings`(**红线**) |
| S35 | 生命周期收尾 | `watch(id, load)`/`onMounted`/`onBeforeUnmount` | 11 | |

### 1.3 Style(305 行)

| 区块 | 行号(约) | 职责 |
|------|---------|------|
| 基础布局 `.doc-viewer`/沉浸退出按钮 | 1574–1607 | 根容器 + 悬浮按钮 |
| 工具栏基础 `.doc-viewer__toolbar`/`__btn`/`__sep`/`__back`/`__title`/`__page` | 1608–1701 | |
| 折叠簇 + 溢出菜单 `.doc-viewer__foldable`/`.fold-item*`/`.doc-more*`/`.doc-viewer__mode` | 1703–1813 | 与 S11 一一对应,量尺态(`is-measuring`/`is-settling`)CSS 契约 |
| 渲染区 `.doc-viewer__body`/`__reader`/`__unsupported` | 1814–1837 | |
| 编辑态 `.doc-edit*` | 1838–1878 | |

---

## 2. 拆分方案

设计原则:**不发明新的跨切面抽象**,以原文件已有的 `// ──` 注释横幅分段为准划界(作者已用注释标好域,风险最低的做法是照此切,而非另立分类学)。所有组合(互斥/编排)留在根组件的 `<script setup>` 里做**参数注入式装配**,composable 之间**不互相 import**——避免循环依赖,也让每个 composable 可独立写 vitest。

### 2.1 子组件(路径 / 职责 / 迁移符号 / 接口)

| 文件 | 职责 | 迁移符号(源 §1.2/1.1) | 接口方向 |
|------|------|----------------------|---------|
| `src/components/doc/DocToolbar.vue` | 整条工具栏 + 折叠/溢出菜单(纯展示,不持有业务状态) | 模板 4–441 行;脚本 S11(`foldableEl`/`moreBtnEl`/`showMoreMenu`/`toolbarItemKeys`/`useToolbarOverflow`/`folded`/`menuAct`/`watch(hasOverflow)` 整段随组件下沉,因为它 100% 为该组件私有,不被其它域消费);样式:折叠簇 + 溢出菜单 + `.doc-viewer__mode`(约 180 行) | **props(单向)**:`kind`/`isFoliateKind`/`editing`/`supportsEdit`/`supportsReplace`/`hasDetail`/`pageInfo`/`readerFraction`/`readerTocLabel`/`flowForced`/`sidebarVisible`/`title` + 各面板"是否激活"布尔(`showSearch`/`showToc`/`showBookmarks`/`showReaderSettings`/`autoScroll`/`showVersions`/`showProofread`/`showRepl`)。**v-model**:`pagerMode`/`readerFlow`/`styleMode`。**emits**:`toggle-sidebar`/`back`/`toggle-search`/`toggle-toc`/`toggle-bookmarks`/`toggle-reader-settings`/`toggle-auto-scroll`/`enter-immersive`/`start-edit`/`toggle-versions`/`toggle-proofread`/`toggle-replace`/`open-external` |
| `src/components/doc/DocEditPane.vue` | 编辑态工具条 + textarea(纯展示) | 模板 458–474 行;样式 `.doc-edit*`(约 40 行) | **v-model**:`editLabel`/`editBuffer`。**emits**:`save-new-version`/`overwrite-source`/`cancel` |

**不建议抽出的部分**(显式裁决,非遗漏):
- **沉浸退出浮动按钮**(443–452,9 行模板 + ~25 行样式):体量太小,抽出的接口成本(1 prop + 1 emit)高于维持现状,建议原地保留或并入 `DocToolbar.vue` 的具名插槽。
- **渲染区分发**(454–523,`readerRef` 承接 `PdfReader`/`BookReader`):**不抽**。`readerRef` 被 S17/S18/S20/S22/S23/S31 共 6 处消费方直接调用 `next/prev/getScrollEl/goToHref/searchBook/clearSearch/getCurrentLocation/goToLocator` 共 8 个方法;若下沉进子组件,需 `defineExpose` 转发全部 8 个方法,属于典型"响应式边界"高风险操作(见§3),且模板本身只是一个 3 分支 `v-if` 链,结构move的收益低。保留在根组件。
- **七个侧栏面板**:已是独立 SFC,无需再拆,根组件只做 `v-if` + props 转发。

### 2.2 Composables(新增目录 `src/composables/reader/`,与既有 `src/composables/player/`、`src/composables/selection/` 子目录风格对齐)

| 文件 | 迁移符号 | 约行数 | 依赖(注入) | 说明 |
|------|---------|--------|-----------|------|
| `useReaderProgress.ts` | S18 | 30 | `id`(route computed) | 纯本地闭包,零跨域依赖,**P0 优先** |
| `useBookSearch.ts` | S14 + S21(`toggleSearch`部分) + S22 | 75 | `readerRef`、`closeOtherPanels: () => void` | `searchGen` 保持非响应式 `let`;`showSearch` 与其开关逻辑保留同域(不拆給编排层,避免一个布尔态跨两个文件) |
| `useReaderBookmarks.ts` | S15 + S23 | 60 | `id`、`readerRef`、`closeOtherPanels: () => void` | |
| `useReaderNav.ts` | S13 + S20(`onLocate`/`onToc`/`onTocNavigate`) + S21(`toggleToc`部分) | 45 | `closeOtherPanels: () => void` | TOC 数据与开关同域 |
| `useReaderTypography.ts` | S4 + S31 + S33 中对应 10 项设置的启动引导 | 230(现最大单块) | `requestRemount(captureFirst: boolean): void`(根组件注入) | **建议单独一步落地并逐项核对 14 个 `SET_APP_CONFIG` key 与范围守卫不漏项**(见§4) |
| `useReaderBookPrefs.ts` | S12 + S32 + `load()` 中每书 prefs 解析段(原 S26 内联,收成 `loadForItem(itemId)` 方法) | 65 | `requestRemount(captureFirst: boolean): void` | 与上一条共用 remount 回调,建议同批验证 |
| `useDocEditVersion.ts` | S8(`showVersions`/`showProofread`/`textContent`/`editing`/`editBuffer`/`editLabel`/`currentVersionId`) + S27 | 90 | `id`、`supportsEdit`、`requestRemount`(仅 `refreshText` 用) | |
| `useReaderPanels.ts` | S24 + S6 的 `showReaderSettings`(留根或此处二选一,见下) | 30(**变薄**) | 输入:各域已实例化后的 `Ref<boolean>[]` 数组 + `closeSearch: () => void`;输出:`anyPanelOpen` computed、`closeAllPanels()` | 设计为**薄编排工具**而非状态持有者——状态仍分散在各自域(`useBookSearch.showSearch`/`useReaderNav.showToc`/`useDocEditVersion.showVersions,showProofread`/根组件 `showRepl,showReaderSettings`),本 composable 只做汇总,避免"两处依赖同一状态"的耦合疑虑 |
| `useDocImmersive.ts` | S16 + S25 | 40 | `anyPanelOpen`、`closeAllPanels`、`goBack`(均来自上一条/根组件) | Esc 分层退出的 `window.addEventListener('keydown',...)` 注册/注销随本 composable 的 `onMounted`/`onBeforeUnmount` 走;与 `ContentViewer.vue` 同款 Esc 语义存在**未来可共享**的机会,但本次不做跨文件合并(范围外) |
| `useDocActiveViewerSync.ts` | S34 | 45 | `id`/`detail`/`title`/`immersive`/`goBack` + 前述所有域的 toggle 函数**引用**(非拷贝) | **整合层**,必须在其它 composable 实例化之后才能装配 `viewerApi`;`viewerApi` 对象形状是外部契约(见§3),装配时禁止"重新实现"任何 toggle,只能透传引用 |

**不建议拆出的部分**:S2/S3/S5/S7/S9/S10/S17/S19/S26/S28/S29/S30(+对应 S33 里 pagerMode/readerFlow/styleMode 三项启动引导)——量小(pagerMode/readerFlow/styleMode 合计约 30 行)且与 `kind`/`isFoliateKind` 判定紧邻,拆出反而分散上下文,留根组件。

### 2.3 依赖方向(装配顺序即施工顺序,见§4)

```
DocumentViewer.vue(根 <script setup>)
  ├─ 叶子层(互不依赖,任意顺序实例化):
  │    useReaderProgress(id)
  │    useReaderNav({ closeOtherPanels })          ← closeOtherPanels 暂用占位闭包,实际由编排层回填
  │    useBookSearch(readerRef, { closeOtherPanels })
  │    useReaderBookmarks(id, readerRef, { closeOtherPanels })
  │    useReaderTypography({ requestRemount })
  │    useReaderBookPrefs({ requestRemount })
  │    useDocEditVersion(id, supportsEdit, { requestRemount })
  ├─ 编排层(依赖叶子层的 refs/fn):
  │    useReaderPanels([...叶子层 show* refs, showRepl, showReaderSettings], { closeSearch })
  │    useDocImmersive({ anyPanelOpen, closeAllPanels, goBack })
  ├─ 整合层(依赖以上全部):
  │    useDocActiveViewerSync({ id, detail, title, immersive, goBack, ...toggle 函数引用 })
  └─ 渲染:
       <DocToolbar v-bind="…" v-model:pager-mode=… @toggle-toc="…" />
       <DocEditPane v-if="editing" v-model:edit-label=… @save-new-version="…" />
       <PdfReader|BookReader ref="readerRef" …/>(原地不动)
       <ReplacementPanel>…</ReplacementPanel> 等七个面板(原地不动,props 源已从对应 composable 取)
```

组合根组件不持有业务逻辑,只做**声明式装配**——这与`useToolbarOverflow`已经建立的"composable 提供能力、组件消费"的既有范式一致,不是新范式。

---

## 3. 风险与不变量

1. **foliate 接线契约(高风险)**:`BookReader.vue` 的 `props`(`flow`/`immersive`/`pageTurn`/`autoScroll`/`autoScrollSec`/`readerThemeLight`/`readerThemeDark`/`bookReaderTheme`/`typography`/`vertical`/`styleMode`/`replacer`/`zhConvert`/`textSource`/`url`/`initial`)与 `emits`(`ready`/`progress`/`locate`/`toc`/`flow-forced`)按**名字**精确匹配、且内部有 `watch(() => props.typography, ..., { deep: true })` 之类的深度监听。`useReaderTypography` 返回的 `readerTypography` computed **必须原样保持字段名与结构**(`fontSizePx`/`lineHeight`/`fontFamily`/`maxInlineSizePx`/`fontWeight`/`letterSpacingEm`/`textAlign`/`titleScale`/`paragraphSpacingEm`),composable 边界不能改变这个对象的 identity 语义(仅实变时才产新对象,配合 deep watch 精准重排——这是原注释明确写的设计前提)。
2. **remount 时序不变量(高风险,红线)**:`verticalChanged`(竖排↔横排,`onTypographyChange` 内)与 `needsRemount`(编码/reflow/简繁,`onBookPrefsChange` 内)是**仅有的两条**触发 `reloadToken++` 的路径,且都必须**先 `captureCurrentPosition()` 再 `reloadToken.value++`**——顺序颠倒会导致 remount 后阅读位置跳回开卷(真机曾验证过的坏体验)。拆分后这两处逻辑分居 `useReaderTypography`/`useReaderBookPrefs` 两个文件,统一通过根组件注入的同一个 `requestRemount(captureFirst)` 回调调用,**该回调内部实现只能有一份**,不能两个 composable 各自写一份"capture+bump"逻辑(否则未来改时序容易漏改一处)。
3. **ViewerApi 外部契约(高风险)**:`src/commands/builtins/viewer-reader.ts` 通过 `ctx.activeViewer.api.toggleToc/toggleSearch/toggleBookmarks/toggleSettings` **按方法名**调用(顶栏统一命令层,与阅读器自带工具栏"双入口"并存,P5-5 裁决)。`useDocActiveViewerSync` 组装 `viewerApi` 时必须**引用**各 composable 导出的同一个函数对象,不能重新包一层新函数——否则两个入口触发的实际是不同函数实例,状态可能不同步(虽然目前每个 toggle 内部都是直接改 ref,包一层也不会立即出错,但会引入"哪个是权威实现"的可维护性隐患,应避免)。
4. **阅读进度捕获配对(中风险,红线)**:`onProgress` 捕获 `(itemId, pos)` 配对而非只存 `pos`——2026-07-10 审查 B12 修复,目的是防止路由切换时 flush 用错 id(串档写入)。`useReaderProgress` 抽出时必须保留这个配对写法,不能"简化"回只存 `pos`。
5. **面板互斥登记表(中风险)**:原注释"新增面板必须登记于此"——`panelFlags` 数组决定 `anyPanelOpen`/`closeAllPanels`(进沉浸收拢 + Esc 第③层退出都读它)。`showSearch` **有意不在数组里**(它的关闭需要额外清理搜索迭代/高亮,统一走 `closeSearch()`)。拆分后这份"登记表"由根组件在装配 `useReaderPanels` 时**手工列出全部 flags**,必须与原数组成员一一对应(`showToc`/`showReaderSettings`/`showBookmarks`/`showRepl`/`showVersions`/`showProofread`,**不含** `showSearch`),漏登记 = 沉浸态残留面板或 Esc 跳层——这正是原注释警告的坑,拆分时最容易在"这个 flag 现在在哪个文件"的迁移中漏掉。
6. **Esc 分层退出的注册时序(中风险)**:四层退出依赖 `useFullscreenExitGuard` 在 `window` **捕获阶段**先手(全屏时让行或吞掉),`onGlobalKeydown` 本身在**冒泡阶段**监听。`useDocImmersive` 内部注册 `keydown` 监听器的挂载/卸载时机(`onMounted`/`onBeforeUnmount`)必须与原来一致,不能提前到某个子 composable 的更早期生命周期钩子,否则可能影响与其它 window 级 Escape 监听器的相对顺序。
7. **響应式边界(中风险,通用)**:所有 composable 必须**返回 ref/computed 本身**(不要在 return 时 `.value` 解包再包一层新 ref),否则模板绑定或 `readerTypography` 之类的 deep watch 会失去响应性——这是 composable 抽取的常见坑,需在实现阶段逐个 composable 核对返回值类型与调用处解构方式(`toRefs`/直接返回对象两种写法均可,但不能混用导致某个字段悄悄变成非响应式快照)。
8. **searchGen 非响应式(低风险但需保留)**:`searchGen` 是普通 `let`(非 `ref`),依赖同步自增+异步循环内比较的时序;抽出到 `useBookSearch.ts` 后**不要**改成 `ref<number>`(无必要的响应性开销,且原设计就是闭包变量语义)。
9. **md/txt 分片护栏(中风险,既有裁决,不可回炉)**:`flowForced` 的"只提示一次"判据(`if (flowForced.value) return`)与其在 `load()` 里随换文档复位的行为属于既有裁决(2026-07-17),拆分只能原样搬运,不能"顺手"改成每次挂载都提示或改变触发时机。
10. **TypeScript strict / no any**:项目硬约束(`CLAUDE.md`)。新 composable 的入参/返回值需显式接口(尤其 `FoliateTocItem`/`FoliateSearchResult`/`ReaderBookPrefs`/`ZhConvertConfig`/`ReaderBookmark` 等既有类型的导入路径要在新文件里重新 `import type` 一次,注意 `../vendor/foliate-js/view.js` 与 `../types/reader` 等相对路径深度随文件位置变化需要调整)。

---

## 4. 收益与优先级

**规模预估**(现 1878 行):

| 产物 | 预估行数 | 备注 |
|------|---------|------|
| `DocumentViewer.vue`(拆后) | ~600(script ~350 / template ~130 / style ~120) | 降幅约 68% |
| `DocToolbar.vue` | ~700(template ~440 / script ~90 / style ~180) | 单文件最大,但纯展示,无跨域状态 |
| `DocEditPane.vue` | ~70 | |
| `useReaderTypography.ts` | ~230 | composable 中最大单块 |
| `useDocEditVersion.ts` | ~90 | |
| `useReaderBookPrefs.ts` | ~65 | |
| `useBookSearch.ts` | ~75 | |
| `useReaderBookmarks.ts` | ~60 | |
| `useReaderNav.ts` | ~45 | |
| `useDocActiveViewerSync.ts` | ~45 | |
| `useDocImmersive.ts` | ~40 | |
| `useReaderProgress.ts` | ~30 | |
| `useReaderPanels.ts` | ~30(薄编排) | |

**施工顺序建议**(逻辑层先行、风险从低到高;模板/组件抽离放最后一批,避免逻辑与视图同时变动互相掩盖问题):

- **P0(可任意顺序/并行,零跨域依赖)**:`useReaderProgress` → `useBookSearch` → `useReaderBookmarks` → `useReaderNav`。每步跑一次 vue-tsc + 对应面板手测。
- **P1(依赖 P0 产出)**:`useReaderPanels`(薄编排) → `useDocImmersive`(依赖 P1 的 `anyPanelOpen`/`closeAllPanels`)。
- **P2(体量最大、remount 时序红线,建议单独成步、逐项核对)**:`useReaderTypography` → `useReaderBookPrefs`(共用 `requestRemount`,建议紧邻验证竖排/编码切换后阅读位置不跳)。
- **P3**:`useDocEditVersion`。
- **P4(整合,须等 P0–P3 全部落地)**:`useDocActiveViewerSync`(viewerApi 装配依赖前面所有 toggle 函数的最终引用)。
- **P5(模板抽离,最后一批)**:`DocToolbar.vue`、`DocEditPane.vue`。

---

## 5. 验证策略

- **静态检查**:`vue-tsc --noEmit`(strict/no any)、`eslint` + `prettier`(修复用 `npm run lint:fix`,不跑仓库级 `npm run format`)——每个 composable/组件落地后单跑一次,不攒到最后。
- **新增 vitest**(风险驱动,对应§3 红线):
  - `useReaderProgress.spec.ts`:去抖 1200ms flush;路由切换时旧 `(itemId,pos)` 不串档写入新 id。
  - `useBookSearch.spec.ts`:`searchGen` 令牌使旧迭代产出失效;`closeSearch` 空状态早退不产生多余调用。
  - `useReaderTypography.spec.ts`:`onTypographyChange` 只对"真变化"的键调用 `SET_APP_CONFIG`(连点步进不冗余写);仅 `vertical` 变化触发 `requestRemount`。
  - `useReaderBookPrefs.spec.ts`:仅 `encoding`/`reflow`/`zhConvert` 变化触发 remount,`theme` 变化不触发。
  - `useReaderPanels.spec.ts`:互斥(开一个关其余)、`closeAllPanels` 覆盖登记表全部成员且不含 `showSearch`。
  - 既有 `usePager.spec.ts`/`useToolbarOverflow.spec.ts` 不变,`DocToolbar.vue` 如有余力可加 props→emit 映射的组件测试(非必须)。
- **GUI 手测点**(标注 not automated):
  1. pdf/epub/txt/md 四种 `kind` 逐一打开,工具栏折叠/`⋯`溢出菜单窄窗表现一致。
  2. 竖排 `vertical` 切换后阅读位置不跳回开卷(remount 位置捕获)。
  3. 编码/重排/简繁切换后同上;仅主题切换**不** remount(实时重着色)。
  4. 沉浸模式进入/退出 + Esc 四层退出(全屏优先→沉浸→面板→返回)逐层验证。
  5. 书内搜索连续换关键词/换书,结果不串档。
  6. 书签增/删/定位。
  7. 自动翻页开关 + 速率调整。
  8. 编辑态保存新版本/覆盖源文件,成功与失败提示均正确。
  9. 顶栏统一命令(`viewer-reader.ts` 的 TOC/搜索/书签/设置)与阅读器自带工具栏"双入口"状态同步,不出现行为分裂。

---

顺手发现:无。

## 附注:CSS 外置(D-451,2026-07-25 补录)
- 本组件 `<style scoped>` 可整块外置为同目录 `DocumentViewer.styles.css`,SFC 留 `<style scoped src="./DocumentViewer.styles.css"></style>`;外置文件仍编译为宿主组件 style 块,scope id 归属不变,`:deep()` 穿透语义不变。
- 前提(2026-07-25 核实):全仓 .vue 样式零 `v-bind()`;本文件为单一 `<style scoped>` 块。
- 定位:可选先行批、全场风险最低的行数削减刀;不替代 script 拆分主刀。
- 施工顺序:首刀拿最小文件实测 Vite 构建链 + HMR,通过后铺开。
