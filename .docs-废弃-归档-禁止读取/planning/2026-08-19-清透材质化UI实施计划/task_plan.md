---
status: 施工中
type: 工作记忆
line: UI-多主题系统
created: 2026-08-19
---

# 任务计划:清透材质化 UI 实施

## 目标

在不改变 Scrollery 的数据模型、主题注册、窗口几何或交互契约的前提下，分批验证并落地“宿主材质层 + 大表面分组 + 小面积浮层 blur”。

终态不是复制 Kun，也不是把所有组件玻璃化；应让 canvas、chrome、surface、elevated、float 五层在六套主题中保持可辨、低噪声且可访问，并让高密度资产管理场景的性能不退化。

## 当前阶段

第一批四条并行 lane（Phase 0 基线、material foundation、shell/settings/float 侦察与实现、quality gate 侦察）已全部落盘，合并后自动门禁通过。2026-08-20 工程侧复跑全绿：typecheck / lint / check:contrast 六主题硬门槛 / vitest 135 文件 1549 测（含 theme-contract 键集、alias、单向引用、关闭值） / build（主 bundle 678.45kB/708kB） / verify:channel（14 断言 + 112 文件）。阶段 0 至 4 的代码侧与工程侧判定已就位（详见各阶段状态注记），但视觉/性能验收仍待 phase5 主题矩阵与真实 Windows WebView2 人工 GUI 验收后方可翻为完成。

阶段集成、状态翻转与最终验收只由主任务在完成实际验证后判断；自动门禁通过不等于视觉/性能验收通过，也不构成超出本计划止损线的扩面授权。本次"工程侧判定"仅确认可由契约/门禁/代码事实证明的部分，不替代人工视觉与真机验收。

## 范围与止损线

- 第一批只覆盖 material adaptor、AppShell/WindowChrome、一个 Moonlight Settings 区域、一个 AppToolbar popover 和一个 dialog。
- 主题注册、默认主题、用户主题选择、资源加载、媒体数据及任何业务交互不在本线改动范围内。
- 不删除现有 color/card/popover token；新层先以 alias/adaptor 接入。
- 禁止给媒体网格、长列表父层、整块 sidebar 或整个滚动容器新增 backdrop-filter。
- 不把全量 Settings 卡片、sidebar 结构、圆角、字号、间距 token 绑进第一批。
- 每个阶段单独可回退；若视觉、可访问性或性能门槛未过，停在当前阶段修正或撤回，不扩面。

## 本轮并行结构（2026-08-20）

| lane | 当前边界 | 状态 |
|---|---|---|
| Phase 0 基线 | 运行现有主题矩阵捕获器，记录可复现环境、产物和人工检查缺口；不把 headless 结果当成 GPU/WebView2 性能证明 | 已完成（18 张基线图 + harness 修复；WebView2 性能观察留人工） |
| material foundation | 仅施工最小 adaptor/alias 与相关契约地基；不迁移 AppShell、Settings、popover 或 dialog 消费者 | 已完成（material.css + index.css 导入 + theme-contract 扩展，14 测） |
| shell/settings/float 侦察 | 只读梳理候选消费者、作用域、交互与风险边界，为后续主任务裁决提供映射；不改生产文件 | 已完成（映射已转化为实现） |
| quality gate 侦察 | 只读核对现有门禁、可执行验证入口与人工 GUI 缺口；未实际运行的门禁不得记为通过 | 已完成（基线 71 测 + 六主题 126/126 对比度 + 人工清单） |

各 lane 保持文件 ownership 隔离并保留现有 dirty 内容；生产代码写入仅限 foundation lane 的已授权边界。主任务负责集中集成、阶段翻转和验收结论。

## 阶段

### 阶段 0:锁定视觉与性能基线

- [ ] 在 1440×900 下运行现有主题矩阵捕获器，生成六主题 × gallery/settings/viewer 的 18 张基线图。
- [ ] 记录截图命令、浏览器版本、主题、场景、窗口尺寸和人工检查结论；截图放在可再生产物目录，不将其当作 pixel-diff 基线。
- [ ] 为 popover、dialog、selected、hover/focus、empty/loading 补齐可复现的 harness 状态或人工操作步骤。当前 harness 只声明 gallery/settings/viewer，不能把浮层状态假定为已覆盖。
- [ ] 建立当前 blur、card、popover 和 shadow 消费点清单，明确哪些属于浮层、哪些属于媒体局部覆盖层，防止误把必要的媒体控件纳入全局改造。
- [ ] 记录 `--color-bg-canvas` / `--color-bg-canvas-gap` 的媒体 Canvas 契约；material canvas 只改宿主背景，不得间接改写 Canvas 像素调色板。
- [ ] 用真实 Windows WebView2 手动观察普通 gallery 与高密度 harness 场景；headless 捕获器使用 disable-gpu，只能验证布局和配色，不能证明 GPU 滚动性能。
- **退出条件:** 基线图、人工检查步骤和”允许改动/明确不改动”的区域清单可供后续批次复现。
- **状态:** in_progress — 工程侧已满足（18 张 1440×900 基线图 + 浮层/状态覆盖缺口 + 允许/不改动清单均在案），视觉判定留人工。

### 阶段 1:建立最小 material adaptor 与契约

- [ ] 新建 src/assets/styles/material.css，只定义宿主层角色及复用 recipe；在 index.css 的主题导入之后、消费者样式之前以固定顺序引入。（最小 foundation lane 已于 2026-08-20 获授权与 Phase 0 并行施工，当前尚未集成、未验证。）
- [ ] 先将 material 角色从既有 color、shadow、card/popover token 派生；仅在确有视觉必要时，为六套主题同时增加同键集的主题材质参数。
- [ ] 以候选角色组织，而非过早冻结命名：canvas、chrome、surface、surface-strong、surface-muted、border、shadow、glaze、float blur。实际名称须保持与现有 token 风格一致。
- [ ] 保留旧 token 与既有消费者，先让一个试点组件改走新 recipe；不得用 important 覆盖 scoped CSS。
- [ ] 补充针对 material token 的小型契约测试，或扩展现有 theme-contract.spec.ts，确保定义、消费、六主题键集和 fallback 关系不会静默漂移。
- [ ] 明确 source token → material token → 新消费者的单向 alias；glaze 关闭为 `none`、float blur 关闭为 `0px`，不得与旧 card/popover token 形成循环引用。
- [ ] 评估透明/gradient 合成后的正文、次级文字、边框与 focus ring。现有 contrast gate 主要检查纯色 color token；无法解析的 material 合成色必须增加人工复核，而非把跳过当通过。
- **退出条件:** 六主题切换没有缺失变量或视觉断层，旧页面未迁移部分保持原样，试点 surface 可明确区分于 canvas 与 float。
- **状态:** pending — 工程侧已满足（material.css 14 角色/recipe 单向 alias、导入位钉在主题后消费者前、glaze= none/float blur=0px、theme-contract.spec 扩展锁定导入顺序/键集/alias/fallback/单向引用/关闭值），六主题无缺失变量由契约测证；视觉断层与 surface 可辨性留人工。

### 阶段 2:改造 AppShell 与标题栏 chrome

- [ ] 将 AppShell.vue 中 app-shell、app-sidebar、独立 app-toolbar、app-statusbar，以及 App.vue/WindowChrome.vue 的合并标题栏背景、分隔线和阴影逐步接到 material recipe。
- [ ] 先只替换 canvas/chrome 层级，再单独评审边框/阴影，最后才在亮色 chrome 小范围试验 haze；暗色主题使用自己的配方，不复用亮色 alpha。
- [ ] 保持独立 toolbar 48px、合并标题栏 40px、statusbar 高度、sidebar 宽度、窗口拖拽区、沉浸模式及 viewer route 覆盖层的几何与事件行为不变。
- [ ] 保持 gallery/viewer 主画布中性；gallery Canvas 继续使用 `--color-bg-canvas` / `--color-bg-canvas-gap`，不让 chrome 的 glaze、纹理或蓝色 haze 覆盖媒体区域。
- [ ] 复核 AppShell 现有的 immersive toolbar/statusbar 阴影：它们是临时浮出状态的层级反馈，不能被普通 chrome 阴影误合并。
- **退出条件:** sidebar、合并/独立 toolbar、statusbar 与内容区连续但可区分；窗口三键、拖拽区、沉浸模式和 viewer 往返无行为回归；没有新增大面积 blur；高密度 gallery 的媒体色彩不受污染。
- **状态:** pending — 工程侧已满足（AppShell 四区 + WindowChrome 合并标题栏接入 canvas/chrome 配方，仅改背景/纹理/边框、无 blur/shadow；新增 blur 仅 AppToolbar popover 与 UiDialog float 小面积浮层；--color-bg-canvas 未被动，--material-canvas 只服务宿主）；连续可辨与行为回归留真机人工。

### 阶段 3:验证 Settings 的大表面分组，并审慎收敛 sidebar

- [ ] 先选一个可由 harness 稳定到达、且不含危险操作的 Moonlight Settings section，试验“一个 surface 外壳 + 标题 + 分隔线 + 行 hover”。
- [ ] 只对试点 section 调整重复 card 外框；危险区、导入导出、外部账户及有独立状态承载需求的区域继续保留 elevated/card。
- [ ] 比较改造前后的定位效率、文字对比、折叠状态、focus ring、窄窗口换行和滚动体验；不以“卡片数量更少”本身作为通过标准。
- [ ] 在 Settings 结论成立后，再降低 sidebar 普通导航与纯入口型工具的容器感；保留 active、hover、focus、拖拽及工具对象状态的识别度。
- [ ] 保留 Xuan 的纸张纹理于 chrome，避免把纹理、glaze 与 selected 背景叠到局部文字上。
- **退出条件:** Settings 不再像同强度卡片墙，但分组、风险级别与可操作性更清楚；sidebar 的状态可见性未退化。
- **状态:** pending — ReaderSettingsSection 阅读区 surface 试点代码已落（单一外壳 + 标题 + 分隔线 + 行 hover）；分组观感/对比/窄窗换行留人工。

### 阶段 4:统一一个 toolbar popover 与一个 dialog 的 float recipe

- [ ] 选择 AppToolbar 的单个 UiPopover 消费者作为试点，优先选择可被 harness 复现的 filter/view popover，而非同时改全部菜单。
- [ ] 将试点 popover 与一个通用 dialog 接到 float surface、float shadow、边界高光/strong border 和可选小面积 blur recipe。
- [ ] 保持 UiPopover/UiDialog 的定位、焦点陷阱、ESC、点击外部关闭、内部滚动与键盘导航行为；本阶段只改材质，不改交互。
- [ ] 收敛未激活 chip 与普通 icon button 的常驻轮廓噪声，但保持搜索框、primary action、active 和 pressed 状态的可操作对比。
- [ ] 不把 MediaGrid、MediaThumb、SelectionToolbar 等已有局部媒体覆盖层顺手迁入；它们须在后续单独进行性能审查。
- **退出条件:** 两个浮层在六主题中的视觉语言一致、关闭与焦点行为无回归，且 blur 仅作用于小面积固定或暂时浮起区域。
- **状态:** pending — 工程侧已满足（AppToolbar 视图选项 popover + UiDialog floatSurface opt-in + ConfirmDialog 启用，均走 material float recipe，blur 经 --material-blur-float 默认 0px 关闭；UiDialog.spec 契约测含 floatSurface 修饰类；未改定位/焦点/ESC 逻辑）；六主题视觉一致与焦点/ESC 回归留真机人工。

### 阶段 5:主题矩阵、性能与工程验证

- [ ] 再次生成六主题矩阵，并补充 trial 中的 selected、hover/focus、empty/loading、toolbar popover、dialog 和至少一个窄窗口状态。
- [ ] 运行 npm run check:contrast、npm run lint、npm run typecheck、npm test、npm run build、npm run verify:channel；记录每条命令的结果。
- [ ] 跑现有 theme-contract.spec.ts，确保主题 CSS 文件与注册表一一对应、主题键集一致且无 token 漂移。
- [ ] 在真实 Windows WebView2 验证主题切换、滚动、窗口缩放、高 DPI、全屏/沉浸模式、媒体查看器往返与 focus 可见性。
- [ ] 对透明、`color-mix` 或渐变合成后的真实背景复核：正文/状态文本至少 4.5:1，辅助文本至少符合既有门槛且不低于 3:1，focus-visible 指示器相对相邻颜色至少 3:1。
- [ ] 覆盖标题栏合并/独立模式，以及 gallery Canvas 底色与宿主材质没有断层的场景。
- [ ] 使用高密度 harness 参数检查滚动体感；若出现掉帧、滚动条/文字对比下降或合成层扩大，先回退相关 blur/透明度而非继续调色掩盖。
- **退出条件:** 六主题无白黑硬编码断层，关键状态可识别，前端 CI 等价检查通过，人工 GUI 检查确认无性能与交互回归。
- **状态:** pending — 工程验证链路（check:contrast / lint / typecheck / test / build / verify:channel / theme-contract）2026-08-20 已复跑全绿；六主题矩阵重跑（含浮层/状态/窄窗口场景）与真机 WebView2 人工项留人工。

### 阶段 6:扩面或止步的评审门

- [ ] 汇总第一批的截图、人工检查、性能观察和门禁结果，明确每项验收是否通过。
- [ ] 仅在阶段 5 全部通过后，按小批次扩展到剩余 Settings、sidebar、更多 toolbar/float 与 viewer 固定控制区。
- [ ] 若任一关键门不通过，选择缩小 recipe、修正单主题参数或撤回试点；不得在未解决问题的情况下全量迁移。
- [ ] 通过后更新本计划的已完成阶段与决策；若实施完成，再按 planning skill 收口、蒸馏耐久决策并迁入 docs/worklogs。
- **退出条件:** 获得清楚的扩面授权，或形成有证据的止步/改向结论。
- **状态:** pending

## 验收总表

| 维度 | 必须满足的结果 |
|---|---|
| 层级 | canvas、chrome、surface、elevated、float 能被区分，且无重边框/重阴影堆叠 |
| 主题 | Moonlight 保持冷白蓝灰；Xuan 保留纸张身份；Ink、Obsidian、Dai 不出现亮色玻璃反光；Porcelain 不退化 |
| 可访问性 | active、selected、focus、disabled、error、loading、empty 可识别；透明合成后文字与 focus ring 仍有足够对比 |
| 性能 | 长列表、媒体网格和滚动父层没有默认 blur；真实 WebView2 未见新增滚动卡顿或媒体色彩污染 |
| 交互 | 搜索、filter、popover、dialog、ESC、点击外部关闭、拖拽区、全屏/沉浸模式及查看器往返行为保持 |
| 工程 | 相关前端门禁、主题契约、构建与主题截图矩阵通过；GUI 项清楚标为人工验收而非自动化通过 |

## 关键决策

| 决策 | 理由 | 候选 ID |
|---|---|---|
| 以既有主题截图捕获器作为视觉基线入口，不新增 pixel-diff 门禁 | 仓库已明确认为像素基线跨机不稳定；token 契约、对比度门和人工矩阵分别覆盖更适合的风险 | D-001 |
| material 先做 adapter，不重命名或删除旧 token | 六主题 token 契约严格，分层迁移能降低 scoped CSS 与主题切换回归风险 | D-002 |
| 第一批限定为 AppShell、一个 Settings section、一个 popover 和一个 dialog | 先验证材质层是否成立，避免把分组、几何和所有组件迁移混成不可归因的大改动 | D-003 |
| backdrop blur 仅留给小面积 float/chrome 试点 | 捕获器不能验证 GPU 性能，且设计方案明确禁止将其扩到媒体网格和滚动父层 | D-004 |

## 错误账

| 错误 | 尝试 | 解法 |
|---|---|---|
| 暂无 | — | — |
