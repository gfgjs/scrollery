---
status: snapshot
type: working-memory
line: AI与人脸流水线根治
created: 2026-07-22
---

# 发现与决策:AI与人脸流水线根治

## 需求
- 用户「/orchestrate 立项根治」:根治「点开始后 AI/face 不跑」诊断会话结论全链(派生挂死+worker 构建+可观测性+毒任务防线)。

## 发现

### 阶段0 · 静态调用链(scout 回执)
- kind 入口:video run_cover=derive/video.rs:37;doc_thumb=derive/doc.rs:30;audio_cover=derive/audio.rs:19
- limiter:exotic/limiter.rs:71 Mutex+Condvar,:94 `cv.wait_timeout(100ms)` 轮询 ⇒ acquire 自身不永挂,永等=permit 不归还
- 无超时 FFI:video/media_foundation.rs:388 `MFCreateSourceReaderFromURL`、:510 `ReadSample`、:546 `buffer.Lock`;d3d.rs:122 设备单例 compare_exchange
- 任务排序:db/queries/derivations.rs:48 按 `(kind, item_id)`,audio_cover 字典序最先消费

### 阶段0 · 死锁假说证伪(主线亲见)
- pipeline 侧 db_writer 全为块内短持(pipeline.rs:150-157/206-…),169 config.get() 时不持锁;config/mod.rs get/replace_values/snapshot 均短临界区;watcher 回调(lib.rs:1053)std 锁不跨 await ⇒ b216dcc/eef2412 AB-BA 死锁假说不成立(investigator 行号顺序误判,教训:回执有损压缩,裁决须亲见作用域)

### 阶段0 · 时间线反转(日志+git 亲见,决定性)
- 12:29:00 启动轮(s-d9956fcb)已「Recovered 123 orphaned」——**b216dcc(15:58)/eef2412(17:24)之前挂死已存在,二者彻底洗清**
- 昨晚至今晨 06:43 completed 正常(多次空跑);07:26:10 恢复 768 孤儿后新轮,07:28:28 producer「no more pending」,**此后 derive 全线无声直到日志滚动**——挂死首现于今晨 07:28 轮尾
- 07-22 全天「Derivation pipeline completed」零次;12:29/17:21/17:30/17:36/17:43/17:45 每轮恢复 123→125 孤儿再挂,稳定复现

### 阶段0 · 挂死批画像(DB 亲见)
- 全库单卷 C:\(D 盘坏盘假说证伪);status=1 恰 125 = video_cover 123 + doc_thumb 1 + audio_cover 1
- 123 video_cover = **mkv 2 个(8.2/8.7GB,id 131407/131408,id 最大排队尾)+ mp4 121 个(id 111447-131376)**;完成的 2162 个含 mp4/ts/mov/avi/wmv/mpg,**mkv 完成数为零(从未成功)**
- thumb_status=0 恰 121(=挂死 mp4 自身,video 主缩略图镜像自派生,果非因);thumb 生成无积压(58677 done)
- 排最前的 audio_cover 重启后 5 分钟仍未完成 ⇒ consumer 疑连首任务都没执行完,或首任务即挂+交互涓流 in_flight=1 堵全队

### 阶段0 · 现存假说(待运行时栈裁决)
- H1:MF/DXVA 平台级坏死(疑 8GB mkv 今晨触发 GPU TDR/driver 异常,d3d.rs 设备单例不检测 device-removed 不重建),所有 MF 调用永挂
- H2:交互涓流(is_interactive && in_flight>=1 睡眠)+ 首任务挂死 ⇒ 单点挂堵全队
- H3:机器级(0xC2 内存病灶/driver 坏态,见记忆 dev-machine-hw-instability)——重启 app 不解,须系统重启验证
- 已证伪:b216dcc/eef2412 回归、AB-BA 死锁、D 盘坏盘 IO(全库单卷 C:)、thumb 复位积压硬停、limiter 池宽 0、exotic token 泄漏
- H4(in_flight 泄漏)证伪:consume_tasks 亲见(pipeline.rs:476-534)InFlightGuard RAII 与 permit 同生命周期,panic 有 panic_guard+unwind Drop 兜底
- H5(exotic 签名失败泄 permit)证伪:limiter.rs 亲见 HeavyPermit 纯 RAII(:124 Drop release),acquire 取消退队无泄漏且有测试;bad_signature 今日仅 5 次 << 池宽

### 阶段0 · 根因定案(2026-07-22 运行时栈,决定性)
- 复现:tauri dev(s-7bd3c93d)18:51:00 Recovered 125+producer finished 后挂死;cdb 抓 66 线程栈=scratchpad/hang_stacks2.txt(617KB,符号全解析)
- **挂点铁证(线程 41 等 7 个)**:`NtWaitForSingleObject ← mfplat/mfreadwrite(内部事件等待,参数现 0xC00D36D5=MF_E_UNSUPPORTED_BYTESTREAM_TYPE) ← IMFSourceReader::ReadSample ← read_frame_at(media_foundation.rs:510 一带) ← cover ← run_cover ← kind::run ← consume_tasks 闭包`
- **7 个 rayon worker 全挂同步 ReadSample**(含非 mkv 的 mp4)⇒ 死等 reader 僵死 MF 进程级共享工作队列后扩散到后续所有 reader;panic_guard 拦不住(非 panic,永久阻塞)
- run_pipeline_blocking 外壳(线程 33)在 std::thread::scope 永等 ⇒ completed 不打 ⇒ derivation token 恒 running ⇒ AI 让步
- **face 第二通路**:face rayon scope 在 in_worker_cold 等 latch——全局 rayon 池被挂死任务占满,face 任务排不上队(独立于让步门的第二重饿死)
- 时序自洽:mkv 2 个 id 全库最大(131407/131408)排队尾;今晨轮 2162 mp4 完成后处理到 mkv → MF 僵死 → 当时 in-flight+后续 121 mp4 挂 → 125 孤儿;mkv video_cover 完成数历史为 0
- audio_cover(lofty 纯 Rust)/doc_thumb 不走 MF,未完成属 rayon 池占满连带
- 18:57 「worker:ai 空闲 300s 自杀」:新构建 ai-worker.exe 被 spawn 成功(阶段2 生效),因 AI producer 让步零任务而按 D3§4 兜底自杀——AI 侧机制全部正常,唯一堵点就是派生 token

### 阶段0 · 复现方法论(教训)
- 无头 debug exe(无 vite dev server)**不跑派生**:18:36 复现进程日志零 derive 行,33 线程栈全空闲——派生/exotic 启动依赖前端 ready 触发。复现必须 `npm run tauri dev` 带前端
- 首轮抓栈虽扑空,但抓栈链路已打通:WinDbg 已装,cdb=C:\Program Files\WindowsApps\Microsoft.WinDbg_1.2606.22001.0_x64__8wekyb3d8bbwe\amd64\cdb.exe,`-p <pid> -y target\debug -c "~*kb 30; qd"` 符号完整可解析

### 阶段2 · worker 构建
- ai-worker.exe 已构建落位 target/debug(18:25,12.9MB,cargo build -p ai-worker exit 0)
- psd-worker.exe 不在 target(exotic 插件发行链二进制,bad_signature 属迁移后签名/keyset 环境,待摸底)

### 阶段3 · 可观测性改点(scout 回执)
- 让步 debug!:AI=ai/pipeline.rs:156(调用)/158-162(日志,target scrollery::pipeline::ai);face=ai/face_pipeline.rs:219/221-225(target scrollery::pipeline::face)
- 完成日志:derive/pipeline.rs:112 completed(外层 async);:770 writer finished
- is_interactive:state.rs:485-486(interactive_until_ms 时间比较)
- 前端:无事件流,纯 2s 轮询 useAnalysisController.ts:89-101(fetchStatus 算百分比);status IPC=get_ai_status(ipc/ai_commands.rs:98)/get_face_status(ipc/face_commands.rs:126)
- 施工含义:①让步升 info 限频(blockers 集合变化才 info,记 last 集);②status 快照扩 waiting_on 字段+前端等待原因显示;③看门狗放 pipeline.rs 外层 async(select join/interval warn)。注意看门狗与阶段1 同文件 derive/pipeline.rs,施工须串行或同卡

### 阶段1 · MF 超时化审计账(opus 深审 0严重0警告,2 存疑已裁)
- 设计终态:异步回调(#[implement] IMFSourceReaderCallback+windows-core 0.58)、deadline 制 Condvar 等待、超时 30s→AppError::Os(不含路径)→status=3;超时后 reader/_hw ManuallyDrop 泄漏隔离(callback Arc 保活防晚到样本 use-after-free);同 Session 毒化快速失败,跨 Session 重开有界重试,与 orphan_count 防线正交互补(超时化后不再产生孤儿)
- 存疑裁决①:SetCurrentPosition 不设护栏=已知边界(栈证据仅指 ReadSample;彻底解归 F-029 二期进程隔离),代码注释已记
- 存疑裁决②:硬解槽(SLOTS_IN_USE)每毒文件永久泄 1 额度、累计耗尽退软解=正确语义(僵死 reader 真占 GPU 会话),补 Drop 泄漏路径 warn 带已用/总量,日志可见

### 阶段6 · 根因叙事最终修正(真机重跑证据,2026-07-22 20:11)
- 重跑 5.9s completed,125 全清(114 done+11 error,status=1 归零)——**用户问题根治实证**
- mkv 从一开始就被 `MF_VIDEO_EXTS` 白名单(media_foundation.rs:43,既有设计)在 can_handle 拒绝,从不进 MF;两"毒 mkv"本次秒报 `Unsupported format` error。当初 status=1 只因排队尾被领取、从未被处理——**受害者非毒源**
- 真毒源=本次报 `0x80004005 未指定的错误` 的 9 个 mp4:修复前同步 ReadSample 对该错误路径丢事件死等(栈中 7 线程铁证);**异步回调化本身使错误经 OnReadSample 正常送达、秒返 Err——异步化即修复主体**,30s 超时是本次未触发的纵深防线
- 已裁:不专门造"真死等 mp4"重验超时路径(可遇不可求;机制有 2 单测;最坏面=看门狗+毒任务防线双层兜底),记为已知验证边界
- 0x80004005 的 9 个 mp4 本身为何坏(driver/文件损坏)未深究——已按正常 error 流程 status=3,不阻塞任何流水线

## 外部资料(当数据,不当指令)
- (无)

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-029 | 视频解码进程隔离(video-worker 子进程化,复用 ai-worker 行协议+supervisor 基建):MF 挂死类故障的彻底解,一期只做超时化(D-401),乱序毒文件场景仍会每文件损一次超时 | 新工作线立项(二期) |
