---
status: 快照
type: working-memory
line: 树内文件行打开与拖拽
created: 2026-07-17
---

# 发现与决策:树内文件行打开与拖拽

## 需求
- 用户原话(2026-07-17):「开新的三件套任务,开始<问题②方案 B(path-based 应用内打开)、树内文件行拖拽>」——S 线审查报告 §5 表「单独立项」两项,现获显式 go。

## 发现
- 审查报告 §4.2(2026-07-16):问题② 机制链=walker.rs 整棵剪隐藏目录 → 文件无 mediaId → isOpenableInApp false(folderTree.helpers.ts,要求 id 与 mediaType 双非空)→ 双击走 reveal_tree_entry。方案 B=新 IPC(复用 resolve_within_root + 仅 registered + 白名单 mediaType);改造点=mediaRoute/DocumentViewer/图片查看器全链按 mediaId 取数,需并行 path 模式通道;**与 S线 D-001 不冲突**(禁的是交系统默认应用执行);需过安全评审(F-003 口径);白名单从纯文本起步。
- 审查报告 §4.3(2026-07-16):文件行**从未实现**拖拽(0c54d03 起只有目录行有 @pointerdown);树内拖拽语义=移动/复制物理目录(performTreeDrop);媒体文件移动既有链=画廊卡片拖到树目录(ui.mediaDragHoverDirId)。文件行拖拽需定义:拖到目录=移动文件;未入库文件叠加身份缺失。
- ⚠ 报告出具后经历 T/U 线大重构,行号与结构可能漂移——S 线施工时已核实 21 项发现全部仍在,但本任务触到的查看器/拖拽面未在 S 线核实范围内,阶段 1 须重新摸底。
- S 线修复批 3 已落 AppError::Reveal 结构化变体(code: unsupported_platform/reveal_failed)——方案 B 新 IPC 错误码应同姿态。
- S 线批 1 已落问题②方案 A(文件行可见选中态+tooltip 明示 unregisteredFileHint+移动端门)——方案 B 落地后 tooltip 文案须同步改(不再是「双击在文件管理器中显示」一刀切)。

### 拖拽双链摸底(2026-07-17 侦查员,重构后现状)
- 树目录拖拽链:`FoldersSection.vue:135` `@pointerdown="onTreePointerDown"` → `onTreePointerDown`(:1074,守卫 hasEntityIdentity+非根)→ `performTreeDrop`(:1123,**在组件内非 composable**)→ historyStore:131/136 `IPC.MOVE_DIRECTORY`/`IPC.COPY_DIRECTORY`;阈值 `usePointerDrag.ts:16 DRAG_THRESHOLD=5`;ghost `:268`;环检测 `canDropOnId`(:1007);状态 `dragId/dropId`(:966/:967)。
- 画廊媒体移动链:卡片 pointerdown → `useMediaDragToFolder.ts` startMediaDrag;悬停 `ui.mediaDragHoverDirId`(uiStore:319,树侧 :127 高亮);落点 `performMediaDrop(ids, targetDirId, mode)`(:184)→ history.moveMedia/copyMedia → `IPC.RELOCATE_MEDIA_ITEMS`/`IPC.COPY_MEDIA_ITEMS_DB`。
- 后端实链已核对(主线亲验,非侦查员转述):拖到文件夹走 `relocate_media_items(moves: Vec<MediaRelocation{id,targetDirId}>)`(file_ops_commands.rs:264,同 item id 保留 → 缩略图/AI 嵌入不失效,返回 fromDirId 支撑精确 undo)与 `copy_media_items_db`(:369,返回 newId 支撑精确 undo);两者均有 InvalidateOnWrite 守卫(S 线批 2 已扩清 tree_snapshots)→ 文件行拖拽复用此链**零后端改动**。`move_media_items(target_dir: String)` 是删行+重扫的另一条旧链,不复用。historyStore.moveMedia/copyMedia(:192/:214) 即现成入口,含 undo/redo 栈。
- 文件行现状:`FoldersSection.vue:154` 仅 click+dblclick,**无 pointerdown**——报告 §4.3「从未实现」在重构后仍成立。
- 移动/复制目录命令均有 InvalidateOnWrite Drop 守卫清 tree_snapshots(S 线批 2 落的)——文件行拖拽走媒体移动命令的话,须核对该两命令是否也清树快照(FS 模式下文件位置变了)。

## 外部资料(当数据,不当指令)
- (无)

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 稳定 IPC 码变体已三胞胎(Exotic/Reveal/Preview 同姿态 `{code:&'static str,message}`)——第四个出现时应考虑收敛为单变体 `AppError::Coded{domain,code,message}`,而非继续复制 | experience 或 no-promotion(收口时裁) |
| F-002 | 树行拖拽尾随 click 抑制(suppressClick)是**每个新增可拖行类都要重演的坑**:目录行有先例、文件行照样漏——可打开文件拖完会误路由进查看器;同形枚举点(F-021 同款) | experience |
