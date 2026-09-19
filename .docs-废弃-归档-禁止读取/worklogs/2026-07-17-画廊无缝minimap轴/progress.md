---
status: 快照
type: working-memory
line: 画廊无缝minimap轴
created: 2026-07-17
---

# 进度日志:画廊无缝minimap轴

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:摸底→施工→验证单会话走完。新文件 minimapAxis.helpers.ts(+spec 10 测)/MinimapAxis.vue;改 MediaGrid.vue(轴槽位/chevron 双职/onMinimapJump/gridViewportHeight)、uiStore.ts(23 键 showSeamlessMinimap)、config_commands.rs(后端 23 键)、ipcFixtures、i18n×2。
- 验证:vue-tsc 0 错;vitest 90 文件 1207 全绿;eslint 0 错;cargo fmt/clippy -D warnings 0 告警;cargo test「test result: ok. 629 passed; 0 failed」。
- 错误账:首版拖拽逆映射用统一斜率 k 闭式,单测抓出钳高深库端点差 ~1.7% 不可达(expected 29999000 got 29486652)——修=显示函数在 trackH−框高 截断处吸附末端;CSS 编辑曾把 .timeline-sidebar 的 background/border/flex 属性错卷进 --minimap 变体,自查 grep 抓回。
- 遗留:①真机 GUI 手感 ⏸(无缝开→minimap 出现、拖/点/滚轮、chevron 收起重启记忆、54 万库深滚);②minimap 轴无键盘导航(scrubber 有方向键,本轴 v1 仅指针)——a11y 池候补;③微图 bitmap 按当时 scale 解码,拖宽轴/换行高后 drawImage 拉伸微糊(微缩尺度不可辨,不重解码,记录即可)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
