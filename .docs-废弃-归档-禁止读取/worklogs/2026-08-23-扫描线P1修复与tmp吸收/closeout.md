---
id: 2026-08-23-扫描线P1修复与tmp吸收-closeout
status: snapshot
type: closeout
line: 扫描线P1修复与tmp吸收
created: 2026-08-23
---

# 收口处置:扫描线P1修复与tmp吸收

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | experience | repo:docs/experience.md | §51 单写连接上的 TEMP 表必须按业务域隔离 | new | — | yes |
| F-002 | experience | repo:docs/experience.md | §51 TEMP 表用后即毁(temp_store=MEMORY 常驻累积) | new | — | yes |
| F-003 | experience | repo:docs/experience.md | §49 批循环 LIMIT 分页优先 keyset(复杂序建一次性 TEMP 队列表) | repo:docs/experience.md | — | yes |
| F-004 | experience | repo:docs/experience.md | §51 fire-and-forget 任务的连接级资源命名带调用代次 | new | — | yes |
| F-005 | todo | repo:docs/status/扫描线P1修复与tmp吸收.md | 2026-08-23 增补·余项②(live photo 分片流式与重排降频立项) | new | — | yes |
| F-006 | todo | repo:docs/status/扫描线P1修复与tmp吸收.md | 2026-08-23 增补·余项①(CI dev 既有红独立立项修复) | new | — | yes |
| D-001 | code | repo:src-tauri/src/db/queries/scan/mark_missing.rs@6c275e7 | seen_table_name / SeenWriter / mark_missing_preloaded | new | — | yes |
| D-002 | code | repo:src-tauri/src/scanner/enricher.rs@226e7c5 | next_queue_gen / QueueGuard | new | — | yes |
| D-003 | code | repo:src-tauri/src/scanner/enricher.rs@226e7c5 | image_queue_create_sql / image_queue_fill_sql / image_queue_fetch_sql | new | — | yes |
| D-004 | todo | repo:docs/status/扫描线P1修复与tmp吸收.md | 2026-08-23 增补·余项②(不移植 live photo/防抖的裁定) | new | — | yes |
| D-005 | todo | repo:docs/status/扫描线P1修复与tmp吸收.md | 2026-08-23 增补·余项①(CI 基线红登记不修的裁定) | new | — | yes |
