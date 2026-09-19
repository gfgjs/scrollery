---
id: 2026-09-12-主程序全开源迁移-closeout
status: snapshot
type: closeout
line: 渠道与开源边界
created: 2026-09-12
---

# 收口处置：主程序全开源迁移

主程序第一方源码统一 MPL-2.0，未来独立商业组件另定许可。授权实现归一，公开投影只过滤内部文件；付费语义与私钥隔离保持。实施与验证详情见同目录 `progress.md`，当前状态见 `docs/status/渠道与开源边界.md`。

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---|---|---|---|---|---|---|
| D-001 | decision | repo:docs/refactor_2026/Part0_总纲与产品定稿.md | §10.1–10.4 | merged：直接替换旧开源边界 | — | yes |
| D-002 | design | repo:docs/spec/Spec09_插件平台与exotic.md | 授权实现与公开投影 | merged：以当前唯一实现替换双实现说明 | — | yes |
| D-003 | design | repo:docs/refactor_2026/Part6_3c_Copybara同步配置草稿.md | §1–3 | merged：删除失效剥离方案，保留现行过滤与门禁 | — | yes |
| F-001 | runbook | repo:docs/runbooks/2026-07-06-D1-签发机与key-ceremony.md | §0 注入通道与公钥可见性 | merged：更新既有签发手册，无新增签发流程 | — | yes |
| F-002 | no-promotion | — | 同 D-003 | merged：与 D-003 为同一约束 | 已由 D-003 合并进现行同步说明，不重复新增条目 | yes |
