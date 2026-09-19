---
status: 快照
type: working-memory
line: 全仓深度review与直修
created: 2026-07-24
---

# 发现与决策:全仓深度review(第二轮)

## 严重度合计
0 P0 · 1 P1 · 5 P2 · 6 P3 · 7 裁决

## P1
- **thumb_config 锁中毒**:26 处调用点未跟随本仓 `db_writer` 已有的容毒惯例(锁持有者 panic 后 `Mutex` 永久中毒 → `.lock().unwrap()` 处处二次 panic → 级联崩溃)。→ 修 C1。

## P2
- worker/face pipeline 缺 `panic_guard`(任一次任务 panic 后管线永久卡死循环重试同一毒任务)。→ 修 C1。
- layout 三缓存锁中毒(35 处),同类根因。→ 修 C1。
- `error.rs` 的 `System`/`Os` 变体 raw 串泄漏审查项 → **回退不修**:该变体被双重复用(既承载数十条刻意撰写的双语用户提示,又承载内部 `.to_string()` 拼接),一刀切脱敏会抹掉刻意消息 → 裁定 H,单独立项跟进(见下)。
- `SemanticResultCard.vue`(272 行)全仓无引用,死组件。→ 修 C3。

## P3
- `schema.rs` 补 `dominant_hex` 等预留列注释、`connection.rs` 读池缓存 caveat(R2 发现)、`VideoSeekBar.vue` 换源清 pending 逻辑(R4 发现)——三处均应用于**用户既有工作树 WIP**(改动未由本线提交,留用户自行提交,见 progress)。
- CSP `style-src unsafe-inline` → 接受(既有决策,非本线新增缺口)。
- logs 窗口走 app 命令而非专用轻量通道(R3 发现)→ 接受。
- `configStore` 沿用 Options API 写法(R5 发现)→ 接受(不强制迁移 Composition API,非契约破坏)。
- `anyhow` 死依赖(未被任何 crate 实际使用,R6 发现)→ 修 C4:移除。

## 已核实清除(无需动作)
- `STATE_KEYS` 三键各有归属:`guide_seen` ∈ `STATE_KEYS`(镜像 `first_launch`),`axis_*` ∈ `SETTING_DEFS`——非重复定义。
- 连接池建池处 `lib.rs:314` 实传 8(与池注释曾写的数字对应,详见用户 WIP 中的 connection.rs 注释修订)。
- `image_meta.dominant_*` 系列列 NULL 值路径安全(无消费方假设非空)。

## 裁决 A-H(逐条结论)
| 裁决 | 内容 | 结论 | 归属 |
|---|---|---|---|
| A | 读池页缓存策略 | 接受(现状合理) | — |
| B | trash 边界校验 | 采纳 | 修 C2 |
| C | CSP 现状 | 接受 | — |
| D | logs 窗口走 app 命令 | 接受 | — |
| E | 删除死组件 SemanticResultCard | 采纳 | 修 C3 |
| F | configStore Options API | 接受 | — |
| G | 移除 anyhow 死依赖 | 采纳 | 修 C4 |
| H | error.rs System/Os 定向脱敏 | 采纳,但**独立立项**跟进(非本轮范围) | 见下 |

### H 立项详情(已批准 · 未开工)
`error.rs` 的 `System`/`Os` 兜底变体身兼两职:一是承载数十条刻意撰写的双语用户提示(卷名校验失败、GPU 忙引导、WebDAV 错误、AI worker 诊断等),二是承载约 30 处 raw `.map_err(|e| System(e.to_string()))`(多为 `spawn_blocking` 的 `JoinError`)。一刀切脱敏会抹掉前者的刻意消息,故需定向方案:只改 raw `e.to_string()` 的构造点(逐点换固定文案 + 原始错误串仅进日志;或为 `JoinError` 引入一个「内部任务失败」映射 helper),保留刻意消息不动。敏感度有限(`io::Error` 不含路径),非紧急,单独复核排期。

## 归属对照(直修 C1-C4 ↔ commit,见 progress.md)
- C1 → `c6cc2c6`(锁容毒 + panic_guard,含 P1 + 2×P2)
- C2 → `f0f5654`(trash 边界校验,P2 安全,即裁决 B)
- C3 → `e8ebdf8`(删除死组件,即裁决 E)
- C4 → `141cd32`(移除 anyhow,即裁决 G)
