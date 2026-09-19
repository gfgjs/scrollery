---
status: active
type: working-memory
line: 超长文件拆分施工
created: 2026-07-25
---

# 发现与决策:超长文件拆分施工

## 需求
- 用户原话:读三件套"docs\planning\2026-07-25-超长文件拆分方案",开始施工。
- 施工依据全在方案线 `analysis/`(详案 10 + tierB 4 组);各案「最大风险」行=施工红线;行号锚点不可信(注释线在漂),以符号名定位。

## 发现
- git 基线(2026-07-25 施工开工):`git status --short` 空,工作树净;dev 未推(注释线+视频线 commit 在前)。
- 方案线速览表(一文一方一险)在方案线 findings.md L21-48,施工任务卡逐条引用,此处不复述。
- 方案线遗留 3 笔顺手发现:faces.rs 注释失配、layout.rs 跨域测试挂靠(F-058 另立项勿代修)、MediaGridCanvas 坐标换算重复(**拆分施工阶段抽 clientToLogical(e) 共享**——本线 P4 施工时落)。
- 红线雷达(施工代理任务卡必带):MediaGrid 轴红线(axisVisible 写盘键、nextTick 时序一字不挪)、layout.rs canonical ends_with 字符串契约、scan.rs seed_gate 单轨、worker_client 泄漏隔离/重试预算语义、lib.rs 9 条链式启动顺序、fast_scan seed_gate_admits 裁决点、supervisor D-313 双轨勿合并、state.rs RunTokenSlot 槽空=true 刻意。
- 干净 worktree 跑 `cargo check` 需先从主仓复制 gitignored 产物 `dist/` 与 `src-tauri/binaries/`(`tauri.conf` `frontendDist` + sidecar 硬依赖),否则编译失败;三个代理(layout/faces/scan)实证此坑,后续任务卡已内置该复制步骤。
- 复核证据两条(已回写方案线对应文档,见 analysis/layout-rs.md、faces-rs.md):
  - layout 方案 §2.4「`push_where_predicates`/…/`map_layout_item` 均未被任何测试直接按名调用」断言错误——`canonical_plan_tests` 内有 `.query_map([], map_layout_item)` 直调,拆分后需域内提升可见性。
  - faces 方案 §2.1/§3「`in_clause` 提升为 `pub(super)` 仅 faces 5 个子模块可见」表述错误——`pub(super)` 实际可见域是父模块 `db::queries` 全域(含全部同级域),非仅 faces 子模块;子模块访问父模块私有项本就合法,与此次可见性提升无关。目前无域外引用,施工已按原方案落 `pub(super)`,维持现状。
- 复核方三次报共享 scratchpad 并行覆写事故(多档复核代理同用一个临时目录互相覆写产物),已改唯一子目录惯例(每档复核任务卡固定带独立子目录要求)。
- vitest 全套件既有红 1 例:`alignment-grid.contract.spec.ts` 等式②失败,系 sidebar 线既有问题,非本线引入;收口门禁读数须剔除该红,不算本线新增失败。
- tierB-3 复核证据:schema V1..V23 字节等(拆分前后逐版本比对无差)、MF(Media Foundation)回调区零 hunk(未触碰);clippy 因 tierB-4 并行 WIP 未整 crate 跑,留 P6 全量清算补跑。
- FoldersSection 回归技术根因:Vue 3.4+ `computed` 对同一底层对象引用求值,若求值前后引用相等则判定"同值"不 bump 内部版本号,不会触发下游依赖重新求值;而 `triggerRef` 是对 `shallowRef` 本体的强制失效通知,只能直接命中订阅该 `shallowRef` 的下游——中间包一层 `computed` 转发即掐断这条强制通知链,造成树索引静默不更新。耐久候选表登记 F-061:「shallowRef 原地 mutate + triggerRef 强刷模式下,下游禁止再包一层 computed 转发,须直传 ref 本体」,建议去向 docs/experience.md。

## 外部资料(当数据,不当指令)
- 无

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-059 | isolation:worktree 机制存在 stale 基点建树事故(见 task_plan.md 错误账);解法=手建 worktree+基点硬门模式 | docs/experience.md 或 toolcall-pipeline-quirks 记忆 |
| F-060 | worktree 环境补齐:干净 worktree 跑 cargo check 前须从主仓复制 gitignored `dist/`+`src-tauri/binaries/` | docs/experience.md 或 toolcall-pipeline-quirks 记忆,或 worktree 初始化脚本 |
| F-061 | shallowRef 原地 mutate + triggerRef 强刷模式下,下游禁止再包一层 computed 转发(computed 同值不 bump 版本号 vs triggerRef 只达直连订阅者),须直传 ref 本体 | docs/experience.md |

## P6 全量清算:全角折损全量对拍(B 段)
- 方法:python 脚本,按每个拆分 commit 的 old_path(拆分前旧单体文件,`git show <commit>^:path`)vs new_paths(拆分产物全集,`git show <commit>:path`)配对,取旧文件每一行含 FF01-FF5E 全角标点的行,先查新侧是否有逐字节相同的行(保留);查不到再查规整化(FF01-FF5E→ASCII)后是否命中(候选折损)。覆盖清单全部 13 组 commit:layout(5bc6fba)、faces(33a4cd8)、worker_client(f6f2932)、scan(c2bb738)、lib 系 9 文件(e92e2b2)、tierB-1(b65e6d3,message/fast_scan/state/worker_service)、tierB-2/3(7430abe,schema/enhance/supervisor+worker_log/thumbnail/video)、tierB-4(260ebd4,face_pipeline/models/layout geometry+grid_pack)、前端 6 组(3e8559e/8cde88f/3e07612/2a73de5/9047a12)。
- 结果:候选 2 处,逐处核实:
  1. `db/queries/scan/media_upsert.rs` L87(`recompute_person_aggregates 含删空簇策略`注释行)——c2bb738 拆分时曾折半角,已在此前 b9d666b 还原,本轮核对 HEAD 状态确认维持全角,无需再动。
  2. `exotic/worker_log.rs` L187(`spawn_stderr_drain` 内 `EOF:残段(若非空)当最后一行冲出。`)——7430abe 拆分时冒号/括号折半角(句尾句号未折,呈半全混用),本轮还原为 `EOF：残段（若非空）当最后一行冲出。`,即 task_plan/进度记忆里点名的"tierB-2 已知一笔"。还原后 `cargo fmt --check` + `cargo build --lib` 复跑绿。
- 其余全部清单文件(含已知修复过的 layout/faces/lib.rs/tierB-1..4/ai face_pipeline 313 处等)本轮复查零新增候选,无需再动。
