---
status: snapshot
type: working-memory
line: 视频播放器重构
created: 2026-07-22
---

# 视频播放器重构 · task_plan

日期:2026-07-22 · 分支:dev

## 目标

现有视频播放器过于简陋。先摸底现状 + 联网调研成熟方案,裁定方向(轻完善 / 换播放器 UI 库 / 原生解码根治),再拆批施工。

## 依赖 DAG

```
P0a 仓内现状摸底(general-purpose@haiku)   ∥   P0b 联网方案调研(researcher@sonnet)
                     └──────────────┬──────────────┘
                          P1 主线综合 + 方向裁决(可能需用户裁)
                                    │
                          P2 architect 细化施工计划
                                    │
                          P3+ 施工批次(方向定后拆,写域不相交并发)
```

## 边界与红线

- `src/components/media/TimelineScrubberCanvas.vue` 开工时已 dirty(用户未提交改动)——本线绕开该文件,涉及必先问用户。
- 缩略图 / 关键帧 / hover-scrub 派生流水线为相邻已收官线,不在本线范围,只允许消费其产物。
- License 红线:GPL 系方案一票否决(既有裁决,阅读器线同款);MIT/Apache/LGPL(动态链接)可议。
- Tauri IPC / 权限 / CSP 约束照项目 CLAUDE.md 硬约束执行。

## 状态

全批次施工+复核+commit 完;GF 门禁结果见 progress;余=GUI 真机手测+收口归档待点名。

---

# 施工计划(architect 定稿,2026-07-22)

## 四项裁决定案

### 快捷键:视频上下文夺走 ←/→ 作 seek,切条目改 Shift+←/→

- `kind === 'video'` 时 Space=播放暂停、←/→=seek ±5s、↑/↓=音量 ±5%、M=静音、F=全屏、L=循环、P=画中画、Shift+←/→=上/下条目、`<`/`>`=倍速;`kind === 'image'` 的 ←/→ 切条目不变。
- 理由:seek 频次压倒切条目,YouTube/VLC/mpv 肌肉记忆;视频上下文切条目仍有滚轮/顶栏/Shift+←→ 三路径;命令注册表 `when` 谓词按 kind 分域(`src/commands/builtins/viewer-image.ts:28-36`)收窄 `viewer.prev/next` 到 image,零双绑。
- 机制前提:`eventToCombo`(`src/commands/keybinding.ts:63-71`)扩展——shift 且 `e.key.length > 1`(非字符键)时产出 `shift+ArrowLeft` 组合串;单字符键(`shift+=` → `+`)语义不动。~4 行窄改+补 spec。

### 存储:逐条目进度进 DB(V23),全局播放偏好进 localStorage

- 播放位置 → `media_items.playback_position_ms`(V23),全链镜像 V20 `view_rotation` 先例:schema(`src-tauri/src/db/schema.rs:782-790`)→ migration(`src-tauri/src/db/migration.rs:184-190`)→ query(`src-tauri/src/db/queries/media.rs:396`)→ command(`src-tauri/src/ipc/media_commands.rs:396-405`)→ 前端 latest-write-wins 队列(`src/stores/mediaStore.ts:357-377`)。
- 音量/静音/倍速/循环 → localStorage 单键 JSON `player_prefs`(先例:`detail_show_faces` @ ContentViewer.vue:905)。

### 截帧:不 taint(有条件),前端 canvas 截帧为主路径

- 证据:tauri-2.11.5 asset protocol 对所有响应发 `Access-Control-Allow-Origin: <window_origin>`(`tauri-2.11.5/src/protocol/asset.rs:21,39`);CSP `media-src` 含 `asset:`(tauri.conf.json:44);媒体根已入 asset scope(lib.rs:416-425)。`<video crossorigin="anonymous">` → drawImage+toBlob 不污染(dev/prod 两 origin 均匹配)。
- 主路径:drawImage → toBlob(png) → plugin-dialog save() → 新后端命令写盘(`*.tmp` 同卷 rename)。crossorigin 属性必须随 `:src` 一同渲染,不可事后追加。
- 防御:toBlob 包 try/catch,SecurityError(macOS WKWebView 未实证)→ toast+log;后端 MF 截帧列二期备选。

### seekbar hover 预览:复用 sprite 产物,纳入正编

- 复用 `GET_KEYFRAME_SPRITE`(`src/constants/ipc.ts:51`)+ 帧映射数学(`src/composables/useHoverPreview.ts:116-125,151-167`,含 30s 负缓存 TTL 模式)。
- 只抄逻辑不共享状态(useHoverPreview 模块级 activeId 单例池是网格悬停互斥语义,混用互踢);无 sprite 降级纯时间 tooltip。不触碰 TimelineScrubberCanvas.vue(用户 dirty,零接触)。

## 改动清单

### 新文件

| 路径 | 职责 |
|---|---|
| `src/components/media/player/VideoPlayer.vue` | 编排层:`<video crossorigin="anonymous">`、装配控制条/诊断/字幕/截帧、接线全部 composables;expose PlayerApi(playPause/seekBy/setVolume/toggleMute/setRate/toggleLoop/togglePip/captureFrame/toggleFullscreenPair/videoEl);根上点击=播放暂停(4px 拖拽守卫,mousedown 不 stopPropagation 保 startDrag 平移链) |
| `src/components/media/player/VideoControlBar.vue` | 纯 UI 控制条(UiIconButton:播放/时间「已播/总长,点击切剩余」/音量/倍速/循环/字幕/PiP/截帧/全屏);props/emits 零业务;闲置 3s 自动隐藏;样式全用既有 token |
| `src/components/media/player/VideoSeekBar.vue` | played+buffered 双层、点击/拖拽 seek(拖中预览、松手提交)、hover 时间 tooltip + sprite 缩略帧 |
| `src/components/media/player/VideoVolumeControl.vue` | 静音钮+滑杆;`hasAudio === false` 禁用+「无音轨」tooltip |
| `src/components/media/player/VideoRateMenu.vue` | 倍速 0.25/0.5/0.75/1/1.25/1.5/2/3(UiPopover),clamp [0.25,3] |
| `src/components/media/player/VideoSubtitleMenu.vue` | 关/内挂轨/外挂/「加载字幕文件…」(dialog open);ass/mkv 内嵌显式不支持 |
| `src/components/media/player/VideoDiagnostics.vue` | 播放失败诊断面板(替换黑屏);渲染时 logger.info 落 canPlayType/isTypeSupported 探针矩阵(mkv 争议裁决数据源) |
| `src/composables/player/useVideoPlayback.ts` | 事件监听生命周期+状态镜像(currentTime/duration/buffered/paused/ended/waiting/rate/volume/muted/loop/pipActive)+操作方法;play() rejection 转 paused |
| `src/composables/player/usePlayerPrefs.ts` | localStorage `player_prefs` {volume,muted,rate,loop} |
| `src/composables/player/useVideoResume.ts` | loadedmetadata 按 `detail.playbackPositionMs` 复位(<5s 或 >98% 或 ≥duration 不复位);写=播放中 5s 节流+pause/seek 落定+切条目/卸载 flush;ended → 写 0 |
| `src/composables/player/useVideoSubtitles.ts` | textTracks 枚举、外挂加载(IPC → srtToVtt → Blob URL → `<track>`)、单选切换、Blob URL revoke |
| `src/composables/player/useSeekbarPreview.ts` | sprite 解析(会话缓存+30s 负缓存)+hover 比例→帧 index→background-position(cols=`ui.videoKeyframeCount \|\| 10`) |
| `src/composables/player/useVideoFrameCapture.ts` | drawImage(原生尺寸,不含 viewRotation)→ toBlob → dialog save(`{basename}_{hh-mm-ss}.png`)→ IPC 写盘;SecurityError/取消/失败分流 toast |
| `src/composables/player/useVideoFullscreen.ts` | F 全屏对:`toggleFullscreen/setFullscreen`(`src/composables/useWindowMode.ts:140,161`)与 `viewer.setImmersive` 成对进退;pairFlag 供 Esc 链;不改 useWindowMode 内部(setResizable 护栏红线) |
| `src/commands/builtins/viewer-video.ts` + `.spec.ts` | video 专属命令册(`when: kind==='video'`):playPause(' ','k',ignoreKeyRepeat)/seekBack/Fwd(允许 repeat)/volumeUp/Down/mute('m')/fullscreen('f')/loop('l')/pip('p')/rateUp/Down('>'/'<')/prevItem/nextItem('shift+ArrowLeft/Right',group navigation 10/20)/captureFrame(无键);run 经 `ctx.activeViewer.api` |
| `src/utils/srtToVtt.ts` + `.spec.ts` | srt→vtt(逗号→点、剥编号行、HTML 转义、容错跳坏 cue);vtt 直通 |
| `src/utils/videoDiagnostics.ts` + `.spec.ts` | {extension, MediaError.code, 探针, videoCodec?} → {titleKey, hintKey, technical};规则:mkv 容器/HEVC 无扩展/AV1 版本/文件不可读/未知回退 |
| `src-tauri/src/ipc/player_commands.rs` | `load_video_subtitle {itemId, path?}`(无 path=同目录同 basename `.vtt`/`.srt` 大小写不敏感探测;canonicalize+扩展白名单+5MB 上限+chardetng;返回 {fileName, content};thiserror 结构化码)、`save_frame_png {targetPath, dataBase64}`(canonicalize+bounds check、`.tmp`+同卷 rename) |

### 改动文件

前端:
- `ContentViewer.vue:59-72` 原生 video 整块换 `<VideoPlayer>`(props: src/poster/transform/dragging/itemId/detail 摘要;@loadedmetadata 继续驱动 updateZoomRatio;transform 施加在内部 video,控制条不随缩放旋转)
- `ContentViewer.vue:661-667,636-643` video 分支改诊断面板接管;availability 拦截(:645-647)不动
- `ContentViewer.vue:730,749-757,760,785,862` videoRef 改组件 ref,尺寸经 expose videoEl
- `ContentViewer.vue:993-1030` onKeydown 加交互目标守卫(input/textarea/select/contenteditable 放行;Space 且焦点 button 不分发);Esc 链(:1019-1026)插一步:video 且 pairFlag → 先退全屏对
- `ContentViewer.vue:1158-1170` viewerApi 扩 video 方法透传
- `src/stores/viewerStore.ts:47-63` ViewerApi 增 optional 段(全 `?:`)
- `src/commands/builtins/viewer-image.ts:49-68` `viewer.prev/next` when 由 isImageOrVideo 收窄 isImage;spec 调整
- `src/commands/keybinding.ts:63-71` eventToCombo shift+非字符键;`:15-44` formatKeybinding `' '`→`Space`;spec 补三例
- `src/commands/index.ts:8-20` 注册 viewerVideoCommands
- `src/constants/ipc.ts:63` 邻域增 SET_PLAYBACK_POSITION/LOAD_VIDEO_SUBTITLE/SAVE_FRAME_PNG
- `src/types/media.ts:179-213` MediaItem 增 playbackPositionMs;`:235-241` MediaDetail 增 videoMeta + VideoMeta interface({videoCodec,fps,bitrate,rotation,hasAudio},对齐 `src-tauri/src/db/queries/metadata.rs:118`)
- `src/stores/mediaStore.ts:357-377` 镜像 rotationWrites 增 playbackWrites+setPlaybackPosition;`:409` 邻域导出
- i18n en-US/zh-CN 新 `player.*` 键组一次性全量(GC 落);锚 en-US.ts:350-356 平级

后端(镜像 V20 链,开工先 `rg view_rotation src-tauri/src` 逐点对照):
- `schema.rs:815` 后新 SCHEMA_V23:`ALTER TABLE media_items ADD COLUMN playback_position_ms INTEGER NOT NULL DEFAULT 0;`
- `migration.rs:17,184-190` 挂 V23+幂等重放测试(仿 :370-384)
- `models.rs` MediaItem 加 playback_position_ms;MediaDetail(:295-303)加 video_meta: Option<VideoMeta>(serde camelCase)
- `queries/media.rs:157` get_media_detail LEFT JOIN video_meta;`:396` 邻域 set_playback_position(bind 全参数,clamp ≥0)
- `ipc/media_commands.rs:396-405` 邻域 set_playback_position command(spawn_blocking,与 set_view_rotation 同构)
- `ipc/player_commands.rs` 新+`ipc/mod.rs` 声明+`registry.rs:49-63` 邻域注册

## 并行分组与依赖 DAG

```
G0 keybinding 窄改          ─┐
GA 后端全链+IPC契约镜像      ─┼─→ GC 播放器核心 ─→ GD 命令/快捷键 ─→ GF 批末门禁
GB 纯函数工具(srt/诊断/时间) ─┘        └─────────→ GE 四路接线 ──────┘
```

- G0/GA/GB 写域不相交并发;GA 独占 ipc.ts/media.ts/mediaStore.ts 三个前端镜像件
- GB 含 `src/utils/format.ts` 增 formatPlayerTime(mm:ss / h:mm:ss / 剩余负号)
- GC(依赖 G0+GB):player 组件+核心 composables+ContentViewer 接入+viewerStore+i18n
- GD(依赖 GC):viewer-video+viewer-image when 收窄+index 注册+onKeydown 守卫/Esc 链
- GE(依赖 GA+GC):①useVideoResume ②字幕 ③截帧 ④seekbar 预览
- GF:批末门禁(phase-closer)

## 边界情况(16 条)

1. 无音轨:hasAudio===false → 音量禁用+tooltip,M/↑↓ no-op;video_meta 缺行按有音轨处理
2. duration NaN → `--:--`+seekbar 禁用;Infinity → `seekable.end(0)` 兜底,不可得只显已播禁 seek;resume 跳过
3. 极短视频:seekBy clamp [0,duration];<5s 规则天然跳过 resume;sprite 映射不越界
4. seek 未缓冲区:waiting/seeking → spinner,seeked/playing 清;拖拽只动预览松手提交
5. autoplay 拦截:play() rejection → paused 态+大播放键覆层;poster 链(:591-631)不动
6. 全屏与 F11:F 走 pair;F11 进入的不带 pairFlag,Esc 不越权代退;不碰 setResizable 护栏与 useFullscreenExitGuard 让行契约(ContentViewer.vue:1226-1229)
7. PiP:切条目/卸载 exitPictureInPicture;监听 leavepictureinpicture;pictureInPictureEnabled===false 隐藏按钮
8. 切条目竞态:resume/字幕/sprite 全带 token 防御(镜像 faceToken @ ContentViewer.vue:916);进度写按 id 队列
9. playbackPositionMs ≥ duration → 忽略
10. Space 焦点守卫;repeat:playPause/mute/loop/pip/fullscreen 设 ignoreKeyRepeat,seek/音量允许(keybinding.ts:89-92)
11. 字幕:>5MB 拒;chardetng;坏 cue 跳过;revoke Blob URL;dialog 文件走 IPC 读非 fetch;vtt 优先于 srt;ass/ssa/mkv 内嵌显式不支持
12. sprite 缺失/驱逐:30s 负缓存;背景 404 → 撤预览留 tooltip
13. 截帧:SecurityError → toast+log;8K 帧防抖置 busy;取消 dialog 静默
14. AC-3 无解码=静默无声非 error,诊断不触发,util 注释标已知限制
15. 诊断只在 error 事件后渲染,可播 mkv 不误伤;探针矩阵照常落日志
16. AppToolbar 双分发:其 defaultPrevented 守卫(:392)+ContentViewer preventDefault(:1029)已备,无双触发

## 验证

施工中(implementer):G0=vitest keybinding.spec+typecheck;GA=cargo check+cargo test migration/set_playback_position/player_commands+clippy 包级+镜像件 typecheck;GB=vitest 三 spec;GC=typecheck+eslint 定向+viewerStore.spec;GD=vitest src/commands 全域;GE=typecheck+定向 spec。lint 修复用 `npm run lint:fix` 定向,禁 repo 级 format。

批末(phase-closer):npm run test / typecheck / lint;cargo test workspace 全量 / clippy --workspace --all-targets / cargo fmt(本仓 fmt 直接全量跑);CI 口径 ci.yml。
手测清单(not automated):mkv 探针裁决、HEVC 无扩展黑屏→诊断、autoplay 拦截、PiP、全屏对×F11、截帧 taint 真机(尤 macOS)、内挂字幕轨可见性、倍速音质、拖拽手感。

## 风险与未决点

1. macOS/iOS WKWebView asset CORS 未实证——Windows 先行,taint 实证则二期后端截帧(需点名)
2. mp4 内挂字幕轨 WebView2 可见性未实证——不可见则菜单如实收窄,无需返工
3. mkv 争议由探针日志真机裁决
4. V23 列随 media_items 走备份/恢复,预期无特殊处理
5. viewer-audio Space 统一留邻接后续
6. i18n 键 GC 一次性全量落

(已弃方案清单见 architect 回执原文,要点:弃第三方播放器库/remux/libmpv/element 级 requestFullscreen/localStorage 存进度/双绑仲裁/后端 MF 截帧主路径/共享 useHoverPreview 状态/前端 fetch 读 srt。)

## 决策表(D-ID 全仓全局序,收口时逐行处置进 closeout.md)

| 决策 | 理由 | 候选 ID |
|---|---|---|
| 自研 headless Vue 播放器,不引入 Vidstack/Plyr/media-chrome/Video.js 等第三方 UI 库 | 定制自由度/包体/长期维护权衡后自研更可控;第三方生态本身正合并进 Video.js v10、1.x 维护降速,时点风险高;项目已有六主题 CSS token 体系,自研零皮肤冲突 | D-406 |
| 轻档先行——不做 ffmpeg remux/MSE(中档)、不做 libmpv/MF 原生解码(重档);二期是否立项收口后另裁 | 摸底+调研已证轻档可控(1-3 天工程量对七组件+八 composables 已是上限);中/重档工程量以周计且涉 License 复核,不应绑死在本次轻档批次里 | D-407 |
| 快捷键契约变更:视频上下文 ←/→ 由「切上下条目」改为「seek ±5s」,切条目改绑 Shift+←/→ | seek 频次远高于切条目,YouTube/VLC/mpv 均是裸方向键 seek 的肌肉记忆;视频上下文切条目仍有 Shift+←→/滚轮/顶栏三路径,未失能力;命令注册表 when 谓词天然按 kind 分域,零双绑 | D-408 |
| 逐条目状态(播放位置)进 DB(镜像 V20 view_rotation 全链),全局偏好(音量/静音/倍速/循环)进 localStorage | 项目里逐条目用户状态的既定归宿是 DB(viewRotation 先例);localStorage 逐条目会无界增长、不随备份线走、多库切换错乱;全局纯前端偏好的既定归宿是 localStorage(detail_show_faces 等先例),后端 config 热更新链对四个标量过重 | D-409 |
