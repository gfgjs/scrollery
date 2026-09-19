---
id: 2026-07-14-closeout
status: snapshot
type: closeout
line: 文件树画廊定位唯一性修复
created: 2026-07-14
---

# 收口处置:文件树画廊定位唯一性修复

| 候选 ID | disposition | target | locator | 去重证据 | N/A 理由 | verified |
|---------|-------------|--------|---------|----------|----------|----------|
| F-001 | test | repo:src-tauri/src/db/queries.rs@c2536e6 | `folder_filename_sort_keeps_duplicate_paths_and_names_in_distinct_groups` | new | — | yes |
| F-002 | test | repo:src/components/sidebar/sections/folderTree.helpers.spec.ts@c2536e6 | `createFolderTreeSyncQueue` 的 latest-wins、null 失效与 drain 收尾测试 | new | — | yes |
| D-001 | code | repo:src-tauri/src/layout/items_cache.rs@c2536e6 | `build_dir_rank` / `derive_order` | new | — | yes |
| D-002 | code | repo:src/components/sidebar/sections/FoldersSection.vue@c2536e6 | `treeSyncTargetId` / `treeSyncQueue` | new | — | yes |
