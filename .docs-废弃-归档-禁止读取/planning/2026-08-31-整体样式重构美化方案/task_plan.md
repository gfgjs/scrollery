---
status: complete
type: 工作记忆
line: 整体样式重构美化
created: 2026-08-31
---

# 任务计划:整体样式重构美化

## 目标
对 Scrollery 前端做整体样式重构/美化:主题色、间距、组件观感大改,不拘泥现有设计与实现,以当下最优为首要目标。先完成带证据的现状审计与重建方案,用户明确接手后按 B0–B7 独立批次施工并完成自动验收。

## 当前阶段
阶段 11:B7 收口验收(已完成;真机 WebView2 项按环境保留手测清单)

## 阶段

### 阶段 1:现状审计与证据收集
- [x] 样式系统清单审计(token/配方/硬编码/密度统计)
- [x] 组件观感审计(外壳/工具栏/侧栏/设置/画廊/浮层)
- [x] 六主题视觉基线截图(capture:themes)
- [x] ui-ux-pro-max 设计系统检索(资产管理器方向)
- **状态:** complete

### 阶段 2:方案设计
- [x] 定设计方向与原则(色彩/材质/密度/圆角/动效)
- [x] 新 token 架构与主题重设计
- [x] 分区域改造蓝图与分批计划
- **状态:** complete

### 阶段 3:报告与方案交付
- [x] 设计文档落 docs/designs/
- [x] 向用户呈现报告+方案+待裁决项,并在接手后按推荐裁决施工
- **状态:** complete

### 阶段 4:B0 几何地基
- [x] 4px 基网间距 token 收敛,补齐 2xs/3xl
- [x] UI 字号阶梯收敛到 11/12/13/14/16/20/24,同步运行时设置与新装默认
- [x] 圆角、控件高度、状态栏、滚动条 token 收敛
- [x] 为运行时字号阶梯补回归测试
- **状态:** complete

### 阶段 5:B1 色板重构
- [x] 六主题 accent 五角色与状态 paired roles 落地
- [x] 扩展主题契约与对比度门禁
- [x] 统一徽章 canonical token 与 Canvas fallback
- [x] 静态锚点生成/校验器、主题预览同步、中性层顺序门禁
- **状态:** complete

### 阶段 6:B2 全局配方
- [x] index.css / material.css 全局控件与表面配方重写
- [x] Ui* 原语样式适配 28/32px 控件档
- [x] dialog/popover/selection/context/toast 统一 float recipe,slot surface 去重
- **状态:** complete

### 阶段 7:B3 外壳
- [x] 工具栏、状态栏、侧栏、沉浸入口统一 compact/quiet/active 配方
- [x] 标题栏/工具栏顺序、侧栏滚动条、状态栏高度与 divider 收敛
- **状态:** complete

### 阶段 8:B4 画廊
- [x] DOM 与 Canvas 徽标统一 scrim、类别点色、评分 amber 与尺寸/圆角 token
- [x] 日期分隔符、选中态、sticky、拖拽幽灵和筛选 chip 收敛
- [x] Canvas/DOM helper 回归与主题对比度门禁通过
- **状态:** complete

### 阶段 9:B5 设置+对话框
- [x] 设置页普通分节扁平化,危险/导入导出等保留独立表面
- [x] 设置导航、主题选择、Reader 设置、表单输入与对话框控件收敛
- [x] 窄内容列与 scoped 子组件迁移后的布局锚点回归
- **状态:** complete

### 阶段 10:B6 次视图
- [x] Duplicates/Persons/Collections/AudioPlayer/PluginStore/Log/Reader 观感跟进
- [x] 选择工具条、播放器、日志、阅读器、语义搜索、视频菜单等浮层/控件收敛
- [x] 硬编码色与状态前景按语义 token 回收;媒体内容/HUD/开发实验区保留在明确豁免边界
- **状态:** complete

### 阶段 11:B7 收口验收
- [x] 六主题 × gallery/settings/viewer 矩阵重新捕获 18/18
- [x] 全量 Vitest、typecheck、lint、build、contrast、theme-palette、verify:channel、Rust fmt、diff check 全通过
- [x] 抽查 Ink/Porcelain 的 gallery/settings/viewer 截图,无明显层级/布局断裂
- [x] 输出真实 Windows WebView2 手测清单;当前环境未自动化原生窗口、高 DPI、玻璃背板与 GPU Canvas 项
- **状态:** complete (自动验收);真机手测是外部环境项,不阻塞代码收口

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 初始阶段只做报告与方案,不动生产样式代码 | 用户先要求方案评审;接手后明确授权施工 | 已由 B0–B7 施工取代 |
| 与前序「清透材质化」第一批(已落地 caebb68)的关系:新方案为准,可推翻 | 用户授权「任何设计和实现都可被推翻」 | |
| 接手后采用方案中 D1/D2 等推荐裁决,先落地 B0 并保留 B1–B7 的批次边界 | 用户要求由本会话接手;B0 只改共享尺度契约,可独立回滚 | D-001 |
| 已有用户保存的字号/滚动条宽度继续视为显式偏好,只调整新装/缺省值与 CSS 基线 | 避免升级时覆盖用户设置;运行时仍由配置值优先 | D-002 |
| 媒体徽标 DOM/Canvas 实际重绘留到 B4,先在 B1 固化共享 token 与读取契约 | 徽标 painter、DOM 角标和评分星需同批改造并做亮/暗/高对比图片验收,避免半套迁移 | D-003 |
| 浮层 slot 不重复绘制 surface | UiPopover/UiDialog 负责外壳,业务 slot 只保留布局与内容,避免嵌套边框/阴影 | D-004 |
| 真机 GUI 不在当前自动化环境内 | headless Chrome 矩阵负责 CSS/布局/主题证据;WebView2、高 DPI、原生玻璃与 GPU 路径输出手测项 | D-005 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| `npm test -- --run ...` 被当前 npm 解析为未知 CLI flag | 试运行聚焦 Vitest | 改用 `npx vitest run ...`,聚焦测试通过 |
| `npm run dev -- --host ...` 被当前 npm 解析为未知配置参数 | 启动截图验收服务 | 改用等价的 `npx vite --host 127.0.0.1 --port 1420`,矩阵捕获正常 |

## 收口

2026-09-02: B0–B7 代码施工与自动验收完成。工作区仍保留用户/其他任务的既有未提交变更,本任务未执行提交、reset 或清理。

## B7 真机手测清单(目标设备执行)

- 启动 Tauri/WebView2，切换六主题并确认标题栏、工具栏、侧栏、设置页、画廊、阅读器的文字与 active/状态色保持可读。
- 在合并标题栏与独立标题栏模式分别检查窗口拖拽、三键区、搜索框与状态栏；在 125%/150% DPI 检查窄窗换行与控件点击区域。
- 打开/关闭 Dialog、Popover、ContextMenu、SelectionToolbar，检查焦点环、ESC 关闭、滚动边界与不重复叠加 surface。
- 在高密度画廊滚动、缩放、拖拽选择，核对 DOM/Canvas 徽标、日期 sticky、minimap 与拖拽幽灵；检查图片/视频/文档 viewer 往返及沉浸模式进出。
- 切换 Mica/Acrylic/none 玻璃材质与透明度，检查失焦/隐藏后恢复、画廊图片缝隙和 GPU Canvas；macOS/Android/iOS 做平台降级确认。
