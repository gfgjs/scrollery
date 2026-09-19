---
id: 2026-08-15-Scrollery清透材质化UI建议方案
status: active
type: design
line: UI-多主题系统
created: 2026-08-15
last-verified: 2026-08-20
---

# Scrollery 清透材质化 UI 建议方案

> 方案日期：2026-08-15
> 参考基线：`C:\workspace\Kun-0.2.28`；基线项目：当前 `C:\workspace\scrollery`
> 方案状态：建议方案，第一阶段已分批实施；首批落地 token 与 recipe 见 [material.css](C:/workspace/scrollery/src/assets/styles/material.css)，其余提案 token/数值仍需评审后实现
> 2026-08-20 审查修订：补齐媒体 Canvas 边界、合并标题栏覆盖范围、主题 token 契约与可复验收要求。
> 2026-08-20 施工回填：首批已获授权并落地（material adaptor + AppShell/WindowChrome chrome + 阅读区 surface 试点 + view popover/dialog float，提交 caebb68）；自动门禁全绿，但 Phase 5 主题矩阵与真实 WebView2 人工验收仍待执行，第 7/10 节验收条件未全部满足。

## 1. 方案摘要

建议 Scrollery 采用“保留主题身份、增加宿主材质层、减少重复外框、控制浮层 blur”的渐进式改造路线。

第一阶段不重做 Moonlight 的主色板，也不把 Scrollery 改造成 Kun 的页面结构。先在现有主题语义变量之上增加一层最小的 host material recipe，把下面五种角色统一起来：

1. `canvas`：应用工作区的底层画布；媒体网格的实际像素画布仍由专用 token 维护。
2. `chrome`：sidebar、toolbar、statusbar 等应用外壳。
3. `surface`：普通内容容器和设置区域。
4. `elevated`：需要从普通表面抬起的内容或控件。
5. `float`：popover、dialog、composer、提示等临时浮层。

然后在 AppShell、Settings、sidebar 和 toolbar 中逐步应用这套角色。目标不是让每一处都半透明，而是让页面形成稳定的层级顺序：

```text
canvas → chrome → surface → elevated → float
```

这条顺序如果成立，即使六套主题使用不同色相、纹理和暗色策略，界面仍然会有统一的清透与秩序感。

## 2. 与对比快照的关系

本方案依据 [Kun-0.2.28 与 Scrollery UI 清透感对比分析](C:/workspace/scrollery/docs/designs/2026-08-15-Kun-0.2.28与Scrollery-UI清透感对比分析.md)。对比快照得出的核心判断是：

- Moonlight 的冷白、浅蓝灰、蓝色强调色方向已经可用。
- 当前差异主要在宿主级材质覆盖范围、表面连续性、重复卡片数量和信息密度。
- Kun 的“清透”不是全局 blur；它依赖透明表面、轻渐变、低噪声边框、低对比阴影和留白共同作用。
- Scrollery 是资产管理器，不能简单复制 Kun chat/settings 的低密度，需要在高密度场景中压低轮廓噪声。

因此，本文优先给出能复用到六套主题、且不改变数据与交互模型的视觉方案。

## 3. 目标与非目标

### 3.1 目标

- 让 AppShell 的 canvas、chrome、surface、float 关系在所有主题中可解释、可维护。
- 让 Moonlight 在不换主色的前提下更接近“浅、透、净、连续”的观感。
- 降低 Settings、sidebar 工具区和 toolbar 的重复外框与边界噪声。
- 保留 gallery 的检索、筛选、批量操作和状态可见性。
- 保持 Xuan 的纸张纹理、Ink/Obsidian 的深色身份，以及其他主题的色彩差异。
- 把 blur 限制在小面积、短链路、真正浮起的区域，控制 GPU 和滚动性能风险。
- 为后续视觉回归提供明确的场景、主题、状态和验收标准。

### 3.2 非目标

- 不复制 Kun 的页面布局、聊天信息架构、文案或组件命名。
- 不把所有背景改成纯白、所有卡片改成透明，或删除所有边框。
- 不通过大规模改字号、换字体来解决主要问题。
- 不给整个滚动区域、媒体网格或大面积 sidebar 默认启用 backdrop blur。
- 不在本方案阶段改变主题注册、用户主题选择、资源加载和数据交互逻辑。
- 不把 Xuan 的纸张噪声或任何单一主题的装饰扩散到其他主题。

## 4. 设计原则

### 4.1 先建立层级，再调整装饰

任何渐变、玻璃或阴影都必须回答“它把哪个层级从哪个层级中托出来”。如果一个效果不能帮助用户判断 canvas、surface、active 或 float 的关系，就不应添加。

### 4.2 画布优先，表面从画布中长出来

媒体画布和主要内容区应保持相对连续。普通表面可以略透出 canvas 的色相，外层 chrome 与 canvas 只保留轻微差异；真正需要独立阅读对比的内容再使用更稳定的 elevated 表面。

### 4.3 一个大表面优于多个同强度小卡片

在 Settings 这类分组页面中，相关内容优先放入一个大表面，内部使用标题、说明、分隔线和行 hover。只有危险区、独立的外部操作或需要明显状态承载的区域保留独立卡片。

### 4.4 强调色只服务于动作和状态

普通文字、辅助说明和未激活 chip 不应同时争夺强调色。强调色集中到 active、selected、focus、primary action 和明确的状态反馈，清透感会更稳定。

### 4.5 Blur 是浮层材料，不是页面背景

默认 blur 只推荐用于 toolbar/header 的小范围悬浮材料、popover、dialog、composer、fullscreen hint 等。滚动内容应优先使用透明度、渐变和阴影，不把大量像素交给 backdrop filter。

### 4.6 主题提供个性，宿主提供秩序

主题文件负责色相、亮暗、纹理和少量材质参数；host material recipe 负责角色、层级、边界和使用纪律。这样 Moonlight 可以是冷白玻璃，Xuan 可以是温暖纸张，Ink 可以是深色低反射表面，但组件结构不需要为每个主题复制一套 CSS。

## 5. 目标材质架构

### 5.1 建议的语义角色

以下变量名是建议名称，不应在未评审前直接视为稳定公共 API。实现时可以根据现有命名体系调整，但角色应保持清楚。

| 提案角色 | 负责内容 | 建议来源 | 允许的主题差异 |
| --- | --- | --- | --- |
| `--material-canvas` | AppShell、非媒体主内容与 viewer 宿主背景 | `--color-bg-primary` | 色相、明暗、是否带极轻渐变 |
| `--material-chrome` | sidebar、toolbar、statusbar | `--color-bg-secondary` | 透明度、轻渐变、纹理开关 |
| `--material-chrome-gradient` | chrome 的连续光照 | 主题组合值 | 亮色可用 haze，暗色应弱化或关闭 |
| `--material-surface` | 普通容器、设置主体 | `--color-bg-surface` / `--card-bg` | alpha、表面暖冷度 |
| `--material-surface-strong` | 需要稳定对比的内容 | `--color-bg-elevated` | 不透明度、边界强度 |
| `--material-surface-muted` | 次级分组、低优先级条目 | `--color-bg-inset` | 与 canvas 的接近程度 |
| `--material-border` | 普通分隔和低对比边界 | `--color-border` | alpha 和色相 |
| `--material-border-strong` | focus、selected、浮层边缘 | `--color-border-strong` | 对比度和强调色混入量 |
| `--material-shadow-shell` | shell 与 canvas 的轻分离 | 现有 shadow token | 亮暗、扩散半径 |
| `--material-shadow-panel` | 大表面托举 | 现有 shadow token | 亮暗、扩散半径 |
| `--material-shadow-float` | popover/dialog/composer | 现有 shadow-lg/xl | 亮暗、扩散半径 |
| `--material-glaze` | 大气高光或 radial haze | 新增主题参数 | 可关闭为 `none`、可换色、不可覆盖媒体本体 |
| `--material-blur-float` | 浮层背景采样 | 新增主题参数 | 建议 8–16px；关闭时为有效的 `0px`，而非无效 CSS 值 |
| `--material-noise` | 主题纹理 | 现有 `--texture-chrome` | Xuan 可启用，其他主题默认关闭 |

**媒体 Canvas 边界：**`--material-canvas` 不替换 gallery 的 `--color-bg-canvas` 与
`--color-bg-canvas-gap`。后两者由 Canvas 渲染管线读取为具体颜色，且现有主题契约要求 gap
与画布底色保持一致。若未来确需改变媒体像素画布，必须同批更新 Canvas palette、主题契约和高密度
画廊人工回归；不得只从 CSS 宿主层间接改色。

### 5.2 与现有主题变量的兼容方式

建议先以 alias/adaptor 方式接入，不立即重命名现有变量。别名方向只能是“既有 source token →
material token → 新消费者”，旧 token 在首批不得反向引用 material token，避免形成循环：

```text
现有 --color-* / --card-bg / --popover-bg
                ↓
     material recipe 的角色别名
                ↓
     AppShell / Settings / Sidebar / Toolbar
```

`material.css` 应在六份主题文件之后、任一消费者样式之前导入，并在 `:root` 提供由既有
`--color-*`、shadow、card/popover token 派生的默认值。只有确有视觉必要的参数才由主题覆盖；任何
主题专有 material 参数必须六套主题同时声明同一键集，并扩展主题契约测试以覆盖定义、消费与 fallback。

这样做有三个好处：

- 既有组件可以分批迁移，不需要一次性重写所有 CSS。
- 未迁移组件继续消费旧 token；material 默认值存在时，主题切换不会因缺少新变量而静默失效。
- 后续如果角色命名需要调整，可以在 adaptor 层完成，而不是把组件全部再改一遍。

### 5.3 推荐的层级约束

建议把以下规则写进 recipe 的注释或设计文档中：

| 层级 | 背景 | 边框 | 阴影 | blur | 典型对象 |
| --- | --- | --- | --- | --- | --- |
| canvas | 最连续、最低噪声 | 通常无 | 无或极轻 | 无 | AppShell 主内容、viewer 宿主背景；媒体像素画布沿用专用 canvas token |
| chrome | 比 canvas 略有区分 | 1 条低对比分隔 | shell 级轻阴影 | 小面积可选 | sidebar/toolbar/statusbar |
| surface | 半透明或稳定浅色 | 弱边框 | panel 级轻阴影 | 通常无 | settings 主容器、工具区 |
| elevated | 更稳定的背景 | 稍强边框 | md/lg | 通常无 | selected block、重要内容 |
| float | 与下层有明显层级差 | 强边框或高光 | float 级阴影 | 仅小面积 | popover/dialog/composer |

不建议让相邻的两个层级使用完全相同的背景、边框和阴影组合；否则语义角色虽然存在，视觉上仍然会变成同一层。

## 6. 按区域的改造建议

### 6.1 AppShell：先建立全局连续感

当前入口：[AppShell.vue](C:/workspace/scrollery/src/components/layout/AppShell.vue:295)、[App.vue](C:/workspace/scrollery/src/App.vue:30)、[WindowChrome.vue](C:/workspace/scrollery/src/components/layout/WindowChrome.vue:132)。

建议：

- `.app-shell` 承载 `material-canvas`，作为非媒体像素层的工作区底色；gallery 自身仍保持专用 canvas token 的中性。
- `.app-sidebar`、独立模式的 `.app-toolbar`、`.app-statusbar` 与 `.window-chrome` 统一使用 `material-chrome`，但保留现有尺寸、拖拽区域和布局行为。
- 默认合并模式下，`AppToolbar` 的 fragment 直接位于 `WindowChrome` 内；chrome recipe 只应由该容器承载一次，不能叠出第二层背景、边框或阴影。独立与合并两种 toolbar 模式均须验收。
- 亮色主题可给 chrome 添加非常轻的纵向渐变或 haze；暗色主题不直接复用亮色 alpha，改由主题提供暗色配方。
- sidebar 与 toolbar 的分隔线优先使用 `material-border`，不要同时叠加高对比边框和明显阴影。
- statusbar 继续保持低存在感；它不应变成第三块突出的卡片。
- viewer toggle、fullscreen hint 继续作为局部浮起对象，不把它们的 blur 扩展到整个 AppShell。

第一批只改背景、边框、阴影和渐变角色，不改独立 toolbar 的 48px、合并标题栏的 40px、statusbar 28px、sidebar 宽度等几何尺寸；同时保持窗口三键、拖拽区、沉浸模式和 viewer route 覆盖层行为，以便回归问题容易归因。

### 6.2 Settings：从“卡片墙”变成“大表面+分隔线”

当前入口：[SettingsView.styles.css](C:/workspace/scrollery/src/views/SettingsView.styles.css:53)，基础卡片规则在 [index.css](C:/workspace/scrollery/src/assets/styles/index.css:409)。

建议分三步：

1. Settings 主内容保留一个 `material-surface` 或 `material-surface-strong` 外壳，承载相关 section。
2. 普通 section 使用标题、说明、行间分隔线和 hover，不默认再套一层独立背景。
3. 对危险操作、外部账户、导入导出或明显独立的管理对象保留 `elevated`/card 表达。

具体视觉目标：

- 相邻 section 不再全部拥有同样强度的圆角、边框和阴影。
- section 之间用留白和 1px 分隔线建立关系，而不是用多张白卡片建立关系。
- header 的现有半透明和 blur 可以保留，但应与 AppShell 的 chrome recipe 共用参数。
- collapsed accordion 的 hover/active 需要清晰，但闭合状态不应像一张悬浮卡片。
- 设置导航仍可保持独立区域，以免牺牲定位效率；先降低背景与边框对比，不先取消结构。

### 6.3 Sidebar：区分“导航入口”和“工具对象”

当前入口：[LibrarySection.vue](C:/workspace/scrollery/src/components/sidebar/sections/LibrarySection.vue:1)、[ToolsSection.vue](C:/workspace/scrollery/src/components/sidebar/sections/ToolsSection.vue:690)。

建议：

- Library 普通导航尽量融入 chrome 背景，active 只使用浅色强调背景、文字/图标色和必要的左侧或内嵌指示。
- 普通 folder 行不使用独立阴影；hover 使用低 alpha 表面变化。
- Tools 中真正可拖拽、可配置或有状态的对象保留 surface；纯入口型工具降低边框和阴影。
- sidebar 内部保留一个明确的分区节奏，但不要让每个 section header、tool card、folder group 都成为同强度容器。
- 主题纹理若启用，只应用于 chrome 背景，不能让文字和选中态的局部对比变脏。

### 6.4 Toolbar：保留密度，降低轮廓噪声

当前入口：[AppToolbar.vue](C:/workspace/scrollery/src/components/layout/AppToolbar.vue:657)。

建议：

- 搜索框继续保持清晰的输入边界和 focus ring；它属于 surface/elevated，不应为了透明而降低输入可用性。
- 未激活的 filter chip 可以使用较弱背景和边框，激活 chip 才使用强调色或明显填充。
- view/filter popover 统一使用 `float` recipe，保持现有定位、关闭和键盘行为。
- 多个并列 icon button 不需要都拥有独立的高对比背景；只有 hover、pressed、active 状态显形。
- 搜索状态的 accent glow 需要限制范围，不能与全局 glaze 叠加成大面积蓝光。

### 6.5 Gallery 与 Viewer：让内容成为视觉焦点

建议：

- gallery/viewer 主区域保持 `material-canvas` 的中性和连续，尤其不要把 sidebar 的蓝色 haze 直接铺到图片上。
- 媒体网格的实际画布继续使用 `--color-bg-canvas` / `--color-bg-canvas-gap`；它不是普通 CSS surface，也不应在本批被透明 recipe 取代。
- 媒体卡片的选中、hover、错误、加载等状态继续使用明确反馈，但普通卡片不额外增加大面积阴影。
- badge、标签和状态点使用紧凑且高识别的局部表达；不要让每个 metadata 都生成新的圆角表面。
- 现有 `MediaGrid`、`MediaThumb`、`SelectionToolbar` 的局部 backdrop filter 先完成“保留 / 移除 / 后续专项”分类；第一批不顺手迁移它们，任何变动均需单独做高密度性能回归。
- 图片预览 overlay 可以使用暗色 float recipe，但只覆盖需要阅读控件的区域。
- 如果未来加入玻璃化工具栏，优先放在 viewer 的固定控制区，不对图片本身做 backdrop blur。

### 6.6 Popover、Dialog、Composer：集中使用浮层材料

Scrollery 当前已经在 popover/dialog 中使用 shadow、border 和局部 blur，这是最适合首先统一的区域。建议：

- 所有浮层统一 `material-surface-strong` + `material-shadow-float` + 可选 `material-blur-float`。
- 浮层边缘使用一条低 alpha 高光或强边框即可，不同时使用深边框、厚阴影和强渐变。
- dialog overlay 的暗化程度按主题调整，避免亮色主题中背景变脏、暗色主题中背景被压黑。
- 浮层内容区域保持现有 padding 和交互尺寸，先改材料，不改行为。
- 需要长列表的 popover 不在内部叠加多个半透明层，避免文字和滚动条对比下降。

## 7. 分阶段实施计划

具体施工状态与可回退操作维护在[清透材质化 UI 实施计划](C:/workspace/scrollery/docs/planning/2026-08-19-清透材质化UI实施计划/task_plan.md)；本节定义设计边界与通过条件，实施计划不得放宽它们。

### Phase 0：基线与样本锁定

目标是让后续每次视觉变化可比较，不改生产视觉。

工作项：

- 在 1440×900 下运行 `npm run capture:themes`，生成六主题 × gallery/settings/viewer 的 18 张可再生产物，并记录命令、浏览器版本、主题、场景与窗口尺寸。
- popover、dialog、selected、hover/focus、empty/loading 必须补齐可复现的 harness 状态，或保留逐步人工操作清单；现有捕获器只覆盖 gallery/settings/viewer，不能把其余状态视为已覆盖。
- 记录 light/dark、sidebar 展开/收起、无选中/选中、hover/focus、空状态和加载状态。
- 标记当前所有直接使用 `--card-bg`、`--popover-bg`、`box-shadow`、`backdrop-filter` 的位置，并为每处标记对象类型、保留/移除/后续专项处置、可见数量与性能检查方式。
- 在真实 Windows WebView2 中观察普通 gallery 与高密度场景；headless 捕获器禁用 GPU，只能验证布局与配色，不能证明滚动或合成性能。

验收：有一组可重复截图、人工操作步骤和明确的“不改动 / 可试点 / 后续专项”清单，能按主题、场景、状态定位差异。

### Phase 1：建立最小 material adaptor

建议涉及：

- 新增或整理 `src/assets/styles/material.css`，只放宿主材质角色和少量通用 recipe。
- 在 `src/assets/styles/index.css` 中于六份主题之后、消费者样式之前以明确顺序引入它。
- 先在 `:root` 通过现有 `--color-*`、shadow、card/popover token 派生角色；仅确有必要的材质参数才在六套主题文件同步声明同键集。
- 不删除旧变量，不改变主题注册和默认主题。
- 补充 material token 的定义/消费/fallback 契约，或扩展 `theme-contract.spec.ts`；禁止 source token 与 material token 互相回指。
- `--material-glaze` 关闭为 `none`，`--material-blur-float` 关闭为 `0px`，并保留不支持 backdrop filter 时可读的实色 fallback。

验收：主题切换无缺失变量、循环别名或死 token；现有页面视觉不应出现大范围断层；新 recipe 可以被 AppShell 和一个试验性 settings surface 使用。

### Phase 2：改造 AppShell

建议涉及：

- `src/components/layout/AppShell.vue`
- `src/App.vue`
- `src/components/layout/WindowChrome.vue`
- `src/assets/styles/material.css`
- 必要时六套主题的 chrome 变量

实施顺序：

1. 先统一 canvas/chrome 的背景角色。
2. 再统一 shell 边框和阴影。
3. 最后加入亮色主题的轻渐变/haze，暗色主题单独调配。

验收：sidebar、标题栏合并/独立两种 toolbar、statusbar 与内容区有连续但可识别的层级；窗口三键、拖拽区、沉浸模式和 viewer route 覆盖层无行为回归；没有新增大面积 blur；媒体画布颜色不被污染。

### Phase 3：收敛 Settings 和 sidebar

建议涉及：

- `src/views/SettingsView.styles.css`
- `src/assets/styles/index.css` 的 `.settings-card` recipe
- `src/components/sidebar/sections/LibrarySection.vue`
- `src/components/sidebar/sections/ToolsSection.vue`

实施顺序：

1. 先在 Settings 选择一个 section 做大表面实验。
2. 对比重复卡片减少前后的可读性和定位效率。
3. 再决定哪些 section 可以合并，哪些必须保留独立 card。
4. sidebar 只先降低普通入口的容器感，保留 active、拖拽和工具状态。

验收：Settings 不再呈现连续同强度卡片，但标题、分组和危险区域仍然一眼可定位；sidebar 的 active、hover、focus 和工具状态不退化。

### Phase 4：统一 toolbar 与浮层

建议涉及：

- `src/components/layout/AppToolbar.vue`
- `src/assets/styles/index.css` 中 popover/dialog/overlay 规则
- viewer overlay 和 composer 相关样式

实施重点：

- 让所有真正浮层共享 shadow/border/blur recipe。
- 让未激活 chip 和普通 icon button 退回低噪声状态。
- 保留输入框和 primary action 的可操作对比。
- 不迁移 `MediaGrid`、`MediaThumb`、`SelectionToolbar` 等已有媒体局部覆盖层；它们只在独立性能专项中裁决。

验收：浮层之间的视觉语言统一；键盘 focus、ESC 关闭、点击外部关闭和滚动行为不变。

### Phase 5：主题矩阵与性能收口

验收覆盖：

- Moonlight、Porcelain、Ink、Obsidian、Xuan、Dai 六套主题，以及各主题所属的 light/dark 槽位。
- Settings、gallery、viewer、toolbar popover、dialog、empty/loading/selected/hover/focus 状态。
- 1440×900 基线，以及至少一个较窄窗口宽度；标题栏合并/独立模式均包含在矩阵或人工步骤中。
- `npm run check:contrast`、`npm run lint`、`npm run typecheck`、`npm test`、`npm run build`、`npm run verify:channel`；主题 CSS 变更还须通过 `theme-contract.spec.ts`。
- 维持现有纯色对比度硬门；对透明、`color-mix` 或渐变合成后的真实背景人工复核：正文和状态文本至少 4.5:1，辅助文本至少符合既有门槛且不低于 3:1，focus-visible 指示器相对相邻颜色至少 3:1。
- 在真实 Windows WebView2 中手动检查普通与高密度 gallery 的滚动、窗口缩放、透明度、焦点可见性、全屏/沉浸模式、viewer 往返和媒体色彩；出现新增卡顿、对比下降或合成层扩大时先回退相关 blur/透明度。

## 8. 风险与缓解

| 风险 | 表现 | 缓解方式 |
| --- | --- | --- |
| 透明度造成对比不足 | 次文字、边框、禁用态在浅色主题中消失 | 保持既有纯色 contrast gate；以实际背景合成后的 4.5:1 / 3:1 阈值人工复核，保留 surface-strong |
| blur 影响性能 | 滚动或大列表掉帧、GPU 占用上升 | 只给小面积固定/浮层对象使用；先为既有媒体局部 blur 分类，再以真实 WebView2 的普通与高密度场景复核；禁止媒体网格和长列表父层 blur |
| 六套主题失去身份 | 所有主题都变成同一套白色玻璃 | recipe 固定结构，主题保留色相、纹理和暗色配方 |
| Xuan 纹理变脏 | 纹理与 glaze/阴影叠加后文字不清 | texture 只在 chrome 开启，必要时关闭 glaze 或降低 alpha |
| 卡片减少后定位困难 | 用户无法快速判断 section 边界 | 保留标题、分隔线、留白和 active/hover；做 Settings 可用性回归 |
| CSS 优先级或 token 契约冲突 | scoped style 覆盖 material recipe，主题切换不一致或变量回退失效 | 先建立明确导入顺序、单向角色别名、fallback 与主题契约测试，分批迁移，避免 `!important` |
| 暗色复用亮色参数 | 暗色出现白色光晕、层级反转 | 每个主题至少提供 canvas/chrome/surface/shadow 的暗色验证值 |
| 视觉变化与产品密度冲突 | gallery 的状态和操作变得不明显 | 只压低非关键轮廓，不降低选中、focus、错误和主要动作的对比 |

## 9. 建议的第一批变更边界

如果后续批准实施，建议第一批只做以下内容：

1. 新增最小 material adaptor 和主题兼容变量。
2. 将 AppShell 与 WindowChrome 在标题栏合并/独立两种模式下的 canvas/chrome 背景、边框、阴影切换到 adaptor。
3. 选择 Moonlight Settings 的一个区域，验证“大表面+分隔线”方案。
4. 将一个 toolbar popover 和一个 dialog 切换到统一 float recipe。
5. 生成六主题和关键状态截图，比较清透感、对比度与密度。

第一批不做：

- 全量替换所有 `.settings-card`。
- 全量重写 sidebar 结构。
- 改变默认主题或删除任何现有主题。
- 统一修改所有圆角、字号和间距 token。
- 给所有容器添加 `backdrop-filter`。
- 顺手迁移 `MediaGrid`、`MediaThumb`、`SelectionToolbar` 等已有媒体局部覆盖层；它们保留给独立性能专项。

这样可以把问题拆成“材料层是否成立”和“分组是否需要收敛”两个可验证假设，避免一次改动过大而无法判断收益来源。

## 10. 验收清单

### 视觉层级

- [ ] canvas、chrome、surface、elevated、float 在截图中能够被区分，但没有重边框堆叠。
- [ ] 普通表面比浮层更安静，浮层的层级来自阴影/边界/局部 blur，而不是更深的底色。
- [ ] Settings 的相关 section 以大表面、标题和分隔线组织，不再全部呈现同强度卡片。
- [ ] sidebar 普通导航融入 chrome，active/hover/focus 仍然清晰。
- [ ] gallery 图片颜色不被全局 glaze 或蓝色 haze 污染。
- [ ] gallery 的实际 Canvas 仍使用专用 `--color-bg-canvas` / `--color-bg-canvas-gap` 契约，未因 AppShell 材质变化而出现底色断层。

### 主题与状态

- [ ] 六套主题均无白色/黑色硬编码导致的断层。
- [ ] Moonlight 保持冷白蓝灰身份；Xuan 仍保留纸张纹理；Ink/Obsidian 不出现亮色玻璃反光。
- [ ] active、selected、focus、disabled、error、loading、empty 状态均可识别。
- [ ] 主题切换前后 popover、dialog、toolbar、settings 的层级关系一致。
- [ ] 标题栏合并/独立模式下，toolbar、窗口三键与拖拽区均保持可用且层级一致。

### 性能与可访问性

- [ ] 长列表、媒体网格和滚动容器没有新增默认 backdrop blur；每个既有或新增 blur 都有保留/移除/专项处置和性能检查记录。
- [ ] 透明表面合成后正文和状态文本至少 4.5:1，辅助文字至少符合既有门槛且不低于 3:1，focus-visible 指示器相对相邻颜色至少 3:1。
- [ ] `prefers-reduced-motion` 或现有动效约束不被新渐变/过渡绕过。
- [ ] 窗口缩放、全屏、系统缩放和高 DPI 下没有明显边缘断裂。

### 工程验证

- [ ] `npm run check:contrast`、`npm run lint`、`npm run typecheck`、`npm test`、`npm run build`、`npm run verify:channel` 通过；主题 CSS 变更同时通过 `theme-contract.spec.ts`。
- [ ] 未引入 `any`、无必要的 `!important`、循环 alias 或缺失 material fallback。
- [ ] 保留一份变更前后截图矩阵、popover/dialog/关键状态的复现步骤，以及真实 WebView2 人工检查记录。

## 11. 待评审决策

以下事项建议在实际编码前确认：

| 决策项 | 推荐意见 | 原因 |
| --- | --- | --- |
| 是否新增 material adaptor 文件 | 是 | 将宿主材料与主题颜色解耦，减少 scoped CSS 分散配方 |
| 是否先改 Moonlight 基础色 | 否 | 当前色相已接近目标，先验证层级和分组收益 |
| 是否全局启用 glaze | 否；仅亮色 chrome 小范围启用 | 避免污染媒体画布和暗色主题 |
| 是否全局启用 backdrop blur | 否 | 性能成本高，且不是清透感的必要条件 |
| 是否删除所有 settings-card | 否 | 需要保留危险区和独立对象的语义容器 |
| 是否调整全局圆角/字号 | 暂缓 | 先验证表面连续性和密度，避免同时改变太多变量 |
| 是否保留 Xuan texture | 是 | 它是主题身份，不应被冷白材料方案覆盖 |
| 是否改变默认主题 | 否 | 视觉问题与默认主题选择无直接证据关系 |

## 12. 预期结果

完成上述路线后，Scrollery 不需要变成一个低密度的聊天应用，也不需要在每个区域复制 Kun 的 CSS。理想结果是：

- 画布、外壳和内容表面之间有连续的冷静光照。
- Settings 和 sidebar 的信息量仍在，但不再由大量同强度卡片同时强调。
- toolbar、popover、dialog 和 viewer overlay 使用统一的浮层材料。
- 主题之间保持各自身份，清透感成为可调的宿主能力，而不是 Moonlight 的专属装饰。
- 用户首先看到的是资产、结构和当前状态，而不是一组互相竞争的边框、阴影和背景块。

最终判断标准不是“看起来像 Kun”，而是：在 Scrollery 自己的资产管理密度和主题体系下，界面是否更连续、更安静、更易读，同时仍然能快速找到操作和状态。

## 13. 关联文件

- 对比依据：[2026-08-15-Kun-0.2.28与Scrollery-UI清透感对比分析.md](C:/workspace/scrollery/docs/designs/2026-08-15-Kun-0.2.28与Scrollery-UI清透感对比分析.md)
- Scrollery 主题注册：[registry.ts](C:/workspace/scrollery/src/themes/registry.ts:73)
- Scrollery 全局变量：[variables.css](C:/workspace/scrollery/src/assets/styles/variables.css:1)
- Scrollery 六套主题：[moonlight.css](C:/workspace/scrollery/src/assets/styles/themes/moonlight.css:1)、[porcelain.css](C:/workspace/scrollery/src/assets/styles/themes/porcelain.css:1)、[xuan.css](C:/workspace/scrollery/src/assets/styles/themes/xuan.css:1)
- Scrollery 全局基础样式：[index.css](C:/workspace/scrollery/src/assets/styles/index.css:1)
- Scrollery 外壳：[AppShell.vue](C:/workspace/scrollery/src/components/layout/AppShell.vue:295)、[App.vue](C:/workspace/scrollery/src/App.vue:30)、[WindowChrome.vue](C:/workspace/scrollery/src/components/layout/WindowChrome.vue:132)
- Scrollery toolbar：[AppToolbar.vue](C:/workspace/scrollery/src/components/layout/AppToolbar.vue:554)
- Scrollery Settings：[SettingsView.styles.css](C:/workspace/scrollery/src/views/SettingsView.styles.css:53)
- Canvas 调色板：[mediaGridCanvas.palette.ts](C:/workspace/scrollery/src/components/media/mediaGridCanvas.palette.ts:72)
- 主题契约：[theme-contract.spec.ts](C:/workspace/scrollery/src/themes/theme-contract.spec.ts:123)；对比度门：[check-theme-contrast.mjs](C:/workspace/scrollery/scripts/check-theme-contrast.mjs:74)；主题截图捕获器：[capture-theme-matrix.mjs](C:/workspace/scrollery/scripts/capture-theme-matrix.mjs:28)
- 当前施工工作记忆：[清透材质化 UI 实施计划](C:/workspace/scrollery/docs/planning/2026-08-19-清透材质化UI实施计划/task_plan.md)

## 14. 方案结论

建议批准“宿主材质层 + 大表面分组 + 局部浮层 blur”的方向，先以 Moonlight AppShell/WindowChrome、一个 Settings section、一个 popover 和一个 dialog 做小批量验证。只有当第一批同时满足媒体 Canvas 边界、主题兼容、标题栏两种模式、视觉层级、性能和可访问性验收，才继续扩展到全量 Settings、sidebar 和 gallery chrome。

本方案是后续实现与评审的设计基线。2026-08-20 审查修订已采纳，第一阶段施工已获授权并落地（caebb68，仅覆盖第 9 节第一批范围）；Phase 5 主题矩阵与真实 WebView2 人工验收通过前，第 10 节验收清单不得翻为完成，后续扩面须另获授权。
