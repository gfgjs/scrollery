---
status: snapshot
type: working-memory
line: 视频播放器重构
created: 2026-07-22
---

# 视频播放器重构 · findings

## 现状摸底(P0a,填写者:摸底代理)

### 组件清单与播放路径

**播放器本体**（仅一处）：
- `src/components/media/ContentViewer.vue:59-72` — 原生 `<video>` 元素（`controls autoplay` 属性）
- 视频引用：`src/utils/assetUrl.ts:4-7` — `resolveAssetUrl()`函数，内部调用 `convertFileSrc()`（Tauri asset protocol）

**播放路径机制**：本地文件路径 → `resolveAssetUrl()` → 检测是否 data/blob/http，否则转 Tauri `convertFileSrc()` → 返回 `tauri://asset/` 协议 URL → 浏览器原生播放。无自定义流、无 Range 请求处理。

**相关 composables / stores**：
- `src/composables/useMediaDetail.ts:1-200` — 缩放/旋转/拖拽（图/视共享），无播放控制
- 缩略图/进度预览来自派生流水线：`src-tauri/src/video/mod.rs` 的 `VideoBackend::keyframes()` 生成雪碧图

---

### 功能矩阵

| 能力 | 有 / 无 | 备注 |
|------|--------|------|
| 播放/暂停 | ✓ | 原生 `controls` 属性 |
| 进度条拖拽 | ✓ | 原生 |
| 音量控制 | ✓ | 原生 |
| 倍速 | ✗ | 完全缺失 |
| 全屏 | ✓ | 原生 `<video>` 全屏 |
| 画中画 | ✓ | 浏览器原生支持 |
| 播放快捷键 | ✗ | 仅有缩放/翻页快捷键（`viewer-image.ts`），无 space/>/< 播放控制 |
| 字幕（内挂/外挂） | ✗ | 完全缺失 |
| 缩略图进度预览 | ✓ | 派生流水线生成 `video_cover` 作 poster |
| 循环播放 | ✗ | 无 loop 属性或控件 |
| 截帧 | ✗ | 完全缺失 |
| 旋转 | ✓ | 共用 `useMediaDetail` 的 `rotate()`（transform 变换） |
| 播放列表/上下切换 | ✓ | `navigate()` 方法，Arrow Left/Right 快捷键 |
| 记忆播放位置 | ✗ | 无 `currentTime` 持久化 |
| 错误兜底 | ✓ | `@error` 事件 → 显示"不可用"占位（`detail-viewer__unavailable`）|

---

### 痛点清单

1. **零播放控制定制**：原生 `<video controls>` 硬编码；无 API 暴露播放状态、无快捷键响应；即使用户想加倍速/记忆位置也无法接入。
   
2. **缺关键功能**：倍速、字幕、循环、截帧、播放位置记忆是现代播放器基本配置，当前无任何一项。播放快捷键（Space 暂停、</> 快进）逻辑缺失。

3. **错误处理脆弱**：仅 `@error` 事件一行，无格式兼容性检测、无流失败重试、无编码格式提示（用户遇"不支持的视频格式"只看到黑屏）。

4. **进度同步隐患**：缩缩放/旋转后滚轮翻页判据累加（`accumulatedDelta >= ±50`）未考虑视频播放中的交互冲突。

5. **后端无直接支持**：视频播放完全由浏览器承载，后端 `VideoBackend` trait 仅服务派生流水线（keyframes）；若需客户端播放进度、码率自适应等高级需求须新增 IPC command。

## 方案调研(P0b,填写者:researcher;落盘由主线代贴——Write 工具对含 "findings" 文件名的子代理写入硬拒)

检索日期均为 2026-07-22。

### 1. UI 库对比(License 红线检查:全部 MIT/Apache-2.0,无 GPL,均通过)

| 项目 | 最近活跃 | Vue3 | TS | gzip | License | 定制 |
|---|---|---|---|---|---|---|
| Vidstack | push 2026-06-10/今日更新;核心 1.15.6 | 官方 Web Components + Vue 安装指南 | 一等公民 | ~55KB | MIT | 高,headless |
| Plyr | push 2026-01-03,star 29.9k | 原生 `<video>` 包装,任意框架 | 非一等公民 | ~32KB | MIT | 中 |
| media-chrome | push 2026-07-01 | Web Components,Vue 内直用 | 完整 | ~42KB | MIT | 高,纯原语需自带解码后端 |
| Video.js | push 2026-06-29,当前 8.23.9 | 无官方 Vue 包装 | 有 | ~202KB(重) | Apache-2.0 | 高,插件生态成熟但重 |
| Artplayer | push 2026-07-13,最新 5.4.0 | 社区 vue3-artplayer + 官方 TS 示例 | 有 | ~36KB | MIT | 高,内置弹幕/字幕 |
| xgplayer | GH push 今日,但正式版本停在 3.0.26(2024-07,矛盾未解) | 官方 xgplayer-vue | 有 | ~123KB | MIT | 高,插件架构 |

**关键分歧**:Vidstack/Plyr/media-chrome 官宣合并进 Video.js v10(beta 早期、GA 年中),1.x 维护降速([discussion #1755](https://github.com/vidstack/player/discussions/1755))。xgplayer 今日仍 push 但正式 release 卡在 2024-07,活跃度判断存疑,未深挖 diff。

### 2. 编解码现实

- HEVC:caniuse 显示 Chrome 107+/Edge 79+ 均仅 "partial support";Windows 需系统装 HEVC Video Extensions(新装机默认无),缺失时 WebView2 黑屏([WebView2Feedback#4285](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4285));Safari 13+ 走 AVFoundation/VideoToolbox **full support**,无需扩展。
- AV1:Chrome 70+/Edge 121+ full;Safari 仅 17+ partial(依赖新 Apple Silicon)。
- mkv 容器:**来源冲突未裁决**——一说"最新 Chromium/WebKit 已支持",另一说 `<video>`/MSE demuxer 白名单本质只认 mp4/webm,不识别通用 mkv,需先 remux;caniuse.com/mkv 页面 JS 渲染未能核实,建议真机实测裁决。
- AC-3/E-AC-3:Chromium 上游默认不启用解码(许可问题),需 `enable_platform_ac3_eac3_audio` 编译开关借道系统解码器;Edge 大概率启用(推断,无官方文档证实);Safari/macOS 系统自带支持。
- 播不了清单:mkv 容器本身(有争议)、无扩展下的 HEVC、老版本下的 AV1、未启用平台解码的 AC3、mkv 内嵌字幕/多音轨。

### 3. 出路

- **a) libmpv**:现成插件 [tauri-plugin-libmpv](https://github.com/nini22P/tauri-plugin-libmpv)(MPL-2.0),Windows 已测试、Linux 嵌入实验性不可用、macOS 未测试,渲染=透明窗口叠加。**License 红线关键**:mpv 默认 GPLv2/v3,需专门 `--enable-lgpl` 编译且阉割部分功能才是 LGPL([mpv issue #2033](https://github.com/mpv-player/mpv/issues/2033)),预编译二进制若未核实构建方式应默认判 GPL,触发一票否决。全格式覆盖但工程/包体/跨平台代价均高,iOS/Android 无路径。
- **b) ffmpeg remux+MSE**:Tauri 官方讨论推荐方案([discussion #15171](https://github.com/orgs/tauri-apps/discussions/15171)),`-c copy` 纯 remux 只解容器壳不解 codec 硬解缺口;ffmpeg 默认 LGPL,需锁死 configure 禁用 `--enable-gpl/--enable-nonfree`;Tauri 自定义协议需自行实现 Range([issue #4133](https://github.com/tauri-apps/tauri/issues/4133))。工程/包体中等,四端可行。
- **c) GStreamer**:核心 LGPL 但常用插件多 GPL/专利受限需逐个审查;无成熟 Tauri 封装先例;窗口叠加痛点同 libmpv;工程代价最高。
- **d) 系统解码器(MF/AVFoundation)**:项目已有 Windows MF/DXVA 硬解管线经验可复用(视频封面关键帧性能线);macOS 走 AVFoundation;零第三方 License 风险,但需四端各写一套原生桥接,长期维护成本高;mkv 容器在系统层同样非一等公民(待查)。

### 4. 同类先例

[nini22P/tauri-plugin-libmpv](https://github.com/nini22P/tauri-plugin-libmpv) / [tauri-plugin-mpv](https://github.com/nini22P/tauri-plugin-mpv)(通用封装非资产管理器)、[Newish0/tauri-media-player](https://github.com/Newish0/tauri-media-player)(Tauri+React+MPV 验证可行)、[lvlrSajjad/electron-player](https://github.com/lvlrSajjad/electron-player)(Electron mkv/mp4 库,解码方案未注明,推测靠原生 Chromium)。Jellyfin/Plex 采用系统/libmpv 路线为旁证,本轮未独立核实,标推断。

### 推荐矩阵

- **轻档**(UI 库换肤如 media-chrome/Vidstack,不改解码):格式面不变;工程低(1-2 天);包体 +30~55KB;跨平台风险低;License 低。
- **中档**(UI 库 + ffmpeg remux/MSE):补齐容器壳,codec 硬解仍受系统/WebView2 版本限制;工程中(以周计);包体 +10~30MB;跨平台风险中;License 低-中(须锁死 ffmpeg 编译选项防误踩 GPL)。
- **重档**(libmpv 或 MF/AVFoundation 原生):理论全格式覆盖;工程/包体/跨平台风险均高(libmpv 尤其 License 风险中高,需强制核实 LGPL 构建;MF/AVFoundation License 风险低但四端各写一套代码)。

## 主线裁决

### P1(方向裁决,用户裁)

1. UI 层自研 Vue 控件——不接 Vidstack/Plyr/media-chrome 等第三方 UI 库(轻档换肤方案未采纳);理由:定制自由度/包体/长期维护权衡后自研更可控,四组件+八 composables 施工量可控。
2. 档位按「轻档」先行——不改解码路径(不做 ffmpeg remux/MSE 中档、不做 libmpv/MF 原生重档);现有 WebView2/AVFoundation 原生解码维持,格式面不变。remux/原生解码是否二期立项收口时另裁(见 progress.md 未决裁决点 A)。

### GB🔴驳回(cue 空行)

复核指出「cue 之间的空行处理」疑似缺陷,主线驳回:SRT/VTT 规范中空行本身即 cue 的终结符(SRT 相邻 cue 间以空行分隔;WebVTT `WEBVTT` 头后同样以空行分隔连续 cue block),`srtToVtt.ts` 按空行切分 cue 是正确实现,非漏判。1🟡(属性标签失配)已按复核意见修正。

### GA 授权模型 + `.ok()` 吞错

- 授权模型定案:`load_video_subtitle` 的显式 `path` 与 `save_frame_png` 的 `target_path` 均来自前端原生对话框(打开/保存)回传——用户在系统对话框中选中/确认该路径即完成授权,后端不再做「是否属于已知媒体目录」之类的二次归属校验;扩展名白名单/大小写不敏感 stem 匹配/5MB 与 64MB 上限/PNG 魔数校验均属纵深防御,不是授权判定本身(详见 player_commands.rs 头部注释)。
- 既有 `.ok()` 吞错点维持原有约定不改(与仓内既有姿态一致,非本线新增债务)。

### crossorigin 保留 + 真机首验

`<video crossorigin="anonymous">` 属性维持(GA-fix 阶段裁决未变):tauri-2.11.5 asset protocol 对所有响应发 `Access-Control-Allow-Origin: <window_origin>`(实证见 task_plan.md「截帧」裁决段),drawImage+toBlob 理论不 taint。但该结论未经真机验证,列为遗留清单①「首验项」——若真机出现 CORS 黑屏,截帧改走 blob URL 二期方案(不回退 crossorigin 属性本身,只改截帧取数路径)。

### HEVC fourcc 取证结论

Scout 取证:`src-tauri/src/video/media_foundation.rs:864-881` `codec_label()` 映射表——`MFVideoFormat_H264` → `"H264"`、`MFVideoFormat_HEVC`/`MFVideoFormat_HEVC_ES` → `"HEVC"`(MF 友好名,非小写 fourcc);表中无 AV1 分支,任何 AV1 subtype GUID 均落 `else` 分支返回 `None`(序列化为 `NULL`)。此前疑虑的「fourcc 缺口」不成立——`videoDiagnostics.ts` 按 `videoCodec` 字符串值域(`"HEVC"`/`"H264"`/...或 `null`)分流即可,无需新增 fourcc 归一层,不修。

### 快捷键契约变更定案

`kind === 'video'` 时 ←/→ 由「切上下条目」改为「seek ±5s」;切条目改绑 Shift+←/→。`viewer-image.ts` 的 `viewer.prev/next` when 谓词由 `isImageOrVideo` 收窄为 `isImage`,消除双绑;`kind === 'image'` 的 ←/→ 切条目行为不变。理由与机制见 task_plan.md「四项裁决定案」第一条。

## 耐久提升候选(F-ID 全仓全局序,收口时逐行处置进 closeout.md)

| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-030 | Tauri 2.11.5 asset protocol 对**所有**响应(含 206/含错误路径)发 `Access-Control-Allow-Origin: <window_origin>`;`<video crossorigin="anonymous">` 由此可 `drawImage`+`toBlob` 截帧不 taint canvas,dev/prod 两 origin 均匹配 | experience(Tauri/环境类知识,未来 canvas/媒体流水线可复用) |
| F-031 | License 尽调陷阱:mpv/libmpv 默认 GPLv2/v3,唯有专门 `--enable-lgpl` 编译且阉割部分功能才是 LGPL;预编译二进制未核实构建方式应默认判 GPL——不能只看项目"声称"的 license,要查构建产物的实际编译选项 | experience(方法论,未来依赖引入决策可复用) |
| F-032 | 本仓 video_codec 落库值域取证:来自 Media Foundation 友好名映射表(`media_foundation.rs:864-881`),值为 `"HEVC"`/`"H264"` 等(非小写 fourcc `hev1`/`hvc1`);AV1 无映射落 `NULL`——diagnoseVideoError 按此值域做字符串匹配无 fourcc 缺口 | code(已内嵌代码注释,冻结引用即可) |
