---
status: 快照
type: working-memory
line: 画廊轴minimap解耦与按钮迁底栏
created: 2026-07-24
---

# 任务计划:画廊轴minimap解耦与按钮迁底栏

## 目标
minimap 不再绑定无缝模式(日期分组也可选);轴按钮(开合/形态/图谱)全迁底栏、版本信息左移;去顶底 fab 修遮挡+常态低透明度;顺手小问题随手修、拿不准列清单待用户裁决。

## 当前阶段
阶段 4:复核 + 门禁 + commit —— 已完(主线复核落 2 项实修 + 1 项待裁,门禁复绿);待用户裁 C-1 与 ⏸GUI 验收

## 阶段

### 阶段 1:摸底 — complete(三路 scout/Explore,结论在 findings.md)
### 阶段 2:方案设计 — complete(architect 出逐文件实施计划,决策落 findings/本表)
### 阶段 3:施工 — complete(两批:批一 config schema 新增 axis_mode + uiStore showSeamlessMinimap→axisVisible/axisMode 更名 + ipcFixtures 同步;批二 MediaGrid/AppStatusBar/TimelineScrubberCanvas 轴按钮迁底栏——门控解耦、按钮迁移、版本左移、fab 修复均落地)

### 阶段 4:复核 + 门禁 + commit
- [x] phase-closer 全量门禁(cargo fmt/test/clippy + vue-tsc/vitest/eslint 六项全绿,证据见 progress.md 会话段)
- [x] reviewer 复核(涉 config 契约)——主线亲审,结论见 findings「复核结论」节:契约面 4 道防线成立,
      落 2 项实修(schema 注释语义过期 / axis 契约零测试)+ 1 项待用户裁(C-1 沉浸态轴钮不可达)
- [x] commit(施工 e1b1789;复核修正另提)、回写
- **状态:** complete(剩 C-1 待裁 + ⏸GUI)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 新 key `axis_mode` 持久化形态偏好 —— 已确认采纳 | 「可选择开启minimap」应跨重启粘滞 | |
| 复用 key `seamless_minimap` 语义扩为轴开合通用、store 字段改名 —— 已确认采纳 | 避免 schema 迁移,StartupConfig 只增不改 | |
| 密度带钮(canvas 内嵌 visualMode/coordMode 钮)方案 B:defineExpose 暴露父组件调用 —— 已确认采纳 | 钮的真身在 TimelineScrubberCanvas 内部状态,父组件(底栏)需跨组件调用而非重复状态 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
