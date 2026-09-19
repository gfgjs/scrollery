---
status: active
type: working-memory
line: 超长文件拆分方案
created: 2026-07-25
---

# 任务计划:超长文件拆分方案

## 目标
全仓盘点 ≥40KB 代码文件(45 个),按分层出拆分方案并落盘 `analysis/`;**纯方案,不动任何代码**(并行会话正在精简注释,施工另立项)。

## 当前阶段
全阶段完毕(在施收尾态,等用户裁:施工立项/收口归档)

## 依赖 DAG
扫描(1) → 分层裁决(2) → **详案×10(3) ∥ 简案×4组(4)** → 汇总回写(5) → 终审+commit(6)

## 阶段

### 阶段 1:尺寸扫描
- 状态:**complete** — 主线直做 1 命令,45 个 ≥40KB 代码文件,榜单见 findings/attachments。

### 阶段 2:分层裁决
- 状态:**complete** — D-446..D-449(见决策表);A 详案 10 件、B 简案 16 件、C 观察 14 件、排除 5 件(vendor×3 + locale×2)。

### 阶段 3:详案 ×10(≥70KB,逐文件一代理,写 analysis/<name>.md)
- [x] 详案×10 全回执,方案落 analysis/(layout-rs/MediaGrid-vue/faces-rs/ContentViewer-vue/scan-rs/FoldersSection-vue/MediaGridCanvas-vue/DocumentViewer-vue/lib-rs/worker_client-rs),主线抽查 MediaGrid/layout 两份合格
- **状态:** complete

### 阶段 4:简案 ×4 组(50–70KB 共 16 件,每组一代理,写 analysis/tierB-N.md)
- [x] 简案×4 组全回执(tierB-1..4,16 文件),基调:测试迁出优先、核心状态机不动
- **状态:** complete

### 阶段 5:汇总回写(phase-closer)
- [x] 各回执要点并入 findings;task_plan 阶段翻转折叠;progress 前情刷新
- **状态:** complete

### 阶段 6:终审 + commit(主线)
- [x] 主线亲读抽查 MediaGrid-vue.md/layout-rs.md 两份详案合格(裁决证据);D-450 拍板;pathspec 只提交本任务目录(commit 见 git log)
- **状态:** complete

### 阶段 7:CSS 外置补录(用户追加,2026-07-25)
- [x] D-451 拍板;6 份 Vue 案(5 详案 + tierB-2 SettingsView)追加附注;findings 补核实证据
- **状态:** complete

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 分层:≥70KB 详案、50–70KB 简案、40–50KB 观察名单 | 详案成本按价值分层,榜单在 ~70K/~50K 有自然断层 | D-446 |
| vendor(foliate-js×3)与 i18n locale×2 不出拆分详案 | vendored 第三方勿动(foliate 线 GPL 红线);locale 是数据文件,拆法=命名空间分模块,与逻辑拆分不同质,一行处置即可 | D-447 |
| 方案锚点以符号名为主、行号区间为辅 | 并行会话精简注释,行号必漂;符号名稳定 | D-448 |
| 本线纯方案不动代码,施工另立项 | 用户明令「先出方案不动代码」;并行注释线在改码,避免写冲突 | D-449 |
| lib.rs 案「向既有文件追加 bootstrap()」接受——迁移落点为既有文件仍属结构移动,行为不变为准绳 | 拒绝会迫使新建单函数小文件,碎片化无收益 | D-450 |
| Vue 大文件 CSS 经 <style scoped src="./X.styles.css"> 外置,scope 归属与 :deep() 语义不变;作可选先行批,首刀实测 Vite 构建链后铺开 | 全仓 .vue 零 v-bind()、六大文件均单一 scoped 块(2026-07-25 核实);风险最低的行数削减刀 | D-451 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
