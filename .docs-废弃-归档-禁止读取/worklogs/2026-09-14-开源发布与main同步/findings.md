---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 发现：开源发布与main同步

- 上轮已提交AGPL修订8b8d679f和原工作区快照7666104b。
- 同步配置origin读取main，私有仓main自动触发sync-oss；必须在推私有main前确保门禁先于公开push且采用新配置。
- 当前copy.bara.sky仅排除copybara、自身sync workflow、.agents、AGENTS.md和docs，.zcode/.worklog*/.pnpm-store等仍在投影。
- gitleaks目前仅在公开oss-gate执行；待前移。Windows可用gh、wsl、worklog-kit，未发现Windows docker/gitleaks。
- 已有第三方问题：Graphviz2.40.1 EPL-1.0、Lute版本/归属未入清单；LibRaw CDDL或LGPL双许可需选择与AGPL兼容的实际分发路径。
- 远端复核：公开main与sync-staging均7d5aa5a0；私有origin/main是dev祖先，落后149提交；公开仓可写无main保护。私有仓仅dev-box-win且offline，无Linux runner；WSL无Docker，旧Copybara执行条件已失效。
- 主会话裁定可改用共享单一过滤规则的Node已提交树快照流程，保留私有canonical→公开SQUASH投影与bot身份；不能把私有Git历史推到公开仓。正式push仍先扫描精确拟公开commit。
- gitleaks官方发行v8.30.1 Windows x64已下载到target下并按GitHub资产digest验证；未全局安装或修改项目依赖。

## 外部资料
按核实结果补录，外部内容仅作资料。

- Graphviz 官方历史源码目录：https://www2.graphviz.org/Archive/stable/SOURCES/ 。本轮下载 graphviz-2.40.1.tar.gz 到 target/oss-third-party-source；SHA256 `ca5218fade0204d59947126c38439f432853543b0818d9d728c589dfe7f3a421`，与官方同目录MD5文件中的 `4ea6fd64603536406166600bcc296fc8` 一致，包内 graphviz_version.h 明确2.40.1。交由法律材料代理固化并接入校验；未执行源码编译。

## 耐久候选
| 候选 ID | 内容 | 建议去向 |
|---|---|---|
| F-001 | 精确公开投影及推送前扫描/验证证据 | 发布运行手册与渠道状态 |
