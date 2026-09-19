# 发现与决策：前端 UI/UX 深度审计

## 需求
- 深度 Review 当前前端样式，包括 UI、UX、配色与操作逻辑。
- 先交付重构方案供用户确认，确认前不进入实现。
- 必要时向用户索取特定页面截图。

## 研究发现
- 初始工作树无已有改动；本轮新增文件只有 3 个审计规划文件。
- 既有记忆仅提供需复核的缩略图交互线索：`thumb_status` 状态机、`MediaThumb.vue`、`useRequestQueue.ts` 30 秒 watchdog，以及可能重复的 `useThumbnail.ts`；均不能替代当前源码检查。
- 前端是 Vue 3 + Pinia + Vue Router + Vite 的 Tauri 桌面应用；没有现成 UI framework，界面体系主要由本地 Vue SFC 与 CSS 自建。
- 页面覆盖图库、人物、收藏、文档阅读、音频播放、设置、插件商店和实验性图库；重构不能只处理单一 Gallery 页面。
- 已存在 6 套主题 CSS、全局 variables/reset/animations，以及 `check:contrast` 脚本，说明项目已有 Design Token 和对比度基础，方案应优先收敛与补全，而不是推倒重来。
- 多个 Vue 文件体积较大（包括超过 50 KB 与 100 KB 的 SFC），样式、结构和交互逻辑可能高度耦合，是后续可维护性审计重点。
- 应用 shell 使用自绘标题栏、侧栏、上下文工具栏、状态栏和路由内容区；查看器还包含沉浸模式下的 hover/F10 标题栏显隐。这套桌面交互明显高于普通网页复杂度，方案必须保留窗口拖拽、窗口三键和键盘逃生路径。
- 路由已为 Gallery、Collection、Person、Plugin、Document、Audio、Viewer 等内容提供独立深链，基础信息架构不是单页 overlay 堆叠；但设置页仍由全局 `v-if` overlay 控制，需要检查是否造成导航状态、返回行为与 URL 不一致。
- 主题系统有注册表、明暗槽位、system 模式、首帧防 FOUC 和主题契约测试；设计资产成熟度较高。当前更可能的问题是 token 使用一致性、状态对比度和控件体验，而非“没有主题系统”。
- ThemePicker 的行为会在用户点击另一明暗组主题时自动切换 appearance，源码已有专门注释修复“选中却不生效”的反馈，说明操作逻辑应以即时可见反馈为原则延续。
- 规则扫描发现至少 20 处 `transition: all`，分布在 Toolbar、Dialog、Settings、MediaThumb、SelectionToolbar 等核心控件；这既违反当前 motion 规范，也会把布局属性卷入补间，应统一改为显式属性。
- 发现多处裸 `span/div @click`（CollectionsView 的编辑/删除/新建，Folder tree 展开箭头等），其中部分缺少键盘等价操作和语义，应改为真实 `button` 或由父级完整承担操作。
- 全局 reset 已提供 `:focus-visible` 基础，但若干局部输入仍以 `:focus { outline:none }` 实现；需逐项确认是否有等价 border/ring，避免键盘焦点弱化。
- 动画大多集中在全局 animations 与局部 spinner/dialog；已有部分 `prefers-reduced-motion` 处理，但多份 dialog/spinner 的重复 keyframes 仍需统一核查。
- 68 个 Vue 文件中 67 个含 scoped style，说明样式高度分散；前 10 个 SFC 均为 850–2289 行，`MediaGrid.vue` 达 2289 行。重构若只改颜色会继续扩大耦合，应同时提取 primitives 与状态模型。
- 硬编码颜色主要集中在 Canvas/Grid/Thumb/Viewer，部分是媒体遮罩和绘制 API 的合理需要，但数量大且绕过主题 token；必须区分“内容渲染常量”与“UI 语义色”后治理。
- URL 路由覆盖内容页，但 Gallery 的 filter/view/group/search 是否同步 URL 尚未从扫描中看到；这是后续操作可恢复性与深链能力的重点。
- 仓库已有两份现行设计：2026-07-06 多主题/UI 优化与 2026-07-08 顶栏重构；本次不是首轮设计，而应作为“现状复审 + 第二阶段整合方案”，不能重复已完成的主题基建。
- 近期提交显示 2026-07-10 已集中修过 toolbar、a11y、画廊 Canvas、时间轴和主题对比度；当前扫描仍出现 `transition: all` 等问题，可能是后续改动回归或之前机械审查覆盖不完整，需要以当前 checkout 为准。
- 旧 UI 设计确定了关键视觉红线：照片画布保持低饱和中性，主题风格只作用于 chrome；这与照片管理器的色彩判断需求一致，建议保留为新方案的不可变原则。
- 现行顶栏设计仍列有 GUI 真机待验项，尤其宽窗单栏、overflow 折叠观感、弹层与查看器命令；本次必须通过真实运行补齐，不能仅引用文档声称已交付。
- 2026-07-06 方案最初统计 55 个 Vue/约 20,700 行，当前为 68 个 Vue 且大型 SFC 明显增多；功能扩张后“组件原语/样式治理”已从优化项升级为架构债务。
- 当前质量门禁全部通过：typecheck 0、ESLint 0、47 个测试文件 615 项测试通过、6 套主题对比度硬门槛通过、production build 成功。现有静态门禁没有覆盖 `transition: all`、非语义 clickable 元素等 Web Interface Guidelines 规则，说明需补专门 UI lint/contract。
- 构建产物提示主入口约 508 KB、C++ 语法资源约 638 KB，且 `scanStore` 同时静态与动态 import 导致无法分 chunk；这不直接决定 UX 方案，但会影响启动/按需加载体验，建议纳入后续性能预算而非本轮视觉先改。
- 浏览器访问 `http://127.0.0.1:1420/#/` 时主题首帧正确落为 `porcelain/light`，但 `WindowChrome.vue` setup 直接调用 Tauri `getCurrentWindow()`，普通浏览器环境因缺少 Tauri metadata/invoke 而整棵应用挂载失败，只剩空白页。真实视觉与交互必须在 Tauri 桌面窗口验证。
- 上述 browser-mode 空白意味着未来若要做自动化视觉回归，应引入 window/IPC adapter 或 mock harness；否则 Storybook/Playwright/截图测试很难覆盖 shell 级 UI。
- Settings 目前是 `position:fixed; inset:0; z-index:1000` 的全屏 overlay，由全局 boolean 打开；模板没有 `role=dialog`/`aria-modal`，也未看到 focus trap 或 URL/history 表达。它视觉上像独立页面、状态上却像 modal，导航模型不一致，应在方案中二选一：正式 `/settings` 路由页（推荐）或完整 modal 语义。
- Settings 把大量异构配置纵向堆在最大宽 860px 的单列 collapsible cards 中，没有分区导航或设置搜索；随着 AI、网络存储、模型库等功能加入，定位成本会持续上升。
- `SettingsView.vue:66-72` 的缓存目录以 clickable `div` 打开资源管理器，缺少 button/link 语义与键盘路径，是具体 accessibility 缺口。
- AppToolbar 的 search input 与 scope select 没有显式 label/aria-label/name；mixed search 候选是 clickable `div` 且没有 listbox/option 语义，虽然输入框处理上下键与 Enter，但读屏无法理解当前候选状态。
- Toolbar overflow 弹层使用 fixed 220/252px，源码明确承认 252px 是按中文标签定、英文需要调大；这属于已知 i18n 布局缺陷，方案应改为内容驱动宽度 + viewport clamp，而不是 locale 魔数分支。
- Toolbar 的 filter/view popover 只有 backdrop 点击与 Esc 关闭，没有从当前片段看到 menu/dialog 语义、打开后聚焦和关闭后还焦；需要按 disclosure/menu pattern 重构。
- 搜索框 focus 时宽度从 200px 动画到 280px，在单行自绘标题栏与 overflow 算法中会动态挤占空间，可能触发 controls 折叠/回弹；应验证并考虑固定槽位或 overlay 扩展。
- Collections 与 Persons 的主卡片均用 clickable `div`，卡片内编辑/删除/选择/隐藏又使用 clickable `span`；键盘、读屏和触控命中区都不完整。源码为避免 `input-in-button` 选择了 div，但更合适的结构是卡片容器 + 独立可聚焦主链接/按钮 + action buttons，而非牺牲语义。
- Collections 打开时先把对象写入 `viewStore.setActiveCollection(c)` 再 `router.push('/')`，URL 不含 collection id；刷新、分享、后退恢复和多窗口深链均不可靠。应升级为 `/collections/:id` 或 query state。
- Persons 的隐藏/误检操作入口仅 hover 可见且是 13px 图标；隐藏后 `visiblePersons` 直接过滤，源码注释明确“暂不提供显示已隐藏”，这会造成不可发现、难恢复的管理操作，需增加管理入口与 undo/恢复路径。
- 缩略图 `<img>` 已给显式 width/height 与 lazy loading，但 `MediaThumb.vue`、`MediaThumbCompact.vue` 均没有 `alt`；若图片在 DOM 卡片中承担内容，应提供文件名 alt，若可访问名由卡片承担则明确 `alt=""` 并 `aria-hidden`，当前属于规范缺口。
- 极密网格为性能刻意移除 hover/评分/收藏等操作是合理 tradeoff，但应在切入 compact 模式时明确告诉用户“进入密集浏览，仅保留选择/可用态”，避免操作能力无提示消失。
- SelectionToolbar 是悬浮可拖动胶囊，CSS 中多处 `transition: all`，tooltip 仅 hover 生效；键盘 focus 时看不到 tooltip 文案，drag handle 也不是可聚焦控件。方案应把常用批量动作提供可见标签或 focus tooltip，并让位置拖动成为次要增强而非访问前提。
- `filterStore` 的媒体类型、收藏、Live、评分、颜色和日期范围全是内存状态，router 不编码这些条件；刷新或从查看器返回以外的导航无法可靠复现当前视图。Gallery 的筛选、分组、排序、布局宜采用 query params，纯视觉偏好（行高等）继续本地持久化。
- Filter chips 以视觉 `.active` 表达状态，但普通 toggle buttons 没有 `aria-pressed`；日期 popover 也没有 dialog/group 语义与焦点管理。视觉重构应同步定义 ToggleChip、Popover、SegmentedControl 的统一 a11y contract。
- 路由表没有 `/settings`、`/collections/:id`、`/persons/:id`；当前内容寻址只完成了 folder/favorites/viewer/doc/audio，信息架构仍处于一半 route、一半 store overlay 的混合状态。
- DynamicSettingControl 的通用 checkbox/select/number 控件本身没有 `id`/`name`/`aria-label`，SettingRow 的视觉 label 也未从已读片段证明与表单控件程序性关联。注册式设置系统应把 accessible name 一并纳入 `SettingSpec` contract。
- NetworkStorage 表单使用包裹式 `<label>`，基础可点击语义较好；但异步测试/保存结果只是普通 `<span>`，未见 `aria-live`，busy 状态也只 disable 按钮而缺少明确进度文案 contract。
- ToastContainer 已正确使用 `role=status aria-live=polite` 和真实关闭按钮，是可复用的正确基线；不过 error/success/warning/info 全部使用同一个 polite 容器，严重错误是否需要 `role=alert` 应按消息类型区分。
- 全仓几乎没有针对窗口宽度的 CSS `@media`/container query，只有 reduced-motion 分支；当前依赖 toolbar overflow 与 auto-fill grid 局部自适应。对于可自由 resize 的桌面应用，这不足以定义窄窗下 Settings、Viewer、Audio、Plugin、Dialog 的降级策略。
- 多个常用控件命中区仅 20–34px（搜索模式 24×22、人物卡 action 20×20、Dialog close 28×28 等）；桌面 pointer 可用但容错低，方案应设统一 32px compact / 36px default / 44px touch 三档，而非各组件自行定值。
- ContentViewer 仍保留底部 `detail-controls`，同时顶栏已有 ContextualToolbar；源码注释写明待 P5 收敛。这会造成相同命令在两个层级重复、文件名与导航信息分裂，是操作逻辑重构的核心收尾项。
- Viewer 的图片 `<img>` 同样没有 alt；查看器本身以 wheel/mousedown/click 承载缩放与拖动，但 `.detail-viewer` 不是可聚焦交互区，键盘帮助/快捷键提示的可发现性需由统一 command surface 提供。
- DocumentViewer 顶栏横向塞入返回、页码、3 类 select、搜索/目录/书签/设置/自动滚动/沉浸/编辑/版本/校对/替换/外部打开，窄窗没有已见的 overflow 策略，属于明显的 responsive 风险。
- SemanticSearchPanel 的 threshold range visually 有 label，但 label 没有包裹 input 或 `for`; 查询/threshold 也不进 URL。AI searching 使用 spinner，但应补 `aria-live`/busy，保留可取消入口。
- ContextualToolbar 已有 `role=toolbar`、可访问名、命令级 `aria-label/aria-pressed` 和 keybinding 同源，这是正确架构；但没有 roving tabindex/ArrowLeft/ArrowRight/Home/End，所有图标按钮都是独立 Tab stop，未完整实现 APG Toolbar Pattern。
- 命令册与源码注释同时承认 Viewer/Audio/Reader 顶栏命令和局部工具栏“双入口并存”。重构应按任务分层：窗口/导航/跨媒体通用命令放标题栏；连续媒体操控（播放、缩放、翻页）保留内容区；同一命令只保留一个主入口，其他入口通过 shortcut/command palette 补充。
- `ContextualToolbar.vue` 写着 overflow “待命令增多接入”；图像查看器已有 8 个按钮，Reader/Audio 还会继续增长。标题栏空间与 Gallery toolbar 共享，必须在方案中提供统一 overflow，而非每个 toolbar 自造折叠。
- 全局 foundation token 使用广泛（accent/border/text/spacing/motion），但 component-level primitives 仍主要靠全局 class + 每个 SFC 自定义；Dialog、Popover、Toolbar、IconButton、Field、Card 的状态 contract 没有单一实现，导致修一次 accessibility 需要扫几十个文件。
- 全局 `.input/.select/.input-number` 在 focus 时 `outline:none` 只换 border 色；虽然 reset 有 `:focus-visible`，局部规则与全局 outline 的 cascade 需实际验证。更稳妥的 primitive 应明确 `:focus-visible` ring，不依赖跨文件顺序。
- 全局 Dialog 基座在 2026-07-10 才补齐，且多个 Dialog 仍保留 scoped 重复实现/重复 keyframes。方案应收敛为一个 Modal primitive（overlay、focus trap、Escape、return focus、scroll lock、reduced motion），避免样式同名但行为漂移。
- ContextualToolbar 对 `/doc`/`/audio` 的“未 populate 则抑制”注释已过时：DocumentViewer 与 AudioPlayer 当前都会 populate viewerStore，因此顶栏命令会与各自局部工具栏并存。应把旧设计注释和实际命令分层一次性收敛。
- Sidebar 的 active 条件混用 route 与 viewStore；`LibrarySection.vue:8-10` 的智能相册 active 判断没有排除 `activePersonId`。进入某人物照片后 `activeSmartAlbum` 被设回 `all`，会导致“全部照片”和“人物”同时高亮，破坏位置感。
- Sidebar 的 4 个 accordion 区块（Library/Tools/Folders/Management）采用复杂双向 sticky stack，工程上有充分注释与键盘化；但视觉复杂度是否高于价值必须看截图。方案暂不推翻，只建议为高频 Library/Folders 保持常驻、低频 Tools/Management 默认折叠。
- Settings 至少有 Escape 关闭，但仍无焦点陷阱、关闭后还焦与 route/history；这进一步支持将其改为正式路由页，避免强行补成全屏 modal。
- 现有设置 registry 已把 section/control/min/max/options 集中管理，是可继续扩展的正确地基；下一步应补 `keywords`、`advanced`、`requiresRestart`、`accessibleName`、`validation` 等元数据，支撑设置搜索与一致反馈。
- i18n 仍有可见硬编码 `ORIG/THUMB/LIVE`；WebDAV 属品牌/协议名可保留并 `translate=no`，状态 badge 则应走 i18n 或图标+可访问名，避免英文界面/中文界面混排策略不一致。

## 技术决策
| 决策 | 理由 |
|------|------|
| 发现必须关联源码或真实页面证据 | 避免只给主观设计评价 |
| 问题按 P0/P1/P2 和影响范围排序 | 便于形成可实施的重构顺序 |

## 遇到的问题
| 问题 | 解决方案 |
|------|---------|
| production build 存在 chunk size 与混合 import 警告 | 记录为性能基线；方案阶段不改代码，实施时独立治理 |

## 资源
- Web Interface Guidelines：已获取 2026-07-11 可访问的最新版，审计范围包括 accessibility、focus、forms、motion、content、performance、navigation/state、touch、theming 与 i18n。
- 仓库历史分析线索：缩略图请求、缓存与渲染链路；当前轮必须重新核验。

## 视觉/浏览器发现
- 1280×720 浏览器首帧：页面为 Porcelain 背景但没有任何可见控件；console 显示 `WindowChrome.vue` 调用 Tauri window API 失败，DOM 高度为 0。
- 该结果不能代表 Tauri 产品界面；需要用户提供真实桌面窗口截图，或后续实现可浏览器运行的 UI harness 后再做自动视觉回归。
- 用户已提供 4 张真实 Tauri 截图并批准全部推荐项。截图尺寸分别约为 2048×1272、1313×1240、2048×1365、1985×1240；未提供系统缩放比例，故只锁定结构结论，不反推逻辑像素。
- Gallery 宽窗：标题、数量、筛选 chips、视图 controls、实验入口、搜索、undo/redo、全屏与窗口三键全部挤在一条标题栏，信息层级弱；路径 `C:/More/0/00` 与紫色“画布”标记出现在媒体内容上沿，像调试信息而非产品 breadcrumb。
- Gallery 右侧时间轴在截图中窄到只剩彩色密度带与被裁切的小标签，主内容又出现右侧悬浮上下箭头，导航控件语义重叠；需要把时间轴作为可折叠 inspector/rail 重新定义最小宽度与空态。
- Gallery 窄窗：筛选 popover 从标题栏左侧展开并大面积覆盖 Sidebar/内容，视觉上脱离 trigger；popover 仍按固定网格陈列，缺少明确标题、清除/完成层级和边缘间距。
- Settings：全屏 overlay 中央单列卡片在宽屏留下大面积空白，卡片内部密度尚可，但没有分区导航/搜索；“全部折叠”和关闭按钮强化了 modal 感，与页面级内容体量冲突。ThemePicker 当前显示 Porcelain/Ink 选中，用户已批准改为 Moonlight/Ink 默认。
- Viewer：截图直接确认 ContextualToolbar 与底部控制条严重重复（上一项/下一项、zoom、fit、rotate、info、immersive 等）；底部悬浮控制条更贴近内容操作，应保留连续操控，顶栏只留导航级命令。
- Viewer 仍显示完整 Sidebar，占据约 1/5 宽度；考虑媒体查看的内容优先原则，实施时改为 viewer route 默认折叠 Sidebar，并提供显式恢复，不强制永久隐藏。
- 截图中的紫色“画布”标签已定位到 `MediaGrid.vue` 的 Gallery/Timeline DOM↔Canvas 原型切换按钮；当前对所有用户可见，属于开发调研控件泄漏到产品 UI。实施中将改为显式 debug flag 才显示。
- browser harness 的最小技术边界已确认：`WindowChrome.vue` 与 `uiStore.ts` 直接调用 `getCurrentWindow`，`ipc.ts` 直接 invoke，`App.vue`/`scanStore.ts` 直接 listen。需要统一 runtime adapter，而非只在一个组件 try/catch。
- 首屏 Gallery harness 至少需要 mock `GET_STARTUP_CONFIG`、`LIST_SCAN_ROOTS`、`GET_STATS`、`COMPUTE_LAYOUT`、`GET_LAYOUT_ROWS_BY_Y`/`GET_BUCKET_ROWS`、`CLOSE_SPLASHSCREEN` 与少量 AI/face 状态命令；仅 mock window API 仍会出现空态或错误噪声。
- `platform.ts` 已在非 Tauri 环境降级 Windows，`useTauriListen` 也会吞 listen 失败；可复用这种“生产真机不变、dev harness 降级”的边界，不需要全库替换所有 Tauri import。
- `useThumbLoader.buildThumbUrl` 对 status 1/3 无条件调用 `convertFileSrc`；harness 需要让以 `/ui-harness/` 开头的受控静态资源直接返回 URL，避免模拟 Tauri asset protocol。
- Layout fixture contract 已确认：`LayoutSummary(totalRows,totalHeight,layoutVersion,totalItems,separators,monthBuckets)` + absolute-positioned `LayoutRow`/`LayoutRowItem` 即可驱动现有虚拟网格，无需为 harness 新建一套 Gallery 组件。
- `uiStore` 直接 window API 只用于 mac theme 与 fullscreen；browser harness 可通过单一 `appWindow` adapter 提供 no-op/Fullscreen DOM fallback，不影响 Windows/mac 真机实现。
- MediaGrid 的 Canvas/DOM 切换按钮本身是原型调研控件，截图中两个“画布”药丸分别来自 timeline 和 gallery render mode；首批修复应一起 dev-gate，不能只藏一处。

## 用户最终裁决（2026-07-11）
- 接受 Moonlight（亮）+ Ink（暗）作为默认主题组合。
- 接受 Settings 改为正式 route 页面。
- 接受按命令 ownership 删除重复入口。
- 接受首阶段补 browser UI harness。
- accessibility 暂不作为必须支持项：保留现有能力、避免明显回退，但不作为本轮阻断交付的硬门禁。

## S0 实施边界补充
- `CommandGroup` 已有 `view` 语义，Viewer 顶栏重复的 zoom/rotate/info 命令可直接改组进入 overflow；底部控制条继续作为连续查看操作的唯一主控制面。
- Gallery 截图中的两个“画布”胶囊不是业务状态，而是 `MediaGrid.vue` 公开暴露的 DOM/Canvas 对照实验入口；产品态应隐藏，仅在 DEV 且显式开启 `scrollery.debug.renderMode=1` 时显示。
- browser UI harness 必须经过现有 `App`、`AppSidebar`、`MediaGrid` 和 stores，mock 边界集中在 `invokeIpc`/window/event adapter；否则只会得到与真实桌面 UI 漂移的第二套演示页面。
- 首屏 fixture 至少覆盖启动配置、扫描根、统计、目录树、布局摘要/行、视图 id、缩略图缓存目录以及 AI/face/derivation 空闲状态。
- Gallery browser 场景已在 1440×900 实际挂载成功：18 个真实 `MediaGrid` 单元、Moonlight、0 个 render-mode debug 按钮；同时稳定复现标题栏拥挤与时间轴窄轨，具备后续视觉回归价值。
- Settings 场景首次暴露 fixture 缺口：`list_volumes` 默认返回 `null` 导致 KnownVolumes render 读取 `.length` 崩溃；已按真实数组 contract 补为空数组。该失败也证明 harness 会运行真实 Settings 子组件，而非静态假页面。
- Settings 挂载时现有 i18n 会对包含 `<strong>`/`<code>` 的消息发 HTML 警告；这不是本次 adapter 引入，列入后续内容安全/渲染治理项。
- Settings 已改为 `/settings` 正式路由并在 browser harness 中验证：页面保留 App shell/Sidebar，Gallery toolbar 隐藏，Settings 不再以 fixed overlay 覆盖整个应用。
- Viewer harness 已验证默认隐藏 Sidebar，内容区占满 1440px；点击唯一“显示侧栏”按钮后 Sidebar 恢复为 flex，Viewer 宽度收缩到 1180px，按钮状态同步切换为“隐藏侧栏”。
- Viewer 顶栏已收敛为上一项、下一项、沉浸模式 3 个 navigation 命令；底部仍残留关闭与沉浸入口，需继续按 ownership 删除重复控制。
- `AppToolbar.vue` 仍是标题/breadcrumb、全部筛选 chips、视图 controls、H-Lab 与搜索的单体组件；仅把它整体换到第二行会把搜索也降级，真正分层需要至少拆出 title/search chrome 与 page filters 两个 surface。
- Viewer 底栏的“关闭”是当前唯一可见返回入口，而“沉浸”与顶栏重复；因此不能机械地同时删除两者。应先为标题栏补明确返回命令，再移除底栏关闭与沉浸，保持操作闭环。
- `ViewerApi` 已有 `close()` 能力，最小风险做法是新增 `viewer.close` navigation command 复用该 API；不需要让 command 层直接依赖 router，也不会破坏 OS 深链的无 history 回根逻辑。
- Settings 的实际卡片天然可归并为 5 个稳定导航域：通用（general/reading）、媒体（thumbnails/video）、AI（aiModels/modelLibrary/faceModels）、存储（networkStorage/knownVolumes）、高级（debug）。各子组件已有稳定 `CollapsibleCard id`，可在不重写设置 registry 的前提下增加 group-level route/scroll/search。
- 现有 `/settings` route 可无破坏扩展为 `/settings/:section?`；导航域写进 path 后刷新与前进/后退都能复现位置，比单纯组件内 active state 更符合已批准的 route 单源方向。
- Settings 已实现 5 域左侧导航、域级搜索和 `/settings/:section?` 深链；窄于 900px 时导航降级为横向滚动，内容仍保持单列，避免原宽屏大面积空白与窄窗硬挤双栏。
- 当前真实 Settings 中破坏性动作未集中出现在已读主模板，不能凭视觉方案虚构 danger zone；本轮先把 debug 明确归入“高级”，后续应在清库/忘记卷等真实动作完成单源梳理后再建立 danger zone。
- Settings 新布局在 1440×900 harness 中无横向溢出；App Sidebar 260px 后的 1180px 内容区被分为 210px section nav + 874px 设置内容，宽屏空白显著减少且设置行仍保持可读宽度。
- Settings 分区导航交互实测把 URL 更新为 `#/settings/advanced`；搜索“缩略图”按标题与描述语料同时命中通用、媒体和高级域，说明搜索覆盖的不只是卡片标题。
- Gallery 分层后 1440px harness 中标题栏 40px、页面工具栏 48px，搜索仅存在于页面工具栏；18 个媒体项正常渲染，H-Lab/Canvas 实验按钮均为 0，页面无横向溢出。
- Viewer 去重后顶栏精确为“返回画廊/上一张/下一张/沉浸模式”，底栏精确为缩小/放大/zoom mode/旋转/收藏/文件夹/信息 7 个内容操作；标题显示当前文件名，Sidebar 默认隐藏且 Viewer 占满 1440px。
- Gallery 工具栏虽已分层，但搜索仍在 focus 时从 200px 动画扩到 280px；现在横向空间不再与窗口三键竞争，继续扩宽已无必要，应改成稳定宽槽位。
- 时间轴真实默认值仍是 32px（`configStore` 初始化与 fallback），harness fixture 已用 44px；截图中的窄密度带正是生产默认过窄，需把新装默认与 fallback 对齐到 44px，同时尊重既有持久化值。
- 机械扫描仍有 22 处 `transition: all`；accessibility 已降为非阻断，但该项同时影响动画性能与可预测性，适合后续以独立机械批次治理，不与当前结构重构混改。
- 时间轴默认值有三处必须同步：`configStore` state、`loadConfig` fallback、`MediaGrid` CSS fallback；只改其中一处会在启动/首次打开 Settings/组件直挂时发生 32↔44px 跳变。
- Gallery route 同步可直接覆盖 filterStore 的媒体类型/收藏/Live/评分/颜色/日期和 uiStore 的 group/sort/layout；但 mixed 搜索的 pending query 当前只存在于 `AppToolbar` 局部 state，不能假装已经能可靠深链，需先把搜索 draft/committed query 提升为 store 单源。
- Collections/Persons 仍明确执行“写 active store 后 push `/`”，这正是 route/store 混合寻址根因；两者详情复用现有 MediaGrid 即可，先增加 `/collections/:id`、`/persons/:id` 并让 App 按 route 水合 active state，无需新建详情页面。
- Collection/Person 卡片目前使用 clickable `div/span`，rename/delete/hide/ignore 也是 span；accessibility 虽非阻断，但这些真实操作缺少 button 的按压/focus/disabled contract，同时命中区过小，适合与 route 化同批做低风险元素替换。
- Collection/Person 详情 route 已新增并复用 MediaGrid；App 通过参数水合 active filter，Collection 深链缺实体时先加载列表并用 token 防旧请求回写，Person id 可直接同步。Gallery toolbar 同步覆盖两个详情前缀。
- Sidebar 的“全部照片”双高亮条件已补 `!activePersonId`，Collections/Persons active 改为 route prefix，位置感不再依赖残留 store state。
- `personStore.setHidden(false)` 与 `setIgnored(false)` 后端路径都已存在，Toast 也原生支持 action；因此隐藏/误检无需新增 IPC 就能提供 5 秒 undo。ignored 列表虽然被 SQL 排除，立即 undo 仍可按已知 id 恢复并 reload。
- Persons 现已提供“已隐藏”管理视图、逐项恢复，以及隐藏/误检操作的 5 秒 Toast undo；误检确认文案同步删除了“没有撤销入口”的过时描述。

## 会话续 2 补充发现（2026-07-11，模型切换接续）
- 适配层有传染性：`appEvents.ts` 等新适配模块被业务代码 import，其顶层又 import `harness/runtime.ts`，后者模块加载即读 `window`——9 个 node 环境 spec 收集崩溃。harness 入口模块的顶层副作用必须做环境守卫。
- keybinding 分发与命令分组是两个正交概念：`dispatchKeybinding` 曾按 `group: 'navigation'` 过滤，命令按 ownership 挪组会连带弄坏键盘。已改为全组查询——group 只决定按钮渲染位置，上下文安全由 `when` 谓词保证。此耦合是上批 command ownership 改造引入回归的根因。
- 上批"破坏性动作未集中出现"的判断不完全准确：settingsMap 的 debug 段本就集中了清库/重置设置/清缩略图/清日志四个真实破坏性按钮（与 logLevel/logDir 混排）。danger zone 无需虚构，只需把这四项从 debug 分离成独立分区。已落地（c98ac93）。
- 「清除缓存」按钮语义是纯前端 cache-busting 重载（源码注释明确无对应后端命令），不销毁数据；其危险样式名不副实，已降级为普通按钮。
- 存量幽灵类 bug：`.btn-danger` 只在 SettingsView scoped 内定义，DynamicSettingControl/CloseConfirmDialog 的消费从未渲染出危险视觉；`.btn-secondary` 9 处消费仅 3 处 scoped 定义。已提升为 index.css 全局变体；SettingsView 的 scoped 副本因 `.btn[data-v]` 特异性压制全局变体的 border，必须保留至 S1 UiButton 统一收敛——这正是设计 §9「组件原语缺失」的实证。
- headless Chrome `--dump-dom --virtual-time-budget` + harness `?ui-harness=settings` 可做无 Playwright 的 DOM 断言（Settings 各域是 v-show，全部卡片常驻 DOM，折叠卡也可断言）；正则断言须防命中 dump 出的 `<style>` 文本而非真实元素。

## 会话续 3 补充发现（2026-07-12，Opus 接续，S2 拆段与 S2-a）

- **搜索状态三处碎片**（Explore 子 agent 全量测绘）：`ui.searchQuery`+`ui.searchScope`（普通/文件名）、`ai.semanticQuery`+`ai.searchMode`+`ai.activeMixedQueryType`（语义/混合）、`AppToolbar.pendingMixedQuery`（混合草稿，组件局部）。执行入口两条：普通搜索写 `ui.searchQuery`→`useJustifiedLayout` 的 watch 每键即重算查询（**无防抖，每键即查**，AppToolbar 原防抖只包住冗余 emit）；语义搜索走 `ai.runSemanticSearch`（IPC + searchToken 代次守卫 + 语义模式 group/sort 覆盖 + previousGroupBy 复位）。
- **地图纠正初判**：committed query 其实已在 store（ui/ai），真正缺的不是「存储位置」而是「一个会执行查询的统一写入动作」。这解释了 design/findings 的「URL 看似可恢复、实际不查询」死链根因——没有 owner 负责「应用这个 query 并触发查询」。S2-a 门面的核心交付物因此是 `apply()`（保证执行）而非仅仅搬存储。
- **门面取舍=单一接口 ≠ 物理合并**：searchStore 是协调门面，读表面投影 ui/ai，写表面委托既有 ai action。不搬迁 aiStore 的 searchToken/group-sort 协调（高风险低收益）。单一接口已足以给 URL 一个可靠读写锚点并解决死链——这是 KISS 与低风险的正解。
- **App.vue onSearch 是死重**：`@search` emit → `onSearch(q){ ui.searchQuery = q }`，而普通模式 setter 已同步写过 ui.searchQuery，混合-文件名也已由 setNormalSearchQueryInMixedMode 写过；`@semantic-search` 从未接线。删除 emit + onSearch 无行为变化。
- **路由非对称（S2-b/c 必须调和）**：collections/persons 已经由 `App.vue` 的 route.fullPath watcher 双向路由化；但文件夹/收藏/live/recent/trash 全部停在路径 `/`、仅靠 viewStore 表达（`/favorites`、`/trash` route 存在却被侧栏 `push('/')` 旁路）。全库零 query-param 同步基础设施——S2-b 的 `router.replace({query})` 同步层 + hydration 是 greenfield。
- **两处 backend filter 投影须锁步**：`useJustifiedLayout.ts:38-92`（驱动网格 compute）与 `useViewDescriptor.ts:22-79`（SelectAll 描述同一视图，语义模式返回 null）源码已用 🔴 R1-2 标注一一对应；S2-c 新增/改视图维度必须两处同步。
- **uiStore setup 期硬编码 DOM 依赖**（既存债，本次单测踩到）：`uiStore.ts:52/55` 在 setup 顶层无条件读 `window.matchMedia(...)`，使 node 环境实例化直接崩——这正是全仓回避对 uiStore 做单测的原因，也与上批修的 harness 顶层读 window 同源（**顶层副作用无条件触碰宿主全局=把模块钉死在特定运行环境**）。单测以最小 window 桩绕过；根治属后续（把 matchMedia 读取移入惰性 init 或加 typeof 守卫）。

## 会话续 3 补充发现（S2-b Gallery filter↔URL 同步）

- **filterStore 全局态是 S2-b 同步模型的决定因素**：切换文件夹/收藏/人物**不清筛选**（既有契约，filterStore 无 per-view 存储），所以 URL 里的筛选不是「逐历史独立态」而是「全局筛选在当前路径的投影」。据此定同步方向：store→URL 连续写（含路径变时把全局筛选带到新路径 URL，否则刷新新路径会丢筛选）；URL→store 仅初次水合。若误按「逐历史态」做（Back/Forward 回填筛选），等于把全局契约偷偷改成 per-view——属未经批准的契约变更，不做。
- **双向同步两大隐蔽竞态**（已用双重守卫消解）：①**回环**——store→URL→store→URL 无限往返，用值相等守卫断（编码==当前 query 早退 / 解码==当前快照跳过）；②**初始化冲掉**——初次 readUrl 前，writeUrl 若因路径 resolve 先触发，会用默认空筛选把深链 query 冲掉，用 hydrated 门压住直到初次 readUrl 完成。二者缺一，深链就是死链或抖动。
- **filter 字段全部非持久化**（filterStore 纯内存），故加 URL 是纯增值、零 persist-vs-URL 冲突。反之 **group/sort/layout 是持久化 prefs**（uiStore 异步 get_startup_config 水合），URL 同步它们会撞上「URL 权威 vs persist 权威」+「异步水合竞态」+「missing param 是否覆盖 persist」三重问题，且 per-view vs sticky-global 是**产品契约决策**（不擅自冻结），故后置 S2-b2 待用户拍板。search q/scope/mode 后置的原因不同：restore mode 会触发 aiStore.setSearchMode 的 group/sort 副作用与语义 IPC，需单独处理。
- **isGalleryRoute 单源**：App.vue showGalleryToolbar 与 useGalleryQuerySync 曾各自内联「MediaGrid 承载路由」谓词，抽入 utils/galleryQuery 复用——正是设计所警惕的重复谓词漂移的即时清偿。
- **composable 运行时编织无 test-utils 难单测**，改真机 harness headless DOM 断言（深链 chip 恢复态）作证据；纯 codec（防御式 URL 解析=高风险面）以 19 例单测锁死。测试follow风险而非follow可测性的又一实例。

## 会话续 3 补充发现（S2-c 视图路由化测绘，重估复杂性）

- 🔴 **folder 点击是双模的（按 `ui.groupBy` 分叉）**——这是视图路由化的核心复杂性，设计 §5.1 未预见：
  - `groupBy==='folder'`（文件夹分组单条滚动流）：点文件夹**不设 activeDirectoryId**，而是设 `ui.pendingScrollDirId=node.id`（滚动锚点）+ 强制 album='all'。文件夹在此模式下**不是筛选维度**而是"滚到某文件夹段"的位置；`getViewKey()` 恒为 `album-all`（FoldersSection:629-634）。
  - `groupBy!=='folder'`（date/none 分组）：点文件夹才 `setActiveDirectory(node.id)`（文件夹作筛选）；`getViewKey()`=`dir-<id>`（FoldersSection:636）。
  - 含义：`/folder/:id` 路由化在分组模式下会破坏"全部文件夹单条滚动"语义；folder 在该模式下不是可寻址"视图"而是随滚动变化的位置（`scrolledDirectoryId` 随滚动更新，放 URL 会每滚一下变一次，荒谬）。
- **scroll cache / KeepAlive / 侧栏高亮全部以 viewStore（非 route）为键**：MediaGrid `getViewKey()`（697-698）= `activeDirectoryId ? dir-<id> : album-<smartAlbum>`；侧栏 folder 高亮读 `scrolledDirectoryId`(folder 模式)/`activeDirectoryId`(其它)，smart-album 高亮读 `activeSmartAlbum`，均不读 route。故路由化必须在 MediaGrid 读 getViewKey **之前同步**把 route→viewStore 写好，否则 scroll cache 读写错桶（scroll/KeepAlive 回归）。
- **smart-album 与 folder 共享路径 `/`**：smart-album 'all' 与 date 模式 folder 都停在 `/`（LibrarySection:119 / FoldersSection:638 均 push '/'）。故 `/`→setSmartAlbum('all') 会 clobber date 模式 folder 的 activeDirectory → smart-album 与 folder 路由化必须一起做，不能只做一半。
- **导航站点清单**（路由化须全改）：FoldersSection:638（folder 点击 push '/'）、:561（addRoot 后）、:597（move 后 setActiveDirectory 无 push）；LibrarySection:119（所有 smart-album push '/'）。路由表已有 `/folder/:id`/`/favorites`/`/trash`（均指 MediaGrid，当前无人导航去），缺 `/live-photos`/`/recent`。SmartAlbum='all'|'favorites'|'trash'|'live-photos'|'recent'（types/ui.ts:30）。
- **结论**：S2-c2-3（视图路由化）需 folder 双模的产品决策（分组模式下 folder 是否/如何进 URL）+ 真机滚动/KeepAlive 验证，不宜在无 GUI 会话仓促推进；先做 S2-c1（descriptor 统一，纯重构安全）。
- **两处 backend 投影可统一点**（S2-c1）：useJustifiedLayout(38-92) 与 useViewDescriptor(37-62) 的 precedence（person>collection>smart-album>directory）与 filter 映射几乎相同，仅 person/collection(user)/trash/directory 的表达形态不同（扁平 filters.personId/albumId/trashedOnly/directoryId param vs scope 判别联合）；system collection、favorites/live/recent 的 filter overlay 完全相同。抽 resolveView 纯函数集中 precedence，两投影机械映射到各自形态即可消除 🔴 R1-2 双维护。

## 会话续 3 补充发现（2026-07-12，S1 UiIconButton + S3 roving tabindex）

- **S3「统一 overflow」缺口被现状核实缩小**：`useToolbarOverflow`（纯核 computeOverflowSplit 已 8 例单测 + ResizeObserver 缓存 offsetWidth/gap/padding 校正）**已是内容驱动溢出的完整基建**，满足设计 §6.3「以 priority/group 为输入,不以硬编码宽为输入」。设计与旧 findings(§39) 提到的 252px 是 `AppToolbar` filter popover 的独立固定宽（i18n 缺陷,归 AppToolbar/Search 治理），**不是工具栏溢出魔数**。故 S3 真正的即时缺口是 findings §60 的 **roving tabindex**（非 overflow 引擎本身）。ContextualToolbar 溢出「接入」仍待命令增多（navigation 组当前仅 3-8 命令,现接入会触发设计 §风险表警惕的「折叠振荡」空转）。
- **图标按钮 a11y 的类型层根治**：`.btn-icon` 遍布 9 文件 38 处,可访问名全靠人工在每处写 `aria-label`,漏配即读屏空按钮（findings §45 同类:MediaThumb 无 alt）。UiIconButton 把 `label` 提为**必填 prop**,把「记得写可访问名」从运行期约定变成编译期约束——这是设计 §S1「修 aria-label/aria-pressed」比逐处补 aria-label 更根本的做法。`toggle` 门控 aria-pressed 亦从类型层区分开关型/瞬时型(此前靠 `cmd.isActive ? … : undefined` 三元手写,易漏)。
- **roving 停靠位必须避开 disabled 项**（实现关键正确性,harness 实证）：disabled `<button>` 天然不可聚焦,若 Tab 停靠位(tabindex=0)恰好落在 disabled 项上,该项进不了 Tab 序、其余项又都是 tabindex=-1 → **整条工具栏无任何 Tab 落点**。故 `refresh()` 在挂载/命令集变化时把 activeIndex 校正到首个可聚焦项。gallery harness 实测撤销/重做(disabled) tabindex=-1、全屏(enabled) tabindex=0,校正得证。
- **🟡 既有 bug(非本次回归,surface 记录)**：ContextualToolbar 图标按钮的原生 `title` 键位**双写**——harness dump 显示 `title="撤销 (Ctrl+Z) (Ctrl+Z)"`。根因:命令 i18n `title` 文案本身已烤入快捷键「撤销 (Ctrl+Z)」(resolveCommandTitle 仅原样返回 cmd.title),`tooltipFor` 再 `+ formatKeybinding` 追加一次 → 双写。此前旧模板 `:title="tooltipFor(cmd)"` 与迁移后完全一致,故非 S1/S3 引入。治理归属:命令/i18n 层(要么从各 locale title 去掉内联快捷键,要么 tooltipFor 不再追加),不在 primitive 范围,留后续。同时 aria-label 用 resolveCommandTitle 亦含内联「(Ctrl+Z)」,读屏会读出快捷键,同源问题。
- **UiIconButton 广域迁移的真机依赖**：ContentViewer 的 `.detail-controls .btn-icon`(暗色浮层改白字)是**父作用域后代选择器**,Vue 3 中子组件根元素会带父 data-v,故迁移后仍命中——但 Viewer 属重组件、需真机验证浮层观感,本会话不迁。ToolsSection(9 处,状态驱动多)、GalleryViewControls(布局/排序 toggle) 等 7 文件同批留待广域迁移。

## 会话续 3 补充发现（2026-07-12，S1 UiDialog 原语 + 迁移 2 对话框）

- **对话框外壳是比 `.btn-icon` 更高杠杆的死重灾区**：`.btn-icon` 广域迁移剩 8 文件多状态重/真机依赖,收益分散;而**7 个对话框各自逐字重复 ~120 行外壳**(overlay/content/header/title/关闭键/body/footer + 两个 keyframes)。index.css:446 的「Modal 基座 A2」注释自陈:全局层此前只提 `.dialog-overlay/.dialog-content`,且各对话框 scoped 同名副本靠 `[data-v-*]` 更高特异性照常覆写(观感零变化=证明是纯死重)。UiDialog 把整套外壳收敛为单一可测源,迁 2 个消费者即净删 244 行,杠杆远高于逐处补 aria。
- **Teleport 组件的 SSR 测试范式**（基建补充,复用 UiButton SSR 范式的关键扩展）：`renderToString(app)` **默认不把 `<Teleport to="body">` 内容放进返回串**——teleport 产物落在传入的 `context.teleports` 里(key 为目标选择器)。测试须 `renderToString(app, ctx)` 后 `main + Object.values(ctx.teleports ?? {}).join('')` 合并再断言。这是所有走 Teleport 的原语(未来 UiPopover 亦然)的 SSR 测试前提。
- **原语接管焦点陷阱、初始焦点目标留消费方插槽——解耦成立的机理**：`useFocusTrap`(line 70)用 `container.querySelector('[data-autofocus]')` 定位初始焦点。因 Vue 插槽内容虽在**父组件渲染作用域**编译,但 DOM 上挂在 UiDialog overlay 子树内,`querySelector` 能跨组件边界命中消费方插槽里的 `data-autofocus` 按钮。故「原语持 overlay ref + 消费方保留 autofocus 标记」二者解耦,原语无需知道谁该初始聚焦。
- **顺带修掉 CloseConfirmDialog 漏用 Teleport 的潜伏隐患**（真实缺陷,非纯 dedup）：ConfirmDialog 用了 Teleport、CloseConfirmDialog **没用**——后者在组件 DOM 原位渲染,其 `z-index:9999` 只在原位的层叠上下文内有效,一旦祖先有 `overflow:hidden`/`transform`(建立新层叠上下文)即被裁剪。UiDialog 恒 Teleport 到 body,迁移即统一根治此类。
- **迁移的可验证边界（诚实标注）**：SSR contract 钉死 UiDialog 结构/aria/插槽/门控;两处迁移是构造性行为保持(同一 useFocusTrap、同一 aria id/data-autofocus/handler,结构逐字对齐)。但焦点陷阱 engage/release、Escape、点遮罩关闭、动画属 **DOM 运行期行为,node SSR 环境不触** → 按项目降级政策标注真机验收,与 useFocusTrap 注释「DOM 接线部分无 jsdom,由手测验证」的既有取舍一致。**不声称交互行为已验证**。
- **其余 5 对话框迁移的差异面**（留待,部分真机依赖）：FolderCreateDialog **未用 useFocusTrap**、靠原生 `autofocus` + onMounted 聚焦 overlay,迁 UiDialog 会**新增焦点陷阱=行为变更**(改善但需真机验);OnboardingWizard(多步向导)、FaceApprovalPanel(审批面板)、ExoticActivateDialog(激活流)结构异质,非「标题+正文+页脚」标准壳,迁移需逐个评估插槽切分。

## 会话续 4 补充发现（2026-07-12，摊薄 UiDialog：迁 FolderCreate/FolderTreeSelector + useFocusTrap 通用化）

- **UiField 杠杆低于路线图预期（探查改变了推荐）**：路线图默认下一原语是 UiField,但勘察全库表单面发现——`.form-group`(带 label 无 `for`/input 无 `id` 的**真实 a11y 关联缺陷**)仅集中在 FolderCreateDialog;NetworkStorageSection 用 `<label><span/><input/></label>` 嵌套式(隐式关联、无缺陷但自成 `.ns-field` 样式);其余散在 DocumentViewer/ReplacementPanel/ProofreadPanel/HGalleryLabView 各用本地约定。**表单面碎裂在 12-13 文件、无单一主导约定、无全局 CSS 基座**,造 UiField 须先做「选哪套当 canonical 样式」的设计裁决 + 消费方视觉异构=真机样式风险。故推荐改为**摊薄已建的 UiDialog**（primitive 价值只有摊薄消费方才兑现）。教训:造下一个原语前先量真实杠杆,别被路线图默认顺序带偏。
- **useFocusTrap 的两种挂载范式 + 通用化**：`watch(active)` **非 immediate** 只在 false→true 跳变 engage。①「恒挂载 + open 翻转」(ConfirmDialog/CloseConfirmDialog,由 store 状态驱动 open)——挂载时 open=false,翻 true 时 engage,工作正常。②父 `v-if`「挂载即开」(FolderCreate/FolderTreeSelector,父条件挂载子)——挂载即 active=true 无跳变,**焦点陷阱永不 engage**。通用化=watch 加 `immediate`(挂载即 active=true 也 engage;对范式①挂载时 false 两分支不命中=零变化) + `onBeforeUnmount(release)`(范式②在 open 态被卸载时归还焦点)。安全前提:useFocusTrap 现唯一调用方就是 UiDialog（Confirm/CloseConfirm 已改由 UiDialog 托管,不再直接调）——共享 composable 改动的波及面因此被收敛到单点。
- **immediate 把 DOM 接线提前到 SSR 渲染期（门禁抓到的真实回归）**：焦点陷阱是纯客户端 DOM 关注点。原非 immediate 时 SSR 单次渲染无跳变,`engage` 从不触发,`document` 从不被碰;改 immediate 后它在 SSR setup 期(open=true)就跑,同步读 `document.activeElement` → node 无 document → `ReferenceError`,UiDialog 13 例 SSR 契约全挂。**正解不是撤 immediate(那毁挂载即开),而是给 engage 加 `typeof document==='undefined'` 守卫**——浏览器照常,SSR 空操作。这是「SSR 不触 DOM」契约在**共享 composable 被更多范式复用后**必须补的守卫。
- **摊薄迁移=dedup + 真实缺陷修复二合一**：FolderCreate/FolderTreeSelector 迁 UiDialog 不止删外壳——二者**此前无 Teleport**(原位渲染,z-index 可被祖先 overflow/transform 层叠上下文裁剪,同 CloseConfirm 旧隐患) + **无焦点陷阱**(仅手动 focus 遮罩,Tab 会逃出对话框)。UiDialog 恒 Teleport + useFocusTrap 一次补齐两者。判断某迁移是否「纯 dedup 低价值」时,先核消费方当前是否**缺失** primitive 已内建的能力——缺失即迁移自带修复红利。
- **UiDialog 迁移的样式适配旋钮**：列表/树类对话框正文需满幅(hover/选中高亮到边),而 UiDialog `.dialog-body` 默认有 spacing-lg 内边距,消费方 scoped 无法跨 data-v 覆盖原语元素 → 加通用 `bodyPadding` prop(内联 style,缺省不产)。自定义分栏页脚同理:UiDialog footer 为 flex-end,消费方在 `#footer` 插槽内包一层 `width:100%` 容器即可自撑 space-between,无需改原语。宽度特化走既有 `maxWidth`。**原则:原语暴露少量正交样式旋钮(padding/maxWidth)覆盖高频差异,异形布局靠插槽内自撑,避免为一次性需求改原语结构。**
- **多根组件的嵌套对话框**：FolderTreeSelector 迁 UiDialog 后为多根(UiDialog + 嵌套 FolderCreateDialog),后者作同级根、各自独立 Teleport 到 body。两对话框同 z-index,靠 DOM 顺序分层(后 v-if 挂载者靠后=在上)。父级调用点(ContentViewer/MediaGrid)只传 `:title/@close/@confirm` 无 class 透传,故多根无 fallthrough attr 告警。

## 会话续 5 补充发现（2026-07-12，迁 ExoticActivateDialog + 三待迁对话框异质度分诊）

- **三个待迁对话框按「是否需要原语扩展」分诊**（读齐 Exotic/Onboarding/FaceApproval 后）：ExoticActivateDialog 的外壳与 UiDialog **逐字同构**（overlay+header〔title+X〕+body+footer〔2 键〕、`@click.self`/`@keydown.esc`/`tabindex=-1` 全等内建、范式①），是纯换壳即可迁的「同款消费者」。而 OnboardingWizard、FaceApprovalPanel **都卡在 UiDialog 缺 header 插槽**：前者 header 含副标题 + 3 点步骤进度条（`role=progressbar`），后者 header 含副标题；UiDialog 现只渲染 title-only header。二者还各有非标语义（Onboarding 刻意不支持遮罩/Escape 关闭以防首启误触、FaceApproval 无页脚且动作行内于每组）。**结论：给 UiDialog 加 `#header` 插槽（覆盖默认 title 行）是这两迁移的共同前置，宜作独立原语扩展增量，不塞进本轮。** 教训：迁移排序按「消费方与原语契约的差距」而非文件大小——Exotic 266 行但零契约差，Onboarding 428 行却要先改原语。
- **「N 个关闭入口 → 1 个 @close 出口」的收敛是原语化最易漏接的静默 bug 面**：ExoticActivate 原本遮罩点击/Escape/X 键三路各自绑 `onCancel`（内含 `gate.activating` 激活在途禁关守卫）。UiDialog 把三路统一成单个 `@close` 事件——迁移正确性完全取决于「三路是否都真走 @close」。核对 UiDialog 内部 `@click.self`/`@keydown.esc.stop`/关闭键三处 emit('close') 齐全，故 `@close='onCancel'` 一处即覆盖三路守卫。这类收敛必须逐路验证 emit 源:若某原语只在 X 键 emit 而遮罩点击自行关闭,守卫会被静默绕过。
- **迁移顺带补 aria-labelledby（原组件根本没有）**：ExoticActivate 原 header 的 `<h2>` 无 id、dialog 无 `aria-labelledby`。迁 UiDialog 传 `title-id` 后自动建立 h2↔dialog 的无障碍标题关联。与「无 Tab 焦点陷阱→迁后自动获得」并列:**被迁旧组件往往在 a11y 上欠账，原语把这些欠账一次性补齐**,这是原语化超出「去重」的隐性价值。
- **构建产物尺寸是外壳删除的客观证据**：迁移后 `ExoticActivateDialog-*.css` chunk 缩到 0.70kB（此前含整套 overlay/content/header/footer/keyframes scoped 副本）。当「观感应零变化」在无 GUI 环境难以直接证明时，scoped CSS chunk 显著缩小 + 全量门禁绿是「样式确实移交基座、未新增/丢失规则」的可测旁证。

## 会话续 6 补充发现（2026-07-12，UiDialog 三扩展 + 迁 Onboarding/FaceApproval 收官 7/7）

- **给共享原语加「布局能力」用条件类隔离，而非改默认路径**：FaceApproval 要「封顶高度 + 正文独立滚动」，需 `.dialog-body { flex:1; min-height:0; overflow-y:auto }`。若无条件加到 `.dialog-body`，会波及全部 6 个既存对话框的 flex 尺寸行为（微妙、且各对话框正文结构不同，需逐个真机核）。改为 `maxHeight` prop 触发 `dialog-content--capped` 类、把滚动规则**限定在该类下**——只有传 maxHeight 的对话框(唯 FaceApproval)进这条路，其余 6 个 `.dialog-body` 字节级零变化，回归面归零。原则:**共享原语的布局扩展默认走「加法 + 条件类隔离」，让「不使用新能力」的路径保持字节不变**（比属性/插槽的加法扩展更需警惕，因 CSS 层叠会跨消费者泄漏）。
- **SSR 串断言的负向「裸子串」检查是脆弱点**：`renderToString` 会把**模板注释**渲染进返回串。UiDialog 的 #header 插槽注释含字面「aria-labelledby」,使既有 `expect(html).not.toContain('aria-labelledby')` 误命中而红——实际 `aria-labelledby=` 属性并未渲染(行为正确)。正解=负向断言收紧到属性/标签的**结构形式**(`aria-labelledby=` 带 `=`、`<tag`、`class="x"`),而非裸词。教训:凡 `.not.toContain(裸词)` 的 SSR 断言，都可能被注释/i18n 文案/占位串误命中，写时即用结构形式。
- **useFocusTrap 通用化地基被第 2、3 个「挂载即开」消费者验证**：Onboarding 与 FaceApproval 都是父 `v-if` 挂载(范式②，无 open false→true 跳变)。因上一线已把 useFocusTrap 通用化(watch immediate + onBeforeUnmount release + engage SSR 守卫)，这两次迁移**无需再动 composable**，直接传 `:open="true"` 即获焦点陷阱。地基一次做对、后续零边际成本——印证「共享 composable 的范式扩展一次性收敛在单点」的价值(useFocusTrap 唯一调用方仍是 UiDialog)。
- **原语摊薄收官 = 全库模态外壳单一化**：7 个对话框(Confirm/CloseConfirm/FolderCreate/FolderTreeSelector/Exotic/Onboarding/FaceApproval)现全部经 UiDialog，`.dialog-overlay`/`.dialog-content` 的手写副本在业务组件中归零(仅剩 UiDialog 原语 + index.css 全局基座两处)。UiDialog 能力面 = open/title/titleId/describedById/closeLabel/showClose/maxWidth/bodyPadding/maxHeight/closeOnOverlay/closeOnEsc + slots default/footer/header。后续 primitive 工作重心转向 .btn-icon 广域迁移(8 文件，多真机依赖)与 UiField(已判定杠杆低，见会话续4)/UiPopover/UiToolbar。

## 会话续 7 补充发现（2026-07-12，.btn-icon → UiIconButton 广域迁移 7/8 文件 30/34 站点）

- **子组件根元素携带父作用域 data-v → 类型化包裹迁移视觉零风险（修正「需真机」的过度悲观）**：把 `<button class="btn-icon">` 迁到 `<UiIconButton>`，视觉是否变？不变。二因：①UiIconButton 根仍挂全局 `.btn-icon` 类；②**Vue 3 中子组件根元素同时带父作用域的 `data-v`**（这是 Vue 3 相对 Vue 2 的关键差异——正是为让父级 scoped 能作用于子根）。故父级 scoped 后代选择器（ContentViewer 暗色浮层 `.detail-controls .btn-icon`、AppToolbar 折叠 `.toolbar__foldable > .btn-icon`、直接子 `>` 亦然）与消费方经 class-fallthrough 传下的额外类（`.scan-root__remove`/`.danger-icon`）迁移后**照常命中根元素**。前期规划把这两文件标「需真机」是过度谨慎；实际结构安全，真机仅作精确视觉确认。判据:迁移视觉安全性 = 「根元素类是否保留 + 父 scoped 是否靠该类/标签命中」，二者成立即零风险。
- **模板 ref 落在组件上 → 得实例而非 DOM，弹层锚定类站点是真实迁移阻断**：AppToolbar 的 filter/view 按钮带 `ref=filterBtnRef/viewBtnRef` 供弹层 `getBoundingClientRect` 定位。`<button ref=x>` 时 `x.value` 是 DOM 元素；`<UiIconButton ref=x>` 时 `x.value` 是**组件公共实例**（`<script setup>` 无 defineExpose 时近空），`.getBoundingClientRect()` 不存在 → 定位失效。这是**真实技术阻断（非真机顾虑）**：须先给 UiIconButton `defineExpose({ el })` 或让消费方用 `.$el`。教训:批量把原生元素换类型化包裹前，先 grep 该元素上的**模板 ref**——凡被 ref 取作 DOM 用（定位/measure/focus/scrollIntoView）的站点，包裹前须先解决根元素暴露。
- **机械迁移守 parity，不夹带 a11y 语义变更**：多个图标按钮是真开关（收藏/人脸显隐/LIVE 播放），语义上「该」有 `aria-pressed`，而 UiIconButton 的 `toggle` prop 正为此设。但原生版本都无 aria-pressed。机械迁移一律只映射 `:active`（视觉）、**不加 `toggle`**——加 aria-pressed 会改 a11y 树（读屏播报「已按下」），属独立的 a11y 增强决策，不应搭车进「视觉/行为 parity」的批量迁移，否则回归面与评审范围失控。哪些该补 aria-pressed 留作专门 a11y 走查。

## 会话续 8 补充发现（2026-07-12，UiField 原语 + 迁 4 消费者；template ref-as-DOM；migration parity 分级）

- **template ref 落在组件上得实例，defineExpose 出口解锚**：AppToolbar filter/view 用 `ref.value.getBoundingClientRect()` 做弹层锚定。`<button ref=x>` 时 `x.value` 是 DOM；换 `<UiIconButton ref=x>` 后是**组件公共实例**（script-setup 无 defineExpose 时近空），无 getBoundingClientRect。解法=UiIconButton `defineExpose({ el })`（el 为根 `<button>` 的模板 ref），消费方 `ref.value?.el.getBoundingClientRect()`，类型 `InstanceType<typeof UiIconButton>`。**批量把原生元素换类型化包裹前必先 grep 模板 ref**：凡被 ref 取作 DOM 用（定位/measure/focus/scrollIntoView）的站点，包裹前先给原语加根元素暴露。
- **全库表单字段清点 = 6 种碎裂范式（A-F）**：A `.form-group` 兄弟 label（无关联）/B 包裹 label+span caption（隐式关联）/C 包裹 label+文本节点（隐式）/D 兄弟 label（无关联）/E span 伪 label（无关联）/F 无 label 仅 placeholder（无关联）。wrapping-label 家族(B+C)最普遍(5/9 文件)；A/D/E/F 有真 label-未关联缺陷。两朝向(stacked/inline)并用。UiField 双关联模式(nest 包裹 / for 显式)正为吃下这 6 种。
- **hint/error 必须置于 `<label>` 之外**：nest 模式 `<label>` 包裹 caption+控件给隐式关联，但控件的**可访问名 = label 的全部文本内容**；若 hint/error 文字也在 label 内，会被并入名字（label 名 = "caption hint error"，错）。故结构定为 root div > `[group=label 包 caption+控件]` + `[hint span]` + `[error span]` 同级，hint/error 经 `aria-describedby`（插槽 prop 暴露）关联而非并入名字。「隐式关联 + 独立描述文字」共存的正确 DOM。
- **删「作 CSS 锚点的包裹类」会静默打断后代样式，须重锚**：`.ns-field input` 靠 `.ns-field` 类命中输入框；迁 UiField 后 `.ns-field` 类消失 → 输入框静默失去背景/边框（**门禁抓不到，纯视觉退化**）。修法=重锚到不变的祖先容器 `.ns-form input`。每删一个作锚点的 class，必 grep 有无 `.该类 后代`/`.该类 > 子` 选择器并重锚。
- **迁移的「同构 vs 非同构」决定能否无真机推进**：UiDialog/.btn-icon 迁移是**构造即视觉等价**（同一全局类 + 父作用域命中，观感字节不变）→ 结构门禁验 + 标注真机即可安全落地。而 UiField 迁移**改真实视觉**（label 字号 xs→sm、布局 space-between→gap、加 wrapper div 影响 flex）→ 属非同构变更，无真机无法确认可接受，force 迁移会引入静默视觉退化。**判据：迁移若不改类/结构（同构）→ 可无真机批量；若改视觉度量（非同构）→ 逐个真机或 defer。** 据此把 UiField 消费者分为「干净合身已迁 4」（FolderCreate/Proofread/SemanticSearch/NetworkStorage，stacked 表单，视觉 delta 极小）与「defer 5」（DocumentViewer/ReaderSettings/HGalleryLab/ReplacementPanel/SettingsView，工具栏密度/space-between 行/紧凑栏/lab 工具/checkbox）。
- **checkbox/toggle/stepper/segmented 不是 UiField 形态**：UiField 合身对象=「caption + 单个可关联控件（input/select/textarea/range）」。checkbox（控件在前+行内文字）、stepper/segmented（非原生控件的按钮组）结构本质不同，强塞进 UiField 是坏设计——宜作独立 UiCheckbox/UiToggle 原语。**「迁全部消费者」的范围要跟控件结构走，不是文件清单**：一个文件里的 select 可迁、checkbox 不可迁。

## 会话续 9 补充发现（2026-07-12，UiToggle 原语 + 迁 DynamicSettingControl；UiSelect 边界诊断划出同构迁移的天花板）

- **有全局 CSS 组件可包裹 = 同构原语；无则是新视觉需决策**：checkbox/toggle 部件此前被判「宜作独立原语」。落地时发现二者地基天差地别——`.toggle` 开关在 index.css 有**完整全局 CSS**（label + 隐藏 checkbox + `.toggle__thumb`，态全走纯 CSS `:has(:checked)`），故 UiToggle 可**同构包裹**（像 `.btn-icon`，构造即视觉等价，无真机可验）；而裸 `type="checkbox"`（10 处）**无全局 checkbox CSS**（grep `.checkbox` 命中零，各组件本地/原生样式），UiCheckbox 属**新视觉/需设计决策**，不是无真机可验的干净增量。**判据：建原语前先查它要包裹的全局 CSS 存不存在——存在→同构包裹（无真机）；不存在→新视觉（需决策）。**
- **注册表驱动的单一消费者 = 最优同构迁移标的**：DynamicSettingControl 按 `SETTINGS_MAP.control` 分派控件，所有 `control:'toggle'` 设置项都经它一处渲染。迁这一处即覆盖整个设置开关面——单站点、低风险、高杠杆。与「.btn-icon 8 文件 34 站点」的广域机械迁移相比，注册表驱动标的是原语化性价比最高的形态：找「数据驱动、一处渲染多实例」的组件优先迁。
- **同构迁移的天花板 = 父 scoped 后代选择器是否伸进子组件内部**：`.btn-icon`/UiDialog/UiToggle 能无真机零风险迁，共同前提是**父作用域的样式只作用于子组件根**（Vue 3 child-root 携父 data-v，故 `.detail-controls .btn-icon`、`.compact-toggle` 等作用于根的选择器迁后仍命中）。UiSelect 撞破这个天花板：`.compact-select-wrap .select`（后代选择器）伸进 `<select>` 内部，而 select 迁入子组件后只带子组件 data-v、不带父 data-v → `.compact-select-wrap[data-v-parent] .select[data-v-parent]` **静默失配**，紧凑面板 select 尺寸悄悄退回默认。**这是会话续8「删包裹类作 CSS 锚点静默断样式」隐患的同类第二形态（不是删类，是把被后代选择器命中的元素移进子组件）。迁移前必 grep `.父类 [后代]`/`.父类 > [子]` 形态的 scoped 规则——命中即非纯同构，需 `:deep()` 重锚或 defer。**
- **`:deep()` 可修但改的是无门禁可验的 computed style → 无人值守应停下上报**：`.compact-select-wrap .select` → `.compact-select-wrap :deep(.select)` 能推理证明等价（同一元素、同一声明，仅去掉内层 data-v 约束）。但「compact 面板 select 的计算样式」**无任何自动门禁能验**（SSR 断结构/属性，不断 computed style；对比度门禁只覆盖 token）。撞上已知隐患类首例、且修复涉及无门禁可验的视觉时，无人值守的正确纪律是**停在干净检查点上报**（把 `:deep()` 重锚交用户裁决 / 真机验），而非径直套 workaround 再贴「真机验收」标签——后者会把「首次遇到的隐患类」混入「已验证的机械迁移」，稀释证据可信度。
- **可选 a11y 入口不破 parity**：UiToggle 给 `label`→`aria-label` 做**可选** prop（原 `.toggle` 无可访问名，是 a11y 缺口）。canonical 消费者不传 → 渲染与原结构字节等价（parity 保全）；需要修 a11y 的消费者可传。这与 UiIconButton「label 必填」相反——图标按钮**普遍**无名故强制，开关有邻近可见 label（设置项名）故做可选、不强制威胁 parity。**原语的 a11y 契约松紧要看该类控件是否普遍缺名。**

## 会话续 10 补充发现（2026-07-12，UiCheckbox 原语「先定视觉」→ 蒸馏既有 de-facto 模式；类名碰撞规避）

- **「定新视觉」往往塌缩成「蒸馏既有 de-facto 模式」——先 grep 既有用法再决定**：以为 UiCheckbox 要发明新视觉（因无全局 checkbox CSS），侦察后发现全库 checkbox 早有既定语言——`accent-color: var(--color-accent)` on native checkbox（主题感知、零自绘 SVG、原生 a11y 全保留），且 ConfirmDialog/CloseConfirmDialog **重复定义**了同款 `.remember-checkbox`。故"定视觉"= 把重复的本地模式蒸馏进原语（既定视觉又 dedup），而非发明。**判据：建"需定视觉"的原语前，先 grep 相关控件的既有实现——多半已有 de-facto 模式可蒸馏，发明是最后手段。**（对照会话续9：`.toggle` 有全局 CSS→同构包裹；checkbox 无全局类但有分散的 de-facto 模式→蒸馏。二者都不是从零发明。）
- **建"新视觉"原语前必 grep 目标类名有无被占用——全局类会层叠泄漏**：拟用 `.checkbox` 作全局类，grep 发现**已被 MediaThumb 网格选择控件**（20px 圆+白勾，canvas 镜像 MediaGridCanvas）占用。全局 `.checkbox`（index.css，无 data-v）会层叠泄漏到那个 scoped 组件的 `<div class="checkbox">`（全局样式穿透进 scoped 组件元素）→ 污染网格选择视觉。**解法=UiCheckbox 样式作用域私有 + 类名改 `.checkbox-field` 彻底避碰。** 机理区分：wrap 既有全局类的原语（UiButton `.btn`/UiToggle `.toggle`）样式在 index.css 全局；**定义新视觉的原语（UiCheckbox）样式应作用域私有**——既避碰又符合"新视觉不该全局广播"。
- **蒸馏重复的本地 CSS = 同构迁移 + dedup 二合一**：ConfirmDialog/CloseConfirmDialog 的 `.remember-checkbox` 字节等价（后者仅多 `margin-top`）。UiCheckbox 字节复刻该视觉 → 两处迁移**同构可无真机**，且消灭两份重复本地 CSS。消费者特有的 margin-top 经 class fallthrough（`.close-remember-mt` 落 UiCheckbox 根 label）保留——**布局间距归消费者、控件视觉归原语**的正确分层。
- **同构 vs 非同构判据再验（accent-color 版）**：迁移把裸 checkbox 换 UiCheckbox 是否同构，看该 checkbox **现有没有 accent-color**——ConfirmDialog/CloseConfirm 有（同构，字节等价）；HGalleryLab/ReplacementPanel/SettingsView/ReaderSettings 无（浏览器默认蓝，迁后变主题 accent + 16px 定尺寸 = 真实视觉变更 = 非同构，defer 真机）。同一原语的不同消费者按"现有视觉是否已等于原语视觉"分同构/非同构。
- **防御性加固要如实披露其视觉影响面**：UiCheckbox 加 `flex-shrink:0`（原 `.remember-checkbox` 无）防长标签挤扁 16px 方框。这是原语该有的健壮性，但严格说偏离了"字节复刻"。诚实口径=**当前短标签消费者无挤压压力→视觉零变化**，故不破坏同构迁移；但在 commit/docs 明说这是"加固非复刻"，不含糊成"完全字节等价"。加固可以做，但要标出它与 parity 声明的边界。

## 会话续 11 补充发现（2026-07-12，「继续推进」→ 原语缺口全扫 + defer 分类源码级复核）

- **「同构无真机」的干净原语增量已由源码实证枯竭（非信摘要，逐个 grep 核实）**：把 index.css 全 498 行的顶层全局类逐一对照已建原语，剩余未包裹类无一构成 UiToggle 式干净标的——`.chip`（含 hover/active 全局 CSS）**唯一消费者** GalleryFilterChips，且混 `chip--rating/color/date/clear` 变体+`<div>`/`<button>` 混合元素（PluginStore 用的是另一类 `ps-chip`），包裹要引入变体 props+元素多态、单消费者低杠杆；`.input`（文本框全局类）**零消费者**（全库仅 `compact-input` 绑定且作用于 `input-number`），建 UiInput 会是无消费者原语违 YAGNI。**判据固化：干净同构原语标的 = 自足全局类 + 真实多消费者 + 迁移字节等价；缺任一即非干净增量，勿为「有事做」硬建。**
- **UiToolbar/UiPopover 非「包裹全局类」而是「定义新抽象」，属决策非无人值守**：grep 证 `toolbar__*`(AppToolbar)/`selection-toolbar__*`(SelectionToolbar) 全是各组件一次性 bespoke 类，**无共享 `.toolbar` 全局类**可包裹；浮层侧 20+ 组件各有 `position:absolute` 但定位逻辑各异（ContextMenu/各下拉/锚定弹层），UiPopover 需引入定位方案（floating-ui？手写 flip/offset？）=新代码+库选型决策+迁移真机。二者都不是 UiToggle 式「已有全局 CSS 等着被类型化」，而是要**发明抽象**——是产品/架构决策，不在「无需我决策的工作」范围。
- **defer 清单的分类错误被源码复核纠正——「需真机」≠「需 API 设计」**：前几轮把 UiCheckbox 余 5 站点笼统记为「4 默认蓝非同构需真机」。逐个读源码后阻塞实为两类：① **视觉**非同构——仅 HGalleryLab(48/66)，`<label><input v-model>text</label>` 契约吻合、纯换色（蓝→accent），真机可解但是 dev/lab 视图低价值；② **契约/结构不符**——SettingsView(88) 用 `:checked=includes()`+`@click($event,val)` 做多选集合切换（非布尔 v-model）、ReplacementPanel(28) flex 行内**裸** checkbox 无标签+`:title`、ReplacementPanel(48/70) `.repl-row__re` 含 `.*` 字形+`:title`、ReaderSettings(317) `.rs-row--toggle` 标签左·控件右**反序**设置行——这 4 处**真机解不了**，需扩 UiCheckbox API（@click 事件透传 / :title 透传 / 反序布局变体）或裁决排除出范围。**教训:defer 一批时若只记「需真机」会掩盖「需 API 设计/需裁决」的异质阻塞，误导未来会话把契约不符项当「只差真机」白试——defer 原因须落到每站点的具体阻塞类型（读源码得，非按类目笼统归）。呼应「re-verify before restating」红线。**
- **「继续推进」在干净增量枯竭时的正确动作 = 把侦察结论回写、纠正既有错分类，而非制造低价值非同构变更充数**：唯一契约吻合的 HGalleryLab 是 dev/lab 视图、迁移引入无门禁可验的视觉变更、价值微薄——为「看起来在推进」去做它属活动主义，违背纪律。本轮的实际产出=①用源码证伪「well 未干」的疑虑（枯竭是真的）②纠正 defer 清单把「需真机」误当唯一阻塞的错误分类（防错误传播）。**无新代码但有真实价值：证据级确认边界 + 修正会误导后续的文档口径。**

## 会话续 12 补充发现（2026-07-12，用户裁决「继续 3」→ UiSelect 跨「父 scoped 后代选择器伸进子组件」边界，:deep() 重锚）

- **`:deep()` 穿透子组件边界的等价性可逐层推理证明（把「无门禁可验」收敛到精确的一处）**：会话续9 停下上报的隐患本质=scoped 编译给选择器每一段都缀 `[data-v-X]`。迁移前 `.compact-select-wrap .select` → `.compact-select-wrap[data-v-X] .select[data-v-X]`，两元素同在本组件都带 X 故命中。迁移后 `<select>` 移进 UiSelect——**UiSelect 无 `<style scoped>` → 其内部元素不带任何 data-v**（连 UiSelect 自己的 scope id 都没有,因为没有 scoped 块就没有 scope id）；而 `.select-wrap` 根经 fallthrough **仍带父 data-v-X**（child-root 携父 scope 是 Vue 3 机制,前提是父有 scoped 块）。故 `.compact-select-wrap :deep(.select)` 编译成 `.compact-select-wrap[data-v-X] .select`（`:deep` 去掉其后所有段的 data-v 约束）——**同一 wrap 作锚（带 X）、同一 select 作靶（无 X 约束）、同一声明**，与迁移前的匹配结果逐元素等价。**判据：`:deep()` 重锚是否安全 = 左侧锚点元素迁移后是否仍带父 data-v（是→安全，靠 fallthrough）+ 右侧靶元素是否确为要命中的那个（是→`:deep` 去约束不误伤）。二者成立即推理级等价,无门禁可验的只剩「实际渲染尺寸」这一处,标真机确认即可。**
- **子组件无 scoped style 时,其内部元素零 data-v——这决定后代选择器为何失配**：常被忽略的一点——UiSelect 这类「纯包裹全局 CSS、无 `<style scoped>`」的原语,其模板内元素**不带任何 data-v**（不是带 UiSelect 的 data-v,是压根没有）。故父组件 `.父类 .子内元素` 形态的 scoped 后代选择器,迁移后右段的 `[data-v-父]` 必然失配（子内元素既无父 data-v 也无自己的）。这解释了为何**无 scoped 的原语**反而在「后代选择器命中内部元素」时更需 `:deep()`——它不像有 scoped 的子组件那样至少还有自己的 data-v 可辨识。**凡把「被父后代选择器命中的元素」移进任何原语,都要 `:deep()` 重锚,与原语有无 scoped 无关。**
- **v-model 桥接优先用 writable computed 走框架规范指令,而非手写 :value+@change**：UiSelect 内部用 `computed({get:()=>props.modelValue, set:v=>emit('update:modelValue',v)})` + `<select v-model="selected">`,让 Vue 的 vModelSelect 指令在运行期处理「按 modelValue 设 option.selected」+「change 读回 value」。这比手写 `:value="modelValue" @change` 更稳——原生 select 的选中态需遍历 options 设 `.selected`,vModelSelect 的 mounted/updated 钩子已正确处理（含 slotted options 在钩子运行时已是真实 DOM 子节点）。**包裹原生表单控件的原语,v-model 桥接走「writable computed + 原生 v-model」范式,把选中/读值的边角交给框架指令,不自己复刻。**
- **「停下上报的边界」在用户裁决后推进的正确姿态 = 裁决 + 可证等价 + 诚实标注三者齐备**：会话续9 停下不是因为 `:deep()` 不可行,是因为「首遇隐患类 + 无门禁可验 + 无人值守」三条叠加。用户裁决「继续 3」消解了「无人值守」这条,`:deep()` 的可推理等价消解了「盲改」顾虑,剩下「实际尺寸无门禁可验」如实标注真机确认。**三者齐备时推进是对的,与「没裁决就径直套 workaround 贴标签」有本质区别——后者稀释证据可信度,前者是有据推进。规范文本 write-back:progress.md 旧「停下上报」条已加 🔴 前向横幅指向本次落地,非仅追加新条。**

## 会话续 13 补充发现（2026-07-12，「继续做完所有工作，需测的直接说」→ UiCheckbox 迁移终态裁决 + S1 真机验收清单）

- **原语迁移的收官判据 = 剩余候选是否还共享该原语的「内聚视觉/契约身份」；越过此线的合并是为 DRY 而 DRY**：UiCheckbox 蒸馏的是 `.remember-checkbox` de-facto 形态（labeled accent 勾选框）。读全部 5 个剩余 checkbox 站点的源码 + scoped CSS 后确认，它们各是**独立 bespoke 微控件**——裸 inline 控件(ReplacementPanel 28)、`.*` mono 字形 toggle(48/70)、space-between 反序设置行(ReaderSettings 317)、集合成员切换(SettingsView 88)、native 蓝 lab 勾选(HGalleryLab)——无一是 remember-checkbox 形态。force 并入须给原语堆 `:checked` 模式/`@change`·`@click`·`:title` 透传/裸模式/反序布局/mono 字形，把一个内聚原语撑成 kitchen-sink，**毁掉其视觉身份**。**判据：一个原语的迁移「做完」不是「所有同类标签都换成它」，而是「所有共享它内聚身份的 de-facto 实例都收了」；剩下形态各异的就该保持 bespoke。识别信号=为容纳下一个消费者要给原语加与其核心视觉无关的 API/变体时，停——那是 DRY 越界，违高内聚/可读人本。**
- **「需真机」必再细分为 (a) 结构 1:1 仅像素待验 vs (b) 迁移即重构=盲设视觉**：用户「需测的直接说」把边界从「遇真机即停」放宽到「能推进的推进 + 攒测试清单」。但不能一律闷头改——(a) 类（如 UiSelect 紧凑尺寸：DOM 1:1、只 computed style 无门禁可验）可迁 + 列清单让用户测；(b) 类（如 UiField 余 5 消费者改 label 字号/布局/加 wrapper、UiCheckbox 余 5 站点改结构）是**我在盲设视觉**，交清单等于把没设计过的视觉塞给用户测。**判据：迁移是否「结构逐元素同构、仅渲染度量待确认」——是→可推进+真机确认（有据）；否（DOM/布局真变）→需先看真机现状再设计，不能拿门禁绿当挡箭牌闷头改。** 这细化了会话续9「同构 vs 非同构」——非同构里还分「可推理等价的像素待验」与「须重新设计的盲区」。
- **「out-of-scope-by-design」是合法的终态，不是拖延**：把余 5 站点标为 defer 会让未来会话反复重开「是不是该迁了」。读源码下**终态裁决**（不迁、保持 bespoke、附每站点理由）才真正收口。这类裁决可逆（保持现状是保守默认）、可 override（用户要 lab 一致性或把 reflow 改 UiToggle 随时推翻），故无人值守可下——但须**写回正文 + 给旧「defer 需真机」条加 🔴 横幅**（progress 第 285 行旧条已加，指向终态小节），防旧口径复活。呼应「规范文本 write-back 红线」。
- **干净增量枯竭后的正确产出 = 决策收口 + 把累积未验工作整理成可验清单**：本轮无新前端代码（同构 well 会话续11 已实证干涸），实际交付=① UiCheckbox 决策收口（待裁决→终态）② `realtest-checklist-S1.md`——S1 全量迁移（累积从未真机验收）按屏/流整理成可勾选测试项，门禁盲区（像素/焦点陷阱/弹层锚定/动画/主题）置顶。**「做完所有工作」在代码枯竭时 = 把决策和验证债都收干净，而非硬造代码。**

## 会话续 14 补充发现（2026-07-12，首轮真机反馈 6 项修复/特性）

- **破坏性操作的确认绝不能依赖 webview 原生 `window.confirm()`——Tauri v2 WebView2 里它不可靠(不弹框却返回 truthy),危险操作会静默穿透**：真机实测危险区四项(清库等)点击**无确认直接执行**,根因是 handler 用原生 `confirm()`。全库其它确认早已走自建 `useConfirm()` promise 单例 + ConfirmDialog(正是为规避原生 confirm 不可靠而建),唯独危险区 + SettingsView 漏用。**教训:① 原生 confirm/alert/prompt 在 Tauri 内不可信,破坏性 gating 必走 app 内受控组件;② 此前文档/记忆声称危险区有「confirm 契约」,代码在(`if(!confirm())return`)但真机从未生效——这是「门禁绿(typecheck/lint/test 全过)真机废」的教科书案例,`if(!confirm())` 看着完全正确却是 data-loss。凡「确认/权限/守卫」类逻辑,SSR/单测只能验分支,真机才验「弹框真的出现并拦住」——此类必列真机验收,不能凭代码在就声称已具备。** 修复顺带把 useConfirm/ConfirmDialog 扩 `danger`(红确认键)+ `requireText`(输入确认强门)两 opt-in 选项,清库须键入库名关键词才放行(反射式点击无法穿透)。
- **disabled 提交按钮 = 键盘不可达陷阱**：FolderCreate「创建」键 `:disabled="!canCreate"`,空表单时是原生 `<button disabled>`,焦点陷阱按 `button:not([disabled])` 正确排除→键盘用户 Tab 永远到不了。**WAI-ARIA 推荐:表单提交键保持 enabled(可聚焦),在提交 handler 内校验并给行级错误反馈,而非 disabled 到用户无从发现/无从知因。** disabled 视觉看似友好,实则对键盘/读屏用户是黑洞。
- **「用户报的回归」先查 git 是不是自己有意改的**:#4 工具栏分离 + #5 渲染切换消失,派 Explore 查证**都是同一个 S3 提交 bcf0934 有意所为**(#4 ownership 分层推翻 Phase G 合并;#5 把原型药丸 dev-gate 隐藏)。二者都不是 bug 是设计改动,故不是「修」而是**产品决策**——用 AskUserQuestion(带 ASCII 预览)让用户选方向,再实现。**教训:真机反馈里「回归」与「有意改动」要用 git blame/log 区分;有意改动的「回退」是产品决策不是 bugfix,该带方案让用户裁决,而非默默改回。**
- **设置页开关接入既有组件态的最优范式 = localStorage-backed 模块级响应式 composable + 注册表驱动 binding**：两特性(useTitlebarMode/useRenderMode)同一范式——模块级 `ref` 单例(所有消费方共享同一响应式对象)+ localStorage 持久(不动后端 app_config schema)。设置页经注册表驱动的 DynamicSettingControl **加一条 toggle/select binding 即接入**,与 S1 UiToggle/UiSelect「注册表驱动单消费者」同一杠杆:改一处覆盖设置面。切换 live 生效(共享 ref 响应式)、无需刷新、无后端往返。适合「前端本地实验/布局偏好」类(非需跨设备同步的核心配置)。
- **仓库有一批既存非-standalone-prettier-clean 的大 .vue(App/MediaGrid/SettingsView)——真实格式门是 ESLint 非 standalone prettier**：对这些文件跑 `prettier --check` 会报**整模板(从第 1 行起)**不合规,但那是仓库历史状态(CLAUDE.md 明载 `prettier --write` 会 rewrap Vue 模板内联 handler 炸构建)。**判据:改 .vue 后 `prettier --check` 若报错,先 `prettier <file> | diff` 看差异区——若是「从第 1 行起整模板」则是既存脏、非你引入(你的局部改动被裹在里面),`npm run lint` 过即合规,绝不 `--write`;若差异**只**落在你改的几行,才是你引入的、可手工对齐那几行。新/维护良好的 .vue(UiButton/ConfirmDialog/DynamicSettingControl 等)是 prettier-clean 的,改它们后 --check 应仍干净。**

## 会话续 17 补充发现（2026-07-13，UiPopover 原语选型 B=@floating-ui + 迁 3 弹层）

- **「用库 vs 自研」的真判据不是「能不能写」而是「这块逻辑是否已被生态固化成标准件」**：UiPopover 定位数学（flip 下方放不下翻上、shift 沿轴滑动保持视口内、autoUpdate 滚动/resize 跟随）正是 Popper→@floating-ui 固化的标准件（Radix/HeadlessUI/shadcn 底层同款）。CLAUDE.md 依赖纪律两条并列——「不为小需求拉 niche 依赖」**且**「不重造 vetted 生态包」；水平 clamp 是 trivial helper 该自写，flip/shift/autoUpdate 属第二条不属第一条。**证据：date 弹层已带漏水平钳制越界 bug——手写定位反复重新引入同类边界 case，正说明这类几何数学天然易错、不该自研。** 裁决 B。
- **桌面 Tauri app 的 JS bundle 字节非选型约束**：+@floating-ui ~12KB 可摇树，相对已装的 pdfjs-dist/shiki/onnxruntime 无感；「<10MB 核心」约束是 **Rust 二进制**不是 JS bundle，与前端库选型无关。真实成本是依赖数（维护面/供应链）非字节——而 @floating-ui 属 vetted，成本可接受。
- **@floating-ui/vue 天生 SSR 安全**（本项目 SSR 契约测试跑 node 环境无 jsdom，是唯一技术风险点）：useFloating 把所有 DOM 访问关在 `whileElementsMounted`（客户端 autoUpdate）+ null 元素守卫后，SSR/node setup 期不触 document/window。UiPopover.spec 6 例首跑即过，风险排除。**教训：把 DOM 定位库引入 SSR-tested 代码前，先单跑该原语的 SSR spec 验证不抛，再迁消费者。**
- **原语选择「持有契约、委托实现」而非「自绘一切」**：UiPopover 与「同构包裹全局类」的原语（UiToggle/UiSelect/UiEmptyState）不同——无 `.popover` 全局基座类可包，是真正的新抽象。但抽象边界仍遵同一原则：原语持有**项目契约**（Teleport/焦点陷阱/dismiss/SSR 结构），把**定位数学**委托给引擎、把**表面视觉**留给消费方 slot。引擎藏在原语 API 后 → 将来换回自研零消费者改动（契合「不冻结契约」）。
- **迁移顺带清理为绕开旧能力缺口而写的降级 hack**：AppToolbar 旧 `onWindowResize` 在 resize 时**关闭**弹层（因手写定位会 stale）——autoUpdate 落地后该 hack 反而劣化 UX（该跟随而非关），故删除。**教训：引入更强能力时，主动识别并清除为绕开旧缺口而写的降级代码，否则新旧叠加互相抵消。**
- **真机盲区（门禁不覆盖，见 realtest-round5）**：① 定位视觉/flip 触发（近视口边缘翻转）② 焦点陷阱（date 弹层此前无、现新增，首焦点落 from 输入）③ **嵌套弹层**——filter ⋯ 菜单（UiPopover）内的 date 弹层（另一 UiPopover）：两者 backdrop z-index 均 300、floating 均 301，date backdrop(300) < filter floating(301) 致「点 filter 菜单区不关 date 弹层」（旧实现 date backdrop 300 > filter ~200 会关）——轻微 nested-dismiss 差异，待真机验，必要时改动态 z-index（按打开序递增）。

## 会话续 18 补充发现（2026-07-13，selection bar 合并/分离方案产出并批准落盘——纯设计会话,零代码）

- **Teleport 消解「UI 在别处=逻辑须搬家」的结构张力,推翻会话续16 评估前提**：会话续16 记「批量动作须从 MediaGrid 抽共享 composable 为使能前提」,隐含假设=docked 条由 AppShell/AppStatusBar 渲染。实际 Vue 的 Teleport 只移动**渲染产物(DOM)**,虚拟树父子关系/事件链/依赖注入全部原位——docked 形态由 SelectionToolbar(MediaGrid 子组件)自己渲染、DOM 投送进状态栏 outlet,9 个纠缠网格局部状态(patchVisibleSelected/pendingDeleteIds/FLIP/compute)的 handler 一行不动。**判据:遇「组件 A 的动作要出现在组件 B 的位置」先问 Teleport 能否只搬 DOM,把「抽共享 composable」留给真出现第二逻辑消费者的时刻(YAGNI)。** 仓内先例=UiDialog 恒 Teleport 到 body 而消费方逻辑不动。
- **width:max-content 容器测不出溢出**：useToolbarOverflow 的可用宽=容器 clientWidth;胶囊 max-content 时 clientWidth 恒等于内容宽,引擎永远判「装得下」。凡给浮动/自适应宽容器接折叠引擎,必须先加 max-width 约束+内层 flex:1 min-width:0,使 clientWidth 变成真实预算。
- **测量帧防闪的两种手段有适用面之分**：AppToolbar 的容器 overflow:hidden 兜底会裁掉一切越界内容——包括条外绝对定位的 tooltip(data-tooltip 弹在按钮上方、条外);改用测量帧容器 visibility:hidden 则 offsetWidth 仍可测(display:none 才归零)且不裁 tooltip。**有条外定位子元素(tooltip/badge)的折叠容器应选 visibility 方案。**
- **useToolbarOverflow 的 ResizeObserver 只在 onMounted 绑一次容器**：v-if 换壳(floating/docked 两形态互斥渲染)会让 observer 钉在死元素上、后续窗口 resize 失联。解法=每壳独立组件实例(SelectionActions.vue,mount 即 measure+observe),而非改共享 composable。
- **`bottom:32px` 的「safe-area 缺失」是误诊**：胶囊定位上下文 .media-grid-layout 在 .app-content 内,状态栏是 .app-content 的**兄弟** footer——胶囊物理上不可能与状态栏重叠,32px 纯审美间距。魔数的正确处置=补注释钉语义,非发明 safe-area 机制。
- **并行会话活跃期,「对话中产出的方案」到「落盘」之间须重跑地面真相核对**：本会话方案初稿(对话中)拟为溢出菜单手写第三个 AppToolbar 式弹层、显式不发明 UiPopover;数小时后落盘时核对发现并行会话续17 已交付 UiPopover 并清零全库手写弹层——初稿前提已失效,修订为直接消费 UiPopover(placement='top-end',焦点陷阱/backdrop/Esc/autoUpdate 免费)。re-verify before restating 不只适用于文档互引,也适用于**同一会话内跨时段的自家结论**。
- **工作树发现非本会话的文本错乱(已顺带修复)**：主设计 §7.1 line 219 存在未提交改动,把「新方向：selection bar 支持合并/分离一键切换」短语错位拼接进「现实指」中间(HEAD=c21d315 的版本完好),来源不明(疑并行会话或 IDE 误操作)。本会话按红线重写该 bullet 指向新方案文档,顺带消除错乱;另一在途文件(2026-07-12-工作记忆治理体系开源工具化方案.md)不属本线,未触碰未提交。

### 落盘后 Review（应用户要求核实并行会话相关代码,方案增补一处 a11y 一致性）

- **并行会话 f7684b0（filter a11y）核实结论=方案主体零冲突,补一处 disclosure 一致性缺口**：用户告知落盘期间并行会话有相关代码,核实 `f7684b0`（filter a11y）改动面=realtest §E + task_plan filter a11y 条 + AppToolbar.vue + GalleryFilterChips.vue **四文件**,均**非**我方案锚点(SelectionToolbar/useSelection/MediaGrid/AppStatusBar/useToolbarOverflow/UiPopover 全未触碰,useSelectionBarMode/SelectionActions.vue 尚未被抢先创建)→ 方案主体无需改。但它确立了**弹层触发钮 de-facto a11y 范式**:`aria-haspopup="true"`(date 为 `"dialog"`)+ `:aria-expanded=开阖态`,经 UiIconButton 单根 fallthrough 落根 button(UiIconButton.vue:8/54 证：aria-* 作普通属性透传,无需给原语加 prop)。**我方案初稿 §3.5 的 ⋯ 溢出菜单触发钮遗漏了这组 disclosure 语义** → 增补 §3.5(触发钮加 aria-haspopup+aria-expanded、瞬时命令钮不加 pressed)+ §4 C2(SSR 断言覆盖 disclosure 属性)+ §5 ⑦(读屏播报验收)。**判据:并行确立的 a11y/交互 de-facto 范式,同类新组件须主动对齐——「跟随既有约定」不只是代码风格,弹层 disclosure 是可被门禁(SSR)+真机(读屏)双验的可访问性契约。** task_plan 阶段11 两条 bullet(filter a11y〔并行改〕/selection bar〔我改〕)git 干净共存无冲突,selection bar 条已指向方案文档为单一真源故不重复回写增补细节。

## 会话续 19 补充发现（2026-07-13，selection bar C1–C5 施工落地）

- **Teleport 内容的 gate 条件必须与「接收方兄弟组件的让位条件」同源，否则 desync 出空白**：docked 形态由 SelectionToolbar 经 Teleport 把 DOM 投进 AppStatusBar 的 outlet，AppStatusBar 据 `docked && isSelectionMode` 让出 info 区。但 Teleport 的 gate 还含第三条件 `hostActive`（MediaGrid KeepAlive 活跃度）——**选区残留进查看器时（图片查看器沉浸是用户手动 toggle，非进入即隐 footer），Teleport 已随 hostActive=false 摘除，AppStatusBar 却仍按「两单例」让位 → outlet 既无操作条又无 info 的空白**。修法=把 `hostActive` 提升为 useSelectionBarMode 的**共享信号**（SelectionToolbar 于 onActivated/onDeactivated 写入），让 Teleport gate 与接收方让位**同读三条件**。**判据：凡「组件 A Teleport 内容进组件 B 的槽 + B 依共享态腾位」，A 的渲染 gate 与 B 的腾位条件必须逐条一致；跨组件的生命周期信号(如 KeepAlive 活跃度)要提到共享单例，不能一方持有本地 ref 另一方看不见。**
- **`Math.max(-0, …)` 产负零，污染 JSON 序列化与 `Object.is`/`toBe` 断言**：clampOffset 竖直钳制 `Math.max(-upSlack, Math.min(downSlack, y))` 在 upSlack=0 时返回 `-0`，`expect(-0).toBe(0)` 失败（Object.is 区分 ±0），且 `JSON.stringify` 会写出 `-0`（下次读回虽等价但值不洁）。**凡会被序列化持久化或做等值断言的坐标数学，末尾归一 `x === 0 ? 0 : x`。**
- **模板 HTML 注释渲染进 SSR 串——断言测量属性须收紧到属性形式并禁在注释里写裸 token（会话续16 教训的具体复现 + 精化）**：SelectionActions 的折叠流容器注释里写了裸 `data-toolbar-item`，SSR 串把注释也渲染进去，`/data-toolbar-item/g` 计数从 6 变 7。**双重修法：① 断言收紧到属性形式 `/data-toolbar-item[ >]/`（属性其后必跟空格或 `>`，注释里的 `(…data-toolbar-item)` 跟的是 `)` 不匹配）；② 注释不嵌裸属性/类名 token。** 附带地面真相：Vue SSR 把无值属性渲染为 `data-toolbar-item`（后跟空格），**不是** `data-toolbar-item=""`（首版断言据此误判为 0）——写属性形断言前先 dump 一次真实 SSR 串确认渲染形态。
- **prettier「`{` 后换行即保留多行」启发式，让禁-`--write` 的大 .vue 也能一次写对多行对象**：MediaGrid.vue 属三大 non-standalone-prettier-clean 文件（禁 `--write`），新增的 10 项 commands 数组若写成单行会超 printWidth 被 prettier 判脏、须 lint:fix（有 rewrap 整模板风险）。**解法：主动把对象字面量写成「`{` 后换行、每属性一行、末尾带逗号」的完全展开式——prettier 对「`{` 后有换行」的对象保留展开不回折，故新增代码一次即 ESLint-prettier-clean，无须对危险文件跑任何 autofix。**
- **折叠引擎（useToolbarOverflow）复用到第二消费者，验证「引擎-消费者」分离可迁移**：为 AppToolbar 建的 Priority+ 折叠引擎(纯核 computeOverflowSplit 已单测 + data-toolbar-item 测量契约)原样落进 SelectionActions，只换了**外壳的宽度约束**(胶囊 max-width 链 vs 工具栏 flex:1)与**测量帧遮法**(visibility vs overflow，因选区条有条外 tooltip)。引擎零改动。**印证：把「测量+切分」抽成纯核 + 属性契约、把「壳的布局/防闪」留给消费者，是折叠这类 DOM 逻辑的正确分层——第二消费者只需声明自己的宽度约束与遮法，不碰引擎。**

## 会话续 20 补充发现（2026-07-13，round6 真机反馈三修）

真机第六轮验收暴露 3 个门禁盲区问题，均属 DOM 布局/动画运行期行为（SSR/单测测不到）：

- **折叠引擎的「容器宽度语义」是隐性契约，内容宽容器会自锁棘轮（#1/#2 根因）**：`useToolbarOverflow` 默认假设它观察的容器**恒等于可用宽**——AppToolbar 的 `.toolbar__foldable` 是 `flex:1`（全宽），故 ResizeObserver 观察它、`clientWidth` 即真可用宽，变宽变窄都能被观察到。但 SelectionActions 的折叠流是 `flex:0 1 auto`（**内容宽**）：一旦折叠，流变窄→自身尺寸变化→但窗口再变宽时它**不跟着变宽**（flex-grow:0），RO 收不到任何信号 → **永不回弹**（真机症状：窄窗折叠后拉宽须刷新）。这是**折叠自锁棘轮**——折叠让容器变窄，变窄又让它保持折叠；刷新能修是因重挂载时窗口已宽、measure 从头算。**判据：把折叠引擎复用到内容宽容器时，必须显式声明 `containerFillsWidth:false`，引擎改走 window resize→完整 measure（测量帧内 isMeasuring=true 全渲染时，容器被父级 max-width 钳到的 clientWidth = 真可用宽，是内容宽容器唯一能测到可用宽的时机）。docked 变体同理（flow 在状态栏 outlet 内仍是内容宽），同一修复一并覆盖。**
- **measure() 必须在测量帧内捕获可用宽，不能在重隐藏后再读 clientWidth**：旧 measure() 尾部是 `isMeasuring=false; await nextTick(); recompute()`——recompute 读的是**折叠项已重新隐藏后**的 clientWidth。对 flex:1 全宽容器无碍（clientWidth 恒定），但对内容宽容器，此刻 clientWidth 已塌回折叠内容宽 → 读到错值 → 无法展开。修法=在测量帧内（全渲染时）就地捕获 `available = clientWidth − paddingX − reserved`，关帧后直接用捕获值 `applySplit`。对 flex:1 容器帧内/帧后 clientWidth 相同，**行为完全向后兼容**（AppToolbar 零回归）。
- **@floating-ui 默认 transform 定位与 CSS 过渡的 transform 互斥，且首帧未就位会「从左上角飞入」（#3 根因）**：@floating-ui/vue 默认用 `transform: translate(x,y)` 定位。两个后果叠加成真机观感「弹层从画面左边很远飞入」：① 定位算出前首帧 transform=`translate(0,0)`=视口左上角(0,0)，随后跳到锚点，0.15s 过渡把这段跳跃演成飞入；② 过渡里想用的 `translateY(-4px)` 被内联定位 transform（更高优先级）压掉，等于入场动画从未真正生效。**三件套修法：①`useFloating({ transform: false })`→用 top/left 定位，把 transform 整个让给动画的 scale；②取 useFloating 返回的 `isPositioned`，未就位时内联 `visibility:hidden`，消除左上角闪现；③取解析后 `placement` 推 `transform-origin`（top-*→底边、bottom-*→顶边、-start/-end→左右锚边），`scale(0.9)→1` 从贴锚点那条边生长=像从触发按钮弹出。** 一处改动令全库弹层（date/filter/view/选区 ⋯）同时受益，方向随各自 placement 自适应（选区 top-end 上弹、顶栏 bottom-end 下弹），且 flip 翻转方位时原点跟着翻。**印证 UiPopover「持有契约、委托实现」的原语红利：修一处，所有消费者一致改善。**
- **测量帧的 isMeasuring 双改在同一微任务批内完成，window-resize 重测无闪烁**：担心 window resize→measure 会让折叠流每帧 `visibility:hidden` 闪烁，但 `isMeasuring.value=true → await nextTick() → 读宽 → isMeasuring.value=false → applySplit` 全程是 microtask，浏览器在 microtask 队列排空后才 paint，故只 paint 最终态。读 offsetWidth 触发的是内部 forced reflow（非 paint），对 ~10 项工具栏 sub-ms，且仅在选区激活 + 窗口拖拽 resize 的边缘交互期发生。rAF 节流兜一次/帧。

## 会话续 21 补充发现（2026-07-13，两栏对齐设置 + 弹层卡片动画两新需求）

用户两新需求：①折叠 ⋯ 弹层改卡片弹出(类 Win11) + 居中对齐按钮；②顶栏/选区栏默认靠中 + 可选居中/靠左/靠右(两栏各一)。三处产品岔路先经 AskUserQuestion 定夺(顶栏只挪中部 chips 簇 / 胶囊对齐=默认位拖拽仍可覆盖 / 两栏各一设置)，再施工。

- **同一「折叠引擎」在全宽容器 vs 内容宽容器上，`justify-content` 对齐的可行性截然不同——这是决策①能小改落地的关键**：AppToolbar 的 `.toolbar__foldable` 是 `flex:1`(全宽)，加 `justify-content:center` 后 items 自然宽、多余空间即留白，窗口收窄先吃留白(items 不变)、容器 < 内容才触发 useToolbarOverflow 折叠——**「先减留白再折叠」是 flex justify + 溢出折叠的天然复合，零额外逻辑**。而选区浮动胶囊是内容宽壳(会话续20 的自锁教训)，其对齐靠**外层 wrapper**(`.selection-toolbar-wrapper`,left:0 right:0 全宽)的 justify-content 实现，胶囊只是其内的 flex item。**判据：「工具簇对齐」应作用在包住簇的那个全宽轨道上，而非簇自身；全宽轨道的 justify 空间就是留白，与折叠引擎正交(引擎测的是内容能否放下，不管 justify 怎么摆)。**
- **可拖拽元素引入「对齐默认位」后，钳制数学必须把对齐基位 baseLeft 纳入，否则非居中对齐的可拖范围算错**：旧 clampOffset 假设胶囊居中、可动范围对称 ±halfSlackX。加对齐后，靠左的胶囊静止在左侧、几乎只能右移，靠右反之——**对称假设仅对居中成立**。泛化=先据 align 算静止基位 baseLeft(居中=(bounds−capsule)/2 / 靠左=SIDE_INSET / 靠右=bounds−capsule−INSET)，再把 x 合法区间定义为「使胶囊左缘落在 [margin, bounds−capsule−margin] 的位移集」= `[margin−baseLeft, bounds−capsule−margin−baseLeft]`。**关键手法:让 align 参数缺省 'center'，且此时公式与旧对称式逐值恒等(数学验证:baseLeft=(bounds−capsule)/2 代入即得 ±((bounds−capsule)/2−margin))→ 既有 3 参调用与 14 条居中单测零改动通过，新增只加靠左/靠右用例。改钳制数学时,「新默认路径与旧行为逐值等价」是安全泛化的黄金判据。**
- **对齐基位变更须清空旧拖拽偏移(否则偏移叠加到新基位会把元素推到意外位甚至屏外)**：offset 是「相对当前对齐基位的位移」。切对齐=换基位，同一 offset 叠到新基位语义已变。故 watch(align) → offset 归零 + 清持久化 + 下一 tick 再钳。**判据：当一个持久化的相对量(offset)的参照系(baseLeft)会被另一设置(align)改变时，改参照系必须同时复位相对量，不能让旧相对量静默跨参照系漂移。**
- **CSS 视觉内缩与钳制数学的基位常量必须同源**：wrapper `padding-inline:12px`(靠左/靠右呼吸)与 clampOffset 的 `SELECTION_BAR_SIDE_INSET=12`(算 baseLeft)是同一个 12——一改一漏则 CSS 静止位与钳制基位错位、拖拽边界与视觉不符。已把常量 export + 两处注释互指同源(CSS 侧无法直接读 TS 常量，靠注释锁定)。
- **三态选择设置用 `control:'select'` 走既有泛型分派即可，不必造 segmented**：DynamicSettingControl 的 segmented 目前硬编码仅 thumbSize；而 select(下拉)是全泛型(selectBindings + settingsMap.options 声明式驱动)，加两项零模板改动。且 Win11 任务栏对齐设置本身就是下拉——select 反而更贴系统习惯。**判据:三态/多态设置优先用既有泛型 select，除非交互强需分段按钮的即时可视对比；不为「更好看」造一次性控件分支。**
- **原语层动画一处改、全库弹层受益的第二次兑现**：需求①的卡片动画(scale from origin + 朝锚点侧 translateY，--ui-popover-pop-y 随 placement 侧定正负)与居中对齐(placement -end→无后缀)都在 UiPopover + 各消费者 placement 上落地；date/filter/view/选区 ⋯ 四弹层同时获得卡片观感，方向随各自 placement 自适应(承会话续20 结论)。

## 会话续 22 补充发现（2026-07-13，round7 真机反馈：docked 态对齐未生效修复）

用户真机报「底部工具栏的对齐设置未生效」。诊断=**浮动态对齐本来就生效**（对齐 class 在 SelectionToolbar 的 `.selection-toolbar-wrapper` 全宽轨道上），但用户 round6 测过「合并到底部状态栏」，持久化 `selection_bar_docked='1'` 使条处 **docked 态**——此时浮动 wrapper 因 `v-if="!docked"` 根本不渲染，SelectionActions 被 Teleport 进状态栏 outlet，那条 DOM 路径上**没有任何对齐宿主** → 改设置无作用。修法=让 SelectionActions 读同一个 `mode.align`，在 **docked 变体**上对填满 outlet(`flex:1 1 auto`)的 `.selection-actions` 施 `justify-content`，把「同一对齐语义」补齐到 docked 宿主。

- **同一设置语义在多形态下落在不同宿主容器时，新增该设置必须覆盖每个形态的宿主，漏一个即该形态静默失效**：选区栏有浮动/docked 两形态，round7 只把对齐 class 挂到浮动壳的 wrapper；docked 态是**另一条 Teleport DOM 路径**（进 AppStatusBar 的 outlet），对齐 class 没跟过去。这类「一个设置、多个渲染分支」的漏配没有编译期/SSR 信号（两分支各自能渲染、都不报错），只在用户切到被漏的形态时暴露。**判据：为一个跨形态共享的状态(align)加视觉落地时，先枚举该状态的所有渲染宿主(浮动 wrapper / docked outlet)，逐个确认落地，而非只改「默认那条路径」。**
- **realtest 里标注「有意范围外」的项，若用户实际就在那个形态里操作，它是缺口不是豁免**：会话续21 的 realtest §D 明写「docked 态对齐不改布局，属本轮有意范围」——但用户的「底部栏」心智模型恰是 docked 态（字面就在底部，浮动胶囊反而是悬浮的）。**判据：把某形态划出验收范围前，先问「用户默认/常用的是哪个形态」；被最常用形态命中的『有意范围外』等于把缺口写进了验收豁免。已按红线回写 §D 正文。**
- **docked 对齐的「轨道」是 outlet 区而非整条状态栏**：docked 动作簇在 `#statusbar-selection-outlet`(状态栏左侧、版本号之前，`is-active` 时 flex:1)内对齐，故居中/靠右是在该可用轨道内对齐，非整条状态栏几何中心(版本号恒在最右)。与浮动态「在 left:0 right:0 全宽 wrapper 内对齐」是各自轨道内对齐的一致范式——**对齐永远相对「元素所在的那条可用轨道」，两形态轨道不同（全窗 vs outlet 区）是可接受的语义差**。折叠测量不受 justify 影响：折叠流 flex-grow:0，自由空间只进 justify 留白不进流内，收窄先吃留白再折叠，与浮动态一致。

## 会话续 23 补充发现（2026-07-13，S1 阶段7 UiToolbar 决策收口 + S2 阶段8 视图路由化/URL 同步）

### S1:UiToolbar「不建组件」决策的地面真相

- **UiToolbar 与 UiButton/UiDialog 的本质差别 = 有无「可包裹的共享外观」**:UiButton 包裹全局 `.btn`、UiDialog 包裹 `.dialog-overlay` → 迁移即摊薄、视觉零风险。而三 toolbar(AppToolbar/ContextualToolbar/SelectionActions)**无任何共享全局 `.toolbar` 类**(Explore 勘查:`src/**/*.css` grep `.toolbar` 零命中,各自 scoped;真正跨组件共享的是 `data-toolbar-item` **属性契约** + 两个 composable,不是 CSS)。无共享外观壳 → 建组件无摊薄价值,只会为统一而统一。**判据(承 UiField 会话续4「造原语前先量真实杠杆」):原语价值 = 能否同构包裹一层共享外观;无共享外观时,复用应停在 composable 层(逻辑),而非升到 component 层(结构+外观)。**
- **复用逻辑早已在 composable,缺口是 a11y 而非抽象**:`useToolbarOverflow`(computeOverflowSplit 8 测,消费者 AppToolbar/SelectionActions)+ `useRovingTabindex`(nextRovingIndex 14 测,消费者仅 ContextualToolbar)已是完整基建。三 toolbar 各缺标准工具栏契约的不同部分(ctx 缺 overflow / AppToolbar/选区缺 role+roving),但**这些缺口用现成 composable + 加属性即可补,不需要新组件**。
- **roving 只有一个干净候选 = ContextualToolbar,已在 S3 做完**:roving 的正确性前提是「成员为同质可聚焦控件」。核实三 toolbar 后:
  - **ContextualToolbar** = 清一色 UiIconButton,`data-toolbar-item` 直接挂在按钮、逐项绑 `:tabindex="tabindexFor(i)"` → roving 干净,已交付。
  - **AppToolbar** 折叠区含 `select`(分组/排序)+ `range` 滑块(行高)——这些控件**自己要吃方向键**,套 roving 会与其键盘交互互搏 → 不适合 roving。
  - **SelectionActions** 亦非纯钮:`colors` 命令渲染 `ColorLabelPicker`=**内联 8 个色块 button** 的复合控件;且 `data-toolbar-item` 挂在 `.fold-item` 包裹 span(overflow 需连 groupStart divider 一起量),而 roving 需成员为可聚焦元素并逐项绑 tabindexFor → **overflow 测量契约与 roving 成员契约挂在不同元素上,相撞**;色块又在子组件 ColorLabelPicker 内部(父无法绑 tabindexFor)、固定件(停靠/✕)在折叠流容器外。要正确套 roving 须改「命令式 tabindex 管理」新 composable + 让子组件参与,且 roving 本质 ⏸GUI(只能真机键盘验)。
  - **结论**:S1 落地 = SelectionActions 加 `role="toolbar"` + 可访问名(`selection.actionsToolbar`,读屏分组播报;镜像 ContextualToolbar「role 先落地、roving〔S3〕后补」的轨迹),**不强套 roving**;findings 收口本分析;「SelectionActions/AppToolbar roving」标为需新基建 + 真机键盘验的后续项(与 ContextualToolbar roving 同属 ⏸GUI)。
- **模板注释含被断言的字面 token 会污染 SSR 计数**(再次踩会话续6 陷阱):给根加 role 时注释里写了字面 `data-toolbar-item`,而「每命令一折叠单元」用 `/data-toolbar-item[ >]/g` 计数 → 注释文字被多计一次(`expected 7 to be 6`)。组件既有注释一律称其为「折叠标记属性/测量对象」而不写字面 token,正是此约定。修法=注释改述、不嵌字面 role/折叠标记 token。**判据:模板注释会被 SSR 渲进串,凡 spec 计数/负向断言的字面 token 都不得出现在模板注释里。**

### S2:视图路由化 + URL 同步的地面真相(施工输入)

- **/favorites、/trash、/folder/:id 是「僵尸路由」**:已注册、`isGalleryRoute` 已认作画廊路由,但全库**零 `router.push` 导航到它们**、App.vue view watcher **也不从它们回填 viewStore**。smart-album/folder 视图态当前完全由 viewStore(store 单源)驱动、URL path 恒 `/`。真正路由化 = 补导航侧(FoldersSection/LibrarySection 改 push 路径)+ 回填侧(App.vue watcher 加分支)。live-photos/recent **连路由壳都没有**,须补。
- **folder 双模的干净切割线**:`groupBy==='folder'` 点文件夹 = 设 `ui.pendingScrollDirId` 滚动锚点(activeDirectoryId 恒 null、getViewKey 恒 `album-all`),画廊仍是全库单列表;`groupBy!=='folder'` 才 `setActiveDirectory`(筛选、getViewKey=`dir-<id>`)。**两模都最后 push '/'**。切法:模式B → `/folder/:id`(可寻址筛选,getViewKey 已 dir-<id> 分桶、scrollCache/KeepAlive 零改造);模式A 保持 '/' + pendingScrollDirId(滚动位置非视图,进 URL=每滚改 history)。**模式A 永不导航到 /folder/:id、模式B 才去 → 两模天然不撞,无需在 /folder/:id 里分模判断**。
- **侧栏高亮双源(不改)**:folder 模式读 `ui.scrolledDirectoryId`(滚动派生、高频),非 folder 模式读 `viewStore.activeDirectoryId`。scrolledDirectoryId 属滚动态,**不进 URL**(与模式A 不路由同因)。
- **App.vue watcher 监听 path 而非 fullPath 是有意的**:隔离 S2-b filter 的 `router.replace({query})` 不误触发 view 回填(否则切筛选就 clearSelection+重水合)。故 view 维度走 **path 段**(/folder/:id、/favorites…)、filter/view-pref/search 走 **query**——两者分层,watcher 继续只认 path 段;新增 folder/smart-album 回填分支须加 token/相等守卫(`if activeDirectoryId!==id 才 set`)防 clearSelection 重入(collection/person 已踩过、已有 viewRouteToken 范式)。
- **view-pref/search URL 同步的竞态(S2-b2 施工核心)**:group/sort/order/layout 在 uiStore、经 `SET_APP_CONFIG` 持久化;水合在 `startupConfigPromise.then` **异步**赋值。`useGalleryQuerySync.hydrated` 门只等 `router.isReady()`、**不等 startupConfigPromise** → 一旦把 view-pref 加进 URL,persist 的 `.then` 晚到会覆盖已从 URL 水合的值。**修:hydration 门改等 `Promise.all([router.isReady(), startupConfigPromise])`**。另:aiStore 语义/混合模式**临时**改 groupBy='none'/sort='similarity'(全传 persist=false),退出用 previousGroupBy 复位 → view-pref URL 同步须**镜像 persist=false 语义**(临时态不写 URL),否则语义搜索一开 URL 就被污染。`sortOrder` 无 setter/无 persist/无 startup key(恒 'desc')——URL 同步须先补 `setSortOrder` 或跳过(施工时定)。
- **search URL 同步是高风险面(用户在环已选做)**:searchStore 是门面(committedQuery/mode/scope 投影 ui/ai、commit* 委托 aiStore action)。restore mode 会触发 `aiStore.setSearchMode` 的 group/sort 副作用 + 语义 IPC + searchToken 代次守卫 → 须用代次 + 相等守卫做稳,单独标真机验收重点(门禁验不了语义 IPC 实际行为)。

### S2-c 施工期补充发现(Stage 2 落地)

- **「僵尸路由激活」的连带 clobber 面 = 所有此前 push('/') 的目录选中站点**:FoldersSection 有三处「选中目录作筛选」入口——onNodeClick 模式B、addRoot 后自动选中(scanRoots watcher)、move 后自动选中(reloadTreePreserveExpansion),此前全部 `setActiveDirectory(id) + push('/')`。S2-c 给 watcher 加了 `'/'→setSmartAlbum('all')` 回填后,这三处若仍 push('/'),watcher 会**把刚设的 directory 清成 all**(自我抵消)。**判据:给某 path 加「回填 store」的 watcher 分支时,必须反查全库所有导航到该 path 的站点,确认它们导航后期望的 store 态与回填态一致;否则「先设 X 再导航到会回填成 Y 的 path」= 静默自我抵消**。修法=抽 `navigateToFolder(id)` 统一走 `/folder/:id`,三站点共用(既 DRY 又杜绝漏改)。这类 bug 无编译/类型信号,只在真机点击时暴露。
- **原绑 `route.path === '/'` 的 UI 须随视图拆分平移,否则在新路径静默失效**:SemanticSearchPanel 此前 `v-show="route.path === '/'"`——因 S2-c 前 favorites/trash/live/recent/folder 全停在 '/',这判据实际含义是「主库血统视图」。拆成多路径后 `=== '/'` 会让面板在 /favorites 等**静默消失**。抽 `isPrimaryGalleryRoute(path)=routeToView(path)!==null` 精确复现「拆分前在 '/' 的视图集合」(smart-album + folder 筛选,不含 collection/person 详情——它们此前即非 '/')。**判据:路由拆分时,grep 全库 `path === '<被拆的旧路径>'`,逐个判断是「该跟随新细分」还是「本就只指旧路径」,平移到语义判据而非留裸字面比较。**
- **RouterView 无 :key + KeepAlive 只保活 MediaGrid = 多路径复用同实例、零重挂**:`/`、`/favorites`、`/folder/:id` 等都解析到同一 MediaGrid 组件,RouterView(`App.vue:55`)无 `:key` 故不强制重挂,组件复用、靠 viewStore 变化重布局(与拆分前全在 '/' 时一致)。故 S2-c 不引入滚动位/重取回归——但这依赖「RouterView 不加 key」这一前提,若后续为其它需求给 RouterView 加 `:key="route.path"` 会瞬间把每次 smart-album 切换变成重挂,须联动评估。
- **完备相等守卫必须覆盖全部互斥维度,不能只比「主维度」**:watcher 回填 smart-album 时,若只比 `activeSmartAlbum !== album` 会漏一个 case——从 /collections/5 导航到 '/' 时 activeSmartAlbum 恰为 'all'(setActiveCollection 把它置 'all'),只比 album 会误判「已是 all」而跳过,导致 activeCollection 不被清。故守卫须连 `activeDirectoryId/activeCollection/activePersonId === null` 一起比。**判据:相等守卫的「相等」= 目标态的完整快照相等,不是主字段相等;互斥四维度里任一非空都意味着「当前不是纯粹的目标态」。**

### S2-b2 施工期补充发现(Stage 3a/3b 落地)

- **「per-view」有两种不同强度的实现,差在导航语义、代价悬殊**:用户裁决「per-view、URL 权威覆盖持久值」。
  拆开看是两个诉求:①**URL 权威覆盖持久**(深链/刷新时 URL 值盖过 persist)——低成本,decode 返 Partial +
  applyViewPref 只赋出现的键 + persist=false 即得;②**逐视图独立**(视图 A 的 group 与视图 B 互不影响)——
  **高成本**,因为它要求「切视图时各读各的 URL」(read-on-navigation),而 filter 是「切视图带着筛选走」
  (carry-on-navigation),二者在导航时**一个该读一个该写、正好相反**,混在一个同步层里矛盾;且要让工具栏
  改动不落全局 persist(改 toolbar 持久语义=UX 契约变更)。本轮实现**「全局携带 + URL 恢复/覆盖」**(view-pref
  像 filter 一样全局携带、URL 恢复、覆盖 persist),完整兑现①;②(纯逐视图独立)标为待真机 UX 判定的后续,
  不擅自冻结(符合「开发期不冻结契约」)。**判据:「per-view」是模糊需求,落地前须拆成「URL 恢复/覆盖」与
  「逐视图独立」两级——前者是序列化问题、后者是导航语义 + 持久契约问题,别把后者的成本悄悄摊进前者。**
- **持久化维度的 URL 同步,hydration 门必须等 persist 源、且用 nextTick 去序依赖**:filter 无 persist,水合门
  只等 `router.isReady()` 即可;但 view-pref 有 persist(startupConfigPromise.then 异步赋值)。若 readUrl 不等
  startupConfigPromise,persist 的 `.then` 晚到会覆盖已从 URL 水合的值(反了)。修 = 门改 `Promise.all([isReady,
  startupConfigPromise])`;更隐蔽的是**注册顺序依赖**——uiStore 的 persist `.then` 与我的 `.then` 都挂在同一
  promise 上,谁先跑取决于注册顺序。用 `.then(() => nextTick())` 再退一拍,保证 uiStore 的 persist 赋值微任务
  已 flush 后才 readUrl,不赌顺序。且 `startupConfigPromise.catch(→null)` 放行,防配置 IPC 失败卡死整个 URL 同步。
  **判据:给「有异步 persist 水合」的状态加 URL 同步,门 = router-ready ∧ persist-ready,且用 nextTick 消解
  同 promise 上多个 .then 的注册顺序不确定性。**
- **临时态(语义搜索的 group/sort 覆盖)绝不能进 URL——不只是「脏」,会污染 previousGroupBy**:语义搜索把
  groupBy/sort 临时改 none/similarity 并存 previousGroupBy 以便退出时复位。若 writer 把这个 transient 写进 URL,
  刷新时 readUrl 以 URL 的 group=none 覆盖 persist,而随后语义恢复会把「已是 none」的 groupBy 存进 previousGroupBy
  → 退出语义时复位到 none(错)。修 = writeUrl 在 `ai.isSemanticMode` 时删 group/sort 两键(退出语义后 groupBy
  复位、writer 自然写回真实值)。**判据:凡「有临时覆盖 + 覆盖前值需被记忆以便复位」的状态,序列化层必须识别并
  跳过临时态,否则持久化/URL 会把临时值固化成复位目标,复位链断裂。**这也是 uiStore 早有的 persist=false 参数
  (临时切换不落持久)在 URL 层的对偶——同一「临时态不外泄」原则,persist 和 URL 两个出口都要堵。
- **search 恢复是唯一会在水合期触发 IPC 的同步项,恢复顺序决定 previousGroupBy 正确性**:filter/view-pref 恢复
  都是纯 store 赋值;而 search 恢复经 searchStore.apply → aiStore.runSemanticSearch 触发**语义 IPC**。恢复顺序
  定为 filter→view-pref→search:使 setMode 的语义 group 覆盖发生在 view-pref 赋值**之后**,让 aiStore 存
  previousGroupBy 时捕获的是 URL/persist 的真实 group 而非默认。这段 IPC 行为门禁验不了(纯函数只覆盖编解码),
  属真机验收重点(见 realtest-round8 §C)。

## 会话续 25 补充发现（2026-07-15，S7 token 引用闭环）

### 地面真相：`--color-*` 引用闭环双向断裂

全库 `--color-*` 做集合差（**剥注释后**），两个方向都破：

| 方向 | 数量 | 后果 |
|------|------|------|
| **幽灵**（消费但无定义） | 4 token / 5 消费者 | 带 fallback→静默走死值不随主题；无 fallback→整条声明失效（渲染透明/继承色） |
| **死票**（定义但无消费） | 5 token × 6 主题 = 30 条声明 | 不渲染任何像素，却让 check:contrast 为不上屏的值背书 |

修复后 **49 定义 = 49 消费**，双向硬门锁死。

### 教训 ①：注释里的历史 token 名会顶穿任何正则扫描（本仓第三次踩）

本仓修幽灵时按约定在注释留旧名作历史记录（`/* 原 var(--color-danger) 为幽灵 token(S5 修) */`）——那些恰是
**已修站点**。我的初版扫描器与 Explore 测绘都把它们当活引用，误报 **5 幽灵/11 消费者**，真值 **4/5**
（`--color-primary`、ProofreadPanel/ReplacementPanel/VersionPanel 的 `danger`、PdfReader 的 `bg-base` 全是假阳性）。

**同族前科**：会话续 16「模板 HTML 注释渲进 SSR 串使负向断言恒失败」、会话续 23「注释里的字面
`data-toolbar-item` 顶穿折叠单元计数」。**判据**：任何基于正则的扫描/门禁，若被扫面允许注释，
必须先剥注释——否则工具**反噬良好注释**（写历史记录反而触发永久误报），这是最坏的激励。
门禁的 `stripComments` 刻意**只剥不解析**，且 `//` 规避 `://` 防吃 URL：宁可漏判（少报一个幽灵）
也不误判（把已修站点报红）——假阴性只是少抓一个，假阳性会让人关掉门禁。

### 教训 ②：孪生实现里带 fallback 的那一路会掩盖幽灵，使 bug 只在另一路显形

`--color-badge-size` 是最典型的一例：

- `MediaThumb.vue:639`（DOM）：`background: var(--color-badge-size);` **无 fallback** → 六主题全透明 → 白字直压照片。
- `MediaGridCanvas.vue:417`（canvas）：`g('--color-badge-size', 'rgba(0,0,0,0.6)')` **有 fallback** → 渲染正确。

于是 canvas 那路**看起来一直是对的**，DOM 那路坏了很久没人发现——两路观感不同却无人报，因为 canvas 网格是实验开关。
**修法反过来利用了这一点**：六主题的新定义直接取 canvas 的 de-facto 回落值 `rgba(0,0,0,0.6)`，
于是 DOM 收敛到「本就正确的那一路」、canvas 读到的值与原回落值逐字节相同 = **零变化**，消费端一行没改。
（同一蒸馏范式：UiCheckbox 蒸馏 `.remember-checkbox` de-facto 视觉，会话续 11。）

### 教训 ③：死 token 让门禁为不上屏的值背书——门禁可信度是独立于覆盖率的属性

`check:contrast` 的 21 对硬门里有 `--color-text-placeholder × bg-surface ≥ 3.0`，年年绿。
但**两处真实 `::placeholder` 规则用的都是 `--color-text-tertiary`** —— 那个 token 零消费。
门禁在**为一个不渲染的值背书**：报告「占位符对比度合格」，而屏幕上的占位符是另一个值渲染的
（tertiary 另有门覆盖，故无真实 a11y 洞，但门禁在说与画面无关的事）。

这类问题不体现为覆盖率下降，只体现为**可信度**下降，且靠加测试发现不了——只能靠「定义面 ≡ 消费面」这种
**闭环不变量**。故本轮选择**接线而非删除** placeholder：接上后该门对由幻影变真，比删掉更优。

### 教训 ④：用户裁决的前提被地面真相推翻时，带证据回报重裁，不硬推（本轮连翻两次）

- **第一次**：用户裁「5 个死 token 全接线」。逐个核对靶子后发现 4 个**没有可接的靶子**——
  `--color-badge-video` 的同名消费者 `.badge-video` 是**居中播放键**（`position:absolute` +
  `translate(-50%,-50%)` + `<Play>` 图标）而非角标（兄弟 `.badge-live` 才是 `position:static` 的角标行成员），
  接线 = 把播放键涂成浅蓝；`badge-audio`/`badge-document` 连元素都不存在，「接线」= 凭空发明 UI；
  `--color-accent-dim` **六主题取值给不出一致语义**（若为按下态，该在每套主题沿 hover 同方向再走一步，
  实际 4/6 反向）→ 意图不可考，接线 = 现编。**带证据重问 → 用户改裁**。
- **第二次**：用户裁「type 元素键默认开」。查实 `thumbInfoElements` 默认 `[]`、`showThumbInfo` 默认 `false`、
  **现有 10 个元素键无一有默认值**（纯 opt-in 模型），唯 LIVE 徽章不受总开关管 →「进 elements 面板」与
  「默认开」在这套模型里**自相矛盾**（进面板即 opt-in）。我初问给的选项本身失真。
  **带证据重问 → 用户裁「进面板 + 总开关默认设为开」**。

**先例**：目录排序线 D-018「施工前核实反转时区前提 → AskUserQuestion 报证据 → 用户改选 A′」。
**判据**：裁决基于我给的描述；描述失真时，执行「用户的原话」反而背叛「用户的意图」。

### 教训 ⑤：默认值翻转要连坐审计所有消费者——`showThumbInfo` 背后挂着 IPC

把 `showThumbInfo` 默认翻成 `true` 看似纯 UI，实则 `MediaGrid.vue:1843` 有条 `immediate:true` 的 watcher：

```
if (!ui.showThumbInfo) return
… media.ensureMeta(ids)   // → get_meta_for_viewport:EXIF/GPS/名称/路径
```

而 `ensureMeta` **无守卫**（照单收下每个可见 id，120ms 后 flush）。默认翻转 = **所有用户每屏拉重型元数据**，
而 `elements=[]` 意味着拉回来无人渲染——直接违 A1「元数据从常驻布局缓存剥离、按需供给」的本意与项目性能优先。

**且此 bug 今天就存在**：开了总开关但只勾 `size`/`status`（纯 item 字段）的用户同样在白拉。
修法 = `needsViewportMeta(elements)`（取用面 = filename/path/geo/camera/params，**与 `buildThumbInfoLines`
里的 `meta?.x` 取用面一一对应，spec 对拍钉死防两处漂移**）+ 把元素列表补进 watch 源（否则中途勾上 camera
要等行变化才补拉）。**可证明安全**：`viewportMeta` 全库唯一终点是 `buildThumbInfoLines`（已逐消费者核实）。

### 设计：新增两路共享条件时，把判定提成单源而非逐字重写第二遍

`MediaGridCanvas.vue:937` 原注释：「徽章收集（**条件逐字对齐 DOM 模板 v-if**）」——两路一致性靠人肉守约。
而刚修的 `--color-badge-size` DOM/canvas 分叉**正是这类漂移的产物**。故类型角标的判定提为
`helpers.typeBadgeOf`（照 `TEXT_CARD_FORMATS`/`docBadgeKind` 既有共享范式），两路同调 —— 漂移从构造上消除，
而非靠注释叮嘱。纯函数身份也让「哪些类型该出角标」这条产品规则获得可穷举的 spec。

## 会话续 26 补充发现（2026-07-15，S7 chunk 治理）

### 地面真相：被告警的两个块全是误报，而真风险告警根本不管

| 块 | 体积 | `isDynamicEntry` | gzip | 进首屏？ |
|---|---|---|---|---|
| `cpp-*.js` | 637.55 kB | **true（懒加载）** | 47.22 kB | ❌ 阅读器高亮 C++ 时才取 |
| `index-*.js` | 549.75 kB | false（入口） | 184.84 kB | ✅ 16 路由全懒加载后的外壳 |

`pdfjs-dist`/`shiki`/`@shikijs/*` **零模块在入口**（`@shikijs/langs` 全部 1943.2 kB 均在懒块）——
即**当前架构本就正确，没有可修的东西**。

### 教训 ①：区分「告警」与「门」——前者在噪音中等于不存在

实测对照（同一回归：`main.ts` 静态 `import 'pdfjs-dist'`，首屏 +365 kB）：

| 机制 | 输出 | 退出码 | CI 后果 |
|---|---|---|---|
| Rollup `chunkSizeWarningLimit` | 多喊一句 | **0** | **绿，无人察觉** |
| 本轮打包预算门 | 指名 `pdfjs-dist` + 首个模块路径 | **1** | 红，阻断 |

更毒的是叠加：**常年两条误报会训练所有人无视此类告警**，于是真回归多喊的那一句也淹在噪音里——
狼来了效应本身就是伤害，而非仅仅"没用"。这与 S7 上一轮「幻影对比度门为不上屏的值背书」同源：
**门禁可信度是独立于覆盖率的属性**（`docs/experience.md`）。

### 教训 ②：为不可控的上游体积设阈值 = 制造下一个幽灵门禁

最大懒块（shiki 的 C++ 语法）体积由上游决定，我们既不控制也无从优化。给它设上限的话，
**唯一可能的响应就是「把数字调大」**——那正是本仓认定的反模式。故有意只报告不拦截：
保住可见性，但不制造一个注定被调大的数字。判据：**设阈值前先问「红了我能做什么」；
若唯一答案是「改阈值」，就别设。**

### 教训 ③：门禁自身的量测口径必须与被信任的报告逐字节对齐（本轮被自己的门捉到两次）

| # | 错误 | 后果 |
|---|---|---|
| 1 | 用 KiB（`/1024`）而 Vite 的 kB 是 SI（`/1000`） | 落盘 549,751 字节报成 536.87，**预算比字面值悄悄松 2.7%** |
| 2 | 在 `generateBundle` 量 `chunk.code` | 548,106 vs 落盘 549,751，**少算 1,645 字节**；加 `enforce:'post'` 也不消失 |

②的处置有普适性：**与其逐个追查是谁在后面改写 `code`，不如直接量发货物**——
改在 `writeBundle` 里 `statSync` 盘上文件，对任何未知后处理免疫。
判据：**门禁量的东西与真正发货的东西之间，每多一层推断就多一处可静默失准的缝**。
①已锁进回归测试（用实测字节数 549,751 断言报出 `549.75`）。

### 已排除的路径（实测，勿重复尝试）

| 尝试 | 实测收益 | 结论 |
|---|---|---|
| `__VUE_OPTIONS_API__: false` | **-4.31 kB** | 全库 79/79 SFC 皆 `script setup`、`vue-i18n legacy:false`，安全但收益是噪音 |
| `__VUE_PROD_DEVTOOLS__: false` | **0** | 已默认 false；entry 里 4 个 devtools 模块**不受该开关管**（四种配置下纹丝不动） |
| `__VUE_PROD_HYDRATION_MISMATCH_DETAILS__: false` | **0** | 已默认 false |
| `manualChunks` 拆 vendor | **首屏 0** | **Tauri 本地加载无 HTTP 缓存**，拆分不减解析字节、只增文件数 = 纯装饰性消告警 |
| 懒加载 `PerformancePanel`/`OnboardingWizard` | ~15-18 kB minified ≈ **1ms 解析** | 教科书式 async 候选，但在 Tauri 下收益不材料化 → 不做（做了是 cargo cult） |

## 会话续 27（2026-07-15）：S7 收官 —— 6 主题视觉矩阵（round9 真机验收通过后）

### 落地形态：捕获器，不是 pixel-diff 基线门

「6 主题视觉矩阵」此前一直挂着「真机」标签。实际上它**大半可自动化**：本仓早有先例
（会话续：headless Chrome + `?ui-harness=` 做无 Playwright 的 DOM 断言），把 harness 加一个
`&theme=<id>` 维度即可出 6 主题 × 3 场景 = 18 张。`npm run capture:themes`。

**有意不做 pixel-diff 基线门**：字体栅格/抗锯齿/Chrome 版本任一变动即红，而红了唯一可能的
响应是「重新生成基线」= 幽灵门禁反模式（`docs/experience.md`，与 chunk 那轮同一判据）。
故机器只干**捕获**这件苦力，判定留人眼：token 契约（消费≡定义）与 check:contrast 已各有真门，
人眼要抓的是它们**证明不了**的那类。

两处设计选择值得记：

| 选择 | 为什么不选反面 |
|---|---|
| 主题经 `startupConfig` 覆盖，**不直接写 `data-theme`** | 直接 poke DOM 会让截图为**产品里不存在的路径**背书；经此则完整跑真实链（`normalizeThemeId` → `resolvedThemeId` → `applyAppearance` 单点写），非法 id 行为亦与生产一致 |
| 主题 id 取自 `themes/*.css` **文件名**，不解析 `registry.ts` | 二者等价有硬门担保（`theme-contract.spec` 钉死 `cssById.keys() === registryIds`）→ 零解析零漂移，新增主题自动纳入矩阵。**能靠既有硬门推导的等价，就别再写一个解析器** |

### 首个战果：网络存储卡盖掉全局卡（六主题全中）

设置页 9 张卡里唯独「网络存储」底色/圆角/间距不同。像素实测（列 x=1000，y 130-880）：

| | elevated（8 张卡） | **surface（网络存储）** | bg-primary（间隙） |
|---|---|---|---|
| 修前 | ~402px | **58px** | ~271px |
| 修后 | ~459-469px | — | ~255-265px |

六主题 `bg-surface ≠ bg-elevated` **无一例外** → 六套全错，只是 Xuan 差值最大（`#ece5d4` vs
`#fffef8`）最扎眼，暗色主题下肉眼几乎看不出。

根因是 **Vue scoped CSS 的根节点双作用域规则**，不是「重复定义」这么简单：组件根即
`<CollapsibleCard>`，而 style 里留着一份迁移前的自绘 `.settings-card` → 编译成
`.settings-card[data-v-NS]`，而 `data-v-NS` **恰会打在子组件根节点上** → 盖掉全局卡。
同一 style 块里 `.settings-card__header` 相反：CollapsibleCard 内部 DOM 拿不到本组件
data-v，是**死代码**。**残留样式一半活一半死，死活线正好卡在 scoped 规则上。**

**这类 bug 三道现有门全绿**：token 契约绿（两个 token 都合法定义，非幽灵）、check:contrast
绿（surface 本就是被守的合法底）、typecheck/lint 绿。
**取值全合法，错的是取了哪个** —— 正是视觉矩阵才抓得到的那类。

### 测量法：跨主题**不变像素图**，直击「硬编码色治理」

逐张眼过 18 图效率低且易漏。改为算「跨六主题恒定的像素」——不随主题动的像素即硬编码嫌疑：

| 场景 | 恒定像素 | 判读 |
|---|---|---|
| settings | **1 / 1,296,000 = 0.0%** | 完全随主题走 ✅ |
| gallery | 7.9% | 恒定区**全部落在照片内**（SVG fixture 本就与主题无关）；侧栏/顶栏/底色/分隔标签/时间轴均 6 色唯一 ✅ |
| viewer | **92.3%** | 见下 |

viewer 的 92.3% 一度像重大发现，实为**已声明的有意为之**：`ContentViewer.vue` style 块顶部
就写着 S5 硬编码色豁免（设计 §6.2）——「看图台永远黑（专业看图惯例）、人脸框需在任意照片上
可见、星级金全主题统一，刻意不随主题」。全库剥注释扫描 `background:#000|black` 仅 2 处，
**都在这个已豁免组件内**。故该测量**反向印证了豁免的边界正如声明**：viewer 里唯一随主题动的
是标题栏/状态栏（应用外壳），豁免既真实落地、也没外溢。
**测量的价值不只在抓 bug，也在证实「有意为之」确实只有意在它该在的地方。**

（扫描仍按 §18 先剥注释——该组件的豁免说明里就写着 `#000`，不剥则自己中招。）

### 矩阵的真实盲区（勿当已覆盖）

- **viewer 的主题化控制条未出镜**：脚本用全新临时 profile（无 localStorage）→ 侧栏按默认隐藏；
  headless **无指针** → 自动隐藏的底部控制条根本不渲染。viewer 这格实测只覆盖了
  舞台（黑，已豁免）+ 标题栏/状态栏。控制条仍是真机面。
- **非 Tauri**：headless Chrome 与 WebView2 同为 Chromium，故 CSS 变量解析/布局/配色可信；
  **原生窗口边框、WebView2 特有行为、GPU canvas 路径不在覆盖内**。
- **canvas 网格未入矩阵**：需 DEV + localStorage flag，全新 profile 下不启用。

### 捕获确定性（先自证工具可信，再拿它的数当证据）

同一主题连拍两次：porcelain **逐字节相同**；ink 差 482 像素（0.04%）、最大通道差 3、集中在
y=14 顶栏。噪声只会让不变性**低报**，故上表三个数可信（尤其 settings 的 0.0% 与 viewer 的
≥92.3%）。**拿测量当证据前先测量测量本身**。

**一处已测但未解释（如实记录，非结论）**：照片内部像素在 porcelain 下比其余五主题**每通道低 1**
（`#98b1ba` vs `#97b0b9`，三个采样点皆然、跨两次捕获可复现）。已排除「与页面底色合成」
（moonlight 底色与 porcelain 几乎相同却与 ink 一致）。**1/255 ≈ 0.4%，亚感知级、不影响任何
决策**，按比例原则不再深挖。

### 顺带核实：徽章 token 家族自洽

| token | 取值 | 判读 |
|---|---|---|
| `badge-live` / `badge-audio` / `badge-document` | 亮暗两组（如 audio `rgba(52,199,89,.85)` / `rgba(40,167,69,.85)`） | 与设计「随主题分亮暗两组」一致 ✅ |
| `badge-size` | 六主题恒 `rgba(0,0,0,0.6)` | 照片上的恒定浮层，与 S5 豁免同族 ✅ |
| `badge-doc-word/excel/ppt/md/generic` | 六主题恒定 | 品牌色（Word 蓝 / Excel 绿 / PPT 橙红）✅ |

（fixture 此前 18 项**全是 image** → AUDIO/DOC 角标在任何场景都渲染不出、等于不在覆盖内。
改 2 项为 audio/pdf 并开满缩略图信息元素后才谈得上「覆盖」。
**harness 是视觉场景，不是新装默认模拟器**——整套 fixture 本就是 18 张假图。）

## 会话续 28(2026-07-15):验收清单本身会过期——被推翻的「什么算对」是假 bug 生产器

**判据:验收清单是 normative 文本(它规定「什么算对」),故受与设计文档同一条规则约束——推翻它的裁决必须
当次回写它的正文。只写进新清单 = 留下一份长得和有效清单一模一样、却会主动生产假 bug 报告的陷阱。**

合并 7 份积压清单时,逐项对代码核实,发现 **3 处正文与代码相反**:
| 站点 | 清单写 | 代码实测 |
|---|---|---|
| 顶栏筛选/视图 ⋯ | `bottom-end` 右缘对齐(round5 §B / round6 §H) | `AppToolbar.vue:232/245` = `placement="bottom"` 居中 |
| 选区条 ⋯ | `top-end` 右缘对齐(round6 §B) | `SelectionActions.vue:107` = `placement="top"` 居中 |

根因是 round7 的「卡片弹出 + 与被点 ⋯ 按钮居中对齐」需求**推翻了 round5/round6 的对齐契约**,但只写进
round7 自己的清单。**危害不对称**:过期的*设计*文档最多误导实现者,而过期的*验收*文档会让验收者照着错的
预期去测,然后**如实报告一个不存在的 bug**——报告本身是诚实的,错的是标尺。且清单不像代码有编译/门禁信号,
一份陈述性文档不会因为和代码脱节而变红。

**与 §19 的关系**:§19 说集合级契约证明不了「选得对」;这里更进一步——**连「什么叫对」的定义本身都会漂移,
而定义漂移时没有任何门会红**。check_docs 只验结构/断链/frontmatter,验不了「这句话是否仍为真」。

**做法**:三处正文就地更正(带代码位置佐证)+ round6 §H 整节加「已被 round7 §A 取代」横幅 + 六份原清单
加指向 round10 的横幅。**未新设门禁**——「清单断言是否仍为真」是语义判断,为它设正则门会退化成幽灵门禁
(判据同 chunk 那轮 / experience §19「别为不可控或合法差异设基线门」)。**可行的替代不是门,是纪律**:
产出新一轮清单时,若本轮推翻了前轮的任何预期,当次回写前轮正文——与「推翻的裁决回写正文」同一条规则,
只是把它的适用面从设计文档扩到验收清单。

---

## 会话续 29(2026-07-16)——round10 真机结果:9 项异常的共同形状

**触发**:用户走完 round10 合并清单,回报「除以下记录外全部验收通过」+ 9 项异常 + 2 节未测。
逐项定位后 8 项已修(4 提交),#8 F11 用户定押后。

### 教训 1:「已修」标记不可信——未经真机的修复是薛定谔的修复

**#3(选区条折叠不回弹)是本轮唯一的 🔁 复验项,而它挂了**,且症状比原报告更具体:「收窄到一半再拉宽,
**还会继续**把剩余按钮收进折叠」。拉宽却继续折 = 测到的可用宽在变小 → 不是「没重测」,是**重测了但读数
错**——与 round6 当时的归因(RO 失联)是两种病。

时间线还原:
- `97354b6`(07-13, round6 #1)判定「内容宽容器 RO 失联」→ 改走 window resize → 完整 measure()。**标记为已修,从未上真机**。
- `a9d5379`(07-14, round7)为消除量尺帧闪动引入 measureAvailable 探针 → `onWindowResize` 改成
  `if (探针) applySplit else measure()`。**那条 else 分支正是 round6 #1 的修复本体**,自此在浮动态成死代码。
- 修复活了两天、被无声掐掉、07-16 才发现。

**没有门能红**:`useToolbarOverflow.spec.ts` 8 例自陈「纯核,与 DOM 无关」,只测 `computeOverflowSplit`;
失效发生在**「available 怎么被算出来」**那一层——零覆盖。纯核对「available 变大 → visibleCount 回升」
是成立的(贪心保证单调),坏的是喂给它的 available。

**与会话续 28 同族**:那次是**账面预期**脱离代码(过期的验收断言),这次是**账面状态**脱离代码(过期的
「已修」)。共同形状=**记录与地面真相脱钩,而没有门会红**。
**纪律**:标 🔁 复验的项在真机确认前,不得据以开阶段门;新一轮若改动了前轮修复所在的执行路径,须显式
复核那条路径是否还活着。

### 教训 2:保守估计在自反馈回路里不是保守,是发散

`overflowButtonWidth` 声明 40 而实际 36,差 4px。同一个数字:
- **AppToolbar(flex:1 全宽容器)**:available 由父级决定=**外生**,与 visibleCount 无关 → 高估 4px 只是
  早折一项,**稳定**,四个月无人察觉。
- **浮动选区胶囊(内容宽容器)**:`available = f(visibleCount)`=**内生** → 高估 → budget 比当前内容窄 →
  掉一项 → 内容更窄 → 再掉一项 → **一路吞到全折叠,与拉宽/收窄方向无关**。

40 不是「写错了」,是**从全宽容器的直觉里借来的、借错了拓扑**。
**判据**:凡「容器宽随其内容变化」的折叠场景,任何预留量的估计误差都会被回路放大;此类常量必须与实际
渲染宽**同源**(本轮做法:同一常量 inline 绑到按钮 style,由构造保证相等),而非「宁大勿小」。

**新增测试刻画了这个回路**(而非只测纯核)。写它时我杜撰了「低估也会反向失稳」,**被测试当场证伪**
(`expected 2 to be greater than 2`):低估**不发散**,其危害是预留不足致渲染越界=**静态错**。
**高估发散、低估静态错——失效模式不对称,别把一侧镜像成另一侧。**

### 教训 3:KeepAlive 的账单是「不可见不再蕴含不响应」

**#5**:`App.vue:183` 落 `/collections` 时 `setActiveCollection(null)` —— 这句是**纯粹的状态卫生**,
写它的人完全正确(总览页当然没有选中项)。错的是 **viewStore 里「无选中」与「要全库」共用同一个 null**:
对**在屏**的 MediaGrid,null 确实该显示全库;对一个**不可见但活着**的 MediaGrid,同一个 null 就是一道
50 万行的查询指令。

组件树里少了一个渲染节点,**watcher 图里一个都没少**。KeepAlive 的代价从来不是内存。
**不变量**:昂贵的取数必须挂「在屏」闸门(本轮:`useJustifiedLayout` 的 `enabled` + `flushIfDeferred`),
失活期只记 dirty、激活时用**当前**状态补算一次(**不重放中间态**——中间态正是要跳过的东西)。

### 教训 4:补丁救症状,不救根因——一条 `:not()` 让三个属性失守,只有两个被发现

**#6**:`.collection-card > :not(.collection-card__primary)` 的 `:not()` 把参数特异性算进来 → (0,3,0)
高过 `.collection-card__del` 的 (0,2,0),**与书写顺序无关**地把角标的 `position:absolute` 覆写成
`relative`。而作者**已经察觉**这条 blanket 规则会误伤——他补了一条 (0,3,0) 把 `pointer-events`/
`z-index` 抢回来。**唯独漏了 `position`**:按钮点不动像 bug,排版乱掉像「没写好」,不在症状清单里。

**判据**:发现 blanket 规则误伤时,**从 `:not()` 里排除**,而不是在下面补一条把被抢的属性抢回来——
补丁只覆盖你当时想到的属性。(本轮 `.collection-card__input` **不排除**:它是流内元素,正需要那条
`relative`——排除面要按每个成员的实际需要定,不是一刀切。)

### 教训 5:机制的副作用僭越成产品策略

**#7**:没有人决定过「撤销只有 5 秒」。`addToast` 里的 `setTimeout(removeToast, duration)` 本意是让提示
条消失,却因为回调寄生在 toast 对象上,**顺手把撤销能力一起删了**。而后端 `restore_items` /
`restore_collection` **无时效校验、全库无 purge**——用户要的「无限期」在数据层**早就是事实**。

**故修法不是把 5 秒调长**(调多久都是同一个错),是**把撤销从 toast 里搬出来**。
**判据**:当用户抱怨某个「策略」时,先查它是不是策略——很多「策略」只是某个机制的生命周期外溢。

### 教训 6:清单的可测性也会脱离现实(§7 未测)

用户回报 §7「APP 内无地址栏,无法测试」。**这是清单的错**:§7 通篇写「盯地址栏/改地址栏回车」,而 Tauri
WebView **没有地址栏**。

与会话续 28 的「过期预期」同族但换了一层:那次是**预期**脱离代码,这次是**验证手段**脱离交付形态。
两者都源于「写清单时脑子里跑的是另一个环境(浏览器)」。
**纪律**:写验收步骤时,每个「怎么看/怎么改」都要在**交付形态**里确认存在。(本轮改用 DevTools Console
`location.hash` 读写,零代码改动。)

---
---
*每执行 2 次查看、浏览器或搜索操作后更新此文件。*
