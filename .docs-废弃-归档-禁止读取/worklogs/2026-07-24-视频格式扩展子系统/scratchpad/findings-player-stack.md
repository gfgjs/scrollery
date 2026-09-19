---
id: 2026-07-24-findings-player-stack
status: active
type: working-memory
line: 视频格式扩展子系统
created: 2026-07-24
---

# 摸底 A:当前视频播放链路与格式支持面

> 来源:general-purpose(haiku)两轮回执合并,主线代写落盘(子代理 Write 受挡)。证据为 path:line,细节以代码为准。

## 两层架构(核心结论)

支持面分两层,勿混:

1. **播放层(WebView2 Chromium 解码)**:HTML5 原生 `<video>`,17 个识别格式在 Chromium 侧大多可播(WebM/MKV/VP9 原生支持);受限于 Chromium codec 面(HEVC 依赖系统/硬件,AC-3/DTS 音轨等不支持)。
2. **后端层(缩略图/关键帧/Seek Bar,Media Foundation)**:MF 直支持 13 格式;mkv/webm/flv/ogv 4 格式需 FFmpeg 后端(feature `ffmpeg`,Perf-only 变体,**未交付**)。

## 关键代码依据

| 事实 | path:line |
|---|---|
| 内置识别格式表:17 个 video() 宏(mp4, m4v, mov, avi, mkv, webm, wmv, flv, mpg, mpeg, 3gp, 3g2, ts, mts, m2ts, ogv, asf) | src-tauri/src/utils/format.rs:158-178 |
| MF 直支持清单 const MF_VIDEO_EXTS = 13(mp4, m4v, mov, wmv, avi, 3gp, 3g2, ts, mts, m2ts, asf, mpg, mpeg) | src-tauri/src/video/media_foundation.rs:42-47 |
| FFmpeg 后端声明("FFmpeg feature `ffmpeg`, Perf-only") | src-tauri/src/video/mod.rs:8-14 |
| "Lite + mkv/webm/flv → needs Perf/FFmpeg" | src-tauri/src/video/mod.rs:88 |
| backend_for() 无后端时返回 None(后端选择逻辑) | src-tauri/src/video/mod.rs:100-102 |
| 播放器:原生 `<video>` + crossorigin,无第三方播放库 | src/components/media/player/VideoPlayer.vue:2-11 |
| 源供给:convertFileSrc + assetProtocol scope(本地文件 → asset:// URI) | src/utils/assetUrl.ts:4-7 |
| VideoMeta:codec/fps/bitrate/rotation/hasAudio | src/types/media.ts:250-257 |
| 播放失败诊断:HEVC/AV1/MKV 不支持、网络错误分类 | src/utils/videoDiagnostics.ts:49-85 |

## 「当前支持格式」回答(两层拆分)

- **扩展名被识别**:上表 17 个。
- **实际能播(Windows)**:播放层走 Chromium——H.264/VP8/VP9/AV1(硬件相关)+ 常见容器可播;HEVC 及部分音轨 codec 受限。后端缩略图层 MF 13 格式;mkv/webm/flv/ogv 缩略图链路缺 FFmpeg 后端未交付。
- 即:**播放与缩略图支持面不一致**,mkv/webm 可能"能播但没缩略图/关键帧",HEVC 可能"有缩略图(MF 硬解)但 WebView 播不动"。诊断层(videoDiagnostics.ts)已对 HEVC/AV1/MKV 做失败归因。

## 对设计的输入

- 已存在 FFmpeg 后端接缝(video/mod.rs backend_for()),但 Perf 变体未交付——新子系统可顺此接缝落 worker。
- 播放层缺口(HEVC/AC-3 等)与缩略图层缺口(mkv/webm/flv/ogv)是两个不同问题,方案须分别应对。
