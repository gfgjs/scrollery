---
status: snapshot
type: working-memory
line: 本地目录改名picasa-next至scrollery
created: 2026-07-15
---

# 进度日志:本地目录改名 picasa-next → scrollery

## 会话:2026-07-15
- 做了:
  - 前置任务(同会话,未单独建三件套):全仓扫描确认 `D:\photoapp\picasa-next` 完整路径无硬编码依赖(为盘符搬迁问题作答)。
  - 本任务:全仓扫描确认「picasa-next」文件夹名本身无生效依赖——逐一核查 Cargo.toml(src-tauri + 根 workspace)、package.json、tauri.conf.json、`.git/config`(本地全量 `git config --local --list`)、改名门禁 `check-rename-gate.mjs`/`rename-gate.json`、CI 自托管 runner 运维手册。
  - `tasklist` 检查本机进程占用:确认无 cargo/node/rustc 运行,发现 2 个 Code.exe 在跑。
  - 建立本三件套,固化调研结论与阶段 1-4 清单,供用户关闭会话、完成改名后在新会话接续。
  - 用户追加明确目的地为 `D:\workspace\scrollery`(非同级改名,是换父目录+改名);核实 `D:\workspace` 已存在且为空(2026-07-15 用户预先建好),据此把三件套里所有 `D:\photoapp\scrollery`/`d--photoapp-scrollery` 的假设路径与命令(`Rename-Item`)全部改写为 `D:\workspace\scrollery`/`d--workspace-scrollery`/`Move-Item`。
- 验证:
  - `git config --local --get-all safe.directory` / `--global` 均无输出(exit 1)——确认未持久设置。
  - `git config --local --list` 全量核对——仅 SSH remote URL 与分支跟踪关系,无路径相关配置。
  - 读取 `src-tauri/Cargo.toml`(name="scrollery")、根 `Cargo.toml`(members 全相对路径)、`package.json`(name="scrollery")、`src-tauri/tauri.conf.json`(productName/identifier 均为 Scrollery 品牌)确认现状,不采信旧文档里过期的 `name="picasa-next"` 记录。
  - 读取 `scripts/check-rename-gate.mjs` 全文 + `scripts/rename-gate.json` 全文,确认门禁扫描逻辑与物理文件夹名正交。
  - `ls /d/workspace` 确认已存在且为空;`ls /d/` 核对顶层目录布局(含 `photoapp`/`actions-runner`/`work`/`work.old` 等既有目录),无命名冲突。
- 遗留:
  - 阶段 1(用户在会话外关闭 VS Code + 改名)尚未执行,是下一步、也是会话边界所在。
  - 阶段 2-4(改名后验证 + 两项收尾)全部等阶段 1 完成后,在新路径的新会话里接续——新会话开始时应先读本目录三件套对齐现状,而不是凭对话记忆(改名后 Claude Code project key 本身也会换,旧会话记忆不会自动带过去,这正是本任务要固化到文件而非仅存对话里的原因)。

## 会话:2026-07-15(续,新 project key `d--workspace-scrollery` 下的会话)
- 做了:用户完成阶段 1(关闭旧会话、`Move-Item` 到 `D:\workspace\scrollery`、开新会话前复制 Claude Code 项目数据),回来要求"帮我跑一遍检查"——执行阶段 2 全部验证项 + 阶段 4 全部验证项。
- 验证(全部通过,证据见 task_plan.md 阶段 2/4 已回填的具体输出):
  - `pwd` 实测确认 cwd 确实是 `/d/workspace/scrollery`(不只是信环境提示)。
  - `git status`/`git remote -v`/`git log -1` 全干净,历史(`85fdb66`)与远程连接完好。
  - `cargo check`:`Finished dev profile ... in 37.65s`,通过;耗时属预期内的一次性增量缓存重建。
  - `npm run typecheck`:先跑了一遍带 `| tail` 管道的版本(输出为空看似过了),但按规矩不能信管道后的空输出,又不带管道单独重跑一次确认 `EXITCODE=0`。
  - `node scripts/check-rename-gate.mjs --force-active`:`✓ 改名门禁通过`。
  - `ls`/`du` 核对 `C:\Users\gf\.claude\projects\d--workspace-scrollery\`:31 条记忆文件 + MEMORY.md 齐全、77 个会话 jsonl 齐全、730MB vs 旧目录 729MB 基本一致;旧目录 `d--photoapp-picasa-next` 仍完整保留未被误删。
- 遗留:仅剩阶段 3(更新 `docs/runbooks/2026-07-06-开发者项目手册.md` 里过时的"目录名是旧代号,不必改"一行)未做;阶段 4 的旧 Claude 数据副本清理是开放项,用户自行决定。

## 会话:2026-07-15(续二,阶段 3 落地)
- 做了:用户在另一任务中要求"核实文档里的 picasa-next 可纠正项并修改",阶段 3 随该批一并落地。
  - 阶段 3 主项:开发者项目手册本地路径行改为 `D:\workspace\scrollery`,并把 F-001 结论(git/Cargo/npm/Tauri 均不依赖目录名)就地固化进该行,而非只改路径字面。
  - 阶段 3 查漏项:`git grep -n "目录名是旧代号"` 实跑——业务文档零命中,余 4 处全在本三件套内自指该行,确认无第二处遗漏。
  - 该批同时纠正了本三件套调研范围**未覆盖**的一类漂移:findings §一只按 `photoapp`/绝对路径两轮扫描,没审 runbooks 正文里的 **crate 名 / 包名 / 仓名**。实查出 5 处会导致照抄即失败的活手册错误(`crates/picasa-next-exotic-trust`→`scrollery-exotic-trust`、`crates/picasa-next-pro`→`scrollery-pro`、`cargo test -p picasa-next`→`-p scrollery`、`gfgjs/picasa-next-registry`→`scrollery-registry`、安装包名 `Picasa Next_0.1.0`→`Scrollery_0.1.0`),均已改。
- 验证:`node scripts/check-rename-gate.mjs --force-active` 复跑 `✓ selftest 通过(6 词例 + 路径豁免)` + `✓ 改名门禁通过`;`git grep "目录名是旧代号"` 业务文档零命中。
- 遗留:阶段 4 的旧 Claude 数据副本(`d--photoapp-picasa-next`,729MB)清理仍是开放项,用户自行决定;本任务主体可收口。

## 回顾(收口时填)
- 亮点:阶段 0 先做"内部自证点核查"(git remote URL / Cargo.toml name / package.json name / tauri identifier)而非搜文件夹名字符串,一次性排除了改名风险,没有走一轮又一轮的"再确认"；三件套跨会话(甚至跨 Claude Code project key)接续验证了设计初衷——新会话开局直接读文件对齐进度,没有凭对话记忆重新调研。
- 教训:三件套自身调研范围有边界——findings §一/§二只扫了路径字面量和文件夹名依赖,没审 runbooks 正文里的 crate 名/包名/仓名这类"文字提及但非文件夹名"的关联漂移,导致 5 处会致命令失败的错误(`cargo test -p picasa-next` 等)留到另一个任务(picasa-next 文档纠正批)才被发现。以后类似"改名收尾清单"任务应把"仓库内所有提及旧名字符串的活文档"也纳入阶段 0 扫描面,不能只扫"文件夹名依赖"。
- 意外:Claude Code 项目数据的路径编码键(`d--workspace-scrollery`)在用户把目的地从 `D:\photoapp\scrollery` 改口为 `D:\workspace\scrollery` 后完全变了(父目录都换了),若沿用早先草稿假设的 `d--photoapp-scrollery` 去迁移会迁到错误目录、旧会话历史找不回——推导编码键必须锚定用户最终拍板的确切路径,不能用中间假设定案。
