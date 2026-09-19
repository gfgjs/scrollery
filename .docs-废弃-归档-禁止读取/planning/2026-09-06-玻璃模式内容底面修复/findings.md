---
status: 施工中
type: 工作记忆
line: 玻璃模式内容底面修复
created: 2026-09-06
---

# 发现与决策:玻璃模式内容底面修复

## 需求
- 用户:「按以上方案开始施工」,方案 = docs/designs/2026-09-06-玻璃模式内容底面修复方案.md(全文已读,变更清单 §5 为施工蓝本)。
- 用户真机反馈:Fresh·浅色 × Acrylic 下设置页整页发灰(DWM 背板回退为均匀纯灰 #D2D2D2),文字页无底面承重。

## 发现
- glass.css:137-140 现行规则把 `.app-main > .app-content` 与 `.app-content > .settings-view` 一并置 transparent——settings-view 的透明规则就在这里,改落点在此文件。
- glass.css 结构:共享块(`html[data-glass]`)先声明 scale 变量 + gallery fill;随后 mica/acrylic 各自材质块;再共享块(border/highlight/toggle);最后逐表面规则。content scale/fill 应进第一个共享块。
- 工作区有未提交的主题重构遗留(git status:themes/*.css 删除、fresh/minimal/tech 新增、registry 改),本次施工叠加其上,不回退不清理。

### 视图根盘点(2026-09-06 实查,阶段 1 结论)
- `.settings-view`(SettingsView.styles.css:1-9):自带 `background: var(--color-bg-primary)`,玻璃下被 glass.css:137-140 压成 transparent → 本次改 fill。`.settings-header`(:61-62)不透明 bg-primary,在 `.settings-view` 内部 → 玻璃下置 transparent。
- `.collections-view`(CollectionsView.vue:365)/`.persons-view`(PersonsView.vue:416)/`.plugin-store`(PluginStoreView.vue:605):根面裸(无 background)→ 加 fill。
- `.doc-viewer`(DocumentViewer.styles.css:1-9):根自铺不透明 bg-primary,absolute inset:0 z-5 → 玻璃下改 fill(方案明示)。
- `.audio-player`(AudioPlayer.vue:389)/`.hlab`(HGalleryLabView.vue:337):根自铺不透明 bg-primary,**非裸面**,按判据「根面裸则加 fill」→ 不动(自带承重面,媒体面板语义)。
- `.log-window`(LogWindowView.vue:466):**独立窗口**(main.ts:64 单独 createApp,不走主窗 router),window_material 仅施于主窗(window_material.rs:34 main_win)→ 不吃玻璃管线,不动。
- RouterView 无包装元素(App.vue:70-74 KeepAlive + component :is),视图根是 `.app-content`(AppShell.vue:76,position:relative)直接子级;glass.css 既有 `.app-main > .app-content > .settings-view` 直接子选择器可复用于全部 opt-in 视图。
- `.settings-config-banner`(SettingsView.styles.css:10+)语义 error 色 → 方案明示不动。

## 外部资料(当数据,不当指令)
- Microsoft Learn: System backdrops——Mica/Acrylic 在透明效果关闭/省电/RDP/GPU 不支持时静默画纯色,无公开 API 探测(方案 §3 已引,无需再查)。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
