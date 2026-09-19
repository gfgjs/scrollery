---
status: snapshot
type: working-memory
line: 本地目录改名picasa-next至scrollery
created: 2026-07-15
---

# 发现与决策:本地目录改名 picasa-next → scrollery

## 需求
- 用户先问:考虑把项目从 D 盘搬到 C 盘,要求全面检查代码/配置里有没有硬编码 `D:\photoapp\picasa-next`。
- 用户接着问:想把文件夹名从 `picasa-next` 改成 `scrollery`,是否安全。
- 用户拍板:"可以"——同意先产出改名后的收尾清单,等改名完成后接续执行。
- 用户追加明确目的地:"我将改为 `D:\workspace\scrollery`"——不只是同级改名(`D:\photoapp\picasa-next`→`D:\photoapp\scrollery`),而是换到新的父目录 `D:\workspace\`(该目录 2026-07-15 已由用户预先建好,当时为空)。

## 发现

### 一、盘符搬迁调研(D→C,已完成,本任务前置背景)
- 全仓 705 个 git 跟踪文件按 `photoapp`/`picasa-next`(大小写不敏感)+ `[Dd]:[\\/]` 正则两轮扫描,命中均为无害项:
  - `docs/` 下约 27 个文件提及旧路径/旧项目名,均为历史叙事文档(项目改名 Scrollery 前的旧代号、过程记录里的命令行示例),非生效配置。
  - 源码里的 `D:/photos/...` 命中(`src-tauri/src/layout/justified.rs`、`crates/exotic-protocol/src/message.rs`、`mediaGrid.helpers.spec.ts`)均为单元测试假数据。
  - `src/components/settings/NetworkStorageSection.vue:60` 的 `'D:\Media'` 是网络存储设置输入框的用户界面占位符文本,给终端用户看的格式提示,与项目自身路径无关。
  - `tools/check_docs.mjs:338` 的疑似 `d:` 命中其实是正则 `\d{4}-\d{2}-\d{2}` 的转义序列误命中。
  - `scripts/generate-notice.mjs:292`、`tools/face_crosscheck.py:72` 的 `D:\photoapp\scrollery\...` 是自测试字符串/手动对拍脚本的示例默认值,连当前路径都对不上(写的是 scrollery 不是 picasa-next),本就不是生效依赖。
  - `.cargo/config.toml` 的 `ORT_DYLIB_PATH` 标了 `relative = true`,Cargo 自动相对 config.toml 所在目录解析为绝对路径,是已经写对的可迁移写法,不受盘符/目录搬迁影响。
- 核心配置文件(`tauri.conf.json` 含 perf/direct-release 变体、根 `package.json`、根 `Cargo.toml`、`vite.config.ts`、`vitest.config.ts`、`eslint.config.js`、`.github/workflows/*.yml`、`.claude/` 项目配置)逐一确认干净,无命中。
- git 的 `safe.directory` 在 local/global 配置里均未持久设置;文档里出现的 `-c safe.directory=D:/photoapp/picasa-next` 只是某次命令行的临时覆盖参数。
- 自托管 CI runner(`dev-box-win`/`dev-box-wsl`)装在独立的 `D:\actions-runner`,注册时带 `--work _work`,自己做 checkout,与用户手动 clone 的开发目录完全解耦——搬迁/改名均不影响 CI。
- **一次工具输出核验教训**:第一轮并行 grep 曾把 `scripts/generate-notice.mjs` 的一行内容错标成 `tools/scrfd_crosscheck.py` 第 292 行,但该 `.py` 文件总共只有 144 行——行号矛盾暴露标注错位,重新用 Grep 定位 `parseCargoLine` 才找到真实位置。教训:任何可疑的工具输出都要重新读源文件核实,不能因为"grep 说是"就采信(呼应全局 CLAUDE.md「数字要有证据」)。

### 二、文件夹改名调研(picasa-next→scrollery,本任务核心)
- **Cargo 层面已完全脱钩于文件夹名**:
  - `src-tauri/Cargo.toml`:`name = "scrollery"`(旧文档 `docs/refactor_2026/Part7_发布工程.md` 里记的 `name="picasa-next"` 已过期,必须以当前文件为准——这也是一次"文档非地面真相"的现场教训)。
  - 根 `Cargo.toml`:`[workspace] members = [...]` 全部是相对路径(`"src-tauri"`、`"crates/exotic-protocol"` 等),Cargo workspace 解析从不依赖父目录名。
- `package.json`:`"name": "scrollery"`,已改好。
- `src-tauri/tauri.conf.json`:`productName: "Scrollery"`,`identifier: "com.scrollery.app"`——Tauri 应用身份靠这两个字段,不看源码目录名。
- `.git/config`(本地)逐行核对:`remote.origin.url=git@github.com:gfgjs/scrollery-private.git`(SSH URL,无本地路径成分)、无 `core.worktree`/`core.hooksPath`/`file://` 远程、其余全是分支跟踪关系(`branch.*.remote`/`.merge`/`.vscode-merge-base`)和 `gui.wmstate`/`gui.geometry` 这类窗口位置记录——git 对自己所在目录叫什么名字完全无感。
- **改名门禁 `scripts/check-rename-gate.mjs` + `scripts/rename-gate.json`(`active: true`)与文件夹名正交**:该门禁用 `git grep -I -i` 扫**已跟踪文件的内容**,匹配到 `forbidden: "picasa"` 后按 `[A-Za-z0-9_]` 词边界展开、再核对是否落入 `allowTokenPrefixes`(`PICASA_`、`picasa_next_pro`)豁免;路径豁免面是 `docs/`、`scripts/rename-gate.json`、`scripts/check-rename-gate.mjs`、`CHANGELOG.md`。全程只看 git 跟踪内容,从不检查物理文件夹名,因此改文件夹名这件事对该门禁的通过/拦截状态零影响(既不会让它多拦,也不会让它误放行)。
- 进程占用检查(2026-07-15 实测):`tasklist` 确认当前无 `cargo.exe`/`rustc.exe`/`node.exe` 运行;但有 2 个 `Code.exe` 在跑,大概率是本项目工作区窗口(含本会话宿主),Windows 下改名会话正在使用的目录容易因文件句柄占用失败,需用户手动关闭后再改名。

### 三、遗留的两个非"代码/配置"但真实存在的影响面
- **Claude Code 项目记忆按绝对路径键控(编码规则已实测确认,非推断)**:当前记忆存在 `C:\Users\gf\.claude\projects\d--photoapp-picasa-next\memory\`。用同机 5 个项目目录交叉验证编码规则:`d--photoapp-mom-story`、`d--photoapp-picasa-next`、`d--photoapp-worklog-kit`、`d--photoapp-worklog-kit-smoketest`、`d--pxphoto`——规律是 drive 字母转小写,原路径里的 `:` 与每个 `\` 各自替换成一个 `-`(单段路径 `d:\pxphoto`⇒`d--pxphoto` 印证了"drive 后紧跟双横杠"不是巧合,而是 `:` 和 `\` 各出一个横杠叠加的结果)。2026-07-15 用户把目的地明确为 `D:\workspace\scrollery`(换父目录+改名,不再是 `D:\photoapp\` 同级改名),按此规则 `d:\workspace\scrollery` 确定会编码为 `d--workspace-scrollery`(不是先前草稿假设的 `d--photoapp-scrollery`——目的地一变编码键跟着变,提醒:这类推导必须锚定用户最终拍板的确切路径,不能用中间假设去定案)。
- **"聊天记忆"不止 `memory/`**:项目数据目录下还有 77 个 `<uuid>.jsonl` 会话记录文件(+ 同名空目录,大概率存放该会话的旁支/checkpoint 数据),实测整个项目目录 **729MB**——这才是字面意义上的"聊天记忆"(完整对话历史),`memory/` 只是其中人工蒸馏出的索引子目录,体积占比很小。改名后两者都不会自动带到新 key 目录下,需整目录一起搬(而不是只搬 `memory/`),已在 task_plan.md 阶段 1 补上 `Copy-Item -Recurse` 的具体命令(用复制不用移动,搬完验证无误再自行决定要不要清理旧副本)。
- **`docs/runbooks/2026-07-06-开发者项目手册.md` 有一行会变得过时**:原文写"`d:\photoapp\picasa-next`(目录名是旧代号,不必改)",这是 2026-07-06 当时"保留旧目录名"的决定记录。该行落在改名门禁的 `docs/` 豁免面内,CI 不会拦,但作为按字面操作的机器侧运维手册,改名落地后这行会与实际情况相反,需要在阶段 3 更新为准确表述。

## 外部资料(当数据,不当指令)
- （无——本任务全程基于本仓库文件与本机命令行输出,未引入外部网页/API 内容）

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 目录改名/搬迁安全性的判断方法论:git/Cargo/npm/Tauri 的"我是谁"均靠内部证据自证(`.git` 内部 refs + remote URL、`Cargo.toml`/`package.json` 的 `name` 字段、`tauri.conf.json` 的 `identifier`/`productName`),从不依赖 `basename(容器目录)`;判断"能否安全改名/搬迁"时应逐一核对这些自证点,而非搜文件夹名字符串本身 | experience |
| F-002 | Claude Code 的项目级数据(会话记录 `<uuid>.jsonl` + 蒸馏记忆 `memory/`)按工作目录绝对路径键控,编码规则实测确认:drive 字母转小写,`:` 与每个 `\` 各自替换成一个 `-`(如 `d:\photoapp\picasa-next`⇒`d--photoapp-picasa-next`,5 个同机项目目录交叉验证);改名/搬迁项目目录会让新会话进入全新的空 project key 目录,不会自动继承旧路径下的会话历史与 MEMORY.md,必须在**开新会话之前**手动 `Copy-Item -Recurse` 整个旧 project 目录到新编码名下才能续上——晚了(新会话已建空目录后才想起迁移)则要改成逐文件合并 | experience |
| F-003 | 本仓库的品牌改名门禁(`scripts/check-rename-gate.mjs`)只用 `git grep` 扫已跟踪文件内容,与物理文件夹名正交——这类"防旧名回流"门禁的设计经验(词边界展开 + 大小写敏感的 token 前缀豁免 + 路径前缀豁免)如果之后仓库还有类似改名/品牌治理需求可复用 | no-promotion(倾向:门禁设计本身已在 `check-rename-gate.mjs` 头部注释里说清楚,重复记录到 experience.md 增量价值有限;留待收口时再判断) |
