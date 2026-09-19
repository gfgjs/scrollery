---
status: 快照
type: working-memory
line: 视频格式扩展子系统
created: 2026-07-24
---

# 进度日志:视频格式扩展子系统

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:批1-5(V1-V7+补遗)全落,已入 worktree video-format-ext 分支(commit b2216f4/4b43d2a/1327df5/347b701/32b922e);V8 收口完成,余=GUI 手测+合并 dev 协调
- 未解错误:无
- 关键指针:design.md(全文,§0-§11+开放问题)+ 三份 scratchpad/findings-{player-stack,exotic-plugin,industry}.md

## 会话:2026-07-24
- 做了:三路委派摸底(播放栈/exotic 接入面/业界调研)+ architect 综合设计,design.md 落盘(remux 优先三级判定 + video-worker builtin/商店卡片/FFmpeg LGPL 按需下载方案)
- 验证:设计线无门禁,文档批,无代码门禁
- 遗留:阶段 4+(等用户裁决开放问题后另立施工批次)
- 过程账:子代理 A(播放栈摸底)、C(业界调研)的 Write 落盘被 harness 拒,回执改由主线代写落盘完成——教训:检索/调研类子代理的落盘动作不可靠,收尾归并类工作应交 phase-closer 而非依赖调研代理自行写文件

### 施工批1
- V1:exotic 协议 additive 六 op/四 capability/FfmpegUnavailable/SuccessBody 四 video_* 字段(tag="kind");深审 1P0+3P1+3P2 全清(穷举 match 断裂波及 psd/ai/enhance/raw 四 worker 兜底臂,raw-worker 并补 2a36286 遗留 Enhance 三臂);增量核验通过;cargo test -p exotic-protocol 40/40、cargo check --workspace exit 0
- V2:tools.rs FFmpeg 工具管理;BtbN autobuild-2026-07-24-13-32 win64-lgpl-shared 实测(无 --enable-gpl、h264_mf/aac 在,evidence 见 scratchpad/v2-ffmpeg-evidence.md);深审 2P1+8P2 全清(install 重验/fail-closed 镜像/416 边界/.extract.tmp rename/sweep 策略);13 单测绿、clippy 0
- 裁决:D-444 六项(用户采纳全部建议)已入 task_plan
- 验证:批内门禁见 commit message,全量归 V8

### 施工批2(V3 video-worker)
- 新 crate crates/exotic-workers/video-worker(15 文件 +3364):probe/remux/transcode/frames 四能力、GPL 保险丝 fail-closed、Job Object kill-on-close、-progress 节流、输出白名单
- 深审 2P1+10P2:P1=雪碧元数据契约位(video_frames)+白名单拒绝分支 cleanup 任意删除面;修复批 12 项+核验残留 4 项全清,增量核验通过
- 裁决:spawn→assign 毫秒孤儿窗接受注记;out_duration_ms 产物实测回填;configuration 缺失 fail-closed
- 验证:cargo test -p video-worker 60/60;真 ffmpeg e2e 全链路 passed(含真进程 Hello/Ready/SessionInit/Probe 帧级);clippy -D warnings 净;e2e 证据在 scratchpad/v2-ffmpeg-evidence.md「V3 e2e 实跑」节
- exotic-protocol lib.rs 补 Video* re-export(V1 遗漏,随 V3 提交)

### 施工批3/批4(V4/V4b/V5/V6)
- V4:host 服务(双优先队列/抢占/硬取消/超时表/进度观察者参数化)
- V4b:rmvb/vob 收编(format.rs + builtin 豁免 + seed_gate 单轨)+ finalize 心跳
- V5:缩略图桥(rotation 换轴/几何钉死/防御上限)
- V6:播放链路(五态判定表/video_playable kind/20GB 池/6 IPC 命令/TOCTOU 原子 claim/fingerprint 复核)
- 四轮深审 + 三轮增量核验全清,commit 347b701(32 文件 +5548)
- 验证:cargo test --lib 1043/1043、video-worker 65+2 真 ffmpeg e2e、clippy 双 crate 净、vue-tsc 净

### 施工批5(V7+补遗)+ V8 收口
- V7:播放五态前端(VideoPreparingOverlay 等状态组件)、HEVC 系统扩展转码可选引导(ms-windows-store 商店卡片,productid 待真机核实)、播放池统计接入设置页;深审 2P1+10P2 全清
- V8 收口:全量门禁复跑(见下),发现 2 处本线新红并机械修复——`src-tauri/src/video/worker_service.rs` clippy `redundant_pattern_matching`(`matches!(r, Ok(_))` → `r.is_ok()`)、`src-tauri/src/ipc/video_commands.rs` fmt 单文件违规(rustfmt --edition 2021 单文件重排,先误用 edition 2024 格式化出偏差已回退重做);`src/components/media/player/VideoPreparingOverlay.vue` 主题契约新红——消费幽灵 token `--color-bg-base`(六套主题均未定义),按既有同类 scrim 用法改用同义 token `--color-bg-surface`,`theme-contract.spec.ts` 复跑绿
- 验证:cargo test --workspace exit 0(全绿,quiet);clippy --workspace --all-targets -D warnings exit 0(修复后复跑净,enhance-worker/run.rs 3 处 err_expect 确认非本线改动、维持 exit 101 基线红不计入);cargo fmt --all --check 仅余 enhance-worker/{main,run,session}.rs 基线违规(与本线 diff 为空,dev 基线存量);npm run lint exit 0;vue-tsc --noEmit exit 0;vitest run 118 files/1430 tests 全绿;raw-worker cargo check exit 101(libclang 缺口,与已知项一致)

## 遗留(已解/未解合并整理)
- **已解**:av_write_trailer 静默窗(施工批3 finalize 心跳落地,§2.4)、V8 收口两处新红装配(worker_service.rs clippy + video_commands.rs fmt + VideoPreparingOverlay.vue 幽灵 token,均已修复复验绿)、进度细粒度(V4 观察者参数化落地)
- **未解**(V8 未展开深查,归下一步或专项):
  - faststart 产物级深检
  - worker 侧 fingerprint 核对
  - error.rs tracing 竞态 flake 一次隔离(未复现新证据,维持已知)
  - cargo fmt --all --check 基线存量违规:enhance-worker/{main,run,session}.rs(dev 基线即有,非本线产物,本线 diff 为空,未顺手清)
  - raw-worker 独立 gnu 构建本机缺 libclang(bindgen/rsraw-sys 预置环境缺口,V8 复验仍 exit 101,归本机环境已知项)
  - H2b-prod release builtin 校验(design 开放项,未施工)
  - ContentViewer.vue 合并冲突面 import 区(待合并 dev 时人工核对,本线 diff 未触碰)
  - download/fetch.rs:130/150 同步哈希 async 直跑同病(基线存量挂账,随下载引擎线收口)
  - lib.rs:931 疑似宏 panic 降级误报(dist 缺失时 E0614,占位后消失,未再核实)
  - ffmpeg_tool_status 统计接口 IPC 暴露(V7 已接消费面,设置页池统计行待 GUI 手测确认展示正确)
  - e2e ffmpeg 二进制留存 scratchpad ffmpeg-e2e/(临时目录,重启即失)

## GUI 手测清单(⏸,不可自动化,待用户真机验收)
- mkv 直接播放全流程(打开→播放→拖动→关闭)
- 取消 / 重新准备(转码中途取消,重新触发准备)
- 组件下载引导(FFmpeg 工具包缺失时的下载/安装提示流程)
- HEVC 引导卡片(点击后跳转 Microsoft Store,核实 `productid=9n4wgh0z6vhq` 真实落点为对应商品页而非搜索结果/404;若失效需核实付费版 `9NMZLZ57R3T7`)
- 超池确认弹窗(播放池将超容量时的用户确认交互)
- 设置页池统计行(容量/占用/清理展示是否正确)
- 商店声明卡(exotic 商店内 video-worker/FFmpeg 组件卡片展示)
- rmvb/vob 扫描入库 + 缩略图生成
- mkv/webm 封面雪碧图落库(V5 缩略图桥)
- 竖拍 mkv 显示宽高(rotation/几何是否正确不拉伸)
