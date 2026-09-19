---
status: snapshot
type: working-memory
line: 本地目录改名picasa-next至scrollery
created: 2026-07-15
---

# 任务计划:本地目录改名 picasa-next → scrollery

## 目标
把本地开发目录从 `D:\photoapp\picasa-next` 迁移改名为 `D:\workspace\scrollery`(2026-07-15 用户明确目的地,从"同级改名"升级为"换父目录+改名"),补齐 2026-07-06 品牌改名(R2-7,产品名定为 Scrollery)当时特意保留、如今用户决定收掉的目录名尾巴。改名本身在用户侧、本会话外执行(需先关闭本会话所在的 VS Code);本三件套负责跨会话固化调研结论与收尾清单,保证新会话(新路径、新 Claude Code project key)接续时不必重新调研一遍。

## 当前阶段
阶段 0-2、4 已完成(2026-07-15 用户完成迁移,新会话在 `D:\workspace\scrollery` 内跑完验证)。阶段 3(更新过时文档行)是唯一剩余收尾项。

## 阶段

### 阶段 0:可行性调研与安全性确认
- [x] 确认 `D:\photoapp\picasa-next` 全字符串未被任何代码/配置硬编码依赖(上一任务,同会话完成,未单独建三件套)
- [x] 确认「picasa-next」文件夹名本身未被任何生效机制依赖(Cargo/npm/tauri/git/CI runner/改名门禁逐一查证)
- [x] 确认当前无 cargo/node/rustc 进程占用文件夹;发现 2 个 Code.exe 在跑(需用户手动关闭)
- **状态:** done

### 阶段 1:用户执行改名(会话边界——需关闭当前 VS Code 后在会话外操作)
- [x] 关闭本项目在 VS Code 中打开的所有窗口(含本会话宿主窗口)、关闭所有 `cd` 进 `D:\photoapp\picasa-next` 的终端/PowerShell 窗口
- [x] 复查一次没有 cargo/node/rustc 等进程占用该目录
- [x] `Move-Item -Path D:\photoapp\picasa-next -Destination D:\workspace\scrollery`(同盘符,瞬时完成)
- [x] 开新会话前把 Claude Code 项目数据目录复制到 `d--workspace-scrollery`
- [x] 改名后在 `D:\workspace\scrollery` 重新打开 VS Code / 新的 Claude Code 会话
- **状态:** done(2026-07-15 用户完成)

#### Claude Code 项目数据迁移(2026-07-15 已实测确认,非推断)
- 编码规则用 5 个同机项目目录交叉验证过(`d--photoapp-mom-story`、`d--photoapp-picasa-next`、`d--photoapp-worklog-kit`、`d--photoapp-worklog-kit-smoketest`、`d--pxphoto`):drive 字母转小写,`:` 与每个 `\` 各自替换成一个 `-`。目的地改为 `D:\workspace\scrollery` 后,`d:\workspace\scrollery` ⇒ **`d--workspace-scrollery`**(父目录从 photoapp 换成 workspace,编码结果跟着变,不是 `d--photoapp-scrollery` 了)。
- 项目数据目录不止 `memory/`:还有 77 个 `<uuid>.jsonl` 会话记录文件(+ 同名空目录)= 实测 **729MB**,这才是"聊天记忆"字面意义上的主体;`memory/` 只是其中的蒸馏索引子目录。两者同在一个目录下,建议整目录一起搬,不要只搬 `memory/`。
- 推荐用**复制不用移动**,搬完先在新会话验证一遍,确认没问题再自行决定要不要清掉旧目录(不建议我代劳删除,数据不可再生):
  ```powershell
  Copy-Item -Path "C:\Users\gf\.claude\projects\d--photoapp-picasa-next" `
            -Destination "C:\Users\gf\.claude\projects\d--workspace-scrollery" `
            -Recurse
  ```
- 验证:新会话里检查 `MEMORY.md` 索引是否正常被读取(比如提一句"回忆一下这个项目"应该能带出记忆内容)、Claude Code 历史会话列表里能否看到改名前的旧对话。

### 阶段 2:改名后验证(新会话执行)
- [x] `git status` 干净(仅剩无关的 2026-07-13-WAL 任务未跟踪目录,与本任务无关);`ahead of origin/dev by 50 commits`(未 push,符合预期)
- [x] `git remote -v` = `git@github.com:gfgjs/scrollery-private.git`(SSH,不变);`git log -1` = `85fdb66`(与移动前一致)
- [x] `cd src-tauri && cargo check` —— `Finished dev profile ... in 37.65s`,通过;耗时是增量缓存路径指纹失效触发的一次性部分重编译(预期内的良性开销,非报错)
- [x] `npm run typecheck` —— 无输出、`EXITCODE=0`(单独验证过真实退出码,不是被管道吞掉的假阴性)
- [x] `node scripts/check-rename-gate.mjs --force-active` —— `✓ 改名门禁通过:tracked 文件零「picasa」残留`
- **状态:** done(2026-07-15 新会话验证全绿)

### 阶段 3:收尾 A——更新过时文档行
- [x] `docs/runbooks/2026-07-06-开发者项目手册.md` 里 "`d:\photoapp\picasa-next`(目录名是旧代号,不必改)" 这一行,现状已反转(目录名已改),改写为准确表述(改记为新路径 `d:\workspace\scrollery`,并补一句改名时间/原因,而不是留着旧结论)——2026-07-15 已改为"`D:\workspace\scrollery`(2026-07-15 由 `d:\photoapp\picasa-next` 搬迁改名;git/Cargo/npm/Tauri 均不依赖目录名)",末句即 F-001 结论的就地固化
- [x] 顺手 `git grep -n "目录名是旧代号"` 确认没有第二处同样表述遗漏 —— 实跑:业务文档零命中,余 4 处全在本三件套内(findings:44 / progress:36 / task_plan:52-53)自指该行,非遗漏
- **状态:** done(2026-07-15;随同一会话的 picasa-next 文档纠正批一并落地,详见该批对 runbooks 的其余修正)

### 阶段 4:收尾 B——验证 Claude Code 项目数据迁移
- [x] `ls C:\Users\gf\.claude\projects\` 确认 `d--workspace-scrollery` 已存在(编码名与预期一致,未出现拼写/编码分歧的第二个目录)
- [x] `memory/` 子目录核对:`MEMORY.md` + 31 条记忆文件齐全,与旧目录一一对应
- [x] 77 个 `<uuid>.jsonl` 会话记录文件齐全;新目录 730MB vs 旧目录 729MB(基本一致)
- [ ] 清理 `C:\Users\gf\.claude\projects\d--photoapp-picasa-next\` 这份 729MB 旧副本——留给用户自行决定何时清理,不阻塞本任务收口
- **状态:** done(核心验证全部通过;旧副本清理是开放项,不算阻塞)

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 批准执行本地目录改名 picasa-next→scrollery | 阶段 0 逐项核实:Cargo/package.json/tauri.conf.json 均已用 scrollery 品牌名、git 本地配置无路径依赖、改名门禁只扫内容不看文件夹名、CI 自托管 runner 独立于本目录——无阻碍,是 2026-07-06 品牌改名的收尾(蒸馏候选见 findings.md F-001) | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| （尚无) | | |
