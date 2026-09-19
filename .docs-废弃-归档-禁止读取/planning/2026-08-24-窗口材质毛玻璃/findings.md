---
status: 施工中
type: 工作记忆
line: 窗口材质毛玻璃
created: 2026-08-24
---

# 发现与决策:窗口材质毛玻璃

## 需求
- 标题栏/底部状态栏/全局增加半透明毛玻璃,能半透看到 Windows 桌面
- 裁决:默认 Mica,Acrylic 做成用户可选项
- 2026-08-25 红圈反馈:毛玻璃上的分组标题、工具卡、搜索框与开关不能再呈纯白纸片感，需统一为低对比玻璃层
- 2026-08-25 追问:确认画廊背景是否属于毛玻璃；若不是，改为毛玻璃，若已是则进一步提高透明度
- 2026-08-25 后续追问:画廊大底与图片间隙也需要独立调节，不能与卡片/控件表面共用缩放语义

## 发现
- 方案(2026-08-24 定稿)已核实全部技术锚点:window-vibrancy 0.6 为 tauri 2.11.4 传递依赖零新增解析;tauri set_effects 吞错误不可用须直用 window-vibrancy;transparent 在 macOS/Linux 为 no-op;可见性变化会重置材质(tauri#12854)故四处 show 点重应用。施工前无须重查,详见方案 §2。
- 首张 Win11 Acrylic 真机截图复盘:红圈不是原生背板失效;`glass.css` 主动给 `.app-content` 铺不透明 canvas,`SettingsView.styles.css` 又给 `.settings-view` 铺 `--color-bg-primary`,因此设置页空白处必然完全不透。该截图验收要求已推翻方案原“内容区保持不透明”的边界,后续修复须把实色下沉到媒体像素画布/卡片。
- 右上窗口三键区在 `.window-chrome` 半透明父层之上再次绘制同一 chrome 背景:Acrylic 82% 两层叠成约 96.8% 不透明,Mica 72% 叠成约 92.2%,对应截图顶部接缝;控件区应复用父层而非重复着色。
- 侧栏“图库/工具/管理”等红箭头对应 `AccordionSection .acc-header` 的显式实色 `--color-bg-secondary`;文件夹 sticky/tree 行也有同类实色。透明模式不能粗暴删背景(滚动内容会穿字),需玻璃专用遮罩/材质并保 sticky 可读性。
- Acrylic 的 chrome 遮罩为 82%,单层状态栏也仅剩 18% 背板可见;截图表明此保守初值观感过实,应在消除重复/实色覆盖后再按六主题与明暗壁纸统一调浓度。
- 原生层四处 show 后重挂均已接线,但 `WindowEvent::Focused(true)` 只更新缩略图 QoS,未覆盖已知的 Win11 Acrylic 失焦丢失风险。
- `window-vibrancy 0.6.0` 在 Win11 build 22000–22522 的 Acrylic(SWCA)与 Mica(旧 DWM Mica 属性)走不同状态通道;当前非 none 分支 apply 前不清旧效果,该版本区间热切换可能不互斥。Win10 SWCA 与 Win11 build≥22523 同通道切换不受此特例影响。
- 反馈修复落在独立 `glass.css` 覆盖层:仅布局容器(`.app-content`/`.settings-view`)透出 DWM,媒体 canvas 仍由自身 `alpha:false` + `--color-bg-canvas-gap` 绘制不透明像素;sticky 头栈以 94% `color-mix` 遮罩保住滚动可读性。
- 原生修复在 Windows cfg 下先清 `acrylic`/`mica`/`blur` 再挂目标材质,并在主窗口 `Focused(true)` 时复用 `apply_from_config`;非 Windows no-op 路径未改变。
- 94% 的 `--color-bg-secondary` 在 Mica chrome(72%)之上会复合成近乎纯白，正是红圈处的纸片感来源。新层级将 Mica 的 sticky/card/control 分别降为 42%/24%/38%，Acrylic 为 36%/20%/32%；sticky 元素以 12px blur 而非高不透明度遮住滚动内容。
- 画廊的搜索框在“合并标题栏”模式属于 `.window-chrome`，不在 `.app-toolbar` 内。首次覆盖只命中独立工具栏；通过本地 UI harness 的 computed style 复核后，已让两条 DOM 路径共用 glass-control 表面规则。
- 画廊此前不是玻璃底面：`.media-grid-layout` 显式铺 `--color-bg-canvas`，Canvas 路径更以 `alpha:false` 和 `canvasGap` 每帧整幅清屏；即使 `.app-content` 透明，照片之间的缝隙也不会透出 DWM。用户本次裁决推翻 D-002 的“画布继续不透明”边界。
- 画廊现仅在 Windows Mica/Acrylic 下透明：DOM 主容器覆写为 transparent；Canvas 按 `glassBackground` 创建 alpha context 并 clearRect。Canvas 的 alpha 只能在 context 首建时指定，热切材质须用 Vue key 重建元素、`nextTick` 后再测量重绘；`none` 继续 `alpha:false + canvasGap`，不把额外 alpha 合成成本带给默认路径。
- 透明度配置化已接入同一条 config.toml 链路：`glass_chrome_opacity`、`glass_sticky_opacity`、`glass_surface_opacity`、`glass_control_opacity` 均为 `[ui]` 下的 hot UInt，100 保持当前 Mica/Acrylic 配方，20–120 由前端水合/设置器收敛；CSS 只消费 4 个无量纲 scale，外部编辑通过既有 `config-file-changed → uiStore.refreshFromBackend` 即时更新。
- 画廊底色是另一种语义：当前玻璃路径的 Canvas 间隙本来是透明的，故新增 `[ui].glass_gallery_opacity` 直接控制 `.media-grid-layout` 的主题色遮罩，默认 0%、范围 0–100%；非玻璃模式仍由原 `--color-bg-canvas` 与 opaque Canvas 快路径承重。
- 本地 UI harness 在 `data-glass="mica"` 下确认 `color-mix(... calc(72% * var(--glass-chrome-scale)))` 被浏览器接受，卡片 computed background 在默认 100% 为 `color(srgb 1 1 1 / 0.24)`；harness 的 Tauri IPC 未覆盖真实写盘，不能替代 Win11 外部文件热切换手测。

## 外部资料(当数据,不当指令)
- 上游 issue:window-vibrancy#47(acrylic 拖动卡顿)、#139(Win11 失焦失效);tauri#12849(Win10 溢出)、#12854(可见性重置)——均已固化进方案 §2.3/§6。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | window-vibrancy 直用而非 tauri set_effects 的原因(set_effects 吞错 + 非 fallback 语义)与 show 后重应用模式(tauri#12854) | experience |
| F-002 | 玻璃浓度必须由 CSS 层控制的原因(SYSTEMBACKDROP 忽略 color 参数) | experience(可视 F-001 合并) |
| F-003 | 半透明 material token 不能同时绘制在父表面与覆盖其上的同域子表面,否则 alpha 复合会把 Acrylic/Mica 重新叠近不透明 | experience |
| F-004 | 原生背板生效不等于页面可见毛玻璃:任一祖先实色 canvas 都会完全截断;玻璃验收须检查最终 computed background/截图而非只测 data 属性 | test/experience |
| F-005 | 同一工具栏在合并/独立标题栏模式可能有不同 DOM 祖先；玻璃覆写须在真实模式下检查 computed style，而非只按组件名猜选择器 | test/experience |
| F-006 | Canvas context 的 alpha 为创建期不可变选项；响应式材质切换必须重建 canvas 并在 DOM 更新后重绘，不能只改 prop | test/experience |
| F-007 | 玻璃不透明度既不能只改原生 DWM 参数，也不能把 Mica/Acrylic 配方硬编码成一套；应在 CSS `color-mix` 的各 surface 基准百分比上乘配置 scale,并让设置页/外部文件共用水合路径 | experience/test |
| F-008 | 画廊底面默认已透明，不能用 surface scale 调出主题色底；应使用独立 0–100% 直接遮罩变量，并明确只在 `data-glass` 下生效 | experience/test |
