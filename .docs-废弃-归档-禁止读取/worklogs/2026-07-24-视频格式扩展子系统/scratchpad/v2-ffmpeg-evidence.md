---
id: 2026-07-24-v2-ffmpeg-evidence
status: snapshot
type: plan
line: 视频格式扩展子系统
created: 2026-07-24
---

# V2 FFmpeg 工具管理·实测证据

选包：BtbN/FFmpeg-Builds，**钉死具体 dated release tag**（非浮动 `latest`）。

- release tag：`autobuild-2026-07-24-13-32`
- asset：`ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1.zip`（win64 LGPL-shared 变体）
- 下载地址：`https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-24-13-32/ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1.zip`

## 命令 + 决定性输出

```
$ curl -L -o ffmpeg.zip "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-24-13-32/ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1.zip"
100 59.5M  100 59.5M    0     0  7365k      0  0:00:08  0:00:08 --:--:-- 10.4M
```

```
$ sha256sum ffmpeg.zip
b809e561254cc0634d9fe4ce469c02ac2e194d560e079bb242b96906948ffbeb *ffmpeg.zip
```
zip 大小：62417810 字节。

```
$ unzip -o -q ffmpeg.zip -d extracted
$ sha256sum extracted/.../bin/ffmpeg.exe
1d53a27637354c9e81317b48ec63940260e1312b57f8f9ba9139639f6ff38f75
$ sha256sum extracted/.../bin/ffprobe.exe
05cf93b999f8f2a7988b99c91e04d9ac2e88e2a106aabea1bcfdf3c946b47b4a
```

```
$ ./bin/ffmpeg.exe -version
ffmpeg version n7.1.5-10-g2aefd64d48-20260724 Copyright (c) 2000-2026 the FFmpeg developers
configuration: ... --enable-version3 ... --disable-libx264 --disable-libx265 --disable-libxavs2 ...
```
**决定性**：configuration 行**无 `--enable-gpl`**；`--disable-libx264`/`--disable-libx265` 确认无 GPL 编码器链入（LGPL-shared 构建）。

```
$ ./bin/ffmpeg.exe -hide_banner -encoders | grep -iE "h264_mf|aac"
 V....D h264_mf              H264 via MediaFoundation (codec h264)
 A....D aac                  AAC (Advanced Audio Coding)
 A....D aac_mf               AAC via MediaFoundation (codec aac)
```
**决定性**：`h264_mf` 与原生 `aac` 编码器均在——§3.2 编码器阶梯验证项通过，无需触发 openh264 备选决议。

```
$ ./bin/ffprobe.exe -version
ffprobe version n7.1.5-10-g2aefd64d48-20260724 Copyright (c) 2007-2026 the FFmpeg developers
```

## 回填结果

以上 tag/URL/size/sha256 已回填 `src-tauri/src/exotic/tools.rs` 顶部常量
（`BTBN_RELEASE_TAG`/`BTBN_ZIP_URL`/`BTBN_ZIP_SIZE`/`BTBN_ZIP_SHA256`/`FFMPEG_EXE_SHA256`/`FFPROBE_EXE_SHA256`）。

## V3 e2e 实跑（2026-07-24）

不改代码，仅下载真实 BtbN zip 并跑 `video-worker` e2e 测试真机取证。

```
$ curl -L --max-time 600 --retry 3 -o ffmpeg.zip "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-24-13-32/ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1.zip"
100 59.5M  100 59.5M ... EXIT:0
-rw-r--r-- 1 gf 197121 62417810 Jul 24 22:29 ffmpeg.zip
```
（首次 curl 因 schannel 握手偶发失败 exit 35，重试一次即成功；zip 落地于 scratchpad `ffmpeg-e2e/ffmpeg.zip`，62417810 字节，与 `BTBN_ZIP_SIZE` 一致。）

```
$ sha256sum ffmpeg.zip
b809e561254cc0634d9fe4ce469c02ac2e194d560e079bb242b96906948ffbeb *ffmpeg.zip
```
与 `BTBN_ZIP_SHA256` 常量逐字符一致——**通过**。

```
$ unzip -q ffmpeg.zip -d extracted   # EXIT:0
$ find extracted -iname ffmpeg.exe
extracted/ffmpeg-n7.1.5-10-g2aefd64d48-win64-lgpl-shared-7.1/bin/ffmpeg.exe
$ ./bin/ffmpeg.exe -version   # EXIT:0
ffmpeg version n7.1.5-10-g2aefd64d48-20260724 Copyright (c) 2000-2026 the FFmpeg developers
```

```
$ export SCROLLERY_TEST_FFMPEG=<绝对路径>\extracted\...\bin\ffmpeg.exe
$ cargo test -p video-worker --test e2e_ffmpeg -- --nocapture
test e2e_probe_remux_transcode_frames ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```
决定性输出行：`e2e PASS:probe/remux/半转码/transcode/frames 全链路真跑通过`（测试内自打印，随后 `ok`）。

**结论**：1 个 e2e 测试、1 passed / 0 failed / 0 skipped，退出码 0，probe/remux/半转码(mpeg4→h264 via libopenh264)/transcode/frames 全链路真实 ffmpeg.exe 调用通过。zip 与解压目录留在 scratchpad `ffmpeg-e2e/`（含 `ffmpeg.zip` 与 `extracted/`）供 V4 dev e2e 复用，未删除。