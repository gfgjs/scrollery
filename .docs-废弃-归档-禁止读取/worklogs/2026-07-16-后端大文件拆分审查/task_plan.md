---
status: 快照
type: working-memory
line: 后端大文件拆分审查
created: 2026-07-16
---

# 任务计划:后端大文件拆分审查

## 目标
普查后端(src-tauri + crates)单文件巨大行数问题,逐文件裁决「拆分 / 观察 / 豁免」并对值得拆的给出可施工方案落盘;同时核实既有 T 线 queries 拆分方案在 S 线 P3/P4 落地后的时效性,登记漂移与增补。本任务只产出文档,不实施 Rust 重构。

## 当前阶段
阶段 7:获批四批(P1-a/P2/P3/P4-a)已施工落地(2026-07-16 施工会话);余=release 冒烟门复核 + 真机 smoke(GUI)+ 可选 P1-b 待重测裁决

## 阶段

### 阶段 1:现状普查
- [x] 全仓 `*.rs` 行数排序(git ls-files + wc)
- [x] 划定审查阈值:≥1000 行深测绘(10 文件),900–1000 行轻裁决(5 文件),<900 不进本轮
- [x] 统计 2026-07-01 以来各候选文件 churn(commit 数 + 增删行)
- **状态:** completed

### 阶段 2:T 线 queries 方案时效性核查
- [x] 测量 queries.rs 当前口径(行/字节/pub fn/私有 fn/公开类型/测试/消费文件)
- [x] 对 `f3498dd..HEAD` 做符号级 diff,点名新增符号与被改函数
- [x] 核实 S 线 P3/P4 完成度(todo.md S 节:P3 四提交 landed,P4 八门全绿,余真机)
- [x] 形成 T 设计增补裁决:新符号归属 + search.rs 叶子前提被推翻的迁移时点问题
- **状态:** completed

### 阶段 3:候选文件结构测绘
- [x] 4 个并行只读 agent 测绘 15 个候选文件(exotic 组 / ai 组 / ipc+scanner 组 / layout+core 组)
- [x] 汇总各文件:段落图、pub 面、测试占比、bounded context 数、耦合与天然缝
- **状态:** completed

### 阶段 4:逐文件裁决与设计件落盘
- [x] 按判据(领域数/churn/公开面/测试占比,非 LOC 单指标)逐文件三档裁决(拆分 4 批/观察 4/豁免 6)
- [x] 对「拆分」档文件写可施工方案(目标结构/不变量/阶段/验收),落 designs/2026-07-16-后端大文件拆分审查与方案.md
- [x] U 线登记:docs/README.md 字母表(下一空闲 U→V)+ todo.md 新增 U 节
- **状态:** completed

### 阶段 5:T 线文档回写
- [x] T 设计件:状态头追加三次核实 + §4.3 增补两符号归属 + §11 追加 D-012 待裁行
- [x] T 三件套 findings 追加三次核实实测数字;todo.md T 节头注与 P0 行同步
- **状态:** completed

### 阶段 6:代码审查修订与用户裁决
- [x] 结合 J/M/P/N 审查资产、T 线纪律与当前 Rust 源码复核 U-D-001~006/T-D-012
- [x] 落 `docs/reviews/2026-07-16-后端大文件拆分方案代码审查.md`,回写 U 权威方案
- [x] 用户采纳修订后的 U-D-001~006;T-D-012 归 T 线采纳方案 a(2026-07-16)
- [x] 将裁决同步回写 U/T 设计、todo、审查快照与两线三件套;未修改代码
- **状态:** completed

### 阶段 7:获批批次施工(2026-07-16 施工会话,开工 HEAD `1a7a34a`)
- [x] U-P1-a 注册清单下沉(96162c6):按开工 HEAD 重验(lib.rs 1,062→1,013 行漂移、Tauri 2.11.4/tauri-macros 2.6.3 未漂);命令路径集合对拍 186=186;施工中发现并收敛一处方案偏差——11 个命令签名收具体 `AppHandle`(=`AppHandle<Wry>`),`handler<R>()` 泛型形态不满足 `CommandArg<R>`,收敛为具体 `Wry` 返回类型(语义等价,`Builder::default()` 即 Wry)
- [x] U-P2 层次纠偏 + 共享 IPC 下载编排迁移(d169f03/e3fc686 纯搬迁 + 63a3fb3 A15 行为债独立修复):`rg 'ai_commands' src-tauri/src/ai` 归零(含注释)、`rg 'tauri::ipc::Channel' src-tauri/src/download` 归零;dest_safety 两测经 `--list` 确认随迁不丢
- [x] U-P3 exotic/AI 接缝重整(fccc9f0):outcome/validate/worker_traits 三模块 + `pub use` 保旧路径;`--list` 末级名对拍 27=27(validate 11/worker 9/pipeline 7);worker_e2e 编译绿
- [x] U-P4-a QoS 下沉 + 预算纯函数测试(b757203):`thumb_cpu_budget_for(logical)` 边界测试钉死保留策略折点;P4-b 去重继续归 M 线不施工
- [x] 重测 P1-a 后的体量:lib.rs 832 行(setup 闭包 ~585 行仍在);**U-P1-b 维持可选、不自动开工**——P1-a 已消灭最高频冲突面(命令注册),余下 churn 源=setup 生命周期改动(低频),建议数周后重测 churn 再裁
- [x] release 冒烟门四批统跑:`npx tauri build --no-bundle`(2m52s)+ `PICASA_SMOKE_TEST=1 target/release/scrollery.exe` → `boot reached RunEvent::Ready — startup OK`,exit 0,Rust boot→Ready 977ms,退出 WAL checkpoint 与后台任务优雅停止日志正常(本地,非 CI)
- [ ] 真机 smoke(App 起、托盘在、AI 面/缩略图面可用)= GUI 项,不自动化
- [ ] 全部获批批次完成后再由用户决定收口方式;当前按用户指示保留在 `docs/planning/`,不归档 worklog
- **状态:** in_progress(代码 + 冒烟门全绿;余 GUI 真机 smoke + 可选 P1-b 重测裁决)

## 关键决策

| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 审查阈值:≥1000 行深测绘、900–1000 轻裁决、<900 不进本轮 | queries.rs(11,228)是唯一数量级离群点;1000–1400 区间是否拆取决于内聚性而非行数,须逐文件测绘后裁决 | |
| 裁决三档「拆分/观察/豁免」,不以 LOC 单指标机械拆 | 沿用 T 线 F-001 判据:领域数、调用面、churn、历史缺陷综合;内聚算法文件拆了反而制造跨模块耦合 | D-001 |
| queries.rs 不另起方案,只做 T 设计时效性增补 | T 设计已批准且自带抗漂移机制(P0 重建 manifest);重写=制造第二真源 | |
| 新符号 `list_library_formats`/`push_in_predicate` 归 `layout.rs` | 被保护不变量=facet 基础谓词与 `push_where_predicates` 一致性 / IN 谓词参数序号算术;沿 T 设计 §4.3 判据「谁的不变量被保护」 | |
| search.rs 迁移时点按 T-D-012 已采纳方案 a 回写 T §6 | `push_in_predicate` 使 search 不再是叶子;P1 期引用 facade `super::push_in_predicate`,P4 owner 迁移时顺改为 `super::layout::push_in_predicate`,以一处显式路径改点换取 P1 低耦合练手顺序不变 | |
| U-P2 的共享 Channel 下载编排留在 IPC 层(`ipc/model_download.rs`),通用 `download/` 继续只管传输机制 | 当前 `download_assets` 直接接收 `tauri::ipc::Channel`;原搬入 `download/model_assets.rs` 会把 transport 反向带进底层,与现有 `download/mod.rs` 明示边界冲突 | D-002 |
| U-P1-b 先由 `bootstrap::setup` 完整接管 setup 生命周期,再在 owner 内拆块;并后置到 P1-a 重测之后 | 原五块函数化既达不到 `lib.rs ≤300` 目标,又会把启动顺序与捕获量扩散成参数袋;完整 owner 能保持 DB→配置→状态→任务→托盘顺序 | D-003 |
| 豁免只对带 commit 的本轮快照生效,出现结构/增长/缺陷触发器即重审 | 永久「不再复查」与本方案的快照/开工重验纪律冲突,代码演化会推翻内聚性前提 | D-004 |

## 错误账

| 错误 | 尝试 | 解法 |
|------|------|------|
| `grep -n` 搜 todo.md 报 "Binary file matches" | 直接 grep 段落标题 | todo.md 含非文本字节(S 节记载过源码 NUL 字节事件的引用);改 `grep -a` 或 Read 定向读行号区间 |
| P1-a `handler<R: Runtime>()` 泛型形态 11 处 E0277(`AppHandle: CommandArg<R>` 不满足) | 首版按设计写泛型签名 | 多命令直接收 `AppHandle`(默认 `AppHandle<Wry>`),泛型 R 无从满足;收敛为具体 `Wry` 返回类型——原 lib.rs 调用点因类型推断从未暴露此约束,设计期 compile spike 若真跑 10 分钟即会命中(本次施工即 spike) |
| pipeline.rs 拆 trait 后 lib 测试 E0422(`WorkerSpec/WorkerConfig` 不在作用域) | 主体 import 收窄后依赖编译器兜底 | 真实 Worker e2e 测试独用这两类型;测试模组内补显式 `use crate::exotic::worker::{WorkerConfig, WorkerSpec}`——`cargo check` 不编译 `#[cfg(test)]`,搬迁后须跑 `cargo test` 才暴露测试侧缺口 |
