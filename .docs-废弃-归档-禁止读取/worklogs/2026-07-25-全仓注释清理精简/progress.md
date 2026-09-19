---
status: 快照
type: working-memory
line: 全仓注释清理精简
created: 2026-07-25
---

# 进度日志:全仓注释清理精简

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:阶段 3 已收口;**待裁 5 项已全部处置**(用户 2026-07-25 裁「开工」)。四个 commit 落 dev(全部未推):`6f1cd57 fix(gate)`、`d9b337f refactor(comments)`、`762d1e2 style(fmt)`(fmt 5 文件既有债清掉)、`ba3be1c refactor(comments)`(待裁第 2/3 项)
- 5 项去向:①fmt 债→清掉;②state.rs→落地(顺带清同文件 15 处同类);③fast_scan.rs→落地;④en-US.ts `U-2`→自证无误,无需改动;⑤ReaderSettingsSection.vue→跳过是对的,无可删项。逐项取证见 findings.md F-g
- 门禁现状:`cargo fmt --all --check` 🟢(**基线红已消**)/ clippy -D warnings 🟢 / cargo test --workspace 🟢
- 关键指针:findings.md F-a(脚本误删类型)/F-b/F-f(审计回执编造教训)/F-c(误删候选明细路径)/F-d(顺带修的基线红)/F-e(fmt 债,现已清)/F-g(5 项处置)/**F-h(残留量实测 515 行 84 文件,第二轮待裁)**
- 未决:无。F-h(第二轮清残留)已裁**暂不做、留账**,重启须新决策

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:15 代理 workflow 分域严审 1078 hunk,取证式审计对 1016+ 条被删非 CJK 注释行逐条对拍,守住了两次脚本/代理误删
- 教训:复核结论必须落盘可对拍、引文须逐字可查——两份复核回执被查出编造发现(引文在全仓 diff 中不存在),其中一条还被修复代理照抄进代码后经证伪删除,见 findings.md F-f
- 意外:同一批清理里冒出了 5 条无法自动裁决的边界情况(既有 fmt 债范围、既有中英混杂残留、既有重复行、新增文案的可验证性、代理主动跳过的文件),已列入待裁清单

## 会话:2026-07-25
- 做了:阶段 1 摸底(体量统计+首轮英文/双语定位+门禁基线)完成;阶段 2 施工中,规则 1 由确定性 Python 脚本执行(脚本 `C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\2134415b-1664-49db-aa81-5bd689140ab4\scratchpad\dedup_comments.py`,未入仓),规则 2–7 与「精简」分约 15 个域由 sonnet 施工代理人工执行;发现规则 1 脚本存在误删(见 findings.md F-a),已用取证式复核流程核实并修复 BEGIN-INTERNAL 标记(原样恢复,BEGIN/END 计数校验通过,全仓无其他功能性标记被误删)及审计批 A(src-tauri 14 条)、批 B(crates+前端 9 条)、批 C(ipc+db 6 条)
- 验证:门禁基线(改前)记录于本节下方「门禁基线」子节;误删候选复核采用「先机械抽取 diff 中被删非 CJK 行 + 上下文落盘,再逐条对文件判定」,未采信首份 opus 审计回执(该回执引文 8 条经 grep 全仓核验均不存在,已废弃);同一代理二次运行给出的 40 条发现经主线抽验 3/3 属实
- 遗留:阶段 2(CONT/LINK+ID 两组候选复核未完);阶段 3(全量门禁复跑 + 分批提交 + 待裁清单交付用户)

## 门禁基线(改前,阶段 1 摸底)
- vue-tsc 🟢 / vitest 🟢 120 文件 1454 测试通过 / cargo test --workspace 🟢 约 1279 测试通过
- eslint 🔴 185 error(均来自 `.claude/worktrees/**` 与 `target/**` 非源码目录,已在本线修复,见 findings.md F-d)
- cargo fmt --all --check 🔴 44 文件 589 行 diff(dev 既有债,本线不代修,见 findings.md F-e)
- clippy 🔴 3 处 err_expect(已在本线修复,见 findings.md F-d)
- 完整基线记录:`C:\Users\gf\AppData\Local\Temp\claude\c--workspace-scrollery\2134415b-1664-49db-aa81-5bd689140ab4\scratchpad\gate-baseline.md`

## 当前工作树状态(阶段 2,未提交)
- 282 文件改动,+553 / −2860(净减约 2307 行注释)
- 已修复的误删:BEGIN-INTERNAL 标记原样恢复;审计批 A/B/C(共 29 条)全部落地
- 在跑:ipc-A(15 文件)/ ipc-B+db(22 文件)规则 2–7;误删候选 CMD+LIST+NUM 复核修
- 待办:CONT(557)与 LINK+ID(1050)两组候选复核;全量门禁复跑;分批提交

## 会话:2026-07-25(阶段 3 收口)
- 做了:CONT(557 条)与 LINK+ID(1050 条)两组误删候选完成复核;15 代理 workflow 分域严审 1078 hunk,50 条发现经对抗核验确认 11 条并修复;另一轮改写型注释复核 42 条过压缩已全部补回;终局跑 npm run lint / vue-tsc / vitest / cargo clippy / cargo test --workspace,清理落地为两个提交
- 验证:npm run lint 🟢 / vue-tsc 🟢 / vitest 🟢(120 文件 1454 测试)/ cargo clippy 🟢 / cargo test --workspace 🟢;`cargo fmt --all --check` 仍红,文件集与基线相同(enhance-worker 的 main/run/session.rs + src-tauri/src/enhance/service.rs + src-tauri/src/ipc/enhance_commands.rs),属 dev 既有债,本线未代修
- 落地:`6f1cd57 fix(gate)`(eslint 排除 `.claude/**` + clippy err_expect×3)、`d9b337f refactor(comments)`(283 文件,+744/−3068,净减 2324 行,含三件套本身),两 commit 已落 dev,未推
- 遗留:无(阶段 2、阶段 3 均 complete);待用户裁决 5 项见下

## 待用户裁决清单(2026-07-25 用户裁「开工」,已全部处置——保留原文+去向)
1. ~~`cargo fmt --check` 5 文件既有债是否要单开一批清掉~~ → **清掉**,`762d1e2`,fmt 门转 🟢
2. ~~`src-tauri/src/state.rs:126` 改动前就存在的中英混杂残留注释~~ → **落地**,`ba3be1c`(行号已漂,取证锚点见 findings.md F-g)
3. ~~`src-tauri/src/scanner/fast_scan.rs:8-9` 改动前就存在的重复行~~ → **落地**,`ba3be1c`
4. ~~`src/i18n/locales/en-US.ts` 里新增的 `U-2` 工单号标签无法从代码验证~~ → **不是新编**:逐字复制 zh-CN.ts 同 key 既有注释,U-2 与 `ExportSource::View` 各有独立锚点,**无需改动**
5. ~~`src/components/settings/ReaderSettingsSection.vue` 被施工代理按红线跳过~~ → **跳过是对的**:12 条注释全中文且全解释「为什么」,按规则集属「留」,**无可删项**

## 已裁:第二轮清残留 = 暂不做,留账(2026-07-25 用户裁)
- **F-h**:确定性扫描实测全仓仍有 **英文散文注释行 515 行 / 84 文件**、其中 **疑似漏删双语对 150 处**,另 **99 个 .rs 带自指文件路径头**(按规则 5 本该删)。根因=上一轮规则 1 脚本守卫「英文块须单行」,多行英文块整批逃逸。明细与扫描器路径见 findings.md F-h
- **裁决:暂不施工,仅记录**。重启须新决策——不得因「看见英文注释」就顺手清,那会变成一次未授权的全仓改动波

## 会话:2026-07-25(待裁 5 项处置)
- 做了:5 项逐项取证 + 落地。第 1 项 `cargo fmt --all`(只动那 5 文件);第 2/3 项在 state.rs / fast_scan.rs 删 21 处英文冗余块、2 条纯英文孤行改中文、删 1 条重复行、J1 英文复述改中文;第 4/5 项核实为「无需改动」并留证。顺带用新扫描器量出全仓残留规模(F-h)
- 验证:第 1 项 → `cargo fmt --all --check` 🟢(此前长期红)+ `cargo check --workspace --all-targets` 🟢;第 2/3 项 → 机械校验 diff 中**非注释增删行为 0**(`git diff -U0` 过滤 `///|//!|//` 后为空),`cargo fmt --all --check` 🟢 / `cargo clippy --workspace --all-targets -- -D warnings` 🟢 / `cargo test --workspace` 🟢(scrollery-lib 1066 + 各 crate,0 failed);两文件残留英文散文行 53→0、14→0(扫描器复测)
- 遗留:无待裁。F-h 当场裁「暂不做,留账」;四 commit + 两 docs commit 全未推
