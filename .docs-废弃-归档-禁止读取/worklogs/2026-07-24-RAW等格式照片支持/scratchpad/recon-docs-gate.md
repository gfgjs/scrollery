---
status: active
type: working-memory
line: RAW等格式照片支持
id: recon-docs-gate
created: 2026-07-24
---

# 摸底报告:docs 门禁 RAW 线红项 + worklog-kit 播种能力

## 1. docs 门禁命令定位

可跑命令：`npx --yes --package worklog-kit@0.1.0-alpha.4 worklog-kit check`  
定义处：`.github/workflows/docs-governance.yml:41`（核心 gate job；同含 `index` 命令在 42 行）

补充：整套门禁依赖 `.worklogrc.jsonc`（config）、`.worklog-baseline.json`（brownfield 豁免清单）与 `docs/README.md`（frontmatter schema 唯一真源）。

## 2. 实际跑门禁:RAW 线红项统计

**退出码：0**（门禁通过）

输出摘要：`✓ docs 门禁通过(断链+frontmatter+位置+收口;495 个文档,11600 个代码/配置文件)；· 存量豁免 158 条(命中 .worklog-baseline.json;brownfield 档)`

**结论**：无 RAW 线新增红项。两个 untracked 文件（`docs/lines/RAW等格式照片支持.md` 与 `docs/status/RAW等格式照片支持.md`）非 git 跟踪资源，门禁不检查 untracked；158 条存量豁免项对应历史债项，非本线新增。

## 3. worklog-kit upgrade 子命令能力

help 定义（摘自 `worklog-kit --help`）：  
`upgrade [--dry-run]` — 按 schemaVersion 逐级迁移配置(先备份、原子写、写后复验、失败回滚)

**关键发现**：upgrade **确实播种 docs/lines/ 实体文件**。证据：
- `docs/lines/RAW等格式照片支持.md` 文首注释：`(占位实体,由 \`worklog-kit upgrade\` 从存量 \`line\` 值播种。请复核并补一句话使命,可选 owner。)`
- `docs/status/RAW等格式照片支持.md` 文首注释：`(分片由 \`worklog-kit upgrade\` 生成;收口时 disposition=todo 的候选落此,请随施工滚动更新。)`

**结论**：upgrade 幂等播种操作，从 .worklogrc.jsonc 中 `dispositions` 定义的 `todo` 靶点（配置值 `"statusDir": "docs/status"`）与现存 planning/worklogs 三件套中引用的 line 字段值，自动生成对应的 lines 实体与 status 滚动分片。此是工作线存在性同步机制的核心。

## 4. untracked 文件现状

**docs/lines/RAW等格式照片支持.md**  
Frontmatter（全文）：  
```yaml
---
id: 2026-07-24-RAW等格式照片支持
status: active
type: line
line: RAW等格式照片支持
created: 2026-07-24
---
```
正文 3 行内摘要：占位实体，由 upgrade 生成，需补一句话使命与可选 owner。  
标记：有「生成器」标记（文首注释写明由 upgrade 生成）。  
判断：**工具产出**——frontmatter/占位文本/注释均为 upgrade 模板，非人手编写。

**docs/status/RAW等格式照片支持.md**  
Frontmatter（全文）：  
```yaml
---
id: 2026-07-24-status-RAW等格式照片支持
status: active
type: rolling-status
line: RAW等格式照片支持
created: 2026-07-24
---
```
正文 3 行内摘要：分片由 upgrade 生成，收口时 disposition=todo 的候选落此，随施工滚动更新。  
标记：有「生成器」标记（文首注释写明由 upgrade 生成）。  
判断：**工具产出**——同上，upgrade 标准模板。

## 5. 门禁对 scratchpad research 文件的 frontmatter 契约

规则定义：`docs/worklogs/README.md:33`

**摘录契约原文**：
> docs 门...对 \`worklogs/**\` 与 \`planning/**\` 下的 \`task_plan.md\` / \`findings.md\` / \`progress.md\` 豁免 frontmatter 与断链检查...。**\`closeout.md\` 与两目录内手写文件(README/索引/验收清单等)不在豁免名单**,照常受治理(需合法 frontmatter:status/type/created)。

**对 research 文件适用**：scratchpad/ 下的 research 文件属「两目录内手写文件」范畴，不在豁免名单。必须含合法 frontmatter，**最少字段**：`status` + `type` + `created`（对应 status 枚举值、type 枚举值、ISO 日期）。

## 6. research-R2-libraw-binding.md 的 frontmatter 现状

**完整 frontmatter**（第 1–7 行）：
```yaml
---
status: active
type: working-memory
line: RAW等格式照片支持
id: research-R2
created: 2026-07-24
---
```

**合规性**：✓ 满足。status ∈ {draft/active/snapshot/superseded/archived}；type ∈ 配置枚举（working-memory 对应已下沉 Scrollery 自定义 type）；line 字段指向存在的工作线（docs/lines/RAW等格式照片支持.md）；created 为有效日期；额外字段 id + line 均为允许的可选扩展。

---

## 改动清单

无改动。（本摸底为只读操作，未 git add/commit）

---

## 顺手发现

无。
