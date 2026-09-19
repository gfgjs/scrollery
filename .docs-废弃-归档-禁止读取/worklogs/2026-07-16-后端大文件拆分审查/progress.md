---
status: 快照
type: working-memory
line: 后端大文件拆分审查
created: 2026-07-16
---

# 进度日志:后端大文件拆分审查

## 会话:2026-07-16
- 做了:
  - 阶段 1 普查:`git ls-files "*.rs" | xargs wc -l` 排序 + `git log --numstat --since=2026-07-01` 逐文件 churn;定阈值(≥1000 深测绘 / 900–1000 轻裁决)。
  - 阶段 2 T 线时效性核查:queries.rs 当前口径实测(11,228 行/214 pub fn/138 测/41 消费文件);`git diff f3498dd..HEAD` 符号级差异=3 个新增声明;确认 S-P3/P4 代码已 landed(todo.md S 节);裁定新符号归属(双归 layout.rs)+ 发现 search.rs 叶子前提被推翻 → T-D-012。
  - 阶段 3 测绘:4 个并行只读 Explore agent 覆盖 15 文件(exotic / ai / ipc+scanner / layout+core 四组),逐文件段落图、计数、职责、耦合、天然缝、外部面全量入 findings.md。
  - 阶段 4 落盘:设计件 `docs/designs/2026-07-16-后端大文件拆分审查与方案.md`(三档裁决 16 文件 + U-P1~P4 四批拆分方案 + 观察触发条件 + 豁免理由 + U-D-001~006 裁决表待 Review);U 线登记 docs/README.md 字母表(下一空闲字母 U→V)+ todo.md 新增 U 节。
  - 阶段 5 T 线回写:T 设计状态头追加三次核实、§4.3 增补两符号行、§11 增补 D-012 待裁行;T 三件套 findings 追加三次核实小节;todo.md T 节头注与 P0 行同步更新。
- 验证:
  - 全部数字来自实测命令(wc/git diff/git log/grep -c),快照 commit=13a3192;agent 测绘结果与我方抽测(新符号行号 400/1439/11143、消费文件 41)相互印证。
  - Edit 全角标点陷阱两次命中(T 设计 `；`、T findings `：，`),按 T 错误账既有解法 node JSON.stringify 导真实字节后重试成功。
  - docs 两门(check_docs / check_docs_index)见下一条会话记录。
- 遗留:U-D-001~006 与 T-D-012 待用户 Review;后续代码审查把顺序修订为 P1-a > P2 > P3 > P4 > 可选 P1-b(见下条会话记录);本任务文档产出后收口。

## 会话:2026-07-16(代码审查接续)
- 做了:
  - 重读 planning skill、docs 治理/收口约定与 U 三件套,以磁盘状态接续阶段 6。
  - 结合 J/M/P/N 审查资产、T 线拆分纪律及当前 Rust 源码,复核 U-D-001~006 与 T-D-012。
  - 亲核锁定依赖 Tauri 2.11.4 + tauri-macros 2.6.3 的 `invoke_handler`/`generate_handler!` 类型,确认 U-P1-a `handler() -> impl Fn` 主路径可行。
  - 发现并修正两处结构性方案问题:U-P1-b 改完整 `bootstrap::setup` owner;U-P2 下载编排改留 `ipc/model_download.rs`,避免 Tauri Channel 下沉通用 download。
  - 落审查快照 `docs/reviews/2026-07-16-后端大文件拆分方案代码审查.md`;回写 U 权威设计、todo 与三件套;豁免改触发式重审,U 内顺序改 P1-a > P2 > P3 > P4 > 可选 P1-b。
- 验证:`git diff --check` 通过;`node tools/check_docs.mjs` 通过(170 文档/480 代码配置文件);`node tools/check_docs_index.mjs` 通过(目录表/字母表/worklogs 三组不变量)。
- 遗留:U-D-001~006 与 T-D-012 仍待用户裁决;本轮未改 Rust。用户批准后另起施工会话,按开工 HEAD 重验。

## 会话:2026-07-16(建议采纳与裁决回写)

- 做了:用户采纳代码审查后的 U-D-001～006 修订版,并对 T-D-012 采纳方案 a;将 U 设计转为现行施工契约,同步回写 U/T 设计、todo、审查快照与两线三件套。
- 做了:把后续施工队列固化为 U-P1-a > U-P2 > U-P3 > U-P4 > 重测后可选 U-P1-b;T-P1 保留 search.rs,T-P4 迁 layout owner 时顺改一处 helper 路径。
- 边界:本轮只修改 docs;未修改 Rust、前端或配置。按用户明确指示,U 三件套继续留在 `docs/planning/`,不生成 closeout、不迁 worklog。
- 遗留:U/T 代码施工均另起会话,按开工 HEAD 重建相应基线与 manifest。

## 会话:2026-07-16(二次复核——受托核实并行会话的代码审查)

- 做了:对审查快照与裁决回写后的 U/T 文档做全量事实核验,逐条对源码/依赖源码取证:
  - `download_assets` 确收 `tauri::ipc::Channel<DownloadProgress>`(ai_commands.rs:871/982);`download/mod.rs` 模块头契约确为「机制下沉、领域编排留调用方」——P2 改 `ipc/model_download.rs` 的修正**成立**。
  - `arch_for_image_file`/`variant_fixed_batch` 全仓仅 ai_commands.rs 自用(rg 零外部命中)——原地保留**成立**。
  - Cargo.lock 实测 tauri 2.11.4 + tauri-macros 2.6.3;`Builder::invoke_handler` 签名亲核(tauri-2.11.4/src/app.rs:1658=`Fn(Invoke<R>) -> bool + Send + Sync + 'static`);`generate_handler!` 展开亲核(tauri-macros-2.6.3/src/command/handler.rs:174=`quote!(move |#invoke| {`)——P1-a `handler() -> impl Fn` 主路径**成立**。
  - `PICASA_SMOKE_TEST` 启动门确存(lib.rs:1031-1034,置环境变量则就绪即退 0)。
  - M 线 §5 测试盲区第 1 条确为 thumbnail_commands「后端第一命门」,C1(token 清理)/C2(线程 churn)逐条在案;P 线 A15(下载同步 fs)原文核对一致——P4 复用 M 线债、A15 独立行为债的引用**准确**。
  - 跨文档一致性:T 设计头/§4.3/§6(方案 a 施工注)/§11(D-012 ✅)、T 三件套三文件、todo T/U 节、U 设计 §3.2/§10 全部同拍,无残留「待裁」字样冲突。
- 复核新发现(仅 1 处,已修):P2 依赖方向门原样式 `rg 'ipc::ai_commands::'` 会漏别名导入(`use ... as`)与花括号分组导入(`ipc::{ai_commands, ...}`),放宽为 `rg 'ai_commands' src-tauri/src/ai` 归零并注明理由(改现行 U 设计;审查快照按冻结纪律不动)。
- 验证:上述每条均有命令级证据;docs 两门重跑见 commit;本轮零 Rust 改动。
- 结论:并行会话审查的全部事实声称**核验通过**,修订后的 U-D-001~006 / T-D-012 方案 a 无新异议;施工可按 P1-a > P2 > P3 > P4 > 可选 P1-b 另起会话执行。

## 会话:2026-07-16(阶段 7 施工——获批四批全落地)

- 做了:开工 HEAD `1a7a34a`(较快照 13a3192 漂移:lib.rs 1,062→1,013 行,Tauri 2.11.4/tauri-macros 2.6.3 未漂,重验通过),按 P1-a > P2 > P3 > P4 顺序施工,6 提交:
  - **U-P1-a**(96162c6):232 行 generate_handler 宏清单(186 命令)逐行迁 `ipc/registry.rs::handler()`;结构门=规范化抽取命令路径集合对拍 186=186。方案偏差一处:`handler<R>()` 泛型形态被 11 个收具体 `AppHandle` 的命令签名否决(E0277),收敛为具体 `Wry`(语义等价,见错误账)。
  - **U-P2-a**(d169f03):7 个跨层 helper 迁 `ai/runtime_config.rs`,ai 四模块 + face/exotic 命令一步改直接 import,无过渡 re-export;门=`rg 'ai_commands' src-tauri/src/ai` 归零(含注释,连 runtime_config 自身出处注释都改写避裸词)。
  - **U-P2-b**(e3fc686):DownloadProgress/download_assets/is_safe_model_file_name/dest_safety_tests 迁 `ipc/model_download.rs`;门=`rg 'tauri::ipc::Channel' src-tauri/src/download` 归零;serde 形状不变;两处过时路径注释(download/mod.rs 头注、face_commands)随迁更新。
  - **U-P2 A15**(63a3fb3):download_assets 六处 `.part` 同步 fs 改 `tokio::fs`,独立行为债提交,不混结构 diff。
  - **U-P3**(fccc9f0):outcome.rs(4 outcome 类型)/validate.rs(4 校验器+default_thumbnail_limits+11 测试)/worker_traits.rs(5 trait/struct+3 impl)拆出;worker.rs 710 行、pipeline.rs 1,129 行,两文件 `pub use` 保旧路径消费方零迁移;两状态机零行改动;门=`--list` 末级名对拍 27=27 + worker_e2e 编译绿。
  - **U-P4-a**(b757203):QoS 块 ~110 行迁 `thumbnail/qos.rs`;预算数学拆纯函数 `thumb_cpu_budget_for(logical)` 补边界测试(0-4/8/9/16/28 核逐值钉死折点);rayon start_handler 一处收敛 `apply_current_thread_qos()`;lib.rs 窗口事件改调新路径。P4-b 去重维持不施工归 M 线。
- 验证:每批 cargo fmt + check + clippy(-D warnings) + test --workspace/--lib `--locked` 全绿(最终主 lib 608/0/5,含 +1 QoS 边界测试);U 设计 §8 各专项结构门逐一核销(集合对拍/依赖方向门/测试名对拍);全部**本地非 CI**。release 冒烟门四批统跑一次:`npx tauri build --no-bundle`(2m52s)+ `PICASA_SMOKE_TEST=1` → Ready 977ms 即退 0,冒烟通过;退出 WAL checkpoint/后台任务停止日志正常。
- 体量重测(P1-a 后):lib.rs 832(setup ~585 仍在)/ai_commands 847(设计预估 ~700–750,偏差 +~100 来自估算,非漏迁)/worker 710/pipeline 1,129/thumbnail_commands 1,016;新模块 registry 252/model_download 267/runtime_config 129/outcome 74/validate 677/worker_traits 127/qos 145。
- 裁决建议:**U-P1-b 维持可选、不自动开工**——最高频冲突面(命令注册)已消灭,lib.rs 余下 churn 源=setup 生命周期(低频);建议数周后重测 churn 再裁。
- 遗留:真机 smoke(App 起/托盘/AI 面/缩略图面,GUI 不自动化);29+ 本地提交仍待用户批准 push。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
