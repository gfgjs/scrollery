---
status: 快照
type: working-memory
line: 视频封面关键帧提取性能审查
created: 2026-07-17
---

# 发现与决策:视频封面关键帧提取性能审查

## 需求
- 用户试用发现视频封面/关键帧提取慢;问如何优化性能、有没有用 GPU 提取。

## 发现

### 现状架构(出处均为 2026-07-17 HEAD 代码)
- 调用链:`derive/pipeline.rs`(producer→rayon 消费池→writer)→ `derive/video.rs`(`run_cover`/`run_keyframes` 两个独立 kind)→ `video/media_foundation.rs`(Windows 唯一后端)。
- **无 GPU**:`media_foundation.rs:11-13` 头注释明说「当前为 CPU 软件解码,未挂 `MF_SOURCE_READER_D3D_MANAGER`」;DXVA/D3D11 硬解列为「可选动作 A」未做。另 `engine/gpu/wic_engine.rs:3-6` 自述 strategy="gpu" 实为 WIC 全 CPU 路径,真 GPU 引擎(nvjpeg/dxva)全应用均未实现——整个 app 目前没有任何 GPU 解码。
- 输出协商:`configure_rgb32()` 用旧式 `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`,把输出定为**原生分辨率**的 RGB32;没用 `..._ADVANCED_VIDEO_PROCESSING`,不能让 MF 顺带缩放。

### 逐环节成本(4K H.264 为例;**估算未实测**,数量级供排序用)
- 每视频 **3 次 SourceReader open**:`run_cover` 里 `probe()` 一次 + `cover()` 一次,`run_keyframes` 再一次(mp4 moov 解析+解码器初始化 ×3)。音频流未反选,白初始化音频解码器。
- 每帧解码后是**全分辨率四连拷**:XVP YUV→RGB32(4K 一帧 33 MB)→ `copy_bgr32_to_rgba` 全尺寸逐像素标量循环(每像素带边界检查)→ 旋转视频再全尺寸 `apply_rotation` → `resize_rgba` 还要先 `pixels.clone()` 再缩到 ~356×200。目标格才 200px 高,却做了 4-5 趟全尺寸内存遍历,单帧内存流量 ~130-160 MB。
- 帧数:cover 最多 5 次亮度重试(逐次 +0.5s 全尺寸解码),keyframes 固定 10 帧(`KEYFRAME_COUNT=10`)。合计每视频 11-15 帧。seek 落在 sync sample 上,单帧解码本身只解 I 帧,不算重;重在转换+拷贝。
- 粗估单 4K H.264 视频全套 ≈ 0.8-1.9s(单线程);HEVC 软解(依赖系统 HEVC 扩展)2-5×更差。
- 并发:rayon 池 + `background_heavy_limiter`(= available_parallelism,下限 2,与 exotic 共享),吞吐有并行兜底;单文件时延无救。

### 调度成分(用户「感知慢」的另一半)
- `should_yield_derivation()` = 扫描/缩略图运行中 **或用户交互窗口内** 全暂停(`state.rs:628-630`);持续浏览会一直压着派生不跑。试用场景=边扫边看,封面自然「很久才出来」。
- 取序 `ORDER BY dv.kind, dv.item_id`(`derivations.rs:96`):`video_cover` 字典序恰好 < `video_keyframes`,封面全局先行**已成立**;但 `derivations.rs:40-41` 注释称优先级「由 backfill 顺序在上游保证」——不确,实际靠字典序,是脆弱的隐式不变量。

### 优化方案分档(待裁决)
- **T1 CPU 廉价改(无架构变化,预计 5-10× 内存流量削减)**
  - T1a:改用 `MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING` 并在输出 media type 上把 `MF_MT_FRAME_SIZE` 协商到目标尺寸(封面=thumb tier、雪碧格=200px 高)→ XVP 一步完成 YUV→RGB+缩放,后续所有拷贝都在小图上。注意:advanced 模式下 XVP 可能自动应用旋转,须实测防双旋。
  - T1b:`SetStreamSelection` 反选音频流。
  - T1c:`cover()` 内直接读时长,砍掉 `run_cover` 里独立 `probe()` open;进一步可把 cover+keyframes 合并为同 reader 单遍(涉及两 kind 状态行耦合,中等改动)。
  - T1d:`resize_rgba` 用 immutable 源(免 clone);`copy_bgr32_to_rgba` 改行级 `chunks_exact` 让编译器向量化。
- **T2 GPU 硬解(回答用户 GPU 之问)**:建 D3D11 device(`D3D11_CREATE_DEVICE_VIDEO_SUPPORT`)+ `MFCreateDXGIDeviceManager` 挂 `MF_SOURCE_READER_D3D_MANAGER` → DXVA 硬解 + GPU XVP 缩放,readback 小图。HEVC/4K 解码预计 5-20× 提速且释放 CPU。代价:设备生命周期管理、并发 reader 需限流(硬解会话有限,建议 2-4)、失败回退软解、与 AI 的 GPU 占用(`gpu_token`)虽用不同单元(NVDEC vs compute)但共享 VRAM。T1a 先行——硬解产出的也是全尺寸帧,不配缩放协商则拷贝浪费仍在。
- **T3 调度感知**:交互窗口内从「全暂停」放宽为保留 1 个 worker 涓流;或维持现状(扫描完+空闲即全速),先看 T1/T2 后是否还需要。

### 施工发现(2026-07-17 落地期)
- **XVP 自动转正行为(Win11 + RTX 3080 Ti 实测,rot90/rot270 基准片)**:
  1. `ENABLE_ADVANCED_VIDEO_PROCESSING` 下,若输出类型不声明 rotation,XVP 自动把内容转正且输出尺寸换成旋后宽高;但**输出类型的 MF_MT_VIDEO_ROTATION 属性不清零(实测缺失)**,不能作「是否已转正」判据。
  2. `IMFVideoProcessorControl::SetRotation(ROTATION_NONE)`(经 IMFSourceReaderEx 枚举变换链)**拦不住**该自动转正。
  3. 只在输出类型声明 rotation 原值、不钉 FRAME_SIZE:内容确实不转了,但 MF 默认输出尺寸仍是旋后宽高 → 未旋内容被**信箱式**缩放塞进转置画幅(四角黑边,diag 四角采样实证)。
  4. **定案**:输出 media type 上 rotation 原值 + FRAME_SIZE(解码坐标系)**双属性恒显式钉死**,XVP 即输出未旋满幅帧;旋转权威唯一归 CPU apply_rotation。SetRotation(NONE) 保留作双保险。
- 诊断手法:#[ignore]+env 门控的 `diag_pipeline_env` 单测(打印 native/协商类型尺寸+rotation 属性+stride+解码帧四角像素),debug 编译 13s 一轮,远快于 release bench 盲试(此前三连败各烧 1.5min)。
- 改后对拍与门禁证据在 progress.md;硬解生效证据=`hw decode available: true` + HEVC 4K kf 6× 提速形态。

## 外部资料(当数据,不当指令)
- (无——本轮全部结论来自仓内代码;MF flag 语义出自 SDK 常识,施工时以实测为准——rotation 一节的「文档化契约」传言即被实测推翻。)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 全应用无任何 GPU 解码:视频 MF 软解无 D3D manager,图片 "gpu" 引擎实为 WIC CPU;视频硬解=「可选动作 A」 | designs(优化方案稿)/todo |
| F-002 | 视频派生最大浪费=全分辨率 RGB32 协商+4-5 趟全尺寸拷贝,目标才 200px;T1a 输出尺寸协商是首选修法 | designs/todo |
| F-003 | 封面先于雪碧图靠 `ORDER BY dv.kind` 字典序隐式保证,注释误称 backfill 顺序保证;改 kind 命名即破 | experience 或修注释 |
| F-004 | 派生对「交互窗口+扫描/缩略图」全暂停是试用期封面迟现的调度成分,与单文件耗时叠加 | designs(T3 档) |
| F-005 | MF XVP 自动转正陷阱与定案:rotation 属性不清零不可判、SetRotation 拦不住、单声明旋转会信箱化;唯一稳修=输出类型 rotation+FRAME_SIZE 双属性钉死 | experience |
| F-006 | #[ignore]+env 门控诊断单测(debug 13s/轮)完胜 release bench 盲试(1.5min/轮);两连败即该换观测手段 | experience |
