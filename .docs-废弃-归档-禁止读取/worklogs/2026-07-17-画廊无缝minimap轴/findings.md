---
status: 快照
type: working-memory
line: 画廊无缝minimap轴
created: 2026-07-17
---

# 发现与决策:画廊无缝minimap轴

## 需求
- 用户原话:「画廊无缝模式下右侧也可选显示/隐藏 minimap 轴,做成 vscode 那种缩略预览的形式」

## 发现
- 无缝分组(seamlessGroups)下后端不产分隔行 → layoutSummary.separators/monthBuckets 全空 → `canShowScrubber=false`(MediaGrid.vue:559)→ 整个 `.timeline-sidebar-wrapper` 连 chevron 钮一并隐藏(MediaGrid.vue:224/251)。uiStore.ts:289 注释早已把「无缝下时间轴失据」标为**留决策项**——本任务即该决策项的落地答案。
- 数据源现成:`mediaStore.fetchRowsByY(topY,bottomY)`(视口相交语义,mediaStore.ts:204)按任意逻辑 Y 区间拉 LayoutRow;LayoutRowItem 自带 x/w/h/placeholderColor(后端 thumbhash 预算均色 hex)/thumbStatus/thumbPath——minimap 色块层零额外后端改动。
- 微缩略图 URL:`buildThumbUrl(status,path,cacheDir)`(useThumbLoader.ts:37,纯函数可直接 import);解码缓存样板 `canvasThumbState.ts`——带字节预算 LRU + ImageBitmap 显式 close,MediaGridCanvas 在用,可实例化独立小预算副本。
- 交互样板:MediaScrollbar.vue——pointer capture 拖拽、轨道点击直达+无缝转拖、rAF 节流 emit('jump')、显形滞回 LINGER_MS=700;几何纯函数单测锁定(mediaScrollbar.helpers.ts,thumbGeometry/thumbTopToLogicalY round-trip)。
- Canvas DPR 样板:TimelineScrubberCanvas.vue:396-439(缓冲按 devicePixelRatio 放大 + setTransform 缩回 CSS 像素)。
- 配置持久化链:后端 `config_commands.rs:88-125` StartupConfig struct(现 22 键)+ get_startup_config 逐键读;前端 uiStore.ts:49 接口 + :514 批载分支 + ipcFixtures.ts:237 固件——加键须四处同步。
- 引擎参数:MediaGrid 已有 `currentLogicalY`(:788)、`scrollToY`(scrubber @jump 同一入口)、`media.totalHeight`;bucket 引擎(默认)与方案 A 双引擎统一走这三者,minimap 对引擎无感知(与 MediaScrollbar 同理由)。
- 快滚保护:`useThumbLoadGate`(setDeferThumbLoad)飞掠闸门全局共享——minimap 微图加载须挂同一闸门,停稳回填。

## 外部资料(当数据,不当指令)
- VSCode minimap 行为参照(常识,未抄代码):内容过长时 minimap 按比例滑窗(proportional slide),视口框半透明可拖,点击定位。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | uiStore.ts:289「无缝下 scrubber 失据留决策项」由本线落地为 minimap 轴——注释已在施工时同步改指终态(uiStore.ts seamlessGroups 段) | 已落地,收口裁 no-promotion 或验存 |
| F-002 | minimap 滑窗几何:框顶对 currentY 恒线性(斜率 k 闭式,滑动/非滑动统一),唯 MIN_SLIDER 钳高需「截断点吸附末端」补可达性——单测先抓到端点差 1.7% 才发现,纯推导会漏 | experience 候补(几何映射端点必单测,勿信推导) |
