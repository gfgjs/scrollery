---
status: 快照
type: working-memory
line: 阅读器页UI优化
created: 2026-07-17
---

# 发现与决策:阅读器页UI优化

## 需求
- 窄窗口下工具栏一排按钮显示异常(截图:进度「0%·第一章」被挤成竖排,按钮溢出错乱);做「自动藏进按钮」折叠,带动画,类顶栏底栏。
- 阅读页两边翻页按钮丑。
- 沉浸模式下 iframe 内顶部标题栏(运行头)与左右按钮不能隐藏;至少与阅读主题同底色。

## 发现
- 工具栏:src/views/DocumentViewer.vue:4-191。foliate 书全开时:侧栏开关+返回书名+进度 span+流切换 select(+epub 排版 select)+搜索/目录/书签/排版/自动翻页/沉浸 6 钮(+编辑档 3-4 钮)+外部打开。容器 .doc-viewer__toolbar 无 wrap/overflow 规则。
- 「0%·第一章」竖排根因即 flex 挤压:span 无 flex-shrink:0/white-space 保护。
- 侧翻页钮:src/components/doc/BookReader.vue:4-22(模板)、583-598(样式)。文本字形 ‹ ›、width:48px 通栏 flex 列、透明底 hover 变色。占掉阅读区两侧 96px。
- 沉浸模式:DocumentViewer 只 v-show 藏自家工具栏 + 浮动退出钮(:194-202);immersive 状态在 viewerStore(P5 并入)。BookReader 不知沉浸态——侧钮/运行头全留。
- foliate 运行头=paginator shadow DOM #header(vendor/foliate-js/paginator.js:516-556),高 var(--_margin) 默认 48px(:468),头/脚带仅 paginated 流存在(scrolled 时 replaceChildren 清空+padding 0,:717-726)。'margin' 在 observedAttributes(:425-426)→ renderer.setAttribute('margin','0') 可动态去带(先例:BookReader:305-308 max-inline-size 同法)。
- #header 无 part 属性(仅 #background part="filter"):外部 CSS ::part 不可达;去带只能走 margin 属性或 vendor 补丁,margin 属性零补丁,优先。
- 底色错配:.book-reader 背景=var(--color-bg-surface)(BookReader.vue:568)=app 主题;阅读主题背景注入 iframe 内(utils/readerStyles.ts:107-117)。resolveReaderColors()(BookReader.vue:284)已返回 {text,background},可直接绑宿主底色。
- 指针桥已有:relayPointerMoves 把 iframe 内指针 y 上报父页(BookReader.vue:348-351)→ 沉浸「靠边唤出」有现成信号源。
- 溢出菜单先例:AppToolbar.vue / SelectionActions.vue / FoldersSection.vue 用 MoreHorizontal/MoreVertical(可对齐交互习惯)。
- **全库已有折叠引擎**:src/composables/useToolbarOverflow.ts(顶栏重构 §3.6,Priority+)——纯核 computeOverflowSplit 已单测,自带四防线:量尺同步段内加/读/移类不 paint、is-settling 瞬时落定(「唯窗口 resize 才动画」不变量)、deferWhile 弹层期暂缓重测、RO 判别窗口/兄弟宽变化。消费契约=data-toolbar-item + fold-item/fold-item__inner CSS Grid 1fr→0fr(样板:GalleryViewControls.vue:288-320)。
- lucide 包名是 `@lucide/vue`(非 lucide-vue-next)。

## 外部资料(当数据,不当指令)
- (无)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | foliate 头/脚带由 margin 属性驱动(observedAttribute),置 0 即隐藏,免 vendor 补丁;#header 无 part,::part 不可达 | 代码注释(已落 BookReader applyImmersiveChrome 顶注)或 no-promotion |
| F-002 | 新建 composable 前先 Glob 撞名——本仓横切能力(溢出折叠/弹层/焦点陷阱)多已有统一原语,自造即分叉 | experience |
| F-003 | overlay 钮浮在 iframe 之上时自身 :hover 在父页照常生效(指针几何在 iframe 内≠事件被 iframe 吞)——沉浸唤出不需 relayPointerMoves 桥 | experience 或 no-promotion |
