---
status: 快照
type: working-memory
line: asbuilt-spec
created: 2026-07-24
---

# 任务计划:施工规格集(docs/spec/,16 篇 as-built 重建级)

## 目标
为 Scrollery 出一套 as-built 重建级规格集(16 篇,`docs/spec/`):按当前代码现状撰写,使人类或低能力 LLM 能据此从零重建系统,不依赖阅读原始施工计划文体的 Part0-8。

## 当前阶段
阶段 1:脚手架 + 治理登记 — complete(本次)

## 16 篇 TOC
- README.md 索引
- 00 产品与架构全景
- 01 数据层
- 02 扫描与画廊
- 03 缩略图与派生
- 04 图像与色彩管线
- 05 视频与音频
- 06 AI-人脸-OCR
- 07 文档与阅读器
- 08 存储-备份-导出-文件操作
- 09 插件平台与exotic
- 10 IPC与错误契约
- 11 前端架构
- 12 配置-状态-日志
- 13 构建-发布-商业化
- 14 不变量与约定

## DAG / 分批
阶段1 脚手架+治理(本任务)→ 阶段2 样板2篇(01数据层+06AI)审后锁模板 → 阶段3 子系统篇分批并发(02/03/04/05,07/08/09/11,12/13)每批随批复核 → 阶段4 聚合篇(10 IPC契约、14 不变量)opus复核 → 阶段5 封顶(00 + README索引 + 交叉链接)→ 阶段6 收口(一致性+治理合规复核+单一git写手提交,CI 跑 worklog-kit)。

## 治理决策
| 决策 | 理由 |
|------|------|
| 新增顶层目录 `docs/spec/`,登记 `.worklogrc.jsonc` dirs + `docs/README.md` 目录职责表 | `refactor_2026/` 本身即"新增顶层目录"先例,语义上 spec 与既有目录均不吻合(非计划体、非单点设计) |
| `line: asbuilt-spec`,建 `docs/lines/asbuilt-spec.md` + `docs/status/asbuilt-spec.md` | 治理体系要求正典文档挂靠工作线实体 |
| frontmatter 方案(见下)全篇统一 `type: canon`、`status: active`、`line: asbuilt-spec`、`created: 2026-07-24`,id 仿 Part 文件命名规律(`PartN_标题.md` → id=`创建日期-PartN_标题`)同构为 `SpecNN_标题.md` → id=`创建日期-SpecNN_标题` | 与既有 Part0-8 frontmatter 结构逐字段对齐,便于同一批治理/门禁工具处理;`.worklogrc.jsonc` 机器面锁 ASCII canonical,严格使用英文枚举值,不沿用近期出现的中文字面值写法(`status: 现行` 等) |
| 门禁外置:`worklog-kit`(外部 npm 包,`tools/check_docs.mjs` 已于 2026-07-21 移除)由 CI(`docs-governance.yml`)校验,本仓不可本地跑等价检查(只能装 npx 跑,非本任务范围) | 本仓无法定制/复现门禁逻辑,新文档只能靠遵循既有 frontmatter/目录惯例规避红线,收口前建议委托 CI 或用户手动 `npx --package worklog-kit@0.1.0-alpha.4 worklog-kit check` 验证一次 |
| 写手模型 = sonnet;聚合篇(00 全景、10 IPC契约、14 不变量)opus 复核 | 子系统篇工作量大但范围窄,sonnet 够用;聚合篇跨系统一致性风险高,需 opus 复核把关 |
| 引用纪律:命名/品牌、Part1 schema 现状、04 色彩管理+05 视频硬解、06 第4插件OCR+MF死等故障、02轴/minimap+11前端UI原语——新写;打包/盈利/四层防护(Part0)、协议骨架+开闭源边界(Part6)、CI矩阵+渠道cfg门控(Part7)、签发/加密/激活(Part8)——引用讲理由,不复述 | 决策理由本身未过时的部分应链接回 Part0-8,避免重复论证;事实性内容(命名、schema版本、新增子系统)已漂移或原文无对应章节,续写等于重新论证一遍没有意义 |

## frontmatter 样板(真实例,01 数据层篇)
```
---
id: 2026-07-24-Spec01_数据层
status: 快照
type: canon
line: asbuilt-spec
created: 2026-07-24
---
```

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 文件命名 `SpecNN_标题.md`(数字两位,标题用中文,下划线分隔,同构 `PartN_标题.md`) | 与既有 refactor_2026 目录命名习惯完全对齐,治理工具/人工检索零学习成本 | |
| spec/ 内暂不加交叉链接(仅 README.md TOC 为纯文本清单) | 目标 15 篇正文尚未创建,加链接会被 check-docs 断链门拦截;链接留到阶段5封顶统一补 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
