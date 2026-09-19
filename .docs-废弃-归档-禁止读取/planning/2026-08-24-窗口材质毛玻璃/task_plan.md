---
status: 施工中
type: 工作记忆
line: 窗口材质毛玻璃
created: 2026-08-24
---

# 任务计划:窗口材质毛玻璃

## 目标
落地[2026-08-24-窗口材质毛玻璃方案](../../designs/2026-08-24-窗口材质毛玻璃方案.md):默认 Mica、Acrylic 用户可选的原生玻璃模式,`window_material` 热键三值(mica/acrylic/none),Rust DWM 背板 + 前端 glass.css 两层配合,单 commit 交付。

## 当前阶段
阶段 6:毛玻璃分层与画廊底面透明度配置化已施工,等待 Win11 原生材质与外部 config.toml 手动验收

## 阶段

### 阶段 1:三件套与施工准备
- [x] 读方案、核对现状锚点
- [x] 建 planning 三件套
- **状态:** done

### 阶段 2:两侧并行施工(子代理)
- [x] Rust 侧:tauri.conf.json / Cargo.toml / window_material.rs 新建+注册 / schema.rs / config_commands.rs / 三处 show 后重应用
- [x] 前端侧:uiStore(含 spec 两用例)/ settingsMap / DynamicSettingControl / i18n 两份 / glass.css 新建+index.css 导入
- [x] 主会话补漏:ipcFixtures.ts StartupConfig 构造缺 windowMaterial(vue-tsc 抓到)
- **状态:** done

### 阶段 3:终审与集中测试
- [x] 主会话 diff 终审(对照方案 §4 逐项吻合;前端白名单含 none 三值为合理裁决)
- [x] cargo fmt/clippy/test(1327)+ npm lint/typecheck/test(1564)/build + NOTICE --check 全绿
- [x] 既有门禁债(fmt 5 处 + clippy 1.98 chunks_exact 新 lint 11 处)修复,独立 commit 不混入功能
- **状态:** done

### 阶段 4:收口
- [x] 双 commit:门禁债 224348b3 → 功能 0aca66ed(方案 §9 原子性)
- [x] 回写状态分片 docs/status/UI-多主题系统.md(施工落地条目)
- **状态:** done

### 阶段 5:真机手动验收(方案 §7 手动清单)
- [x] 首张 Win11 Acrylic 截图与实现交叉审查(原生/CSS/测试三路子代理)
- [x] 定位红箭头与红圈根因:嵌套半透明重复合成、侧栏实色 sticky header、内容画布主动不透明
- [x] 顺查 Mica/none 与原生生命周期:确认 none 静态清理路径;登记失焦重挂、旧 Win11 切换互斥风险
- [x] CSS:内容/设置根容器透出 DWM,三键区复用父材质,sticky 头栈使用高不透明玻璃遮罩
- [x] Rust:非 none 切换前清理旧背板,窗口重新获得焦点时按配置重挂
- [x] 新增 glass.css 静态契约测试并完成定向/全量自动门禁
- [x] 按红圈反馈将侧栏标题、工具卡、搜索框、紧凑控件收敛为低透明度玻璃 token；仅 sticky 标题/路径行使用 12px blur，普通文件夹树行透明
- [x] 按追问将画廊主底面接入玻璃：DOM `.media-grid-layout` 透明，Canvas 仅在 Windows Mica/Acrylic 时重建为 alpha 画布并透明清屏；无材质仍保留 opaque 快路径
- [x] 按后续需求将玻璃浓度配置化：窗口栏、分组/路径遮罩、卡片表面、控件表面 4 组 `glass_*_opacity`，默认 100、范围 20–120；配置文件与设置页共用启动水合/热更新链路
- [x] 追加画廊底面/图片间隙 `glass_gallery_opacity`：默认 0、范围 0–100；仅玻璃模式绘制主题色遮罩，非毛玻璃保留原有底色
- [x] 设置页 5 个数字输入、双语描述、CSS `color-mix` 变量与前端/Rust 回归测试；本地 harness 确认 `data-glass=mica` 下变量可被浏览器解析
- [ ] Win11 全新 profile:默认 Mica;三值切换观感;照片缝隙在 Mica/Acrylic 露出背板、DOM/Canvas 双路径无残帧;none 与现状逐像素一致
- [ ] 外部编辑 config.toml 热切换 5 组浓度与材质;托盘/最小化往返/F11 材质不丢;窗口操作无 artifact
- [ ] 六主题 × 亮暗壁纸可读性 → 必要时调 chrome 72/82 与 surface/sticky/control 玻璃浓度
- [ ] Win10 blur 退化观感;macOS 构建 no-op 回归
- [ ] 全过后收口蒸馏(F-001/F-002 候选)迁 worklogs
- **状态:** in_progress

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 全盘照方案 §4-§5 施工,不另起炉灶 | 方案已定稿含逐文件代码与锚点 | |
| Rust/前端两侧并行子代理施工 | AGENTS 善用子代理;两侧无相互依赖,方案为唯一契约 | |
| 截图红圈判为验收标准推翻原“内容区不透明”边界,不归因于 DWM Acrylic 失效 | `.app-content` 与 `.settings-view` 均显式铺实色,背板不可能透出 | D-001 |
| 玻璃模式同时透出布局容器与画廊底面，媒体像素/卡片继续各自承重 | DOM `.media-grid-layout` 透明；Canvas 仅在 Windows Mica/Acrylic 用 alpha+clearRect，none 保留 `alpha:false + canvasGap` 性能快路径 | D-002 |
| 每次非 none 原生应用前先清三类旧背板,Focused(true) 再按配置重挂 | 覆盖 Win11 22000–22522 跨通道切换与 Acrylic 失焦丢失风险 | D-003 |
| sticky 标题/路径行用低 alpha + 12px blur 保住滚动可读性；卡片与控件只用半透明填色 | 去掉 94% 白色纸片感，同时把 backdrop-filter 控制在少量 sticky 元素，避免工具卡多层合成 | D-004 |
| 玻璃浓度按 4 类 surface 独立缩放,100% 保持每种材质现有配方 | 用户既可整体调淡也可只压低突兀的某一类白色表面；不把 Mica/Acrylic 的相对配方抹平 | D-005 |
| 画廊底面使用独立直接不透明度,默认 0% | 四类表面缩放无法改变原本为透明的画廊底色；用户需要在完全透出 DWM 与主题色底之间连续调节,且 none 路径保持原样 | D-006 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
