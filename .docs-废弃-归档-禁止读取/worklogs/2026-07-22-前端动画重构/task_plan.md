---
status: 快照
type: working-memory
line: 前端动画重构
created: 2026-07-22
---

# 任务计划:前端动画重构

## 目标
前端动画体系化重构:统一动画 token(时长/缓动 CSS 变量)+ 补齐缺失过渡(画廊→内容查看器、大图浏览器切图等)+ 现存动画调优;全程避让并行「视频播放器重构」线的文件域。

## 当前阶段
阶段 3:收口中(全量门禁已过,GUI 真机手测+push 待批)

## 依赖 DAG
```
阶段0A 动画清单 ─┐
阶段0B 缺失点位 ─┼─→ 阶段1 architect 设计(token 体系+缺失动画方案+调优清单)
阶段0C 冲突区 ✅ ─┘         │
                            ├─→ 阶段2 施工(按文件域不相交分批并行)
                            │       └─(随批)reviewer 复核 + phase-closer 回写
                            └─→ 阶段3 收口:全量前端门禁 + GUI 手测清单 + commit
```
0A/0B/0C 互不依赖已同批发出;1 依赖 0 全回执;2 内部按文件域并发,与随批复核/回写并行。

## 阶段

### 阶段 0:摸底 — complete:三路并行回执齐(动画清单 200+ 点/缺失点位定位/冲突区划定),细节在 findings.md
- **状态:** complete

### 阶段 1:设计(依赖 0)— complete:architect 计划+主线裁决落 attachments/architect-plan.md
- **状态:** complete

### 阶段 2:施工(依赖 1)— complete:五批全落(A=8bee8e4 基座+route-fade,C=9f5ebb1 UiDialog leave,D1/D2=a60acfc token 收编 25 处,B=ecea926 ContentViewer dip 争用区最小改);复核=opus 深审(A/C 零发现,B 两 minor 已修+增量核验)+cavecrew 机扫无发现
- **状态:** complete

### 阶段 3:收口(依赖 2)
- [x] 全量前端门禁:vitest + vue-tsc + eslint(本次执行,均绿)
- [x] GUI 真机手测发现回归(route-fade 黑屏)并已修复,见错误账;commit 1f3ea06
- [ ] 剩余 GUI 真机手测清单(attachments/architect-plan.md §P5,余 5 条:连翻 dip/对话框关闭/reduce/万级帧率/深链)— 待用户
- [ ] 六 commit 待批 push
- **状态:** in_progress

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 排除区:`TimelineScrubberCanvas.vue`(dirty 硬禁)、`src-tauri/src/video/`、`assetUrl.ts`、`useMediaDetail.ts` | 并行视频播放器线触碰域;其线在 P0 未施工,但计划碰这些文件 | |
| `ContentViewer.vue` = 争用区:动画优先挂父级挂载点/包装层,不进其内部;确需进内部则最小化并单独裁决(可能 defer) | 视频播放器线明确计划改它;画廊→查看器过渡恰以它为挂点,包装层方案可解耦 | D-001 |
| 视频播放器自身控件动画不做,归其线 | 避免双线同域 | |
| spinner 全库统一 800ms 单 token(`--duration-spin`) | 用户嫌快慢改一处即全局生效,免各处零散硬编码 | D-403 |
| ContentViewer dip 两条 residual minor(过期 load 误清窗口/回退路径边界可见 dip)接受为已知限制 | 装饰性动画无卡死路径(250ms 兜底+error 清理),争用文件不值再扩 diff | D-404 |
| D2 顺带清 7 文件 prettier 属性折行存量债 | git diff -w + 机扫核证无语义漂移,批内改动前即红,顺手清偿成本低 | D-405 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 画廊进大图黑屏、控件全失(真机复现,`.content-viewer`/`.app-content` 均空,ContentViewer 未挂载) | 先疑 detailItem 竞态派 general-purpose 追因(store/watch 侧排查),中途真机复核推翻前提 | 根因=App.vue `<Transition mode="out-in">` 包 KeepAlive 在 Vue 3.5.13 上 leave 完成后新组件不插入(模板与 vuejs/router#1655 逐字同构,相邻已知缺陷 vuejs/core#12465,3.5.13 早于其修复合并);撤路由级 Transition,查看器进入动画改纯 CSS 挂载 keyframe(`viewer-in`)。commit 1f3ea06。代价:查看器关闭无淡出、其他路由无过渡,defer 待升 Vue 版本 |
