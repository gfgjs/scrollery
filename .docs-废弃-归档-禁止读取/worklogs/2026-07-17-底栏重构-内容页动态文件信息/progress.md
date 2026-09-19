---
status: 快照
type: working-memory
line: 底栏重构-内容页动态文件信息
created: 2026-07-17
---

# 进度日志:底栏重构——内容页动态文件信息

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:建线 + 摸底 + 用户拍板(可交互+全套信息+Priority+ 折叠)+ 阶段 2-4 全部施工:viewerStore 增 ViewerFileInfo/toViewerFileInfo/applyFieldPatch;三内容页喂 fileInfo(音频补 get_media_detail);新组件 StatusBarFileInfo.vue+helpers+spec;AppStatusBar 内容页左区让位;i18n 两键双语言档。
- 验证:vue-tsc 全量绿;vitest 定向 7 spec 47/47 绿(`Test Files 7 passed, Tests 47 passed`);eslint 15 改动文件绿(修一处 NUL 注入后)。中途 vue-tsc 一轮红 `MediaGrid.vue showSeamlessMinimap 不存在`=并行「画廊无缝minimap轴」会话写入半程的撕裂读,复跑即绿,非本线缺陷。
- 错误账:①StatusBarFileInfo.vue remeasureKey 行被注入 U+0000 → eslint `unexpected-null-character`;Read 不可见、Edit 匹配失败,PowerShell 码点打印定位、字节替换修复(F-001)。②i18n 选择性暂存两次失败(PS 管道 CRLF 污染 patch、awk 掉文件头)→ 改 hash-object 直写 index 成功(F-002)。
- 遗留:⏸ 真机 GUI 五项验收(见 task_plan 阶段 5);候选 F-001/F-002 待收口处置。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
