---
status: 快照
type: working-memory
line: 树内文件行打开与拖拽
created: 2026-07-17
---

# 进度日志:树内文件行打开与拖拽

## 会话:2026-07-17
- 做了:建三件套;从审查报告 §4.2/§4.3 蒸馏两需求实底;侦查员×2 摸底(打开链全按 mediaId 无 path 通道;拖拽实链=relocate_media_items 非 move_media_items,后者是删行重扫旧链——侦查员标 ⚠ 后主线亲验 historyStore 定案);写设计件(方案 B 收窄为弹层预览 D-001、白名单 txt/md/markdown D-002、拖拽复用 relocate 零后端 D-003);施工 A(后端 get_tree_text_preview + AppError::Preview + 前端 FilePreviewDialog + 双击分流 + Enter 可达 + i18n + fixture)与施工 B(onFilePointerDown + dragFileId 分离 + canDropFileOnDir + performFileDrop + autoScroll 回调注入 + suppressClick 补 onFileClick)全落地;todo.md S 节新行 + S 线设计件 §4.4 收窄标注。
- 验证(本地非 CI):cargo test --lib **615/0/5**(新增 7:预览 6 + serialize 契约 1)、clippy --lib --all-targets `-D warnings` 净、rustfmt 过(一行重排)、vitest **1155/1155**(88 文件,新增 9:isTextPreviewable 5 + canDropFileOnDir 4)、vue-tsc 净、ESLint(改动面 8 文件)净。
- 遗留:真机 GUI 验收(清单在 todo.md S 节新行);收口迁 worklogs 待 GUI 后;F-001/F-002 候选待收口裁。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
