---
id: 2026-07-12-realtest-checklist-S1
title: S1 UI primitives 真机验收清单
status: active
type: acceptance
created: 2026-07-12
acceptance: 待真机验收
snapshot_date: 2026-07-12
scope: S1 阶段全部 primitive 化 / 迁移改动（截至 2753777）
---

# S1 UI primitives 真机验收清单

> 🔀 **本清单已并入 `realtest-round10-2026-07-15-合并验收.md`，验收以后者为准**（2026-07-15）。
> round10 把本清单与 round3–round8 去重合并为一次启动可走完的顺序；本文件保留作来源与理由存档。

> **用途**：S1 阶段的原语化与迁移全部已过**本地门禁**（typecheck / lint / vitest SSR 契约 / build / 对比度 / diff --check），但**尚无真机视觉验收**。本清单只列**门禁无法覆盖、唯真机能验**的维度——像素级视觉 parity、DOM 运行期交互（焦点陷阱 / 弹层锚定 / 动画 / hover·active 态）、跨主题渲染；不重复门禁已保证的结构 / 类型 / 属性。
>
> **验收方法**：`npm run tauri dev` 起真机（非浏览器 harness——浏览器无法验 Tauri IPC 依赖的对话框数据流与真实字体度量）。逐项勾选；任何一项异常，记「站点 + 现象 + 期望」回报即可，我据此定位。
>
> **判据基线**：绝大多数迁移是**同构**（构造即视觉等价，见各项标注），真机预期是「与迁移前逐像素一致」；**少数标 ⚠️ 的项**是有门禁盲区的重点，优先测。

---

## ⚠️ 最高优先级（有门禁盲区，先测这些）

- [ ] **UiSelect 紧凑面板 select 尺寸**〔2753777，`:deep()` 重锚〕：设置页里**钉住区 / 紧凑设置面板**内的下拉框（`compact-select-wrap`，如缩略图尺寸等 `control:'select'` 项），应保持 **height 26px、padding 4px 24px 4px 8px、font 12px** 的紧凑尺寸，**不能变回默认大尺寸**。这是本阶段唯一「computed style 无门禁可验」的点——`:deep(.select)` 重锚的推理等价需真机最终确认。对照非紧凑区的普通设置下拉，二者尺寸应有明显区分。
- [ ] **AppToolbar 筛选 / 视图弹层锚定**〔937ff03，UiIconButton `defineExpose({el})`〕：点顶栏「筛选」「视图」两个图标按钮，弹层应**精确锚定在按钮下方**（不偏移、不跑到左上角）。此处 ref 从 DOM 元素改为经 `defineExpose` 暴露的 `.el`，`getBoundingClientRect()` 定位链需真机验证不失准。
- [ ] **此前无焦点陷阱的两对话框**〔8f78174 / c7c02d4〕：**FolderCreateDialog**、**FolderTreeSelectorDialog** 打开后按 **Tab** 连续切，焦点应在对话框内**循环不逃逸**到背景页面（迁移前这两个漏了焦点陷阱，是本次顺带修的真实缺陷）。特别测 **FolderTreeSelector 内再打开 FolderCreate** 的嵌套场景——两层各自 Teleport，焦点应落在最上层。

---

## A. Settings 设置页（覆盖 UiButton / UiIconButton compact / UiToggle / UiSelect / danger zone / UiField）

- [ ] **UiButton**〔e8a7e3e〕：设置页 5 个按钮外观、hover、disabled、loading 态与迁移前一致。
- [ ] **UiToggle 开关面**〔ec8e964〕：所有 `control:'toggle'` 设置项的开关——选中/未选颜色、滑块滑动动画、focus 环，逐一与迁移前一致。`compact-toggle`（缩放态）的开关尺寸正确。
- [ ] **UiSelect 普通下拉**〔2753777〕：非紧凑区 `control:'select'` 下拉，`::after` 箭头位置、宽度、选项文案（i18n）正确；展开选择后值正确回写。
- [ ] **UiIconButton compact/danger**〔5250087〕：DynamicSettingControl 的 compact danger 与 plain 图标按钮外观正确（危险色、尺寸）。
- [ ] **danger zone**〔c98ac93〕：清库 / 重置设置 / 清缩略图 / 清日志四项在独立 danger 分区、默认折叠、点击有 confirm；「清除缓存」是普通按钮（无损重载）不在 danger 区。
- [ ] **UiField — NetworkStorage**〔01278ec〕：网络存储表单字段的 label 关联、`--grow`/`--kind` 布局变体（宽窄）与迁移前一致。
- [ ] **设置搜索 / 分区导航 / advanced 分区**〔S4〕：搜索过滤、分区切换、advanced 展开正常。

## B. 对话框 7/7（UiDialog + 焦点陷阱 + UiCheckbox）

> 通用检查（每个对话框都验）：overlay 遮罩、内容居中、打开/关闭动画、**Esc 关闭**、**点遮罩关闭**（除显式禁用者）、**Tab 焦点循环不逃逸**、打开时初始焦点落在 `data-autofocus` 元素。

- [ ] **ConfirmDialog**：含「记住选择」**UiCheckbox**〔94a8c35〕——勾选框 accent 主题色（非浏览器默认蓝）、16px、label 间距 8px、点击可勾选。
- [ ] **CloseConfirmDialog**：同上 UiCheckbox，且勾选框上方 margin（`close-remember-mt`）保留。
- [ ] **FolderCreateDialog**〔8f78174〕：含 **UiField**〔b39b57b〕字段；焦点陷阱（见最高优先级）。
- [ ] **FolderTreeSelectorDialog**〔c7c02d4〕：树列表满幅（`bodyPadding='0'`）、max-width 500px、分栏页脚；嵌套 FolderCreate（见最高优先级）。
- [ ] **ExoticActivateDialog**〔6577d04〕：textarea 初始聚焦；激活在途（`gate.activating`）时三条关闭路径均被禁用守卫。
- [ ] **OnboardingWizard**〔6cfbe9b〕：自定义头部（副标题 + 步骤点）经 `#header` 插槽；3 步正文定高居中；**禁遮罩 / Esc 关闭**（应无法点外部或 Esc 关掉）；按钮 skip 色不再是被禁用的 tertiary。
- [ ] **FaceApprovalPanel**〔f8d7673〕：自定义头（标题 + 副标题 + X）；分组列表满高**可滚**（`maxHeight='84vh'`，头部固定不滚）；新增的 Esc 关闭生效。

## C. Gallery / Viewer 顶栏（UiIconButton 暗色浮层 + roving tabindex）

- [ ] **ContentViewer 暗色浮层图标按钮**〔377b73c〕：查看器内 zoom/rotate/LIVE/faces/favorite/explorer/info/关闭等——**白字在暗色浮层**上清晰；favorite 等 active 态强调色（`.detail-controls .btn-icon.active`）正确。
- [ ] **GalleryViewControls / FoldersSection / ToolsSection / SidebarFooter / ManagementSection**〔5250087〕：各图标按钮外观、hover、`:disabled`（如人脸四控）、扫描 toggle 的 active=isRunning 态正确。
- [ ] **roving tabindex**〔8f4f249〕：ContextualToolbar 整组只占一个 Tab 停靠位；组内 **←/→/Home/End** 移焦；disabled 项（如撤销/重做不可用时）被跳过。

## D. Doc / Reader 流程（UiField 迁移的另两处）

- [ ] **Proofread 校对面板**〔b39b57b〕：UiField 迁移后字段 label 关联、布局一致。
- [ ] **SemanticSearch 语义搜索**〔b39b57b〕：UiField range 控件（inline 朝向）label 关联、滑块布局一致。

## E. 横切项

- [ ] **transition 清理**〔74767eb〕：全局 `transition:all` 已改具体属性——各处 hover/态切换动画**无异常闪烁或缺失过渡**。
- [ ] **6 主题快查**〔S7 前哨〕：墨/素/宣/玄/黛/月白 逐一快切，上述原语（按钮/开关/下拉/勾选框/对话框）在每个主题下配色正常、无硬编码色残留导致的突兀。

---

## 不在本清单（已知边界，非「待测」）

- **UiField 余 5 消费者**（DocumentViewer / ReaderSettings / HGalleryLab / ReplacementPanel / SettingsView）：**未迁移**（判定非同构变更需真机 + 部分非 UiField 形态，见 task_plan 阶段7）——无迁移则无需验，属后续真机设计项。
- **UiCheckbox 余 5 checkbox 站点**（SettingsView 集合切换 / ReplacementPanel×2 / ReaderSettings 反序行 / HGalleryLab lab 工具）：**裁决 out-of-scope-by-design**（各具独立 bespoke 视觉/契约，非 remember-checkbox de-facto 形态，见 progress「UiCheckbox 迁移终态」），保持原样不迁——非待测。
- **UiPopover / UiToolbar**：**未开工**（无共享全局类，需发明定位抽象/库选型=决策）。
- **S2-b2/c2-3 路由化、S3 overflow/Search/Reader-Audio**：依赖产品决策或更大改动面，非 S1 primitive 验收范畴。
