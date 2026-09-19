---
id: 2026-07-22-status-AI与人脸流水线根治
status: active
type: rolling-status
line: AI与人脸流水线根治
created: 2026-07-22
---

# AI与人脸流水线根治 · 滚动状态

- ✅ 一期收官(2026-07-22):MF 读帧异步回调化+30s 超时+泄漏隔离(e670369/3f15308)、可观测性三修(874f511)、V22 orphan_count 毒任务防线(acdca33)、beforeDevCommand 前置 ai-worker 构建(f3939a5);真机实证 5.9s completed、125 毒批全清、全量四门绿;GUI 手测通过(用户确认)。worklog:docs/worklogs/2026-07-22-AI与人脸流水线根治/。
- ⬜ **F-029 二期候选:video-worker 进程隔离**——视频解码丢子进程(复用 ai-worker 行协议+supervisor 基建),MF 挂死类故障的彻底解;一并覆盖 seek(SetCurrentPosition)无护栏已知边界与乱序毒文件每文件损一次 30s 超时的残余面。启动须用户点名立项。
