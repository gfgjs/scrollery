---
status: 快照
type: working-memory
line: 画廊无缝minimap轴
created: 2026-07-17
---

# 任务计划:画廊无缝minimap轴

## 目标
画廊无缝模式(及不分组等 scrubber 失据场景)右侧提供可选显隐的 VSCode 式 minimap 轴:微缩内容预览 + 视口框拖拽/点击跳转,持久化显隐偏好。

## 当前阶段
阶段 1-6 施工全完(2026-07-17 单会话);余真机 GUI 手感验收 ⏸

## 阶段

### 阶段 1:摸底
- [x] 现轴结构(timeline-sidebar / canShowScrubber / MediaScrollbar)
- [x] 数据源(fetchRowsByY / LayoutRowItem.placeholderColor / buildThumbUrl)
- [x] 可复用件(canvasThumbState LRU / TimelineScrubberCanvas DPR 模式 / mediaScrollbar.helpers 几何)
- [x] 配置持久化模式(StartupConfig 22 键批载 + set_app_config)
- **状态:** complete

### 阶段 2:纯几何 helpers + 单测
- [x] minimapAxis.helpers.ts:滑窗映射(minimapScale/minimapWindow/minimapSlider/sliderTopToLogicalY/clickToLogicalY)
- [x] vitest 单测 10 项(镜像 mediaScrollbar.helpers.spec 样板,round-trip + 对齐不变量 + 钳高端点可达锁定)
- **状态:** complete

### 阶段 3:MinimapAxis.vue 组件
- [x] canvas 渲染:行块 placeholderColor 即时层 + 微缩略图回填层(解码期 resize 到绘制尺寸)
- [x] 行数据分块拉取(fetchRowsByY,8192px 块)+ layoutVersion 失效 + 远块修剪
- [x] 微图走 canvasThumbState LRU(2048 条/8MB 双约束)+ 飞掠闸门只画色块、放行回填
- [x] DPR 适配(镜像 TimelineScrubberCanvas)
- [x] 交互:视口框拖拽(pointer capture + rAF 节流)、轨道点击居中直达转拖、wheel 1:1 转发
- **状态:** complete

### 阶段 4:MediaGrid 集成 + 显隐
- [x] 挂 timeline-sidebar 槽位:canShowMinimap = totalRows>0 && !canShowScrubber;宽 --minimap-axis-width(96px)
- [x] chevron 收合钮双职(scrubber 管 showTimeline 会话态 / minimap 管持久开关)
- [x] onMinimapJump:复用 onScrollbarJump 拖拽链关闸变量 + scrollToY(y,false) 双引擎通吃;gridViewportHeight 由既有 RO 回填
- **状态:** complete

### 阶段 5:显隐持久化(第 23 键)
- [x] 后端 config_commands.rs StartupConfig + get_startup_config 加 seamless_minimap
- [x] 前端 uiStore StartupConfig 接口 + showSeamlessMinimap ref + setter + 批载分支(默认开,仅显式 'false' 隐)+ ipcFixtures
- [x] i18n zh/en toolbar.hideMinimap/showMinimap
- **状态:** complete

### 阶段 6:验证
- [x] vitest 90 文件 1207 全绿(含新 spec)+ vue-tsc 0 错 + eslint 0 错
- [x] cargo fmt --check / clippy -D warnings / test 629 过 0 败
- [ ] 真机 GUI 手感 ⏸(惯例入 GUI 池:无缝开→右轴出 minimap、拖拽/点击/滚轮、chevron 收起重启记忆)
- **状态:** complete(施工面);GUI ⏸

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| minimap 启用条件 = totalRows>0 && !canShowScrubber,非仅 seamlessGroups 开关 | 不分组(none)与无缝同病症(无分隔数据右轴全隐),一并覆盖;有 scrubber 时不与时间轴抢槽位 | D-001 |
| 滑窗映射用 VSCode proportional 模式:scale=miniW/containerWidth,窗顶随滚动比例平移 | 54 万库逻辑高数千万 px,全高 1:1 缩不可行;固定 scale 保证微图可辨 | |
| 渲染两层:placeholderColor 色块即时 + 微缩略图停稳回填 | 色块零 IO 逐帧可画;微图复用 canvasThumbState LRU(独立小字节预算)+ 飞掠闸门,不与主画廊抢解码 | D-002 |
| 显隐持久化为 app_config 第 23 键 seamless_minimap,默认 'true' | 用户点名「可选显示/隐藏」;默认开保可发现性,chevron 收起后记住 | |
| 轴宽独立 CSS var --minimap-axis-width 默认 96px | 时间轴 44px 放缩略预览太窄;不动共享的 --timeline-axis-width 语义 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
