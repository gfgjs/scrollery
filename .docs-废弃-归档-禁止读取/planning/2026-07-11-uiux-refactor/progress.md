# 进度日志

## 会话：2026-07-11

### 阶段 1–5：审计、验证边界、方案设计与交付
- **状态：** complete
- **开始时间：** 2026-07-11
- 执行的操作：
  - 读取适用技能的完整说明。
  - 确认工作树初始状态干净，仓库根目录不存在旧规划文件。
  - 创建本次审计的持久化规划文件。
  - 重新读取规划文件并检查落盘结果。
  - 快速检索项目记忆，提取需要在当前源码中复核的缩略图交互线索。
  - 获取最新版 Web Interface Guidelines。
  - 盘点前端技术栈、页面、组件、样式文件和运行脚本。
  - 阅读应用入口、路由、shell、窗口 chrome、UI store 与主题注册表/选择器。
  - 全仓扫描 accessibility、focus、motion、表单、导航与硬编码色候选。
  - 统计 SFC 体积和 scoped style 分布，定位高耦合重构热点。
  - 核对现行 UI/主题与顶栏设计文档、todo 状态和近期前端提交历史。
  - 实跑 typecheck、ESLint、Vitest、主题对比度和 production build。
  - 连接本地 Vite 页面并采集 DOM、console 与首屏截图；确认普通浏览器无法承载 Tauri shell。
  - 深读 Settings、Collections/Persons、Onboarding、Toolbar、Shell/Sidebar/StatusBar 的结构与局部样式。
  - 深读 Gallery、SelectionToolbar、MediaThumb/Compact 的加载、选择与信息覆盖层结构。
  - 核对 router、filter/view store、Gallery chips/view controls 及常用 Dialog/Settings 表单语义。
  - 审读 Viewer、Document、Audio、Plugin、SemanticSearch 的主模板并扫描 responsive/touch target。
  - 审读 ContextualToolbar、viewer command registry、WindowChrome 与全局 CSS primitives/token 使用。
  - 审读 Sidebar 导航、Settings registry/折叠状态，并核查 Settings/Viewer 状态与 route/focus 的真实连接。
  - 回读全部发现后，编写并复核完整 UI/UX 第二阶段重构方案。
- 创建/修改的文件：
  - task_plan.md
  - findings.md
  - progress.md
  - docs/designs/2026-07-11-前端UIUX深度审计与重构方案.md

## 测试结果
| 测试 | 输入 | 预期结果 | 实际结果 | 状态 |
|------|------|---------|---------|------|
| 初始工作树检查 | git status --short | 无已有改动 | 无输出 | 通过 |
| TypeScript | npm run typecheck | 0 error | exit 0 | 通过 |
| ESLint | npm run lint | 0 error | exit 0 | 通过 |
| 前端测试 | npm test -- --run | 全部通过 | 47 files / 615 tests passed | 通过 |
| 主题对比度 | npm run check:contrast | 6 套主题硬门槛通过 | 全部通过 | 通过 |
| Production build | npm run build | 成功构建 | exit 0，含 chunk 警告 | 通过（有警告） |
| 文档卫生 | git diff --check | 无空白错误 | exit 0 | 通过 |

## 错误日志
| 时间戳 | 错误 | 尝试次数 | 解决方案 |
|--------|------|---------|---------|
| 2026-07-11 | 启动 Vite 时 1420 端口已占用 | 1 | 不终止未知现有进程，复用现有服务 |
| 2026-07-11 | 浏览器环境缺少 Tauri metadata/invoke，WindowChrome setup 失败 | 1 | 停止把 browser 页面当真机；改为源码审计 + 请求桌面截图，方案中加入 test harness |

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 方案已交付，等待用户确认与真实 Tauri 截图 |
| 我要去哪里？ | 用户确认后按 S0–S7 分阶段实施 |
| 目标是什么？ | 输出待确认的前端 UI/UX 重构方案，不改业务实现 |
| 我学到了什么？ | 见 findings.md |
| 我做了什么？ | 已完成源码审计、门禁验证、浏览器承载边界验证和完整方案文档 |

---
*每个阶段完成后或遇到错误时更新此文件。*

## 会话续：方案批准与实施启动

### 阶段 6：S0 视觉基线与 browser UI harness
- **状态：** in_progress
- 执行的操作：
  - 用户确认全部推荐项并将 accessibility 调整为非阻断目标。
  - 原始分辨率检查 4 张真实 Tauri 截图。
  - 将 Gallery、Settings、Viewer 的真机视觉结论回写 findings 与设计方案。
  - 检查现有 IPC、platform、event composable、Vite 配置和首屏 stores，确定 harness 需要覆盖 window API、invoke 与 listen 三类边界。
  - 定位截图中的“画布”标签为 MediaGrid 的公开原型切换控件，列入首批产品 UI 清理。
  - 确认 harness 可复用现有 LayoutSummary/LayoutRow 驱动真实 MediaGrid，只需 mock IPC 与静态示例资源。
  - 确认 AI/face status 的空闲 fixture contract，以及 MediaGrid compute/fetchRows 的首屏调用顺序。
  - 确认 Gallery/Timeline 两个 render-mode 原型按钮可用单一 dev flag 隐藏。
  - 确认命令注册表已有 `view` 分组；Viewer 的 zoom/rotate/info 可从顶栏 `navigation` 收回到底部控制面，无需新增契约。
  - 确认 S0 应在统一 `invokeIpc` 入口做 DEV-only fixture 分流，并为窗口 API/事件监听补轻量适配层；harness 继续挂载真实业务页面。
  - 已把新装默认亮色主题切换为 Moonlight，暗色仍为 Ink；既有用户持久化选择不受影响。
  - 已将 Gallery/Timeline 的 DOM↔Canvas 实验胶囊改为 DEV + 显式 localStorage debug flag 才显示。
  - 已按 command ownership 将 Viewer 的 zoom/fit/rotate/info 归入 `view`，顶栏 `navigation` 仅保留 prev/next/immersive。
  - 已新增 DEV-only `?ui-harness=gallery|settings|viewer` 场景选择、IPC fixture、window/event adapter 与受控 asset URL 解析；production/Tauri 路径保持真实 API。
  - harness fixture 通过真实 `App`/stores/`MediaGrid` 渲染 18 个稳定示例资产，并覆盖 Gallery/Settings/Viewer 的启动依赖。
  - 针对 Viewer command ownership 和 data URL 增补单测；定向 14 tests、typecheck、ESLint 均通过。
  - Settings 已从全局 overlay 改为 `/settings` 路由页；Sidebar footer 使用 router 导航，Escape/返回按钮按 history 回退。
  - Viewer route 默认折叠 Sidebar，并提供显式显示/隐藏按钮；browser harness 已验证两种状态及内容区宽度变化。
  - browser harness 已完成 Gallery/Settings/Viewer 三场景挂载，Moonlight、实验按钮隐藏、真实 Settings 子组件和 Viewer 图片加载均已验证。
  - 回读 AppToolbar/ContextualToolbar/ContentViewer 后确认：Gallery 必须拆为 chrome 与 page toolbar 两个 surface；Viewer 需先补标题栏返回入口再删除底栏关闭，避免产生不可见的退出路径。
  - 核对 `ViewerApi` 后确认已有 `close` 契约；Viewer 返回入口可完全复用现有 history-aware close 实现，命令层无需新增 router 耦合。
  - Gallery 工具栏已移入 AppShell 页面 toolbar，标题栏只留 brand/window/context navigation；Viewer 标题栏补文件名层级。
  - 盘点 Settings 10 个实际卡片后确定 5 域信息架构，并决定以 `/settings/:section?` 保存当前域，搜索按域过滤、域内仍沿用现有 CollapsibleCard。
  - Settings 已落地 5 域导航、域级搜索、route 深链、空结果状态与 900px 窄窗布局；返回按钮从危险红色 hover 改为普通导航反馈。
  - Settings browser 回归确认 5 个分区导航、搜索框、Moonlight/Ink 选择、1180px 主区内 210/874px 双栏和 0 横向溢出均实际渲染。
  - Settings 导航与搜索交互实测通过：高级域写入 route，搜索按 registry label/description 过滤分区。
  - Gallery browser 回归确认 40px titlebar + 48px page toolbar 分层、18 项渲染、0 实验入口、0 横向溢出。
  - Viewer browser 回归确认顶栏 4 个 navigation 命令、底栏 7 个唯一内容命令、文件名层级、默认隐藏 Sidebar 与 0 横向溢出。
  - 回看批准方案与当前实现差距：下一批先完成稳定搜索宽度、时间轴默认宽度、Settings danger 元数据；22 处 transition:all 留作独立机械批次，避免与结构改动混在一起。
  - 核对时间轴启动链路后确认默认值需同步 config state、配置读取 fallback 与 CSS fallback，既有持久化宽度继续优先。
  - 核对 Gallery query 所有权：filter/layout 可立即 route 化；mixed 搜索 query 仍困在 AppToolbar 局部 state，必须先收敛搜索单源，避免 URL 看似可恢复、实际不执行查询。
  - 深读 Collections/Persons 后确认两者均以 active store + `/` 表达详情；下一步直接复用 MediaGrid 新增参数化 route，并把卡片主/次操作改为真实 button。
  - 已落地 `/collections/:id`、`/persons/:id`，卡片打开改走参数 route，App 负责深链水合；Sidebar active 改为 route prefix 并修复人物详情时“全部照片”双高亮。
  - Collections/Persons 卡片主操作和次操作已改为真实 button；人物操作命中区提升到 32px。确认现有 person IPC + Toast action 足以实现隐藏/误检立即撤销。
  - Persons 已增加隐藏人物管理视图、恢复按钮、隐藏/误检 5 秒撤销 action，并同步中英文影响说明。
- 创建/修改的文件：
  - task_plan.md
  - findings.md
  - progress.md
  - docs/designs/2026-07-11-前端UIUX深度审计与重构方案.md

### 错误日志（实施阶段）
| 错误 | 尝试次数 | 处理 |
|------|---------|------|
| PowerShell 中组合双引号 rg pattern 导致字符串终止符解析错误 | 1 | 不重复原命令；后续拆为单引号 pattern 的独立查询 |
| Viewer 命令批量替换首次误命中 prev/next | 1 | 立即回读 diff 后停止扩改，改按 command id 精确修正并再次通读完整文件 |
| Settings harness 首次把 `list_volumes` 未覆盖命令降级为 null，KnownVolumes 读取 length 崩溃 | 1 | 按真实 contract 将 `LIST_VOLUMES` fixture 补为空数组，并补 `GET_LOG_DIR` 稳定字符串 |
| Viewer harness 首次把 `get_item_faces` 未覆盖命令降级为 null，ContentViewer 读取 length 崩溃 | 1 | 按真实 contract 将 `GET_ITEM_FACES` fixture 补为空数组；scene route 改为等待 router ready 后进入 |
| 首次检索 i18n 路径使用不存在的 `src/locales`/glob | 1 | 回读目录后改用真实路径 `src/i18n/locales`，未重复错误命令 |
| browser 全页截图在复杂场景多次超时 | 2 | 停止重复全页截图，改用 DOM 状态断言与限定 clip 截图完成验证 |
| 组合 `rg` 模式再次受 PowerShell 双引号转义影响而解析失败 | 1 | 不再拼接含 HTML 属性引号的正则；后续按固定字符串或拆分查询 |
| 首次多文件 `apply_patch` 的 hunk 分隔缺少完整上下文 | 1 | 未产生文件改动；拆为带明确上下文的精确 patch 后成功落盘 |
| Settings 搜索直接读取字面量联合中的可选 `descKey` 导致 TS2339 | 1 | 使用 `'descKey' in spec` 做联合类型收窄，保持 registry 字面类型不被宽化 |
| browser API 误用 `tab.playwright.screenshot` | 1 | 改用 `tab.screenshot`；该次限定截图返回空白，后续以 DOM/几何/交互断言为主要证据，不重复依赖截图 |
| Settings 搜索验证读取不存在的空结果节点并传给 `getComputedStyle` | 1 | 保留已完成的点击/输入动作，下一次仅用节点存在性判断，交互结果确认正确 |
| 首次读取 token 使用了不存在的 `src/styles` 路径 | 1 | 目录回读确认真实位置为 `src/assets/styles`，随后只从真实路径继续 |
| component token 多文件 patch 首次存在不完整 hunk 分隔 | 1 | 未产生改动；拆分 variables 与消费端 patch 后落盘，并回读 diff 核对实际命中位置 |

## 会话续 2：模型切换接续（2026-07-11，Claude Fable 5）

### 接续核验与门禁修复
- 完整读取四份任务文档，确认中断点为「静态门禁未跑、全部改动未 commit」。
- 首跑门禁发现两处回归并修复：
  - `harness/runtime.ts` 顶层读 `window`，9 个 node 环境 spec 在收集阶段崩溃（84 测试未执行）→ 补 `typeof window` 环境守卫。
  - keybinding 分发器只查 `navigation` 组，zoom/info 挪入 `view` 组后 `+`/`-`/`i` 真实失灵（非仅测试红）→ 分发去组过滤，组只决定按钮渲染位置，上下文安全由 `when` 谓词保证。
- 全量门禁复跑：typecheck 0 / ESLint 0 / **617 测试全绿** / 6 主题对比度过 / build 过 / diff --check 过（仅本地验证）。
- 逐项核实上批笔记「已核对」项的真实落地：时间轴 44px 三处同步 ✅、搜索稳定宽槽位 clamp(220px,26vw,320px) ✅、Persons 隐藏管理+undo ✅——均已在代码中。
- 分批 commit：设计文档 `8470349`；S0–S6 首批结构改造 34 文件 `bcf0934`（含门禁修复）。规划三文件按文档治理约定（根目录不新增散件）留在工作区不入库。

### Settings danger zone（S4 收尾，c98ac93）
- 前置核对真实破坏性操作归属（承上批「不虚构 danger zone」裁决）：settingsMap debug 段即破坏性操作集中地（清库/重置设置/清缩略图/清日志）；AI restart/忘记卷为对象级操作留原语境。
- settingsMap 新增 danger 分区（破坏性递增排序，清库最后）；SettingsView 高级域内独立危险卡（红调描边+提示语+默认折叠）；搜索语料收编。
- 清除缓存实为无损 cache-busting 重载，降级普通按钮留 debug 区；clearLogs 补 confirm 对齐危险区契约。
- 修存量幽灵类 bug：`.btn-danger` 仅 SettingsView scoped 定义而 DynamicSettingControl/CloseConfirmDialog 消费不可达（危险按钮从未有过红色视觉）；`.btn-secondary` 同病（9 处消费 3 处定义）。二者提升 index.css 全局变体。
- browser harness headless DOM 断言：危险卡默认折叠（aria-expanded=false）+ 恰好 4 个 btn-danger + debug 卡零危险按钮 + 缓存按钮已降级 btn-secondary。

### S1 机械批：transition:all 清零（74767eb）
- 20 处真实现场（另 2 处为警示注释）逐一读上下文，按 hover/active/selected 实际变化的属性显式列举；保留原时长/缓动；step-dot 的 width 形变与 MediaThumb ::after 的 border-radius:inherit 跟随均有意保留。
- 全库 grep 核验清零；全量门禁复跑全绿（仅本地验证）。

## 会话续 3：模型切换接续（2026-07-12，Claude Opus 4.8）

### S2 拆段决策与 S2-a 搜索单源门面（cc4bfb7）
- 用 Explore 子 agent 完整测绘搜索/筛选/视图数据流后确认：S2「Gallery filter/query 同步」牵涉搜索状态三处碎片、两处必须锁步的 backend filter 投影（useJustifiedLayout / useViewDescriptor）、以及「collections/persons 已路由化 vs 文件夹/智能相册仍停 `/`」的路由非对称，全库零 query-param 基础设施。一次性大 commit 高风险（watch 回环 / 双查询 / KeepAlive 滚动回归 / 深链非对称）。据方案 §14 风险表「route descriptor 与 scroll anchor 分开提交」，把 S2 拆为 S2-a（搜索单源）→ S2-b（URL 双向同步）→ S2-c（view 维度归一+Sidebar descriptor）三段。
- **S2-a 交付**：新增 `stores/searchStore.ts` 协调门面。地图纠正了初判——committed query 其实已在 store（ui.searchQuery / ai.semanticQuery），真正缺的是「一个会执行查询的统一写入动作」；门面的核心交付物是 `apply(query)`/`clear()`（供 S2-b URL 回填调用，保证深链真正触发查询）+ `committedQuery`/`mode`/`scope` 读表面（供序列化）。draftMixedQuery 提升出 AppToolbar；AppToolbar 降薄消费者；删除 emit('search'/'semantic-search') 与 App.vue onSearch 冗余双写（普通搜索本就每键即写 ui.searchQuery，emit 是死重）。
- 门面取舍=**单一接口而非物理合并存储**：不搬迁 aiStore.runSemanticSearch 的 searchToken 代次守卫、语义模式 group/sort 覆盖等协调逻辑（搬迁高风险低收益），commit* 委托既有 ai action。
- 单测 21 例（searchStore.spec.ts）：读表面投影 + commit/apply/clear 按模式派发 + 模式切换清草稿。用真 ui/ai store + mock `../utils/ipc` + `vi.spyOn` ai action 验证路由。

### 门禁（本地验证，非 CI 门）
| 测试 | 结果 |
|------|------|
| typecheck | exit 0 |
| ESLint | exit 0 |
| 全量测试 | 48 files / 638 tests 通过（较上批 617 增 21） |
| 主题对比度 | 6 套硬门槛全过 |
| build | exit 0（仅既有 scanStore 混合 import + cpp 语法块警告，无新增） |
| diff --check | 净（仅 LF/CRLF 信息性提示） |

### 错误日志（本会话）
| 错误 | 尝试次数 | 处理 |
|------|---------|------|
| searchStore.spec 首跑 8 例 `window is not defined`（uiStore.ts:52 setup 期同步读 window.matchMedia，node 无 window） | 1 | beforeEach 补最小 window 桩（matchMedia/location/addEventListener）；仅用 aiStore 的 13 例先通过，证明模块图加载正常，定位精准。未引 jsdom（KISS，config 注释表明尚未按需引入）。

### S2-b Gallery 筛选 ↔ URL query 双向同步（3719be1）
- **同步模型决策**：filterStore 是全局态（切换视图不清筛选，既有契约），故 URL 表达「全局筛选在当前路径上的投影」而非逐历史态。据此定：store→URL 连续写（筛选变/画廊路径变都重投影，router.replace 不污染历史）；URL→store 仅初次水合（深链/刷新）。回环用值相等守卫 + hydrated 门双重断开——hydrated 门是双向同步最隐蔽的初始化竞态防线（防默认空筛选在初次 readUrl 前把深链 query 冲掉）。
- **本批仅 filter 维度**（types/favorite/live/rating/color/from/to）：纯内存、零副作用、零 persist 竞态，加 URL 是纯增值。group/sort/layout（持久化，persist-vs-URL 优先级+异步水合竞态）与 search q/scope/mode（与 aiStore 模式切换 group/sort 副作用纠缠）有意后置 S2-b2，且 group/sort/layout 的 per-view vs sticky-global 需用户拍板（不擅自冻结契约）。
- 新增 `utils/galleryQuery.ts`（纯编解码 + isGalleryRoute，防御式解析外部 URL）+ `composables/useGalleryQuerySync.ts`（挂 App.vue，画廊路由内生效，保留非管理 query 键为 S2-b2 预留）。App.vue showGalleryToolbar 改用 isGalleryRoute 单源（消除重复谓词漂移）。
- 单测 19 例（galleryQuery.spec.ts）：isGalleryRoute + 编解码往返 + 防御（白名单/clamp/垃圾/数组取首）+ 快照相等。composable 的 watcher/router 编织无 test-utils 难单测，改用真机 harness 验证（见下）。

### 门禁（本地验证，非 CI 门）+ 真机 harness
| 项 | 结果 |
|------|------|
| typecheck / ESLint | exit 0 / exit 0 |
| 全量测试 | 49 files / 657 tests（较上批 638 增 19） |
| 主题对比度 / build / diff --check | 6 套过 / exit 0 / 净 |
| harness headless Chrome DOM 断言 | 深链 `#/?types=video&rating=3` → video chip + rating chip 恢复 active、clear-filters chip 出现（筛选真落 store 非仅 URL 字符串）、页面正常渲染 392KB（无 echo 死循环、无深链被冲掉）|

（harness 验证补上单测覆盖不到的运行时编织层：codec 纯函数单测锁死，watcher/router/reactivity 靠真机 DOM 断言——无 Playwright 时的可行证据，同上批验 danger zone 之法。）

### S2-c 测绘与视图路由化重估 + S2-b 回归修复（71c32ed）
- 用户裁决下一步做 S2-c（视图路由+descriptor）；view-pref 语义采 per-view URL 权威但延后（记入 task_plan/memory）。
- **Explore 子 agent 测绘 folder 导航+scroll-anchor+MediaGrid 生命周期后重估**：视图路由化比设计 §5.1 假设复杂得多——① **folder 双模**：`ui.groupBy==='folder'` 时点文件夹是设 `pendingScrollDirId` 滚动锚点（非筛选）、`getViewKey()` 恒 `album-all`；`groupBy!=='folder'` 时才 setActiveDirectory 作筛选。folder 在分组模式下不是可寻址视图而是滚动位置。② 所有 scroll cache/KeepAlive/侧栏高亮以 **viewStore（非 route）**为键（getViewKey 697-698），路由化须同步写 viewStore 于 MediaGrid 读取前。③ smart-album 与 folder 共享路径 `/`，必须一起路由化。结论：S2-c2-3 路由化需 folder 双模产品决策 + 真机滚动/KeepAlive 验证，不在无 GUI 环境仓促做；重排为先做 S2-c1 descriptor 统一（安全纯重构）。
- **同一测绘确认并修复 S2-b 引入的回归（71c32ed）**：App.vue collection/person 视图 watcher 监听 `route.fullPath`，S2-b 的筛选 query 同步（router.replace 改 query）→ fullPath 变 → 误触发 setActiveCollection/Person（同一个）→ clearSelection+重水合。即「收藏夹/人物视图切筛选清选区」，S2-b 前不存在。改监听 `route.path`（view 维度只依赖路径段），watcher 体内本就只读 route.path，行为等价。门禁：typecheck 0/ESLint 0/657 测试全绿/build 过/diff 净（本地）。

### 错误日志（本会话续）
| 错误 | 尝试次数 | 处理 |
|------|---------|------|
| S2-b 引入回归：collection/person 视图切筛选清选区（fullPath watcher 被 query 同步误触发） | 1（测绘时发现） | App.vue watcher fullPath→path；已修 71c32ed |

### S2-c1 descriptor 统一（e54a73f）
- 承接用户「直接推进」+ 我的建议：先做 S2-c 里安全、纯重构、可测的那一半。
- 新增 `utils/resolveView.ts` 纯函数：把 useJustifiedLayout.compute() 与 useViewDescriptor 各自内联的「视图维度→backend」precedence（person>collection>smart-album>directory）+ 映射决策收敛为 ResolvedView 单源。两投影只保留各自机械映射（JL→扁平 filters+directoryId param；VD→scope 判别联合+filter overlay）。🔴 R1-2「新增维度须两处同步」双维护 hazard 消除。
- 关键取舍：**统一的是"决策"不是"输出"**——两处输出形态本就不同（backend 两 API：compute_layout 收扁平 MediaFilter+directoryId；view_to_sql 收 scope DTO），不可也不该合并输出；抽出共享的 precedence 决策即根治双维护。特意不动 JL 的 directoryId param（恒=activeDirectoryId，与 filters 分支正交），把改动面压到最小、对可达状态零行为变化。
- resolveView.spec 13 例穷举决策矩阵（各维度映射 + 优先级 + system 夹无 mediaTypeFilter 回落 collection）。证据取向=覆盖决策矩阵的 characterization 单测 + switch 类型穷尽 + 全量绿；不用 harness（默认 all 视图不经改动分支，渲染快照无区分力）。
- 门禁（本地）：typecheck 0 / ESLint 0 / 50 files 670 tests（+13）/ 6 主题对比度过 / build 过 / diff 净。

### S1 UiButton 原语 + SFC 测试范式（e8a7e3e）
- 承接用户「做 S1 S3」：S1 先行（S3 toolbar 会消费 UiIconButton）。首件选 UiButton——最常用、且顺手消掉前批发现的 scoped .btn 遮蔽。
- 现状核实：全局 .btn 体系 + --control-size token **已存在**（index.css:68-144，且 :117 注释已指向 S1 UiButton 收敛）。故 UiButton 不造 CSS，而是**类型化组件包裹**：variant 语义替代裸 class 字符串，杜绝拼写漂移与 scoped 遮蔽。
- **地基性副产品：项目首个 SFC contract 测试范式**。此前 vitest 只测纯 .ts（全仓零组件测试）；本批加 @vitejs/plugin-vue + 用 @vue/server-renderer renderToString 做 SSR 断言（无 @vue/test-utils，仍 node 环境 SSR 不触 DOM，现有 .ts 测试零回归）。后续 11 个原语/任意组件的 contract 测试均可复用。
- 迁移 SettingsView 5 按钮（4 secondary+1 primary）→UiButton，删 scoped .btn/.btn-secondary/.btn-primary/.btn-danger 副本（primary/danger 本是死码：本视图模板无消费者且 scoped 不穿透子组件；btn-toggle-all/btn-back 独立类不受影响）。SettingsView 净减 71 行。唯一视觉 delta=按钮 padding 7→6px（收敛到全局，微不可感）。
- 证据：UiButton.spec 11 例（SSR 断言 variant/type/loading/disabled/插槽）+ 真机 harness（Settings 场景 DOM：5 按钮渲染正确 btn btn-{secondary,primary}，证明删 scoped 副本后 UiButton 在真实父作用域仍正确——scoped 遮蔽反面验证）。门禁（本地）：typecheck 0 / ESLint 0 / 51 files 681 tests（+11）/ 6 对比度过 / build 过 / diff 净。

### S1 UiIconButton 原语 + S3 roving tabindex（8f4f249）
- 承接「做 S1 S3」：UiIconButton 是 S1↔S3 的咬合件（ContextualToolbar 这个 S3 主角要消费它），故与 roving 同批落在同一文件。
- 现状核实厘清 S3 范围：**内容驱动溢出基建已存在**（useToolbarOverflow 纯核 computeOverflowSplit 已单测 + ResizeObserver 缓存 offsetWidth/gap/padding 校正）；设计提到的 252px 是 AppToolbar filter popover 独立宽度,非工具栏溢出魔数。故 S3 真正缺口 = **roving tabindex**（审计明列 `ContextualToolbar.vue:6-27`「Toolbar 只占一个 Tab stop」）。
- **UiIconButton**：包裹全局 `.btn-icon`（9 文件 38 处）。核心 a11y 收敛 = `label` 提为**必填 prop**，从类型层杜绝无名图标按钮（此前全靠人工记 aria-label，漏一处读屏即空按钮）；`active`→`.active` 视觉、`toggle` 单独门控 `aria-pressed`（仅开关型该有 pressed 语义,瞬时按钮如放大/旋转不带）、`title` 缺省回落 label 但可覆盖（ContextualToolbar 的 title 含快捷键提示,与 aria-label 有意不同）。无 scoped style（纯类型化包裹,不复制 CSS）。11 例 SSR contract 测试（复用 UiButton 范式）。
- **useRovingTabindex**：纯核 `nextRovingIndex`（按键+当前索引+enabled mask+朝向→下一焦点索引,跳过 disabled、端点环绕、朝向门控 Left/Right vs Up/Down、Home/End 恒响应,14 例穷举单测）+ 薄 composable（DOM 焦点管理属 ⏸GUI）。关键正确性 = `refresh()` 在挂载/命令集变化时把 Tab 停靠位校正到首个可聚焦项，**防其落在 disabled 项**（disabled button 不可聚焦,否则整条工具栏无 Tab 落点）。与 useToolbarOverflow 同构（纯核可测 + DOM 部分薄）。
- **ContextualToolbar 接入**：`<button class="btn-icon">`→`<UiIconButton>`；全组接 roving（`ref=containerRef` + `@keydown=onKeydown` + `@focusin=onFocusin` + `:tabindex=tabindexFor(i)`），rovingKey 编码 id+enabled 触发校正。装配须置于 commands/enabledOf 定义之后（remeasureKey 建 watch 即时求值 → 读 commands，防 TDZ）。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **53 files 706 tests（+25）** / 6 对比度过 / build 过（2.88s）/ diff 净；**headless Chrome dump gallery harness** 确认 ctx-toolbar 渲染 3 个 UiIconButton（btn-icon 类 + data-v 父作用域）：撤销/重做 `disabled tabindex="-1"`、全屏 `tabindex="0"`（**停靠位精确跳过 2 个 disabled 项落到首个可聚焦项,refresh 校正得证**）、开关型全屏 `aria-pressed="false"` 瞬时按钮无 aria-pressed（toggle 门控得证）。
- 余项（本会话未做,依赖真机/更大改动面）：UiIconButton 广域迁移其余 8 文件（ContentViewer/ToolsSection/GalleryViewControls 等,ContentViewer 有 `.detail-controls .btn-icon` 暗色浮层 scoped 覆盖依赖真机验；UiField/UiPopover/UiDialog/UiToolbar 原语；ContextualToolbar 溢出接入（待命令增多）；Reader/Audio 命令双入口收敛（需真机）；Search 稳宽槽位+combobox。

### S1 UiDialog 原语 + 迁移 2 个对话框（本次待提交）
- 承接「继续推进 S1」：勘察发现 `.btn-icon` 广域迁移剩 8 文件多为状态重/真机依赖,而**对话框是更高杠杆的死重灾区**——7 个组件（ConfirmDialog/CloseConfirmDialog/FolderCreateDialog/FolderTreeSelectorDialog/OnboardingWizard/FaceApprovalPanel/ExoticActivateDialog）各自逐字重复 ~120 行外壳。全局层（index.css:474 A2「Modal 基座」）只覆盖 `.dialog-overlay/.dialog-content`,`header/title/关闭键/body/footer` + 两个 keyframes 仍逐文件重复。
- **UiDialog**：全库模态唯一实现。吃下 ①`Teleport to="body"`（恒 Teleport,根治漏用类隐患）②焦点陷阱接线（复用既有 `useFocusTrap`,overlay ref 由原语持;`data-autofocus` 初始焦点目标落在消费方插槽,由 `useFocusTrap:70` 的 `querySelector` 跨组件边界命中——插槽 DOM 挂在 overlay 子树内）③Escape/点遮罩关闭（`emit('close')`,关闭语义交消费方）④`role="dialog"`/`aria-modal`/`aria-labelledby`(titleId)/`aria-describedby`(describedById) 语义。props: open/title/titleId/describedById/closeLabel/showClose/maxWidth;slots: default(body)/footer（footer 空则不渲染 border-top）。scoped 只补 header/title/关闭键/body/footer,overlay/content/动画走全局基座。
- **13 例 SSR contract**：关键处理 = UiDialog 恒 Teleport,`renderToString(app)` 默认不含 teleport 产物,须传 `context` 二参、从 `context.teleports` 取回再与主串合并断言。覆盖 open 门控、dialog 语义、aria-labelledby/describedby 关联（含缺省不产空属性）、closeLabel 双写与回落、showClose 门控、maxWidth 特化、body/footer 插槽投影与空 footer 不渲染。
- **迁 2 个最简同款消费者**：ConfirmDialog（useConfirm 驱动,app 级确认）+ CloseConfirmDialog（关窗确认）。净删外壳 244 行（-303/+59）,各自 scoped 只留 `.dialog-message`/`.remember-checkbox` 正文特化（作用于 UiDialog 插槽内容,但插槽在消费方作用域编译带其 data-v,scoped 依旧命中）。**顺带修掉 CloseConfirmDialog 漏用 Teleport 的隐患**（此前原位渲染,z-index:9999 可被祖先 overflow/transform 层叠上下文裁剪）。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **54 files 719 tests（+13）** / 6 对比度全硬门槛过 / build 过（2.86s）/ diff --check 0。构造性行为保持（同一 useFocusTrap、同一 aria id/data-autofocus/handler,结构逐字对齐）。**焦点陷阱 engage/release、Escape、点遮罩、动画属 DOM 运行期,node SSR 不触 → 标注真机验收**（与 useFocusTrap 自身「DOM 接线由手测验证」的既有取舍一致,不过度声称）。
- 余项：其余 5 个对话框迁 UiDialog——FolderCreateDialog 用原生 autofocus + onMounted 聚焦 overlay（未用 useFocusTrap,迁移会新增焦点陷阱=行为变更）;OnboardingWizard 多步向导、FaceApprovalPanel 审批面板、ExoticActivateDialog 激活流结构异质;均需真机验证焦点/动画,本会话不仓促做。

### S1 摊薄 UiDialog：迁 FolderCreate + FolderTreeSelector + useFocusTrap 通用化（8f78174 / c7c02d4）
- 承接「继续 S1」。**方向经用户确认=摊薄 UiDialog**：探查发现 UiField 杠杆低于路线图预期（表单面碎裂在 12-13 文件、无全局 CSS 基座、消费方视觉异构需先做 canonical 样式裁决），而 UiDialog 已建但消费方未摊薄=当前最高杠杆缺口;且余下对话框迁移不是纯 dedup——FolderCreate/FolderTreeSelector **此前根本无焦点陷阱**（Tab 逃逸）+ **无 Teleport**（可被祖先层叠上下文裁剪），迁 UiDialog 顺带补上=dedup+真实 a11y 修复二合一。
- **关键地基发现**：`useFocusTrap` 现唯一调用方就是 UiDialog（Confirm/CloseConfirm 已改由 UiDialog 托管）。其 `watch(active)` **非 immediate**,只在 false→true 跳变时 engage——ConfirmDialog 等「恒挂载 + open 翻转」能工作,但 FolderCreate/FolderTreeSelector 由父 `v-if`「挂载即开」,挂载即 active=true 无跳变,焦点陷阱**永不 engage**。
- **useFocusTrap 通用化（8f78174,三处改动使其兼容两种挂载范式）**：①watch 加 `immediate`——挂载即 active=true 也 engage;对恒挂载范式挂载时 open=false,immediate 两分支皆不命中,行为零变化（唯一调用方 UiDialog=安全面可控）。②`onBeforeUnmount` 改直调 `release`——挂载即开的对话框在 open 态被父卸载,watch 见不到 true→false,焦点归还改由卸载钩子兜底（release 含移除监听+归还;恒挂载正常关闭已置空=幂等）。③`engage` 加 SSR 守卫（`typeof document==='undefined'` 即返）——**门禁抓到的真实回归**：immediate 会在 SSR setup 期(open=true)触发 engage,若不守卫同步触碰 document 抛 ReferenceError,毁 UiDialog 13 例 SSR 契约。修法非撤 immediate(那毁挂载即开),而是守卫 DOM 访问。
- **迁 FolderCreateDialog（8f78174）**：外壳全删,scoped 只留 form-group/input-text/error-message/base-path-row 表单正文特化;内联 style 收进 class;两按钮改 UiButton;input 由原生 autofocus 改 `data-autofocus`（交焦点陷阱确定性初聚焦）。
- **UiDialog 加 bodyPadding 旋钮 + 迁 FolderTreeSelectorDialog（c7c02d4）**：bodyPadding prop（缺省 spacing-lg;传 '0' 满幅）是列表/树/表格类对话框通用需求,补 1 例 SSR 契约。FolderTreeSelector 树列表定高滚动移到插槽内 `.tree-scroll`,满幅走 `body-padding='0'`,500px 走 `max-width`,自定义左右分栏页脚进 `#footer` 用 width:100% 的 `.footer-split` 自撑 space-between（UiDialog footer 本 flex-end）,四按钮改 UiButton;嵌套「新建文件夹」对话框提为**同级根**（本组件迁移后多根）各自独立 Teleport,后挂载者 DOM 靠后叠于上层。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **54 files 720 tests（+1 bodyPadding）** / 7 主题对比度全硬门槛过 / build 2.90s / diff --check 0。分两笔提交（8f78174 地基+FolderCreate;c7c02d4 bodyPadding+FolderTreeSelector）。**焦点陷阱 engage/归还、Escape、点遮罩、Teleport 分层、嵌套对话框叠放属 DOM 运行期,结构/aria/bodyPadding 已 SSR 门禁验+标注真机验收,不声称交互已验证。**
- 初始焦点小尾：FolderTreeSelector 无 data-autofocus,焦点陷阱落到首个可聚焦元素=关闭键（与迁移前「focus 遮罩→Tab→关闭键」大致同）;Tab 含在框内是真 a11y 净收益。若真机觉得该落树列表,后续加 data-autofocus 微调。
- **并行会话注记**：本轮工作期一个并行文档治理会话在改 `docs/completed.md`（长段落重排 bullet）+ 提交 183f6b4（todo.md 搬迁）。我两笔提交全程用 `git commit -- <显式路径>`,`docs/completed.md` 已排除,留其工作树未动。

### S1 摊薄 UiDialog（续）：迁 ExoticActivateDialog（6577d04）
- 承接「继续推进」,同方向续做。三个待迁对话框读齐后按异质度排序:**ExoticActivateDialog 最标准**——外壳与 UiDialog 逐字同构,`@click.self`/`@keydown.esc`/`tabindex=-1` 全等原语内建行为,属范式①「常驻+open 翻转」(父 ContentViewer/PluginStoreView 恒挂载切 `:open`),标准右对齐 2 键页脚。OnboardingWizard/FaceApprovalPanel **均需先给 UiDialog 加 header 插槽**(前者头含副标题+步骤进度点、后者头含副标题),且各有非标语义(Onboarding 禁遮罩/Escape 关闭、FaceApproval 无页脚+行内动作),故留作原语扩展的独立增量。
- **迁移（一文件 +49/−158，净删 109 行）**：外壳(Teleport/overlay/content/header/关闭键/footer/两 keyframes/dialog-body)全交 UiDialog + 全局基座;宽度走 `max-width='460px'`;正文 message+textarea+error 进默认插槽(UiDialog body 本 flex-column gap,间距不变);2 键(ghost/primary)进 `#footer` 走 UiButton;加 `title-id='exotic-activate-title'` 令 aria-labelledby 关联标题(原组件根本无 aria-labelledby=顺带补 a11y)。`watch(open)` 只留复位输入,初始聚焦交焦点陷阱(textarea 标 `data-autofocus`),删 tokenRef/nextTick/X 导入。
- **三条关闭路径收敛 + 守卫保全**：原组件把遮罩点击、Escape、X 键各绑 `onCancel`(内含 `gate.activating` 激活在途禁关守卫)。UiDialog 把三路统一成一个 `@close` 事件 → 只需 `@close='onCancel'`,守卫对三路一并生效,语义零漂移。这类「N 入口→1 出口」收敛是原语化最易漏接的地方(漏一路=守卫被绕过),逐路核对 UiDialog 三处 emit 源无遗漏。
- **又一次 dedup+缺陷修复二合一**：原 ExoticActivate 只在打开 `textarea.focus()`、**无 Tab 焦点陷阱**(Tab 可跳出对话框)。迁 UiDialog 后自动获得 useFocusTrap 的 Tab 循环+焦点归还——与折叠/树两对话框同构:被迁组件缺失的正是原语已内建的能力。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **54 files 720 tests（迁移零测试回归）** / 7 主题对比度全硬门槛过 / build 2.81s / diff --check 0。构建产物 `ExoticActivateDialog-*.css` 缩到 0.70kB(外壳样式删除的客观旁证)。**焦点陷阱/Escape/遮罩/激活在途禁关属 DOM 运行期,结构/aria 经 UiDialog spec 已 SSR 门禁验+标注真机验收,不声称交互已验证。** UiDialog 累计迁 5/7 对话框。
- 余下对话框：其余 2 个（OnboardingWizard/FaceApprovalPanel）——见下节，二者均需先给 UiDialog 加 #header 插槽。

### S1 摊薄 UiDialog（终）：加 #header/关闭控制/maxHeight 三扩展 + 迁 Onboarding/FaceApproval（6cfbe9b / f8d7673）
- 承接「无人值守」。三待迁对话框按契约差距分诊后:ExoticActivate 已迁(上节);OnboardingWizard/FaceApprovalPanel **都卡在 UiDialog 缺 #header 插槽**(前者头含副标题+3 点步骤进度条 role=progressbar、后者头含副标题),故先给原语补三处**加法扩展**再迁。
- **UiDialog 三扩展(均默认向后兼容,现存消费者零变化)**：①`#header` 具名插槽(带 fallback)——缺省渲染标准「标题+关闭键」行,消费方传则整体覆盖(自定义头部用);须在其中放 id=titleId 的标题元素维持 aria-labelledby 关联。②`close-on-overlay`/`close-on-esc`(默认 true)——点遮罩/Escape 关闭经开关门控,首启向导置 false 只留显式按钮出口(关闭键 X 不受门控)。③`maxHeight`——设则 content 内联 max-height + 加 `dialog-content--capped` 类,**该类下** .dialog-body 成唯一滚动区(flex:1+min-height:0+overflow-y:auto)、头/脚 flex-shrink:0 固定。补 2 例 SSR 契约(#header 覆盖 / maxHeight+capped 类)。
- **迁 OnboardingWizard(6cfbe9b,6/7)**：范式② v-if 挂载传 :open='true';外壳(overlay/content/两 keyframes)交基座;自定义头(副标题+步骤点)走 #header;3 步正文进默认插槽,body-padding='0' 让 .onboarding-body 自持 spacing-lg 内边距+min-height:220px 定高(跨步骤高度稳定)+居中;跳过/导航按钮改 UiButton(ghost/secondary/primary),分栏页脚 footer-split;禁遮罩/Escape 关闭走 close-on-overlay/esc=false + show-close=false。**顺带修**:此前无 Teleport+无焦点陷阱(迁后补齐)、补 h2 id 令 aria-labelledby、清违「交互文本禁用 tertiary」的 skip scoped 色(→ghost secondary)。
- **迁 FaceApprovalPanel(f8d7673,7/7 收官)**：范式② v-if;自定义头(标题+副标题+X)走 #header;分组列表满高可滚——走 maxHeight='84vh' 触发 --capped(正文独占滚动、.approval-header 加 flex-shrink:0 配合);680px 走 max-width;无 footer(不传插槽)。**顺带修**:此前无 Teleport+无焦点陷阱、补 h2 id、新增 Escape 关闭(标准模态 a11y,关闭仅弃本地选区无数据损失)。宽度 92%→100%(max 680)极窄视口微差。
- **给共享原语加布局能力用条件类隔离**：maxHeight 的滚动布局若无条件加到 .dialog-body 会波及全部 6 对话框 flex 尺寸行为(需逐个真机核),改为 `--capped` 类限定→现存对话框字节级零变化,回归面归零。这是「加法扩展默认向后兼容」在 CSS 布局层的落地。
- **门禁抓到的测试脆弱性(顺带硬化)**：#header 插槽注释含字面「aria-labelledby」,而 Vue SSR 会把模板注释渲染进串,使既有负向断言 `not.toContain('aria-labelledby')`(裸子串)误命中。正解=把断言收紧到属性形式 `aria-labelledby=`(带 `=` 后缀,注释无此形)——这才是本测真正意图。教训:SSR 串断言的负向裸子串检查易被注释/文案误命中,应断言属性/标签的结构形式。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **54 files 722 tests（+2:#header/maxHeight;另修正 aria-labelledby 负向断言为属性形式）** / 7 主题对比度硬门槛全过 / build 2.82s(PersonsView CSS 缩小=外壳移交基座) / **scoped prettier --check 三文件合规**(模板重缩进用 scoped prettier --write 单文件,非全库 format,单表达式处理器无 rewrap 风险) / diff --check 0。**焦点陷阱/Escape/遮罩门控/长列表滚动/多步或多组交互属运行期,结构/aria/插槽/capped 类经 SSR 契约验+标注真机验收,不声称交互已验证。UiDialog 迁 7/7 对话框,全库模态外壳全摊薄收官(唯一实现)。**

### S1 .btn-icon → UiIconButton 广域迁移（5250087 第一批 / 377b73c 第二批）
- 承接「无人值守」。UiDialog 7/7 收官后转 .btn-icon 广域迁移（此前只 ContextualToolbar 一处消费 UiIconButton）。grep 定位 8 消费文件 34 站点（ContextualToolbar 已迁不计），其中仅 ContentViewer(`.detail-controls .btn-icon` 暗色浮层)/AppToolbar(`.toolbar__foldable > .btn-icon` 折叠布局)带 scoped 覆盖。
- **视觉零风险机理（修正前期「需真机」的过度悲观）**：memory 已记且本轮复核——UiIconButton 根 button 仍挂全局 `.btn-icon` 类，且 **Vue 3 子组件根元素同时携带父作用域 data-v**，故父级 scoped 后代选择器(`.detail-controls .btn-icon`、`.toolbar__foldable > .btn-icon`、消费方额外类 `.scan-root__remove`/`.danger-icon` 经 fallthrough)迁移后仍命中根元素。迁移唯一实质变化=`label` 从可省 aria-label 变必填 prop（类型层杜绝无名图标按钮）。故「暗色浮层需真机」降级为「结构安全，真机仅作精确视觉确认」。
- **第一批 6 无覆盖文件 20 站点[5250087]**：SidebarFooter(2)/ManagementSection(2 含扫描 toggle active=isRunning)/DynamicSettingControl(2 compact danger·plain)/GalleryViewControls(2 布局·排序)/FoldersSection(3 #actions 插槽)/ToolsSection(9 缩略图·AI 四控·人脸四控含 :disabled)。
- **第二批 ContentViewer 10 站点暗色浮层[377b73c]**：zoom/rotate/LIVE·faces·favorite(active)·explorer·info·信息栏关闭。active 三处→`:active` 保 `.detail-controls .btn-icon.active` 强调色。均无 toggle（保持行为 parity，原无 aria-pressed；开关型 aria-pressed 属另行 a11y 增强）。
- **AppToolbar 3 站点留待（真实技术阻断，非真机）**：H-Lab(1)可迁，但 filter/view(2)带 `ref=filterBtnRef/viewBtnRef` 模板 ref 做弹层锚定定位——迁 UiIconButton 后 `ref` 指向**组件实例而非 DOM 元素**（弹层 getBoundingClientRect 会失效），须先给 UiIconButton `defineExpose` 暴露根 `<button>`。1/3 部分迁移无意义，整体留待原语加 ref 转发。
- 证据：两批各自门禁（本地,非 CI）typecheck 0 / lint 0 / 54 files 722 tests（迁移零测试回归）/ build ~2.8s / scoped prettier --check 合规（属性换行用 scoped prettier --write 逐文件，单表达式无 rewrap）/ diff --check 0。视觉观感（暗色浮层白字、active 强调、折叠测量宽度）属真机，结构安全已由门禁 + 父作用域命中机理保证，标注真机验收。**.btn-icon → UiIconButton 累计 7/8 文件 30/34 站点。**

### S1 收 AppToolbar（.btn-icon 34/34 收官）+ UiField 原语 + 迁 4 消费者（用户三裁决后续做）
- 承接用户三裁决（AppToolbar defineExpose 收全 3 站点 / UiField 双关联模式并存 / UiField 全量迁 ~10 消费者）。
- **AppToolbar 收尾[937ff03]**：UiIconButton 加 `defineExpose({ el })`（ref 落组件取到实例，须显式暴露根 <button>）。filter/view ref 类型 HTMLElement→`InstanceType<typeof UiIconButton>`，弹层 `.value.getBoundingClientRect()`→`.value?.el.getBoundingClientRect()`；H-Lab 直接迁。typecheck 0 印证 `.el` 类型链通过。**.btn-icon → UiIconButton 8/8 文件 34/34 站点全收官。**
- **UiField 原语[dba6833]**：清点全库表单字段得 **6 种碎裂范式 A-F**（清点 Explore 代理，见 findings 会话续8），wrapping-label 家族最普遍，A/D/E/F 有真 label-未关联缺陷；两朝向并用。API：associate='nest'（包裹 label 隐式关联）/'for'（label[for]+控件[id] 显式，useId 生成，SSR 安全）；orientation stacked/inline；hint/error（**刻意置 label 外**避免并入可访问名，其 id 经 describedby 汇聚供控件绑 aria-describedby）；labelHidden sr-only。结构=root div > group(nest=label/for=div，含 caption+控件) + hint/error span。与 SettingRow 正交（后者=设置页注册表行壳）。8 例 SSR 契约。
- **迁 4 消费者**：B1[b39b57b] FolderCreate(A，basePath 复合体用 for、余 nest；**修真 a11y**)/Proofread(C nest dedup)/SemanticSearch(D range nest inline；**修真 a11y**)；NetworkStorage[01278ec]（B canonical dedup，class passthrough 传 --grow/--kind；**选择器重锚** `.ns-field 后代`→`.ns-form 后代`）。
- **余 5 消费者 defer（清点后判定，非同构变更需真机 / 非 UiField 形态）**：迁移不同于 UiDialog/.btn-icon 的「构造即视觉等价」——UiField 迁移改 label 字号(xs→sm)、布局(space-between→gap)、加 wrapper div，属**真实视觉变更**，force 迁移会引入真机不可验的静默视觉退化。逐个：DocumentViewer（工具栏 select xs→sm 密度变 + 紧凑编辑栏 flex 重构）、ReaderSettings（rs-row space-between 布局 UiField inline 只 gap 不 justify + stepper/segmented 非原生控件）、HGalleryLab（experimental lab 工具低值 + `.hlab__param input[type=range]{width}` 重锚 + checkbox 出范围）、ReplacementPanel（紧凑 find/replace 行非字段形态）、SettingsView（checkbox 属独立部件、search=labelHidden 低值）。**checkbox/toggle/stepper/segmented 宜作独立 UiCheckbox/UiToggle 原语，非 UiField 职责。**
- 证据：各批门禁（本地,非 CI）typecheck 0 / lint 0 / **55 files 730 tests（+8 UiField）** / build ~2.8s / scoped prettier --check 合规 / diff 0。字段视觉/布局属真机，结构/label 关联经 UiField SSR 契约 + 各文件门禁验，标注真机验收。**UiField 原语建成 + 迁 4/9 消费者。**

### S1 UiToggle 原语 + 迁 DynamicSettingControl（同构收单一开关面）+ UiSelect 边界诊断（ec8e964）
- 承接「继续推进」+「无人值守，完成所有无需决策的工作」。挑下一个**干净、同构、可无真机验证**的增量：侦察 checkbox/toggle 生态定位到全局 `.toggle` 开关组件已有完整 CSS（`label.toggle > input[type=checkbox] + span.toggle__thumb`，选中/滑动/焦点态全走纯 CSS `:has(input:checked)`/`input:checked ~ .toggle__thumb`/`:focus-within`，无 JS 态）。
- **UiToggle 原语[ec8e964]**：同构包裹 `.toggle`/`.toggle__thumb`——复用已过 contrast 门禁的全局样式，收单一 `v-model`（`:checked` + `@change` emit 布尔，与原生 checkbox v-model 语义一致）；`disabled` 透传原生；**可选 `label`→`aria-label`** 为 a11y 改进入口（原 `.toggle` 结构无可访问名，属缺口；不传则与原结构保持 parity，避免破坏机械迁移的字节等价）。7 例 SSR 结构契约（.toggle 根 + 隐藏 checkbox + thumb / checked 映射 modelValue / disabled / aria-label 可选 / class fallthrough 合并到根）。
- **迁 DynamicSettingControl（单行替换，高杠杆）**：`<label class="toggle" :class><input v-model><span thumb></label>`（4 行手写三元）→ `<UiToggle v-model="toggleModel" :class="{ 'compact-toggle': compact }" />`。此组件**注册表驱动**——所有 `control:'toggle'` 的设置项都经它渲染，迁一处即覆盖整个设置开关面。`toggleModel` 是直调 setter 的 writable computed，`v-model` 赋值触发 setter，行为零漂移。`.compact-toggle`（DynamicSettingControl 作用域，纯 `transform:scale(0.8)` 作用于根、无后代选择器）经 class fallthrough 合并到 UiToggle 根 `<label>`，child-root 携父 data-v 仍命中 → 同构零视觉风险。
- **同构校验先行**：迁移前先 grep DynamicSettingControl scoped 块确认**无** `.toggle input`/`.toggle__thumb` 后代选择器（若有会因 select/thumb 移入子组件而静默失配）——仅 `.compact-toggle` 作用于根，安全。
- **UiSelect 诊断为边界，停下上报（未建）**〔🔴 已由下方「UiSelect 原语」小节推翻：用户裁决「继续 3」后已建成并迁移，`:deep()` 重锚已落地，见 2753777〕：同一 DynamicSettingControl 还有 `control:'select'`（`.select-wrap > select.select`，同样注册表驱动），本可比照建 UiSelect。但**同构校验命中真隐患**：`.compact-select-wrap .select`（line 435-441，给紧凑 select 实打实的 padding/font 12px/height 26px 尺寸）是**父 scoped 后代选择器**——把 `<select>` 移进 UiSelect 后，内层 `.select` 只带 UiSelect 的 data-v、不带父 data-v，`.compact-select-wrap[data-v-parent] .select[data-v-parent]` 静默失配 → 钉住区紧凑设置面板 select 悄悄变回默认尺寸。这是 findings 会话续8「后代选择器穿不透子组件」隐患的**首次真实发生**。可用 `:deep(.select)` 显式穿透修复（推理可证等价），但它改的是**无自动门禁可验的 compact 面板 computed style**（SSR 断结构不断 computed style）。撞上已知隐患类首例时，无人值守正确本能=**停下上报**，交用户裁决是否 `:deep()` 重锚，而非径直套 workaround 再贴标签。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **56 files 737 tests（+7 UiToggle，零回归）** / build 3.06s / scoped prettier --check 三文件合规 / diff --check 0。**开关选中/滑动/焦点态属 DOM 运行期（纯 CSS 驱动），结构/checked 映射/aria 经 SSR 契约验 + 标注真机验收，不声称交互已验证。** UiToggle 迁 1/1 同构消费者（DynamicSettingControl，注册表驱动）。

### S1 UiCheckbox 表单复选框原语 + 迁 Confirm/CloseConfirm（蒸馏既有视觉并 dedup）（94a8c35）
- 承接用户裁决「UiCheckbox 先定视觉」。**「先定视觉」塌缩成「蒸馏既有 de-facto 模式」**：侦察发现全库表单 checkbox 早有既定视觉语言——`accent-color: var(--color-accent)` on native checkbox（主题感知、零自绘 SVG、原生键盘/读屏 a11y 全保留），且 ConfirmDialog/CloseConfirmDialog 两处**重复定义**了同款 `.remember-checkbox`（label flex + gap 8px + font-size-sm secondary，input 16×16 + accent-color）。故无需发明新视觉，把它蒸馏进 UiCheckbox 即可。
- **UiCheckbox 原语[94a8c35]**：字节复刻 `.remember-checkbox`，收单一 `v-model`（`:checked`+`@change` emit）+ `label`/默认插槽（插槽优先）+ `disabled`（加 `--disabled` 视觉）。可访问名经 `<label>` 包裹隐式关联（label 文本即控件名），无需额外 aria。加 `flex-shrink:0` 防御性加固（长标签下 16px 方框不被挤扁；当前短标签消费者无挤压压力，视觉零变化——已如实披露）。
- **⚠️ 类名碰撞规避**：全局 `.checkbox` 类**已被 MediaThumb 网格选择控件**（20px 圆 + 白勾，canvas 镜像 MediaGridCanvas）占用；若在 index.css 定义全局 `.checkbox` 会层叠泄漏污染那个控件。故 UiCheckbox **样式作用域私有 + 类名 `.checkbox-field`** 彻底避碰（与「wrap 既有全局类」的 UiButton/UiToggle 不同——此原语是**定义新视觉**，作用域私有正合适）。
- **迁 ConfirmDialog + CloseConfirmDialog（同构 + dedup）**：二者 `.remember-checkbox` 字节等价（CloseConfirm 仅多 `margin-top: spacing-sm`）→ 字节复刻使迁移**同构可无真机**，且 dedup 两处重复本地 CSS。`<label class=remember-checkbox><input v-model><span></label>` → `<UiCheckbox v-model :label />`。CloseConfirm 的 margin-top 经 class fallthrough（`.close-remember-mt` 落到 UiCheckbox 根 label，child-root 携父 data-v 仍命中）保留。ConfirmDialog/CloseConfirmDialog 本地 `.remember-checkbox` scoped CSS 归零。
- **余 4 处默认蓝 checkbox defer（非同构）**〔🔴 本条「4 处/笼统需真机」的口径已被会话续11 源码复核（细分 5 站点、区分「视觉非同构」与「契约不符」两类阻塞）+ 会话续13 终态裁决（out-of-scope-by-design）推翻，当前结论见下方「UiCheckbox 迁移终态裁决」小节〕：HGalleryLab/ReplacementPanel/SettingsView/ReaderSettings 的裸 checkbox **无 accent-color = 浏览器默认蓝**（grep accent-color 仅 3 处命中）→ 迁 UiCheckbox 会把默认蓝改成主题 accent + 16px 定尺寸 = 真实视觉变更 = 非同构，force 迁移引入真机不可验的静默视觉变更，defer 真机。（HGalleryLab 本就 experimental 低值。）
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **57 files 744 tests（+7 UiCheckbox，零回归）** / build 2.84s / **对比度门禁全过（`--color-accent × --color-bg-primary: 4.69 ≥3` 印证 accent 对比安全）** / scoped prettier --check 四文件合规（UiCheckbox.vue 经 scoped --write 规整内联 slot 空白，语义不变）/ diff --check 0。**勾选/焦点态属 DOM 运行期（原生 checkbox + accent-color 驱动），结构/checked/label 关联经 SSR 契约验 + 标注真机验收，不声称交互已验证。** UiCheckbox 迁 2/6 同构消费者。

### S1 UiSelect 下拉原语 + 迁 DynamicSettingControl（用户裁决「继续 3」跨子组件边界，:deep() 重锚）（2753777）
- 承接用户裁决「继续 3」——推进上一会话诊断为边界、停下上报的 UiSelect（此前判定需 `:deep()` 重锚 compact 后代规则，涉无门禁可验的 computed style，交用户裁决）。用户拍板走 `:deep()`。
- **UiSelect 原语[2753777]**：同构包裹全局 `.select`/`.select-wrap`（含 `::after` 下拉箭头）。结构 = `.select-wrap` div > `select.select` > **默认插槽渲染选项**。设计要点：①**选项走默认插槽**由消费方渲染（保持 `<option v-for>` + i18n 内联字节等价，原语不依赖 i18n keys，且可容纳任意 optgroup）；②**v-model 经 writable computed** 桥接（get 读 modelValue、set 向父 emit）走 Vue 规范 vModelSelect 指令，选项选中/change 读值由其在运行期处理；③可选 `disabled`/`ariaLabel`，canonical 消费者不传 → 与原 `.select` 结构 parity。
- **全库 `.select` 唯一消费者 = DynamicSettingControl**（grep 证 ReaderSettings 用自有 `.rs-select`、SemanticResultCard 用 `.select-icon` 均无关）。注册表驱动——所有 `control:'select'` 设置项经它一处渲染，迁这一处即 100% 覆盖全局 `.select` 用法（与 UiToggle 同为「注册表驱动单消费者」最优标的）。
- **跨越首个「父 scoped 后代选择器伸进子组件」边界**（这是本次的核心，也是上一会话停下上报的那个隐患的真实解决）：`<div class="select-wrap" :class><select class="select">…</select></div>` → `<UiSelect v-model :class="{ 'compact-select-wrap': compact }"><option v-for…/></UiSelect>`。`.compact-select-wrap` 类经 fallthrough 落 UiSelect 根（`.select-wrap` div，携父 data-v），`.compact-select-wrap`（直接作用于 wrap，431-434）仍命中；唯 `.compact-select-wrap .select`（435-441 后代伸进 `<select>`）因 select 移进 UiSelect（UiSelect 无 scoped style → select 无任何 data-v）而失配 → 改 `.compact-select-wrap :deep(.select)` 穿透子组件边界重锚。
- **等价性可推理证明**：迁移前编译成 `.compact-select-wrap[data-v-X] .select[data-v-X]`（两元素同在本组件都带 X）；迁移后 `:deep(.select)` 编译成 `.compact-select-wrap[data-v-X] .select`（去 select 侧 data-v 约束）。同一 wrap 作锚（fallthrough 后仍带 X）、同一 select 作靶、同一声明 → computed style 可证等价。默认（非紧凑）select 本就走全局 `.select`/`.select-wrap`/`::after`（无 data-v）字节不变。
- **诚实边界**：紧凑面板 select 的**实际渲染尺寸**（padding/font 12px/height 26px）**无自动门禁覆盖**（SSR 断结构/属性/插槽转发，不断 computed style；对比度门禁只覆盖 token）——需真机做最终视觉确认。但这是**推理级等价，非盲改**，与「首次遇到隐患类时套 workaround 贴标签」有本质区别（那次没有用户裁决、且是首遇；这次有用户裁决 + 可证等价）。
- 证据：门禁（本地,非 CI）typecheck 0 / ESLint 0 / **58 files 751 tests（+7 UiSelect，零回归）** / build 2.88s / **对比度门禁全过** / scoped prettier --check 三文件合规（UiSelect.spec.ts 经 scoped --write 规整长 h() 行，非 Vue 模板无 rewrap 风险）/ diff --check 0。**选项选中/change 属 DOM 运行期（vModelSelect 依赖真实 DOM），结构/插槽/属性经 SSR 契约验 + 紧凑尺寸标注真机确认，不声称交互与 computed style 已验证。** UiSelect 迁 1/1 同构消费者（DynamicSettingControl，注册表驱动）。

### S1 UiCheckbox 迁移终态裁决（out-of-scope-by-design）+ S1 真机验收清单产出（「继续做完所有工作，需测的直接说」）
- 承接用户「继续做完所有工作，有需要我测的地方直接说」——工作边界从「遇真机即停」变为「能推进的推进，需你测的攒清单」。守一条纪律区分：**「需真机」分两类**——(a) 结构 1:1、仅像素级 computed style 无门禁可验（如 UiSelect 紧凑尺寸），可迁移 + 列测试清单；(b) 迁移会真实重构 DOM/改布局 = 盲设视觉，不能闷头改。据此逐项复核剩余项。
- **UiCheckbox 余 5 站点：读全部当前源码 + scoped CSS 后下终态裁决 = out-of-scope-by-design**。逐站点地面真相：① **HGalleryLab(48/66)** `<label class=hlab__check><input v-model type=checkbox>text</label>`——契约吻合，但 `.hlab__check{gap:4px}` 与 `.checkbox-field{gap:8px}` 经 class fallthrough 落同一根元素时**同权靠源序**（跨组件 style 注入序不定，不可靠），且原生蓝→accent + 加 font-sm/secondary 是真实视觉变更；dev/lab 低值。② **ReplacementPanel(28)** `:checked`+`@change`（非 v-model）**裸** checkbox（`.repl-row` 直接 flex 子，无 label 壳）+`:title` 在 input 上。③ **ReplacementPanel(48/70)** `.repl-row__re{inline-flex;gap:2px;font-mono;10px}` 的 `.*` 微字形 toggle——套 `.checkbox-field` 基座（gap 8px/font-sm/包 span）会打烂紧凑字形。④ **ReaderSettings(317)** `.rs-row--toggle{space-between}` **反序**设置行（`.rs-row__label` 标签左·checkbox 右），`:checked`+`@change`——本质是 SettingRow 形态非 remember-checkbox。⑤ **SettingsView(88)** `:checked=includes()`+`@click($event,val)` **集合成员切换**，根本不是布尔 v-model。
- **裁决理由**：这 5 处各具**独立 bespoke 视觉/契约**，无一是 UiCheckbox 蒸馏的 `.remember-checkbox` de-facto 形态。force 并入须给原语加 `:checked` 模式 / `@change`·`@click`·`:title` 透传 / 裸模式 / 反序布局 / mono 字形——**为 DRY 而 DRY，bloat API 且毁掉「labeled accent remember 勾选框」的视觉内聚**（违高内聚/低耦合/可读人本）。UiCheckbox 真实 de-facto twins（Confirm/CloseConfirm）已 2/2 收，这是它的**迁移终态**。裁决可逆、已文档化，符合项目「不冻结契约 + 高内聚 + YAGNI」哲学——保持 bespoke 是保守/可逆的默认，force 合并才是激进不可逆的一方。**若你要 lab-view 一致性或把 reflow 行改 UiToggle，可 override 本裁决。**
- **S1 真机验收清单产出**：`realtest-checklist-S1.md`——覆盖 S1 全量已交付迁移（UiButton/UiIconButton 34/34/UiDialog 7/7/UiField 4/UiToggle 1/UiCheckbox 2/UiSelect 1 + transition 清理 + danger zone + roving tabindex），只列门禁盲区（像素 parity / 焦点陷阱 / 弹层锚定 / 动画 / 跨主题），⚠️ 三个高优先盲区置顶（UiSelect 紧凑尺寸 / AppToolbar 弹层锚定 / 此前无焦点陷阱的两对话框）。直接回应「需测的直接说」。
- **本轮产出**：无新前端代码（clean 同构增量已源码实证枯竭，见会话续11）——产出 = ① 用当前源码 + scoped CSS 地面真相把 UiCheckbox 余 5 站点从「待裁决」推进到**终态裁决**（消除 dangling 决策项，防未来会话重开）② S1 真机验收清单（把累积未验的 S1 全量整理成可勾选测试项）。**证据级边界确认 + 决策收口，非制造低价值变更充数。**

### 首轮真机反馈 6 项修复/特性（用户实测 #1-#5 + 同类 #497）
- 用户跑真机后反馈 5 项(#3 数据丢失级、#4 重大回归)。全部处理:4 bug 修复 + 2 产品特性(用户 AskUserQuestion 裁决方向)。提交 ab88960 / 43b3aab / a4d4fda / 5776ae4;验收清单 `realtest-round2-2026-07-12.md`。
- **#3 数据丢失(ab88960,CRITICAL)**:危险区四项(清库/设置/缩略图/日志)点击**无确认直接执行**。根因=用的是原生 `window.confirm()`,Tauri v2 WebView2 里**不弹框却返回 truthy**→危险操作穿透。全库其它确认早已走 `useConfirm()` 单例 + ConfirmDialog(promise 版),唯独危险区漏用。**此前文档/记忆声称的「危险区 confirm 契约」代码在、真机从未生效=典型「门禁绿真机废」,已在 commit 如实纠正。** 修复:四 handler 改走 `useConfirm()`;扩 useConfirm/ConfirmDialog 加 `danger`(红色确认键)+ `requireText`(输入确认强门,opt-in,既有调用零影响);清库须键入「清除数据库」/"DELETE" 才启用确认键(防反射式点击穿透);useConfirm.spec 加 3 例锁定契约。
- **#497 同类(43b3aab)**:SettingsView 高级元数据警告也用原生 `window.confirm`→同改 useConfirm;异步无法 await 后 preventDefault,取消时手动撤销原生勾选 + 跳过集合写入(:checked 绑定值仍 false,Vue 下帧对齐)。
- **#1 焦点(43b3aab)**:FolderCreate「创建」键 `:disabled="!canCreate"`→原生 disabled 按钮不在 Tab 焦点序(useFocusTrap 按 `button:not([disabled])` 正确排除)。改恒可聚焦 + create() 内校验缺项给行级错误(WAI-ARIA 推荐的可达性模式:disabled 提交键对键盘/读屏不友好)。
- **#2 缩进(43b3aab)**:`.danger-zone__hint` 是 CollapsibleCard slot 直接子无行内 padding,补 `padding-left:12px` 对齐下方 SettingRow(`.settings-card__item` 的 12px)。
- **#4 标题栏合并切换(5776ae4,用户裁决「加开关」)**:工具栏与标题栏分离=S3 提交 bcf0934 有意所为(推翻 Phase G e036787 合并)。新增 useTitlebarMode composable(模块级单例+localStorage,默认合并),App.vue merged 时 AppToolbar 并入 WindowChrome(照 Phase G 结构)/分离走独立 #toolbar 槽,titlebarMerged 互斥、切换 live(slot v-if 反应式)。AppToolbar 是多根 fragment,两根直接作 flex 子项流入(不加 class:多根无法继承+避 Vue 警告)。设置界面段加 toggle。
- **#5 DOM/Canvas 设置开关(a4d4fda,用户裁决「设置页实验开关」)**:切换 dev-gate 隐藏也是 bcf0934 所为(canvas 是原型)。新增 useRenderMode composable(同 useTitlebarMode 范式),渲染态从 MediaGrid 局部提升为共享单例;settingsMap 开发者工具段加 galleryRenderMode/timelineRenderMode 两 select;canvasCapable 不满足仍自动回退 DOM。
- **两特性共用范式**:localStorage-backed 模块级响应式 composable(useTitlebarMode/useRenderMode)= 让设置页开关与消费组件共享同一 ref、live 生效、不动后端 app_config schema。**注册表驱动的 DynamicSettingControl 加一条 binding 即接入设置页**——与 S1 UiToggle/UiSelect「注册表驱动单消费者」同一杠杆。
- 门禁(本地,非 CI):四提交各自 typecheck 0 / lint 0 / vitest 754(+3 useConfirm)/ build ~2.8s。**三大 .vue(App/MediaGrid/SettingsView)既存非-standalone-prettier-clean(整模板从第 1 行起,仓库历史状态)——真实格式门是 ESLint(已过),改动行均 lint-clean;绝不 `prettier --write` 这些文件(会 rewrap 整模板炸构建,CLAUDE.md 红线)。** 真机验收待确认见 round2 清单(合并模式观感/输入确认门/焦点可达/渲染切换 live 属真机)。

### 会话续 15：round-2 验收通过 + S5 阶段11「Collections 删除 undo / Persons ignored 历史管理」两子缺口收官

- 用户「验收通过,继续推进」：round-2 真机六项确认通过 → `realtest-round2-2026-07-12.md` 按写回红线转「已验收通过」(frontmatter + 横幅 + 勾选,f8fd873)。
- 选题：用 Explore 摸清后确认 S5 阶段11 剩「Collections 删除 undo、ignored 历史管理」是当前**决策无关、可门禁验证**的实打实缺口(其余 S 项依赖真机/产品决策)。分两 stage 单子系统推进。
- **核心发现（Explore 地面真相）**：① Collections 删除是**硬删除**(`DELETE FROM albums` + `album_items` 级联)——**undo 能力是数据模型属性,硬删除无可撤销对象**,故 Persons 式客户端 undo 无法直接镜像,须后端升级软删除。② 「ignored 历史管理」经查实指 **Persons `is_ignored` 误检桶**(标记后仅 5 秒 undo,之后无处查看/恢复,缺口 §2.6),与 collections 无关。
- **Stage 1：Collections 删除 undo（软删除,73b698a + 既存 lint 修 d1c2d3f）**
  - 后端：V18 迁移 `albums.deleted_at`(migrate_step 版本事务保非幂等 ALTER 恰一次,`migrations_are_idempotent` 已证重跑安全);`delete_collection` 改 `UPDATE SET deleted_at`(仅 kind='user' + `deleted_at IS NULL` 守卫防双删覆盖);新增 `restore_collection` 清零;list/recent 过滤 `deleted_at IS NULL`。restore_collection IPC 命令 + lib.rs 注册。
  - 前端：`IPC.RESTORE_COLLECTION` + `collectionStore.restore`;CollectionsView 删除后 5 秒 undo toast → restore(镜像 Persons)。i18n `collections.deletedDone` 中英平衡。
  - 单测 `soft_delete_and_restore_collection`(软删从 list 消失/deleted_at 置位/成员保留/restore 复现/系统夹保护)。**v17 重放测试修**：回拨版本重放旧迁移时须撤销 V18 的 `deleted_at` 列(非幂等 ALTER 否则 duplicate)——凡「回拨 schema_version 重放」式测试对未来非幂等迁移天然脆弱,修法=回拨时 DROP 所跨越的后续列,使「回拨到 N」真等价「库处于版本 N」。
  - 顺带修并行会话 `9b64751` 引入的既存 lint 债(install-skills.mjs 未用 import `sep`,阻塞全库 eslint;工作树零改动、Boy Scout 单独提交)。
- **Stage 2：Persons ignored 历史管理（a5dc9ce）**
  - 后端：`list_persons` 重构抽共享体 `list_persons_by_ignored(conn, model, ignored)`(墙=0/误检桶=1 各一行委托,避免重复 ~40 行 SQL);新增 `list_ignored_persons` + `list_ignored_face_persons` 命令 + 注册。单测 `list_ignored_persons_returns_only_ignored`。
  - 前端：**来源不对称**是设计关键——hidden 人物在 `store.persons`(服务端返回,前端 `visiblePersons` 过滤),ignored 人物服务端 `WHERE is_ignored=0` 不返回,须**独立列表 `ignoredPersons` + 独立拉取**。`setIgnored` 在墙↔误检桶两列表间**乐观搬移**(移入桶从 persons 删/加 ignoredPersons 首位,移出反向 + load() 重载墙)免二次拉取。PersonsView **三态展示(墙/隐藏/误检桶,两切换互斥+清合并选区)**;头部「已忽略 (N)」入口,桶内卡片只留「移出误检桶」恢复键(RotateCcw),隐去选择/隐藏/误检;`.person-card__restore` 复用共享动作按钮基座。i18n showIgnored/restoreFromIgnored/restoredIgnoredDone 平衡。`onMarkIgnored` 过时注释回写(「暂无显示已忽略 UI」→ 本次闭环)。
- **门禁（本地,非 CI）**：Stage 1 cargo test db 126/0 + typecheck 0 / lint 0 / vitest 754 / build 2.85s;Stage 2 **cargo test lib 全量 492/0**(list_persons 重构零回归)+ typecheck 0 / lint 0 / vitest 754 / build 2.80s。真机验收待确认：收藏夹删除→undo toast 恢复夹与成员、系统夹无删除入口;标记误检→头部「已忽略(N)」出现→切入桶见该人物→「移出误检桶」恢复上墙。
- **教训（本会话）**：① **undo 能力是数据模型的属性,不是 UI 的属性**——设计删除操作时就该决定是否可逆(硬删除后加 toast 无意义)。② 「回拨版本重放」式迁移测试对未来非幂等 DDL 脆弱,须在回拨处撤销所跨越的非幂等变更。③ 并行会话既存 lint 债:先 `git diff HEAD -- <file>` 判定是工作树在途还是 committed 基线,committed 且工作树零改动的 1-token 修复可安全单独提交。④ 自定义 Tauri v2 命令**免 capability 声明**(default.json 只有 plugin 权限,7 个现存 collection 命令零声明照跑)。

### 会话续 16：round-2 验收后继续推进 + S5 阶段11「Gallery empty-state 重构」收官（e0a85ca）

- 用户「我在测,你继续推进」：round-3(Collections/Persons undo)真机验收进行中,并行推进 S5 阶段11 唯一剩项 Gallery filter/selection/empty-state 重构。
- **范围判断（两路 Explore 测绘）**：这项是**开放式重构**(范围由设计意图 + 代码债共定),先测绘再定边界防蔓延。三块可交付性差异极大——filter 主体(overflow-fold/chips/roving/URL 同步 S2-b)已建成,剩 a11y 契约需 **UiPopover 原语=需发明定位抽象属决策** + 真机;selection 改造(浮动可拖胶囊→底部固定动作条)属**产品决策 + 真机拖拽验证**;唯 **empty-state** 是决策无关 + 可门禁验证 + 修真实 bug 的完整切片。锁定 empty-state。
- **交付**：① **UiEmptyState 原语**(icon/title/description + primary/secondary 动作;**同构包裹全局 `.empty-state*`**,视觉由未动全局 CSS 提供构造即等价、免真机;纯结构无 scoped 样式;5 例 SSR 契约)② **resolveGalleryEmptyState 纯决策源**(照 resolveView.ts 范式:决策与呈现分离,返回 i18n key + 动作枚举而非渲染串/图标组件;precedence search > filtered > directory > smartAlbum + 动作门控;10 例穷举 spec)③ 迁 MediaGrid 内联空状态(收敛 emptyStateText/showEmptyAction 双 computed)。
- **核心修真实错 CTA**（设计 §7.1「所有 empty state 都含下一步」）：全局筛选态激活却零结果时,此前 precedence 无 filter 分支 → 落「空库·添加文件夹」误引导加目录。新增 **filtered-empty 分支置于视图维度之上**——filter 是全局持久态,切视图不清,极易留 stale filter 静默藏空(在文件夹里看不到照片却不知是上个视图残留筛选),「清除筛选」是通用逃生口(说明文案不宣称当前视图有照片,对真空 + stale filter 也诚实)。动作 clear-filters 复用既有 `toolbar.clearFilters`(不新增重复键,DRY)。
- **顺带消 \n-split 脆弱约定**：allPhotos/favorites 的「标题\n说明」此前塞进一个 i18n 串再按 `\n` 切分(翻译须保留换行、单行键静默丢说明)——拆为独立 `*Title`/`*Desc` 键,原语走 title/description 双 prop。全局 `.empty-state__action` 死类替换为 `.empty-state__actions` 横向容器。
- **门禁（本地,非 CI）**：typecheck 0 / lint 0 / vitest **769**(61 文件,含 localeIntegrity 中英平衡 + resolveEmptyState/UiEmptyState 两新 spec)/ build 2.82s。真机盲区(filtered-empty 文案/FilterX 图标/清除筛选交互)见 `realtest-round4-2026-07-13.md`。
- **教训（本会话）**：① **开放式重构先测绘定边界再动手**——三块里只挑决策无关 + 可门禁验证 + 修真实 bug 的完整切片(empty-state),把依赖决策/真机的两块(filter popover/selection bar)显式留档并注明阻塞类型,不硬凑充数。② **模板 HTML 注释会渲染进 SSR 串**——注释里写裸类名 `.empty-state__actions` 使负向断言恒失败;稳健修法=注释不嵌裸类名 + 断言收紧到属性形式 `class="X"`(会话续12 已记教训,门禁当场抓出复现)。③ **决策与呈现分离**(复用 resolveView 范式)——把藏在 2290 行组件里的 precedence 抽成纯函数,那个导致错 CTA 的逻辑才获得独立可测身份。

### 会话续 17：S5/S3 弹层定位前置——UiPopover 原语（@floating-ui 引擎）+ 迁全库 3 处手写弹层（4b02272 / 7c7289f）

- 用户接「除 SelectionToolbar 外还有哪些工作未完成?」→ 按 task_plan 列全 UIUX 线剩余项，指出「同构无真机干净原语增量已枯竭」、剩余全卡决策/真机，最高杠杆单点=拍板 UiPopover 定位方案。用户「可以」→ 出 ASCII 对比 + 选型 brief。
- **决策：用户裁决 B（@floating-ui/vue 引擎，非自研）**。地面真相纠正「自研=从零」假设：全库已有 3 处手写弹层 + de-facto `positionMenu`（AppToolbar，右缘对齐 + 水平钳制），date 弹层更漏钳制（近右缘越界出屏，潜在 bug）。选型理由链见 findings 会话续 17。
- **交付**：① **UiPopover 原语**（委托 @floating-ui 的 flip/shift/autoUpdate 定位数学；原语持有项目契约=Teleport 逃逸 overflow 裁切 + useFocusTrap 焦点陷阱 + backdrop/Esc dismiss；非同构包裹〔无 .popover 全局类〕，表面视觉留消费方 slot；6 例 SSR 契约）② 迁 **date 弹层**（GalleryFilterChips，阶段1 4b02272）**顺带修漏水平钳制越界 bug**（shift 中间件）③ 迁 **filter/view ⋯ 菜单**（AppToolbar，阶段2 7c7289f，placement=bottom-end 等价旧右缘对齐），删 positionMenu + 常量 + *PopTop/Left + 两 backdrop 死 CSS + position:fixed + onWindowResize「resize 即关」降级（autoUpdate 现自动跟随）。**全库手写弹层定位归零**。
- **依赖**：+@floating-ui/vue 2.0.1（+dom/core/utils，~12KB 可摇树，全 MIT）；重生 NOTICE.md（生产依赖闭包 npm 95→114，过 CI generate-notice --check 新鲜度门——装 npm 包会静默作废归属清单，typecheck/lint/test 全绿也拦不住，须主动重生）。
- **门禁（本地，非 CI）**：阶段1 typecheck 0 / lint 0 / vitest **775**（+6 UiPopover）/ build 2.92s；阶段2 同样全绿 / build 2.84s。真机盲区（定位视觉/flip/焦点陷阱/嵌套弹层）见 `realtest-round5-2026-07-13-uipopover.md`。

### 会话续 18：selection bar 合并/分离一键切换——设计+施工方案产出、批准、落盘（纯设计会话,零代码）

- 用户指令：读 task_plan 阶段11 selection bar 条 + 主设计 §7.1 line 219 + 记忆会话续16 末条,结合实际代码给出「合并/分离一键切换」的设计+施工方案,并须支持收窄窗口折叠（参考顶部工具栏实现）。方案经用户审阅后批复「批准建议,方案落盘以便我在新会话施工」。
- **代码测绘**：通读 SelectionToolbar（拖拽/定位/9 emit）、useSelection（单例状态 vs MediaGrid 局部 handler）、MediaGrid 批量 handler 纠缠面（patchVisibleSelected/pendingDeleteIds/FLIP/compute）、AppShell/AppStatusBar（28px footer 槽）、useTitlebarMode→settingsMap→DynamicSettingControl 全链、useToolbarOverflow+AppToolbar/GalleryFilterChips 折叠范式、UiPopover（会话续17 产物,API 契约核实）。
- **方案核心**（详见 `designs/2026-07-13-SelectionBar合并分离一键切换方案.md`,施工蓝本单一真源）：① docked 形态用 **Teleport** 实现,**推翻会话续16「须先抽 useBatchActions」使能前提**（9 个批量 handler 原地留 MediaGrid;两边界=挂载顺序靠 v-if 天然安全+KeepAlive 残留用 onActivated/onDeactivated kaActive 守卫）② `useSelectionBarMode`（照 useTitlebarMode 范式:module localStorage ref 单例,默认分离）③ 收窄折叠复用 useToolbarOverflow（两形态各持 SelectionActions.vue 实例,胶囊需 max-width 链;测量帧用 visibility:hidden 替代 overflow:hidden 防裁 tooltip）,⋯ 溢出菜单**直接消费 UiPopover**（placement='top-end' 上弹）④ 前置重构 9-emit→**commands prop**（溢出菜单以数据渲染同组动作的结构前提）⑤ 拖拽位置持久化+clampOffset 纯函数（删「退选区即复位」watch,属有意行为变更）⑥ 状态栏**替换式**共存（28px 恒高,info 让位,版本区保留;docked 钮 24×24=WCAG 2.2 下限）。施工计划 C1–C5（commands parity→浮动折叠→docked+切换→拖拽持久化→docs 回写）,各带门禁与真机项。
- **用户已批三决策**：默认形态=分离（浮动,保现状）/ 状态栏共存=替换式 / SelectionToolbar 契约 9-emit→commands prop。
- **落盘前 re-verify 抓到初稿过时**：对话中初稿拟手写第三个弹层（当时 UiPopover 未建）;落盘核对地面真相发现并行会话续17 已交付 UiPopover 且全库手写弹层归零→方案修订为消费 UiPopover（详见 findings 会话续18）。
- **落盘内容（单一 docs 提交,显式路径）**：新建方案文档 + task_plan 阶段11 selection bar 条回写（🔴 待方案→🟢 已批落盘）+ 主设计 §7.1 bullet 回写指向新方案（**顺带修复工作树中该行非本会话的文本错乱**,见 findings）+ 本三件套(findings/progress)。并行会话在途文件（工作记忆治理开源工具化方案）未触碰未提交。
- 门禁：check_docs / check_docs_index（纯 docs 变更,无代码门禁面）。接续=施工会话直接读方案文档,按 C1–C5 推进。
- **并行会话隔离**：本会话仅动代码 + planning 三件套 + 记忆；检测到并行会话正编辑 `docs/designs/` 两文件（2026-07-11-UIUX 方案、2026-07-12 工作记忆治理方案），两份均以显式路径排除出我的三次提交。⚠ **2026-07-11-UIUX 方案 line 219（selection bar 裁决段）工作树副本被打乱成断句**（我上会话 c21d315 提交的干净正文被并行编辑搅乱），已上报用户裁量（未擅自 revert，恐冲并行会话在途 WIP）。

### 会话续 19：selection bar 合并/分离一键切换——按 C1–C5 施工落地（4 代码 + 1 docs 提交）

- 用户指令「按计划施工」→ 直接读施工蓝本 `designs/2026-07-13-SelectionBar合并分离一键切换方案.md`,按 C1–C5 分提交推进,每提交独立过本地门禁 + 显式路径提交(并行会话防收割)。
- **C1（9b893e5）动作数据驱动化**:新增 `SelectionCommand` 类型(icon/labelKey/danger/kind/groupStart/run);MediaGrid 组装 10 项数组(handler 全为既有函数引用,零迁移);SelectionToolbar 迭代 commands 渲染、删 9 emit。新增 SelectionToolbar SSR 契约 spec(7 例:顺序/aria-label 源自 labelKey/danger/colors/✕)。新增 `selection.colorLabel` i18n。门禁 vitest 782。
- **C2（0156db8）收窄折叠**:抽 `SelectionActions.vue`(props commands/variant,**自持 useToolbarOverflow**,双实例各绑自己 flow 容器);宽度约束链(胶囊 max-width:calc(100%-24px) + flow flex:0 1 auto + min-width:0);测量帧 visibility:hidden(不裁条外 tooltip);⋯ 溢出菜单消费 UiPopover(placement='top-end' 上弹)+ disclosure 语义(aria-haspopup/expanded);菜单行图标+可见文字标签。spec 拆分 SelectionActions.spec(8)/SelectionToolbar.spec 收敛壳级(4)。新增 `selection.more`。门禁 vitest 787。
- **C3（3974dfc）docked 形态 + 一键切换**:`useSelectionBarMode`(镜像 useTitlebarMode,默认分离 + offset + clampOffset + hostActive);**docked 用 Teleport**(SelectionActions 投送进 `#statusbar-selection-outlet`,9 handler 原地留 MediaGrid);AppStatusBar 恒存在 outlet + **替换式** info 让位(28px 恒高零 reflow);两框架边界守卫(isSelectionMode gate + hostActive 共享信号);条上切换钮(PanelBottom/PanelTop);settingsMap `selectionBarDocked` + DynamicSettingControl binding + 4 i18n 键。spec useSelectionBarMode.spec(14:clampOffset 穷举/parseStoredOffset/单例响应式)。门禁 vitest 801。
- **C4（7cac029）拖拽位置持久化**:offset 走 useSelectionBarMode(localStorage);删「退选区即复位」watch(有意行为变更);clampOffset 三时机(恢复条出现/拖拽结束/窗口 resize,拖拽中不钳);仅钳制真改值时回写。门禁 vitest 801。
- **C5 docs 回写**:本条 + 设计文档 §7 施工记录 + 顶部状态 banner + task_plan 阶段11 selection bar 条(🟢 已施工落地)+ findings 会话续19 + 新建 `realtest-round6-2026-07-13-selectionbar.md`(A 浮动/B 折叠/C docked/D 切换双入口/E Teleport+KeepAlive/F 六主题/G 读屏)。
- **施工相对蓝本一处加固(诚实披露)**:蓝本 §3.7 写 AppStatusBar 让位读「两单例」;施工发现选区残留进查看器(图片查看器沉浸是用户手动 toggle 非进入即隐 footer)时会 outlet 空白 → 加共享信号 `hostActive`,docked Teleport gate 与 AppStatusBar 让位同读第三条件。已写回设计 §3.2/§3.7 语义 + §7。
- **门禁(本地,非 CI)**:四代码提交各 typecheck 0/lint 0/vitest 782→787→801→801/build ~2.8s 全绿。真机盲区(折叠视觉/docked 28px/Teleport+KeepAlive/拖拽 clamp/读屏)见 realtest-round6。
- **并行会话隔离**:本会话仅动 selection bar 相关代码 + planning 三件套 + 设计文档;并行会话在途文件 `docs/designs/2026-07-12-工作记忆治理体系开源工具化方案.md` 全程以显式路径排除出 5 次提交,未触碰未提交。

### 会话续 20：round6 真机反馈三修（2026-07-13，2 代码 + 1 docs 提交）

- 用户第六轮真机验收报 3 问题（均门禁盲区，DOM 布局/动画运行期）:①窄窗折叠后拉宽不回弹须刷新 ②docked 宽度足够时也应展开 ③⋯ 弹层动画从画面左边飞入很怪，应改为自按钮处上弹（顶栏菜单同理下弹）。
- **诊断**:①②同根=`useToolbarOverflow` 假设容器恒等于可用宽（AppToolbar `flex:1` 成立），但 SelectionActions 折叠流是内容宽（`flex:0 1 auto`），折叠后自身变窄、窗口变宽时不回宽 → RO 失联 → 自锁棘轮。③根=@floating-ui 默认 transform 定位，首帧未就位在(0,0)闪现 + 定位 transform 压掉过渡 transform。
- **修 #1/#2（useToolbarOverflow.ts + SelectionActions.vue）**:引擎加 `containerFillsWidth` 选项（默认 true=保 AppToolbar 行为）;false 时改监听 window resize→rAF 节流→完整 measure。measure() 重构为**测量帧内捕获可用宽**（全渲染时 clientWidth=真可用宽，关帧后 applySplit）——对 flex:1 容器帧内/帧后同值，零回归。SelectionActions 传 `containerFillsWidth:false`。docked 变体（flow 在 outlet 内仍内容宽）同一修复覆盖。
- **修 #3（UiPopover.vue）**:`useFloating({ transform:false })` 用 top/left 定位让出 transform;取 `isPositioned` 就位前 `visibility:hidden` 消左上角闪现;取解析后 `placement` 推 `transform-origin`（top-*→底边生长/bottom-*→顶边生长），入场 `scale(0.9)→1` 自贴锚点边生长=从按钮弹出。一处改动全库弹层（date/filter/view/选区 ⋯）同时受益，方向随各自 placement 自适应。
- **门禁（本地，非 CI）**:typecheck 0/lint 0/vitest 801（65 文件，纯逻辑改动不新增用例——回弹/动画属 DOM 层布局盲区，归真机）/build 2.92s 全绿。
- **真机复验**:回写 realtest-round6 §B（新增「变宽自动回弹展开」项 + docked 同验）+ §B「⋯ 菜单上弹」项补充动画自按钮生长的期望 + 新增 §H（UiPopover 全库弹层入场动画统一目验）。
- **两 durable 教训**（详见 findings 会话续20）:①折叠引擎的容器宽度语义是隐性契约，内容宽消费者会自锁棘轮，须显式声明走 window-resize 重测;②@floating-ui transform 定位与过渡 transform 互斥+首帧闪现，transform:false + isPositioned 守卫 + 原点式 scale 三件套根治，原语「持有契约」使修一处全消费者受益。
- **并行会话隔离**:本轮仅动 useToolbarOverflow/SelectionActions/UiPopover 三代码 + 三件套 + realtest 清单;并行在途 `docs/designs/2026-07-12-工作记忆治理...md` 继续显式路径排除。

### 会话续 21：两栏水平对齐设置 + 折叠 ⋯ 弹层卡片动画（2026-07-13，3 代码 + docs）

- 用户两新需求:①折叠 ⋯ 弹层改卡片弹出式(类 Win11 开始菜单) + 与 ⋯ 按钮居中对齐;②顶栏/选区栏默认靠中,增设置项可选居中/靠左/靠右(符合 Win11/macOS 系统习惯),收窄先减留白再折叠。
- **先 AskUserQuestion 定三产品岔路**(答案改架构故必问):顶栏三区固定(标题恒左/chips 中/搜索恒右)→对齐**只挪中部 chips 簇**;浮动胶囊可拖→**对齐=默认位、拖拽仍可覆盖**(改对齐清旧偏移);设置粒度=**两栏各一**。
- **commit 1(4533d67)需求①**:UiPopover 卡片动画(scale 0.95→1 自 transform-origin + 朝锚点侧 translateY,`--ui-popover-pop-y` 随解析 placement 侧定正负,cubic-bezier 0.2s);⋯ 菜单 placement -end→无后缀居中(SelectionActions top / AppToolbar filter·view bottom)。
- **commit 2(2f1ac22)需求②状态层**:新建 useToolbarAlign(BarAlign 三态单例,默认居中,localStorage);useSelectionBarMode 加 align + setAlign + 导出 SELECTION_BAR_SIDE_INSET=12;clampOffset 泛化(align 参数缺省 center 向后兼容,据 baseLeft 算非对称 x 区间)。vitest useSelectionBarMode 14→21 + useToolbarAlign 2。
- **commit 3(16f7eae)需求②接线**:AppToolbar `.toolbar__foldable` 绑 justify(仅挪 chips 簇,先减留白再折叠天然成立);SelectionToolbar wrapper 绑 justify + padding-inline:12 + clampToBounds 传 align + watch align 清旧偏移;settingsMap 加 toolbarAlign/selectionBarAlign 两 select;DynamicSettingControl 两 selectBinding;i18n zh/en 对称(alignCenter/Left/Right)。
- **门禁(本地,非 CI)**:typecheck 0/lint 0/vitest 801→810(66 文件,localeIntegrity 平衡)/build 2.96s 全绿。对齐视觉/收窄留白-折叠次序/拖拽非对称钳制/卡片动画属真机(realtest-round7)。
- **六 durable 教训**(详见 findings 会话续21):①工具簇对齐作用在包簇的全宽轨道而非簇自身,justify 留白与折叠引擎正交;②可拖元素引入对齐默认位后钳制须纳 baseLeft,非居中可拖范围非对称;③「新默认路径与旧行为逐值等价」是安全泛化钳制数学的黄金判据(缺省 center 令 14 条旧单测零改动过);④对齐基位变更须清空旧相对偏移(参照系变则复位相对量);⑤CSS 视觉内缩与钳制基位常量同源(export+注释互指);⑥三态设置用泛型 select 不造一次性 segmented(且贴 Win11 下拉习惯)。
- **新建 realtest-round7-2026-07-13-两栏对齐.md**(A 弹层卡片+居中/B 顶栏 chips 对齐/C 选区胶囊对齐/D 两栏独立+与停靠共存)。
- **并行会话隔离**:本轮仅动 UiPopover/SelectionActions/AppToolbar/SelectionToolbar/useToolbarAlign(新)/useSelectionBarMode/settingsMap/DynamicSettingControl/i18n + 三件套 + realtest;并行在途 `docs/designs/2026-07-12-工作记忆治理...md` 继续显式路径排除。

### 会话续 22：round7 真机反馈——docked 态选区栏对齐未生效修复（2026-07-13，1 代码 + docs）

- 用户真机报「底部工具栏的对齐设置未生效」。
- **诊断**:浮动态对齐本就生效(class 在 SelectionToolbar `.selection-toolbar-wrapper` 全宽轨道);但用户 round6 测过 docking,持久化 `selection_bar_docked='1'` 使条处 **docked 态**——浮动 wrapper `v-if="!docked"` 不渲染,SelectionActions 被 Teleport 进状态栏 outlet,那条 DOM 路径**无对齐宿主** → 改设置无作用。round7 只把对齐补到浮动壳,漏了 docked 这条渲染分支。
- **修(SelectionActions.vue)**:destructure `align` from useSelectionBarMode;root `.selection-actions` 加 `selection-actions--align-${align}` class(与 variant class 并列);CSS 仅 `.selection-actions--docked.selection-actions--align-center/right` 施 `justify-content`(填满 outlet 的 selection-actions,决定簇在 outlet 内落位;align-left=默认 flex-start)。浮动态 selection-actions 为内容宽无自由空间,限定 `--docked` 故不与浮动 wrapper 对齐重叠。折叠测量零影响(flow flex-grow:0,自由空间只进 justify 留白)。
- **测(SelectionActions.spec.ts)**:+2 SSR 用例(docked 默认 align-center / setAlign('right')→align-right,try-finally 复位防单例污染);顺带证明模板内 align ref auto-unwrap 正确(未解包→畸形 class→断言失败)。setAlign 在 node 无 localStorage 仍驱动 ref(先赋值后写盘、写盘 catch)。
- **门禁(本地,非 CI)**:typecheck 0/lint 0/vitest 810→812(66 文件)/全绿。docked 对齐视觉(outlet 轨道内居中/靠右、先减留白再折叠)属真机盲区。
- **docs 回写(红线)**:realtest-round7 §D「docked 态对齐属本轮有意范围」这条正文**已被推翻**,按规范文本回写红线改写为「docked 态对齐同样生效」+ 新增 docked 对齐验收项 + 顶部补 round7 修复 banner;findings 会话续22(三 durable 教训:多形态宿主须逐个落地/被常用形态命中的「范围外」是缺口/对齐相对元素所在轨道)。
- **并行会话隔离**:本轮仅动 SelectionActions.vue + 其 spec + 三件套 + realtest;并行在途 `docs/designs/2026-07-12-工作记忆治理...md` 继续显式路径排除。

### 会话续 23:S1 阶段7(UiToolbar 决策)+ S2 阶段8(视图路由化 + view-pref/search URL 同步)——用户三裁决 + 无人值守施工(2026-07-13)

用户指令:做 S1 原语(阶段7)+ S2 路由(阶段8),先出决策点裁决、再无人值守施工、用户回来真机测。两路 Explore 勘查(toolbar 家族 / S2 路由与 view 单源)取证后经 AskUserQuestion 定三裁决:

- **裁决①(UiToolbar)= 不建组件,补 a11y 缺口**。地面真相:三 toolbar(AppToolbar/ContextualToolbar/SelectionActions)**无共享全局 `.toolbar` 类**,复用逻辑早在 `useToolbarOverflow`(2 消费者)+ `useRovingTabindex`(1 消费者:ContextualToolbar);无共享外观壳 → 无「摊薄」价值,建组件属为 DRY 而 DRY(重蹈 UiField 会话续4 判杠杆低 / UiCheckbox 会话续13 out-of-scope)。**施工中二次核实收窄范围**:SelectionActions 亦非纯钮(`colors` 命令是 ColorLabelPicker=内联 8 色块 button 复合控件),且 `data-toolbar-item` 挂在 `.fold-item` 包裹 span(overflow 测量契约)而 roving 需成员为可聚焦元素+逐项绑 tabindexFor(两契约撞),色块又在子组件内、固定件在流容器外 → 强套 roving 须改「命令式 tabindex 管理」新基建+子组件参与,且 roving 本质 ⏸GUI(只能真机键盘验)。**故 S1 落地=SelectionActions 加 `role="toolbar"`+可访问名(安全 a11y 增值,镜像 ContextualToolbar 上 roving 前先有 role 的轨迹);roving 扩到 SelectionActions 标为需新基建+真机键盘验的后续项;findings 收口「三 toolbar 各自不套 roving 之由」分析**。
- **裁决②(S2-c 视图路由化)= 分模全量(smart-album + folder 筛选)**。5 smart-album 各独立路径(/=all、/favorites、/trash、/live-photos、/recent)+ folder 筛选态(模式B,groupBy≠folder)→ /folder/:id;folder 滚动锚点态(模式A,groupBy=folder 点文件夹设 pendingScrollDirId)**保持 '/' 不进 URL**(滚动位置非视图,进 URL=每滚改 history 灾难,回避即正确)。关键切割线:模式A 永远导航 '/'、模式B 才去 /folder/:id,两模天然不撞;getViewKey 模式B 已 `dir-<id>` 分桶,scrollCache/KeepAlive 零改造。僵尸路由(/favorites/trash/folder 已注册无人导航、isGalleryRoute 已认)本轮激活;live-photos/recent 补路由壳。
- **裁决③(S2-b2 URL 同步)= view-pref + search 全做**(用户选更全范围,非我推荐的「仅 view-pref」)。group/sort/order/layout 进 URL(per-view、URL 权威覆盖持久值)+ search q/scope/mode 进 URL。**推翻用户 07-12「先保持 sticky-global、真实使用检验后再定 per-view」裁决**(用户明示改判)。竞态修:hydration 门加等 `startupConfigPromise`(否则 persist `.then` 晚到覆盖 URL 水合);镜像 persist=false(语义搜索临时 group=none/sort=similarity 不写 URL)。**search 高风险面(用户在环已知)**:mode 恢复触发 aiStore.setSearchMode 的 group/sort 副作用 + 语义 IPC + searchToken 代次守卫——用代次+相等守卫做稳,单独标真机验收重点(门禁验不了语义 IPC 实际行为)。

**施工分三阶段**(各独立门禁+独立提交,便于真机按阶段回归):
- **Stage 1(S1 a11y)**:SelectionActions role/可访问名 + spec + findings 收口。
- **Stage 2(S2-c 路由)**:router 补壳 + FoldersSection/LibrarySection 导航改 push 路径 + App.vue watcher 补 smart-album/folder 回填分支(带 token/相等守卫防重入清选区)+ resolveView/两投影锁步核对 + spec。
- **Stage 3(S2-b2 URL)**:galleryQuery 编解码扩 view-pref+search 键 + useGalleryQuerySync 门加等 startupConfigPromise + searchStore.apply 接线 + 镜像 persist=false + spec。

**Stage 1 完成(S1 a11y)**:SelectionActions 根加 `role="toolbar"` + `:aria-label="$t('selection.actionsToolbar')"`(i18n 双语键,过 localeIntegrity 平衡门);+1 SSR spec 断言 role/可访问名;findings 会话续23 收口 UiToolbar「不建组件」决策 + 三 toolbar roving 适配分析;task_plan 阶段7 三处 UiToolbar 状态回写「决策已定=不建组件」。**踩坑修**:模板注释里写字面 `data-toolbar-item` 被 SSR 渲进串、污染「每命令一折叠单元」的 `/data-toolbar-item[ >]/g` 计数(expected 7 to be 6)——按组件既有约定改述注释不嵌字面 token(会话续6 陷阱翻版)。门禁(本地非 CI):typecheck 0 / lint 0 / SelectionActions+localeIntegrity 13 测过。roving 视觉/键盘属 ⏸GUI(SelectionActions 本就不套 roving,ContextualToolbar roving 早前已标真机)。提交 `b90b5e2`。

**Stage 3a 完成(S2-b2 view-pref)**:新建 `utils/viewPrefQuery.ts`(encodeViewPref 仅非默认→干净 URL / decodeViewPref 返 Partial 只含 URL 出现且合法的键,白名单防御式)+ spec(9 测)。useGalleryQuerySync 扩:MANAGED 加 group/sort/order/layout;writeUrl 合并 encodeViewPref;readUrl 加 applyViewPref(只对 URL 出现的键赋值、persist=false、缺失键保持 persist=URL 权威覆盖语义);writer watch 加 4 个 ui getter。**竞态修**:hydration 门 router.isReady()→`Promise.all([isReady, startupConfigPromise.catch(→null)]).then(nextTick)`——persist 先落 uiStore、URL 再覆盖出现的键(否则 persist 的 .then 晚到覆盖 URL 水合),nextTick 保 persist 赋值微任务已 flush 不依赖 .then 注册顺序,配置 IPC 失败也放行不卡死同步。**正确性(镜像 persist=false)**:语义搜索把 groupBy/sort 临时改 none/similarity(aiStore.isSemanticMode),writeUrl 此时删 group/sort 两键——否则刷新以 URL 覆盖 persist 且污染 aiStore.previousGroupBy(把被覆盖后的 none 当复位目标)。sortOrder 经核实可变(GalleryViewControls 直接切、无 persist)故同步。门禁 typecheck 0/lint 0/vitest 829/build ✓。提交 `724dcf2`。

**Stage 3b 完成(S2-b2 search)**:新建 `utils/searchUrl.ts`(encodeSearch 非默认〔mixed/filename/空 q〕裁剪 / decodeSearch mode/scope 白名单〔scope=filename/folder/date/device/location/global〕+ q 原串)+ spec(10 测)。useGalleryQuerySync 扩:MANAGED 加 q/scope/mode;searchSnapshot 读 searchStore.mode/scope/committedQuery;writeUrl 合并 encodeSearch;readUrl 加 applySearch(mode→scope→apply,置于 filter/view-pref 之后使 setMode 语义 group 覆盖在 view-pref 后、previousGroupBy 捕获正确);writer watch 加 3 个 search getter。**search 是全局态(同 filter)跨视图携带、hydration 恢复一次;恢复经 searchStore.apply 触发真实查询(语义走 IPC)= 真机验收重点**(门禁只验编解码纯函数)。门禁 typecheck 0/lint 0/vitest 839(69 文件,+10)/build ✓。

**⚠ 诚实边界(view-pref 语义模型)**:本轮实现的是**「全局携带 + URL 恢复/覆盖持久」**模型——view-pref 像 filter 一样全局(切视图带着走)、URL 反映当前值、深链恢复、hydration 时 URL 覆盖 persist。**未实现「纯 per-view」**(每视图各记各的 group/sort、互不影响)——那需要一套与 filter 相反的 read-on-navigation 模型 + 改工具栏改动不落全局持久化,属改动 toolbar 持久语义的 UX 契约变更、宜真机判定后再定(不冻结契约)。用户裁决用词是「per-view、URL 权威覆盖持久值」:「URL 权威覆盖持久」已完整兑现;「per-view 逐视图独立」标为待真机后定的后续项(见真机清单末与下方说明)。

**Stage 2 完成(S2-c 视图路由化)**:新建 `utils/viewRoute.ts`(smartAlbumToPath/folderToPath/routeToView/isPrimaryGalleryRoute 纯函数,SMART_ALBUM_PATHS 单一真源 + 机械反查表)+ `viewRoute.spec.ts`(7 测,含往返一致 + 防御式非数字 folder id)。router 补 `/live-photos`、`/recent` 路由壳(此前连壳都没有)。galleryQuery.isGalleryRoute + spec 补两条路由。App.vue:watcher 加 smart-album/folder 回填分支(完备相等守卫连 collection/person 一起比,防从 /collections 到 '/' 漏清 collection)+ SemanticSearchPanel v-show 从 `route.path==='/'` 平移到 `isPrimaryGalleryRoute`。LibrarySection.onAlbumClick 改 push smartAlbumToPath。FoldersSection:抽 `navigateToFolder` 助手,onNodeClick 模式B/新增根后/移动后三站点共用(模式A 保持 push '/' + pendingScrollDirId 不进 URL)、showAll 补 push '/'。**施工期关键发现**(见 findings 会话续23 续):FoldersSection 的 addRoot-after(553)与 move-after(590)两站点此前 push('/'),在新 watcher 下 push('/') 会被回填成 smart-album 'all' **清掉刚设的 directory** → 属路由化强制连带改动(非可选)。RouterView 无 :key、KeepAlive 只保活 MediaGrid → 多路径同为 MediaGrid 组件复用不重挂,零滚动丢失回归。门禁(本地非 CI):typecheck 0 / lint 0 / vitest 820(67 文件,+7 viewRoute) / build ✓。真机盲区:各 smart-album/folder 深链恢复 + 刷新 + 前进后退 + 模式A 锚点不进 URL + 切视图不误清选区(见下真机清单)。

### 会话续 24：S3 阶段9——Search combobox 契约（2026-07-15，1 代码 + docs）

用户指令「开始施工 S3 Search combobox」。施工前核对 task_plan S3 双 facet(稳宽槽位 / combobox contract)+ 现有 AppToolbar 搜索实现 + 已就绪 UiPopover。

- **地面真相**:混合搜索下拉(`mixed-search-dropdown`)交互上早已是 combobox+listbox(输入驱动出现 / ArrowUp-Down 导航 / Enter 选择 / Esc 关闭 / `@mousedown.prevent` 保输入框焦点不失),但**零 ARIA 接线**——input 无 role/aria-expanded/controls/activedescendant、下拉无 role=listbox、项无 role=option。读屏用户听不到候选数与当前高亮项。
- **稳宽槽位 facet 已满足(本轮核验非新写)**:`.toolbar__search-wrap` 宽 `clamp(220,26vw,320)` 与模式内容无关(scope select / spinner 在固定宽壳内 flex 收缩)、仅随窗口单调 → 不非单调扰动中部 foldable 可用宽,天然不是折叠振荡源(顶栏闪展根治线关心的正是这类扰动源)。combobox 改动纯 ARIA、零布局变更,无回归。
- **实现(纯决策源 + 薄消费方,沿用本仓 resolveGalleryEmptyState/nextRovingIndex 范式)**:新建 `utils/searchCombobox.ts`——`resolveSearchCombobox({mode,dropdownOpen,activeIndex,listboxId})` → 输入框 ARIA 属性集(**仅 mixed 生效**,normal 实时过滤 / semantic 防抖提交皆无工具栏内建议列表 → 保持原生 searchbox;据 mode 门控防「切出 mixed 后 stale open 泄漏 combobox 语义」);aria-expanded 在 combobox 上常在(true/false),aria-controls / activedescendant 随下拉 open 门控(下拉 v-if,引用不存在 id 属无效 ARIA);`searchOptionId` 由 listboxId 派生 option id 与 activedescendant 字节对指。**7 例穷举 spec**(mode×open×activeIndex 全组合 + 非 mixed 即便 open 也不泄漏的防漂移用例 + activedescendant↔option id 一致契约)。
- **AppToolbar 接线**:input 加 `:role` / `:aria-expanded` / `:aria-controls` / `:aria-autocomplete="list"` / `:aria-activedescendant`(全绑 `searchCombobox` computed)+ `:aria-label`(占位符兜可访问名——placeholder 非可靠可访问名);下拉加 `:id=listboxId` / `role="listbox"` / `:aria-label`;两项各加 `:id=searchOptionId(...)` / `role="option"` / `:aria-selected`。**listboxId 走 `useId()`**(Vue 3.5)保 SSR 稳定 + 跨实例唯一(硬编码常量在多实例/hydration 撞 id)。顺带 **DRY**:把 template 里重复的模式占位符三目收敛为 `searchPlaceholder` computed(placeholder + aria-label 共用)。
- **i18n**:新增 `toolbar.searchSuggestions`(listbox 可访问名,中英平衡过 localeIntegrity 门)。
- **门禁(本地非 CI)**:typecheck 0 / lint 0(含 Prettier-through-ESLint,.vue 模板未破)/ vitest **909**(75 文件,+searchCombobox 7 例)/ build 3.11s ✓。chunk-size 警告是既有项(Phase 13 S7 待办,非本次引入)。
- **真机盲区**:读屏播报(NVDA/VoiceOver 在 ArrowUp/Down 时是否随 aria-activedescendant 播报当前候选、进入下拉是否播报「搜索建议」列表 + 候选计数)——门禁验不了 AT 实际行为,归 realtest §E(与 filter a11y round5 §E 同类盲区)。
- **docs 回写(红线)**:task_plan 阶段9 S3 项由 `[ ]` 改写为 `[~]`,拆出 combobox `✅（452f808）` + 稳宽槽位 `✅（既有满足）` 两子项 + 状态行回写;overflow 接入仍 `[ ]`(待命令增多,触发条件未到)。
- **并行会话隔离**:仅动 5 文件(searchCombobox.ts/.spec.ts + AppToolbar.vue + 中英 locale),显式路径提交 `452f808`,doc 单独提交。

**S3 剩余**:统一 overflow(ContextualToolbar 溢出接入,待命令增多)/ Reader/Audio 后续收敛(真机)/ combobox 读屏真机验收。

### 会话续 25:S7 阶段13——token 引用闭环(幽灵根治 + 死票清理 + 类型角标 + 双向硬门)(2026-07-15,3 代码 + docs)

用户「接续任务」,无指定切片。**按阻塞类型盘点剩余项**:⏸真机类(S2-b2/S2-c2-3 路由与 URL、filter a11y 读屏、
selection bar 手感、combobox NVDA)+ 待决策/触发类(S3 overflow 待命令增多、S6 Viewer 控制面、Reader/Audio)
+ **可推进类(唯 S7 两项:硬编码色治理 / chunk 治理)** → S7 是唯一未开工阶段,锁定它。

- **测绘(一路 Explore + 自查)**:Explore 报「~236 处硬编码色债 + 5 幽灵 token/8 消费者」。**自查推翻其精度**:
  正则扫描把**注释里的历史记录**当活引用——本仓修幽灵时按约定在注释留旧 token 名(「原 var(--color-danger) 为
  幽灵 token(S5 修)」),那些恰是**已修站点**。剥注释后真值 = **4 幽灵 / 5 消费者**(`--color-primary` 与
  ProofreadPanel/ReplacementPanel/VersionPanel/PdfReader 的 danger·bg-base 全是注释假阳性)。
  同族陷阱:会话续16「模板注释渲进 SSR 串」、会话续23「注释里的 data-toolbar-item 顶穿计数」。
- **提交 1(6c778cd)幽灵根治 + 消费⊆定义门**:4 幽灵,**两处用户可见 bug**——① `--color-badge-size` 六主题均无
  定义且 MediaThumb 无 fallback → 大小徽章底色全透明、白字直压照片;其 canvas 孪生 `g('--color-badge-size',
  'rgba(0,0,0,0.6)')` 带回落值反而渲染正确 → **DOM/canvas 分叉**(修法:六主题补定义,**取值=canvas 既有回落值**,
  使 DOM 收敛到 canvas 且 canvas 零变化,消费端零改动)。② MediaGrid 粘性分隔胶囊
  `rgba(var(--color-bg-primary-rgb,255,255,255),.85)` 回落写死白 → 四套暗色主题白条(→ color-mix,先例 variables.css)。
  另 `--color-danger`→`--color-error`(PerformancePanel 无 fallback 致 color 回落继承色 + color-mix 整条失效,
  录制/错误态一直没红出来)、`--color-bg-base`→`--color-bg-primary`(HGalleryLab 根衬底)。**全部沿用 S5 同族先例**。
  门禁:消费面 ⊆ 定义面,**剥注释后再扫**(否则惩罚良好注释),**变异测试双向自证**(注入活引用即红 / 仅注释提名放行)。
- **提交 2(d499dce)死票清理**:5 个定义但无消费。**用户裁「全接线」→ 逐个核对靶子后带证据回报 → 用户改裁「删」**:
  `--color-badge-video`(该族语义=角落类型标签〔兄弟 `--color-badge-live` 的消费者 `.badge-live` 即红底 LIVE 角标〕,
  而 `.badge-video` 是 `position:absolute + translate(-50%,-50%)` 的**居中播放键**,只是碰巧同名 → 接线=把播放键涂蓝)、
  `--color-accent-dim`(**六主题取值无一致语义**:若为按下态该在每套主题沿 hover 同方向再走一步,实际 4/6 反向
  〔ink hover 变亮而 dim 变暗、moonlight hover 变暗而 dim 变亮〕→ 意图不可考,接线=现编)。
  `--color-text-placeholder` 相反,**语义自洽**(六主题一律比 tertiary 暗一档)=真设计意图未接线 → 两处 `::placeholder`
  由 tertiary 改接专属 token;**顺带修门禁可信度**——check:contrast 本就硬门守 placeholder×bg-surface,而占位符实际由
  tertiary 渲染,**门在为不上屏的值背书**;接线后该门对由幻影变真(比删更优)。
- **提交 3(1f3ddcc)类型角标 + 死票门 + 元数据白拉修复**:接线最后两死票,**引用闭环双向归零(49 定义 = 49 消费)**。
  角标范围按**「不重复标记」原则**(用户裁决 VIDEO 冗余 → 我据同一原则推导 DOC 亦然并明示):audio 出
  (网格内无播放键无时长无专属视觉,角标是唯一标识);document **仅限走真实缩略图者**(pdf/svg/有封面 epub)——文本卡格式
  (txt/md/office/od*,含封面盖棺失败降级的 epub)已自带扩展名角标 `.media-thumb__textcard-ext`,再叠 DOC 属重复;
  video 不出。判定走 `helpers.typeBadgeOf` **共享单源**(两路此前靠「条件逐字对齐 DOM 模板 v-if」的注释维持一致=纯人肉
  守约,而刚修的 badge-size 分叉正是这类漂移的产物 → 新增条件不再逐字重写第二遍)。
- **显示模型经第 4 轮裁决收敛**:我初问给的「默认开/默认关」选项**失真**——查实 `thumbInfoElements` 默认 `[]`、
  `showThumbInfo` 默认 `false`、**现有 10 个元素键无一有默认值**(纯 opt-in),唯 LIVE 徽章不受总开关管。带证据重问 →
  用户裁「**进面板与其余 10 项一致 + 总开关默认设为开**」。落地=`type` 进面板默认不勾 + `showThumbInfo` 默认 true
  (**只为让元素面板可发现**——面板由 `v-if=showThumbInfo` 门控,默认关等于把整套逐元素配置藏起来;elements 仍默认空
  → **缩略图观感零变化**;显式存过 'false' 的用户不受影响)。
- **连带修真实性能问题(默认翻转的前置)**:`ensureMeta` **无守卫**,而 MediaGrid 那条 `immediate:true` watcher 只看总开关
  → 默认翻 true = **所有用户每屏拉 EXIF/GPS/路径**,而 elements=[] 意味着拉回来无人渲染(违 A1「元数据从常驻布局缓存剥离、
  按需供给」本意 + 项目性能优先)。**且此 bug 今天就存在**(开总开关只勾 size/status 的用户同样白拉)。
  修:新增 `needsViewportMeta(elements)`(取用面 = filename/path/geo/camera/params,**与 buildThumbInfoLines 的 `meta?.x`
  一一对应,spec 对拍钉死防漂移**)+ 元素列表补入 watch 源(否则中途勾 camera 要等行变化才补拉)。
  **可证明安全**:`viewportMeta` 全库唯一终点是 buildThumbInfoLines(已逐消费者核实)。
- **死票硬门**:定义面 ⊆ 消费面,与提交 1 合成**双向互等**;变异测试自证会红(注入死票即红,并顺带被既有键集齐平门抓一道)。
- **门禁(本地,非 CI)**:三提交各 typecheck 0 / lint 0 / vitest 911→911→**920**(75 文件,+9:typeBadgeOf 4 例穷举、
  needsViewportMeta 4 例含与 buildThumbInfoLines 的 meta 取用面对拍、死票门 1)/ check:contrast 硬门全过 / build ~2.9s。
- **真机盲区**:徽章底色/占位符明暗/六主题目验/canvas 两路一致/元数据 IPC 观察 → 新建 `realtest-round9-2026-07-15-S7token闭环.md`
  (A 幽灵修复 6 项含两 bug / B 类型角标 8 项 / C 占位符 2 项 / D 元数据白拉 3 项 / E 读屏)。
- **五 durable 教训**(详见 findings 会话续25):①注释里留旧 token 名是好习惯,但任何正则扫描/门禁必须先剥注释,
  否则工具反噬良好注释(本仓第三次踩,前两次是 SSR 串与 data-toolbar-item 计数);②幽灵 token 带 fallback 时静默走死值、
  无 fallback 时整条声明失效——**两种都肉眼难察且反复复发(S5/S6/S7 各修一批)**,故须门禁钉死而非人肉复查;
  ③**孪生实现里带 fallback 的那一路会掩盖幽灵**,使 bug 只在另一路显形(badge-size:canvas 对、DOM 错);
  ④死 token 会让门禁为不上屏的值背书(placeholder 幻影门),**门禁可信度是独立于覆盖率的属性**;
  ⑤**用户裁决的前提被地面真相推翻时须带证据回报重裁,不硬推**(本轮连翻两次:「全接线」对 4 个无靶 token 不成立、
  「默认开」在纯 opt-in 模型里自相矛盾)——照 D-018 时区前提反转的先例。
- **并行会话隔离**:仅动 token/徽章/元数据相关 10 文件 + 三件套 + realtest,三次显式路径提交。

## 会话续 26(2026-07-15):S7 chunk 治理收官 + §18 蒸馏

- **`09320fa` 蒸馏 experience.md §18**(用户点名):「正则扫描器必须先剥注释,否则门禁反噬良好注释」。
  本仓第三次踩同一根因,前两次只记在各自 findings,未升为通用判据。给出与 §12 的**选择判据**:
  被扫面是工具自己的代码 → 源头避让;是全库业务代码 → 只能修工具(不能要求全仓为门禁的正则让路)。
  同步修正 MEMORY.md 页脚失效指针(原指 `§5/§7–11`,实际已到 §18)。
- **`ec7c378` chunk 治理收官**(S7 最后一个可门禁验证项)。**先测绘后施工,结论推翻「治理=瘦身」**:
  - 被 Rollup 喊的两个块**全是误报**(`cpp` 637.55 kB `isDynamicEntry=true` 懒加载/gzip 47.22/不进首屏;
    `index` 549.75 kB 是 16 路由全懒加载后的外壳),且 pdfjs/shiki **零模块在入口** = 架构本就正确,**无可修**。
  - **真风险是「告警不是门」**:实测把 pdfjs(365 kB)静态 import 进 `main.ts`,`npm run build` 照喊照过
    **exit=0**、CI 全绿。叠加常年两条误报训练人无视告警 → 真回归也淹在噪音里。
  - 落地 `scripts/vite-plugin-bundle-budget.mjs` 两条可行动不变量(破则 build 失败):①入口 ≤ 620 kB
    (实测 549.75 + ~13% 余量);②pdfjs/shiki/@shikijs 不得进入口。**有意不设懒块上限**(上游体积不可控,
    为它设阈值唯一响应是调大数字 = 制造下一个幽灵门禁)。
  - **被自己的门捉到两个量测错误**:①Vite 的 kB 是 SI(`/1000`)而首版用 1024 → 落盘 549,751 报成 536.87,
    预算悄悄松 2.7%(已锁回归测试);②`generateBundle` 阶段 `chunk.code` 未定稿(少算 1,645 字节,
    `enforce:'post'` 也不消失)→ 改在 `writeBundle` 直接 `statSync` 盘上文件 = **量发货物,对未知后处理免疫**。
  - **纯判定层与 Vite 解耦 + 双向钉死**(21 测试,每条不变量配「合规过/违规红」),另对适配层做**两次端到端变异**
    (压预算→exit=1;真塞 pdfjs→exit=1)——单测证不了插件真能拦停构建。`vitest.config` include 扩
    `scripts/**/*.spec.mjs`,CI 既有 `npm test` 即覆盖,无需改 ci.yml。
  - **已排除路径(实测,勿重试)**:`__VUE_OPTIONS_API__=false` 仅 -4.31 kB;`__VUE_PROD_DEVTOOLS__` 已默认 false 省 0;
    `manualChunks` 拆 vendor 在 Tauri(本地加载无 HTTP 缓存)下**首屏零收益**纯装饰;懒加载 PerformancePanel/
    OnboardingWizard ≈1ms 不材料化。
- **门禁(本地,非 CI)**:lint 0 / typecheck 0 / **vitest 920→941**(76 文件,+21)/ build exit 0 **且 Rollup 告警消失** /
  入口 549.75 kB 与 Vite 报告逐字节对齐 / check_docs 绿。
- **三 durable 教训**(详见 findings 会话续26):①**区分「告警」与「门」**——告警在噪音中等于不存在,狼来了效应
  本身就是伤害;②**为不可控的上游体积设阈值 = 制造下一个幽灵门禁**,判据=「红了我能做什么?若唯一答案是改阈值,
  就别设」;③**门禁量的东西与真正发货的东西之间,每多一层推断就多一处可静默失准的缝**。
- **S7 状态**:唯一剩项 = 六主题视觉矩阵截图。round9 清单待用户验收。
  〔**会话续27 更正**:此处「(真机)」标签已被实测推翻——该项大半可自动化,harness 加 `&theme=` 维度即可,
  非真机专属;round9 亦已于同日验收通过。S7 已收官。〕
- **并行会话隔离**:仅动 `scripts/vite-plugin-bundle-budget.{mjs,spec.mjs}` + `vite.config.ts` + `vitest.config.ts` +
  `docs/experience.md` + 三件套 + realtest,显式路径提交。

## 会话续 27(2026-07-15):round9 真机验收通过 + S7 收官(6 主题视觉矩阵)

- **round9 真机验收通过**(用户,A–E 全项无异议,21 项全勾):两处用户可见 bug(大小徽章透明底、暗色主题
  粘性胶囊白条)确认已修;AUDIO/DOC 角标、占位符接线、元数据按需拉取均如设计。
- **6 主题视觉矩阵收官**(`3620e26` 捕获器 + `5308ff8` 首个战果):
  - **推翻「真机」前提**:此项自 S7 立项即挂真机标签,实测**大半可自动化**——本仓早有先例(headless Chrome +
    `?ui-harness=` 做无 Playwright DOM 断言),harness 加 `&theme=<id>` 维度即出 6 主题 × 3 场景 = 18 张
    (`npm run capture:themes`)。
  - **有意只做捕获、不做 pixel-diff 基线门**:字体栅格/AA/Chrome 版本任一变动即红,红了唯一响应是
    「重生成基线」= 幽灵门禁反模式(判据同 chunk 那轮)。机器干苦力,判定留人眼。
  - **两处设计选择**:①主题经 `startupConfig` 覆盖而非直写 `data-theme`——后者会让截图为**产品里不存在的
    路径**背书,经此则完整跑真实链(normalizeThemeId → resolvedThemeId → applyAppearance);②主题 id 取自
    `themes/*.css` 文件名而非解析 registry——等价由 `theme-contract.spec` 硬门担保,零解析零漂移。
  - **首个战果**:网络存储卡经 **Vue scoped 根节点双作用域规则**盖掉全局卡外观,**六主题全中**
    (六主题 bg-surface ≠ bg-elevated 无一例外,Xuan 差值最大)。像素实测修前 surface 段 58px、修后消失。
    残留 style **一半活一半死**(`.settings-card` 命中子组件根=活=bug 源;`.settings-card__header` 拿不到
    data-v=死代码)。**三道现有门全绿**(token 契约/对比度/typecheck)——取值全合法,错的是取了哪个。
  - **测量法**:跨六主题**不变像素图**。settings **0.0% 恒定**(完全随主题);gallery 7.9% 恒定区全落在照片内;
    viewer 92.3% 恒定 = **已声明的 S5 硬编码色豁免**(看图台永远黑,设计 §6.2),全库剥注释扫描 `#000|black`
    仅 2 处且都在该豁免组件内 → **测量反向印证豁免边界正如声明、未外溢**。
  - **先自证工具再用其数**:同主题连拍两次 porcelain 逐字节相同、ink 差 482 像素(0.04%/最大差 3);噪声只会让
    不变性低报,故三个数可信。一处**已测未解释**:porcelain 照片像素比其余五主题每通道低 1(可复现,已排除
    与页面底色合成),1/255 亚感知级,按比例原则不深挖、如实记录。
  - **fixture 补覆盖**:18 项此前全是 image → S7 新建的 AUDIO/DOC 角标在任何场景都渲染不出**等于不在覆盖内**;
    改 2 项为 audio/pdf + 开满信息元素后才谈得上覆盖(harness 是**视觉场景**不是新装默认模拟器)。
- **门禁(本地,非 CI)**:lint 0 / typecheck 0 / **vitest 941**(76 文件)/ check:contrast 0 / build exit 0
  (入口 549.89 kB < 620 预算门)。
- **矩阵盲区(勿当已覆盖)**:viewer 主题化控制条**无指针不出镜**(headless 无鼠标 + 全新 profile 致侧栏默认隐藏);
  canvas 网格需 DEV+localStorage flag 不在矩阵内;原生窗口边框/WebView2/GPU canvas 仍属真机面。
- **截图有意不入库**(`.gitignore` 加 `.screenshots/`):18 张 ~9MB **可再生**证据,脚本才是耐久资产;
  且它不是基线(本仓有意不做 pixel-diff 门)。
- **并行会话隔离**:仅动 `scripts/capture-theme-matrix.mjs` + `package.json` + `.gitignore` +
  `src/harness/{runtime,ipcFixtures}.ts` + `src/components/settings/NetworkStorageSection.vue` + 三件套 +
  realtest,显式路径提交。

## 会话续 28(2026-07-15):七份未验清单合并为 round10 单次验收 + 纠正 3 处过期预期

**触发**:用户问「UI/UX 线还剩哪些工作?」→ 盘点后确认**剩余的大头是真机验收积压**(7 份清单长期停在
`待真机验收`,累计 ~130 项),用户裁「合并验收,给我清单」。零代码,纯清单工程。

**盘点地面真相**(读 frontmatter 而非推断):9 份 realtest 中仅 round2 / round9 是 `快照`+已通过;
S1 / round3 / round4 / round5 / round6 / round7 / round8 七份全是 `施工中`+`待真机验收`。其中
round6/round7 **已有过真机反馈且修完落地**(97354b6/3c501df/ac650e2),只是清单没回写 → 属**复验**非首验。

**🔴 核心发现:3 处清单正文已与代码相反,照测会报假 bug**。代码实测:
- `AppToolbar.vue:232/245` = `placement="bottom"`(居中) —— 而 round5 §B / round6 §H 写 `bottom-end`(右缘对齐);
- `SelectionActions.vue:107` = `placement="top"`(居中) —— 而 round6 §B 写 `top-end`(右缘对齐)。
根因:**round7 的「卡片弹出 + 与被点 ⋯ 按钮居中对齐」需求推翻了 round5/round6 的对齐预期,但只改了新清单、
没回写旧清单**。已按本仓规则**就地更正三处正文**(非只加脚注)+ round6 §H 加节首「已被 round7 §A 取代」横幅。
**教训**:验收清单是 normative 文本——它规定「什么算对」。后续轮次改了契约却只写进新清单,旧清单就变成
**会主动生产假 bug 报告的陷阱**,且它长得和有效清单一模一样。

**产出** `realtest-round10-2026-07-15-合并验收.md`:按**屏幕位置**而非轮次重排(设置页/对话框/画廊/顶栏/
选区条/查看器/路由 URL/合集人物),一次启动连续走完;~130 项 → **~80 项**。三处真实压缩:
- **跨重启批**:round6 胶囊位置 + round6 停靠形态 + round7 顶栏对齐 + round7 选区对齐 = 4 次重启 → **1 次**(§9);
- **六主题目验**:原 5 处(S1 §E / r5 §D / r6 §F / r6 §H / r7 §A)→ **1 处且缩减范围**——S7 矩阵已机器覆盖
  静态配色(settings 实测 0.0% 跨主题恒定),§10 只留矩阵**结构上够不到**的面(无指针→hover/active/focus 拍不到;
  弹层/对话框/docked 条不出镜)。**这是矩阵的直接红利:机器覆盖的面从人眼清单里删掉,而不是两边都留。**
- **round6 §H 整节剔除**(被 round7 §A 取代)。
另标注 ⚠️ **顶栏折叠须重验**:round7 §B 的观察建立在**旧量尺**上,而 07-15「闪展再收根治」重写了量尺链
(计数源头解耦 inline-grid sizer + recompute 统一不变量) → 不照抄旧结论。
另立 §12「需你判定」4 项(view-pref 全局携带 vs 纯 per-view / 嵌套弹层 dismiss 差异 / filtered-empty 图标
偏好 / 弹层 resize 行为)——**把裁决题与对错题分开**,避免用户在验收时被迫当场做产品决策。

**门禁**:check_docs / check_docs_index 均 exit=0(**本地,非 CI**)。零代码改动故未跑前端门禁。

**并行会话隔离**:仅动 `docs/planning/2026-07-11-uiux-refactor/` 下 8 份文档(新增 round10 + 6 份加横幅/更正 +
progress/findings),显式路径提交。

---

## 会话续 29(2026-07-16)——round10 真机验收回报 + 8 项修复落地

**触发**:用户走完 round10 合并清单,回报「除以下记录外,全部验收通过」+ 9 项异常 + 2 节未测。

### 做法

四路并行只读排查(设置页选中态 / 选区条量尺 / 收藏夹三问 / 撤销+全屏),每路给 file:line + 关键片段,
不提修复建议;主线据此定根因、决修法。三处关键判别:

- **#2**「再点一次才变蓝」= 决定性证据(第二次点击已在底部 → scrollTo 不产生 scroll → spy 不回填 → 置位活下来),
  排除了其它猜想。
- **#3** docked 好而浮动坏的**不对称**,只有「探针撑不动内容宽胶囊 + outlet 本就 flex:1 全宽」能同时说通。
- **#8** 向用户取了一个判别观察(「任务栏图标还在吗」),答「图标没了,纯黑一条」→ 锁定 **webview 没铺满**侧,
  排除「shell 没让位」侧。**猜错就白改一轮**,故没猜。

### 交付(4 提交,门禁 typecheck/lint/vitest 956/build 全绿,本地非 CI)

| 提交 | 内容 |
|---|---|
| `5d2d088` | #1/#2 设置页:程序化滚动锁(scrollend + 800ms 兜底) + 触底判末分区;判据剥成 `utils/settingsNav.ts` 纯函数 + 11 例单测(设置页此前零覆盖);顺带修 `behavior:'auto'` 被 CSS `scroll-behavior:smooth` 反噬 |
| `7f7e77e` | #3 选区条:探针同步撑胶囊 + ⋯ 宽常量 inline 绑到按钮 style(单一事实源);+4 例刻画正反馈回路 |
| `8eaebf3` | #9 弹层:卡片弹出 → 抽屉推拉(纯位移 24px,双轴泛化,删死掉的 transform-origin) |
| `74ea3f4` | #4 退出栏+Esc(person-view-bar 泛化为 view-back-bar) · #5 取数闸门(`enabled`+`flushIfDeferred`) · #6 `:not()` 排除角标 · #7 撤销搬 historyStore(`CallbackRecord`/`pushUndoable`/`undoIfTop` + AppShell 注册表兜底分发) |

### 三处「不是照着报告改」的判断

1. **#7 用户选了「全部搬进 historyStore」,但 `when: ctx.view === 'grid'` 不需要放宽**——查明 `view` 只区分
   grid/viewer,总览页 activeViewer 为 null → 判据本就成立。真正的缺口是 **AppToolbar 只在 `isGalleryRoute`
   为真时挂载**,合集/人物总览页根本没有分发器。故改 AppShell 兜底,且**仅在无查看器时**(查看器内键位由
   ContentViewer 自持,重复分发会双执行——这个 blast radius 是查了 viewer-image.ts 才排掉的)。
2. **`undoIfTop` 是主动加的**:多条 undo toast 可同时在屏而 `undo()` 恒撤栈顶,反序点击会「提示说 A、回来的是 B」。
   用户没报,但搬进栈之后这是新引入的正确性风险。
3. **`undoDelete` 改为不自捕获**:它原本自己 catch 并 toast,搬进 store 后会变成「既报错又报成功」(store 见不到
   异常就照常弹 undoMessage)。

### 本轮查出、用户未报的隐患(待裁决)

**软删除的收藏夹在 DB 里永久滞留却无任何 UI 可捞回**——删照片有回收站兜底(`get_trash`+`idx_media_trash`),
收藏夹没有;后端又无 purge。既没真删也捞不回=软删最坏形态。已写入 `historyStore.ts` 头注 + round10 待办。

### 清单回写

round10 frontmatter 改 `status: 已验收` + 结果表(9 项 + 2 节未测) + 修复台账 + 阶段门结论。
**§7 整节重写验证手段**:原文「盯地址栏」在 Tauri WebView 里不可能——改 DevTools Console
`location.hash` 读写,零代码改动。这是清单的缺陷,不是用户的。

### 阶段门

- **阶段 7(S1)**:真机面全通过 → 可开。
- **阶段 11(S5)**:#3 是复验挂,已修但**须 round11 复验**才能开门(#3 上次正是「已修但没真机」被 round7 无声撤销)。
- **阶段 8(S2)**:§7 仍未测 → 门未开。

### 遗留

round11 复验 8 项修复 · §7 走 DevTools 版 · §8.2 人物误检桶首验 · #8 F11 单独一轮(用户押后,且报另有多个
操作逻辑异常待并) · 收藏夹软删滞留待裁。

### 并行会话隔离

代码仅动 11 个前端文件 + 2 新增,文档仅动本目录三件套 + round10;全部显式路径提交,未 `git add -A`。

---

## 会话续 30(2026-07-16)——两项遗留裁决落地:分离模式工具栏常驻 + 收藏夹软删 limbo

**触发**:用户点名推进 L 节/round10 留下的两项待裁遗留。两项都**只差裁决、不差实现思路**,故先取
地面真相再一次问清,不猜。

### 裁决 1:分离模式工具栏常驻(`038656f`)

**先澄清歧义**:上一会话只在 todo 留了一句「分离模式工具栏是否常驻(用户直觉认可,未施工)」,
提案正文没落盘,而「常驻」有两种成立读法——① 沉浸态不隐;② 跨路由不消失。带证据与代价各自
列出后由用户裁,**答「两者都要」**(即「选了分离 = 这条 bar 永远在」)。已知代价当场写明并被接受:
分离模式下 F11 不再是「无顶栏」纯沉浸画廊(那是一天前刚验收的需求),想要纯沉浸走默认合并模式。

**三处「不是照着裁决直译」的判断**:

1. **AppToolbar 有意不随 bar 一起常驻**。它 onMounted 挂 document 级 keydown 注册表分发;而 AppShell
   的兜底分发(onKeyDown ①)正是 round10 #7 为「总览/设置页没有 AppToolbar」而加的。两者同时在场
   = 双执行。故做成「bar 常驻、内容按路由换」:画廊页给 AppToolbar,非画廊页只给页面标题。
2. **查看器路由不给槽**。裁决字面是「永远在」,但 /view /doc /audio 要的是最大内容面积且各自
   自持局部控制条,给一条空 bar 是纯负价值。范围收在「非查看器路由」。
3. **非画廊页渲染页面标题而非空条**。选项预览里写的是「空条或只留标题」,空 48px 是纯浪费;
   route.meta.title 每条路由都有(与窗口标题同源),取来即用,零新能力零风险。

**让位的几何**:标题栏沉浸态是 fixed 铺视口顶 40px,常驻的工具栏在流内 y=0..48 会被它盖住上半 →
标题栏唤出期工具栏整体 translateY(40px),与其拼成 40+48 两条栈。**恰好等于 sideChromeExtent('top')
量到的收起边界**(两条都带 data-chrome-top、offsetHeight 求和)→ 指针在栈内移动不误收,
useChromeReveal 一行未改。让位用 transform 不用 margin:唤出是高频 hover,不能推 .app-content 重排。
z-index/transition 挂 --immersive 而非 --pushed:否则去类瞬间 z-index 一并消失,回程 0.18s 里本条会被
后置兄弟 .app-content 盖住(两者都 position:relative,DOM 序决定层叠)。

### 裁决 2:收藏夹软删 limbo(`99dce7d`)

**取数改写了选项本身**:原 round10 待办把选项列为「回收站 / 定期 purge / 接受现状」,隐含前提是
「媒体侧有 purge、收藏夹没有」。实测**媒体回收站同样永不 purge**(全库无任何按 deleted_at 老化的
清理),二者真实差距只有**可达性**一条:media 有 get_trash + /trash + idx_media_trash,albums 的
deleted_at 只有写路径、零读路径。据此重列选项,用户裁**只补读路径、不加 purge**——补上即与媒体侧
同构;加 purge 反而让收藏夹的保留策略严于照片。

**实现要点**:list_deleted_collections 与 list_collections 的 deleted_at 判据严格取反(不重不漏划分,
单测锁死);有意不建索引(albums 数十行 vs idx_media_trash 服务百万行 keyset);排序测试直接钉死两个
时间戳——delete_collection 用 `strftime('%s','now')` 取整秒,同测试内两次删除撞同一秒会退化为 id 倒序
兜底,**测不出真实排序键**。UI 侧「已删除(N)」仅在有软删夹时出现(恒显空回收站是噪音,而它需要被
看见的时机恰好就是有东西可捞的时机)。

### 门禁

cargo test --workspace(主 lib 527/0/5,含新增 1 例)/ clippy --workspace -D warnings / rustfmt /
vue-tsc / eslint(全仓,CI 的真实前端门)/ vitest 1019 —— 全绿,**仅本地非 CI**。

**顺手挡掉一次污染**:`prettier --write` 改动文件时连带重排了 6 处**既存**超行宽的 i18n 长行 + 1 处
App.vue 旧 span(都非本次改动)。核实 CI 前端门是 `eslint .`、prettier 不在门内(且项目 CLAUDE.md 禁
仓库级 format)后,逐条还原,只留本次新增行。

### 遗留

- **⏸GUI(门禁不覆盖)**:F11 下贴顶唤出的 40px 让位观感 · 分离模式跨路由切换不跳 · 非画廊页标题条
  观感 · 重启后「已删除」区出现与恢复闭环。建议并入 round11。
- 前端侧本批无新增单测(改动面是布局 CSS 与视图接线,纯判据只是既测函数的两项合取);后端读路径
  已单测锁定。
- round11 复验 8 项修复 · §7 走 DevTools 版 · §8.2 人物误检桶首验 · #8 F11 单独一轮(用户押后)。

### 并行会话隔离

代码仅动 2(顶栏)+ 9(收藏夹)个显式路径,分两批提交;期间 HEAD 被并行会话推进到 `1d89018`
(docs/filter 线),无重叠。未 `git add -A`。
