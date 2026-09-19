---
status: 施工中
type: 工作记忆
line: 去重功能全局方案
created: 2026-08-30
---

# 进度日志:精确内容去重施工

## 会话:2026-08-30
- 做了:完整读取 AGENTS.md、docs/README.md、去重设计、状态和既有方案 worklog；读取 `/planning` 技能；检查工作树。
- 验证:工作树为 `dev...origin/dev [ahead 18]`，初步无未提交改动；既有方案明确 P0–P4 均未施工。
- 做了:创建本施工三件套，冻结主代理负责的共享集成文件范围。
- 做了:波次 0 三个只读 Luna（Max）完成交叉侦察；确认 token/seen 同 root 代次竞态、V25 schema、抽样 hash、硬删失败语义和 P3/P4 UI 边界。
- 做了:启动波次 1 三个 Luna（Max）实现切片；主代理集中添加 BLAKE3 依赖并准备迁移/模块集成。
- 做了:额度恢复后续派 Faraday/Mendel/Sartre 收口中断切片；主代理补齐编辑/增强 ingest 的纳秒 mtime 构造器、crate 入口和稳定去重错误码。
- 验证:V26 迁移、dedup hash 9/9、media_upsert 13/13、mark_missing 12/12、scan_run 2/2、ingest 3/3 通过；cargo check --locked 通过；波次红队曾因长时间 Cargo 无回执，短审因未读到共享 diff 结论不可判定，主代理已按实际代码独立复核并记录后续风险。
- 结论:波次 1 P0/P1 基线已集成，进入波次 2；需在后续补强 source_revision 条件写回、Live Photo 代次和物理清理边界测试。
- 做了:波次 2–4 已完成任务状态机、keyset 查询、稳定 IPC、`/duplicates`、keeper/保护标记、应用回收站、cleanup journal、启动恢复和跨路径安全预检；共享集成文件由主代理复核后保留。
- 做了:补强可疑 mtime 变化路径：change fingerprint 相同不再保留 exact sidecar；推进 `source_revision`，并在 companion 变化时同步使主项组合摘要失效；新增同 hash touch 与 companion touch 回归测试。
- 做了:补强历史 `remove_media_items_hard`，系统回收站失败时立即返回稳定 OS 错误，不再静默删除 DB 行；补充 IPC harness 的去重状态、组、成员和预检夹具。
- 验证:聚焦 `media_upsert` 测试 14/14 通过；`cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --locked -- -D warnings`、`cargo test --workspace --locked` 均通过，核心库 1166 passed / 0 failed / 6 ignored，工作区退出码 0。
- 验证:前端 `npm run lint`、`npm run typecheck`、`npm test`、`npm run build` 均通过；Vitest 143 个文件、1605 个测试通过，生产构建 2614 个模块并通过 bundle budget；`git diff --check` 通过。
- 终审:启动只读 Luna（Max）进行全链路 P0/P1/P2 审查，并尝试恢复此前中断上下文；代理初期未返回终稿，主代理当时完成了源码复核和全量测试，但该次不计作独立通过证据；平台回收站/网络卷/移动端仍保留手工验收边界。
- 终审复核:此前终审代理随后返回 4 项 P1 与 1 项 P2：绝对路径保真、cleanup journal 混合状态恢复、回收站对象替换竞态、mtime 相同但 size 变化失效、前端 groupKey 异步回写。原 closeout 已撤回，待修复和重新验收。
- 2026-08-31 续施工:额度恢复后恢复并核对 A/B/C 三个中断子任务；A 修复 POSIX/Windows 根标记，B 改为 succeeded item 级 DB 收尾恢复，C 修复同 mtime 不同 size 失效与前端迟到响应。三者实际 diff 与聚焦测试均已审阅。
- 2026-08-31 续施工:主代理补齐 quarantine journal helper、V29 migration、同卷原子隔离/无覆盖恢复、启动 writer 锁外对账，并修复隔离目录层级校验；新增 cleanup/boot/query/frontend 回归测试。
- 验证:cleanup 17/17、boot 5/5、dedup query 11/11、前端 dedupStore 1/1 通过；Rust 全工作区核心库 1175 passed / 0 failed / 6 ignored，clippy 与前端 lint/typecheck 已通过。此后全量 `npm test` 为 144 文件/1606 测试通过，`npm run build` 转换 2614 个模块并通过 bundle budget；最终 Rust 等价检查与只读终审仍待收口。
- 2026-08-31 最终门禁:修复全量暴露的 boot 启动恢复夹具（缺 `unit_digest/unit_size`，导致 2/2 失败后通过，单测 6/6）以及 workspace `--all-targets` 下的 bench `source_revision` 初始化与 3 处 `items_after_test_module` 门禁。`cargo clippy --workspace --locked --all-targets -- -D warnings`、`cargo test --workspace --locked`、`npm run lint`、`npm run typecheck`、`npm test`、`npm run build` 均通过；Rust 主库 1221 passed / 0 failed / 6 ignored，Vitest 144 文件 1622 测试。

## 回顾(收口时填)
- 亮点:把“扫描变化侦测”和“字节级重复证据”拆成两个代次受保护的流水线；系统回收站与 DB 删除由 journal 明确分段。
- 教训:抽样指纹相同只能保留普通派生缓存，不能保留 exact digest；mtime 纳秒和 companion 逻辑单元都要纳入失效链。
- 意外:初始的只读终审子任务两次在 Cargo 工作树扫描窗口内未返回；主代理据源码、聚焦测试和全量回归独立完成收口，最终仍保留平台手工验收边界。
- 意外:最终只读终审代理（Parfit）恢复后仍未返回可消费终稿；不能把它当作独立通过证据。主代理以源码、聚焦/全量测试和最终门禁完成收口，系统回收站等平台边界继续按 fail-closed + 手工验收处理。
