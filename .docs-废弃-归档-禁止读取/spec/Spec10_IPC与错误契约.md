---
id: 2026-07-24-Spec10_IPC与错误契约
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec10-IPC 与错误契约

> 一句话:本篇讲 Tauri v2 native IPC 的命令注册/派发/错误传播全链路,服务「新增或排查任一 IPC 命令」的工程师与低能力编码代理;每条命令的业务语义不在本篇,查各子系统篇。

## 1. 概览

Scrollery 前端(Vue 3)与后端(Rust/Tauri v2)之间**唯一**的通信通道是 Tauri native IPC(`invoke`)。本篇覆盖:命令如何注册与派发、错误如何从 Rust 结构体变成前端可分流的对象、前端如何强制走统一入口、以及权限(capabilities)声明。命令的**业务逻辑**(参数含义、算法)分散在 27 个 `*_commands.rs` 文件里,归属各自子系统篇,本篇只给命令清单 + 契约。

- 命令定义目录:`src-tauri/src/ipc/`(27 个 `*_commands.rs` + `registry.rs` + `blocking.rs` + `mod.rs` + `model_download.rs` + `reveal.rs`)
- 命令注册:`src-tauri/src/ipc/registry.rs`(`handler()` 函数,内含 `tauri::generate_handler!` 宏调用)
- 命令挂载:`src-tauri/src/lib.rs:915` — `.invoke_handler(ipc::registry::handler())`
- 统一错误类型:`src-tauri/src/error.rs` — `AppError` 枚举 + 自定义 `Serialize` 实现
- 前端统一入口:`src/utils/ipc.ts` — `invokeIpc` / `invokeIpcRaw`
- 命令名常量:`src/constants/ipc.ts` — `IPC` 常量对象(单一事实源,前端全仓禁裸字符串命令名)
- 权限声明:`src-tauri/capabilities/{default,logs}.json`

**整机位置**(单次命令调用的路径):

```
前端 Vue 组件 / Pinia store / composable
    │  调用 invokeIpc(IPC.XXX, args)             ← src/utils/ipc.ts:86
    ▼
Tauri IPC 传输层(JSON 序列化,或 invokeIpcRaw 走 raw body)
    ▼
后端 tauri::generate_handler! 派发                ← src-tauri/src/ipc/registry.rs:19-305
    ▼
对应 #[tauri::command] async fn(在某个 *_commands.rs 里)
    │  DB 访问经 read_blocking / write_blocking 下沉 spawn_blocking  ← src-tauri/src/ipc/blocking.rs:17,32
    ▼
Result<T, AppError>
    │  成功 → T 经 Tauri 序列化直接回传
    │  失败 → AppError 经自定义 Serialize 变成 {"code","message",...}  ← src-tauri/src/error.rs:346-474
    ▼
前端 invokeIpc 捕获 reject → parseAppError()       ← src/utils/ipc.ts:58-66
    ▼
IpcError { code, message } 抛给调用方,按 code 分流(而非匹配 message 文案)
```

## 2. 数据模型

### 2.1 后端:`AppError`(`src-tauri/src/error.rs:10-211`)

`#[derive(Debug, Error)]`(thiserror)的单一枚举,**全应用共用一个错误类型**——没有按子系统拆分的 Error 类型,子系统差异靠枚举内的「结构化变体」承载(见 §4)。

```rust
pub type Result<T> = std::result::Result<T, AppError>;   // error.rs:478
```

所有 IPC 命令的返回类型统一是 `Result<T>`(即 `Result<T, AppError>`),`?` 可直接从 `std::io::Error`、`rusqlite::Error`、`r2d2::Error`、`exif::Error`、`quick_xml::Error`、`image::ImageError` 等经 `#[from]` 自动转换传播。

### 2.2 前端:`IpcError` 类与 `AppErrorCode`(`src/utils/ipc.ts:16-50`)

```typescript
export type IpcCommand = (typeof IPC)[keyof typeof IPC]   // ipc.ts:17,常量强制的类型基础

export type AppErrorCode =                                  // ipc.ts:23-40,非穷尽联合类型
  | 'Io' | 'Db' | 'Pool' | 'UnsupportedFormat' | 'PathResolution'
  | 'LayoutNotReady' | 'ViewStale' | 'ScanRootNotFound' | 'MediaNotFound'
  | 'Cancelled' | 'Ai' | 'AiModelNotLoaded' | 'System' | 'Internal'
  | 'VolumeOffline' | 'Unknown' | (string & {})   // 开放尾:exotic 等自定义 code 不丢字面量补全

export class IpcError extends Error {
  readonly code: AppErrorCode
  constructor(code: AppErrorCode, message: string) { ... }
}
```

`AppErrorCode` 只列前端常按类型分流的常用码,**不穷尽**——结构化变体(Exotic/Backup/...)的 code 是运行期字符串,靠 `(string & {})` 开放尾兜住,IDE 补全不因此失效。

### 2.3 命令名与事件名枚举:`IPC` / `EVENTS` 常量(`src/constants/ipc.ts:3-436`)

该文件导出两个常量对象,key 均为 `SCREAMING_SNAKE_CASE`、value 为后端 `snake_case` 字符串:`IPC`(命令名,`src/constants/ipc.ts:3-403`)与 `EVENTS`(前端监听的后端事件名,`ipc.ts:406-436`)。IPC 对象(命令名)232 个 key + EVENTS 对象(事件名)14 个 key = 整文件 246 个 key;IPC 对象 232 个与 `registry.rs` 现注册 232 条逐条对应(按 `IPC` 对象切片统计,不用整文件 GREP 计数),事件名与命令名分属不同对象、不同命名空间,不共表(先前「一张表混装、待核实哪几个是事件」的说法据此作废)。

`invokeIpc`/`invokeIpcRaw` 的形参类型是 `IpcCommand = (typeof IPC)[keyof typeof IPC]`——**编译期**只接受该常量表里的字符串字面量,任何裸字符串命令名或改名后忘同步都会在 `npm run build`/`vue-tsc` 报错,不会等到运行期才发现命令不存在。

## 3. 关键流程与算法

### 3.1 命令注册与派发机制

**步骤**(以新增 `list_backends` 为例反推通用流程):
1. 在某个 `src-tauri/src/ipc/<module>_commands.rs` 里写 `#[tauri::command] pub async fn xxx(...) -> Result<T>`。
2. 在 `src-tauri/src/ipc/registry.rs:19-305` 的 `handler()` 函数体内的 `tauri::generate_handler![...]` 列表里加一行 `ipc::<module>_commands::xxx,`。
3. `generate_handler!` 宏(tauri-macros)在**编译期**展开为一个无捕获 `move` 闭包,签名 `Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static`(`registry.rs:19` 处 doc 注释已钉定,Runtime 收敛为具体 `Wry` 而非泛型 `R`,因为部分命令签名直接收 `AppHandle<Wry>` 不满足泛型 `CommandArg<R>` 约束)。
4. `src-tauri/src/lib.rs:915` 处 `Builder::default().invoke_handler(ipc::registry::handler())` 把该闭包接入 Tauri 的 IPC 派发链路。
5. 命令名由**路径最后一段**决定(如 `ipc::scan_commands::add_scan_root` → 前端可调用的命令名是 `add_scan_root`),与文件名/模块名无关,只与函数名有关。
6. 前端在 `src/constants/ipc.ts` 补一个 `IPC.XXX = 'xxx'` 常量项(否则 `invokeIpc` 编译不过)。

**回码核实**:`src-tauri/src/ipc/registry.rs` 的 `generate_handler!` 列表实际含 **232** 条命令(P13 旧去重链 11 条 + P14 闲置命令消融后)(`grep -oE 'ipc::[a-z_]+::[a-z_]+' registry.rs | sort -u | wc -l` = 232),分布在 27 个 `*_commands.rs` 模块(见 §5 命令索引)。

### 3.2 错误传播:AppError → 序列化 → 前端分流

```rust
// error.rs:346-417,impl Serialize for AppError 核心逻辑(伪码)
match self {
    AppError::Io(_) => ("Io", "文件读写异常 | IO error"),
    AppError::UnsupportedFormat(m) => ("UnsupportedFormat", m.as_str()),
    // ... 简单变体:code = 变体名,message = 固定中英双语文案或携带的字符串参数
    AppError::Exotic { code, message } => (*code, message.as_str()),
    AppError::Backup { code, message } => (*code, message.as_str()),
    // ... 结构化变体:code = 该变体自带的运行期稳定字符串(原样透出,不是变体名)
};
// 序列化为二字段 JSON:{"code": code, "message": msg}
```

- 简单变体(如 `Io`/`Db`/`Internal`):IPC `code` **就是变体名**(编译期固定)。
- 结构化变体(如 `Exotic { code, message }`):IPC `code` **是变体内部携带的运行期字符串**,由各命令按场景赋值(如 `"backup_dir_unset"`),变体名(`"Backup"`)本身**不会**出现在 IPC code 里。这是本契约里最容易踩的坑——前端/新代码若按变体名匹配结构化变体的 code 必然落空。
- **日志去重**(`error.rs:241-332,419-467`):非预期错误(排除 `LayoutNotReady`/`Cancelled`/`ViewStale`/`VolumeOffline`——这四种是「预期可恢复」分支,不计入去重表也不记错误日志)按 `(module_path!(), code)` 签名去重,30 秒窗口内:首见 `EmitFirstSeen`(`repeat_count=1`)→ 窗口内重复 `Swallow`(降级为 debug 留证,不占 error 档)→ 窗口过期后再现 `EmitAfterSuppression(N)`。`clear_logs` 命令会重置该去重表(`reset_error_log_dedup`,`error.rs:298`)。
- **错误链收集**(`error_chain_strings`,`error.rs:334-344`):沿 `std::error::Error::source()` 逐层收集展示文本,序列化成 JSON 数组字符串(而非 `{:?}` 的 Debug 多行文本),避免类 anyhow 的 Caused-by 撕裂按行解析的日志管道。

前端侧:

```typescript
// ipc.ts:58-66,parseAppError 分流逻辑
if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
  return new IpcError(String(o.code), String(o.message))   // 结构化 {code,message} → 原样取
}
if (typeof e === 'string') return new IpcError('Unknown', e)   // 裸字符串旧命令 → 兜底 Unknown
if (e instanceof Error) return new IpcError('Unknown', e.message)
return new IpcError('Unknown', String(e))
```

### 3.3 前端调用契约:invokeIpc / invokeIpcRaw

```typescript
// ipc.ts:86-93
export async function invokeIpc<T>(cmd: IpcCommand, args?: Record<string, unknown>): Promise<T> {
  if (isUiHarness) return invokeHarness<T>(cmd, args)   // UI 测试沙盒分支,不经真实 IPC
  try { return await invoke<T>(cmd, args) }
  catch (e) { throw parseAppError(e) }
}
```

- `IpcCommand` 类型强制杜绝裸字符串命令名(编译期)。
- `args` 用 camelCase 键,Tauri 自动转 snake_case 匹配 Rust 参数名。
- `invokeIpcRaw`(`ipc.ts:101-112`)用于大二进制负载命令(如 `store_doc_thumbnail`):body 走 `Uint8Array` 直传而非 JSON 数字数组(每字节膨胀约 4 字符),元数据经自定义 header 传递,后端经 `request.headers()` 读取。
- `ipcErrorMessage`(`ipc.ts:115-118`)是 toast 等展示场景的兜底文案提取函数,优先取 `IpcError`/`Error` 的 `message`。
- `generateOperationId`(`ipc.ts:73-78`)生成贯穿「前端发起 → command → spawn_blocking/后台流水线」的关联 id,用于扫描/AI 等长任务的 tracing span(span 不跨 `spawn_blocking`,故需显式字符串传参而非依赖 tracing 上下文)。

### 3.4 并发/线程模型

- 命令函数体本身跑在 tokio 异步 worker 上(`async fn`)。
- **DB 访问必须下沉**:`src-tauri/src/ipc/blocking.rs:17-49` 提供 `read_blocking`/`write_blocking` 两个共享助手,把闭包内的 rusqlite 调用经 `tokio::task::spawn_blocking` 移出 tokio worker;`write_blocking` 额外处理 `db_writer` 互斥锁中毒恢复(`unwrap_or_else(|e| e.into_inner())`,SQLite 事务原子、panic 会回滚,恢复出的连接仍可用)。
- **回归门**:`blocking.rs:78` 的 `ipc_commands_keep_rusqlite_off_async_workers` 测试逐行扫描 `src/ipc/*`、`state.rs`、`scanner/volume_watch.rs`、`lib.rs`、`exotic/coordinator.rs`,断言任何 `db_writer.lock`/`db_read_pool.get` 调用点的「最近上游标记」必须是 `spawn_blocking`/`read_blocking(`/`write_blocking(`/`thread::spawn` 之一,不能是裸 `async fn`/`spawn(async` 正文——这是 AGENTS.md 硬约束「DB 命令走 spawn_blocking」的机器可执行版本。

## 4. 契约与不变量(施工红线)

| 不变量 | 为什么(违反会怎样) |
|---|---|
| 命令名走 `IPC` 常量,前端禁裸字符串 | 改后端命令名而前端忘改,裸字符串在运行期才报「命令不存在」;走常量则编译期报错(`src/constants/ipc.ts`) |
| 新增命令只碰 `ipc/*_commands.rs` + `registry.rs`,不碰 `lib.rs` | `registry.rs` 从 `lib.rs` 下沉(2026-07-16,U-P1-a)专为让新增命令的高频改动脱离 `lib.rs` 的其他初始化逻辑,减少合并冲突面(`registry.rs:1-6` 注释) |
| 结构化变体的 IPC `code` 是变体**携带**的运行期字符串,不是变体名 | 前端/新代码若按变体名(如 `"Backup"`)匹配会永远匹配不到,必须按各变体文档化的稳定码集(见 §6) |
| 结构化变体的 `message` 不携带绝对路径/SQL/内部错误串 | 泄漏面——`message` 会原样透到前端并可能进日志/截图,`error.rs` 各结构化变体的 doc 注释逐条钉定此约束(Preview/Relink/Backup/Restore/Export/Edit/Config/Player/Color/Ocr 全部适用) |
| IPC 命令内的任意 rusqlite 调用必须经 `spawn_blocking`(`read_blocking`/`write_blocking`或自建块) | 同步 SQLite 调用直跑在 tokio worker 上会阻塞该 worker 上排队的其他并发 IPC 调用;`blocking.rs:78` 测试机器强制 |
| `AppError::LayoutNotReady`/`Cancelled`/`ViewStale`/`VolumeOffline` 不计入错误日志与去重表 | 这四种是「预期可恢复」分支(重算布局重试/用户主动取消/视图漂移重发/卷重连即恢复),记错误日志会造成噪音;`error.rs:419-426` 显式排除 |
| 新增 Tauri 原生插件权限或暴露命令需在 `src-tauri/capabilities/*.json` 声明 windows + permissions | Tauri v2 ACL 默认拒绝未声明权限,运行期报权限错误而非编译期(见 §7);细则见 [Spec14 不变量与约定](./Spec14_不变量与约定.md) |
| 本地资源经 scoped `assetProtocol` + `convertFileSrc`,不裸传文件路径给前端 `<img src>` | CSP/权限红线,详见 [Spec14](./Spec14_不变量与约定.md) |

## 5. 命令索引(表内 210 条,按模块分组)

> 覆盖边界:本表是**索引**而非完整注册清单——表内 `file:line` 为编写时点的历史定位,当前源码行号已漂移、不保证精确定位;完整注册集合以 `src-tauri/src/ipc/registry.rs` 现注册 **232** 条为唯一权威(`IPC` 常量对象同为 232 条,见 §2.3)。P13/P14 已删除的接口不在此表补录。
> 每行:命令名 — `文件:行`(函数定义处) — 一句话。业务语义深挖见对应子系统篇链接(模块标题旁)。

### 5.1 scan_commands.rs(11 条)— 扫描根与目录索引 → [Spec02 扫描与画廊](./Spec02_扫描与画廊.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| add_scan_root | scan_commands.rs:25 | 新增扫描根目录 |
| remove_scan_root | scan_commands.rs:120 | 移除扫描根 |
| set_scan_root_hidden | scan_commands.rs:143 | 设置扫描根显隐(V21,库级排除) |
| remove_scan_root_with_options | scan_commands.rs:181 | 移除扫描根并可选清理缩略图 |
| relink_scan_root | scan_commands.rs:321 | 文件夹迁移后改根路径,免重扫(#7 方案A) |
| check_folder_overlap | scan_commands.rs:479 | 检测新增扫描根与既有根的路径冲突 |
| list_scan_roots | scan_commands.rs:533 | 枚举所有扫描根 |
| start_scan | scan_commands.rs:549 | 启动全量扫描 |
| stop_scan | scan_commands.rs:725 | 停止扫描任务 |
| clear_database | scan_commands.rs:736 | 清空媒体库数据库 |
| clear_settings | scan_commands.rs:801 | 清空应用设置 |

### 5.2 layout_commands.rs(6 条)+ hgallery_commands.rs(2 条)— 布局计算 → [Spec02](./Spec02_扫描与画廊.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| compute_layout | layout_commands.rs:160 | 计算全库布局(行/高度映射) |
| get_view_ids | layout_commands.rs:517 | 按布局序返回视图全集 id(Part5 选区前置) |
| get_layout_rows_by_y | layout_commands.rs:555 | 按视口 Y 坐标范围获取行(视口相交) |
| get_bucket_rows | layout_commands.rs:576 | 按段精确取行(T16 方案B) |
| get_item_y_by_id | layout_commands.rs:608 | 获取单项 Y 坐标(定位) |
| get_subtree_scroll_target | layout_commands.rs:634 | 获取子树滚动目标 |
| compute_h_layout | hgallery_commands.rs:44 | 计算横向画廊 H-Lab 布局(独立缓存) |
| get_h_blocks_by_x | hgallery_commands.rs:109 | 按 X 坐标范围获取横向块 |

### 5.3 media_commands.rs(23 条)— 媒体详情/元数据/批量操作 → [Spec02](./Spec02_扫描与画廊.md) / [Spec05 视频与音频](./Spec05_视频与音频.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| prioritize_dimensions | media_commands.rs:27 | 优先级尺寸计算 |
| get_media_detail | media_commands.rs:100 | 获取单项媒体详情 |
| get_meta_for_viewport | media_commands.rs:110 | 获取视口内媒体元数据(缩略图、尺寸等) |
| get_adjacent_media | media_commands.rs:122 | 获取前/后一项媒体 |
| get_companion_video_url | media_commands.rs:141 | 获取伴随视频 URL(如关键帧精灵配套视频) |
| get_keyframe_sprite | media_commands.rs:217 | 获取视频关键帧精灵(时间轴缩略图) |
| toggle_favorite | media_commands.rs:245 | 单项收藏切换 |
| batch_toggle_favorite | media_commands.rs:293 | 批量收藏切换(选区) |
| set_rating | media_commands.rs:326 | 单项评分(0-5) |
| batch_set_rating | media_commands.rs:345 | 批量评分(选区快捷键用) |
| set_color_label | media_commands.rs:376 | 单项颜色标签(0-7,T16) |
| set_view_rotation | media_commands.rs:396 | 看图台显示旋转(0/90/180/270) |
| set_playback_position | media_commands.rs:415 | 播放器播放位置记忆(ms,V23) |
| batch_set_color_label | media_commands.rs:457 | 批量颜色标签 |
| soft_delete_items | media_commands.rs:486 | 软删(放入回收站) |
| restore_items | media_commands.rs:509 | 从回收站恢复 |
| get_stats | media_commands.rs:581 | 获取库统计(分类、尺寸等) |
| list_registered_formats | media_commands.rs:598 | 列出已注册格式(内置 ∪ exotic Catalog) |
| list_library_formats | media_commands.rs:611 | 列出库内实际存在的格式(DISTINCT) |
| get_directory_tree | media_commands.rs:618 | 获取目录树(按分组) |
| get_directory_children | media_commands.rs:628 | 获取目录子项 |
| get_directory_ancestors | media_commands.rs:636 | 由目标项 id 反查祖先链(定位) |
| list_directory_files | media_commands.rs:644 | 列出目录内文件 |

### 5.4 audio_commands.rs(1 条)→ [Spec05](./Spec05_视频与音频.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_audio_detail | audio_commands.rs:26 | 获取音频元数据(持续时间、采样率等) |

### 5.5 player_commands.rs(2 条)→ [Spec05](./Spec05_视频与音频.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| load_video_subtitle | player_commands.rs:66 | 加载视频字幕 |
| save_frame_png | player_commands.rs:189 | 截帧保存为 PNG |

### 5.6 viewer_color_commands.rs(4 条)— 查看器渲染色域 → [Spec04 图像与色彩管线](./Spec04_图像与色彩管线.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_viewer_color_url | viewer_color_commands.rs:46 | 获取查看器色域 URL(sRGB/P3/DCI-P3/自定义 ICC) |
| import_icc_profile | viewer_color_commands.rs:146 | 导入自定义 ICC profile |
| list_icc_profiles | viewer_color_commands.rs:161 | 枚举自定义 ICC profiles |
| delete_icc_profile | viewer_color_commands.rs:173 | 删除自定义 ICC profile |

### 5.7 thumbnail_commands.rs(7 条)→ [Spec03 缩略图与派生](./Spec03_缩略图与派生.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| batch_request_thumbnails | thumbnail_commands.rs:63 | 批量请求缩略图 |
| full_thumb_gen_status | thumbnail_commands.rs:499 | 获取全量生成进度 |
| start_full_thumbnail_generation | thumbnail_commands.rs:520 | 启动全量缩略图生成 |
| start_incremental_thumbnail_generation | thumbnail_commands.rs:530 | 启动增量缩略图生成 |
| stop_full_thumbnail_generation | thumbnail_commands.rs:962 | 停止全量生成 |
| regenerate_missing_thumb | thumbnail_commands.rs:989 | 重生成缺失缩略图 |
| clear_all_thumbnails | thumbnail_commands.rs:1056 | 清空所有缩略图缓存 |

### 5.8 backup_commands.rs(8 条)— 数据备份(方案 B)→ [Spec08 存储备份导出文件操作](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| backup_status | backup_commands.rs:81 | 获取备份进度 |
| preflight_backup | backup_commands.rs:204 | 备份预检查(目的地校验) |
| start_backup | backup_commands.rs:235 | 启动备份任务 |
| stop_backup | backup_commands.rs:464 | 停止备份 |
| list_backups | backup_commands.rs:471 | 列出备份包 |
| restore_stage | backup_commands.rs:539 | 恢复预检查(schema 校验) |
| restore_arm | backup_commands.rs:570 | 执行恢复(pre-restore 回滚) |
| relaunch_app | backup_commands.rs:640 | 恢复完成后重启应用 |

### 5.9 export_commands.rs(4 条)— 导出整理成果(方案 A)→ [Spec08](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| export_status | export_commands.rs:114 | 获取导出进度 |
| preflight_export | export_commands.rs:190 | 导出预检查(目的地校验) |
| start_export | export_commands.rs:265 | 启动导出任务 |
| stop_export | export_commands.rs:442 | 停止导出 |

### 5.10 edit_commands.rs(4 条)— 图片简单编辑(方案 C)→ [Spec04](./Spec04_图像与色彩管线.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_editing_entitlement | edit_commands.rs:73 | 查询编辑功能授权状态 |
| activate_editing_feature | edit_commands.rs:82 | 激活编辑功能 |
| get_edit_preview | edit_commands.rs:127 | 获取编辑预览(裁剪、旋转、调色) |
| save_edited_image | edit_commands.rs:151 | 保存编辑结果 |

### 5.11 exotic_commands.rs(18 条)— 冷门格式插件系统 → [Spec09 插件平台与exotic](./Spec09_插件平台与exotic.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_exotic_format_resolutions | exotic_commands.rs:20 | 列出冷门格式分辨率 |
| get_exotic_item_state | exotic_commands.rs:57 | 获取冷门格式项处理状态 |
| list_installed_exotic_plugins | exotic_commands.rs:91 | 列出已安装冷门格式插件 |
| get_plugin_entitlement | exotic_commands.rs:100 | 查询插件授权(试用/激活) |
| start_exotic_processing | exotic_commands.rs:157 | 启动冷门格式处理 |
| pause_exotic_processing | exotic_commands.rs:169 | 暂停处理 |
| stop_exotic_processing | exotic_commands.rs:178 | 停止处理 |
| get_exotic_processing_status | exotic_commands.rs:201 | 获取处理进度 |
| list_exotic_task_details | exotic_commands.rs:251 | 列出处理任务详情 |
| retry_exotic_plugin_failures | exotic_commands.rs:322 | 重试插件所有失败 |
| activate_exotic_plugin | exotic_commands.rs:346 | 激活插件授权 |
| deactivate_exotic_plugin | exotic_commands.rs:385 | 禁用插件授权 |
| fetch_exotic_registry | exotic_commands.rs:476 | 拉取插件 Registry(含新版本) |
| list_exotic_registry | exotic_commands.rs:510 | 列出 Registry 条目 |
| install_exotic_plugin | exotic_commands.rs:543(`channel-direct` 关闭时的桩,返回 `channel_unsupported`)/ 556(`channel-direct` 开启时的真实实现) | 安装插件——同名命令按 `#[cfg(feature = "channel-direct")]` 二选一编译(Store 渠道禁「下载-执行」,Part7-T11) |
| repair_exotic_plugin | exotic_commands.rs:689 | 修复/重装插件 |
| rollback_exotic_plugin | exotic_commands.rs:743 | 回滚插件版本 |
| uninstall_exotic_plugin | exotic_commands.rs:823 | 卸载插件 |

### 5.12 volume_commands.rs(3 条)— 卷管理(T13 离线 UX)→ [Spec08](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_volumes | volume_commands.rs:43 | 列出已知存储卷 |
| rename_volume | volume_commands.rs:62 | 重命名卷标签 |
| forget_volume | volume_commands.rs:85 | 遗忘卷(从卷表删除) |

### 5.13 search_commands.rs(0 条,接口已撤销)→ [Spec02](./Spec02_扫描与画廊.md)

`search_media` 全文搜索 IPC 已随 P14 闲置面消融撤销:不再注册,也不保留兼容壳,`src/constants/ipc.ts` 的 `SEARCH_MEDIA` 同步移除。
**当前搜索走主画廊视图查询**——前端把搜索条件编码进画廊视图描述符,与全库/文件夹/收藏/回收站/重复镜头共用同一条
`compute_layout` → `get_bucket_rows` 取数链(见 [Spec02](./Spec02_扫描与画廊.md) §画廊);不再有独立的搜索结果列表接口与页面入口。
### 5.14 config_commands.rs(8 条)— 配置与设置 → [Spec12 配置状态日志](./Spec12_配置状态日志.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_app_config | config_commands.rs:31 | 获取应用配置(JSON) |
| get_startup_config | config_commands.rs:134 | 获取启动配置(lite 专用) |
| set_app_config | config_commands.rs:274 | 更新配置设置 |
| get_thumb_cache_dir | config_commands.rs:493 | 获取缩略图缓存目录路径 |
| get_log_dir | config_commands.rs:501 | 获取日志目录路径 |
| get_cache_stats | config_commands.rs:510 | 获取缓存统计(大小、文件数) |
| get_config_status | config_commands.rs:599 | 获取 config.toml 编辑状态 |
| open_config_file | config_commands.rs:627 | 用外部编辑器打开 config.toml(A2) |

### 5.15 log_commands.rs(6 条)→ [Spec12](./Spec12_配置状态日志.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| open_log_window | log_commands.rs:21 | 打开独立日志窗口(S4) |
| list_log_files | log_commands.rs:59 | 列出日志文件 |
| read_log_file_page | log_commands.rs:171 | 分页读取日志 |
| get_log_diagnostics | log_commands.rs:211 | 获取诊断数据(错误统计、热点等,S5) |
| compute_log_histogram | log_commands.rs:313 | 计算日志时间分布直方图 |
| export_diagnostics_package | log_commands.rs:349 | 导出诊断包(脱敏) |

### 5.16 system_commands.rs(8 条)→ [Spec12](./Spec12_配置状态日志.md) / [Spec14](./Spec14_不变量与约定.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| show_in_explorer | system_commands.rs:31 | 在文件管理器中显示(reveal) |
| open_directory | system_commands.rs:67 | 打开文件夹(显示内容) |
| clear_logs | system_commands.rs:155 | 清空日志文件(同时重置错误去重表) |
| log_frontend_events | system_commands.rs:238 | 记录前端事件到后端日志 |
| exit_app | system_commands.rs:248 | 退出应用 |
| hide_window | system_commands.rs:258 | 隐藏窗口 |
| set_as_wallpaper | system_commands.rs:267 | 设置壁纸 |
| copy_image_to_clipboard | system_commands.rs:287 | 复制图像到剪贴板 |

### 5.17 tree_commands.rs(4 条)— 文件树受限 FS 访问 → [Spec08](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_tree_entries | tree_commands.rs:126 | 列出目录树条目 |
| invalidate_tree_cache | tree_commands.rs:258 | 失效目录树缓存 |
| reveal_tree_entry | tree_commands.rs:331 | 在文件管理器中显示树条目 |
| get_tree_text_preview | tree_commands.rs:385 | 获取文本文件预览(path-based 只读) |

### 5.18 ai_commands.rs(13 条)— AI 推理与语义搜索 → [Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| detect_ai_provider | ai_commands.rs:69 | 检测可用 AI 后端(ONNX/ollama 等) |
| get_ai_status | ai_commands.rs:98 | 获取 AI 推理状态 |
| semantic_search_cmd | ai_commands.rs:190 | 语义搜索(嵌入向量匹配) |
| start_ai_analysis | ai_commands.rs:247 | 启动 AI 分析(打标签) |
| restart_ai_analysis | ai_commands.rs:295 | 重启 AI 分析 |
| pause_ai_analysis | ai_commands.rs:347 | 暂停 AI 分析 |
| stop_ai_analysis | ai_commands.rs:372 | 停止 AI 分析 |
| reload_ai_engine | ai_commands.rs:449 | 重载 AI 引擎 |
| retry_failed_ai_items | ai_commands.rs:472 | 重试失败项 |
| rebuild_embeddings | ai_commands.rs:488 | 重建所有嵌入向量 |
| list_model_registry | ai_commands.rs:548 | 列出模型 Registry(可下载) |
| set_active_model | ai_commands.rs:673 | 切换活跃 AI 模型 |
| download_model | ai_commands.rs:752 | 下载 AI 模型 |

### 5.19 ocr_commands.rs(4 条)— OCR 文字提取(B′ 路线,T7)→ [Spec06](./Spec06_AI人脸OCR.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| ocr_status | ocr_commands.rs:220 | 获取 OCR 模块状态 |
| ocr_extract_image | ocr_commands.rs:254 | 从图像提取文字 |
| ocr_extract_frame | ocr_commands.rs:289 | 从视频帧提取文字 |
| download_ocr_models | ocr_commands.rs:392 | 下载 OCR 模型 |

### 5.20 face_commands.rs(22 条)— 人脸识别(F5)与批量审批 → [Spec06](./Spec06_AI人脸OCR.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_face_status | face_commands.rs:126 | 获取人脸识别状态 |
| start_face_analysis | face_commands.rs:208 | 启动人脸分析 |
| retry_failed_face_items | face_commands.rs:254 | 重试失败项 |
| restart_face_analysis | face_commands.rs:274 | 重启人脸分析 |
| pause_face_analysis | face_commands.rs:326 | 暂停人脸分析 |
| stop_face_analysis | face_commands.rs:341 | 停止人脸分析 |
| list_face_persons | face_commands.rs:360 | 列出识别到的人物(聚类) |
| list_ignored_face_persons | face_commands.rs:376 | 列出忽略的人物 |
| get_item_faces | face_commands.rs:393 | 获取单项中的人脸(BBOX+特征) |
| rename_face_person | face_commands.rs:408 | 重命名人物 |
| set_face_person_hidden | face_commands.rs:419 | 隐藏人物 |
| set_face_person_ignored | face_commands.rs:431 | 忽略人物 |
| merge_face_persons | face_commands.rs:442 | 合并多个人物(聚类调整) |
| recluster_faces | face_commands.rs:462 | 重新聚类所有人脸 |
| confirm_faces | face_commands.rs:504 | 批量确认人脸(Part4 T3) |
| reassign_faces | face_commands.rs:512 | 批量分配人脸到指定人物 |
| unassign_faces | face_commands.rs:529 | 批量取消人脸分配 |
| reject_faces | face_commands.rs:540 | 批量拒绝人脸(不涉人物) |
| create_person | face_commands.rs:558 | 创建新人物记录 |
| list_likely_face_matches | face_commands.rs:577 | 列出人脸可能匹配 |
| list_face_model_registry | face_commands.rs:608 | 列出人脸模型 Registry |
| download_face_model | face_commands.rs:726 | 下载人脸模型 |

### 5.21 derive_commands.rs(3 条)— 派生流水线 → [Spec03](./Spec03_缩略图与派生.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| start_derivation | derive_commands.rs:76 | 启动派生(kinds:video_keyframes/doc_thumb/audio_meta) |
| stop_derivation | derive_commands.rs:153 | 停止派生 |
| derivation_status | derive_commands.rs:172 | 获取派生进度 |

### 5.22 doc_commands.rs(25 条)— 文档处理与阅读器 → [Spec07 文档与阅读器](./Spec07_文档与阅读器.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_replacements | doc_commands.rs:48 | 列出文本替换规则 |
| get_effective_replacements | doc_commands.rs:62 | 获取生效的替换规则 |
| upsert_replacement | doc_commands.rs:72 | 新增或更新替换规则 |
| delete_replacement | doc_commands.rs:95 | 删除替换规则 |
| list_versions | doc_commands.rs:140 | 列出文档版本 |
| get_current_version | doc_commands.rs:150 | 获取当前活跃版本 |
| get_document_text | doc_commands.rs:161 | 获取文档全文 |
| save_version | doc_commands.rs:239 | 保存新版本 |
| set_current_version | doc_commands.rs:325 | 切换活跃版本 |
| delete_version | doc_commands.rs:339 | 删除版本 |
| diff_versions | doc_commands.rs:365 | 比较两版本差异 |
| diff_texts | doc_commands.rs:400 | 比较两文本差异 |
| get_reading_progress | doc_commands.rs:424 | 获取阅读进度(页码/百分比) |
| set_reading_progress | doc_commands.rs:434 | 设置阅读进度 |
| get_text_book_index | doc_commands.rs:606 | 获取文本书籍目录(分章) |
| get_text_chapter | doc_commands.rs:638 | 获取章节内容 |
| convert_chinese | doc_commands.rs:681 | 繁简中文转换(ferrous-opencc) |
| get_reader_book_prefs | doc_commands.rs:690 | 获取书籍阅读偏好(字体/字号) |
| set_reader_book_prefs | doc_commands.rs:700 | 设置书籍阅读偏好 |
| list_reader_bookmarks | doc_commands.rs:713 | 列出书签 |
| add_reader_bookmark | doc_commands.rs:723 | 添加书签 |
| delete_reader_bookmark | doc_commands.rs:738 | 删除书签 |
| ensure_doc_thumb_queue | doc_commands.rs:751 | 确保文档缩略图队列就位 |
| list_pending_doc_thumbs | doc_commands.rs:766 | 列出待生成缩略图 |
| store_doc_thumbnail | doc_commands.rs:801 | 存储文档缩略图(PNG 字节,走 `invokeIpcRaw`) |

### 5.23 proofread_commands.rs(5 条)— 文档 AI 校对(P4 §5.4)→ [Spec07](./Spec07_文档与阅读器.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| get_proofread_config | proofread_commands.rs:43 | 获取校对配置(后端选择) |
| set_proofread_config | proofread_commands.rs:69 | 设置校对配置 |
| set_proofread_key | proofread_commands.rs:87 | 设置 API 密钥 |
| clear_proofread_key | proofread_commands.rs:96 | 清除 API 密钥 |
| proofread_chunk | proofread_commands.rs:109 | 校对文本段 |

### 5.24 collection_commands.rs(9 条)— 收藏夹(需求 7)→ [Spec02](./Spec02_扫描与画廊.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_collections | collection_commands.rs:24 | 列出所有收藏夹 |
| list_deleted_collections | collection_commands.rs:32 | 列出已删除收藏夹 |
| recent_collections | collection_commands.rs:39 | 列出最近收藏夹 |
| create_collection | collection_commands.rs:52 | 创建新收藏夹 |
| delete_collection | collection_commands.rs:67 | 删除收藏夹 |
| restore_collection | collection_commands.rs:77 | 从已删除恢复收藏夹 |
| rename_collection | collection_commands.rs:87 | 重命名收藏夹 |
| add_to_collection | collection_commands.rs:98 | 将项添加到收藏夹 |
| remove_from_collection | collection_commands.rs:115 | 从收藏夹移除项 |

### 5.25 storage_commands.rs(4 条)— 存储后端(网络盘,需求 8)→ [Spec08](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| list_backends | storage_commands.rs:63 | 列出配置的存储后端 |
| test_backend | storage_commands.rs:79 | 测试后端连接 |
| add_backend | storage_commands.rs:94 | 新增存储后端(NAS/云盘) |
| remove_backend | storage_commands.rs:151 | 移除存储后端 |

### 5.26 file_ops_commands.rs(7 条)— 文件操作 → [Spec08](./Spec08_存储备份导出文件操作.md)

| 命令名 | 文件:行 | 一句话 |
|---|---|---|
| create_physical_folder | file_ops_commands.rs:63 | 在库内创建物理文件夹 |
| relocate_media_items | file_ops_commands.rs:269 | 重定位媒体项(迁移源路径) |
| copy_media_items_db | file_ops_commands.rs:376 | 复制媒体项(DB 专用,含衍生) |
| remove_media_items_hard | file_ops_commands.rs:446 | 永久删除项(FS+DB) |
| move_directory | file_ops_commands.rs:671 | 移动文件夹 |
| copy_directory | file_ops_commands.rs:853 | 复制文件夹 |
| delete_directory_to_trash | file_ops_commands.rs:925 | 删除文件夹到回收站 |

> 以上各条 file:line 均逐条 `grep`/`Read` 回验。2026-09-15 复核:`registry.rs` 实际注册 232 条,已移除 P13(旧去重链 11 条)与 P14(闲置命令)删除的接口;搜索/回收站/重复镜头改由主画廊视图查询承担,不为已退场命令保留兼容条目。所有文件路径省略前缀均为 `src-tauri/src/ipc/`。

## 6. 错误码全表:`AppError`(40 个变体,`src-tauri/src/error.rs:10-211`)

**回码核实**:枚举实际共 **40** 个变体(简单变体 28 个 + 结构化变体 12 个),不是常见估算的「约 31」——早期统计漏计了部分简单变体。

### 6.1 简单变体(28 个,`code` = 变体名本身)

| 变体 | IPC code | 适用域 | 序列化示例 |
|---|---|---|---|
| Io | `Io` | 文件读写异常(`#[from] std::io::Error`) | `{"code":"Io","message":"文件读写异常 \| IO error"}` |
| Db | `Db` | 数据库访问异常(`#[from] rusqlite::Error`) | 同上模式 |
| Pool | `Pool` | 连接池异常(`#[from] r2d2::Error`) | 同上 |
| Exif | `Exif` | 照片元数据解析异常(`#[from] exif::Error`) | 同上 |
| Xmp | `Xmp` | XMP 数据解析异常(`#[from] quick_xml::Error`) | 同上 |
| UnsupportedFormat | `UnsupportedFormat` | 不支持的格式(携带格式名) | message = 格式名 |
| Engine | `Engine` | 图像引擎异常(`#[from] image::ImageError`) | 固定文案 |
| PathResolution | `PathResolution` | 路径解析异常(携带字符串) | message = 携带串 |
| FFmpeg | `FFmpeg` | FFmpeg 相关异常 | message = 携带串 |
| AudioMetadata | `AudioMetadata` | 音频元数据异常 | message = 携带串 |
| DocumentRender | `DocumentRender` | 文档渲染异常 | message = 携带串 |
| LayoutNotReady | `LayoutNotReady` | 布局未就绪(需先 `compute_layout`);**不记错误日志** | 固定文案 |
| ViewStale | `ViewStale` | 选区所据布局版本已过期(T18 SelectAll 守门);**不记错误日志**,前端重算布局重发 | 固定文案 |
| ScanRootNotFound | `ScanRootNotFound` | 未找到扫描目录(携带 id) | 固定文案(不透 id) |
| MediaNotFound | `MediaNotFound` | 未找到媒体文件(携带 id) | 固定文案(不透 id) |
| VolumeOffline | `VolumeOffline` | 存储卷离线(T13,携带卷标签);**不记错误日志**,重连自动恢复 | message = 卷标签 |
| Cancelled | `Cancelled` | 操作已取消;**不记错误日志** | 固定文案 |
| Ai | `Ai` | AI 推理异常 | 固定文案 |
| AiModelNotLoaded | `AiModelNotLoaded` | AI 模型未加载(携带字符串) | message = 携带串 |
| System | `System` | 系统异常(携带字符串) | message = 携带串 |
| Os | `Os` | 操作系统异常(携带字符串) | message = 携带串 |
| AiTokenizer | `AiTokenizer` | AI 分词异常(携带字符串) | message = 携带串 |
| Internal | `Internal` | 内部异常(携带字符串) | message = 携带串 |
| CreateFolder | `CreateFolder` | 创建文件夹失败(携带字符串) | message = 携带串 |
| MoveFile | `MoveFile` | 移动文件失败(携带字符串) | message = 携带串 |
| CopyFile | `CopyFile` | 复制文件失败(携带字符串) | message = 携带串 |
| InvalidMove | `InvalidMove` | 非法文件夹移动(携带字符串,8 处调用共用) | message = 携带串 |
| DirectoryExists | `DirectoryExists` | 目标目录已存在(携带字符串) | message = 携带串 |

### 6.2 结构化变体(12 个,`{ code: &'static str, message: String }`,`code` 为运行期动态字符串)

**关键理解**:这 12 个变体的 Rust 变体名(如 `Backup`)**不会**出现在 IPC 层——真正的 IPC `code` 是各命令按场景传入的字符串(如 `"backup_dir_unset"`)。这是本子系统区别于「简单变体=变体名即 code」的核心分流机制,让前端能按**语义**(而非泛化的 `"Backup"`)区分「目的地不可写」和「用户取消」等完全不同的处理路径。

| 变体 | 已知稳定 code 集(`error.rs` doc 注释逐条钉定) | 适用域 |
|---|---|---|
| Exotic | `rollback`(Registry 验签/接受失败,安全警告)、`http`(网络失败,可重试)等,**非穷尽** | 冷门格式插件全流程,→ [Spec09](./Spec09_插件平台与exotic.md) |
| Reveal | `unsupported_platform`(移动端永不支持,前端永久隐藏动作)、`reveal_failed`(通用失败,提示重试) | 在文件管理器中显示 |
| Preview | `preview_unsupported_type`(扩展名不在白名单)、`preview_failed`(读取/显示失败) | 树内文本预览(path-based 只读) |
| Relink | `relink_not_a_dir`(新路径不存在/非目录)、`relink_path_taken`(路径已被别的根占用,UNIQUE 冲突)、`relink_mismatch`(抽样校验不过,非同一目录树) | 扫描根重链接(#7 方案A) |
| Backup | `backup_dir_unset`、`backup_dir_not_writable`、`backup_document_inconsistent`(文档行↔文件不一致)、`backup_cancelled`、`backup_io`、`file_job_busy`(A/B 文件任务占用) | 数据备份(方案 B),→ [Spec08](./Spec08_存储备份导出文件操作.md) |
| Restore | `restore_format_unsupported`、`restore_schema_too_new`、`restore_corrupt`(quick_check/sha256 不符)、`restore_path_invalid`(zip-slip/盘符/符号链接逃逸)、`restore_size_limit`(zip bomb)、`restore_document_missing`、`restore_rollback_failed`、`restore_io` | 数据恢复(方案 B) |
| Export | `export_target_invalid`、`export_target_not_writable`、`export_target_inside_library`(命中扫描根内部)、`export_cancelled`、`export_io`、`file_job_busy`(与 Backup/Edit 共用同一码,前端统一分流「有文件任务在跑」);SelectAll 布局过期**复用全局 `ViewStale`**,不落本变体私有码集 | 导出整理成果(方案 A) |
| Edit | `edit_source_unavailable`、`edit_format_unsupported`(GIF/HEIC/AVIF 等 v1 不承诺)、`edit_decode_failed`、`edit_encode_failed`、`edit_image_too_large`、`edit_crop_empty`、`edit_invalid_ops`、`edit_target_conflict`、`edit_io`、`edit_saved_needs_index`(文件已落盘、单文件 ingest 失败的 partial 恢复态)、`file_job_busy` | 图片简单编辑(方案 C),→ [Spec04](./Spec04_图像与色彩管线.md) |
| Config | `config_invalid_value`(`set_app_config` 值未过 `SettingKind` 校验)、`config_write_failed`(原子写盘失败,也是 `ConfigFileError` 的统一映射目标)、`config_open_failed` | config.toml 读写(A2),→ [Spec12](./Spec12_配置状态日志.md) |
| Player | `player_subtitle_not_found`、`player_subtitle_unsupported_type`、`player_subtitle_too_large`、`player_subtitle_io`、`player_frame_target_invalid`、`player_frame_decode_failed`、`player_frame_io` | 播放器字幕/截帧,→ [Spec05](./Spec05_视频与音频.md) |
| Color | `icc_parse_failed`、`icc_not_rgb`、`icc_not_display_class`、`icc_transform_unsupported`、`icc_too_large`、`icc_io`、`icc_not_found`、`unsupported_platform`、`viewer_render_unsupported`、`viewer_render_decode_failed`、`viewer_render_too_large`、`viewer_render_io` | 查看器渲染色域(B 线),→ [Spec04](./Spec04_图像与色彩管线.md) |
| Ocr | `ocr_unlicensed`、`ocr_license_expired`、`ocr_unavailable`、`ocr_model_missing`、`ocr_manifest_unready`、`ocr_invalid_input`、`ocr_decode_failed`、`ocr_engine_failed`、`ocr_worker_failed` | OCR 文字提取(B′路线),→ [Spec06](./Spec06_AI人脸OCR.md) |

所有结构化变体的 `message` **一致遵守**:不携带绝对路径 / SQL 语句 / 底层异常原始字符串(泄漏面硬约束,`error.rs` 每个变体 doc 注释逐条重申)。

### 6.3 手动 `From` 映射(不产生新变体,归并到既有变体)

- `impl From<scrollery_ai_core::AiError> for AppError`(`error.rs:217-226`):`AiError::Internal` → `AppError::Internal`,`AiError::Tokenizer` → `AppError::AiTokenizer`,其余(含 non_exhaustive 通配臂)→ `AppError::Ai(其 to_string())`。此映射是 Part4-T15/T16「AI 推理核错误收敛」的落地点——理由链见 [Part6 §插件平台与exotic收尾](../refactor_2026/Part6_插件平台与exotic收尾.md)(WorkerErrorCode 等协议错误码设计理由)。
- `impl From<crate::config::ConfigFileError> for AppError`(`error.rs:231-239`):统一映射为 `AppError::Config { code: "config_write_failed", message }`,`message` 取自 `ConfigFileError::to_status_message()`(已按硬约束脱敏)。

## 7. 权限声明(`src-tauri/capabilities/*.json`,共 2 个文件)

仓内仅 2 个已提交的 capability 文件(`src-tauri/gen/schemas/capabilities.json` 与 `target/**/capabilities.json` 均为构建产物,非源文件,不计入)。

### 7.1 `default.json`(`src-tauri/capabilities/default.json:1-24`)

| 字段 | 值 |
|---|---|
| identifier | `default` |
| windows | `["main"]` |
| permissions | `core:default`、`dialog:default`、`dialog:allow-open`、`shell:default`、`shell:allow-open`、`window-state:default`、`os:default`、`core:window:allow-start-dragging`、`core:window:allow-minimize`、`core:window:allow-toggle-maximize`、`core:window:allow-maximize`、`core:window:allow-unmaximize`、`core:window:allow-close`、`core:window:allow-set-theme`、`core:window:allow-set-fullscreen`、`core:window:allow-set-resizable` |

覆盖主窗口(`main`)的窗口操作(拖拽/最小化/最大化/关闭/主题/全屏/可调整大小)、系统对话框(open)、shell(open 外部链接/程序)、`os` 与 `window-state`(启动时还原窗口位置尺寸)插件的默认权限集。

### 7.2 `logs.json`(`src-tauri/capabilities/logs.json:1-7`)

| 字段 | 值 |
|---|---|
| identifier | `logs-window`(注意:**不是** `logs`) |
| windows | `["logs"]` |
| permissions | `["core:default"]` |

独立日志窗口(`open_log_window` 命令打开的窗口,S4,日志能力重构方案 §5/§9.1 #8)的最小权限集——只给 `core:default`,不继承 `default` capability 的 dialog/shell/os 权限,体现「非主窗口按最小权限单独声明」的设计取向。

其余跨切面的 CSP / assetProtocol 权限红线摘要 + 权威出处见 [Spec14 不变量与约定](./Spec14_不变量与约定.md)。

## 8. 边界与失败模式

| 场景 | code | 前端处理 |
|---|---|---|
| 操作被取消(用户主动停止扫描/AI分析/导出等) | `Cancelled` | 静默吞掉,不弹错误 toast,不记错误日志(`error.rs:419-426`) |
| 选区所据布局已过期(SelectAll 后又发生变更) | `ViewStale` | 前端重新 `compute_layout` 拿新 `layout_version` 后重发原命令,不提示用户失败 |
| 布局缓存未就绪 | `LayoutNotReady` | 前端等待 `compute_layout` 完成后重试 |
| 存储卷离线(外置盘/网络盘掉线) | `VolumeOffline` | message 即卷标签,前端弹「请插入设备 `<label>`」,重连后自动恢复,不当硬故障处理 |
| 前端调用了未在 `IPC` 常量登记的命令名 | (不适用,编译期即报错) | `invokeIpc` 的 `IpcCommand` 类型层拒绝,TypeScript 编译失败,不会跑到运行期 |
| 后端命令不存在(如手误漏加进 `registry.rs`) | Tauri 运行期抛 `unknown command` 类错误 | `parseAppError` 收到裸字符串/非 `{code,message}` 结构 → 兜底 `Unknown`,`ipcErrorMessage` 仍能取出可展示文案 |
| 权限未在 capabilities 声明即调用受限 API/命令 | Tauri ACL 运行期拒绝 | 需要在对应 `capabilities/*.json` 补权限后重新构建;这是运行期而非编译期失败,新增原生插件/窗口时容易漏(见 §4 不变量) |
| 旧命令仍返回裸字符串(未迁移到 `AppError`) | 无结构化 code | `parseAppError` 判定为字符串分支 → `code='Unknown'`,不崩溃但前端无法按 code 分流,只能展示 |

## 9. 重建指引(从零实现)

### 9.1 依赖顺序

1. 定义 `AppError` 枚举(thiserror)+ 手写 `Serialize` impl(§3.2 逻辑),此为一切命令的返回类型基础。
2. 建 `ipc/blocking.rs` 的 `read_blocking`/`write_blocking` 助手(依赖 `AppState` 的 DB 连接池/写锁已就位)。
3. 逐模块建 `ipc/<name>_commands.rs`,每个 `#[tauri::command] async fn` 返回 `Result<T>`。
4. 建 `ipc/registry.rs` 的 `handler()`,在 `tauri::generate_handler!` 里逐一列出。
5. `lib.rs` 里 `Builder::default().invoke_handler(ipc::registry::handler())`。
6. 前端 `src/constants/ipc.ts` 建 `IPC` 常量表(命令名字符串必须与后端函数名逐字符一致)。
7. 前端 `src/utils/ipc.ts` 建 `IpcError`/`parseAppError`/`invokeIpc`/`invokeIpcRaw`/`ipcErrorMessage`。
8. `src-tauri/capabilities/default.json` 声明主窗口所需插件权限;新窗口(如独立日志窗口)按需建最小权限的独立 capability 文件。

### 9.2 外部依赖

| crate/包 | 用途 |
|---|---|
| `tauri` 2.x | IPC 框架、`generate_handler!`、capabilities ACL |
| `thiserror` | `AppError` 派生 `Error`/`Display`,`#[from]` 自动转换 |
| `serde` / `serde_json` | `AppError` 手写 `Serialize`、错误链 JSON 数组序列化 |
| `tokio` | `spawn_blocking` 承载 DB 调用 |
| `r2d2` | DB 连接池(`Pool` 错误变体来源) |
| `tracing` | 错误日志(`tracing::error!`/`tracing::debug!`) |
| `@tauri-apps/api`(前端 npm) | `invoke` 基础调用 |

### 9.3 坑与教训

- 结构化变体的 `code` 是运行期字符串而非变体名——新代码按变体名匹配 `Backup`/`Export` 等永远匹配不到(§3.2/§6.2)。
- `file_job_busy` 码在 Backup/Export/Edit 三个变体间共享,前端故意不区分「谁在忙」,统一提示「有文件任务在跑」——不要误以为是遗漏未加变体私有码。
- `Export` 的 SelectAll 过期路径复用全局 `ViewStale`(而非 `export_view_stale`)——`error.rs:141-144` 注释显式记录 2026-07-19 复审曾误记后者,全仓从无该发射点,勿凭直觉补一个「更专属」的码。
- 结构化错误的 `message` 一律不带路径/SQL/内部异常串,是全部 12 个结构化变体的共同硬约束,不是逐个可选项。
- `exotic_commands.rs` 里同名函数 `install_exotic_plugin` 靠 `#[cfg(feature = "channel-direct")]` 二选一编译(543/556 两处定义互斥),grep 命令名会命中两行,不是重复定义 bug。

### 9.4 验收

- 单元测试:`cargo test -p <crate> ipc_commands_keep_rusqlite_off_async_workers`(`blocking.rs:78`,R1-3 spawn_blocking 回归门)。
- `error.rs` 内含多组 `AppError::*` 序列化断言测试(如 `serialize AppError::Relink/Backup/Color/Restore/Ocr/Exotic` 等,`error.rs:519-833` 区间),验证 `{code,message}` 结构与错误链收集正确。
- 前端:`vue-tsc` 严格模式编译作为「命令名走常量」契约的门禁(裸字符串编译不过)。
- 门禁命令:项目根 `cargo test`(Rust 侧)+ `npm run type-check`/`vue-tsc`(前端类型)按变更面运行,详见 `.github/workflows/ci.yml`。

## 10. 关联

- 上游正典:[Part6 插件平台与exotic收尾](../refactor_2026/Part6_插件平台与exotic收尾.md)(WorkerErrorCode 等协议错误码设计理由,exotic/OCR/AI worker 通路错误码设计脉络)。
- 相关规格篇(各命令业务语义归属):[Spec02 扫描与画廊](./Spec02_扫描与画廊.md)、[Spec03 缩略图与派生](./Spec03_缩略图与派生.md)、[Spec04 图像与色彩管线](./Spec04_图像与色彩管线.md)、[Spec05 视频与音频](./Spec05_视频与音频.md)、[Spec06 AI人脸OCR](./Spec06_AI人脸OCR.md)、[Spec07 文档与阅读器](./Spec07_文档与阅读器.md)、[Spec08 存储备份导出文件操作](./Spec08_存储备份导出文件操作.md)、[Spec09 插件平台与exotic](./Spec09_插件平台与exotic.md)、[Spec12 配置状态日志](./Spec12_配置状态日志.md)。
- 跨切面权限/CSP 红线:[Spec14 不变量与约定](./Spec14_不变量与约定.md)。
- 前端调用侧架构(store/composable 如何用 `invokeIpc`):[Spec11 前端架构](./Spec11_前端架构.md)。
