---
status: 快照
type: working-memory
line: 阅读器页UI优化
created: 2026-07-17
---

# 任务计划:阅读器页UI优化

## 目标
阅读器页(DocumentViewer/BookReader)三处 UI 缺陷修复:窄窗工具栏溢出折叠、侧翻页按钮重造型、沉浸模式残留 chrome(运行头/侧按钮)隐藏与主题同底。

## 当前阶段
阶段 5:验证与收尾(代码全落地,本地门全绿;GUI ⏸真机)

## 阶段

### 阶段 1:摸底
- [x] 工具栏按钮清单与溢出行为(DocumentViewer.vue:4-191,≈13 按钮+2-3 select+进度 span,flex 无换行/溢出处理)
- [x] 侧按钮现状(BookReader.vue:4-22 + :583-598,48px 通栏列、文本字形 ‹ ›)
- [x] 沉浸残留源:foliate paginator shadow DOM #header 运行头(高=--_margin 默认 48px,margin 是 observedAttribute);.book-reader 底色=app surface 非阅读主题色
- **状态:** complete

### 阶段 2:A 窄窗工具栏溢出折叠
- [x] 复用既有 useToolbarOverflow 引擎(Priority+,AppToolbar 同款)——非自造;fold-item CSS 契约照搬 GalleryViewControls
- [x] 全部控件(3 select + 11 钮)包 fold-item 进折叠容器,⋯ 钮+UiPopover 菜单只渲染已折叠项
- [x] 进度文本防竖排挤压(nowrap+ellipsis+max-width:28vw)
- **状态:** complete(GUI ⏸)

### 阶段 3:B 侧翻页按钮重造型
- [x] 通栏 48px 列改 overlay 浮动圆钮(ChevronLeft/Right,垂直居中,opacity 0.45→hover 1)
- [x] 阅读面回收 96px 宽度(absolute 脱流)
- **状态:** complete(GUI ⏸)

### 阶段 4:C 沉浸残留 chrome 治理
- [x] applyImmersiveChrome:margin '0px'/'48px' 切换,remount 进入也先于首渲施加
- [x] 沉浸时侧钮 opacity 0,自身 :hover/:focus-visible 唤出(钮浮 iframe 上,不需指针桥)
- [x] hostBg=resolveReaderColors().colors.background 绑宿主 :style(挂载即着色,免闪 app 色)
- **状态:** complete(GUI ⏸)

### 阶段 5:验证与收尾
- [x] vue-tsc 0 错;ESLint 4 改动文件 0 报;全量 vitest 88 文件 1184 测试全绿(含 localeIntegrity)
- [ ] GUI 手测(⏸真机,步骤清单见 progress.md,不自动化)
- **状态:** in_progress(仅余真机)

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 窄窗折叠取「溢出菜单钮」而非 flex-wrap/横滚 | 用户点名「自动藏进按钮,带动画」;wrap 加高工具栏、横滚无可见性 | |
| 折叠引擎复用 useToolbarOverflow 而非新写 | 顶栏同引擎已带闪动四防线(量尺同步段/settled/deferWhile);自造必重踩 | |
| 折叠优先序=现 DOM 序(尾部先折),不重排 | 宽窗视觉零变化;外部打开/替换等低频项恰在尾部,先折合理 | |
| ⋯ 钮放折叠容器内 | 引擎 budget 已预留其宽,容器内恰好单算;容器外会双计 | |
| 侧钮改 overlay 浮钮而非只调样式 | 通栏列占 2×48px 阅读宽;overlay 同时解决「丑」与占宽,且浮 iframe 上自身 :hover 即唤出通道 | |
| 沉浸=隐藏运行头(margin='0px')+主题同底双做 | 用户「至少同底色」,隐藏是首选,同底兜底其余残留;margin 还原用显式 '48px' 不用 removeAttribute | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 自写 useToolbarOverflow 撞名(Write 报 File has not been read) | 按摸底结论直接新建同名 composable | 全库已有同名成熟引擎(顶栏重构 §3.6);改为复用。教训:新建 composable 前先 Glob 撞名 |
