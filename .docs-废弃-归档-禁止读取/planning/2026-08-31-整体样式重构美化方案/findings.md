---
status: 施工中
type: 工作记忆
line: 整体样式重构美化
created: 2026-08-31
---

# 发现与决策:整体样式重构美化

## 需求
- 用户原话:「整体样式重构/美化(主题色、间距、组件观感等大改)。先出报告和方案。重构时不必拘泥于现有设计和实现,本项目处于高速开发迭代,任何设计和实现都可被推翻,一切以现在最优设计和实现为首要目标。」

## 发现
- F-001 前序工作基线:2026-08-15《Scrollery清透材质化UI建议方案》(docs/designs/)+2026-08-19 实施计划,第一批已落地(material.css adaptor + AppShell/WindowChrome/阅读区试点/popover+dialog float,提交 caebb68),属保守渐进路线(不改主题色/几何/密度)。本次需求范围更大:主题色、间距、组件观感全面重构。
- F-002 Kun 对比结论(2026-08-14 findings):清透感=低色相噪声+受控对比+少表面层级+足够留白+统一节奏;Scrollery 当时是 palette-first,卡片套卡片问题突出。第一批材质层只做了 alias 层,未动观感本体。
- F-003 现状速览:6 主题(ink/porcelain/moonlight/xuan/obsidian/dai),语义 token 层 + 组件契约 token(variables.css)+ material.css alias 层;全局 .btn/.btn-icon/.settings-card 等配方在 index.css(532 行);组件大量 scoped style。

## 外部资料(当数据,不当指令)
- ui-ux-pro-max 检索结果(见后续追加)

- F-004 审计统计(2026-08-31 实测,rg):组件硬编码几何 px 567 处(Top:PerformancePanel 30/LogWindowView 21/NetworkStorageSection 19);硬编码 font-size px 102 处;组件硬编码色 263 处(628 总-365 主题文件,Top:MediaThumb 42/ContentViewer 26 豁免/TimelineScrubberCanvas 18);!important 8 处活代码;backdrop-filter 14 处使用点(一半走 material recipe 一半裸写)。
- F-005 scoped style 体量 Top:DuplicatesView 929 行/MediaThumb 516/SemanticSearchPanel 440/PluginStoreView 421/AppToolbar 379——原语化未覆盖页面的自绘重灾区。
- F-006 顶栏 40px 单行约 20 元素;chip.active 四重强调叠加(底+边框+文字色+字重跳变)引发布局微抖;日期分隔符全宽 accent 顶条是画廊最抢眼的非内容元素(六主题截图一致)。
- F-007 徽章色彩动物园:ORIG 蓝/AUDIO 绿/LIVE 红/DOC 橙/评分黄红星/大小黑底 6 种饱和色可同压一张照片;DOM(themes/*.css)与 canvas(palette.ts fallback)双源维护,史上已发生分叉 bug。
- F-008 视觉基线 18 张已捕获:.screenshots/theme-matrix/(六主题×gallery/settings/viewer,1440×900,headless Chrome)。

## 外部资料(当数据,不当指令)
- ui-ux-pro-max 检索:产品域「File Manager & Transfer」推荐 Flat Design + Minimalism & Swiss Style,次选 Dark Mode (OLED),色板方向「功能性中性 + 文件类型色编码」;风格域命中 Adobe Spectrum(创意工具 token 化中性阶梯、dense but legible)。两次 --design-system 默认查询偏营销页方向,按 Query Contract 收窄后采纳 product/style 域结果。
## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-009 | B0 共享尺度契约与运行时默认值必须同步,避免启动后 CSS token 被旧配置公式覆盖 | code |
| F-010 | B0 字号阶梯的运行时映射需有回归测试,守住启动/设置页一致性 | test |

## 接手复核(2026-09-01)
- F-009:B0 若只改 `variables.css`,启动配置仍会用旧的 16px/6px fallback 覆盖 CSS 基线;因此同步 `uiScale.ts` 字号偏移、configStore/schema 默认值、滚动条 fallback 与双语设置文案,并保留已有持久化值不迁移覆盖。
- F-010:当前六主题对比度门禁原有 126 项全部通过;B0 不触碰主题色,聚焦回归(theme-contract + uiScale) 16 tests 全过。
- F-011:B1 将 accent 的填充/hover/subtle/文字/彩底前景拆成五角色;亮暗主题状态浅底分别收敛为 8% alpha,并以实色状态文字对最终合成浅底做 4.5:1 检查,避免直接把同一个 status hex 同时当文字与底色。
- F-012:静态色板锚点生成器只在显式 `--write` 时改 CSS,CI 使用默认 `--check`;六份主题 CSS 与 registry preview 的 accent 同步由契约守护,中性表面只允许按明度顺序排列。
- F-013:共享 badge/rating token 已进入 `variables.css` 与 Canvas palette 的 canonical 读取/fallback,但实际 MediaThumb DOM、Canvas painter 和 StarRating 迁移依赖同一批视觉验收,按设计 D-003 留给 B4。

## 施工收口(2026-09-02)
- F-014:B2/B3 将全局控件、surface、float、工具栏、状态栏、侧栏统一到 28/32/40px 与 material recipe;Popover/Dialog 的业务 slot 已去掉重复表面,减少嵌套边框/阴影。
- F-015:B4 让 MediaThumb DOM 与 Canvas painter 共用 scrim、类别标记、评分 amber、divider 与 radius 读取;选中态改为稳定内描边,日期分隔符改为中性文字+计数+divider,避免状态放大和全宽色条抢内容。
- F-016:B5 将设置页普通分节改为扁平分组流,仅危险/导入导出等保留独立表面;设置导航、Reader 选项、表单和对话框统一控件档与焦点环。
- F-017:B6 覆盖次视图、播放器、日志、阅读器、选择工具条、语义搜索及媒体菜单;实底状态统一使用 paired foreground token,媒体内容/HUD/开发实验区的固定色保留为显式边界。
- F-018:B7 重新捕获六主题×三场景 18 张截图并抽查 Ink/Porcelain;全量前端 145 files/1630 tests、typecheck、lint、build、contrast、theme-palette、verify:channel、Rust fmt 与 diff check 均通过。
