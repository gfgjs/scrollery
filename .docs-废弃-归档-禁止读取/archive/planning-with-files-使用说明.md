---
status: archived
type: runbook
line: 工作流工具
created: 2026-07-11
last-verified: 2026-07-11
---
> 📦 **已归档(2026-07-11,建档当日):** 本项目长任务工作记忆已改用**自研轻量 skill `/planning`**(`.claude/skills/planning/SKILL.md`;施工期 `docs/planning/`,收口迁 `docs/worklogs/`,治理接线见 docs/README 维护规则)。planning-with-files 插件在本项目弃用——弃用理由:legacy hook 逐工具调用注入的 token 税、CJK 任务名被 slugify 退化、全套协议(attestation/Stop 门/仪式化打卡)对在环工作方式过重。本文正文冻结,仅作该插件机制的参考资料。

# planning-with-files 使用说明

> 适用范围:planning-with-files 是一个 Claude Code Skill(Manus 式「文件即工作记忆」),用磁盘上三个固定 markdown 文件承载长任务的计划/发现/进度,使其不随上下文丢失 / `/compact` / `/clear` 而消失。本手册记录它的机制、命令、两条正交的模式轴,以及本项目(Scrollery)围绕它的 `.gitignore` 与 `worklogs/` 归档约定。
>
> Skill 安装目录:`${CLAUDE_PLUGIN_ROOT}`(本机 = `C:/Users/gf/.claude/plugins/marketplaces/planning-with-files/`)。

## 0. 一句话

把「任务计划、发现、进度」写进磁盘上三个固定文件,让 5 步以上的多步任务状态可持久、可恢复。简单问答不用它。

## 1. 心智模型

> 上下文窗口 = RAM(易失、有限);文件系统 = 磁盘(持久、无限)。重要的东西一律落盘。

Skill 装了一组 hook,会在合适时机把三件套自动读回上下文,并在关键节点提醒回写。所以它不只是「约定写三个文件」,而是一套带自动注入 + 抗失忆的机制。

## 2. 三个文件各管什么

| 文件 | 装什么 | 何时写 |
|---|---|---|
| `task_plan.md` | 阶段(phase)划分、每阶段状态、决策表、错误表 | 每完成一个阶段 |
| `findings.md` | 研究发现、证据、外部/不可信内容 | 有任何发现就追加 |
| `progress.md` | 会话日志、测试结果、错误账、五问自检 | 全程滚动追加 |

**安全铁律**:`task_plan.md` 每回合(legacy 下每次工具调用)被自动注入上下文,**别往它塞 web/外部不可信内容**——那些放 `findings.md`。

## 3. 两条正交的轴(核心)

两个**互相独立**的选择,别混:

- **位置轴——文件放哪**
  - `root`:三件套直接躺**仓根**(默认)。
  - `slug`:躺 `.planning/<日期>-<任务名>/`,每任务一独立目录(隔离、可并行)。
- **注入轴——hook 怎么注入 / 要不要防篡改**
  - `legacy`:每次工具调用灌 plan 前 30 行(有 token 税),不防篡改。
  - `autonomous`:砍掉逐调用注入(省税),换合成摘要 + 默认开防篡改(attestation)。
  - `gated`:在 autonomous 基础上,加一道「没干完所有 phase 就别停」的 Stop 门。

**两轴自由组合**,命令决定落在哪个格子:

| | **root(仓根)** | **slug(`.planning/<日期>-名/`)** |
|---|---|---|
| **legacy** | `init-session.sh` / `/plan` / `/pwf` / `/start` | `init-session.sh "名"` |
| **autonomous** | `init-session.sh --autonomous` | `init-session.sh --autonomous "名"` |
| **gated** | `init-session.sh --gated` | `init-session.sh --gated "名"` |

一句话:**给不给任务名 → 决定 root/slug;加不加 `--autonomous/--gated` → 决定注入轴。**

## 4. 怎么起一个规划

**斜杠命令(最省事,但默认落 root + legacy):**

- `/plan`、`/pwf`、`/start` 三者等价,建仓根三件套。`/pwf` 若话里带「autonomous / gated / 别停直到完成」会自动切对应注入模式,但**位置仍是 root**(斜杠命令都不切 slug)。
- `/status`:只读,显示当前在第几阶段。

**脚本(要 slug 或想看清产出,必走这条):**

```bash
sh "C:/Users/gf/.claude/plugins/marketplaces/planning-with-files/scripts/init-session.sh" "gallery-query-routing"
```

各变体产出对照:

| 命令 | 产出 | 落点 |
|---|---|---|
| `/plan` `/pwf` `/start` 或 `init-session.sh` | 仓根三件套 | root + legacy |
| `init-session.sh "名"` | `.planning/<日期>-名/` 三件套 + `.active_plan` 指针 | slug + legacy |
| `init-session.sh --autonomous "名"` | 上者 + `.mode` + nonce + 自动 attest | slug + autonomous |
| `init-session.sh --gated "名"` | 上者 + Stop 门 | slug + gated |

也可**直接用自然语言让 Claude 起**——「用 slug 模式起个规划,叫 gallery-query-routing」→ Claude 替你跑脚本;「再加 autonomous」→ 加 `--autonomous`。底层总要执行一次脚本,但用户端只需一句话。

## 5. 起好之后,会话里自动发生什么(hook)

| Hook | 何时触发 | 干什么 |
|---|---|---|
| UserPromptSubmit | 每条用户消息 | 注入 `task_plan` 前 50 行 + `progress` 末 20 行(autonomous 换成 ledger 摘要) |
| PreToolUse | 每次 Read/Edit/Bash/Grep/Glob/Write **前** | 注入 `task_plan` 前 30 行(**autonomous 整段丢弃**) |
| PostToolUse | 每次 Write/Edit **后** | 提醒「去更新 progress.md」 |
| Stop | 以为要结束时 | 检查是否所有 phase 完成(legacy=仅提示;gated=可拦回继续) |
| PreCompact | `/compact` 或上下文满前 | 提醒先把进度 flush 进 progress.md,以便压缩后重新读回(抗失忆核心) |

hook 挑哪个计划注入,按此顺序解析:① 终端 `export PLAN_ID=<id>` 钉的 → ② `.planning/.active_plan` → ③ `.planning/` 里最新目录 → ④ 仓根 `task_plan.md` → ⑤ 都没有 = **不注入、零开销**。

## 6. 并行任务

两条长任务同时进行时,各开一个终端,各自:

```bash
export PLAN_ID=2026-07-11-gallery-query-routing
```

把该终端钉死到某个 `.planning/` 计划,互不串台。这正是 slug 模式存在的理由(root 模式只有一套文件,并行会互相覆写)。

## 7. 中途 / `/clear` 后恢复

再次调用 skill(或 `/plan`),它先**读三件套**恢复状态,再跑 `session-catchup.py` 找出上次会话未同步的改动。也可随时 `/status` 看「现在在第几阶段」。

## 8. 收口:归档到本项目的 `worklogs/`

执行期三件套在 root 或 `.planning/`,**都被 `.gitignore` 忽略**(不入库、不进公开镜像、免暂存竞态)。任务**收官时**按「收口即处置」:

1. 把三件套复制 / `git mv` 进 `docs/worklogs/<日期>-<任务名>/`(此路径在 copybara 排除面内,安全);
2. 回写 [todo.md](../todo.md) 该线状态;
3. commit。

`check_docs` 门禁已对 `worklogs/**` 下的 `task_plan.md` / `findings.md` / `progress.md` 豁免 frontmatter(机器生成的过程件)。约定详见 [worklogs/README](../worklogs/README.md)。

## 9. 坑与注意

- **token 税只有两种止法**:legacy 逐工具调用灌 30 行是税源;**`.gitignore` 止不住**(hook 查文件是否存在,不看 git 跟踪)。要么**空闲期让盘上没有 plan**(收口归档后工作树干净 = 零注入),要么活跃任务用 `--autonomous`。
- **CJK 任务名会退化**:slugify 把非 ASCII 全替成 `-`,`前端重构` 会塌成空/残;**任务名用英文或拼音**。
- **autonomous 的 attest 摩擦**:它默认锁 plan 的 SHA-256,**每改一次 `task_plan.md` 都要重跑 `/plan-attest`**,否则 hook 报 `[PLAN TAMPERED]` 拒绝注入。所以「每阶段翻状态」的施工型计划别轻易上 autonomous。
- **它 ≠ todo.md**:`task_plan` 是**单任务**工作记忆;`todo.md` 是**跨工作线**的唯一滚动状态源。收口时务必把结论回写 todo,别让两者分叉。

## 10. 推荐用法(Scrollery 定制)

- **简单问答 / <5 步的活**:不用它,直接做。
- **一条长任务**:`init-session.sh "ascii-name"`(slug + legacy)——隔离、可并行、`.planning/` 已 gitignore 天然不泄漏。活跃期认那点逐调用税(是安全网的对价)。
- **并行第二条**:另开终端 `export PLAN_ID=<id>`。
- **收官**:归档进 `docs/worklogs/` + 回写 todo + commit。
- **只有**长时间无人值守 + plan 稳定时,才 `--autonomous`(省税,但接受 attest 摩擦)。

## 11. 速查表

| 想干什么 | 命令 |
|---|---|
| 起一个隔离计划(推荐) | `sh "${CLAUDE_PLUGIN_ROOT}/scripts/init-session.sh" "名"` |
| 起在仓根(最省事) | `/plan` 或 `/pwf` 或 `/start` |
| 看现在到哪了 | `/status` |
| 锁定计划防篡改 | `/plan-attest` |
| 定时自检循环 / 盯到完成 | `/plan-loop` / `/plan-goal` |
| 并行钉终端 | `export PLAN_ID=<id>` |
| `/clear` 后恢复 | 再调 skill,自动读三件套 |
