---
status: active
type: working-memory
line: 自定义ICC与色域切换
created: 2026-07-23
---

# 进度日志:自定义ICC与色域切换

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:A/B 双批入库(0d677d3 + 0e1aa8b),阶段 7 全量门禁已跑:本线面(viewer_color/editing/thumbnail/engine/ipc/error/state/schema/前端)零红。GUI 手测竞态修已入库(92d8396),GUI 清单①需真机复测。新批「查看器底栏色域切换菜单」已入库(c9618f6),视频色域裁不支持(image-only);GUI 待测清单并入底栏菜单切换四 target+自定义列表空/非空两态+竞态修复复测一项。
- 未解错误:并行 OCR 线在途改动导致 clippy 与 ai-worker cargo check 两处外线红(exotic_protocol OCR 符号缺失),原样记录不修,归主线。
- 关键指针:D-410..D-420(task_plan 决策表)+ attachments/plan-B.md 头部主线裁决 + commit 0d677d3(A 批)/0e1aa8b(B 批)。
- 待用户:真机 GUI 验收(见下清单)、批 push、收口指令;并行 OCR 线在途勿混。

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-23
- 做了:评估定案(A/B/C 三档)→ 用户裁 A+B 开工;建三件套,D-410..D-414 拍板,派 S1(scout)/S2/S3(gp-haiku)三路摸底。A 批全落地+复核+修复,commit 0d677d3(施工 sonnet ≈8 轮;复核 opus 1 警告 3 建议;修复 builder+主线 import;增量核验过)。B 线计划(architect)已批入 attachments/plan-B.md,组 F/组 R 施工在飞。
- 验证:cargo check 退出 0;cargo test --lib engine::image_rs 4 passed;editing::color 9 passed(A 批涉改面;全量归阶段 7)。
- 遗留:阶段 5/6/7

## 会话:2026-07-23(B 批收尾,阶段 5-7)
- 做了:B 批(rust ∥ 前端)commit 0e1aa8b(+2091/-67,28 文件)——R1(sonnet)33 轮停机收束→R2(opus)34 轮接盘,F(sonnet)14 轮,F/R 文件域不相交并行;F 复核 opus(2警告2建议1存疑)+ R 复核 opus(3警告1建议1存疑),两修复批(sonnet)全落,增量核验双双通过,同入 0e1aa8b。补裁 D-418/D-419/D-420。阶段 7 批末全量门禁执行。
- 门禁结果(quiet,均为实测):
  - `cargo test -q`(src-tauri):退出 0;946 passed / 6 ignored / 0 failed。
  - `cargo clippy --lib -- -D warnings`(src-tauri):退出 101;仅 exotic/validate.rs 两处(`exotic_protocol::OcrItem/OcrItemResult/OcrLine` 未解析导入 + 1 处 unused import),属并行 OCR 线在途改动,外线面,原样记录不修。
  - `npx vitest run --reporter=dot`:退出 0;115 test files / 1397 tests passed。
  - `npx vue-tsc --noEmit`:退出 0;空输出。
  - `cd crates/exotic-workers/ai-worker && cargo check`:退出 101;main.rs `OcrSessionReadyBody` 未解析 + `handle_request` 签名不匹配 + `RequestBody` match 未穷尽(OcrSessionInit/OcrSessionClose/OcrBatch),同源 OCR 线,外线面,原样记录不修。
  - 结论:本线面(viewer_color/editing/thumbnail/engine/ipc/error/state/schema/前端)零红;两处红均可归属并行 OCR 线,非本批引入。
- GUI 手测清单(not automated):
  ① 四 target(sRGB/Display P3/DCI-P3/自定义 ICC)切换看大图换源与色变。
  ② 自定义 ICC 导入(合法/坏文件/超16MB/Gray)与删除(含删当前选中→复位 srgb)。
  ③ 设置页缓存统计 viewer_color 行。
  ④ 移动端构建锁 sRGB(无真机则 cfg 单测+编译代偿已有)。
  ⑤ WebView2 嵌 P3 ICC 显色对拍(§6-1)。
- 遗留:真机 GUI 验收(上述清单)、批 push、收口指令;A2/B2 见 task_plan 遗留节。

## 会话:2026-07-23(GUI 手测竞态修)
- 做了:用户真机手测报 GUI 清单①缺陷——打开大图预览切换色域偶发不生效,关闭重开再切才有效。根因定位:configStore setViewerColorTarget/setViewerColorCustomId 原先「先改本地 state、后 await saveConfig」,本地 state 一变 useViewerColorSource watch 立即发 GET_VIEWER_COLOR_URL,而后端(viewer_color_commands.rs:55)从 ConfigManager 单源读 target(D-411 不收前端传参)——渲染 IPC 与 set_config 持久化竞速,渲染先到即按旧 target 渲染返回旧 URL,画面无变化。修:两 setter 翻转为先 await saveConfig 再改本地 state + 次序契约注释(configStore.ts:232-243),commit 92d8396。
- 验证:`vue-tsc --noEmit` 退出 0;`vitest useViewerColorSource.spec.ts` 8 passed 退出 0;cavecrew-reviewer 快扫无发现。
- 遗留:真机复测色域切换(本修后)+ 原 GUI 清单其余项(②③④⑤)。

## 会话:2026-07-23(查看器底栏色域切换菜单)
- 做了:用户裁两点——①大图查看器下方加色域切换模块,已落地;②视频可否切色域,主线答复不支持且短期不做(派生渲染=全片转码不可行;WebGL 逐帧 LUT 属独立大线须另立项),查看器色域钉 image-only。新组件 src/components/media/ViewerColorMenu.vue(UiIconButton+UiPopover,三固定 target + LIST_ICC_PROFILES 现取自定义列表,空列表灰字提示);ContentViewer.vue 底栏左组 OCR 钮后插入,仅 image 且 !isMobilePlatform(D-414);i18n 双语加 detail.colorGamut/colorGamutNoCustom;选自定义先 customId 后 target(次序契约);取数令牌丢迟到响应+卸载兜底(复核 2🟡 修复,增量核验通过)。commit c9618f6。
- 验证:`vue-tsc --noEmit` 退出 0;eslint 四涉改文件退出 0;`npx vitest run` 全量 117 files / 1414 tests passed 退出 0;cavecrew-reviewer 复核 2🟡 全修+增量核验通过。
- 遗留:真机手测:底栏菜单切换四 target+自定义列表空/非空两态+竞态修复复测;视频色域如需另立项(WebGL 逐帧 LUT 评估先行)。
