# 任务计划：前端 UI/UX 深度审计与重构方案

## 目标
按用户批准的方案完成前端 UI/UX、配色、信息架构与操作逻辑重构，并通过自动门禁与真实 Tauri 视觉验收；accessibility 为非阻断次级目标。

## 当前阶段
**阶段 13（S7）token 引用闭环已收官（2026-07-15，三提交，真机 round9 验收通过）**：`--color-*` 定义面与消费面双向互等
（49=49）+ 双向硬门（消费⊆定义 / 定义⊆消费，均变异测试自证会红）；修 2 处用户可见 bug（大小徽章透明底致白字压照片
〔含 DOM/canvas 分叉〕、暗色主题粘性胶囊白条）+ 1 处真实性能问题（无人消费 meta 仍每屏拉 EXIF/GPS/路径）；
新建 AUDIO/DOC 类型角标。
**chunk 治理已收官（`ec7c378`）**：测绘推翻「治理=瘦身」——Rollup 喊的两个块全是误报（cpp 是懒加载语法、index 是路由
全懒加载后的外壳），而它掩盖的真风险是「告警不是门」（实测 pdfjs 静态进首屏仍 exit=0/CI 绿）→ 换成打包预算门的
两条可行动不变量（入口块预算 + 重依赖不得进入口），vitest 941。
**6 主题视觉矩阵已收官（`3620e26` 捕获器 + `5308ff8` 首个战果）**：此项此前挂「真机」标签，实测**大半可自动化**——
harness 加 `&theme=<id>` 维度 + headless Chrome 出 6 主题 × 3 场景 = 18 张（`npm run capture:themes`）。
**有意只做捕获、不做 pixel-diff 基线门**（红了唯一响应是重生成基线 = 幽灵门禁反模式，判据同 chunk 那轮）。
首查即得一处六主题全中的真 bug（网络存储卡经 Vue scoped 根节点双作用域规则盖掉全局卡外观，
而 token 契约/对比度/typecheck 三门全绿——**取值全合法，错的是取了哪个**）。
跨主题不变像素测量：settings **0.0% 恒定**（完全随主题）、gallery 恒定区全落在照片内、
viewer 92.3% 恒定 = **已声明的 S5 硬编码色豁免**（看图台永远黑）反向印证边界正如声明。
**S7 收官，阶段 13 complete。** 余真机盲区见 findings 会话续 27（viewer 控制条无指针不出镜、
原生窗口/WebView2/GPU canvas 路径）。

阶段 7/9/12 余项：S1 UiField 迁余 5 消费者（清点判定 defer，需真机/非 UiField 形态）+ UiCheckbox 迁余 5 checkbox 站点**已裁决 out-of-scope-by-design**（会话续13 源码级逐站点确认：各具独立 bespoke 视觉/契约〔HGalleryLab lab 工具·ReplacementPanel 裸控件/`.*` mono 字形·ReaderSettings 反序设置行·SettingsView 集合切换〕，非 remember-checkbox de-facto 形态，force 迁移须 bloat 原语损内聚，判定不迁；迁移终态 2/2）+ **UiPopover 原语已建成（会话续17，用户裁决 B=@floating-ui/vue 引擎，迁全库 3 处手写弹层）**、**UiToolbar 决策已定（会话续23，用户裁决=不建组件）**：三 toolbar 无共享外观类、复用逻辑早在 composable（useToolbarOverflow/useRovingTabindex），无摊薄价值；S1 落地为 SelectionActions 加 role=toolbar+可访问名，roving 只有 ContextualToolbar 一个干净候选（已 S3 交付），SelectionActions/AppToolbar 因含复合控件（ColorLabelPicker/select/滑块）不套 roving（详见 findings 会话续23）、S3 overflow 接入/Search/Reader-Audio（依赖真机或更大改动面）、S2-c2-3 view 路由化（需 folder 双模产品决策+真机）、S2-b2 search+view-pref URL（UiDialog 对话框 7/7、.btn-icon 34/34、UiField 建成+迁 4 消费者、**UiToggle 建成+迁 DynamicSettingControl**、**UiCheckbox 建成+迁 Confirm/CloseConfirm** 均已交付）
（S1：UiButton[e8a7e3e] → UiIconButton[8f4f249] → UiDialog[162f4b3]+迁 Confirm/CloseConfirm[98656e8 前] → useFocusTrap 通用化(挂载即开)+迁 FolderCreateDialog[8f78174] → bodyPadding 旋钮+迁 FolderTreeSelectorDialog[c7c02d4] → 迁 ExoticActivateDialog[6577d04] → #header 插槽+关闭控制 props+迁 OnboardingWizard[6cfbe9b] → maxHeight 可滚正文+迁 FaceApprovalPanel[f8d7673，对话框 7/7 收官] → .btn-icon 广域迁移第一批 6 文件 20 站点[5250087] → 第二批 ContentViewer 10 站点[377b73c] → defineExpose 收 AppToolbar[937ff03，.btn-icon 34/34 收官] → UiField 原语[dba6833] → 迁 UiField 第一批 3 消费者[b39b57b] → NetworkStorage[01278ec] → UiToggle 原语+迁 DynamicSettingControl[ec8e964，同构包裹 .toggle，注册表驱动覆盖全部 control:'toggle'] → UiCheckbox 原语+迁 Confirm/CloseConfirm[94a8c35，蒸馏 .remember-checkbox 并 dedup，同构] → UiSelect 原语+迁 DynamicSettingControl[2753777，用户裁决继续3，:deep() 重锚 compact 后代规则跨子组件边界]；S3：roving tabindex[8f4f249]；S2 拆段：S2-a[cc4bfb7] → S2-b[3719be1] → S2-c-fix[71c32ed] → S2-c1[e54a73f] → 余 S2-c2-3 / S2-b2）

## 各阶段

### 阶段 1：仓库与产品界面盘点
- [x] 识别前端技术栈、入口、路由、页面和共享组件
- [x] 梳理现有主题 token、布局体系与交互状态
- [x] 记录当前工作树与约束
- **状态：** complete

### 阶段 2：源码规范与 UX 审计
- [x] 按最新 Web Interface Guidelines 检查源码
- [x] 审计信息架构、视觉层级、配色、状态反馈和操作闭环
- [x] 形成带源码位置和优先级的发现清单
- **状态：** complete

### 阶段 3：真实页面与关键流程验证
- [x] 启动或连接本地应用并检查浏览器承载边界
- [x] 记录普通浏览器无法替代 Tauri 真机验证的边界
- [x] 明确需要用户补充的截图清单并已提出请求
- **状态：** complete（真机视觉校准转为用户确认后的 S0 前置条件）

### 阶段 4：重构方案设计
- [x] 定义设计原则、视觉方向与 Design Token
- [x] 规划导航、布局、组件和操作逻辑重构
- [x] 划分实施阶段、文件范围、验收标准与风险
- **状态：** complete

### 阶段 5：方案交付
- [x] 复核方案与证据的一致性
- [x] 输出待用户确认的方案及需要确认的关键选择
- [x] 明确本轮未修改业务代码及验证边界
- **状态：** complete

### 阶段 6：S0 视觉基线与 browser UI harness
- [x] 收集 Gallery 宽窗/窄窗、Settings、Viewer 截图
- [x] 回写用户最终裁决和截图发现
- [x] 建立 Tauri platform/IPC browser adapter
- [x] 建立可重复渲染的 Gallery/Settings/Viewer UI harness 与视觉场景
- **状态：** complete

### 阶段 7：S1 UI primitives 与机械规范治理
- [~] 建立最小 Button/Field/Popover/Dialog/Toolbar primitives
  - [x] **UiButton（e8a7e3e）**：variant/loading/disabled/type 契约，包裹全局 .btn 体系；SettingsView 5 按钮
    迁入 + 删 scoped .btn* 副本。**并确立项目首个 SFC contract 测试范式**：vitest 加 @vitejs/plugin-vue +
    @vue/server-renderer SSR 断言（无 @vue/test-utils），11 例。地基性副产品，后续原语/组件测试复用。
  - [x] **UiIconButton（8f4f249）**：包裹全局 .btn-icon（9 文件 38 处）。`label` 提为必填 prop 从类型层
    杜绝无名图标按钮；`active`→.active 视觉、`toggle` 单独门控 aria-pressed（仅开关型）、`title` 缺省回落
    label 可覆盖。11 例 SSR contract 测试。已接入 ContextualToolbar（S3 主角）。
  - [x] **UiDialog[162f4b3]**：包裹全库模态范式(此前 7 个对话框各自逐字重复 ~120 行外壳:
    overlay/content/header/title/关闭键/body/footer + 两个 keyframes)。原语吃下 Teleport + 焦点陷阱接线
    (复用既有 useFocusTrap,overlay ref 由原语持,`data-autofocus` 初始焦点落消费方插槽由 querySelector
    跨插槽命中) + Escape/点遮罩关闭 + role/aria-modal/aria-labelledby(titleId)/aria-describedby 语义;
    props: open/title/titleId/describedById/closeLabel/showClose/maxWidth**/bodyPadding[c7c02d4]/maxHeight[f8d7673]/closeOnOverlay/closeOnEsc[6cfbe9b]**,
    slots: default(body)/footer**/header[6cfbe9b,整体覆盖默认标题行]**。16 例 SSR contract(用 renderToString 二参 context.teleports 取回 Teleport 产物再断言)。
  - [x] **迁 7/7 对话框到 UiDialog（全库模态外壳全摊薄收官）**：ConfirmDialog + CloseConfirmDialog[162f4b3,净删 244 行,修 CloseConfirm
    漏 Teleport 隐患] → **FolderCreateDialog[8f78174]** → **FolderTreeSelectorDialog[c7c02d4]**(净删外壳,
    树列表满幅走 bodyPadding='0',500px 走 max-width,分栏页脚走 #footer + width:100% footer-split,
    嵌套 FolderCreateDialog 提为同级根各自 Teleport) → **ExoticActivateDialog[6577d04]**(范式①常驻+open 翻转,
    净删 109 行外壳;textarea 标 data-autofocus 交焦点陷阱;三关闭路径收敛 @close=onCancel 保 gate.activating
    在途禁关守卫;顺带修此前无 Tab 焦点陷阱缺陷) → **OnboardingWizard[6cfbe9b]**(范式② v-if 挂载,自定义头部
    〔副标题+步骤点〕走新 #header 插槽,3 步正文 body-padding='0' 交 .onboarding-body 自持定高居中,分栏页脚
    footer-split;禁遮罩/Escape 关闭走 close-on-overlay/esc=false;按钮迁 UiButton 顺带清违「交互文本禁用 tertiary」
    的 skip 色) → **FaceApprovalPanel[f8d7673,7/7 收官]**(范式② v-if,自定义头〔标题+副标题+X〕走 #header,
    分组列表满高可滚走新 maxHeight='84vh'→dialog-content--capped 类〔正文 flex 撑满可滚、头 flex-shrink:0 固定〕,
    无 footer;新增 Escape 关闭)。**两处原语加法扩展**:#header 插槽〔带 fallback,消费方整体覆盖默认标题行〕+
    close-on-overlay/close-on-esc〔默认 true〕[6cfbe9b] + maxHeight〔条件类 --capped 隔离,其余 6 对话框布局零变化〕[f8d7673]。
    **关键地基:useFocusTrap 通用化[8f78174]**——watch
    加 immediate(支持父 v-if「挂载即开」范式,此前只支持「恒挂载+open 翻转」) + onBeforeUnmount 直调 release
    (挂载即开在 open 态被卸载时归还焦点) + engage 加 SSR 守卫(immediate 会在 SSR setup 触发,守 document)。
    这批迁移顺带修 FolderCreate/FolderTreeSelector **此前漏 Teleport + 无焦点陷阱**(Tab 会逃逸)两真实缺陷。
    焦点陷阱/Escape/点遮罩/动画/Teleport 分层属运行期行为,结构/aria/bodyPadding 已 SSR 门禁验+标注真机验收。
  - [~] **UiField 原语已建 + 迁 4 消费者**（用户裁决：双关联模式并存 + 全量迁）。**UiField[dba6833]**=label+单控件
    统一外壳，associate='nest'（包裹 label 隐式关联）/'for'（label[for]+控件[id] 显式，useId 生成）双模、
    orientation stacked/inline、hint/error（置 label 外经 aria-describedby 关联）、labelHidden sr-only，8 例 SSR 契约。
    与 SettingRow 正交。迁移（B1[b39b57b]：FolderCreate/Proofread/SemanticSearch，含 2 处真 a11y 修复；NS[01278ec]：
    NetworkStorage canonical dedup）。**余 5 消费者经清点判定 defer（非同构变更，需真机或非 UiField 形态，见 findings 会话续8）**：
    DocumentViewer（工具栏 select 字号密度 + 紧凑编辑栏 flex 重构）、ReaderSettings（rs-row space-between 布局 +
    stepper/segmented 非原生控件）、HGalleryLab（lab 工具 + range-width 重锚 + checkbox 出范围）、ReplacementPanel
    （紧凑行非字段）、SettingsView（checkbox=独立部件、search=labelHidden 低值）。checkbox/toggle/stepper/segmented
    宜作独立 UiCheckbox/UiToggle 原语（**UiToggle 已建成并迁 DynamicSettingControl，见下项**）。**UiPopover 已建成（会话续17，@floating-ui 引擎，见阶段11）**，UiToolbar 决策已定=不建组件（会话续23，见阶段7 状态）。
  - [x] **UiToggle 原语 + 迁 DynamicSettingControl**（ec8e964）：同构包裹全局 .toggle/.toggle__thumb（选中/滑动/焦点态纯 CSS，
    构造即视觉等价），单一 v-model API + 可选 label→aria-label（不传保持 parity）。迁注册表驱动的 DynamicSettingControl 一处
    覆盖全部 control:'toggle' 设置项；.compact-toggle 作用域变体经 class fallthrough 合并根 label 仍命中。7 例 SSR 契约。
  - [x] **UiSelect 原语 + 迁 DynamicSettingControl**（2753777，用户裁决「继续 3」推进此前停下上报的边界项）：同构包裹全局
    `.select`/`.select-wrap`（含 ::after 箭头），选项走默认插槽由消费方渲染（i18n 内联保留、原语不依赖 i18n keys），v-model 经
    writable computed 走 Vue 规范 vModelSelect 指令，可选 disabled/ariaLabel 不传保持 parity。全库 `.select` 唯一消费者=注册表驱动的
    DynamicSettingControl 一处覆盖全部 control:'select'。**跨越首个「父 scoped 后代选择器伸进子组件」边界**：`.compact-select-wrap .select`
    因 select 移入 UiSelect（无 scoped→无 data-v）而内层 data-v 失配 → 改 `.compact-select-wrap :deep(.select)` 穿透边界重锚。
    等价性可推理证明：`.compact-select-wrap` 类经 fallthrough 落 UiSelect 根仍带父 data-v 作锚，`:deep` 去 select 侧 data-v 约束，
    同锚同靶同声明。7 例 SSR 契约。**诚实边界：紧凑面板 select 实际渲染尺寸无门禁覆盖（SSR 断结构不断 computed style），需真机做最终视觉确认——推理级等价非盲改（见 findings 会话续12）。**
  - [~] **UiCheckbox 原语 + 迁 Confirm/CloseConfirm**（94a8c35，用户裁决「UiCheckbox 先定视觉」）：全库无全局 checkbox 组件类，
    **蒸馏既有 de-facto 视觉**——ConfirmDialog/CloseConfirmDialog 两处重复的 `.remember-checkbox`（native input 16×16 +
    `accent-color:var(--color-accent)` + flex 行 gap 8px，主题感知、原生 a11y、零自绘 SVG）。单一 v-model + label/插槽 + disabled。
    **样式作用域私有 + 类名 `.checkbox-field`**（避开已被 MediaThumb 网格选择控件占用的 `.checkbox`，防全局层叠泄漏污染）。
    迁 Confirm/CloseConfirm=字节复刻使二者同构（可无真机）+ dedup 两处重复 CSS；CloseConfirm 的 margin-top 经 class fallthrough
    （`.close-remember-mt`）保留。7 例 SSR 契约。对比度门禁过（accent×bg 4.69≥3）。**余 5 处 checkbox 站点经源码逐个复核（会话续11），
    阻塞分两类而非笼统「需真机」**：① 仅 HGalleryLab(48/66) 契约吻合（`<label><input v-model>text</label>` 尾随标签），纯视觉非同构（蓝→accent）
    可真机解，但属 dev/lab 视图低价值；② SettingsView(88，多选 `:checked=includes()`+`@click($event,val)` 集合切换)/ReplacementPanel(28，flex 行内裸控件+`:title`
    无标签 / 48·70，`.repl-row__re` 含 `.*` 字形+`:title`)/ReaderSettings(317，`.rs-row--toggle` 标签左·控件右反序设置行) 均**契约/结构不符**，
    真机解不了。**会话续13 裁决（out-of-scope-by-design）**：这 5 处各具独立 bespoke 视觉/契约（裸控件 / `.*` mono 字形 / 反序设置行 / 集合切换），均非 UiCheckbox 蒸馏的 `.remember-checkbox` de-facto 形态；
    force 并入须给原语加 `:checked` 模式 / `@change`·`@click`·`:title` 透传 / 反序布局，会 bloat API 并毁掉原语视觉内聚（违高内聚/可读，属为 DRY 而 DRY）。故判定**不迁、保持 bespoke**——UiCheckbox 迁移终态 = Confirm/CloseConfirm 2/2（真实
    de-facto twins 已收）。HGalleryLab 虽契约吻合，但 `.hlab__check` gap:4px 与 `.checkbox-field` gap:8px 经 class fallthrough 冲突（源序不定不可靠）+ dev/lab 低值，一并排除。**修正前记「4 默认蓝非同构需真机」的笼统口径。**
  - [~] 广域迁移 .btn-icon → UiIconButton：**7/8 文件 30/34 站点已迁**（第一批 6 无 scoped 覆盖文件 20 站点
    [5250087]：SidebarFooter/ManagementSection/DynamicSettingControl/GalleryViewControls/FoldersSection/ToolsSection；
    第二批 ContentViewer 10 站点暗色浮层 [377b73c]）。视觉零风险机理=UiIconButton 根仍挂全局 .btn-icon + Vue 3
    子根带父作用域 data-v，故 `.detail-controls .btn-icon` 等父级后代覆盖迁移后仍命中。**余 AppToolbar 3 站点留待**：
    H-Lab(1)可迁，但 filter/view(2)带 `ref=filterBtnRef/viewBtnRef` 模板 ref 做弹层锚定——迁 UiIconButton 后 ref 指
    组件实例非 DOM，须先给 UiIconButton 加 defineExpose 暴露根元素（原语改动，见 findings 会话续7）。.btn(非 icon)
    消费者（UiButton 广域）另计。
  - [x] 对话框迁 UiDialog **7/7 全摊薄收官**（含向 UiDialog 加 #header 插槽 + close-on-overlay/esc + maxHeight 三处加法扩展）
- [x] 清理 transition:all（20 处清零，74767eb；.btn-secondary/.btn-danger 幽灵类已全局化 c98ac93→UiButton 收敛已启）
- [ ] 保证现有 accessibility 不明显回退（非阻断）
- **状态：** in_progress（UiButton/UiIconButton/UiDialog 三原语 + SFC 测试范式已交付；**UiDialog 已迁 7/7 对话框全摊薄收官**
  [Confirm/CloseConfirm/FolderCreate/FolderTreeSelector/ExoticActivate/Onboarding/FaceApproval]，useFocusTrap 已通用化支持「挂载即开」范式，
  UiDialog 累计能力 = #header 插槽/close-on-overlay·esc/maxHeight(--capped 可滚正文)/bodyPadding/maxWidth；
  **.btn-icon → UiIconButton 广域迁移 8/8 文件 34/34 站点全收官**[5250087+377b73c+937ff03，AppToolbar 经 UiIconButton defineExpose({el}) 收尾]；
  **UiField 原语建成[dba6833]+迁 4/9 消费者**[b39b57b+01278ec：FolderCreate/Proofread/SemanticSearch/NetworkStorage，含 2 真 a11y 修复]，
  余 5 消费者清点判定 defer（非同构变更需真机/非 UiField 形态）；**UiToggle 原语建成[ec8e964]+迁 DynamicSettingControl**（同构，注册表驱动覆盖全部 control:'toggle'）；
  **UiCheckbox 原语建成[94a8c35]+迁 Confirm/CloseConfirm**（蒸馏 .remember-checkbox 并 dedup，同构；余 5 站点会话续13 裁决 out-of-scope-by-design：各具独立 bespoke 视觉/契约非 remember-checkbox 形态，不 bloat 原语，迁移终态 2/2）；
  **UiSelect 原语建成[2753777]+迁 DynamicSettingControl**（同构包裹 .select，注册表驱动覆盖全部 control:'select'；用户裁决继续3 跨「父 scoped 后代选择器伸进子组件」边界，`:deep()` 重锚 compact 规则，推理级等价待真机确认尺寸）；**UiPopover 原语建成[4b02272/7c7289f，@floating-ui/vue 引擎，用户裁决 B]+迁全库 3 处手写弹层**(date/filter ⋯/view ⋯)；**UiToolbar 决策已定=不建组件（会话续23）**：无共享外观类→无摊薄价值，复用停在 composable；S1 落地=SelectionActions 加 role=toolbar+可访问名（有语义无 roving 有意，因含 ColorLabelPicker 复合控件+测量契约撞 roving 成员契约），roving 仅 ContextualToolbar 干净候选已 S3 交付，SelectionActions/AppToolbar roving 标为需新基建+真机键盘验后续项）

### 阶段 8：S2 Route/ViewDescriptor 单源
- [x] Settings route 化
- [x] Collection/Person route 化
- [x] **S2-a 搜索 query 单源门面**（cc4bfb7）：新增 searchStore 协调门面（读表面 mode/scope/
  committedQuery + 写表面 apply/clear/commit*），draftMixedQuery 提升出 AppToolbar，删除 emit/
  onSearch 冗余双写；21 例单测。这是 S2-b 的 URL 读写锚点。
- [x] **S2-b Gallery filter ↔ URL 双向同步**（3719be1）：filter 维度（types/favorite/live/rating/
  color/from&to）编解码 + echo-guard 同步层。utils/galleryQuery.ts 纯编解码+isGalleryRoute（防御式
  解析：白名单/clamp/垃圾回落默认）+ useGalleryQuerySync composable（store→URL 连续写、URL→store
  仅初次水合、值相等守卫+hydrated 门断回环）。真机 harness 验深链恢复+无冲掉+无死循环。同步模型=
  filter 全局态在当前路径的投影（非逐历史态）。
- [~] **S2-b2 search + view-pref URL 同步 🟢 会话续23 已施工（Stage 3a/3b）⏸真机**：
  🔴 **下方「2026-07-12 裁决：延后、保持 sticky-global 现状不动」已被 2026-07-13 用户改判推翻**——裁决=
  **view-pref（group/sort/order/layout）+ search（q/scope/mode）全做进 URL**。施工:新建 `utils/viewPrefQuery.ts`
  (Stage 3a,+9 测)与 `utils/searchUrl.ts`(Stage 3b,+10 测)纯函数编解码 + useGalleryQuerySync 扩三维度。
  竞态修:hydration 门 `Promise.all([isReady, startupConfigPromise])`+nextTick(persist 先落、URL 再覆盖出现的
  键、不赌 .then 顺序);镜像 persist=false(isSemanticMode 时删 group/sort,防污染 previousGroupBy);search 恢复
  顺序 filter→view-pref→search(previousGroupBy 捕获正确)。门禁 typecheck/lint/vitest 839/build 全绿(**仅本地**)。
  提交 `724dcf2`(3a)+ 3b。真机验收见 `realtest-round8`。
  🟡 **诚实边界(view-pref 语义)**:本轮落地=**「全局携带 + URL 恢复/覆盖持久」**(view-pref 像 filter 全局携带、
  深链恢复、URL 覆盖 persist),完整兑现「URL 权威覆盖持久值」;**未做「纯 per-view 逐视图独立」**(每视图各记各的
  group/sort)——那需 read-on-navigation 模型 + 工具栏改动不落全局 persist(改 toolbar 持久契约),属 UX 契约变更、
  待真机后定(不冻结契约,见 findings 会话续23「per-view 两种强度」)。
  （历史:~~用户 2026-07-12 裁决 group/sort/layout 采 per-view 语义但延后、保持 sticky-global~~ 已推翻。）
- [ ] **S2-c view 维度归一 + Sidebar descriptor 单源**（测绘后重估：视图路由化比设计 §5.1 假设复杂得多，
  拆为三子步，按风险排序）：
  - **S2-c-fix ✅（71c32ed）**：App.vue 视图 watcher `fullPath`→`path`，修 S2-b 引入的「收藏夹/人物视图
    切筛选清选区+重水合」回归（根因=view 维度只依赖 path 却监听 fullPath）。
  - **S2-c1 descriptor 统一 ✅（e54a73f）**：新增 utils/resolveView.ts 纯函数，把两投影重复的
    precedence（person>collection>smart-album>directory）+ 映射决策收敛为 ResolvedView 单源；
    useJustifiedLayout/useViewDescriptor 的 if/else 链 → resolveView + 机械 switch（directoryId param/
    searchQuery/aiSearch/sort 不动）。🔴 R1-2 双维护消除。resolveView.spec 13 例穷举决策矩阵+优先级。
    门禁 670 测试全绿。
  - **S2-c2 smart-album 路由化 + S2-c3 folder 路由化 🟢 已施工（会话续23，用户裁决=分模全量）**，⏸ 真机验收：
    🟢 **decisions/ 已记：用户 2026-07-13 裁决「分模全量」**——5 个 smart-album 各占独立路径（`/`=all、
    `/favorites`、`/trash`、`/live-photos`、`/recent`）+ folder 筛选态（模式B，groupBy≠folder）→ `/folder/:id`；
    **folder 滚动锚点态（模式A，groupBy=folder 点文件夹设 `pendingScrollDirId`）保持 `/` 不进 URL**——它是全库
    单列表内的滚动位置而非视图，进 URL 会「每滚一下改一次 history」。**下方原「需产品决策+真机、不在无 GUI 仓促
    推进」的判断已被此裁决推翻**（folder 双模有干净切割线：模式A 永远导航 `/`、模式B 才去 `/folder/:id`，两模
    天然不撞；getViewKey 模式B 已 `dir-<id>` 分桶，scrollCache/KeepAlive 零改造）。施工：新建 `utils/viewRoute.ts`
    纯函数（smartAlbumToPath/folderToPath/routeToView/isPrimaryGalleryRoute，7 测）+ router 补 `/live-photos`
    `/recent` 壳 + galleryQuery.isGalleryRoute 补两条 + App.vue watcher 加回填分支（完备相等守卫）+ SemanticSearchPanel
    v-show 平移 isPrimaryGalleryRoute + LibrarySection/FoldersSection 导航改 push 视图路径（FoldersSection 抽
    `navigateToFolder` 统一三站点，杜绝 addRoot/move 后 push('/') 被回填清 directory 的 clobber）。门禁 typecheck/
    lint/vitest 820/build 全绿（**仅本地**）。真机盲区：深链恢复 + 刷新 + 前进后退 + 模式A 锚点不进 URL + 切视图
    不误清选区 + scroll/KeepAlive 无回归（realtest 清单）。
- **状态：** in_progress（S2-a/S2-b/S2-c-fix/S2-c1/**S2-c2-3**/**S2-b2 均已施工⏸真机** done；S2-b2 落地为「全局携带+URL 覆盖持久」,纯 per-view 逐视图独立待真机后定）

### 阶段 9：S3 Shell/Toolbar ownership
- [x] 标题栏与 Gallery 页面 toolbar 分层
- [x] **roving tabindex（8f4f249）**：新增 useRovingTabindex（纯核 nextRovingIndex 跳过 disabled/端点
  环绕/朝向门控，14 例穷举单测 + 薄 composable）；接入 ContextualToolbar——全组只占一个 Tab 停靠位，
  ArrowLeft/Right/Home/End 组内移焦。refresh() 校正停靠位避开 disabled 项（harness 实证：撤销/重做
  disabled tabindex=-1、全屏 tabindex=0）。
- [~] Search 稳宽槽位/combobox contract（AppToolbar）：
  - **combobox contract ✅（452f808）**：混合搜索下拉早已是交互 combobox（输入驱动出现 / ArrowUp-Down 导航 /
    Enter 选择 / Esc 关闭）却无任何 ARIA 接线,读屏听不到候选数与当前高亮项。补齐 ARIA 1.2 combobox（input +
    listbox）:抽 **resolveSearchCombobox 纯决策源**（utils/searchCombobox.ts;mode×dropdownOpen×activeIndex →
    输入框 ARIA 属性集,7 例穷举 spec）+ **searchOptionId**（listbox↔activedescendant 字节对指契约）。**语义边界=
    仅 mixed 模式生效**(normal 实时过滤 / semantic 防抖提交皆无工具栏内建议列表 → 保持原生 searchbox;据 mode
    门控防「切出 mixed 后 stale open 泄漏 combobox 语义」);aria-controls / activedescendant 随下拉 open 门控
    (下拉是 v-if,引用不存在 id 属无效 ARIA)。input 接 role=combobox / aria-expanded / controls /
    autocomplete=list / activedescendant + aria-label（占位符兜可访问名,顺带 DRY 收敛重复三目）;下拉接
    role=listbox + aria-label（新 i18n `toolbar.searchSuggestions` 中英平衡）、项接 role=option + aria-selected;
    listboxId 走 **useId()** 保 SSR 稳定 / 跨实例唯一。门禁 typecheck/lint/vitest 909/build 3.11s 全绿(本地非 CI)。
    **读屏播报（NVDA/VoiceOver aria-activedescendant 跟随）属真机盲区,见 realtest §E**。
  - **稳宽槽位 ✅（既有 .toolbar__search-wrap 固定 clamp(220,26vw,320) 宽已满足,本轮核验无回归）**：搜索槽宽与
    模式内容无关（scope select / spinner 皆在固定宽壳内 flex 收缩）、仅随窗口单调 → 不非单调扰动中部 foldable
    可用宽,天然不是折叠振荡源;combobox 改动为纯 ARIA、零布局变更。
- [ ] 统一 overflow（基建 useToolbarOverflow 已就绪；ContextualToolbar 溢出接入待命令增多,触发条件未到）
- [x] 修正 Viewer 顶栏命令分组
- [x] 删除 Viewer 底部残余重复命令
- [ ] Reader/Audio 后续收敛（需真机）
- **状态：** in_progress（分层 + roving tabindex + **Search combobox/稳宽槽位（452f808）** done;余 overflow 接入
  〔待命令增多〕/ Reader-Audio〔真机〕/ combobox 读屏真机验收 依赖真机或触发条件）

### 阶段 10：S4 Settings 信息架构
- [x] 分区导航、设置搜索、advanced 分区
- [x] danger zone（已核对真实归属：settingsMap debug 段的清库/重置设置/清缩略图/清日志四项移入独立 danger 分区，默认折叠+confirm 契约；清除缓存实为无损重载降级普通按钮；AI restart/忘记卷等对象级操作按裁决留原业务语境。c98ac93）
- [x] Moonlight/Ink 默认组合
- **状态：** complete

### 阶段 11：S5 Gallery/Collections/Persons
- [~] Gallery filter/selection/empty-state 重构（**empty-state 已收官**;filter/selection 依赖决策/真机,见下）：
  - **empty-state ✅（e0a85ca）**：抽 **UiEmptyState 原语**（icon/title/description + primary/secondary 动作,
    同构包裹全局 `.empty-state*`,5 例 SSR 契约）+ **resolveGalleryEmptyState 纯决策源**（消息 key + 动作枚举,
    10 例穷举 spec,precedence: search > filtered > directory > smartAlbum）,迁 MediaGrid 内联空状态（收敛此前
    藏在 2290 行组件里的 emptyStateText/showEmptyAction 双 computed）。**修真实错 CTA**（设计 §7.1「所有 empty
    state 都含下一步」）:全局筛选态激活却零结果时,此前 precedence 落「空库·添加文件夹」误引导加目录——新增
    **filtered-empty 分支置于视图维度之上**（filter 是全局持久态,切视图不清极易留 stale filter 静默藏空,
    「清除筛选」是通用逃生口）+ clear-filters 动作（复用既有 `toolbar.clearFilters` 不新增重复键）。顺带消
    **\n-split 脆弱约定**（allPhotos/favorites 的「标题\n说明」拆为独立 i18n `*Title`/`*Desc` 键,原语走
    title/description 双 prop）;全局 `.empty-state__action` 死类替换为 `.empty-state__actions` 横向容器。
    门禁 typecheck/lint/vitest 769（61 文件,含 localeIntegrity 中英平衡 + 两新 spec）/build 2.82s 全绿;
    filtered-empty 文案/FilterX 图标/清除筛选交互属门禁盲区,真机验收见 `realtest-round4-2026-07-13.md`。
  - **filter a11y/popover（🟡 定位前置已解锁,剩 chip aria-pressed 语义 + 真机）**：**UiPopover 原语已建成**
    （会话续17,用户裁决 **B=@floating-ui/vue 引擎**——委托 flip/shift/autoUpdate 定位数学,原语持有 Teleport 逃逸
    overflow 裁切 / useFocusTrap 焦点陷阱 / backdrop·Esc dismiss 契约;非同构包裹〔无 .popover 全局类〕,表面视觉
    留消费方 slot;6 例 SSR 契约,useFloating 验证 node/SSR 安全)。全库 **3 处手写弹层（date + filter ⋯ + view ⋯）
    已全部迁入**[4b02272 date/7c7289f filter·view],positionMenu 式手算定位归零、**date 弹层漏水平钳制越界 bug 由
    shift 中间件顺带修复**。**「UiPopover 无共享全局类=需发明定位抽象」的决策已消解**（选 @floating-ui 而非自研,
    理由=flip/shift/autoUpdate 属被生态固化标准件、桌面 app 字节非约束、Search combobox 前置所需;引擎藏原语 API 后可逆）。
    **语义层已收口（会话续17 续）**：4 切换 chip（图片/视频/Live/收藏）加 `:aria-pressed`、3 disclosure 钮
    （date `aria-haspopup="dialog"` + filter/view ⋯ `aria-haspopup="true"`，均 `:aria-expanded` 绑开阖态；⋯ 钮经
    UiIconButton 单根 fallthrough）——声明式镜像既有 active 条件，评分/颜色 chip 是 StarRating/ColorLabelPicker
    容器 div 各自持语义故不加。**UiToggleChip 判定不需**（inline aria-pressed 已足，另建原语为 4 个同构 chip 属为
    DRY 而 DRY）。剩余=定位视觉/flip/焦点陷阱/**嵌套弹层（filter 菜单内 date 弹层 backdrop z-index 层叠）** +
    **读屏播报**（aria-pressed/expanded 真机 NVDA/VoiceOver）真机验收（见 realtest-round5 §E）。
  - **selection bar 改造（🟢 C1–C5 已施工落地,待真机验收 realtest-round6）**：~~设计 §7.1「默认不可拖、
    拖拽降级高级选项」~~ 已被 **2026-07-13 用户裁决覆盖**——可拖拽是解决「胶囊遮挡内容」的核心能力,**保留**。
    施工蓝本单一真源 = **`docs/designs/2026-07-13-SelectionBar合并分离一键切换方案.md`**,按 C1–C5 分提交推进:
    - **C1（9b893e5）动作数据驱动化**:SelectionToolbar 9-emit → 单一 `:commands`(SelectionCommand 类型,
      MediaGrid 组装,handler 零迁移);新增 SelectionToolbar SSR 契约 spec。
    - **C2（0156db8）收窄折叠**:抽 `SelectionActions.vue`(自持 useToolbarOverflow,双实例各绑自己容器);
      宽度约束链(胶囊 max-width + flow 容器 flex:0 1 auto);测量帧 visibility:hidden(不裁条外 tooltip);
      ⋯ 溢出菜单消费 UiPopover(top-end 上弹)+ disclosure 语义(aria-haspopup/expanded)。
    - **C3（3974dfc）docked 形态 + 一键切换**:`useSelectionBarMode`(照 useTitlebarMode,默认分离);
      **docked 用 Teleport 实现**(9 个 handler 原地留 MediaGrid,推翻会话续16「须先抽 useBatchActions」);
      AppStatusBar 恒存在 outlet + **替换式**info 让位(28px 恒高);两框架边界守卫(isSelectionMode gate +
      **hostActive** 共享信号——对初版「两单例让位」的加固,防选区残留进查看器时 outlet 空白);条上切换钮 +
      settingsMap/binding/i18n(注册表驱动一处接入)。
    - **C4（7cac029）拖拽位置持久化**:offset 走 useSelectionBarMode(localStorage);删「退选区即复位」watch
      (有意行为变更);clampOffset 三时机(恢复/拖终/resize),防小窗口条失踪。
    - **C5 docs 回写**(本条 + 设计文档进度 + progress/findings + realtest-round6)。
    已批三决策=默认分离/替换式共存/9-emit→commands prop 全落地;`bottom:32px` 经测绘证实为内容区内审美间距
    (状态栏在定位上下文外,物理不重叠)。门禁本地全绿(typecheck/lint/vitest 801/build);真机盲区(折叠视觉/
    docked 28px/Teleport+KeepAlive/拖拽 clamp/读屏)见 **realtest-round6-2026-07-13-selectionbar**。
- [x] Collections/Persons route 与卡片操作 contract
- [x] Persons 隐藏管理与即时 undo
- [x] **Collections 删除 undo、ignored 历史管理**（两子缺口分两 commit）：
  - **Collections 删除 undo（73b698a）**：删除此前是硬删除（`DELETE FROM albums` + `album_items`
    级联），一次确认即永久丢失。核心=**undo 能力是数据模型属性,硬删除无可撤销对象**→ 后端升级软
    删除:V18 迁移 `albums.deleted_at`(NULL=未删)、`delete_collection` 改 `UPDATE SET deleted_at`
    (仅 kind='user' + deleted_at IS NULL 守卫)、新增 `restore_collection` 清零、list/recent 过滤
    `deleted_at IS NULL`。前端 CollectionsView 删除后 5 秒 undo toast → `store.restore`（镜像 Persons
    隐藏/忽略范式,成员原样保留）。单测 soft_delete_and_restore_collection；v17 重放测试修（回拨版本
    须撤销 V18 非幂等 ALTER 的 deleted_at 列）。
  - **Persons ignored 历史管理（a5dc9ce）**：经查「ignored 历史管理」实指 **Persons `is_ignored` 误检
    桶**（标记误检后仅 5 秒 undo,之后无处查看/恢复,缺口 §2.6),非 collections。后端 `list_persons`
    重构抽共享体 `list_persons_by_ignored(conn, model, ignored)`,新增 `list_ignored_persons` +
    `list_ignored_face_persons` 命令。前端 PersonsView **三态展示（墙/隐藏/误检桶,两切换互斥）**：
    误检桶来源不对称（hidden 在 persons 前端过滤,ignored 服务端不返回须独立列表 `ignoredPersons`),
    `setIgnored` 两列表间乐观搬移免二次拉取;头部「已忽略 (N)」入口,桶内卡片只留「移出误检桶」恢复键。
- **状态：** in_progress（**empty-state 重构已收官[e0a85ca]** + **UiPopover 原语建成[4b02272/7c7289f]**：UiEmptyState +
  resolveGalleryEmptyState 纯决策源 + 修 filtered-empty 错 CTA;UiPopover(@floating-ui 引擎,用户裁决 B)+ 迁全库 3 处
  手写弹层(date/filter ⋯/view ⋯)、positionMenu 归零、顺带修 date 弹层越界 bug;余 filter a11y 的 chip aria-pressed
  语义+真机、selection bar 改造〔产品决策+真机〕。Collections/Persons 卡片操作与隐藏/忽略/删除全 undo 闭环已收官,均待真机 GUI 验收）

### 阶段 12：S6 Viewer/Reader/Audio
- [x] Viewer 默认折叠 Sidebar，并提供显式恢复入口
- [ ] Viewer 控制面最终收敛
- [ ] Reader/Audio 窄窗与局部控制面收敛
- **状态：** in_progress

### 阶段 13：S7 主题视觉与性能收尾
- [x] 6 主题视觉矩阵与硬编码色治理：**token 引用闭环已收官（三提交 6c778cd/d499dce/1f3ddcc，
  真机 round9 于 2026-07-15 验收通过）**；**视觉矩阵已收官（`3620e26`+`5308ff8`）**。
  - **矩阵落地为捕获器而非人工截图**（`scripts/capture-theme-matrix.mjs`，`npm run capture:themes`）：
    harness 加 `&theme=<id>` 维度，headless Chrome 出 6 主题 × 3 场景 = 18 张，无 Playwright
    （沿用本仓既有先例）。主题经 **startupConfig 覆盖而非直写 `data-theme`**——后者会让截图为
    产品里不存在的路径背书；主题 id 取自 `themes/*.css` **文件名**而非解析 registry（等价由
    `theme-contract.spec` 硬门担保 → 零解析零漂移，新增主题自动纳入）。
    **有意不做 pixel-diff 基线门**：字体栅格/AA/Chrome 版本任一变动即红，红了唯一响应是
    「重生成基线」= 幽灵门禁反模式（判据同 chunk 那轮，见 `docs/experience.md`）。
  - **首个战果（`5308ff8`）**：设置页 9 张卡里唯独「网络存储」底色/圆角/间距不同，**六主题全中**
    （六主题 `bg-surface ≠ bg-elevated` 无一例外，Xuan 差值最大最扎眼）。根因是 **Vue scoped CSS
    根节点双作用域规则**：组件根即 `<CollapsibleCard>`，style 里留着迁移前的自绘 `.settings-card`
    → `data-v-NS` 恰打在子组件根节点上 → 盖掉全局卡（bg 取 surface 而非 `--card-bg`、圆角 md 非 lg、
    多 16px 下边距）。同块 `.settings-card__header` 则是死代码——**残留样式一半活一半死，
    死活线正好卡在 scoped 规则上**。像素实测：修前 surface 段 58px，修后消失、elevated 吸收。
    **三道现有门全绿**（token 契约绿=两 token 都合法定义非幽灵、对比度绿=surface 本就是被守的合法底、
    typecheck/lint 绿）→ **取值全合法，错的是取了哪个**，正是矩阵才抓得到的那类。
  - **硬编码色治理的测量法**：算跨六主题**不变像素**（不随主题动 = 硬编码嫌疑）。settings **0.0% 恒定**
    （完全随主题走）；gallery 7.9% 恒定区**全落在照片内**（SVG fixture 与主题无关）；viewer 92.3% 恒定
    = **已声明的 S5 豁免**（`ContentViewer` style 顶部即写明「看图台永远黑（专业看图惯例）…刻意不随主题」，
    设计 §6.2），全库剥注释扫描 `background:#000|black` 仅 2 处且都在该豁免组件内 →
    **测量反向印证豁免边界正如声明、未外溢**。
  - **矩阵盲区（勿当已覆盖）**：viewer 的主题化控制条**无指针不出镜**（headless 无鼠标 + 全新 profile
    致侧栏默认隐藏）；canvas 网格需 DEV+localStorage flag 故不在矩阵内；原生窗口边框/WebView2 特有
    行为/GPU canvas 路径仍属真机面。详见 findings 会话续 27。
  - **幽灵 token 根治（6c778cd）**：全库 `--color-*` 剥注释后做集合差，查出 **4 个消费但无定义**。
    **两处用户可见 bug**：① `--color-badge-size` 无 fallback → 大小徽章底色六主题全透明、白字直压照片；
    其 canvas 孪生带 de-facto 回落值 `rgba(0,0,0,0.6)` 反而渲染正确 → **DOM/canvas 分叉**。
    ② MediaGrid 粘性分隔胶囊 `rgba(var(--color-bg-primary-rgb,255,255,255),.85)` 回落写死白 → 四套暗色主题白条。
    另两处：`--color-danger`（PerformancePanel 无 fallback 致录制/错误态没红出来；BookmarkPanel 走死红）
    → `--color-error`；`--color-bg-base`（HGalleryLabView 根衬底）→ `--color-bg-primary`。
    **修法全部沿用 S5 同族先例，无新语义**；badge-size 取 canvas 既有回落值使两路同色（DOM 消费端零改动）。
    **门禁：消费面 ⊆ 定义面**（theme-contract.spec）。**扫描前必须剥注释**——本仓约定在注释里留旧 token 名作
    历史记录（「原 var(--color-danger) 为幽灵(S5 修)」），不剥则已修站点被永久误报、门禁反噬良好注释
    （我的初版扫描器与 Explore 测绘均中招，误报 5 幽灵/11 消费者，真值 4/5）。变异测试双向自证。
  - **死 token 清理（d499dce）**：5 个定义但无消费。用户裁「全接线」→ **逐个核对靶子后两个接不了，带证据回报后改裁「删」**：
    `--color-badge-video`（该族语义是角落类型标签，而 `.badge-video` 实为居中播放键，只是同名；接线=把播放键涂蓝）、
    `--color-accent-dim`（六主题取值无一致语义：若为按下态该沿 hover 同方向再走一步，实际 4/6 反向 → 意图不可考）。
    `--color-text-placeholder` 相反（六主题一律比 tertiary 暗一档=语义自洽），两处 `::placeholder` 误用 tertiary → 接线；
    **顺带修门禁可信度**：check:contrast 本就硬门守 placeholder×bg-surface，而占位符实际由 tertiary 渲染 → 该门对由幻影变真。
  - **类型角标 AUDIO/DOC + 死票硬门（1f3ddcc）**：接线最后两个死 token，**引用闭环双向归零（49 定义 = 49 消费）**。
    范围按「不重复标记」原则（用户裁决）：audio 出（网格内无播放键无时长无专属视觉）；document **仅限走真实缩略图者**
    （pdf/svg/有封面 epub），文本卡格式已自带扩展名角标故不出；video 不出（已有播放键+时长）。
    判定走 `helpers.typeBadgeOf` **共享单源**——两路此前靠「条件逐字对齐 DOM 模板 v-if」的注释维持一致（人肉守约），
    刚修的 badge-size 分叉正是这类漂移产物。显示模型：新增 `type` 元素键进面板（与其余 10 项一致）+ **总开关 showThumbInfo
    默认改开**（元素面板由 `v-if=showThumbInfo` 门控，默认关等于把整套配置藏起来；elements 仍默认空 → 观感不变）。
    **连带修真实性能问题（默认翻转的前置）**：`ensureMeta` 此前只看总开关 → 勾 size/status 这类纯 item 字段元素也每屏拉
    EXIF/GPS/路径、拉回来无人渲染（违 A1「元数据按需供给」本意）→ 改由 `needsViewportMeta(elements)` 判定
    （取用面与 buildThumbInfoLines 的 `meta?.x` 对拍钉死）+ 元素列表补入 watch 源。
    **死票硬门**：定义面 ⊆ 消费面，与上轮合成双向互等，变异测试自证会红。
- [x] bundle/chunk warning 治理（**已施工 `ec7c378`**，本地门禁绿⏸非 CI）。
  **测绘结论推翻了「治理=瘦身」的默认预期：告警是噪音，但它掩盖的风险是真的** → 不调阈值、不重排 chunk，**换掉机制**。
  - **地面真相（实测）**：被 Rollup 喊的两个块**全是误报**——`cpp-*.js` 637.55 kB 是 shiki 的 C++ 语法（`@shikijs/langs`），
    `isDynamicEntry=true` 即**懒加载**、gzip 仅 47.22 kB、不进首屏；`index-*.js` 549.75 kB 是 16 路由全懒加载后的应用外壳。
    且 pdfjs/shiki **零模块在入口** → 当前架构本就正确，**没有可修的东西**。
  - **真风险**：Rollup 那条是**告警不是门**。把 pdfjs（365 kB）静态 import 进 `main.ts` 造回归，`npm run build`
    照喊照过 **exit=0**、CI 全绿。叠加效应最伤：常年两条误报训练所有人无视此类告警，真回归也淹在噪音里。
  - **落地**：`scripts/vite-plugin-bundle-budget.mjs` 两条可行动不变量（破则 build 失败）——①入口块 ≤ 620 kB
    （实测 549.75 + ~13% 余量）；②`pdfjs-dist`/`shiki`/`@shikijs/*` 不得进入口（设计契约，红了必是真回归）。
    **有意不设懒块上限**：最大懒块体积由上游决定，为它设阈值唯一可能的响应就是调大数字＝幽灵门禁反模式
    （见 `docs/experience.md`），改为只报告不拦截保住可见性。
  - **已排除的路径（实测，勿重复尝试）**：`__VUE_OPTIONS_API__=false` 仅省 **4.31 kB**（全库 79/79 SFC 皆 `script setup`）；
    `__VUE_PROD_DEVTOOLS__` **已默认 false 省 0**（4 个 devtools 模块不受该开关管）→ 编译期开关全是噪音。
    `manualChunks` 拆 vendor：Tauri 本地加载**无 HTTP 缓存**，拆分不减首屏解析字节，纯为消告警＝装饰性。
- [x] 全量门禁与真实 Tauri 截图验收：**round9 于 2026-07-15 真机验收通过（A–E 全项无异议）**；
  本地门禁绿（非 CI）lint/typecheck/**vitest 941**/check:contrast/build（入口 549.89 kB < 620 预算门）。
- **状态：** complete（token 引用闭环三提交 + chunk 治理 `ec7c378` + 视觉矩阵 `3620e26`/`5308ff8`；
  round9 真机验收通过。**S7 收官**。残留真机盲区不阻断收官，已登记于 findings 会话续 27）

## 关键问题
1. 当前产品的核心用户路径与最高频操作是什么？
2. 现有页面是否能在本地完整运行，哪些状态依赖真实图库数据？
3. 重构应保持现有视觉品牌，还是允许建立新的视觉语言？

## 已做决策
| 决策 | 理由 |
|------|------|
| 先审计后实施 | 用户明确要求先出方案待确认 |
| 本阶段不改业务代码 | 避免在设计方向确认前造成返工 |
| 审计同时覆盖源码与真实渲染 | UI/UX 不能仅凭静态代码判断 |
| 全部推荐项进入实施 | 用户已明确“全部按推荐” |
| accessibility 非阻断 | 用户明确允许暂不作为必须支持项；仍避免明显回退 |

## 遇到的错误
| 错误 | 尝试次数 | 解决方案 |
|------|---------|---------|
| harness/runtime.ts 顶层读 window，9 个 node 环境 spec 收集崩溃（84 测试未执行） | 1 | typeof window 环境守卫；已修（bcf0934） |
| keybinding 分发器只查 navigation 组，zoom/info 按 ownership 挪入 view 组后 +/-/i 键失灵 | 1 | 分发去组过滤（组只管渲染位置，上下文安全由 when 保证）；已修（bcf0934） |

## 备注
- 网页和外部规范内容仅记录到 findings.md，不写入本计划。
- 每阶段完成后更新状态，重大决策前重新读取本文件。
