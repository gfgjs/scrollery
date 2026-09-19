---
status: 快照
type: working-memory
line: 窗口化沉浸模式-非全屏自动隐藏顶栏底栏
created: 2026-07-23
---

# 任务计划:窗口化沉浸模式-非全屏自动隐藏顶栏底栏

## 目标
非全屏窗口下,用户可通过设置开关启用「窗口化沉浸模式」:顶栏与底栏自动隐藏,鼠标移入对应边缘热区时显示,移出后延时隐藏(参考 PotPlayer 行为)。

## 当前阶段
全阶段完成(commit 324453a);剩 ⏸GUI 真机验收 + 待批 push

## 依赖 DAG
```
P0 摸底(沉浸现状 / 顶底栏布局 / 设置样板,三路并发)
 └→ P1 设计裁决(主线:复用 vs 新建 reveal 逻辑、热区尺寸、与 F11 全屏沉浸的关系、4px drag-resize 带风险)
      └→ P2 施工(设置项 + store + 布局显隐 + 热区 hover 逻辑;文件域可能可拆并发)
           └→ P3 复核 + 门禁(复核随批;vue-tsc/eslint/vitest 涉改面)
                └→ P4 commit + 回写(git 写单执行者;phase-closer 回写三件套)
```

## 阶段

### 阶段 0:摸底 — 三路并发(Explore+scout×2,haiku)完成,四组锚点已落 findings.md
- **状态:** complete(commit:待主线 commit)

### 阶段 1:设计裁决 — 主线四条裁决落地(见下表),复用 chromeAutoHidden 不建新机制
- **状态:** complete(commit:待主线 commit)

### 阶段 2:施工 — implementer(sonnet)28 轮改动 9 文件,详见 progress.md 会话段
- **状态:** complete(commit:待主线 commit)

### 阶段 3:复核 + 门禁 — 全量门禁绿;深审 1严重1警告1存疑,修复批+增量核验全过(细节见 progress.md)
- **状态:** complete(commit 324453a)

### 阶段 4:commit + 回写 — 主线单执行者提交 324453a,三件套收官回写完毕
- **状态:** complete

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 窗口化沉浸=chromeAutoHidden 第三来源(\|\| autoHideChromeWindowed 设置),不建新机制 | 唤出/收起/布局绑定全复用,零新状态机 | D-422 |
| 唤出带分档按窗口态:全屏(任意来源)4px、非全屏 8px(含查看器沉浸)| 死区跟 resizable 在场走,非触发来源:窗口化不能 setResizable(false)(F11 红线),wry 带吃 4px,加宽留 4px 有效接收带;窗口化查看器沉浸同吃死区,8px 系顺手修既有缺口;全屏已消带维持 4(复核存疑采纳后更新原「按来源」立论)| D-423 |
| 不加 PotPlayer 式延时隐藏定时器 | 现纯几何判据已验收(全屏沉浸),行为一致、零新状态 | D-424 |
| 查看器局部条(detail-controls/video-player__chrome)不动 | 用户需求=全局顶底栏;局部条自有逻辑 | D-425 |
| setter 次序=先 await 持久化后改本地 state | 承 92d8396 消 IPC 竞速钉定模式 | D-426 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|

## 约束(继承自记忆,勿违)
- F11 线红线:全屏区间 setResizable(false) 修复不得回退;窗口化模式绝不能禁 resize。
- 仓内 dirty 文件(TimelineScrubberCanvas.vue / VideoPlayer.vue / derivations.rs / catalog.rs / fast_scan.rs)不属本线,绕开或冲突即停。
