---
id: 2026-07-24-findings-exotic-plugin
status: active
type: working-memory
line: 视频格式扩展子系统
created: 2026-07-24
---

# Exotic 插件子系统接入面事实 — 新视频 Worker 设计参考

## 1. 协议消息层（worker 协议消息类型概览）

**crates/exotic-protocol/src/message.rs**

- **RequestBody 请求枚举** (line 28-133)
  - 基础能力：Thumbnail、Metadata 单项请求
  - 会话族：SessionInit/SessionClose、EmbedBatch、FaceDetectEmbed、EncodeText
  - OCR专族：OcrSessionInit/OcrSessionClose、OcrBatch
  - 影像增强：EnhanceSessionInit/EnhanceSessionClose、EnhanceRun

- **SuccessBody 响应体** (line 320-351)
  - 必填字段：item_id、input_fingerprint、mime（"image/webp"）、width、height

- **能力字符串常量** (line 300-313): THUMBNAIL、METADATA、EMBEDDING、FACE_DETECT_EMBED、OCR_TEXT、ENHANCE

- **WorkerErrorCode** (line 503-536): Terminal/Retryable 错误码

## 2. Worker 生命周期、Spawn、崩溃/重试策略

**src-tauri/src/exotic/supervisor.rs**

- **spawn 握手** (line 260-300): subprocess + stdin/stdout/stderr 独立线程，Hello→Ready 握手
- **SessionDescriptor** (line 217-240): 记录 arch_id、batch_size、embed_dim、caps 用于切换判定
- **异常处理** (line 318-435): TimedOut/Disconnected/Protocol → kill → wait → alive=false
- **stderr 管理** (line 272-275、537-579): 行扫描转发 tracing + 64KiB 字节环形缓冲

## 3. Worker 路径解析与 Builtin 分支实现

**src-tauri/src/exotic/installer.rs::resolve_worker_path** (line 325-364)

- **Debug 旁路** (line 332-354)
  - PSD: EXOTIC_PSD_WORKER_PATH
  - RAW: EXOTIC_RAW_WORKER_PATH (builtin 新增，dev 未设时明确拒绝)
  - 新格式应添: EXOTIC_VIDEO_WORKER_PATH 同款

- **Release 分支** (line 358-364): builtin worker 明确拒绝 + None (H2b-prod 待办)

- **设计**: dev env + 构建型号两条件都需通过; Release 编译期消除 #[cfg]

## 4. 现有 Worker 对比 — 能力声明与注册方式

| Worker | WORKER_ID | Capabilities | 特点 |
|--------|-----------|--------------|------|
| ai-worker (line 37-40) | "ai-worker" | embedding/face/ocr/enhance | SessionInit 300s |
| psd-worker (line 19-22) | "psd-worker" | thumbnail | 无会话 |
| raw-worker (line 19-22) | "raw-worker" | thumbnail | 与 psd 同构 |
| enhance-worker (line 33-36) | "enhance-worker" | enhance | EnhanceSessionInit 300s |

Ready 统一格式: WORKER_ID + WORKER_VERSION + PROTOCOL_VERSION + capabilities + MAX_BLOB_LEN

## 5. 插件商店前端 IPC 与授权流

**src-tauri/src/ipc/exotic_commands.rs** (line 1-150+)

- **只读查询** (line 19-117)
  - list_exotic_format_resolutions() → Vec<FormatResolution>
  - get_plugin_entitlement(plugin_id) → PluginEntitlement
  - list_installed_exotic_plugins() → Vec<InstalledExoticPlugin>

- **序列化**: #[serde(rename_all = "camelCase")], 前端一一对应无手写转换

## 6. 派生管线挂接 — derivations 表结构与回写路径

**src-tauri/src/db/queries/derivations.rs** (line 1-100+)

- **表结构** (line 18-23): media_derivations(item_id, kind, status), status 0/1/2/3 机
- **消费接口** (line 44-49): get_pending_derivations() JOIN 解析绝对源路径
- **回写约定**: {work_dir}/{job}.tmp 同卷 rename 原子操作

## 7. RAW 免费授权实现位置与机制

**src-tauri/src/exotic/mod.rs::ExoticHost::availability_of** (line 390-447)

- **核心** (line 406-424): if off.builtin && off.license_tier=="free" → Authorized (无验签)
- **Catalog 配置**: distribution="builtin", license_tier="free"
- **插件常量** (coordinator.rs:32-36): PSD_PLUGIN_ID、RAW_PLUGIN_ID
- **授权查询** (line 480-495): entitlement_of() 返回 PluginEntitlement

## 8. 新增一个视频格式 Worker 需要动的全部接入点清单

| # | 接入点 | 文件路径 | 行号 | 动作 |
|----|--------|--------|------|------|
| 1 | 协议能力字符串 | crates/exotic-protocol/src/message.rs | 300-313 | 新增或复用 THUMBNAIL |
| 2 | Worker 进程实现 | crates/exotic-workers/video-worker/src/main.rs | 新建 | Ready 帧 + Thumbnail op |
| 3 | Builtin 路径分支 | src-tauri/src/exotic/installer.rs | 339-353 | EXOTIC_VIDEO_WORKER_PATH env 条件 |
| 4 | 插件常量定义 | src-tauri/src/exotic/coordinator.rs | 32-36 | VIDEO_PLUGIN_ID 常量 |
| 5 | 插件描述符 | src-tauri/src/exotic/coordinator.rs | 113-128 | PluginDescriptor 追加 |
| 6 | Catalog 定义 | src-tauri/src/exotic/catalog.rs | BUILTIN_CATALOG_JSON | offering 条目(distribution:"builtin", license_tier:"free") |
| 7 | 派生任务(可选) | src-tauri/src/db/queries/derivations.rs | 44-49 | kind_filter 支持视频类 |
| 8 | 前端无改 | src/components/* | — | 数据驱动自动集成 |

关键红线: Ready worker_id 一致(握手校验), license_tier="free" 必填, *.tmp 同卷 rename, stderr 纯 JSON 或文本
