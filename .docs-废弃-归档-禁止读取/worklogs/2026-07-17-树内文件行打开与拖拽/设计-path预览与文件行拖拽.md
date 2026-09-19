---
id: 2026-07-17-设计-path预览与文件行拖拽
status: active
type: design
line: 树内文件行打开与拖拽
created: 2026-07-17
---

# 设计:path-based 只读文本预览(问题②方案 B v1)+ 树内文件行拖拽 v1

> 依据:S 线审查报告 §4.2/§4.3(2026-07-16)+ 本任务阶段 1 摸底(findings.md)。
> 契约立场:开发期可推翻;两能力互不耦合,可独立回滚。

## 1. 方案 B v1:受限 path-based 应用内只读打开

### 1.1 范围裁定(v1 收窄)
- **白名单 = 纯文本扩展名:`txt` / `md` / `markdown`**(审查建议起步面;零解码器攻击面)。epub/图片等解码器面 v2 再议。
- **呈现 = 只读文本预览弹层**(新组件 FilePreviewDialog),**不**并行改造 mediaRoute/DocumentViewer 的 mediaId 取数链——审查估的「中等工程量」大头在查看器并行通道,v1 用弹层绕开,查看器全链零改动。纯文本渲染进 `<pre>` 文本节点,无 HTML 注入面;markdown **不渲染**(渲染=新增链接/图片攻击面)。
- 触发点:树内文件行双击,`isOpenableInApp` 为 false 且扩展名可预览时走预览;其余照旧 reveal。移动端预览可用(纯应用内,无 opener 依赖),reveal 门(isMobilePlatform)保持不变。

### 1.2 安全自评(F-003 口径)
- **S线 D-001 不违背**:不交系统默认应用、不执行;只读字节进 `<pre>`。
- 路径面:入参 `rootId + relPath` 与 `reveal_tree_entry` 同姿态,后端过 `resolve_within_root`(逐段拒 `..` + canonicalize + 根边界,utils/path.rs 现成)。
- 白名单在**后端**判定(取 canonicalize 后真实文件名扩展名,大小写不敏感);前端同款判定只作 UX 预筛,不是安全边界。
- 内容面:上限 1 MiB,超限截断并置 `truncated` 标;`from_utf8_lossy`(伪装 .txt 的二进制安全降级为替换字符,不 panic)。
- 错误面:结构化稳定码(同 R-08 姿态),**不回带绝对路径**;码 = `preview_unsupported_type` / `preview_failed`。

### 1.3 后端(tree_commands.rs)
```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeTextPreview {
    pub content: String,
    pub truncated: bool,
}

#[tauri::command]
pub async fn get_tree_text_preview(root_id: i64, rel_path: String, state: ...) -> Result<TreeTextPreview>
```
- spawn_blocking;查 root(同 reveal_tree_entry 取根路径方式)→ resolve_within_root → 扩展名白名单 → 打开文件 `take(1 MiB + 1)` 读 → 截断判定 → lossy 转码。
- error.rs 新变体 `AppError::Preview { code: &'static str, message: String }`(serialize 与 Reveal 同姿态)。
- registry.rs 注册;单测:白名单拒(exe/无扩展名)、txt 可读、截断标、越界路径拒(resolve_within_root 已有测试,补一条命令级)。

### 1.4 前端
- `constants/ipc.ts`:`GET_TREE_TEXT_PREVIEW: 'get_tree_text_preview'`。
- `folderTree.helpers.ts`:`isTextPreviewable(fileName)`(镜像白名单,注释标明后端是安全边界)。
- 新组件 `FilePreviewDialog.vue`(复用现有 dialog 原语姿态):标题=fileName;正文 `<pre>` 滚动区;截断提示条;桌面端补「在文件管理器中显示」按钮(复用 reveal 链)作逃生口。
- `FoldersSection.vue` `onFileDblClick` 分流:openable→照旧 return;`isTextPreviewable`→invoke 预览(错误按 code 分流 toast);否则→现行 reveal(移动端门不变)。
- `fileTitle` tooltip 文案分叉:可预览的未入库文件提示「双击预览」而非「双击在文件管理器中显示」。
- i18n(zh-CN/en-US):预览失败/不支持、截断提示、tooltip 新文案。
- harness fixture:`GET_TREE_TEXT_PREVIEW` 返回样例文本。
- 测试:helpers spec(isTextPreviewable);dblclick 分流逻辑若可低成本进组件测试则加,否则记 GUI 验收项。

## 2. 树内文件行拖拽 v1

### 2.1 语义裁定
- **拖文件行到树内目录 = 移动物理文件;Ctrl/⌘ = 复制**——与目录行拖拽同手势、同 ghost、同边缘自动滚动。
- **仅实体身份文件可拖**(`file.id !== null`);FS-only/未入库文件不可拖(身份缺失,审查 §4.3 明示叠加问题②,v1 不碰)。
- 落点 = 具实体身份的目录行(现行 `[data-dir-id]` 命中面);同目录落点无效(前端预筛 + 后端 relocate 本就 no-op)。文件无子树,无环检测。
- 拖到画廊无语义,不做。

### 2.2 复用面(零后端改动)
- 执行链 = `history.moveMedia([id], targetDirId, label)` / `history.copyMedia`(historyStore:192/:214)→ `relocate_media_items` / `copy_media_items_db`——同 id 保留(缩略图/AI 嵌入不失效)、undo/redo 完整、InvalidateOnWrite 已清 tree_snapshots(S 线批 2)、refresh() 重载树。
- 同名冲突:relocate 返回 `AppError::MoveFile("目标已存在同名文件…")`——catch 走现行 opFailed toast。

### 2.3 前端改动(FoldersSection.vue 为主)
- 文件行加 `@pointerdown="onFilePointerDown(row.file, $event)"`。
- `onFilePointerDown`:左键 + `file.id !== null` 才起手;beginPointerDrag 同款阈值/ghost(label=fileName);状态用**独立** `dragFileId`(不复用 dragId——目录 id 与媒体 id 是两个 id 空间,复用会把媒体 id 误入 canDropOnId 的目录索引)。
- 落点判定 `canDropFileOn(targetId)`:目录存在于 nodesById && targetId ≠ 文件当前目录(`nodesByKey.get(file.parentKey)?.id`;取不到时放行,后端 no-op 兜底)。
- 边缘自动滚动:把 `autoScrollStep` 里写死的 `recomputeDrop(autoScrollSrcId, …)` 改为存回调 `autoScrollRecompute`(目录拖拽与文件拖拽各自注入),行为不变。
- 拖拽源样式:文件行 `is-drag-source`(与目录行同款视觉);drag-over 高亮沿用 dropId。
- toast 复用 `sidebar.movedTo` / `sidebar.copiedTo`(文件名作 name)。
- 测试:落点判定/守卫抽纯函数进 helpers spec(FS-only 不可拖、同目录拒、目标须实体身份);拖拽手感属 GUI 验收。

## 3. 决策候选(登记 task_plan)
- D-001:方案 B v1 = 弹层预览而非查看器并行 path 通道(工程量与攻击面双收窄,可扩展)。
- D-002:白名单 txt/md/markdown,后端为安全边界,markdown 不渲染。
- D-003:文件行拖拽复用 relocate 链零后端改动;仅实体身份文件可拖;dragFileId 与 dragId 分离(id 空间不混)。

## 4. 验收
- 单测/类型/门禁:vitest、vue-tsc、ESLint、cargo test、clippy -D warnings、docs 双门。
- GUI 验收项(真机,并入 todo P4 池):隐藏目录 .md 双击弹预览可读、截断提示、exe 改名 .txt 显示替换字符不崩、文件行拖到目录移动成功+undo 还原、Ctrl 拖复制、FS-only 文件拖不动、移动端 .md 可预览。
