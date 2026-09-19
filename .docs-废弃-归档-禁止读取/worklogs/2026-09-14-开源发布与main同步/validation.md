---
id: 2026-09-14-开源发布与main同步-validation
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 公开前源码验证（任务C）

## 绑定

权威树：宿主 `C:/workspace/scrollery`，起点 HEAD `8b8d679f67b158e810d1e010bf69dc33a00bd8c9`。

**绑定口径（执行时共享工作树）**：起点快照（`00-provenance.txt`，02:06:53）为干净树、`git diff` 为空、未跟踪仅 `.research-tmp/` 与本 planning 目录。测量窗口内 A/B 正在改动同一共享工作树（收尾时 `generate-notice.mjs`、`third-party/license-sources.json`、`tauri.conf.json`、LICENSE 相关文档等已变脏），故各步骤实际绑定的是执行时刻的工作树内容，不能整体断言等于干净 HEAD `8b8d679f` 的快照。

私有 `main` 落后 `dev` 149 提交；下列结果覆盖这些累积改动（在 dev 树上测得）。日志：`target/oss-release-check/logs/`（隔离验证目录，未改源/脚本/workflow，未做依赖升级）。

## 通过项（宿主树）

| 项 | 命令 | 结果 | 日志 |
|---|---|---|---|
| 前端类型 | `npm run typecheck` | rc=0（12.8s） | 10-frontend-typecheck.log |
| 前端单测 | `npm test` | rc=0；168 文件 / 1916 用例通过 | 11-frontend-test.log |
| 前端构建 | `npm run build` | rc=0（10.5s） | 12-frontend-build.log |
| Rust 格式 | `cargo fmt --all -- --check` | rc=0 | 20-rust-fmt.log |
| Rust 编译 | `cargo check --workspace --locked` | rc=0（90.8s） | 21-rust-check.log |
| Rust 全套测试（串行） | `TAURI_CONFIG` 源码模式 + `cargo test --workspace --locked -- --test-threads=1` | **rc=0**（100s）；22 个测试二进制 + 5 组 Doc-tests 全绿；1578 条 `ok`；0 失败 | 50-rust-workspace-serial.log |
| RAW 独立 check | `cargo check --locked --release --target x86_64-pc-windows-gnu`（cwd=raw-worker） | rc=0 | 51-raw-check.log |
| RAW 独立 test | `cargo test --locked --release --target x86_64-pc-windows-gnu` | rc=0 | 52-raw-test.log |
| RAW 结构+测试+产物 | `node scripts/acceptance/raw-worker-test.mjs --expect-binary` | rc=0；结构守卫过、3 用例过、产物 9283450 字节 | 30-raw-worker-verify.log |

`TAURI_CONFIG='{"bundle":{"externalBin":[],"resources":[]}}'` 在本机 `cargo test` 下被接受（无配置解析错误），可用作门禁同形复核。

### 两项可疑用例：逐轮逐日志事实

| 运行 | 日志 | `fetch_io_profile_after_fix` | `concurrent_lifecycle_writers_...` |
|---|---|---|---|
| `cargo test --workspace --locked`（默认并行，第 1 轮） | 22-rust-test.log | **FAILED**（L1476） | 挂起 >60s（L1477） |
| `cargo test -p scrollery --lib --locked`（默认并行，第 2 轮） | 25-rust-test-lib-parallel.log | ok（L1357） | 挂起 >60s（L1358） |
| 单跑 `--lib layout::items_cache::tests::fetch_io_profile_after_fix` | 23-rust-test-items_cache-isolated.log | ok（2.14s） | — |
| 单跑 `--lib state::scan_run_tests::concurrent_lifecycle_writers`（默认线程） | 26-rust-test-lifecycle-default-threads.log | — | ok（0.04s） |
| 单跑 `--lib state::scan_run_tests::concurrent_lifecycle_writers`（`--test-threads=1`） | 24-rust-test-lifecycle-isolated.log | — | ok |
| `cargo test -p scrollery --lib --locked -- --test-threads=1` | 27-rust-test-lib-serial.log | ok | ok（1347 通过 / 9 ignored） |
| `cargo test --workspace --locked -- --test-threads=1` | 50-rust-workspace-serial.log | ok | ok |

即两轮并行的用例结果**互不相同**：`fetch_io_profile_after_fix` 第 1 轮 FAILED、第 2 轮 ok；两轮唯一稳定复现的现象是同一挂起用例。该用例本轮未做根因修复。

## 未解决：并行测试挂起（保留为未解决项，非本源码发布阻塞）

`cargo test --workspace --locked`（默认并行）**2/2 复现挂起**，两次都在 `scrollery_lib` harness 报出 `state::scan_run_tests::concurrent_lifecycle_writers_restore_active_after_both_finish has been running for over 60 seconds`，随后无输出（日志停在 123149B / 112499B），仅 cargo 主进程存活、无 CPU 进展；两次均已由我按 180s/300s 上限终止，未无限等待。同一整套加 `-- --test-threads=1` 则 100s 内全绿，挂起与 harness 线程数相关。

该用例默认线程单跑与显式串行单跑均通过（见上表），故不是纯串行缺陷；具体线程/时序机制未定位，**本轮不做根因结论、不做修复**。

`fetch_io_profile_after_fix` 的 FAILED 仅见于第 1 轮并行，隔离单跑与第 2 轮并行均 ok。**推测**（未取证定论、未做根因修复）：该用例注入 2ms/read_dir + 1ms/stat，单批热批实测 p50≈374ms，而 `THUMB_PROBE_TTL=1000ms`，且只对末批计数断言目录探测为 0；末批前的间隔被负载拖过 TTL 即可误报「热批重探目录」。此为计时边缘的推测解释，非已验证根因。

**裁定与影响（主会话已裁）**：`oss-gate` 改为显式串行测试。故该并行缺陷保留为未解决项，但**不构成本轮源码公开的阻塞**；报告也不断言托管 Linux 上必然挂住——本机 Windows 并行复现不足以推出 Linux 结论，以门禁按串行形态实跑为准。

## 静态门禁（宿主树，非构建面）

通过：`sync-version --check`、`check-exotic-protocol-sync`、`check-theme-contrast`、`generate-theme-palette --check`、`check-path-hygiene`。

`generate-notice.mjs --check` 在我执行时为 rc=1（`NOTICE.md` 与 lockfile 不同步，日志 40-gate-notice.log）。此结果绑定**执行时刻的共享工作树**；该门与其 NOTICE/lockfile 归属同属 B 的第三方法律材料范围（`scripts/generate-notice.mjs`、`third-party/license-sources.json` 均在其改动面内），故**不**断言它绑定干净 HEAD，也**不**归属为 A 的问题；以 B 收尾后复跑为准。

`check-rename-gate.mjs` 在我执行时为 rc=1（`scripts/acceptance/isolated-app-smoke.mjs:613` 残留 `[PICASA]`，日志 46-gate-rename-gate.log）。据主会话通知，该注释已在 main 修正且 rename-gate 现已通过；该修复非我执行，我未复跑该门。

## 未做的验证

- 未构建正式 Tauri 安装包（本次授权为源码验证）。
- 未在最终公开投影上验证：`target/oss-public-checkout` 为空 clone，待 A 产出最终树后按通知执行，只核可构建性、不重复整套同源业务测试。
- 未跑本机 WSL 的 Linux 编译面（本机 WSL 无 docker）。

