---
id: 2026-07-17-方案A-导出整理成果
status: active
type: design
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-17
last-verified: 2026-07-19
---

# 方案 A:导出整理成果

> 2026-07-18 复审结论:**有条件通过**。本稿已按三稿落盘 commit `247d05c` 之后的代码终态重审；
> 施工前须按本稿替换原来的「前端传全量有序 ID + Channel-only 进度 + 直接写用户目录」设计。
> 2026-07-19 二次核对:对 HEAD(`5f92f1d`，含 F-025 `RunTokenSlot` 四槽统一 `0bf5ebb`)逐项复核承重契约，结论与工程量不变。
> 本线只更新方案，不施工。

## 1. 目标与导出契约

- 导出对象 = 当前选区或当前相册/画廊视图的**有序媒体集合**。标签、收藏、搜索结果由当前视图 + 全选自然覆盖，不另建第二套筛选契约。
- v1 导出的是**原始文件字节 + 可选整理清单**，不是重编码、同步或备份；不改源文件、不改库内状态。
- 相册手排、当前视图顺序通过文件名前缀或 manifest 保留；标签、评级、色标、收藏、相册归属只进 manifest。
- V20 新增的 `media_items.view_rotation` 是看图台展示偏好，物理复制不会把它烤进像素。v1 必须在预检中提示非零项，并把角度写入 manifest；「烤入展示旋转」属于重编码导出，进 P2。
- 全量进入 free：anti lock-in 是产品信任项，现有代码也没有通用 per-feature gating 原语。

## 2. v1 范围

### 2.1 输入集合

- `start_export` 直接接收现有 `SelectionDescriptor`，不再由前端物化全量 ID。
- `Explicit` 保持显式选择顺序；`SelectAll` 复用 `db::queries::layout::resolve_selection`，按 `ViewDescriptor` 布局序解析，并沿用 `layoutVersion` 的 `ViewStale` 守门。
- V21 已让 `SelectAll` 自动排除隐藏扫描根；导出不得另写一套 WHERE 或绕过该规则。
- 后端可在任务开始时把 ID 物化为 `Vec<i64>` 形成稳定快照，但元数据与路径必须分块查询（建议每批 500～1000），不得拼超长 `IN` 或逐项 N+1 查询。

### 2.2 输出形态

- 用户选择的是**父目录**；应用在其下创建独立目录 `Scrollery-export-{YYYYMMDD-HHmmss}`。
- 施工目录使用同级、应用自有的 `.scrollery-export-{jobId}.tmp`；全部文件与 manifest 成功落盘后，同卷 rename 为正式目录。
- 取消/失败只清理本 job 创建的 staging 目录；永不递归删除用户既有目录。崩溃遗留 staging 在下次启动列出并允许清理，不静默扫任意 `.tmp`。
- v1 不提供 `overwrite`。内部同名冲突只支持 `rename`（默认）与 `skip`；独立导出目录已消除覆盖用户既有文件的正当需求。

### 2.3 文件命名与元数据

命名档：

| 档位 | 规则 | 用途 |
|---|---|---|
| `original` | 原文件名 | 普通拷贝 |
| `sequence` | `001-原名.jpg`，位宽 `max(3,total 位数)` | 保留手排/视图顺序 |
| `date` | `YYYYMMDD_HHmmss-原名.jpg`，取 `sort_datetime` | 按拍摄时间交换 |

- 文件名生成须处理 Windows 保留名、结尾空格/点、非法分隔符、空 stem、大小写不敏感文件系统冲突与路径过长；单项失败进入结果明细，不让整批崩溃。
- 源文件名来自 DB 仍须验证为单一文件名组件；源绝对路径由 `root + rel_path + file_name` 在 Rust 侧解析，前端不传源路径。
- 复制后显式保留源文件修改时间；不得以「Windows 当前碰巧保留」替代跨平台契约。
- 预检返回：条数、DB `file_size` 汇总的近似体积、离线/缺失数、非零 `view_rotation` 数、库内目的地警告。体积标明“估算”，源在执行中仍可能变化。

### 2.4 manifest

- 文件名固定为 `manifest.scrollery.json`，在全部文件落地后最后原子写入。
- 默认关闭，勾选态可记忆；UI 明示其中含标签、相册名和相对路径，分享前应检查隐私。
- 只记录成功导出的项；失败/跳过明细由任务结果返回。禁止写源绝对路径、凭据、人脸框/人物身份。
- `source` 使用结构化枚举，不接收前端自由字符串。

```json
{
  "formatVersion": 1,
  "app": "Scrollery",
  "appVersion": "0.1.0",
  "exportedAtUtc": "2026-07-18T12:00:00Z",
  "source": { "kind": "album", "id": 12, "name": "精选" },
  "items": [{
    "file": "001-IMG_0001.jpg",
    "rootAlias": "照片库",
    "rootRelativePath": "2025/旅行/IMG_0001.jpg",
    "sortIndex": 1,
    "viewRotation": 90,
    "rating": 4,
    "colorLabel": 2,
    "favorited": true,
    "tags": ["家人", "2025旅行"],
    "albums": ["精选"]
  }]
}
```

## 3. 后端设计

新模块 `src-tauri/src/ipc/export_commands.rs`，注册进 `ipc/registry.rs`，命令名同步到 `src/constants/ipc.ts`。

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub selection: SelectionDescriptor,
    pub target_parent: String,
    pub naming: ExportNaming,
    pub conflict: ExportConflict, // Rename | Skip
    pub include_manifest: bool,
    pub source: ExportSource,
    pub allow_inside_library: bool,
}

#[tauri::command]
pub async fn preflight_export(req: ExportRequest, state: ...) -> Result<ExportPreflight>;
#[tauri::command]
pub async fn start_export(req: ExportRequest, app: AppHandle, state: ...) -> Result<ExportJobId>;
#[tauri::command]
pub fn export_status(state: ...) -> Result<ExportJobSnapshot>;
#[tauri::command]
pub async fn stop_export(job_id: String, state: ...) -> Result<()>;
```

### 3.1 任务生命周期

- 不再用 Channel 作为唯一进度源。2026-07-18 缩略图线已实证：Channel 随发起它的 WebView 销毁，后台仍跑时 UI 会永久丢进度。
- 采用 `AppState` 快照 + app 级事件 + `export_status` 查询；前端先订阅事件、再启动任务，刷新/重建后重新订阅并查快照。
- token 使用现有 `RunTokenSlot` 的 generation + compare-and-clear 纪律；旧轮迟到收尾不得清掉新轮状态或发布新轮终态。state.rs 现并存两种终态门控姿态(F-025 迁移后)，语义不同勿混用:thumb/derive 以 `finish()` 返回值门控终态发布，ai/face 以 `!token.is_cancelled()` 门控终态副作用；导出这类「快照 + 事件」文件任务采**前者**，勿仿 ai/face。
- 同时只允许一个文件类 job（A 导出或 B 备份）。已占用时返回 `file_job_busy`，不以“新 start 自动取消旧任务”制造隐式数据损失。
- 启动、停止与完成都带 `job_id`；停止只能取消匹配任务。

### 3.2 路径与复制

1. 后端规范化并 canonicalize 已存在的父目录，确认是目录且可写；测试文件名必须应用自有随机前缀并立即清理。
2. 与所有 `scan_roots.path` 做规范化的路径组件比较，不能用字符串 `starts_with`。命中库内时先返回 warning；前端确认后以 `allow_inside_library=true` 重试，后端仍须重新检查。
3. 每项复制到 staging 内的 `.{name}.{seq}.tmp`，flush/close 后 rename 为目标文件；取消检查放在查询批、每项复制前后和 manifest 前。
4. 源在预检后消失、卷离线、权限失败均记稳定单项码并继续；磁盘满、目标目录失联等任务级故障停止任务并保留可诊断结果。
5. 完成后写 manifest、同步目录所需数据、rename 整个 staging 目录；正式目录只代表完整导出。

### 3.3 错误契约

- 新增结构化 `AppError::Export { code, message }`，沿现有 `Exotic/Reveal/Relink` 模式把稳定小写 code 原样序列化；message 不带内部绝对路径。
- 至少覆盖：`export_target_invalid`、`export_target_not_writable`、`export_target_inside_library`、`export_already_running`、`export_view_stale`、`export_cancelled`、`export_io`。
- 单文件结果只返回脱敏文件名 + 稳定码；上限封顶（例如前 100 条 + 总数），避免百万失败项撑爆 IPC。

## 4. 前端设计

- 入口：选区工具条、媒体右键、相册/当前视图工具栏；全部生成同一 `SelectionDescriptor`。
- `ExportDialog.vue`：目的父目录、命名档、冲突策略、manifest 开关、预检摘要与库内/旋转警告。`ViewStale` 时重算布局后要求用户再次确认，不静默换集合。
- 进度：新增 A/B 共用的轻量 `BackgroundFileJobIndicator`，放在 `AppStatusBar` 右区；当前左区会被查看器文件信息或 docked 选区替换，不能再把文件 job 仅塞左区。它不是任务中心，只展示唯一文件 job 的进度、取消和完成入口。
- `exportStore` 负责事件 + 快照恢复；完成 toast 提供“打开导出目录”。
- zh-CN/en-US 文案与错误码分流齐全。

## 5. 测试与验收

### 自动化

- 选区：`Explicit` 原序、`SelectAll` 布局序、V21 隐藏根排除、`ViewStale` 拒绝、超大 SelectAll 不经前端传全量 ID。
- 查询：分块元数据查询不乱序、不重复，不出现逐项 SQL。
- 文件系统：staging→正式目录、取消/磁盘满/源消失的 partial 清理；确认只删本 job 自有路径。
- 命名：保留名、非法字符、结尾点/空格、大小写冲突、长路径、序号位宽、日期缺失回退。
- 时间：复制后 mtime 一致。
- manifest：无绝对路径；非零 `viewRotation`、标签/相册与导出文件名一致；写入失败不得留下正式目录。
- 任务态：WebView 重载快照恢复；停止→立即重启的旧轮收尾不覆盖新轮；A/B 互斥。

### 手动 GUI（不自动化）

- 文件夹选择器、预检警告、进度与取消、刷新后进度恢复、完成后打开目录。
- 10 万+ 项 SelectAll、离线盘、FAT/exFAT/网络盘、库内目标确认。

## 6. 工作量（复审估算，未实测）

- 后端 2.5～3 天；前端 1.5～2 天；测试与真机 1～1.5 天；合计约 **5～6.5 天**。
- 原稿 3.5～4.5 天未计入 staging 整体提交、进度恢复、V20/V21 契约和跨文件 job 互斥，已作废。

## 7. 待裁决

| # | 问题 | 复审建议 |
|---|---|---|
| A-1 | manifest 默认开还是关 | 默认关；记忆选择，并明确隐私提示 |
| A-2 | 相册导出默认命名档 | `sequence`，保留手排成果 |
| A-3 | zip 单包是否进 v1 | 否；独立目录 + 原子整目录提交 |
| A-4 | 全量进 free | 是 |
| A-5 | 非零 `view_rotation` 怎么处理 | v1 原字节复制 + manifest/预检提示；烤入像素进 P2 |
| A-6 | 是否保留 overwrite | 否；v1 只允许 rename/skip，杜绝覆盖用户既有文件 |

## 8. P2 池

- zip 单包、保留目录树、CSV、SHA-256 清单、缩边/格式转换、烤入 `view_rotation`、导出预设、流式 SelectAll 游标（免后端物化全部 ID）。
