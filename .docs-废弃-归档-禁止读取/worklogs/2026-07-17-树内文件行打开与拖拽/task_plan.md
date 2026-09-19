---
status: 快照
type: working-memory
line: 树内文件行打开与拖拽
created: 2026-07-17
---

# 任务计划:树内文件行打开与拖拽

## 目标
落地 S 线遗留的两个单独立项(用户 2026-07-17 显式 go):
1. **问题②方案 B**:受限 path-based 应用内只读打开——树内「registered 但无 mediaId」的文件(隐藏目录内 .md 等)可在应用内查看,不再抛去文件管理器;白名单从纯文本类起步,不违 S 线 D-001(禁的是交系统默认应用执行,不是应用内只读查看)。
2. **树内文件行拖拽**:文件行历史上从未实现拖拽(非失效);补上「树内文件行拖到目录 = 移动/复制物理文件」能力,与目录行拖拽同手感。

## 当前阶段
收官:施工全落地+全量门绿;余真机 GUI 验收(清单已回填 todo.md S 节新行)

## 阶段

### 阶段 1:核实与调研
- [x] 侦查员×2 摸底:应用内打开全链(路由/查看器取数/reveal/resolve_within_root/assetProtocol)+ 拖拽双链(树目录拖拽 / 画廊卡片→树目录媒体移动)
- [x] 核对审查报告 §4.2/§4.3 断言在 T/U 线重构后是否仍成立(均成立;文件行仍无 pointerdown;打开链仍全按 mediaId)
- **状态:** completed

### 阶段 2:设计件
- [x] 方案 B 设计:弹层预览收窄(不并行改造查看器链)+ 白名单 txt/md/markdown + resolve_within_root + 安全自评
- [x] 文件行拖拽设计:v1 语义(拖到树目录=移动,Ctrl=复制)、仅实体身份文件、复用 relocate 链零后端改动
- [x] 设计文档落任务目录:[设计-path预览与文件行拖拽.md](设计-path预览与文件行拖拽.md)
- **状态:** completed

### 阶段 3:施工 A(path-based 应用内打开)
- [x] 后端:`get_tree_text_preview`(tree_commands.rs,resolve_within_root + 白名单取真实扩展名 + 1 MiB 截断 + lossy)+ `AppError::Preview{code,message}`(error.rs,serialize 契约测试)+ registry 注册;6 单测(白名单拒/大小写/截断/二进制伪装/目录伪装)
- [x] 前端:FilePreviewDialog.vue(UiDialog 原语,代际守卫,reveal 逃生口)+ onFileDblClick 分流 + Enter 键盘可达 + fileTitle 三分支 + `isTextPreviewable`(helpers+spec)+ IPC 常量 + i18n×5(zh/en)+ harness fixture
- **状态:** completed

### 阶段 4:施工 B(文件行拖拽)
- [x] 文件行 @pointerdown=onFilePointerDown(左键+hasEntityIdentity 守卫)+ dragFileId 独立状态 + recomputeFileDrop/canDropFileOnDir(helpers+spec)+ performFileDrop 复用 history.moveMedia/copyMedia + drag-source 样式 + suppressClick 补进 onFileClick + autoScroll 回调注入化
- **状态:** completed

### 阶段 5:验证与收口
- [x] 全量门(本地非 CI):vitest 1155/1155(88 文件)/ vue-tsc 净 / ESLint 净 / cargo test --lib 615/0/5 / clippy --lib --all-targets -D warnings 净 / rustfmt
- [x] todo.md 回写(S 节新行+旧行「待裁」改「已开工」)+ S 线设计件 §4.4 收窄标注
- [ ] 真机 GUI 验收(清单见 todo.md;收口迁 worklogs 待 GUI 后)
- **状态:** in_progress(仅余 GUI)

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空。引用 S 线旧决策一律写「S线 D-xxx」防撞号 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 方案 B v1 = 只读预览弹层,不并行改造 mediaRoute/查看器 mediaId 取数链 | 审查估「中等工程量」大头在查看器并行通道;弹层绕开后查看器零改动、攻击面收窄,后续可扩 | D-001 |
| 白名单 txt/md/markdown,后端为安全边界(canonicalize 后扩展名),markdown 不渲染只作纯文本 | 零解码器攻击面;渲染 md=新增链接/图片面;前端同款判定仅 UX 预筛 | D-002 |
| 文件行拖拽复用 relocate_media_items 链零后端改动;仅实体身份文件可拖;dragFileId 与 dragId 状态分离 | relocate 保 item id(缩略图/嵌入不失效)+undo 现成+树快照失效已覆盖;目录 id 与媒体 id 两个空间不可混 | D-003 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
