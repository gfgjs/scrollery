---
id: 2026-07-24-视频格式扩展子系统-design
status: active
type: design
line: 视频格式扩展子系统
created: 2026-07-24
---

# 视频格式扩展子系统 · 架构与实施方案(video-worker)

> 来源:architect 代理方案回执全文,主线代写落盘(2026-07-24)。设计依据三份摸底文件(scratchpad/findings-*.md);§0 锚点均经 architect 读源码核对。

## §0 关键事实锚点(设计依据,均已核对现行代码)

| 事实 | 锚点 |
|---|---|
| Progress 帧已存在:静默限时重置 + 总上界 `PROGRESS_TOTAL_CAP=3600s` | `src-tauri/src/exotic/worker.rs:34-38, 250-267`;`crates/exotic-protocol/src/message.rs:573-586` |
| OCR/Enhance 先例:capability 只作通告、**不进 exotic 任务化调度**,host 侧 Service 直持 supervisor(D-OCR-7 豁免注记) | `crates/exotic-protocol/src/message.rs:312-316` |
| Catalog 红线:offering 声明的 format 撞 `classify_media_type` 常见格式 → **拒绝整个 Catalog**(`CommonFormatConflict`) | `src-tauri/src/exotic/catalog.rs:97-100`;17 个常见视频扩展名在 `src-tauri/src/utils/format.rs:158-178` |
| EnhanceSessionInit 先例:`work_dir` 输出白名单 + `output_tmp_path` 越界拒 | `crates/exotic-protocol/src/message.rs:107-136` |
| 派生 kind 注册表 + 通用流水线(status 0/1/2/3、claim/finish、kind_filter) | `src-tauri/src/derive/kind.rs:20-90`;`src-tauri/src/db/queries/derivations.rs:44-119` |
| FFmpeg 后端接缝已预留(`backend_for()` 返回 None → unsupported) | `src-tauri/src/video/mod.rs:91-103` |
| assetProtocol scope 运行时已含 cache_dir(递归)+ 扫描根 | `src-tauri/src/lib.rs:417-426`;`src-tauri/src/ipc/config_commands.rs:441-442` |
| 缩略图缓存池默认 10GB、LRU 驱逐 | `src-tauri/src/thumbnail/cache.rs:313-317`;`src-tauri/src/lib.rs:791` |
| builtin+free 直放行授权 | `src-tauri/src/exotic/mod.rs::availability_of`(findings-exotic-plugin §7);Release builtin 路径尚未接(H2b-prod 待办,`installer.rs:358-364`) |

## §1 路线裁决

**采纳业界主推路线(见 scratchpad/findings-industry.md §6),含两处修正。**

裁决:**remux 优先(容器转 fmp4,`-c copy`)+ 兜底一次性转码 H.264/AAC MP4,产物走派生缓存;同一 video-worker 兼供 FFmpeg 缩略图后端**。

- **采纳理由**:高频缺口是「伪不兼容」——mkv/avi/wmv/ts 内装 H.264/AAC,Chromium 能解 codec 只是不认容器。remux 秒级、无损、CPU 近零,契合「派生文件 tmp+同卷 rename」既有红线;资产管理场景是「看这个文件」不是直播流,一次性转码产物可缓存复用,比 HLS 实时会话简单一个量级。
- **修正 1(三级不是两级)**:remux 与全转码之间插入**半转码档**——视频流 webview 可解、仅音轨不支持(AC-3/DTS/E-AC-3/TrueHD,mkv 常见)时,`-c:v copy -c:a aac` 只转音轨。音轨转码速度≈几十倍实时,用户等待接近 remux 档,覆盖大量「有画面没声音/整个不播」场景。判定表见 §5。
- **修正 2(统一解两缺口)**:缩略图缺口(mkv/webm/flv/ogv 无 MF 后端,`video/mod.rs:100-102` 返回 None)**由同一 worker 解决**,不再走「feature `ffmpeg` 进程内链接、Perf-only」旧计划。方式:host 侧新增 `WorkerVideoBackend`(实现现有 `VideoBackend` trait,`video/mod.rs:54-84`),probe/cover/keyframes 三方法委托给 video-worker;`backend_for()` 在 MF 不认扩展名且插件可用时返回它。派生流水线(`video_cover`/`video_keyframes` kind)零改造,自动开始产出 mkv/webm 封面与雪碧图。

**同一 worker 兼供缩略图后端:是。** 理由:(a) 一份 FFmpeg 二进制、一次下载、一个 supervisor 实例;(b) 视频派生在流水线里本就串行,无并发放大;(c) 崩溃隔离与低优先级(`worker.rs:95-102` BELOW_NORMAL)天然复用;(d) 进程内链接 FFmpeg 的旧路线在 Windows 链接痛点大、崩溃不隔离、LGPL 动态链接合规更绕,worker 子进程方案全面占优(见 §11 已弃方案)。

## §2 Worker 边界与协议

### 2.1 定位:Service 型 worker(OCR/Enhance 先例),不进 exotic 任务化调度

**决定性约束**:mkv/webm/flv/ogv/avi/wmv 等全部是 `classify_media_type` 已认领的常见格式,catalog offering 若声明它们会触发 `CommonFormatConflict` 拒绝整个 Catalog(`catalog.rs:97-100`)。因此 video-worker 对常见格式的增强服务(remux/转码/缩略图)**不走 catalog format 认领**,走 OCR/Enhance 同款:host 侧 `VideoWorkerService` 直持 supervisor + worker client,以 `availability_of(VIDEO_PLUGIN_ID)` 的授权结论为总闸。catalog offering 仍存在(商店卡片 + 可选认领真 exotic 格式,见 §4)。

### 2.2 能力声明

`crates/exotic-protocol/src/message.rs:307-317` capability mod 追加(均带「不进 exotic 任务化、豁免一一对应」注记,同 OCR_TEXT/ENHANCE):

```
VIDEO_PROBE     = "video_probe"      // 流事实探测
VIDEO_REMUX     = "video_remux"      // 容器改封 fmp4(含仅音轨转码档)
VIDEO_TRANSCODE = "video_transcode"  // 一次性全转码 H.264/AAC MP4
VIDEO_FRAMES    = "video_frames"     // 封面帧 + 关键帧雪碧图(缩略图后端)
```

新 worker:`crates/exotic-workers/video-worker/`,`WORKER_ID="video-worker"`,Ready 帧四能力全报(与 psd/raw/enhance 同构,findings-exotic-plugin §4)。

### 2.3 消息扩展(v2 additive,不动 PROTOCOL_VERSION,同 OCR/Enhance 先例)

`RequestBody`(`message.rs:35-137`)追加五个 op:

- **`VideoSessionInit { session_id, ffmpeg_exe_path, ffmpeg_sha256, work_dir }`** — worker 校验 ffmpeg_exe_path 存在 + sha256 相符(防换包),运行 `ffmpeg -version` 读取版本与 configuration 行,**发现 `--enable-gpl` 立即回 terminal 失败**(许可运行时保险丝,§3.4);`work_dir` 为输出白名单前缀,语义与 `EnhanceSessionInit.work_dir`(`message.rs:115-118`)完全同型。响应 Success + `SuccessBody.video_session: { caps, ffmpeg_version }`。
- **`VideoSessionClose { session_id }`** — 同型样板。
- **`VideoProbe { session_id, source_path, input_fingerprint }`** — worker 用 `ffprobe`(随 shared 包)输出**流事实**,不做判定:容器名、时长、宽高、rotation、fps、bitrate、video codec/profile/位深/像素格式、音轨列表(codec/声道/语言/默认标记)、字幕轨存在性、HDR 元数据存在性。响应 Success + `SuccessBody.video_probe`。**判定表在 host**(「host 不信任 worker」+ 策略集中,§5.1)。
- **`VideoRemux { session_id, source_path, output_tmp_path, audio_transcode: bool, audio_track_index: Option<u32> }`** — `-c:v copy`,`audio_transcode=false` 时 `-c:a copy`、true 时 `-c:a aac`;输出 `-movflags +faststart` 的 MP4(见 §5.2 产物形态注)。字幕轨一律 `-sn` 丢弃(外挂字幕链路 `useVideoSubtitles` 已存,v1 不内嵌)。**每 ≤2s 发一帧 Progress**(`stage="remux"`,`detail=out_time/duration 百分比`),数据源 = ffmpeg `-progress pipe:1`。响应 Success + `SuccessBody.video_out: { out_bytes, out_duration_ms, video_copied, audio_copied }`。
- **`VideoTranscode { session_id, source_path, output_tmp_path, encoder_ladder: Vec<String>, crf_or_bitrate, max_long_edge: Option<u32>, audio_track_index, hw_decode: bool }`** — encoder_ladder 由 host 下发(如 `["h264_nvenc","h264_qsv","h264_amf","h264_mf"]`),worker 逐个试起、首个成功者用之;Progress 同上(`stage="transcode"`)。响应同 `video_out`。
- **`VideoFrames { session_id, source_path, input_fingerprint, mode }`**,`mode = Cover { max_long_edge } | Keyframes { n, cell_height }` — Cover 回单张 WebP(同帧 blob,复用既有「独立解码器验证 WebP」host 校验,`worker.rs:11-12`);Keyframes 回**一张横向 n 格雪碧条 WebP** + JSON `{cell_width, cell_height, n}`,host 解码后切成 `Vec<DecodedImage>` 喂现有 sprite 组装逻辑。时间戳选择规则(≈min(1s,10%时长)+黑帧规避)在 worker 内实现,契约与 `video/mod.rs:66-83` trait 文档一致。

`SuccessBody`(`message.rs:325-355`)追加 `video_session` / `video_probe` / `video_out` 三个 `Option` 字段(None 不序列化,线上形状不变,有既有测试锚 `success_body_thumbnail_wire_shape_unchanged` 先例)。

`WorkerErrorCode`(`message.rs:510-540`)追加一枚:**`FfmpegUnavailable`(terminal)** —— ffmpeg 路径缺失/hash 不符/`-version` 起不来/GPL 构建检出,镜像 `OrtDylibUnavailable` 先例;host 收到即标记工具待重下载。其余复用:codec 不支持→`UnsupportedVariant`(terminal)、源损坏→`MalformedInput`、磁盘不足→`ResourceLimit`、ffmpeg 非零退出且无法归因→`InternalError`(retryable,预算 1 次)。

### 2.4 超时与生命周期适配

- **静默限时机制直接复用**:`worker.rs:250-267` 已实现「收 Progress 即重置计时」。remux/transcode 的 per-op timeout(coordinator 超时表,`coordinator.rs:49-73` 一带)设 **60s 静默限时**——ffmpeg `-progress` 每 ≤1s 出数,60s 无 Progress = 真卡死。probe 30s、frames 120s、session_init 30s(总限时语义,不发 Progress)。
- **必须改一处**:`PROGRESS_TOTAL_CAP=3600s`(`worker.rs:38`)对慢机全转码 4h 影片不够。改法:`run_task` 增加可选 per-op 总上界参数(缺省沿用 3600s,既有 op 行为逐字节不变);video transcode 传 `max(2h, 探测时长 × 6)`、上限 12h。既有回归锚 `progress_resets_silence_deadline_and_stale_ignored`(`worker.rs:486-540`)保住旧语义。
- **取消与抢占**:复用 `CANCEL_POLL`(`worker.rs:32`)+ supervisor kill。**worker 必须把 ffmpeg 子进程纳入 Windows Job Object(kill-on-close)**,否则 supervisor kill worker 后 ffmpeg 成孤儿继续烧 CPU 写文件。host 侧 `VideoWorkerService` 维护两级优先队列:交互(播放触发)> 背景(封面/雪碧图);交互任务到达时,若在途是背景任务 → 置取消 → kill → respawn → 跑交互任务 → 背景任务重回队列(status 机天然支持续跑)。
- **崩溃策略**:沿用 supervisor 既有 TimedOut/Disconnected/Protocol → kill → wait → alive=false(`supervisor.rs:318-435`);转码中途崩溃 → `.tmp` 作废,host 启动时清扫 `work_dir/*.tmp`(新增 boot sweep,与备份线 tmp 清扫同型)。
- **finalize 心跳(施工批3 裁决落地)**:`-progress` 流停更且 ffmpeg 存活 → worker 每 ≤2s 发 `stage="finalize"` Progress,自持预算 60s + out_bytes/50MiB·s,耗尽 kill;host 60s 静默限时语义不变。remux total_cap 同批参数化 `max(1h, 时长 × 6)` 上限 12h。

## §3 FFmpeg 分发与许可

### 3.1 形态与来源

- **BtbN LGPL-shared win64 构建,钉死具体 release tag + sha256**(如 `ffmpeg-n7.1-latest-win64-lgpl-shared-7.1`;含 ffmpeg.exe/ffprobe.exe + avcodec/avformat/avutil/swscale/swresample DLL)。**绝不用 gyan.dev(GPLv3)、绝不用 BtbN GPL 变体、绝不用 ffmpeg-sidecar 的 auto-download(默认源即 GPL 包)**。
- worker crate 用 **ffmpeg-sidecar 作纯进程包装库**(显式路径构造,禁用其 download feature;若其 API 与显式路径耦合度差,退化为手写 `Command` + `-progress pipe:1` 解析,工作量可控)。
- **安装器不带 FFmpeg**,按需下载:走既有 exotic registry 下载机制(len+sha256 校验、签名 registry entry,`registry.rs` 单文件 8GiB 上限内)。zip 校验 sha256 → 解压到 `{app_data}/exotic/tools/ffmpeg/{tag}/` → 二次校验 ffmpeg.exe/ffprobe.exe 的逐文件 sha256(manifest 编入 host)→ 落 ready 标记文件。下载/解压产物同样 `*.tmp` + 同卷 rename。

### 3.2 编码器阶梯(LGPL 构建没有 libx264,必须现在定)

LGPL 构建**无 H.264 软件编码器**(libx264 是 GPL)。转码兜底的编码器阶梯:`h264_nvenc` → `h264_qsv` → `h264_amf`(厂商硬编,LGPL 兼容 hook)→ **`h264_mf`(Media Foundation 编码器,Windows 10+ 恒在,含软件回退,专利责任落 OS 层)**。AAC 用 FFmpeg 原生 aac 编码器(LGPL)。**施工第一阶段的验证项:确认所选 BtbN LGPL-shared 包编入 `h264_mf` 与 mediafoundation 支持**(BtbN 默认开,但必须实测 `ffmpeg -encoders`)。若意外缺失 → 备选 openh264(BSD,需另下 Cisco 二进制才有专利豁免)列为 fallback 决议点,不默认采。

### 3.3 下载失败降级

工具未就绪时:播放侧判定命中 remux/transcode → 前端得 `needs_component` 状态,展示「下载视频扩展组件(约 xx MB)」卡片 + 重试;缩略图侧 `backend_for()` 继续返回 None,行为与今天完全一致(无回归)。下载中断可续/重试,校验不过即整包作废重下。

### 3.4 许可红线落实

1. 禁 GPL 构建、禁 `--enable-gpl`、禁链 x264/x265 —— 钉死 BtbN LGPL tag + sha256,**加运行时保险丝**:`VideoSessionInit` 时 worker 解析 `ffmpeg -version` configuration 行,含 `--enable-gpl` 即回 `FfmpegUnavailable`,杜绝用户手工换包把产品拖进 GPL 分发争议。
2. (A)GPL 不可链入 —— 全链路无链接:worker 与 ffmpeg.exe 是进程边界,连 LGPL 动态链接义务都不触发;但仍按 LGPL 精神**在商店详情页展示 FFmpeg 声明 + 源码获取链接**(BtbN release 对应 source tarball;若 CDN 自托管二进制则同镜像源码包)。
3. HEVC 专利 —— 本方案**不做 HEVC 编码**,解码走两条免责路径:remux 后交 WebView2 系统硬解(用户自装 HEVC Video Extensions,责任在 OS 层),或转码时 ffmpeg 的 LGPL 原生 hevc 解码器仅解码 + 按需下载(非预装)形态,贴近 HEVC Advance 软解豁免条款;调研标注此为推断无一手确认,风险节 §10 登记。
4. DTS/AC-3 —— 仅解码转 AAC,解码器为 libavcodec 原生 LGPL 实现;AC-3 核心专利已到期,E-AC-3/DTS 残余专利风险登记 §10,不做静态链接、不预装,维持「用户主动下载组件」姿态。

## §4 商店接入与授权

### 4.1 分发形态裁决:**builtin worker + 商店卡片 + FFmpeg 按需下载**(RAW 同款,修正一处)

- worker 二进制本体是小 Rust exe(FFmpeg 不在里面),编译进应用分发最省事、验签链最短;商店「安装」按钮语义 = **授权确认 + 触发 FFmpeg 组件下载**。这正是催化剂答案:大二进制与安装包解耦后,商店包不再有存在必要——builtin 即可。
- 复用 RAW 的 `distribution:"builtin"` catalog 语义(`catalog.rs:136-139`)与 `availability_of` builtin 门(findings-exotic-plugin §7);dev 路径解析加 `EXOTIC_VIDEO_WORKER_PATH` 分支(`installer.rs:339-353` 同款);**Release builtin 解析共享 H2b-prod 待办**(`installer.rs:358-364`),video-worker 不单独解、随 raw-worker 一起收口。
- catalog offering 声明:`plugin_id="video-extended"`(常量进 `coordinator.rs:32-36`),`media_kind:"video"`,**formats 只认领真 exotic 扩展名**(候选:`rmvb, rm, vob, f4v, m2v, mxf, dv`——均不在 format.rs 17 表内,不触 `CommonFormatConflict`),`capabilities:["thumbnail"]`(对这些 exotic 格式走正常 exotic 任务化出缩略图,worker 已有 `video_frames` 能力顺便服务),对常见格式的 remux/转码增强则如 §2.1 走 Service 门。**v1 可先声明空经验集最小格式(仅 rmvb/vob)**,判定表兜底转码即可播(FFmpeg LGPL 原生含 RealVideo/cook 解码器)。
- **施工批3 修正**:rmvb/vob 已进 `utils/format.rs` 注册表(`CommonFormatConflict` 对 builtin 豁免,RAW 裁决 A 同型,原「均不在 17 表内」表述系设计时未见豁免路径);缩略图单轨 = video 派生链经 V5 桥,exotic 播种对 builtin+video 恒掐(`fast_scan` `seed_gate_admits`),descriptor/`VideoThumbnailFactory` 保留为一致性面。

### 4.2 license_tier 建议(终裁留给用户)

**推荐 `free`**。理由:(a) mkv 不能播/没缩略图在用户感知里是**产品缺陷**不是增值功能,收费修缺陷伤口碑;(b) FFmpeg 下载本身已是一道摩擦,再叠付费墙转化极差;(c) 变现空间留给后续第二个 offering——「稀有格式包」(rmvb/VC-1/MXF 等真 codec 缺口 + 批量预转码)可设 paid tier,与本插件并存不同 SKU。若用户终裁 paid:builtin+paid 需走 License token 验签链(sku 字段已预留,`catalog.rs:122-126`),`availability_of` 的 builtin 分支需从「free 直放行」扩展出「paid 验 token」路径——工作量+1,方案兼容。

## §5 播放链路改动

### 5.1 判定表(host 侧新模块 `src-tauri/src/video/playback_policy.rs`)

输入:DB 已存 `VideoMeta`(`src/types/media.ts:250-257`)或按需 `VideoProbe` 回填(MF 探测不了的容器 DB 里缺 codec → 打开时先派 probe,<1s,结果回写 DB)。输出五态:

| 容器 | video codec | audio codec | 判定 |
|---|---|---|---|
| mp4/m4v/webm/ogv(Chromium 认) | h264/vp8/vp9/av1 | aac/mp3/opus/vorbis/flac | **DirectPlay**(现状路径,零改动) |
| mp4/mov | hevc | 任意可解 | **NeedsHevcExt**:前端用 `MediaCapabilities.decodingInfo` 实测,可解→DirectPlay;不可解→引导装「HEVC 视频扩展」或用户选转码(§10 开放问题 4) |
| mkv/avi/wmv/flv/ts/mts/m2ts/asf/mpg/3gp…(Chromium 不认容器) | webview 可解 | webview 可解 | **Remux**(`-c copy`,秒级) |
| 同上 | webview 可解 | ac3/eac3/dts/truehd… | **RemuxAudioTranscode**(`-c:v copy -c:a aac`) |
| 任意 | hevc(无扩展)/vc1/mpeg2/rv40/msmpeg4… | 任意 | **Transcode**(H.264/AAC 阶梯,§3.2) |

判定表数据(Chromium 可解集合)硬编 host 侧常量 + 单测钉死;HEVC 一项由前端实测覆盖修正。

### 5.2 派生挂接

- **新 kind:`video_playable`**(`derive/kind.rs:20-90` 枚举 + `as_str`/`from_str`/`ALL` 追加;remux 与 transcode 同 kind,方法与流事实写进 payload 列——keyframe payload 先例)。产物:`{cache_dir}/video/{cache_key}.mp4`(faststart 整文件 MP4;`<video>` 对 file-backed 源可范围请求,无需真 fmp4 分片,「fmp4」目标落实为 faststart MP4 即可,调研语义不变)。
- **入队策略:纯按需**,绝不 backfill 全库(磁盘爆炸)。播放请求 → upsert `media_derivations(item, 'video_playable')` → `VideoPlaybackService` 直接 claim 该行并派 worker(交互路径,不等背景流水线 tick);status 机/断点续传/孤儿恢复全复用。批量预转码留给「稀有格式包」二期(§4.2)。
- `*.tmp` + 同卷 rename:worker 写 `{work_dir}/{job}.mp4.tmp`(work_dir 即 `{cache_dir}/video/`,同卷),host 验收(ffprobe 复检时长±容差、faststart 标志)后 rename。
- **⚠ 协调注记**:`derivations.rs`、`doc_commands.rs` 当前是用户 WIP(review 二轮红线点名勿碰)。本方案只**追加** kind 常量与消费分支,施工批次开工前须与用户确认 WIP 落定或明确合并顺序,禁 stash/checkout。

### 5.3 前端

- 新 composable `src/composables/player/useVideoSource.ts`:调新 IPC `resolve_video_playback(itemId)` → `{ mode: "direct" | "derived" | "preparing" | "needsComponent" | "needsHevcExt", src?, progress? }`;preparing 态订阅进度事件(Channel→事件+快照的缩略图进度先例);就绪后把派生 mp4 的 `convertFileSrc` URL 交给 ContentViewer。
- **`VideoPlayer.vue` 本体近零改**:它只吃 `props.src`(`VideoPlayer.vue:31-59`),源切换发生在 ContentViewer 层;新增覆盖层组件 `VideoPreparingOverlay.vue`(进度 %+取消+「装 HEVC 扩展」引导),挂 ContentViewer。
- `src/utils/videoDiagnostics.ts:49-85` 增补归因分支:判定=needsComponent/needsHevcExt 时给对应引导文案,替代现在的笼统「不支持」。
- **⚠ 协调注记**:`VideoSeekBar.vue`/`MediaGrid.vue` 为用户 WIP,本方案设计上**不触碰**(雪碧图/缩略图经数据驱动自动获益);若施工时发现必须动,先停手协调。

### 5.4 缓存策略裁决:**新独立池,不共池**

10GB 缩略图池(`thumbnail/cache.rs:313-317`)装的是几十 KB 级 WebP;单个转码产物即 GB 级,共池会把全库缩略图驱逐殆尽。方案:`{cache_dir}/video/` 独立池,新设置键 `video_cache_max_mb`(schema.rs:428 `thumb_cache_max_mb` 同款注册),**默认 20GB(建议值,终裁留用户)**,LRU 按最近播放时间驱逐;驱逐跳过在播文件(Windows 删占用文件本就失败,失败即跳过下轮再试);设置页缓存统计卡扩一行(CacheStats 扩容先例)。单文件护栏:产物预估 > 池预算 50% → 前端确认框(临时放行/调大池/放弃)。

## §6 硬约束落实清单

| 约束 | 落实 |
|---|---|
| thiserror 结构化错误 | worker 内部错误枚举 + host `VideoServiceError` 均 thiserror;映射到 `WorkerErrorCode`/`AppError` 既有链 |
| IPC 错误稳定码不漏内部串 | `resolve_video_playback` 错误走既有 `AppError` serde 序列化(稳定 code);ffmpeg stderr 只进 tracing/诊断包(已有脱敏链),**不进** IPC message;`FailureBody.message` 不含绝对路径(协议既有红线,`message.rs:596`) |
| rusqlite spawn_blocking | `video_playable` 的 upsert/claim/finish 全走既有 derivations DAO 模式(async command 内 spawn_blocking) |
| 不跨 `.await` 持 std Mutex | `VideoWorkerService` 沿 Enhance 先例:supervisor 句柄在专用阻塞线程/spawn_blocking 内同步驱动,async 侧只过 channel |
| capabilities 权限声明 | 新 IPC 命令(`resolve_video_playback`、组件下载触发、取消)注册进 `src-tauri/capabilities/` 既有 exotic/derive 能力文件 |
| assetProtocol scope | **无需扩**:派生产物在 `{cache_dir}/video/`,cache_dir 已递归 allow(`lib.rs:417-418`),自定义缓存目录迁移路径也已覆盖(`config_commands.rs:441-442`);CSP `media-src asset:` 已在(tauri.conf.json:44) |
| `*.tmp` 同卷 rename | §5.2;FFmpeg zip 解压亦同(§3.1) |
| 路径 canonicalize/越界拒 | worker 侧 `output_tmp_path` 前缀白名单(Enhance 同型);`ffmpeg_exe_path` hash 校验 |

## §7 跨平台前瞻

macOS:WKWebView 原生解 HEVC,播放缺口显著小,但 mkv 容器缺口同在;worker 架构原样移植,**FFmpeg 来源是真缺口——BtbN 不出 macOS 包**,需自托管 runner 出 LGPL 构建(CI 已具备),CSP 按项目红线补 `tauri:` 行并真机验证。iOS:**禁 spawn 子进程,sidecar 架构不可移植**,路线改 AVFoundation 原生(HEVC 天然可播)+ 余量待 ffmpeg-kit 静态 LGPL 评估,明确列为后续独立立项。Android:允许 exec 私有 lib 目录二进制,per-ABI ffmpeg 可行,MediaCodec 覆盖 HEVC;worker 协议不变,spawn 细节适配。三平台共同点:协议与判定表全复用,只有「FFmpeg 获取与执行方式」是平台变量——这正是把判定逻辑放 host、执行放 worker 的边界红利。

## §8 分阶段实施计划

依赖:V1→(V3,V4);V2 与 V1/V3 并行;V5、V6 依赖 V4 且互相并行;V7 依赖 V6;V8 收口。各阶段「施工中验证」归 implementer,「批末门禁」(全量 `cargo test --workspace` + `npm run lint` + vue-tsc + CI 面)归 phase-closer,implementer 不跑。

**V1 协议扩展**(可与 V2 并发)
- 改动:`crates/exotic-protocol/src/message.rs` — RequestBody 五 op(:35-137 追加)、SuccessBody 三字段(:325-355)、capability 四常量(:307-317)、`FfmpegUnavailable`(:510-540 + as_str + default_retryable=false);roundtrip/线形状测试(既有测试样式照抄)。
- 验收:`cargo test -p exotic-protocol` 绿;`success_body_thumbnail_wire_shape_unchanged` 不动仍绿。规模:~400 行(半数为测试)。

**V2 FFmpeg 工具管理**(可与 V1/V3 并发,文件域不相交)
- 改动:新 `src-tauri/src/exotic/tools.rs`(下载/校验/解压/ready 标记/boot sweep);registry entry + sha256 manifest;设置键;选定 BtbN tag 并**实测 `-encoders` 含 h264_mf/aac、configuration 无 `--enable-gpl`**(本阶段验收硬项)。
- 验收:`cargo check`;下载-校验-解压单测(假包/坏 hash/中断续传);实机 ffmpeg -version 证据留档。规模:~500 行。

**V3 video-worker crate**(依赖 V1)
- 改动:新 `crates/exotic-workers/video-worker/`(main.rs 握手样板抄 raw-worker;probe/remux/transcode/frames 四模块;ffmpeg-sidecar 显式路径;Job Object kill-on-close;Progress 泵)。
- 验收:`cargo test -p video-worker`(mock 小样片 fixture:mkv-h264-aac、mkv-h264-ac3、avi-mpeg4);`cargo clippy -p video-worker`。规模:~1200 行。

**V4 host 服务与授权接线**(依赖 V1、V3)
- 改动:`coordinator.rs:32-36` VIDEO_PLUGIN_ID;`resources/exotic-catalog.json` offering(builtin/free/formats=exotic 集);`installer.rs:339-353` EXOTIC_VIDEO_WORKER_PATH dev 分支;新 `src-tauri/src/video/worker_service.rs`(双优先队列/抢占/超时表项);`worker.rs:38` per-op 总上界参数化。
- 验收:`cargo test`(supervisor mock 测试样式复用:静默超时/抢占/崩溃回收);dev 端到端:设 env 起 worker,probe 真 mkv 回流事实。规模:~900 行。

**V5 缩略图后端桥**(依赖 V4;与 V6 并行)
- 改动:新 `src-tauri/src/video/worker_backend.rs` 实现 `VideoBackend`;`video/mod.rs:91-103` backend_for 接入;`derive/kind.rs` 不需动(kind 已存在,`is_implemented` 语义按扩展名×后端可用性走 backend_for 即可,核对 `derive/video.rs:51` 消费点)。
- 验收:dev 实机:mkv/webm 库目录扫描后封面+雪碧图落库,MediaGrid 显示(GUI 项标注不自动化);`cargo test` 涉改面。规模:~400 行。

**V6 播放链路后端**(依赖 V4;与 V5 并行)
- 改动:新 `playback_policy.rs`(判定表+单测钉死);`derive/kind.rs` 加 `VideoPlayable`;`derivations.rs` 消费分支(**⚠ 用户 WIP,先协调**);新 IPC `resolve_video_playback` + 进度事件 + capabilities 声明;`{cache_dir}/video/` 池与 LRU。
- 验收:`cargo test`(判定表全行覆盖、池驱逐、tmp sweep);`cargo clippy`。规模:~1000 行。

**V7 前端**(依赖 V6)
- 改动:`useVideoSource.ts`、`VideoPreparingOverlay.vue`、ContentViewer 接线、`videoDiagnostics.ts:49-85` 归因分支、商店详情页 FFmpeg/LGPL 声明文案、设置页缓存统计行。**不碰 VideoSeekBar/MediaGrid(用户 WIP)**。
- 验收:`vue-tsc --noEmit`;涉改文件 eslint;GUI 手测清单(mkv 直播放全流程、取消、组件下载引导)标注不自动化。规模:~700 行。

**V8 收口**(依赖全部):全量门禁(phase-closer)、docs/todo.md、三件套回写、H2b-prod builtin release 解析随 raw-worker 合并跟进。

## §9 边界情况(施工须逐条落防护)

1. 源文件在 remux 中被改/删 → input_fingerprint(mtime+size)派活前后核对,不符弃产物。
2. 多音轨 → 默认选 disposition=default,否则首条 webview 可解轨;探测结果含轨列表,二期做 UI 选轨。
3. PGS/内嵌字幕 → v1 一律 `-sn` 丢弃,外挂字幕链路不受影响;文案注明。
4. HDR(HEVC HDR10/DV)→ remux 保留;需转码时 v1 不做 tone-map,SDR 化色偏风险在 UI 提示。
5. 磁盘不足/产物超池 → 预估+护栏(§5.4);转码中盘满 → `ResourceLimit` terminal,tmp 清理。
6. 应用中途退出 → boot sweep 清 `work_dir/*.tmp`;status=1 孤儿行走既有孤儿恢复。
7. 驱逐在播文件 → 删失败即跳过(§5.4)。
8. 0 字节/截断源 → probe 失败 `MalformedInput` terminal,不重试。
9. 无音轨视频 → RemuxAudioTranscode 判定分支须容 audio 缺失(直接 Remux)。
10. ffmpeg 被杀后孤儿 → Job Object(§2.4),施工必须验证 kill worker 后 ffmpeg 同死。
11. 网络盘/超长路径/Unicode 文件名 → 沿既有 canonicalize 约定,ffmpeg 参数传路径不经 shell(Command args 直传)。
12. 同 item 并发播放请求 → service 按 (item,kind) 去重挂载同一在途任务。

## §10 风险

- **h264_mf 在 BtbN LGPL 包内的实际可用性**未实测——V2 阶段首要验证项,不通过则触发 openh264 备选决议。
- HEVC「系统硬解转嫁专利责任」与 HEVC Advance 软解豁免均为推断(调研已标低置信度);发布前建议过一次法务口径。
- `PROGRESS_TOTAL_CAP` 参数化触碰 worker 核心等待环——回归锚已有,仍属共享契约改动,批末全量门禁必跑。
- Release builtin 解析(H2b-prod)未收口,video-worker 生产可用性被其挡;需与 raw-worker 同批收口。
- derivations.rs/doc_commands.rs 用户 WIP 合并窗口不确定,V6 排期受制于协调。

## §11 已弃方案

- libmpv 子窗口 —— wry 层级同步公认痛点 + GPL 审计成本,不作首发路线。
- gstreamer-rs —— Windows 运行时打包复杂度最高。
- ffmpeg.wasm/WebCodecs —— 5-20 倍慢、无 GPU,不适合播放/转码。
- 本地 HTTP + HLS 实时转码 —— 会话管理/CSP/服务面复杂度对「资产查看」场景过度设计。
- 进程内链接 FFmpeg(feature `ffmpeg` 旧计划)—— Windows 链接痛、崩溃不隔离、LGPL 动态链接义务更绕;worker 子进程全面替代。
- gyan.dev 静态包 / ffmpeg-sidecar auto-download —— GPLv3,许可红线直撞。
- 安装器捆绑 FFmpeg —— 体积+许可姿态双输,按需下载替代。
- catalog 认领 mkv/webm 常见格式 —— `CommonFormatConflict` 直接拒整个 Catalog(`catalog.rs:97-100`),改走 Service 门。
- 转码产物共用 10GB 缩略图池 —— GB 级产物会清空全库缩略图。

## 开放问题需用户裁决

1. **license_tier**:推荐 free(理由 §4.2);若要变现,建议另立「稀有格式包」paid offering,是否认可?
2. **缓存池**:独立池默认 20GB + 单文件超池 50% 弹确认——默认值与护栏比例请终裁。
3. **真 codec 缺口范围**:v1 是否就做转码兜底覆盖 rmvb/VC-1/mpeg2(catalog 认领 rmvb/vob),还是 v1 只做 remux/半转码、全转码延后?(工程差异集中在 V3 fixture 与 V6 判定表行数,架构无差)
4. **HEVC 引导 UI**:检出可硬解缺失时,默认动作=引导装「HEVC 视频扩展」(免费商店项)还是默认本地转码、扩展作为可选?
5. **V6 施工窗口**:derivations.rs/doc_commands.rs 用户 WIP 的合并顺序与时点。
6. **macOS FFmpeg 自建**(§7):是否现在就在自托管 runner 排 LGPL 构建流水线,还是 macOS 线启动时再说?
