---
id: 2026-07-24-Spec05_视频与音频
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec05-视频与音频

> 一句话:本篇讲 Windows 平台上视频取帧(Media Foundation + DXVA 硬解)与音频元数据(lofty)的 as-built 解码/播放后端;读前建议先扫一眼 [Spec03-缩略图与派生](./Spec03_缩略图与派生.md) 了解派生调用方是谁。

## 1. 概览

视频子系统只做一件事:把容器文件解码成**正立**(已应用旋转)的 RGBA 帧,交给上层派生/播放器使用,自身不持久化任何图像。音频子系统只做标签/封面/歌词的**纯函数式读取**,不解码音频流本身。

代码位置:

| 文件 | 职责 |
|------|------|
| `src-tauri/src/video/mod.rs` | `VideoBackend` trait + `VideoInfo` + `backend_for()` 后端选择表(§1) |
| `src-tauri/src/video/media_foundation.rs` | Windows 唯一实现:探测/取帧/雪碧图,XVP 尺寸协商,异步回调化读帧,旋转归一(§3) |
| `src-tauri/src/video/d3d.rs` | D3D11 + DXVA 硬解设备单例、槽位限流(§3.1.2) |
| `src-tauri/src/audio/mod.rs` | lofty 标签/封面/歌词纯函数层(§3.3) |
| `src-tauri/src/ipc/player_commands.rs` | 播放器 IPC:字幕加载 `load_video_subtitle`、截帧保存 `save_frame_png`(§4.1) |
| `src-tauri/src/ipc/audio_commands.rs` | 音频 IPC:`get_audio_detail`(§4.1) |
| `src-tauri/examples/video_bench.rs` | 手动基准 + `--rotation-check` 防回归探针(§6) |

在整机中的位置:

```
文件系统(mp4/mov/... 或 mp3/flac/...)
      │
      ▼
┌─────────────────────┐        ┌──────────────────────┐
│ VideoBackend         │        │ audio::read_all /     │
│ (media_foundation.rs)│        │ read_tags / read_cover│
│  probe/cover/keyframes│        └──────────┬────────────┘
└─────────┬────────────┘                   │
          │ RGBA DecodedImage               │ AudioTags + 封面字节
          ▼                                 ▼
  derive/video.rs(封面/雪碧图派生,          ipc/audio_commands.rs
  见 Spec03) → thumbnail 缓存               (get_audio_detail → 前端播放器)
          │
          ▼
  ipc/player_commands.rs(字幕/截帧,消费同一 abs_path,不复用解码会话)
```

`video::backend_for(ext)` 是唯一入口:按扩展名挑后端,当前仅 `#[cfg(windows)]` 下的 `MediaFoundationBackend`;非 Windows 平台返回 `None`,调用方(派生层)将该项标记 `unsupported`(`src-tauri/src/video/mod.rs:91-103`)。

## 2. 数据模型与状态

全表权威归 [Spec01-数据层](./Spec01_数据层.md);本节只摘与解码强相关的列。

`video_meta`(`src-tauri/src/db/schema.rs:156-159` 建表,`:287-291` 扩列):

| 列 | 含义 | 写入方 |
|----|------|--------|
| `video_codec` | 短编解码标签(如 `HEVC`) | enricher,源出 `VideoInfo.codec`(`video/media_foundation.rs:865-882`) |
| `fps` | 帧率 | `VideoInfo.fps` |
| `bitrate` | 平均比特率 bits/s | `VideoInfo.bitrate` |
| `rotation` | 拍摄内在旋转元数据(0/90/180/270) | `VideoInfo.rotation`;**与展示旋转正交**,`schema.rs:783` 有交叉注记 |
| `has_audio` | 是否含音轨 | `VideoInfo.has_audio` |
| `cover_time_ms` | 封面取自哪一帧(ms) | 派生层写入,取值来自 `cover()` 内部选帧逻辑(§3.1 时间戳选择),**非本篇代码直接持久化,由 `derive/video.rs` 侧记录**,待核实(该字段写入点未在本次读码范围内确认具体 path:line) |

`audio_meta`(`schema.rs:163-169` 建表,`:294-298` 扩列):`audio_codec`/`artist`/`album_title`/`track_title`/`track_no`/`year`/`genre`/`lyrics_source`(`embedded`\|`lrc`\|`none`)/`lyrics_path`。均由 `audio::AudioTags`(`audio/mod.rs:22-35`)结构一一映射,写入方是 enricher(未在本篇范围读码,归 Spec03/Spec01)。

`media_items.playback_position_ms`(`schema.rs:826`,V23 决策):播放器续播位置记忆,ms,默认 0。本篇未见写入该列的 IPC 命令(`player_commands.rs` 只有字幕/截帧两条),读写路径待核实——**可能在 `media_commands.rs` 或前端直接经通用更新命令写入,不在 video/audio 模块范围**。

状态归属:

- `VideoInfo`/`AudioTags` 均为解码时**现算的临时结构**,不驻留内存,不跨调用缓存。
- D3D 硬解设备(`Hw`)是**进程级单例**(`OnceLock`,`video/d3d.rs:43`),硬解槽计数是**进程级原子计数器**(`SLOTS_IN_USE`,`d3d.rs:44`)——两者都不持久化,进程重启即重置。
- MF 平台初始化(`MFStartup`)全进程只跑一次(`Once`,`media_foundation.rs:198-204`),故意不调用 `MFShutdown`。

## 3. 关键流程与算法

### 3.1 视频取帧(Media Foundation)

`MediaFoundationBackend` 实现 `VideoBackend` trait 的四个方法(`video/mod.rs:54-84`),分别对应:

#### 3.1.1 探测 `probe()`(`media_foundation.rs:67-109`)

不解码帧,开一个**裸** reader(仍挂异步回调,但 probe 从不调 `ReadSample`,回调永不触发)读原生媒体类型:

1. `ensure_mf()` + `init_com()`(每次调用都做,内部用 `Once`/`CoInitializeEx` 天然幂等)。
2. `open_reader()` 拿 reader(`media_foundation.rs:534-566`)。
3. `GetNativeMediaType(FIRST_VIDEO_STREAM, 0)` 读**转换前**的原生类型——比协商后的类型更准确。
4. 从原生类型读 `MF_MT_FRAME_SIZE`(宽高)、`MF_MT_VIDEO_ROTATION`(旋转,`normalize_rotation` 归一到 {0,90,180,270})、`MF_MT_FRAME_RATE`(帧率)、`MF_MT_AVG_BITRATE`、`MF_MT_SUBTYPE`(经 `codec_label` 映射)。
5. 90/270 旋转时交换宽高得到**显示尺寸**(与图片 EXIF orientation 同理,`media_foundation.rs:88-93`)。
6. `read_duration_ms()` 读 `MF_PD_DURATION`(100ns 单位的 `PROPVARIANT`,`media_foundation.rs:849-861`)。
7. 反选视频流之外的其余流不做(probe 不调 `select_video_only`)——仅探测是否有音轨:`GetNativeMediaType(FIRST_AUDIO_STREAM, 0).is_ok()`。

#### 3.1.2 硬解会话建立 `open_session()`(`media_foundation.rs:277-339`)

`cover()`/`keyframes()` 共用同一条会话建立路径:

```
open_session(path, policy)
  ├─ try_acquire() 硬解槽位(video/d3d.rs:115-127,原子 CAS,HW_SLOTS=4)
  │    命中 → open_session_inner(path, policy, Some(slot))
  │            成功 → 返回 Session { _hw: Some(slot), .. }
  │            失败 → debug log,回退软解
  └─ 未命中/回退 → open_session_inner(path, policy, None)  (软解)
```

`open_session_inner`(`:294-339`)内部:

1. `open_reader()` 挂 `MF_SOURCE_READER_ASYNC_CALLBACK`(必挂,见 §3.1.4)+ 可选 `MF_SOURCE_READER_D3D_MANAGER`(挂了即硬解请求)。
2. `select_video_only()`:反选全部流再单独选回视频流,省音频解码器初始化(`:572-582`);若反选/选回任一步失败则**回滚**到全选(F-07 修复,旧实现两步都 `let _` 会留下"全流禁用"的坏 reader)。
3. 读原生类型算出 `native_rotation`/`display_w`/`display_h`/`duration_ms`。
4. `request_size()` 按 `SizePolicy` 在**显示坐标系**算目标尺寸,再换算回**解码坐标系**(旋转前)——因为下面会强制关闭 XVP 自动转正,解码输出恒为未旋转朝向。
5. `configure_rgb32()` 把首个视频流输出类型定为 RGB32(内存序 BGRA),按需带尺寸协商请求;协商被拒(个别驱动/管线)则静默回退原生尺寸,交给 CPU `resize_rgba` 兜底。
6. `pin_no_xvp_rotation()`:枚举变换链上全部 MFT,对实现 `IMFVideoProcessorControl` 的调 `SetRotation(ROTATION_NONE)` 强制关闭 XVP 自动转正(见 D-003,§4)。
7. `output_geometry()` 读协商后**当前**类型的宽高 + 带符号 stride,作为后续每帧读取的几何权威。

#### 3.1.3 尺寸策略

`SizePolicy` 两种(`media_foundation.rs:219-226`,`request_size` 实现于 `:379-403`):

- `FitLongEdge(max)`:正立后长边 ≤ `max`,**不上采样**(小于目标直接原生直出返回 `None`);长边钉死为 `max` 消除浮点回绕。封面用,`cover()` 调 `open_session(path, SizePolicy::FitLongEdge(max_long_edge))`。
- `CellHeight(h)`:正立后格高恒为 `h`,格宽按显示比例推导(`sprite_cell()`,`:368-375`),**允许上采样**保证雪碧格统一尺寸。雪碧图用。

`sprite_cell` 与 `request_size` 的 `CellHeight` 分支**必须用同一算式**——否则协商输出与目标格差 1px,逐帧空跑一次 CPU 缩放(注释显式标注,`:366-367`)。

#### 3.1.4 异步回调化读帧(`media_foundation.rs:405-529, 665-735`)

根因(见 §4 不变量首条):同步 `ReadSample` 对个别损坏/不支持字节流的容器会在 MF 内部事件上**死等永不返回**,且僵死 reader 会占住 MF **进程级共享工作队列**并扩散到后续所有 reader,把 rayon 派生池逐个拖垮。

修复方案:

1. reader 挂 `MF_SOURCE_READER_ASYNC_CALLBACK`,`ReaderCallback`(`:461-500`)实现 `IMFSourceReaderCallback`,`OnReadSample` 把结果(`hr`/`stream_flags`/`sample`)塞进共享槽 `CallbackShared.slot`(`Mutex<Option<ReadOutcome>>`)后 `notify_one()`。
2. 发起线程调 `ReadSample`(异步模式下输出参数全 `None`)后调 `wait_sample_or_timeout()`(`:505-529`)阻塞等 `Condvar`,超时 `READ_SAMPLE_TIMEOUT = 30s`。
3. 超时命中 → 置位 `CallbackShared.timed_out`(`AtomicBool`)→ 返回结构化 `Err`(消息含 "read sample timeout" + seek 位置,不泄底层原始串)。
4. `Session` 的 `Drop`(`:246-273`)据 `timed_out` 做**泄漏隔离**:曾超时则**不析构 reader**(`mem::forget`,含硬解槽一并泄漏并 `warn!` 打印槽占用快照);未超时则正常 `ManuallyDrop::drop`。理由:僵死态下 `Release`/`Drop` 可能与 `ReadSample` 一样死等,宁可泄漏一个 reader(和可能的一个硬解槽)也不能让派生池的清理线程也卡死。
5. `read_frame_at()`(`:673-735`)入口先查 `timed_out`,已判僵死的 reader 直接快速失败,不再发起注定挂死的读(否则封面回退读/雪碧后续帧会各自再等一次 30s)。

`ReadOutcome` 的 `unsafe impl Send`(`:435`)理由见 `media_foundation.rs:426-434` 完整论证:MTA 下 MF 回调对象 agile、`IMFSample` 的 `!Send` 纯因 `NonNull` 保守默认、使用面是单写单读全程经 `Mutex` 建立 happens-before。

#### 3.1.5 旋转处理(D-003)

- ADVANCED 处理管线(XVP)默认会按 `MF_MT_VIDEO_ROTATION` **自动转正**,但实测(Win11 + rot90 基准片)转正后输出类型的 rotation 属性**不清零**——不可作判据,曾与 CPU 侧 `apply_rotation` 叠成双旋转。
- 定案:`pin_no_xvp_rotation()` 强制关闭 XVP 自动转正,`configure_rgb32()` 把原始 `raw_rot` 原样声明在输出类型上(表示"输出仍携带该旋转、由下游补偿"),且**输出 `FRAME_SIZE` 恒显式钉死**(`media_foundation.rs:591-593`:只声明旋转不钉尺寸时,MF 仍会把默认输出尺寸定成旋后宽高,XVP 于是把未旋内容信箱式塞进转置画幅,四角黑边——rot90 基准片 diag 实证)。
- 旋转权威唯一归 CPU 端 `apply_rotation()`(`:899-927`,用 `image` crate 的 `rotate90`/`rotate180`/`rotate270`)。legacy(非 ADVANCED)管线本就取不到 `IMFVideoProcessorControl`,不自动转正,与 ADVANCED+关闭后行为一致,两路归一。

#### 3.1.6 封面选帧(`cover()`,`media_foundation.rs:111-150`)

时间戳 = `min(1s, 时长10%)`(时长未知回退 1s),避开常为黑帧的第 0 帧。最多尝试 5 次,每次 +0.5s,`is_too_dark()`(每 64 像素采样一次亮度,均值 <16 判黑,`:965-986`)判定是否偏暗,取首个不暗的帧;全部偏暗或读取失败则用最后一次拿到的帧,再不行回退读第 0 帧。时长直接取自本会话的 `duration_ms`,**不再为选时间戳单开一次 probe reader**(旧实现的浪费点)。

#### 3.1.7 雪碧图取帧(`keyframes()`,`media_foundation.rs:152-186`)

`n` 张帧在 `[5%, 95%]` 区间均匀采样(跳过片头/片尾黑帧),每帧 `apply_rotation` 转正后再 `resize_rgba()` 规整到统一格尺寸(`:932-961`,尺寸相等时零拷贝直通——XVP 已按格尺寸协商出图时命中;原生回退路径下走 `fast_image_resize` 真缩放)。任一帧解码失败跳过,全部失败才报错。

### 3.2 DXVA 硬解(video/d3d.rs)—— Part3 无对应章节的净新增

`src-tauri/src/video/d3d.rs` 是**进程级单例 + 槽位限流**模块:

1. `hw()`(`:46-96`)首次调用时 `OnceLock::get_or_init`:先 `ensure_mf()`(MF 平台须先启动),再 `D3D11CreateDevice`(硬件驱动类型,`D3D11_CREATE_DEVICE_VIDEO_SUPPORT | BGRA_SUPPORT`),成功后经 `ID3D11Multithread::SetMultithreadProtected(true)` 开多线程保护(MF 内部线程也会访问该 device),再 `MFCreateDXGIDeviceManager` + `ResetDevice` 绑定。任一步失败 → 记一条 `info!` 日志,返回 `None`,此后**恒走软解**——失败不影响正确性,只影响速度。
2. `try_acquire()`(`:115-127`):`SLOTS_IN_USE`(`AtomicUsize`)做 CAS 自旋,`cur >= HW_SLOTS`(=4)即返回 `None`(不阻塞不等待,task_plan D-002)。命中返回 `HwSlot`(RAII),`Drop` 时 `fetch_sub` 归还额度。
3. 槽位上限 4 是经验值:注释说明"消费级 GPU 单视频引擎,3-4 路并发解码已到引擎吞吐,其余任务软解并行避免排队串行化"。
4. 与 AI 推理的 `gpu_token` **无耦合**:硬解用 GPU 视频引擎(NVDEC/QuickSync),AI 用 3D/compute 单元,仅共享显存(DPB 占用几十 MB 级可忽略)。

拿到 `HwSlot` 后经 `MF_SOURCE_READER_D3D_MANAGER` 挂到 reader 属性上(`open_reader`,`media_foundation.rs:559-563`);`sample_to_rgba()` 里的 `buffer.Lock()` 在硬解路径下即 GPU→CPU readback,此时帧已被 XVP 缩小,拷贝量小(`media_foundation.rs:739-742` 注释)。

### 3.3 音频(lofty)

`audio/mod.rs` 是**纯函数层**,不持有状态,供三处复用(enricher/`derive/audio.rs` 封面派生/`get_audio_detail`):

1. `open(path)`(`:105-108`):`lofty::read_from_path` 一次性解析,失败映射 `AppError::AudioMetadata`。
2. `tags_from(&tagged)`(`:111-157`):`primary_tag().or_else(first_tag())` 取标签(规范标签优先,退回如仅 ID3v1 的场景);读 artist/album/title/track/year/genre;`codec_label()`(`:59-76`)按 `FileType` 映射短标签(如 `Mp4` 类型统一映射 `"AAC/ALAC"`,因容器不显式区分);`duration()` 来自音频属性(非标签),故文件即使无任何标签块仍能返回 codec/duration。
3. `cover_from(&tagged)`(`:160-184`):`tag.pictures()` 优先取 `PictureType::CoverFront`,否则退第一张;按 `MimeType` 映射扩展名,未知/JPEG 默认 `jpg`。
4. `read_all(path)`(`:99-102`):标签+封面**单次解析**双产物,供 `get_audio_detail` 用,避免同一文件解析 3 次(标签/歌词/封面)。
5. 歌词来源判定 `lyrics_source()`(`:41-55`):标签内嵌非空 → `"embedded"`;否则同目录 `.lrc`(`find_lrc()`,`:189-198`,先原扩展名后 `.LRC`)存在 → `"lrc"`;都无 → `"none"`。
6. `lyrics_from_tags()`(`:207-220`):内嵌优先,否则读 `.lrc` 文件内容;`is_synced_lrc()`(`:224-238`)启发式检测文本中是否含 `[mm:ss]` 时间戳,决定前端是否按时间轴高亮滚动。

### 3.4 播放器 IPC:字幕加载 + 截帧保存

`load_video_subtitle`(`player_commands.rs:65-96`):

1. 经 `read_blocking` 走读连接池(async command 内 rusqlite 走 spawn_blocking,硬约束)取 item 的 `abs_path`。
2. `path=None` 时 `find_sidecar_subtitle()`(`:101-127`)在同目录找同 basename 的 `.vtt`/`.srt`(大小写不敏感 stem 比较;`.vtt`/`.srt` 并存显式优先 `.vtt`,不依赖 `read_dir` 枚举序)。`Some` 时视为用户经原生对话框选中(授权模型:选中即完成授权,不做归属校验)。
3. `load_subtitle_file()`(`:130-177`):`canonicalize` → 扩展名白名单(`vtt`/`srt`)→ `File::open` + `take(MAX+1)` 单步读(**GA-fix #4**:避免 `metadata().len()` 校验与实际读取间的 TOCTOU 增长窗口)→ 超 5MB 拒绝 → `decode_bytes()`(复用阅读器的 chardetng 编码检测,`reader::encoding::decode_bytes`)。

`save_frame_png`(`player_commands.rs:188-269`):

1. `target.file_name()` + `target.parent().canonicalize()` → bounds check:文件名重新拼回 canonical 父目录(**GA-fix #5**,`file_name()` 天然不含分隔符/`..`,拼接结果恒落在父目录内,杜绝路径穿越)。
2. base64 长度预检(`FRAME_BASE64_MAX_BYTES`,按 64MB 解码目标 × 4/3 估算)→ 解码 → PNG 魔数(`PNG_MAGIC` 8 字节)校验,拒绝非 PNG 字节流。
3. 写 `<file>.tmp` → `sync_all()` → 同卷 `rename()`(项目硬约束:先 tmp 后 rename)。授权模型:`target_path` 仅来自前端系统 save 对话框,覆盖确认由对话框承担,此处不二次确认。

### 3.5 音频 IPC:`get_audio_detail`

`audio_commands.rs:25-75`:`spawn_blocking` 内取 `abs_path` → `audio::read_all()` 一次解析拿 `(tags, cover)` → 组装 `AudioMeta`(懒加载,即便未经 enricher 补全也正确)→ `audio::lyrics_from_tags()` 拿歌词文本+同步标记 → `write_cover_to_cache()`(`:82-103`)把已抽出的封面字节写 `<cache>/audio_covers/<cache_key>.<ext>`(路径经 `thumbnail::cache::audio_cover_cache_path` 单一事实源构造,仅当文件不存在时写入,`cache_key = path|mtime` 语义下封面不可变)。不重新编码,廉价且无损。

### 3.6 线程/平台归属

| 部分 | 线程/进程 | cfg |
|------|-----------|-----|
| `MediaFoundationBackend`、`video/d3d.rs` 全部 | rayon 派生池工作线程(经 `derive/video.rs` 调用)或 tokio `spawn_blocking`(经 player_commands) | `#[cfg(windows)]`(`video/mod.rs:21-24`) |
| MF `OnReadSample` 回调 | MF 内部工作队列线程(进程级共享) | 同上 |
| `audio/mod.rs` | 无平台门控,跨平台(lofty 纯 Rust) | 无 cfg |
| `player_commands.rs`/`audio_commands.rs` | tokio async command → 内部 `spawn_blocking` | 无 cfg(命令层跨平台,底层 `backend_for` 非 Windows 返回 `None`) |

## 4. 契约与不变量(施工红线)

IPC 命令表(全量归 [Spec10-IPC 与错误契约](./Spec10_IPC与错误契约.md);此处摘与本子系统相关的):

| 命令 | 入参要点 | 出参 | 错误码 |
|------|----------|------|--------|
| `load_video_subtitle` | `item_id: i64`, `path: Option<String>` | `SubtitleFile { file_name, content }` | `player_subtitle_not_found`/`player_subtitle_unsupported_type`/`player_subtitle_too_large`/`player_subtitle_io`(`player_commands.rs:27-30`) |
| `save_frame_png` | `target_path: String`, `data_base64: String` | `()` | `player_frame_target_invalid`/`player_frame_decode_failed`/`player_frame_io`/`player_frame_too_large`/`player_frame_unsupported_format`(`:31-35`) |
| `get_audio_detail` | `id: i64` | `AudioDetail`(`item`/`abs_path`/`meta`/`cover_path`/`lyrics`/`lyrics_synced`) | 内部经 `AppError`(`Db`/`Pool`/`System` 等通用变体),无专属音频错误码——`AppError::AudioMetadata` 仅用于 `audio::open()` 内部,`get_audio_detail` 对读取失败取 `unwrap_or_default()` 静默降级,不上抛 |

不变量清单:

1. **勿回退 MF 同步 `ReadSample` 模式**。为什么:同步模式对个别损坏/不支持字节流容器会在错误路径丢事件死等,僵死 reader 占住 MF 进程级共享工作队列并扩散拖垮整条派生流水线(0x80004005 mp4 故障根因,详见 [Spec06-AI人脸OCR](./Spec06_AI人脸OCR.md) 交叉引用与 `docs/worklogs/2026-07-22-AI与人脸流水线根治/findings.md`「阶段0·根因定案」「阶段6·根因叙事最终修正」两节)。异步回调化 + 30s 超时护栏 + 泄漏隔离是唯一已验证方案。
2. **旋转定案 D-003 双钉勿回退**:XVP 自动转正必须经 `pin_no_xvp_rotation()` 关闭,且输出类型必须**同时**声明 `MF_MT_VIDEO_ROTATION` 原值与显式钉死的 `MF_MT_FRAME_SIZE`。为什么:只关自动转正不钉尺寸,或只钉尺寸不声明旋转,都会退化成信箱式黑边或双重旋转(rot90 基准片实证)。**改动本机制必须跑 `examples/video_bench.rs -- --rotation-check <video>`**。
3. **`Send` 论证系 MTA 前提**:`ReadOutcome` 的 `unsafe impl Send`(`media_foundation.rs:435`)成立的前提是回调线程与消费线程都经 `CoInitializeEx(COINIT_MULTITHREADED)` 进入同一 MTA(`init_com()` 保证)。若未来引入 STA 上下文调用本模块,该 unsafe 论证失效,须重新审查。
4. **显式重试归零预算勿改**:`read_frame_at` 的样本读取循环上限 16 次(跳过 null 样本/流 tick)、封面选帧的 5 次偏暗重试、雪碧图采样区间 `[5%,95%]` 均为经验钉定值,改动需重新用真实素材验证黑帧/花屏规避效果。
5. **硬解槽位泄漏隔离是故意设计**:`Session::Drop` 在 `timed_out=true` 时刻意 `mem::forget` reader 与硬解槽,不做"优雅"清理。为什么:僵死态下 `Release()`/`Drop` 可能与 `ReadSample` 同样死等,清理逻辑本身不能成为新的死锁点;代价是硬解槽额度会被逐个永久占用(`warn!` 日志可观测),二期 video-worker 进程隔离(F-029)才是彻底解法。
6. **D3D 硬解失败必须静默回退软解**:`hw()` 内任一步(设备创建/多线程保护/DXGI manager/ResetDevice)失败都只 `info!` 记录后返回 `None`,绝不 panic 或向上传播错误——无 GPU/远程会话/驱动异常是常态,不能因硬解不可用而拒绝播放。
7. **`sprite_cell()` 与 `request_size` 的 `CellHeight` 分支必须同算式**:任何一方改动尺寸计算都要同步改另一方,否则协商输出与目标格差 1px,导致每帧都空跑一次 CPU 缩放(性能回归,无功能性错误,易被忽略)。

## 5. 边界情况与失败模式

| 边界 | 处理 | 呈现 |
|------|------|------|
| 0x80004005(`MF_E_UNSUPPORTED_BYTESTREAM_TYPE` 等)错误路径的 mp4 | 异步回调如实通过 `OnReadSample(hrstatus, ...)` 上报,`outcome.hr.ok().map_err(mf_err)?` 走既有错误路径,不再死等 | 派生行 status=3(失败),不阻塞其余项 |
| mkv 容器 | mkv 本身不是根因(是 MF 进程级工作队列被 mp4 拖垮后的**受害者**);`MF_VIDEO_EXTS` 白名单本就不含 mkv,`can_handle()` 返回 `false` → `backend_for` 返回 `None` | 派生层标记 `unsupported`(需 Perf/FFmpeg 后端,当前未接入) |
| `ReadSample` 30s 超时 | `wait_sample_or_timeout` 返回结构化 `Err` + 置位 `timed_out`;`Session::Drop` 泄漏隔离该 reader(见 §4 不变量 5) | 该次取帧调用报错;`tracing::warn!` 记录路径与超时秒数 |
| XVP 尺寸协商被拒 | `configure_rgb32` 静默回退请求原生尺寸,交由 CPU `resize_rgba` 兜底缩放 | 无用户可见错误,性能路径退化为软缩放 |
| 硬解会话打开/协商失败(拿到槽位但初始化失败) | `open_session` 捕获 `Err`,`tracing::debug!` 记录后回退 `open_session_inner(.., None)` 走软解 | 无用户可见错误 |
| 无音轨视频 | `probe()` 中 `GetNativeMediaType(FIRST_AUDIO_STREAM, 0)` 失败 → `has_audio=false` | 前端播放器按 `has_audio` 决定是否显示音量控件(前端逻辑,归 [Spec11-前端架构](./Spec11_前端架构.md)) |
| 字幕文件超 5MB / 非白名单扩展名 | `load_subtitle_file` 逐条拒绝,返回对应 `player_subtitle_*` 错误码 | 前端按 code 分流提示,message 不携带绝对路径 |
| 截帧 base64 超限 / 非 PNG 字节 | `save_frame_png_sync` 预检+魔数校验拒绝,`.tmp` 残留自动清理 | `player_frame_too_large`/`player_frame_unsupported_format` |
| 音频文件无标签块 | `tags_from` 的 `tag` 为 `None` 分支,`AudioTags` 除 `codec`/`duration_ms` 外全为 `None`/`Default` | `get_audio_detail` 仍返回可用详情(不报错) |
| 音频文件解析失败(损坏/不支持格式) | `audio::read_all(path).unwrap_or_default()` 静默降级为默认空标签 | `get_audio_detail` 仍返回详情,`cover_path=None`,不阻断播放器打开 |
| 编解码不支持(容器内子编码 MF 无法识别) | `codec_label()` 返回 `None`(`video_meta.video_codec` 记为 `NULL`);实际解码失败则走上述超时/错误路径 | UI 侧 codec 标签显示为空,不影响是否可播放的判定(那是 `can_handle`/实际解码结果决定的) |
| seek(`SetCurrentPosition`)理论死等 | **已知边界,未设护栏**——现有挂死证据仅指向 `ReadSample`,seek 走同步返回不经异步回调;`media_foundation.rs:691-694` 注释明确此为已知风险,彻底解法归二期 video-worker 进程隔离(F-029) | 目前无观测到的实际故障,风险留存 |

## 6. 重建指引(从零实现)

依赖顺序(建议路径):

1. **`video/d3d.rs` 之前先有 `media_foundation.rs` 的裸软解路径**——`d3d.rs::hw()` 依赖 `media_foundation::ensure_mf()` 已先启动 MF 平台,且 `open_session` 的硬解分支需要软解分支已能跑通作为回退基线。建议顺序:①软解 probe/cover/keyframes(异步回调化)→ ②旋转归一(D-003)→ ③接入 D3D 硬解与槽位限流。
2. `audio/mod.rs` 与视频链路无依赖,可独立实现,顺序不敏感。
3. `player_commands.rs`/`audio_commands.rs` 依赖上两者跑通后再接。

外部 crate:

| Crate | 版本 | 用途 |
|-------|------|------|
| `windows` | 0.58 | Win32 Media Foundation / D3D11 / COM API,`#[cfg(windows)]` 门控 |
| `windows-core` | 0.58 | `#[implement(IMFSourceReaderCallback)]` 宏展开依赖 |
| `image` | 0.25 | `apply_rotation` 的 `rotate90`/`180`/`270` |
| `fast_image_resize` | 4 | `resize_rgba` 的 SIMD 双线性缩放 |
| `lofty` | 0.22 | 音频标签/封面/歌词读取,纯 Rust |
| `base64` | 0.22 | 截帧命令的 base64 编解码 |
| 可选:FFmpeg sidecar(`perf` feature) | — | 覆盖 MF 不支持的容器(mkv/webm/flv/ogv),**当前代码未接入**,`video/mod.rs:100` 注释标记为后续阶段占位 |

坑与教训:

- 同步 `ReadSample` 死等根因 + 异步回调化修复,详见 `docs/worklogs/2026-07-22-AI与人脸流水线根治/findings.md`「阶段0·根因定案」「阶段6·根因叙事最终修正」两节,并交叉引用 [Spec06 §5 边界与失败模式](./Spec06_AI人脸OCR.md)。
- XVP 自动转正 + rotation 属性不清零的陷阱,及信箱式黑边现象,是本模块最容易被"看起来对"的实现坑掉的一处——任何触碰旋转/尺寸协商的改动都必须真机跑 `--rotation-check`。
- `sample_to_rgba` 尾像素边界(行尾恰余 3 字节仍须拷贝,不得填黑)有专门单测钉住(`media_foundation.rs:1075-1123` 四个测试),改 `copy_bgr32_to_rgba` 前先读这组测试。

验收:

- 单测:`cargo test -p scrollery --lib video::media_foundation::tests`(纯函数/等待逻辑测试,不触碰真实 MF,可在无视频文件的 CI 环境跑);`cargo test -p scrollery --lib` 覆盖 `player_commands.rs` 的字幕/截帧回归测试(`sidecar_tests`/`load_subtitle_file_tests`/`save_frame_png_tests`,`player_commands.rs:271-506`)。
- 手动基准 + 防回归:`cargo run --release --example video_bench -- <video>... [--rotation-check <video>]`(`examples/video_bench.rs`),后者是旋转 D-003 的真机断言探针,报告协商前后尺寸/rotation 属性/首帧四角与中心像素。
- 4K HEVC 硬解对拍:需真机(GUI 手测项,当前未在自动化 CI 覆盖范围内),按 `video_bench` 的 `hw_available()` 输出确认硬解路径确实被命中。
- 无 mac/iOS/Linux CI 覆盖 — 本模块整体 `#[cfg(windows)]` 门控,非本篇引入的新缺口(全仓已知现状)。

## 7. 关联

- 上游正典:[Part3-缩略图派生与GPU引擎](../refactor_2026/Part3_缩略图派生与GPU引擎.md) §2.1/§2.4(视频关键帧雪碧图现状实测)、§3.1(CPU 解码降采样设计)——**该 Part 全篇聚焦 WIC(图片)CPU 解码链与"GPU 名实不符"的批判(`Part3_*.md:75`:WIC 全链系统内存无 D3D/DXGI),不含任何 DXVA/D3D11 视频硬解章节**;本篇 §3.2 的 D3D 硬解槽位限流是该正典**未规划、施工后净新增**的内容,无对应 Part 章节可引。
- 关键帧 sprite 派生调用方:[Spec03-缩略图与派生](./Spec03_缩略图与派生.md)(`derive/video.rs` 调 `backend.cover()`/`backend.keyframes()`)。
- 前端播放器 UI(VideoSeekBar 悬停 scrub、进度条等):[Spec11-前端架构](./Spec11_前端架构.md)。
- 故障叙述交叉引用:`docs/worklogs/2026-07-22-AI与人脸流水线根治/findings.md`「阶段0·根因定案」「阶段6·根因叙事最终修正」两节(MF 同步模式死等的完整故障复盘)。
- 数据层全表定义:[Spec01-数据层](./Spec01_数据层.md)。
- IPC 错误契约总目录:[Spec10-IPC与错误契约](./Spec10_IPC与错误契约.md)。
