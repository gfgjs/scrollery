---
id: 2026-07-22-AI与人脸流水线根治-closeout
status: snapshot
type: closeout
line: AI与人脸流水线根治
created: 2026-07-22
---

# 收口处置:AI与人脸流水线根治

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-029 | todo | repo:docs/status/AI与人脸流水线根治.md | F-029 二期候选条目(video-worker 进程隔离,seek 护栏与毒文件彻底解归其下) | new | — | yes |
| D-401 | code | repo:src-tauri/src/video/media_foundation.rs@e670369 | 异步回调段设计注释+seek 已知边界注释(一期=超时化不做进程隔离的效力载体) | new | — | yes |
| D-402 | no-promotion | — | — | — | 「不做 mkv 容器挡板」议题随根因修正失效:MF_VIDEO_EXTS 白名单(既有设计)本就把 mkv 拦在 MF 之外,mkv 实为受害者非毒源,挡板无对象;论证归档于本 worklog task_plan 决策表与 findings 阶段6 即足 | yes |
