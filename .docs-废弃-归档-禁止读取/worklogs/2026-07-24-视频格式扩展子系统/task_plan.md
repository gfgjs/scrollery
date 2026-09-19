---
status: 快照
type: working-memory
line: 视频格式扩展子系统
created: 2026-07-24
---

# 任务计划:视频格式扩展子系统

## 目标
摸清当前视频播放格式支持面,调研业界成熟方案,产出「支持更多视频格式」子系统的架构与实现方案(做成 exotic 插件 worker,进插件商店);本线只到方案定稿,施工待用户批准另立批次。

## 当前阶段
阶段 4:施工(worktree `video-format-ext`,基线 6453a16;用户 2026-07-24 批准施工并「采纳全部建议」= 6 开放问题按 design.md 建议值终裁,见 D-444)

## 依赖 DAG
```
批1(并发): A 播放栈摸底 | B exotic插件接入面摸底 | C 业界联网调研
                └───────────────┴───────────────┘
批2(依赖A+B+C): architect 架构设计
批3: 主线定稿 task_plan 方案 | phase-closer 归并 findings + progress 回写
```

## 阶段

### 阶段 1:摸底(三路并发)—— A/B/C 三路 findings 已产出 → scratchpad/findings-{player-stack,exotic-plugin,industry}.md
- **状态:** complete

### 阶段 2:架构设计 —— architect 综合三路 findings 产出 design.md(路线裁决+worker 边界+协议扩展+商店接入+许可合规+分阶段实施计划)
- **状态:** complete

### 阶段 3:定稿回写
- [x] 主线把方案终稿写入本文件「方案」节(见下);归并 scratchpad findings 进 findings.md、回写 progress
- **状态:** complete

### 阶段 4:实施(2026-07-24 用户批准,worktree 隔离施工)
- 施工 DAG(design.md §8):`批1: V1协议 ∥ V2 FFmpeg工具 → 批2: V3 video-worker crate → 批3: V4 host服务 → 批4: V5缩略图桥 ∥ V6播放链路 → 批5: V7前端 → 批6: V8收口`
- 每批:implementer 施工 → reviewer 复核(与下批并行)→ commit → phase-closer 回写 progress
- V6 涉 `derivations.rs`(dev 树用户 WIP):worktree 基线版追加式施工,合并回 dev 时与用户协调冲突,禁 stash/checkout dev 树
- **状态:** in-progress(批1 施工中)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 本线只做分析+设计,不动代码 | 用户 WIP 文件多(VideoSeekBar/MediaGrid 等勿碰);方案未批不施工 | |
| 6 开放问题按建议值终裁:①license_tier=free(稀有格式包 paid 另立 SKU)②缓存池默认 20GB+单文件超池 50% 弹确认 ③v1 含全转码兜底、catalog 认领 rmvb/vob ④HEVC 默认引导装免费扩展、本地转码可选 ⑤V6 在 worktree 按已提交基线施工、合并时协调用户 WIP ⑥macOS FFmpeg 自建后置到 macOS 线启动 | 用户 2026-07-24 明示「采纳全部建议」 | D-444 |
| 路线=remux 优先+半转码档(仅音轨)+兜底一次性转码,同一 video-worker 兼供缩略图后端(替代 feature ffmpeg 进程内旧计划) | 高频缺口是「伪不兼容」容器问题,remux 秒级无损;半转码档覆盖「有画面无声音」场景;同 worker 复用崩溃隔离+低优先级,避免 Windows 进程内链接痛点 | D-441 |
| Service 型 worker(OCR/Enhance 先例),不进 exotic 任务化调度 | mkv/webm/flv/ogv 等是 `classify_media_type` 已认领常见格式,catalog offering 声明会触发 `CommonFormatConflict` 拒绝整个 Catalog(`catalog.rs:97-100`) | D-442 |
| 分发=builtin worker + 商店卡片 + FFmpeg(BtbN LGPL-shared)按需下载,禁 GPL 构建+运行时保险丝 | worker 本体小 Rust exe 编译进应用最省事;FFmpeg 大二进制与安装包解耦,商店按钮语义=授权确认+触发下载;许可红线靠运行时解析 `ffmpeg -version` 检出 `--enable-gpl` 即拒 | D-443 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
