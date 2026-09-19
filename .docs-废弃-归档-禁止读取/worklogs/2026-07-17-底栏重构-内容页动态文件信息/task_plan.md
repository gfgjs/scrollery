---
status: 快照
type: working-memory
line: 底栏重构-内容页动态文件信息
created: 2026-07-17
---

# 任务计划:底栏重构——内容页动态文件信息

## 目标
进入内容页(`/view/:id` 查看器、`/doc/:id` 文档阅读器、`/audio/:id` 音频页)后,底部状态栏(AppStatusBar)左区动态切换为**当前文件信息**(星级、收藏状态、颜色标签等);离开内容页恢复现有画廊统计显示。

## 当前阶段
阶段 5:验证收尾(自动门全绿,余真机 GUI 手工验收)

## 阶段

### 阶段 1:摸底与设计
- [x] 定位底栏组件与挂载点(AppStatusBar.vue,App.vue:75,AppShell statusbar slot)
- [x] 确认数据字段(MediaItem.rating / isFavorited / colorLabel)与 store 动作(mediaStore.toggleFavorite/setRating/setColorLabel)
- [x] 「当前项」数据源=viewerStore(activeViewer 全局单源,三页统一 populate;缺三标量字段)
- [x] 用户拍板:可交互 + 全套信息 + Priority+ 自动折叠(见决策表)
- [x] doc 页 detail=MediaDetail(全字段);audio 页 AudioDetail 缺三标量→补一次 get_media_detail 调用
- [x] 细则定稿:ActiveViewer 加 fileInfo 嵌套对象;内容页文件信息接管左区(替换画廊统计/进度链);折叠序=格式→大小→尺寸时长→文件名,交互三件永不折
- **状态:** complete

### 阶段 2:数据桥(已收敛为 viewerStore 扩展)
- [x] ActiveViewer 增 fileInfo(ViewerFileInfo 嵌套对象,toViewerFileInfo 单点投影零值归一),三页 populate/patch 喂入
- [x] applyFieldPatch(id 守卫,不走 token)承接 itemPatchSignal 桥;音频页 get_media_detail 补标量(id 快照防换歌竞态,两种到达顺序都覆盖)
- **状态:** complete

### 阶段 3:底栏 UI 施工
- [x] StatusBarFileInfo.vue 接入 AppStatusBar 左区(hasActiveViewer 时替换画廊统计+进度链+视口缩略图段)
- [x] Priority+ 折叠:useToolbarOverflow(段序=文件名>尺寸>时长>大小>格式,从尾折)+ ⋯ UiPopover 标签值行;deferWhile 防弹层横跳
- [x] 交互三件永不折;沉浸态隐藏由 AppShell 既有机制天然覆盖(组件恒挂载,信号桥不断)
- **状态:** complete

### 阶段 4:交互接线
- [x] StarRating/心形钮/单色块→ColorLabelPicker 弹层,接 mediaStore 三动作;**不本地回写**,统一 itemPatchSignal 桥回灌(防双写漂移)
- [x] aria-label/haspopup/expanded 齐;原生 button 键盘可达
- **状态:** complete

### 阶段 5:验证收尾
- [x] vitest 定向 7 文件 47/47 绿(viewerStore 8 含新 applyFieldPatch 3、helpers 6、命令册 4 档、useToolbarOverflow 12)
- [x] vue-tsc 全量绿(两轮;中途红是并行 minimap 会话撕裂读,非本线)+ eslint 15 文件绿
- [ ] ⏸ 真机 GUI 手工验收:①/view /doc /audio 三页底栏出信息与微操②点星/心/色即改且与查看器信息面板互同步③窄窗从尾折进「更多」④沉浸态贴底唤出仍可操作⑤docked 选区在内容页不冲突
- **状态:** in_progress(自动门全绿,GUI 待真机)

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 内容页范围先按 /view + /doc + /audio 三类规划,施工顺序查看器优先 | 查看器数据源现成(detail ref),doc/audio 数据源待查,风险后置 | |
| 底栏文件信息**可交互**(点星改评分/点心切收藏/点色块改标签)(用户拍板 2026-07-17) | mediaStore 动作现成;常驻微操入口,免开查看器信息面板 | |
| 信息集合=星级/收藏/颜色标签+文件名/尺寸·时长/大小/格式,**Priority+ 自动折叠**,次要先藏(用户拍板 2026-07-17) | 28px 横向空间有限;复用 useToolbarOverflow 引擎与 AppToolbar 同契约 | |
| 数据桥=扩展 viewerStore.ActiveViewer,不另建信号 | 已是三页统一单源+token 时序防御;另建即双源漂移 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
