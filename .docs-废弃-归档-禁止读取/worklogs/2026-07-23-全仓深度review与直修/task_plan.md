---
status: 快照
type: working-memory
line: 全仓深度review与直修
created: 2026-07-23
---

# 任务计划:全仓深度review与直修

## 目标
全仓代码深度复核一遍(近期高改动面加权最深),机械无争议问题直接修掉并提交,需权衡/契约级问题汇成裁决清单交用户。

## 当前阶段
收官:P0-P5 + J1-J17 裁决落地全部完成(2026-07-23 用户采纳全部建议,9 commit 03cd68c..cc88f8c);余 GUI 真机验收+待批 push。J3-J6 随 4 Vue dirty WIP。

## 依赖 DAG
```
P0a repo结构摸底(haiku) ─┐
P0b 近期改动面摸底(haiku) ─┼─► P1 全仓复核扇出(reviewer×N,≤8/批)─► P2 发现分诊(主线)
P0c 未提交diff复核(opus) ──┘        (P0c 与 P1 并行,不互等)            │
                                                              ┌───────┴────────┐
                                                              ▼                ▼
                                                     P3 直修施工(域不相交并发)  裁决清单(等用户)
                                                              ▼
                                                     P4 随批复核+全量门禁(phase-closer)
                                                              ▼
                                                     P5 分批 commit(主线 git 单执行者)+回写
```

## 阶段

### 阶段 P0:建卡+摸底(并行三路)
- [x] 三件套建卡
- [ ] P0a repo 结构地图 → scratchpad/repo-map.md
- [ ] P0b 近期改动面(--since=2026-07-14)→ scratchpad/recent-surface.md
- [ ] P0c 工作区未提交 7 文件 diff 深审
- **状态:** in_progress

### 阶段 P1:全仓复核扇出(域划分,待 P0a/P0b 回执后微调)
拟分域(reviewer=opus;R10 用 haiku 快扫):
- [ ] R1 src-tauri db 层(queries/migrations/连接)
- [ ] R2 src-tauri scanner+缩略图/派生管线
- [ ] R3 src-tauri ai-worker/OCR+worker 协议(最新大块)
- [ ] R4 src-tauri exotic 插件/catalog/license(公开契约)
- [ ] R5 src-tauri 视频/MF/ICC 色彩管线
- [ ] R6 src-tauri IPC commands+capabilities+错误类型+路径安全
- [ ] R7 前端 stores/composables
- [ ] R8 前端 components(media/viewer/player)
- [ ] R9 前端 settings/商店/OCR 面板/主题契约
- [ ] R10 tests/CI/脚本/配置快扫
- **状态:** in_progress(与 P0 并行启动,批 ≤8)

### 阶段 P2:发现分诊(主线裁决)
- [ ] 逐发现分「直修候选」/「需裁决」;直修候选按文件域分批
- **状态:** pending

### 阶段 P3:直修施工
- [ ] 域不相交并发施工(≤4 写);**绕开工作区 7 个 dirty 文件**(其发现全进裁决清单)
- **状态:** pending

### 阶段 P4:随批复核+门禁
- [ ] 每施工批过复核;phase-closer 收口跑全量门禁(cargo test/clippy + vitest/vue-tsc/eslint)
- **状态:** pending

### 阶段 P5:提交+回写+裁决清单
- [ ] 主线单执行者按批显式 pathspec 提交;phase-closer 回写三件套;输出裁决清单
- **状态:** pending

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| dirty 7 文件只审不修,发现进裁决清单 | 未提交 WIP 归属不明,直修会污染用户工作区 | |
| 近期窗口取 2026-07-14 起,近期面加权最深;全仓其余按域扫 | 「最近又增加大量代码」是主诉,老代码多线已审过 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
