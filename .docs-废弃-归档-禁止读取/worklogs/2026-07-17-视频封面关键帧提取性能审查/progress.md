---
status: 快照
type: working-memory
line: 视频封面关键帧提取性能审查
created: 2026-07-17
---

# 进度日志:视频封面关键帧提取性能审查

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-17
- 做了:通读 media_foundation.rs / derive/video.rs / derive/pipeline.rs / video/mod.rs / state.rs(限流+让步)/ derivations.rs(取序);确认无 GPU、定位四类瓶颈、方案分档 T1/T2/T3 写入 findings.md;登记 F-001~F-004。
- 验证:纯代码审读,无构建/测试(未改代码);耗时数字为估算,已在 findings 中标注未实测。
- 用户裁决:**T1+T2+T3 全做**。

### 基线(改前,commit 4c03a6a 工作树,release,i7-14700KF + RTX 3080 Ti)
- 样本:ffmpeg 8.1.1 合成,30s/30fps/g=30;scratchpad vbench/(h264_1080p、h264_4k、hevc_4k(x265 ultrafast)、rot90(480x360 左红右蓝 + rotation=90))。
- 工具:`cargo run --release --example video_bench`(examples/video_bench.rs,口径=后端 probe/cover(含 probe 选时间戳)/keyframes(10),不含 WebP 编码/落盘;3 轮取最优)。
- 数据(best-of-3,暖缓存):
  | 样本 | probe(冷) | cover | kf(10) |
  |---|---|---|---|
  | h264 1080p | 24.2ms | 63.0ms | 416.6ms |
  | h264 4K | 3.5ms | 226.7ms | 1605.3ms |
  | hevc 4K | 2.2ms | 190.5ms | 1155.1ms |
- rotation-check:probe rot=270(ffprobe 侧写 90,MF 报补角,正常),正立 360x480,**top=blue(0,0,255) bottom=red(255,0,0),PASS**——改后此方向必须逐值一致(防双旋/180°)。

### 施工(同日,T1+T2+T3 全落地)
- 改动面:media_foundation.rs 重构(ADVANCED_VIDEO_PROCESSING+XVP 尺寸协商+单会话+反选音频+行级向量化拷贝+免 clone resize)、新增 video/d3d.rs(D3D11 device manager 单例+4 硬解槽位 try_acquire+软解回退)、trait cover 签名改 (path, max_long_edge)、run_cover 砍独立 probe、pipeline.rs 让步拆硬暂停(扫描/缩略图)+交互涓流 1 worker(in-flight 计数)、state.rs 删 should_yield_derivation、Cargo.toml 加 Win32_Graphics_Direct3D feature、examples/video_bench.rs(对拍工具,留仓)。
- **旋转三连败与根因**(细节见 findings F-005):XVP 自动转正致双旋 → 读输出类型 rotation 属性判残余(失败:属性不清零) → SetRotation(NONE)(失败:不拦) → diag 实证「声明 rotation 属性后内容不转了,但输出尺寸仍被 MF 默认成旋后宽高,内容被信箱化」→ 定案:输出类型 **rotation+FRAME_SIZE 双属性恒显式钉死**,旋转权威唯一归 CPU apply_rotation(SetRotation(NONE) 留作双保险)。
- 改后数据(同样本同口径 best-of-3;`hw decode available: true`):
  | 样本 | cover 基线→改后 | kf(10) 基线→改后 |
  |---|---|---|
  | h264 1080p | 63.0→23.7ms(2.7×) | 416.6→213.6ms(1.9×) |
  | h264 4K | 226.7→48.1ms(4.7×) | 1605.3→796.6ms(2.0×) |
  | hevc 4K | 190.5→19.3ms(9.9×) | 1155.1→193.3ms(6.0×) |
  单视频全套(cover+kf):4K H.264 1832→845ms,4K HEVC 1346→213ms(**6.3×**)。
- rotation-check 改后:正立 360x480,top=(0,0,255) bottom=(254,0,0)——与基线逐值一致(bottom 差 1 LSB,XVP 色彩转换舍入,方向正确),**PASS**。
- 验证:`cargo fmt`;`cargo clippy -p scrollery --lib --examples` 净(仅存两条既有构建脚本提示);`cargo test -p scrollery --lib` **629 passed / 0 failed / 6 ignored**(含 4 个拷贝守卫回归 + 新 request_size 策略测试;ignored 含 #[ignore] 的 VIDEO_DIAG 诊断测试)。
- 遗留:真机 GUI 手感验收(⏸,扫描大库观察封面出现速度与交互涓流手感);HEVC 无扩展机器 / 多 GPU / 远程会话下的软解回退未实测(代码路径有,属防御性)。

## 会话:2026-07-17(收尾)
- 做了:三件套回写;pathspec 限定提交(并行会话在动 viewer/底栏线,勿混)。
- 验证:见上节门禁行。
- 遗留:⏸GUI 真机验收。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
