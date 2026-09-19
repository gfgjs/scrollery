---
id: 2026-09-14-开源前最终检查-边界专项
status: snapshot
type: review
line: 渠道与开源边界
created: 2026-09-14
---

# 任务A:开源投影边界与泄密风险(只读核查)

核查对象:`copy.bara.sky`、`.gitignore`、`.github/workflows/{sync-oss,oss-gate,docs-governance}.yml`、跟踪文件面与本地历史。
基线:本地 HEAD `12787c12`(工作树另有未提交改动);9/12 结论仅作线索,逐条以当前代码复核。
线索文档:`docs/reviews/2026-09-12-全仓开源前屏蔽与收费组件边界.md`(O-01..O-05)。

## 1. 投影路径集合实算

命令(仓根,只读):

```
$t = git ls-files
$proj = $t | Where-Object { $_ -notmatch '^(copy\.bara\.sky|\.github/workflows/sync-oss\.yml|AGENTS\.md)$' -and $_ -notmatch '^\.agents/' -and $_ -notmatch '^docs/' }
$t.Count; $proj.Count; $t.Count - $proj.Count
```

结果:`TOTAL=2574 PROJECTED=1582 EXCLUDED=992`(排除面 = copy.bara.sky、.github/workflows/sync-oss.yml、AGENTS.md、.agents/**、docs/**,共 5 条,`copy.bara.sky:33-42`)。
排除面构成交叉校验:docs/**=988 + .agents/**=1 + AGENTS.md=1 + copy.bara.sky=1 + .github/workflows/sync-oss.yml=1 = 992,与实算 EXCLUDED 一致。
投影内顶层分布:src=573、public=537、src-tauri=298、crates=75、scripts=34、third-party=13、tools=9、.github=8、.worklog=6,根部散件 22。

投影来源与计数口径:Copybara 两 workflow 均用 `git.origin`(`copy.bara.sky:58`、`:65`),投影取自**已提交树**,未跟踪文件不参与投影。
索引计数与 HEAD 树复核一致:`git ls-tree -r --name-only HEAD` 得 2574 条,`git ls-files`(索引)得 2574 条,`Compare-Object` 差异 0 行 → 上述 2574/1582/992 对当前 HEAD 成立。
本报告不主张「工作树与投影一致」:工作树另有未提交改动与未跟踪文件；一致的是索引与 HEAD 的文件路径集合。

## 2. 9/12 O-01..O-05 复核

| 项 | 现状 | 证据 | 严重性 |
| --- | --- | --- | --- |
| O-01 | **仍在** | `copy.bara.sky:33-42` 仅文件排除、`:50 transformations = []`;`sync-oss.yml:45-116` 步骤链(guard→checkout→装 key→docker run Copybara→擦 key)无任何提交前扫描;`oss-gate.yml:66-76` gitleaks 在公开仓 `sync-staging` push **之后** | P0(发布阻断) |
| O-02 | **仍在** | `.zcode/plans/plan-sess_a42b2596-6180-4fdc-a6e5-f3dcc6b37672.md`、`.worklog-baseline.json`、`.worklogrc.jsonc`、`.worklog/{manifest.json,templates/*}`(6) 均被跟踪且不在排除面 → 进投影;`.worklog-baseline.json` 正文按条列 `docs/**` 内部路径(首条 `docs/designs/2026-07-19-...`) | P1 |
| O-03 | **本地已闭环** | `.claude/**`、`.commandcode/**` 均不在 HEAD(`git ls-files` 无命中)、磁盘不存在(`Test-Path=False`);删除提交 `67161593`。工作树无残余 | — (远端另计) |
| O-04 | **仍在** | `docs-governance.yml:41-42` 仍调 `worklog-kit check/index`,而 `docs/**` 被排除、`.worklogrc.jsonc`+`.worklog-baseline.json` 反而进投影;`tools/check_plan_canonical.mjs` 读 `docs/`(投影内);`scripts/install-skills.mjs:17` 读 `.agents/`(投影内,`.agents/**` 被排除) | P2 |
| O-05 | **仍在** | `oss-gate.yml:51-53` 只造空 `dist`;`:55-60` 跑根 workspace `cargo check/test --locked`。根 workspace members = `src-tauri` + `crates/*`9 个(`Cargo.toml:16-25`);`raw-worker`/`raw-probe` 是独立空 workspace(`Cargo.toml:10` 注释),不被 `--workspace` 覆盖;`ci.yml:33,118,151,233,283` 全部以 `github.repository == 'gfgjs/scrollery-private'` 守门 → 公开树无前端/独立 worker CI | P2 |

## 3. 本轮新增事实

| 编号 | 事实 | 证据 | 严重性 |
| --- | --- | --- | --- |
| B-01 | 投影内私有仓名 + 死工作流(非泄密) | `.github/workflows/ci.yml:33,118,151,233,283` 各含 `scrollery-private` 1 次(`git grep -c`)。私有仓名不是凭据,该仓本就由同一组织公开镜像;**无需高优先级过滤**;可斟酌的只是这份全 skipped 的 workflow 在公开树的噪声 | P3(非泄密) |
| B-02 | 未跟踪 scratch 目录**无 ignore 规则** | `.research-tmp/` 79 个未跟踪文件;`git check-ignore -v --no-index .research-tmp` 无输出 → `git add -A` 会入库并随下次同步投影 | P1(入库风险) |
| B-03 | 本地 store 产物入库并进投影 | `.pnpm-store/v11/index.db`(8192B)被跟踪;未发现本机路径(`Users.(gf|C:)` 计数 0) | P3 |
| B-04 | **误报,非凭据** | `public/vditor/dist/js/graphviz/full.render.js`:`AKIA[0-9A-Z]{16}` **大小写敏感 0 命中**;此前「1 行命中」来自我方 `git grep -i`(`-i` 使 `[0-9A-Z]` 同时匹配小写)。大小写不敏感 3 处(各 20 字符)全落在**同一条 200,824 字符 base64 长串**内(第 7 行,串起于字符偏移 17678;命中在串内偏移 32260 / 33125 / 84602);该长串解码抽样 `printable_ratio=0.14`(二进制,非文本),解码窗口内不含 AWS 形状;文件内 `accessKey`/`secretAccess`/`credentials`/`AWS_ACCESS` 计数均 0(无 AWS 语义),`AKIAIOSFODNN7EXAMPLE` 计数 0。上游一致:`node_modules/vditor/dist/js/graphviz/full.render.js` 存在且 blob 与投影内文件同为 `c96b7a943e80d7e452b91c9510e2dada4dc0f54d`(无本地改动) | 结案(误报) |
| B-05 | 死链面 | 80 个投影内文件含 `docs/` 引用(多为注释/内部路径引用) | P3 |

## 4. 分类清单

必须过滤(canonical 私有保留,不进公开):`.zcode/**`(1 文件)、`.worklog/**`(6)、`.worklogrc.jsonc`、`.worklog-baseline.json`、`copy.bara.sky`、`.github/workflows/sync-oss.yml`、`AGENTS.md`、`.agents/**`(1)、`docs/**`(988)——后五项已在排除面,前四项为本轮实测遗漏。
建议过滤/一并对齐:`.github/workflows/docs-governance.yml`、`tools/check_plan_canonical.mjs`、`scripts/install-skills.mjs`、`.github/copilot-instructions.md`、`.github/instructions/mermaid.instructions.md`;`.pnpm-store/**`(本地 store 索引,无功能价值)。
无需过滤:`third-party/**`(13,随附许可材料)、`public/**`(537,含 vditor 第三方 dist)、`crates/**`+`src-tauri/**`+`src/**`、根 `LICENSE/NOTICE.md/SOURCE.md/CHANGELOG.md/CONTRIBUTING.md/CLA.md/TRADEMARK.md/README*.md`、`.gitattributes`、`.prettier*`、`.vscode/extensions.json`、`.github/workflows/{oss-gate,drift-alarm,release}.yml`、`.github/CODEOWNERS`(全为注释占位 `@OWNER`,无真实账号)。
另 `.github/workflows/ci.yml` 无需过滤:公开树内 5 个 job 全被 `github.repository` 守门跳过,私有仓名非凭据;若在意死工作流观感,属可选项而非安全项。

## 5. 忽略覆盖与本地历史(私钥面,未读内容)

`git check-ignore -v` 命中:`.release-keys/`→`.gitignore:70`、`/prod-signing/`→`:65`、`.internal-signing/`→`:60`、`.dev-registry/`→`:56`、`.claude/settings.local.json`→`:84`、`.screenshots/`→`:89`;`.research-tmp/` **无命中**(缺口)。
磁盘存在且被忽略:`.release-keys/`、`.internal-signing/`、`.dev-registry/`；`.research-tmp/` 存在但未被忽略；`prod-signing/`、`.claude/` 不存在。
本地历史:`.release-keys`、`prod-signing`、`.internal-signing`、`.dev-registry`、`.research-tmp` 的 `git log --diff-filter=A` 提交数 = 0 → 本仓历史未入库过这些路径。

## 6. 秘密扫描口径与实际范围

扫描器可用性:`gitleaks`、`trufflehog`、`gpg` 在本机**均不存在**(`Get-Command` 全部 absent),本轮**未安装任何工具**。替代手段 = `git grep` + `rg`(`rg` 位于 WinGet Links,`python` 3.11、`node` 亦可但未用)。

实际文本扫描范围(仅文本、`-I` 跳过二进制):

1. `git grep --cached -I -c -i -E <模式>`:覆盖**已提交(索引)文本文件**,回报**匹配行计数**。9 组模式 = PEM 私钥头(`-----BEGIN … PRIVATE KEY-----`)、GitHub token(`gh[pousr]_` + 36)、AWS access key(`AKIA` + 16)、Slack(`xox[baprs]-`)、OpenAI `sk-`、Google `AIza` + 35、JWT(`eyJ…` + `.`)、`password|secret|api_key|access_token|client_secret = "<16+>"` 赋值、`scrollery-private`。
2. `rg --no-heading -c -i -e <同上模式> .research-tmp`:覆盖未跟踪 scratch 目录文本,结果为「除同一处外无命中」。
3. 未覆盖:未跟踪面仅扫 `.research-tmp/`(`docs/` 8 个、`src/` 3 个未跟踪文件未逐个扫描);二进制文件内容未扫;ignored 私钥目录内容未读。

输出纪律:全程只输出「模式名 + 路径:行计数」与位置/边界属性,未打印任何候选值,亦未输出解码内容。

## 7. 未验证项

1. 未读 ignored 私钥目录内容(按约束跳过);私钥目录内容从未进入本报告。
2. 未实跑 Copybara 投影(本机无 Copybara/Docker),1582 为按排除面复算值。
3. 未审远端历史;已知(主会话)公开 `main=7d5aa5a0`(2026-07-06)仍含 `CLAUDE.md`/`CLAUDE_中文对照.md`,属公开仓历史残留,须远端处置,投影配置无法覆盖。
4. 公开侧 gitleaks 未实跑(该 job 只在公开仓运行),本机也无 gitleaks/trufflehog → 「未见秘密」只覆盖 §6 所述文本范围,不等于全量扫描认证。
