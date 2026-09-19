---
id: 2026-07-17-status-UI-多主题系统
status: active
type: rolling-status
line: UI-多主题系统
created: 2026-07-17
---

# UI-多主题系统 · 滚动状态

## 2026-09-16 主题配色全新模型

已落地：[当前方案](../designs/2026-09-16-主题配色全新设计.md)。两套独立种子（背景／前景／强调色／层次对比度／画廊底色）驱动同一生成器；DOM、Canvas、缩略预览与首帧同源。默认中性底色、绿色强调、纯色材质；内置中性／暖纸／冷夜，个人完整主题支持保存应用、另存、按 ID 更新、重命名、删除与撤销。外观模式独立；旧 ID、旧浓度、六 CSS、material 别名层、旧缓存均已移除，不迁移旧配置、不清除其它设置。

草稿只在内存、按动画帧发布；应用走既有批量保存，失败回滚并保留重试草稿，主题外部更新／重置取消草稿。首帧缓存只写后端确认值；共享窗口底直接对应一个 0–100% 不透明度。此处取代本线历史记录中的六主题 ID、分层浓度和额外画廊遮罩约定。

验证：vue-tsc、受影响 lint、各批定向测试与对比度脚本通过；浏览器六组合（浅／深／自定义 × DOM／Canvas）示例画廊及命名/撤销/HEX/取消等交互已验收。详细测试范围和提交见[实施记录](../worklogs/2026-09-16-主题配色重构/progress.md)，不重复加总重叠测试。

待实机：Windows 纯色／Mica／Acrylic 与显式 gallery、真实图库 GPU 和图片像素、高 DPI、其它平台、真实应用重启找回个人主题。本轮未完整打包；浏览器不证明原生玻璃。既有 harness Tauri 事件订阅报错保留为旁支，未扩大施工。
## 2026-09-16 主界面融合玻璃

用户真机反馈：首轮在 Mica/Acrylic 下仍有明显分组横条和工具行色差，因此首轮浏览器透明背景核验不构成原生视觉验收通过。

同日补修：移除标题常驻局部滤镜、标题与工具整行悬停染色。标题使用相邻零高文档流标记判断真实 sticky 位移，只有吸顶/吸底遮挡时在伪元素上加局部衬底；全侧栏共用一个滚动监听与 rAF，主体尺寸变化触发复判。不能使用 sticky 标题自身 `offsetTop` 当自然位置，浏览器实测它会随吸附变化。

补修验证：typecheck、18项既有手风琴/对齐测试及局部ESLint通过；浏览器1280×1200下Mica与Acrylic四标题均背景透明且无滤镜，工具hover透明；1280×720下吸底管理标题及滚动后吸顶图库标题按实际位移启用遮挡，其余标题保持透明；折叠工具后无误判。**原生色差尚待用户复核**，没有把局部滤镜认定为已经实测的唯一原生原因。记录：[玻璃侧栏色差补修](../worklogs/2026-09-16-玻璃侧栏色差补修/closeout.md)。

已实现：玻璃模式下顶栏、侧栏、画廊、时间轴与底栏共享 `.app-shell` 的单层半透明底；各区域默认透明，侧栏分组标题撤掉底色和分割线，工具改为透明任务行。照片与 DOM/Canvas 画廊分组标题保持原样，未增加整面画廊滤镜。搜索焦点、选中态与覆盖照片的浮出顶/底栏保留局部可读表面。

融合布局约定继续有效：沿用原生材质与 `data-glass` 开关，单层共享底面。配色与透明度已由上方新模型替换：非玻璃使用生成的纯色，window_opacity 直接控制共享底层；gallery=auto 透出共享底，显式颜色为不透明覆盖，不再提供额外画廊遮罩。不扩大到原生材质后端、图库渲染或文字页面布局重构。本节替代下方历史记录中“各窗口栏分别绘制材质、工具卡常驻底色、侧栏标题常驻染色”的主窗口视觉约定。

验证：typecheck、31 文件 533 项相关测试、两个语言文件的局部 ESLint、diff 检查通过；浏览器核验浅色 Mica、深色 Acrylic 的根部有色/区域透明、工具无底无影，搜索焦点与文件夹选中态可见，侧栏滚动可用，关闭玻璃后恢复原底色。浏览器截图仅为 UI harness 示例资产，不代表原生 DWM 效果。

待真机：Windows 背板与壁纸下的最终观感、F11/窗口化沉浸顶底栏浮出效果、高 DPI 与真实图库 GPU 滚动。未做全量构建或跨平台实测。记录：[全局融合玻璃界面](../worklogs/2026-09-16-全局融合玻璃界面/closeout.md)。

## 2026-09-12 视觉调查与目录条修复

调查报告：[前端UI视觉问题调查与优化建议](../reviews/2026-09-12-前端UI视觉问题调查.md)。已抽查浏览器三主题及最小窗口，原生材质与物理高 DPI 未验收。当时后端文字默认75、前端/harness兜底100；该旧浓度模型现已删除，旧截图不能作为新模型默认观感。

| 状态 | 候选批次 | 内容与验收 |
|---|---|---|
| ⬜ | UI-V-A 明确缺陷 | V09取色补修后用户要求沉浸式标题：DOM/Canvas已移除所有分组标题底板、阴影、分隔线及专用失效色板字段，保留文字图标统计与目录布局，待用户视觉验收。V01/V04/V05/V08仍待排期 |
| ⬜ | UI-V-B 视觉基线 | V02/V03/V07：统一默认值与fixture、校准实际文字对比度和键盘焦点；保留柔和偏好并提供清晰基准 |
| ⬜ | UI-V-C 观感与密度 | V06及报告设计建议：控件尺寸、照片覆层、设置层级、文字页宽度；先做对比样例，风险项复现后纳入 |

用户已授权V09修复，并在新截图反馈白色条带后明确要求所有画廊分组标题无底板、无阴影、无分隔线，替代此前浅底方案；目录独占一行符合预期，不修改布局。V09界面验收由用户执行。其余候选仍待排期，既有真机验收挂账继续保留。

(分片由 `worklog upgrade` 生成;收口时 disposition=todo 的候选落此,请随施工滚动更新。)

- 2026-09-06(主题色浓度配置化):用户反馈各主题配色偏深。新增 `[ui].theme_tint_strength` hot 键(20–100,**默认 60**,100=满浓度出厂锚点),六主题底色 wash token(bg 五层/canvas 三件/doc-paper 两件)由 `generate-theme-palette.mjs` 锚点生成 `color-mix(in oklch, 锚点 × calc(var(--theme-tint-scale)) × 中性)` 表达式——亮主题向白混(更浅更淡)、暗主题向同明度无彩灰混(`oklch(from 锚点 l 0 h)` 相对色语法,WebView2/WKWebView 2024+ 基线);accent/文字/边框/hover 不随浓度走(身份与可读性锚点)。全链路沿用玻璃键先例(schema/StartupConfig/uiStore/tint.ts/settingsMap/i18n/夹具),快照载荷增 tint、`public/theme-snapshot.js` 首帧预写 CSS 变量、index.html 启动层同配方;Canvas 调色板与 BookReader(iframe 不认 app 变量)经 `utils/cssColor.resolveTokenColor` 探针解成具体色,`parseColorToRgb` 增 oklch/color(srgb) 分支,MediaGridCanvas/BookReader 监听浓度重读。三处 hex 门禁按满浓度锚点评估(最保守界,免复刻 oklch):契约明度层序(同比例混向公共中性,序不变)/预览同步/对比度脚本。`&tint=` harness 覆盖 + capture `--tint=` 供矩阵截图。自动门禁全绿(vitest 1762、typecheck/lint、cargo 1262、palette、contrast);截图已核 DOM/Canvas 两路径、亮暗主题、启动层。**剩余**：真机观感裁决(默认 60 是否合意、玻璃模式下叠 wallpaper 观感、WebView2 相对色语法实测)。工作记忆见[三件套](../planning/2026-09-06-主题色浓度设置/task_plan.md)。
- 2026-09-06（文字浓度配置化）：用户反馈出厂文字对比过强（「太瞎眼」），紧随主题色浓度给文字 ramp 落同机制配置项：新增 `[ui].theme_text_strength` hot 键（40–100，**默认 75**，下限防正文不可读），六主题文字四 token（text-primary/secondary/tertiary/placeholder）由 `generate-theme-palette.mjs` text 锚点组生成 `color-mix(in oklch, 锚点 × calc(var(--theme-text-scale)) × 中性)` 表达式（亮向白/暗向同明度无彩灰，与底色 wash 同配方）；text-inverse/on-accent/on-*/accent-text/sidebar-active-text 不参与（极性随底翻转或属 accent 族）。tint.ts 改名 **strength.ts** 容纳底色/文字两套浓度常量；快照载荷增 text、`public/theme-snapshot.js` 首帧预写、index.html 启动层 `--startup-text/--startup-muted` 同配方。三处 JS 直读点（mediaGridCanvas.palette sepText/textPrimary、TimelineScrubber text1/2/3、BookReader iframe 注入 text）改走 `resolveTokenColor` 探针；MediaGridCanvas 增 textToken 重读，TimelineScrubber 的 data-theme MutationObserver 扩观察 style 属性（浓度 setter 写内联 style），BookReader watch 增文字浓度。对比度门禁正则扩为双 scale 变量，并在注释写明方向不对称：底色稀释改善对比（锚点=保守界），文字稀释恶化对比（锚点=出厂满浓度色板；默认档在默认底色上实测 primary 6.1–8.0、secondary 3.3–3.7 仍守各自门槛，tertiary/placeholder 属元数据文字允许随 knob 变浅）；`&text=` harness 覆盖 + capture `--text=`。全门禁绿（vitest 1775(+13)、typecheck/lint、cargo 1262、palette、contrast）；截图核验：默认 75 侧栏与 Canvas 分隔行明显柔化、text=100 回出厂、暗主题 40 下限仍可读、设置页控件双语渲染。**剩余**：真机观感裁决（默认 75 是否合意、与 tint 60 的组合观感）。工作记忆见[三件套](../planning/2026-09-06-文字浓度设置/task_plan.md)。
- 2026-09-06（玻璃模式内容底面修复）：Acrylic 真机反馈设置页整页发灰（DWM 背板回退均匀纯灰 #D2D2D2 + 文字页无底面承重,像素实测见方案 §2）,按[修复方案](../designs/2026-09-06-玻璃模式内容底面修复方案.md)施工——glass.css 新增 `--glass-content-fill`（bg-primary 90% 半透明,mica/acrylic 共用,头注释附视图根 opt-in 接入手册）,设置/收藏/人物/插件商店/文档阅读器五视图根 opt-in 承重,`.settings-header` 与 doc-viewer 工具条拼布置透明;第 6 个玻璃键 `glass_content_opacity`（UInt 20–120 默认 100,hot）按既有 5 键先例全链路接线（schema/StartupConfig/uiStore/uiScale/settingsMap/Control/i18n 双语/夹具）。audio-player、hlab 根自铺不透明底（判据:根面裸才加 fill）与独立窗口 log-window 不动;画廊 `--glass-gallery-fill` 保持 0% 完全透出。同次修订 2026-08-24 方案 §1/§3/§9 被证伪表述（决策记录第 7 条）与 glass.spec 契约。自动门禁全绿（cargo test workspace 1355、vitest 1747（+2 用例）、typecheck、lint）。**剩余**：方案 §7 真机手动验收（用户场景复测、背板三态矩阵、画廊回归、三风格×亮暗×壁纸可读性、20/100/120 热切换、托盘往返/失焦）。工作记忆见[三件套](../planning/2026-09-06-玻璃模式内容底面修复/task_plan.md)。
- 2026-09-02(整体样式重构收口):B0–B7 代码批次全部完成——全局 28/32px 控件与 material float、外壳/侧栏、DOM+Canvas 画廊徽标与分隔符、设置页大表面化、对话框及次视图/阅读器/播放器/日志收敛;六主题矩阵 18/18 捕获并抽查 Ink/Porcelain。全量 Vitest 145 files/1630 tests、lint/typecheck/build、contrast、theme-palette、verify:channel、Rust fmt、diff check 全通过。真实 Windows WebView2、高 DPI、玻璃背板/切换与 GPU Canvas 仍按 B7 手测清单保留为目标设备项。
- 2026-08-31(方案待裁决):《[整体样式重构/美化方案](../designs/2026-08-31-整体样式重构美化方案.md)》已出稿——「暗房」专业工具语言:13px UI 基准/间距 12px 档补齐/圆角收敛 6·10·12/阴影纪律/OKLCH 锚点派生六主题/状态色六主题统一/徽章 scrim 化/顶栏·画廊·设置页分区蓝图;B0–B7 分批与 D1–D10 待裁决项在文内。本方案批准后将取代 2026-08-15 清透方案的保守边界成为样式线新基线(清透第一批 material adaptor 地基保留)。工作记忆见[三件套](../planning/2026-08-31-整体样式重构美化方案/task_plan.md)。
- 2026-08-20：已采纳《Scrollery 清透材质化 UI 建议方案》的审查修订，设计基线现明确媒体 Canvas 边界、标题栏合并/独立模式、material token 契约、现有媒体 blur 处置和可复验收要求。
- 2026-08-24（方案落盘）：新增设计方案《[窗口材质毛玻璃](../designs/2026-08-24-窗口材质毛玻璃方案.md)》——默认 Mica + Acrylic 用户可选（`window_material` 热键 + window-vibrancy DWM 背板 + `html[data-glass]` chrome 半透明三层配合），已定稿待施工授权;上游卡顿/失焦/可见性重置等坑位与逐文件变更清单均在方案内,施工时另起三件套。
- 2026-08-24（施工落地）：按方案 §4 全量落地并同批提交——`window_material` 热键三值（默认 mica）、window_material.rs DWM 背板（Win10 mica→blur 退化 + 四处 show 后重应用对抗 tauri#12854）、tauri.conf transparent、uiStore `data-glass` 单源写点 + glass.css chrome 覆写（浓度初值 mica 72%/acrylic 82%）、设置行 + 双语五键；自动门禁全绿（cargo fmt/clippy/test 1327、npm lint/typecheck/vitest 1564/build、NOTICE 零 churn）。**剩余**：方案 §7 手动验收清单（Win11 Mica/Acrylic/none 观感与切换、托盘往返、六主题×壁纸可读性浓度微调、Win10 退化、macOS no-op）。工作记忆见[三件套](../planning/2026-08-24-窗口材质毛玻璃/task_plan.md)。
- 2026-08-25（真机反馈修复）：修正祖先实色截断与嵌套材质复合——玻璃模式仅让 `.app-content`/设置根容器透出，媒体/卡片各自承重；窗口三键区复用父材质；Rust 非 none 先清三种旧背板，`Focused(true)` 按配置重挂 Acrylic。新增 glass.css 静态契约测试；自动门禁复跑全绿（Rust workspace test 主库 1094 过/6 忽略、clippy/fmt/check、npm 138 文件/1567 测试、lint/typecheck/build、NOTICE）。**剩余**：同上 GUI/跨平台手动验收，浓度 72/82 与各 surface token 仍待六主题×明暗壁纸实机裁决。工作记忆见[三件套](../planning/2026-08-24-窗口材质毛玻璃/task_plan.md)。
- 2026-08-25（红圈表面微调）：分组标题/路径行由 94% 遮罩改为低 alpha + 12px blur，普通树行透明；工具卡、紧凑 select、合并/独立标题栏搜索框和侧栏 toggle 统一使用 `--glass-*` 半透明表面 token（Mica sticky/card/control=42/24/38%，Acrylic=36/20/32%）。本地 UI harness 已目检 Porcelain/Ink，并以 computed style 复核 Mica、Acrylic 与搜索焦点态；定向 19 测试、六主题对比度门禁、生产构建均绿。**剩余**：Tauri/Win11 原生背板、壁纸、托盘与跨平台人工验收仍未自动化。工作记忆见[三件套](../planning/2026-08-24-窗口材质毛玻璃/task_plan.md)。
- 2026-08-25（画廊玻璃底面）：确认画廊此前仍由 DOM `--color-bg-canvas` 与 Canvas `alpha:false + canvasGap` 不透明铺底，未透出 DWM。现仅在 Windows Mica/Acrylic 时让 `.media-grid-layout` 透明，Canvas 切为 alpha context+透明清屏；`none` 保持旧 opaque 快路径。切材质会重建 Canvas 以兑现不可变 alpha 设置。定向 70 测试、typecheck、生产构建均绿；本地 harness computed style 已证实透明链路，**仍需** Win11 原生背板下目检照片缝隙/DOM↔Canvas/none 切换。工作记忆见[三件套](../planning/2026-08-24-窗口材质毛玻璃/task_plan.md)。
- 2026-08-20（施工落地）：首批 Phase 0–4 代码已提交 caebb68——material adaptor（material.css 14 角色/配方 + theme-contract 扩展）、AppShell/WindowChrome chrome、ReaderSettingsSection 阅读区 surface 试点、view popover + UiDialog floatSurface；自动门禁独立复跑全绿（typecheck / lint / 对比度全硬门槛 / vitest 1549 测 / build 678.45kB）。**剩余**：phase5 六主题矩阵重跑 + 真实 Windows WebView2 人工验收（六主题层可分 / 合并独立标题栏 / popover+dialog 焦点与 ESC / 高密度滚动无退化 / 150% DPI / 沉浸往返）+ 阶段 0–4 退出条件判定 + 阶段 6 扩面或止步评审 + 收口蒸馏 F-001/F-002 与 D-001..D-004 迁 worklogs。

> 状态板 2026-08-24 自 docs/todo.md「前端 UI 优化与多主题系统」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## 权威文档
> 📦 本节 2026-07-12 整节收官搬迁: S1–S6 六主题(墨/素/宣/玄/黛/月白)+ 切换 UI(ThemePicker / 外观 segmented / 侧栏三态)+ Windows 标题栏真彩跟随主题 + S6 注册式设置页重构全交付;完整交付史 / commit / 病历详注已迁 [completed.md](../completed.md) 同名节。设计与门禁契约见 [主题设计](../designs/2026-07-06-前端UI优化与多主题系统.md)。

## 状态板
> **⏸ 残留(仅真机观感,代码已就位)**: ① 主题三态 + 迁移手测统一走查 ② Windows 标题栏着色 Win11 真机确认 ③ 设置页逐项 GUI 对照。
>
> (注:材质化 ⏳ 与上文 2026-08-20 两条同源、已同步在档;以下按 todo.md 原文照录。)
>
> **⏳ 清透材质化 UI（首批已施工，待真机验收）**: 2026-08-20 施工授权落地，首批（Phase 0–4 代码）已提交 caebb68——material adaptor + AppShell/WindowChrome chrome + 阅读区 surface 试点 + view popover/dialog float；自动门禁独立复跑全绿。**剩余**：phase5 六主题矩阵重跑、真实 Windows WebView2 人工验收六项、阶段 0–4 退出条件判定、阶段 6 扩面或止步评审。工作记忆见[实施计划](../planning/2026-08-19-清透材质化UI实施计划/task_plan.md)。

- 2026-08-25（毛玻璃透明度配置化）：新增 `[ui]` 下 4 个 hot 配置键 `glass_chrome_opacity` / `glass_sticky_opacity` / `glass_surface_opacity` / `glass_control_opacity`，默认 100、范围 20–120；设置页提供 4 个数字输入，外置 config.toml 经既有 `config-file-changed` 链路热更新。glass.css 保留 Mica/Acrylic 的相对基准配方，仅按 4 类 surface scale 调整窗口栏、sticky 遮罩、卡片与控件透明度；88 项定向前端测试、Rust schema 目标测试、typecheck/ESLint/fmt 与本地浏览器 CSS computed 核验均通过。**仍需** Win11 真机验证外部文件热切换与各层最终观感。
- 2026-08-25（画廊底面透明度配置化）：新增 `[ui].glass_gallery_opacity`，默认 0%、范围 0–100；设置页提供第 5 个数字输入，玻璃模式下通过 `--glass-gallery-opacity` 调整 `.media-grid-layout` 的主题色底层/图片间隙，非毛玻璃模式保持原有画廊底色。全量前端 1581 测试、typecheck/lint/build、Rust schema 8/8、fmt 与本地 harness 核验均通过；仍需 Win11 原生背板与外部 config.toml 热切换实测。
