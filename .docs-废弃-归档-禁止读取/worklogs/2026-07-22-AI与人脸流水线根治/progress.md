---
status: snapshot
type: working-memory
line: AI与人脸流水线根治
created: 2026-07-22
---

# 进度日志:AI与人脸流水线根治

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:阶段6 完成(全量门禁绿+真机重跑落地);阶段0/1/2/3/4/5 均已完成(五 commit f3939a5/874f511/acdca33/e670369+eef2412)
- 未解错误:无功能性红;clippy 1 条本线新增警告未代修(见下,需 COM Send/Sync 裁决)
- 关键指针:D-401/D-402(task_plan 决策表)、F-029(video-worker 二期);栈证据=scratchpad/hang_stacks2.txt;fixture waitingOn 缺口已主线补(ipcFixtures.ts)
- **新发现待裁**:真机重跑用的两个"毒 mkv"从未触发 e670369 的 30s 超时守护——`MF_VIDEO_EXTS`(media_foundation.rs:43)有意排除 mkv/webm/flv/ogv(既有设计,非本线引入),mkv 在 `can_handle` 阶段即以 `UnsupportedFormat` 拒绝,从未进入异步 ReadSample 超时代码路径。本线核心修复(超时+硬解槽泄漏隔离)**未被本次真机验证实际 exercise**。
- 待办:⏸GUI 真机手测(AI/face 点开始跑通验证解锁生产 + 等待原因 UI 显示 + `--rotation-check`);裁决是否需换用 MF 支持格式的真损坏文件重跑以真正验证超时路径;clippy 新警告(Arc<CallbackShared> 非 Send+Sync)是否需要显式 unsafe impl 裁决

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:运行时栈取证链一次打通(WinDbg cdb `-p <pid> -y target\debug -c "~*kb 30; qd"`,符号全解析);复核随批走全程三批 0 严重;挂死修复(异步化)与防线(超时/看门狗/orphan_count)分层落地互不阻塞。
- 教训:①挂死类故障静态假说性价比极低——五假说(diff 回归/AB-BA 死锁/坏盘/thumb 积压/permit 泄漏)全倒,栈才是终审,先抓栈再推理;②无头 debug exe 不跑派生/exotic(前端 ready 触发),复现必须 tauri dev;③子代理「起后台 monitor 后自停」模式两度卡壳,须 SendMessage 催醒;④phase-closer「进程清干净」断言不实(残留 cargo+node×4),收尾杀进程须主线复核;⑤回执有损压缩会产伪结论(investigator「嵌套持锁」系行号顺序误判),裁决必亲见作用域。
- 意外:①mkv 非毒源——`MF_VIDEO_EXTS` 白名单一直拦着它,从不进 MF;「完成数为零」是白名单的正常结果,误当毒源指纹;②异步回调化本身即修复主体(错误经 OnReadSample 正常送达),30s 超时护栏真机零触发;③b216dcc/eef2412 与挂死纯时间相关非因果(12:29 旧 build 已挂)。

## 会话:2026-07-22
- 做了:立项建三件套;阶段0 静态取证收束(三假说证伪、时间线反转、挂死批画像,详 findings);阶段2 cargo build -p ai-worker exit 0 产物落位
- 验证:DB 只读查询×5(python sqlite3 mode=ro)+ 日志 grep×4;ai-worker.exe 12.9MB @18:25
- 遗留:阶段0 抓栈裁决、阶段1/3/4/5/6 全部

### 阶段6:全量门禁 + 真机重跑验证(收口)
**A. 全量门禁(dev 分支,含本线五 commit)**
- `cargo test --workspace --quiet`:exit 0,970 passed / 0 failed / 8 ignored(21 crate)
- `cargo clippy --workspace --quiet`:exit 0,1 条**本线新增**警告——`media_foundation.rs:439` `Arc<CallbackShared>` 非 `Send`+`Sync`(clippy::arc_with_non_send_sync)。`CallbackShared` 由 e670369 新引入(异步回调超时化),确认属本线;修复需判断 `IMFSample`(COM 接口)跨线程 Send/Sync 是否真正安全——属正确性裁决而非机械改动,超出 ≤5 行机械修复范畴,未代修,原样记录。
- `npx vitest run --reporter=dot`:exit 0,1345 passed(109 files)
- `npx vue-tsc --noEmit`:exit 0,无输出
- 结论:功能性门禁全绿;1 条 clippy 新红待裁(见上)。

**B. 真机重跑验证**
- DB 基线(`npm run tauri dev` 启动前):`status=1(pending)` 共 125,与预期一致
- 20:11:44 新 session 启动;20:11:46.801 `Recovered 125 orphaned derivations (processing → pending)`
- 20:11:52.657 `Derivation pipeline completed` `elapsed_ms=5856`(仅 5.9s,writer `done=114 error=11`)——远快于预期的"两 8GB mkv 各 30s 超时"窗口,原因见下
- DB 终态:`status=1` → **0**(归零);`status=2(done)` 2519→2633(+114);`status=3(error)` 477→488(+11)
- 逐行核实(`updated_at` 落在本 session 窗口的 125 行):`video_cover done=112/error=11`、`audio_cover done=1`、`doc_thumb done=1`
- 11 条 `video_cover` error 拆解:
  - 2 条(item 131407/131408,即两个"毒 mkv")= `Unsupported format: mkv` —— **非预期的 30s 超时**。核实为 `backend_for()`→`MediaFoundationBackend::can_handle()` 在 `MF_VIDEO_EXTS` 白名单阶段即拒绝 mkv(media_foundation.rs:43 注释:"mkv/webm/flv/ogv are intentionally excluded — those need FFmpeg (Perf variant)",既有设计,非本线引入)。这两个毒文件**从未进入 e670369 新增的异步 ReadSample 30s 超时守护代码路径**。
  - 9 条(其余 item,均 MF 支持格式)= `OS error: Media Foundation: 未指定的错误 (0x80004005)`,与本线超时修复无关的既有/正常错误。
- `grep -i "read sample timeout|leak|泄漏"` 全日志:**0 命中**(超时守护路径本次真机验证未被实际触发,与上一条互证)
- `grep "yield blockers cleared|让步解除"`(AI/face producer 让步解除 info,阶段3 新日志):**0 命中** → 记「AI/face 跑通需用户 GUI 点开始终验」
- panic 检查:0 处真实 panic(2 处命中均为 `span_close` 的 `panicked=false` 元数据字段)
- 进程清理:`taskkill /F /IM scrollery.exe` + `/IM ai-worker.exe` 成功;npm/node 包装进程随之退出,`ps aux` 复核无残留

**结论**:阶段1(MF 超时化)代码已落地、门禁绿、无回归,但本次真机重跑因测试素材(mkv)命中既有的格式白名单前置拒绝,**未实际验证到 30s 超时 + 硬解槽泄漏隔离这条核心路径**。需裁决是否换用 MF 支持格式(如损坏的 .mp4)的毒文件重新真机验证。
- 遗留:⏸GUI 真机手测清单(AI/face 点开始跑通验证解锁生产 + 等待原因 UI 显示 + `--rotation-check`);裁决项见前情段「新发现待裁」

### 收口(2026-07-22)
- 用户确认「测试通过」(GUI 真机手测过)并明示收口。clippy 警告已清(3f15308 unsafe impl Send,MTA 论证);「超时路径未真机 exercise」已裁为已知验证边界(findings 阶段6);TimelineScrubberCanvas.vue 工作区改动确认非本线,未吸收。
- 六 commit:f3939a5 / 874f511 / acdca33 / e670369 / 3f15308 / 785e3d1(todo 登记);候选处置见 closeout.md。
