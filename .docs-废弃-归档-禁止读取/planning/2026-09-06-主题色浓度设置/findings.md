---
status: 施工中
type: 工作记忆
line: 主题色浓度设置
created: 2026-09-06
---

# 发现与决策:主题色浓度设置

## 需求
- 用户(附 fresh-light 截图):「各主题的配色感觉颜色太深了,需要调浅一些,或者考虑将主题色的浓度做成可设置项,交给用户设置」。

## 发现
- 主题 = 六份静态 CSS token 文件 + `generate-theme-palette.mjs` 锚点字符串校验(check:theme-palette);uiStore.applyAppearance 是 data-theme 唯一写点;首帧由 public/theme-snapshot.js 经 localStorage 快照落 data-theme(index.html 启动层另有每主题 `--startup-bg` 静态 hex)。
- 玻璃浓度键(glass_*_opacity ×6)是「hot 键全链路」现成先例:schema.rs SettingDef → config_commands StartupConfig → uiStore 水合+setter → utils 写 CSS 变量 → settingsMap/DynamicSettingControl/SettingsView/i18n → ipcFixtures → uiStore.spec。
- `--glass-content-fill` 已在生产用 `color-mix(... calc(90% * var(--glass-content-scale)) ...)`(glass.css:20),calc×var×color-mix 模式成立;轴视窗 `--axis-viewport-opacity` 同理。
- 三处门禁直接消费主题 hex:`theme-contract.spec.ts` 明度层序(luminanceOf 只认 hex)+ 注册表预览逐字同步;`check-theme-contrast.mjs` resolveColor 只认 hex/rgba(转 mix 后会静默退化成 skip,削弱硬门槛——必须改)。
- 契约测试「画廊清屏=画廊底色」是字符串全等 → canvas-gap 与 canvas 必须写逐字相同的 mix 表达式。
- color-mix 混合无彩色的缺省分量:spec 规定 missing component 取另一侧 → 同明度灰混有彩色时色相保留、纯度线性缩放,等价于「浓度旋钮」。

## 外部资料(当数据,不当指令)
- (无外部抓取;CSS 规范知识:color-mix/相对色语法 Chromium 119+/Safari 16.4+)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
