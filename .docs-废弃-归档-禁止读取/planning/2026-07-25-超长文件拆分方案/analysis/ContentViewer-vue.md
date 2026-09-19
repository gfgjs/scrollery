---
id: 2026-07-25-ContentViewer-vue
status: draft
type: plan
line: 超长文件拆分方案
created: 2026-07-25
target: src/components/media/ContentViewer.vue
---

# ContentViewer.vue 拆分方案

> 只读分析,禁改代码。行号以本次读取快照为准(~2036 行/~86KB),并行会话正在精简本文件注释,
> 行号会漂移——落地施工时请以本文列出的**符号名**重新定位,行号仅作大致参考。

## 0. 背景与红线

- 该组件是「顶栏重构 P4-b」后的路由化看图台(`/view/:id`),整合了图片/视频/音频/文档四态渲染、
  缩放平移旋转、人脸框、ICC 渲染色域切换、图片简单编辑、OCR、影像增强、Exotic 授权 gate、
  Live Photo、沉浸模式、底部工具条与文件信息面板、右键菜单与移动/复制对话框、快捷键、
  `activeViewer`(顶栏上下文)契约共 ~13 个功能域。
- 视频悬停 scrub/poster、ICC 色域链是既有裁决产物(见 memory 索引「视频悬停scrub共存+poster」
  「自定义ICC与色域切换线」),本方案**不改变**其调用点/参数/时序,只挪动代码物理位置。
- `useMediaDetail()`、`useVideoSource()`、`useViewerImageSource()`、`useViewerColorSource()`
  均已是独立 composable(grep 确认仅本文件 + 各自 `.spec.ts` 引用),不在本次拆分范围内——
  它们是"仅创建一次"的状态源,拆分时只应作为参数传入新 composable,不得被重复实例化。

## 1. 现状结构图

### 1.1 `<template>`(1–507 行,~507 行)

| 区块 | 行号区间(约) | 职责 | 关键符号锚点 |
|---|---|---|---|
| 外壳 `.content-viewer` / `.detail-panel` | 6–15 | 路由填充容器 + 信息面板停靠挤压 | `ui.viewerInfoVisible` |
| 媒体舞台 `.detail-viewer` | 16–171 | 不可用占位 → Exotic gate → 编辑覆层 → 图/视频/音频/文档 v-if/v-else-if 链;Live Photo 覆盖视频;人脸框叠加;OCR 面板挂载点 | `isUnavailable` `showExoticGate` `viewerImage.displaySrc` `videoSource.src` `videoOverlayMode` `faces` `faceBoxStyle` |
| 底部工具条 `.detail-controls` | 173–320 | 左(缩放/翻页计数/旋转/Live/编辑/OCR/增强/色域菜单)/中(文件名)/右(人脸开关/收藏/资源管理器/信息/关闭) | `state.zoomIn/zoomOut` `handleToggleZoom` `handleRotate` `toggleLive` `openEditor` `onOcrImage` `openEnhance` `toggleFaces` `toggleFav` `showInExplorer` `ui.toggleViewerInfo` `close` |
| 沉浸退出浮动按钮 | 322–331 | 沉浸态下唯一浮动退出入口 | `isImmersive` `toggleImmersive` |
| 文件信息面板 `.detail-info` | 333–450 | 基本信息/EXIF/评分/颜色标签,`<Transition name="slide">` | `formatFileSize` `formatDateTime` `formatFocalLength` `formatAperture` `formatGps` `setRating` `setColorLabel` |
| 尾部对话框聚合 | 455–506 | `ContextMenu` / `FolderTreeSelectorDialog` / `ExoticActivateDialog`×2 / `UiDialog`(编辑 gate)/ `EnhanceDialog` | `ctxMenu` `moveCopyDialog` `activateOpen` `editGateOpen` `editActivateOpen` `enhanceOpen` |

### 1.2 `<script setup>`(509–1600 行,~1090 行)

按出现顺序的逻辑域(每域给锚点符号,便于注释精简后按名重新定位):

1. **导入 + store/route 接线**(510–620):20+ 个 composable/store/util 导入;`media/person/toast/history/ui/config/route/router/viewer/editor/editingGate/ocr` 等实例化。
2. **右键菜单构建**(621–681):`onContextMenu`、`ctxMenu`、`moveCopyDialog`——本地 `move/copy` 命令与命令注册表 `resolveMediaContextCommands` 合流。
3. **`detail` + 渲染色域换源**(683–712):`detail`、`viewerColor`(`useViewerColorSource`)、`absPath`、`viewerImage`(`useViewerImageSource`)。
4. **图片占位 / 视频海报候选**(714–783):`thumbCacheDir`、`imagePlaceholderCandidate/Broken/imagePlaceholder`、`videoPosterCandidate`、`posterBroken`/`healedPosterIds`/`videoPoster`——含"封面 404 自愈"探测逻辑。
5. **视频源接线**(785–797):`videoSource`(`useVideoSource`)、`videoOverlayMode`。
6. **不可用态**(799–855):`loadError`、`isUnavailable`、`unavailableInfo`、`onMediaError`、`onImgLoad`、`onVisibleImageError`。
7. **Exotic 授权 gate**(857–892):`exoticGate`、`showExoticGate`、`exoticFeatureName`、`onExoticActivated`。
8. **查看器状态创建 + 缩放/旋转基础**(894–995):`state = useMediaDetail()`(**仅此一处创建**)、`viewerRef/imgRef/videoRef`、`currentImageElement`、`getMediaDimensions`、`handleToggleZoom`、`rotationDeg`/`rotateLabel`/`handleRotate`(旋转持久化乐观回写)。
9. **图片编辑/OCR/增强 入口与收尾**(997–1068):`openEditor`、`onOcrImage`、`enhanceSource`/`openEnhance`、`openEditingActivation`/`onEditingActivationClosed`/`onEditingActivated`、`onEditorClosed`/`onEditorSaved`。
10. **缩放比例与旋转复原**(1070–1122):`pendingInitialRotation` watch(切图复原持久旋转)、`zoomRatio`/`updateZoomRatio`(内部调用 `recomputeFaceLayout`)、`isZoomChanged` 高亮计时。
11. **人脸框**(1124–1220):`faces`/`showFaces`/`toggleFaces`、`faceContentRect`、`faceToken`(并发新旧应答防御)、`loadFacesFor`、`recomputeFaceLayout`、`scheduleFaceRecompute`(rAF 节流)、`viewerRO`(ResizeObserver)、`faceBoxStyle`。
12. **键盘快捷键**(1222–1277):`onKeydown`(编辑态优先/Esc 双层语义/`dispatchKeybinding` 收敛)。
13. **滚轮翻页**(1279–1308):`onWheelHandler`、`accumulatedDelta`/`deltaTimer`。
14. **卷插拔监听**(1310–1315):`useTauriListen(EVENTS.VOLUMES_CHANGED, …)`。
15. **路由驱动翻页**(1317–1357):`routeId`、`loadFromRoute`、`navigate`、`closeViewer`、`close`。
16. **沉浸模式**(1359–1367):`isImmersive`、`toggleImmersive`。
17. **底部工具条显隐**(1369–1383):`controlsHidden`/`toggleControls`、`detailControlsVisible`。
18. **点击/拖拽判别**(1385–1401):`onViewerPointerDown`、`onViewerClick`(位移阈值 4px)。
19. **`activeViewer` 契约**(1403–1463):`viewerApi`(`defineExpose`)、`viewerSnapshot`、populate/patch watch——**几乎引用本文件所有其他函数**,是全组件的汇聚点。
20. **挂载/卸载生命周期**(1465–1491):`onMounted`/`onBeforeUnmount`(keydown 监听、`thumbCacheDir` 拉取、`loadFromRoute`、清理 `viewerRO`/`state.cleanup`/`viewer.clear`/`media.closeDetail`/`ocr.closePanel`)。
21. **收藏/评分/色标**(1493–1514):`toggleFav`、`setRating`、`setColorLabel`。
22. **资源管理器定位**(1516–1537):`showInExplorer`。
23. **Live Photo**(1539–1560):`toggleLive`。
24. **移动/复制确认**(1562–1599):`onMoveCopyConfirm`。

### 1.3 `<style scoped>`(1602–2036 行,~434 行)

纯表现层,按 BEM 前缀天然分组,与 template 区块一一对应:

| 前缀 | 对应 template 区块 |
|---|---|
| `.content-viewer` `.content-viewer__inner` `.content-viewer__immersive-exit*` | 外壳 + 沉浸退出按钮 |
| `.detail-panel*` | 外壳(含 `--detail-info-w`/`--detail-controls-h` 两个供子孙消费的 CSS 变量) |
| `.detail-viewer*`(img/video/audio/document/unavailable/gate/live-video) | 媒体舞台 |
| `.face-overlay` `.face-box*` | 人脸框叠加层 |
| `.detail-controls*` `.zoom-percentage*` | 底部工具条 |
| `.detail-info*` `.info-*` `.rating-stars` `.star*` `.clickable-path*` `.slide-enter/leave-*` | 文件信息面板 |

顶部有硬编码色豁免说明注释(S5,设计 §6.2):看图台恒黑 #000、人脸框青色、星级金——拆分时这段
说明及其覆盖的选择器必须整体随对应子组件迁移,不得拆散或简化措辞。

## 2. 拆分方案

原则:composable 只接受 getter/ref 参数,不反向 import 组件;子组件是"哑组件"(props in /
emit out),不得在内部重复调用已在父组件创建一次的 composable(`useMediaDetail` 等)。

### 2.1 新增 composables(逻辑下沉,不含 template)

| 路径 | 职责 | 迁移符号 | 接口(草案) |
|---|---|---|---|
| `src/composables/useContentViewerFaces.ts` | 人脸框状态与投影计算 | `faces` `showFaces`/`toggleFaces` `faceContentRect` `faceToken` `loadFacesFor` `recomputeFaceLayout` `scheduleFaceRecompute` `faceBoxStyle` `viewerRO` | `useContentViewerFaces({ imgRef, viewerRef, currentImageElement, transform, personStore })` → `{ faces, showFaces, toggleFaces, faceBoxStyle, loadFacesFor, recomputeFaceLayout }` |
| `src/composables/useContentViewerZoomRotation.ts` | 缩放比例 + 旋转(含持久化复原) | `getMediaDimensions` `handleToggleZoom` `zoomRatio` `updateZoomRatio` `isZoomChanged` `rotationDeg`/`rotateLabel` `handleRotate` `pendingInitialRotation` watch | `useContentViewerZoomRotation({ state, viewerRef, currentImageElement, videoRef, detail, onSizeReady })`;`onSizeReady` 回调用于替代原来 `updateZoomRatio` 内联调用 `recomputeFaceLayout`,由主组件把两个 composable 串起来,调用顺序不变 |
| `src/composables/useContentViewerPosterSource.ts` | 图片加载占位 + 视频海报候选(含 404 自愈) | `thumbCacheDir` `imagePlaceholderCandidate/Broken/imagePlaceholder` `onImagePlaceholderError` `videoPosterCandidate` `posterBroken`/`healedPosterIds` `videoPoster` | `useContentViewerPosterSource({ detail })` → `{ thumbCacheDir, imagePlaceholder, onImagePlaceholderError, videoPoster }` |
| `src/composables/useContentViewerKeyboard.ts` | 键盘快捷键(自带 mounted/unmounted 生命周期,模式同既有 `useTauriListen`) | `onKeydown` 及其 `document.addEventListener`/`removeEventListener` | `useContentViewerKeyboard({ media, editor, viewer, videoRef, close })`(内部自行挂卸载,组件侧不再手写 `onMounted`/`onBeforeUnmount` 里的 keydown 两行) |
| `src/composables/useContentViewerRouteNav.ts` | 路由驱动翻页(不含 `close`/`closeViewer`,那两个要跨多个域被引用,留主组件) | `routeId` `loadFromRoute` `navigate` | `useContentViewerRouteNav({ media, editor, router, route })` → `{ routeId, loadFromRoute, navigate }` |
| `src/composables/useContentViewerContextMenu.ts` | 右键菜单 + 移动/复制对话框 | `onContextMenu` `ctxMenu` `moveCopyDialog` `onMoveCopyConfirm` | `useContentViewerContextMenu({ detail, history, toast, t })` → `{ ctxMenu, moveCopyDialog, onContextMenu, onMoveCopyConfirm }` |
| `src/composables/useContentViewerQuickActions.ts` | 收藏/评分/色标/资源管理器定位/Live Photo | `toggleFav` `setRating` `setColorLabel` `showInExplorer` `toggleLive` | `useContentViewerQuickActions({ detail, media, toast, t })` → 五个函数直出 |

**不下沉的域**(维持现状,理由见风险章节):`viewerApi`/`viewerSnapshot`/`defineExpose`(19)、
Exotic gate(7)、编辑/OCR/增强入口(9)、`onWheelHandler`(13)、`onViewerPointerDown/onViewerClick`(18)、
`isImmersive`/`toggleImmersive`(16)、`controlsHidden`/`detailControlsVisible`(17)——均属于"胶水层",
且 `viewerApi` 天然要引用几乎所有其他函数,再拆一层只会多一层间接、不减耦合。

### 2.2 新增子组件(template + 对应 scoped style)

| 路径 | 职责 | 迁移符号 | Props / Emits(草案) |
|---|---|---|---|
| `src/components/media/ContentViewerControls.vue` | 底部工具条(左/中/右三组按钮) | template 173–320 行 + style `.detail-controls*`/`.zoom-percentage*` | Props: `detail`(只读字段)、`zoomRatio`、`isZoomChanged`、`zoomMode`、`rotationDeg`/`rotateLabel`、`showFaces`、`facesCount`、`isFavorited`、`viewerInfoVisible`、`isMobilePlatform`、`ocrBusy`、`navContext`;Emits: `zoom-in` `zoom-out` `cycle-zoom` `rotate` `toggle-live` `open-editor` `ocr` `enhance` `toggle-faces` `toggle-fav` `show-in-explorer` `toggle-info` `close` |
| `src/components/media/ContentViewerInfoPanel.vue` | 文件信息面板(基本信息/EXIF/评分/色标) | template 333–450 行 + style `.detail-info*`/`.info-*`/`.rating-stars`/`.star*`/`.clickable-path*`/`.slide-*` | Props: `visible`、`detail`;Emits: `close` `show-in-explorer` `set-rating` `set-color-label` |
| `src/components/media/FaceOverlay.vue`(可选,低优先级) | 人脸框叠加层 | template 151–163 行 + style `.face-overlay`/`.face-box*` | Props: `faces`、`faceBoxStyle`(函数或预计算样式数组)、`visible` |
| `src/components/media/ContentViewerDialogs.vue` | 尾部对话框聚合(纯转发,无自身状态) | template 455–506 行 | Props: `ctxMenu` `moveCopyDialog` `activateOpen` `editGateOpen` `editActivateOpen` `enhanceOpen` `exoticGate` `editingGate` `enhanceSource`;Emits: 逐个对话框的 `close`/`confirm`/`activated` 透传 |

**不拆的区块**:媒体舞台(`.detail-viewer` 内 img/video/audio/document/unavailable/gate 的
v-if/v-else-if 链,template 16–171 行)。理由见下节。

### 2.3 依赖方向

```
ContentViewer.vue(编排层,保留 viewerApi/生命周期/媒体舞台 template)
  ├─ 调用 useMediaDetail / useVideoSource / useViewerImageSource / useViewerColorSource(维持原样,仅创建一次)
  ├─ 调用新 composables(2.1),将其返回的 ref/函数作为 props 传给 ↓
  ├─ <ContentViewerControls>  (props ← 组合结果,emit → 组件内函数)
  ├─ <ContentViewerInfoPanel> (props ← detail/emit → setRating 等)
  ├─ <FaceOverlay>            (props ← useContentViewerFaces 结果)
  └─ <ContentViewerDialogs>   (props ← 各 open 状态 ref)
```

单向:composables 不 import 子组件;子组件不直接调用 composable(避免重复实例化),只吃 props/emit。

## 3. 风险与不变量

- **`useMediaDetail()` 单例创建**:现有注释明确"仅创建一次,不要放在 computed() 内部"——拆分后
  `state` 必须仍在 `ContentViewer.vue` 根作用域创建一次,`useContentViewerZoomRotation` 等只能
  以参数形式接收 `state`,禁止在 composable 内部再调 `useMediaDetail()`。
- **媒体舞台不拆的原因**:`imgRef`/`videoRef`/`viewerRef` 三个 template ref 被 `currentImageElement`、
  `getMediaDimensions`、`recomputeFaceLayout`、`onViewerPointerDown/onViewerClick`、`updateZoomRatio`
  等十余处直接读取;若把 img/video 拆进子组件,需要额外一层 `defineExpose` + 父组件 `ref` 转发,
  只增加间接层不减少耦合,且视频悬停 scrub/poster、ICC 色域链这批红线逻辑正巧集中在这一区块——
  拆分收益低、回归风险高,故本方案建议**保留原位**。
- **人脸并发 token 防御(`faceToken`)**:方向键连续翻页时多个 `getFacesForItem` 在途,旧应答必须
  被丢弃(`my !== faceToken` 判定)。这是此前审查修复的正确性不变量,抽成 composable 时必须逐字
  保留该计数器判定,不得"简化"为取消/AbortController 等替代实现(除非另立需求)。
  `viewerRO`(ResizeObserver)必须与 `recomputeFaceLayout`/`updateZoomRatio` 的调用时序一并搬迁,
  `onBeforeUnmount` 的 `viewerRO?.disconnect()` 不能遗漏。
- **`updateZoomRatio` → `recomputeFaceLayout` 的隐式调用序**:当前 `updateZoomRatio` 函数体内直接
  调 `recomputeFaceLayout()`(1106 行)。拆成两个独立 composable 后必须显式保留这个调用序(方案
  2.1 用 `onSizeReady` 回调衔接),不能让"先更新缩放基准、再重算人脸框投影"这个顺序在拆分中丢失。
- **`pendingInitialRotation` 复原时序**:切图 watch 里先记录 `item.viewRotation`,真正落地
  `state.setRotation`/`setZoomMode` 要等 `updateZoomRatio` 首次拿到有效尺寸(由 `@load`/
  `@loadedmetadata` 触发)才执行且仅执行一次。这是"旋转持久化跨翻页复原"功能的核心时序,
  拆分时两个 ref(`pendingInitialRotation` 在切图 watch 中写、在 zoom composable 中读并清空)
  必须共享同一个 ref 实例,不能被复制成两份状态。
- **`viewerApi`/`defineExpose` 契约**:被 `commands/builtins/viewer-image.ts`、
  `commands/builtins/viewer-video.ts`、`stores/viewerStore.ts`(`ActiveViewer.api`)消费,方法名
  与签名是跨文件契约,拆分只能改变各方法"在哪个文件定义",不能改变 `viewerApi` 对象本身暴露的
  方法集合与调用方式。
- **`localStorage` 持久化键名不可改**:`detail_show_faces`、`detail_controls_hidden` 是现有
  用户偏好持久化键,下沉进 composable 时必须原样保留字符串字面量(与本仓其他线的红线一致——
  改名会丢失老用户已保存的偏好)。
- **scoped style 随迁移**:Vue `<style scoped>` 的作用域绑定在单个 SFC 上,拆出子组件时对应
  CSS 选择器块必须整体搬入子组件 `<style scoped>`,不得留在父组件而指望级联生效;`.detail-panel`
  上定义的 `--detail-info-w`/`--detail-controls-h` 两个自定义属性经由普通 CSS 继承跨 scoped 边界
  传递给子组件用是安全的(自定义属性不受 scoped data 属性限制),但前提是 `.detail-panel` 元素仍
  在父组件模板中作为祖先存在(本方案确实保留)。
- **性能热路径**:`onWheelHandler`(滚轮翻页阈值累加)、`recomputeFaceLayout`(rAF 节流)、
  `viewerRO`(ResizeObserver 驱动缩放/人脸联动重算)都是高频触发路径,拆分只能搬运闭包变量
  (`accumulatedDelta`/`deltaTimer`/`faceRaf`),不得引入每帧新增的响应式包装(如把普通局部变量
  错误地改成 `ref` 触发不必要的依赖追踪)。
- **红线域完全不动**:ICC 渲染色域链(`useViewerColorSource`/`useViewerImageSource` 调用点与其
  getter 参数)、视频 `useVideoSource`/`videoOverlayMode`/`VideoPlayer`/`VideoPreparingOverlay`
  接线,本方案不改变其位置与参数,维持在编排层(`ContentViewer.vue`)原地。

## 4. 收益与优先级

预估瘦身后(数字为逻辑行数量级估算,非精确值,施工后应重新量测):

| 文件 | 现状 | 拆后估算 |
|---|---|---|
| `ContentViewer.vue` | ~86KB / ~2036 行 | template ~180–220 行(媒体舞台 + 4 个子组件挂载点)+ script ~550–650 行(store 接线/媒体源胶水/`viewerApi`/生命周期)+ style ~120–150 行 ≈ 30–38KB |
| 7 个新 composable | — | 单文件 40–150 行不等,`useContentViewerFaces`/`useContentViewerZoomRotation` 较大(~120–150 行),其余 40–80 行 |
| 4 个新子组件 | — | `ContentViewerControls.vue` 最大(~180–220 行含 style),`ContentViewerInfoPanel.vue` 次之(~150 行),`ContentViewerDialogs.vue`/`FaceOverlay.vue` 较小(60–90 行) |

施工顺序建议(风险从低到高分批,每批可独立验证回归):

1. **第一批(零模板改动,风险最低)**:`useContentViewerContextMenu`、`useContentViewerQuickActions`、
   `useContentViewerPosterSource`——彼此无交叉依赖,可并发施工。
2. **第二批(需保留调用序,中等风险)**:`useContentViewerFaces` + `useContentViewerZoomRotation`
   一起做(因 `onSizeReady` 回调衔接,建议同一提交内完成,避免中间态调用序缺失一环)。
3. **第三批(触达生命周期/键盘,中等风险)**:`useContentViewerKeyboard`、`useContentViewerRouteNav`。
4. **第四批(模板拆分,依赖前三批产出作为 props 来源)**:`ContentViewerControls.vue` →
   `ContentViewerInfoPanel.vue` → `ContentViewerDialogs.vue` →(可选)`FaceOverlay.vue`。
5. **不建议本轮做**:媒体舞台拆分(2.2 节已论证收益低风险高,留待红线裁决更新后再评估)。

## 5. 验证策略

- **静态**:`vue-tsc --noEmit`(strict 模式,重点看新 composable 的参数类型与 `defineExpose` 返回
  类型是否收窄)、`eslint`(含 `vue/no-unused-properties` 类规则,确认 props/emit 声明完整)。
- **单元/特征测试**:
  - 现有 `useMediaDetail.spec.ts`、`useViewerColorSource.spec.ts`、`useViewerImageSource.spec.ts`
    不应受影响,拆分后需重跑确认零回归。
  - 按项目规则"改动未覆盖的关键行为前先补特征测试":`useContentViewerFaces` 的 `faceToken` 并发
    丢弃判定、`useContentViewerZoomRotation` 的 `pendingInitialRotation` 一次性复原逻辑,建议在
    拆分**之前**各补一个特征测试锁定当前行为,拆完再跑同一测试验证零回归。
  - 新增 `useContentViewerContextMenu`/`useContentViewerQuickActions` 可选配轻量 spec(纯函数式
    逻辑,mock store 即可),按项目规则"低风险改动只需聚焦回归检查",非强制。
- **GUI 手测点**(标注为"未自动化",需人工过一遍):
  1. 方向键/滚轮连续快速翻页:人脸框不残留上一张错位,缩放比例/旋转角一次性复原不抖动。
  2. 视频悬停 scrub、poster 首帧显示与自愈重生成逻辑与拆分前观感一致。
  3. ICC 渲染色域切换(sRGB/P3/自定义)视觉结果不变。
  4. 底部工具条全部按钮(缩放/旋转/Live/编辑/OCR/增强/色域菜单/人脸开关/收藏/资源管理器/信息/关闭)
     功能与可见性条件(移动端隐藏项等)不变。
  5. 信息面板停靠开合动画与图片区 padding 挤压逐帧同步不脱节。
  6. 键盘快捷键(Esc 双层语义、方向键翻页、编辑态裁剪框微调、空格防双触发)行为不变。
  7. 右键菜单 + 移动/复制对话框流程不变。
  8. 沉浸模式浮动退出按钮 + 底部工具条显隐两个 localStorage 偏好在重启后仍生效。

## 附注:CSS 外置(D-451,2026-07-25 补录)
- 本组件 `<style scoped>` 可整块外置为同目录 `ContentViewer.styles.css`,SFC 留 `<style scoped src="./ContentViewer.styles.css"></style>`;外置文件仍编译为宿主组件 style 块,scope id 归属不变,`:deep()` 穿透语义不变。
- 前提(2026-07-25 核实):全仓 .vue 样式零 `v-bind()`;本文件为单一 `<style scoped>` 块。
- 定位:可选先行批、全场风险最低的行数削减刀;不替代 script 拆分主刀。
- 施工顺序:首刀拿最小文件实测 Vite 构建链 + HMR,通过后铺开。

## 顺手发现

无。
