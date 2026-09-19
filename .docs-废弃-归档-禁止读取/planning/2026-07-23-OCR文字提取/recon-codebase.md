---
id: 2026-07-23-recon-codebase
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# OCR 文字提取功能立项地形图

摸底日期：2026-07-23 | 范围：Tauri v2 前后端代码库 | 策略：只读分析，路径定位


## 域1：图片查看器 UI

**现有结构**
- 主组件：`src/components/media/ContentViewer.vue` (root component，看图查看器页面)
- 底部工具栏：`ContentViewer.vue:125–247` (.detail-controls，半透明黑色浮层)
- 工具栏按钮组织：
  - 左侧：缩放/旋转/Live Photo/编辑（`line 128–186`）
  - 中间：文件名
  - 右侧：人脸识别切换/收藏/定位/信息/关闭（`line 200–246`）

**编辑入口位置**
- 编辑按钮：`ContentViewer.vue:180–186` (PencilLine 图标，仅静态图片)
  ```vue
  <UiIconButton v-if="detail.mediaType === 'image'" :label="t('edit.entry')" @click="openEditor">
    <PencilLine :size="18" />
  </UiIconButton>
  ```
- 关联编辑组件：`EditOverlay.vue`（line 42–48）

**样式与命名惯例**
- 按钮类型：`UiIconButton` (全局通用)，尺寸 18px (line 129–244)
- CSS 类：`.detail-controls__left`/`__center`/`__right` 分区（grid/flex 布局）
- 图标源：`@lucide/vue` 第三方库
- i18n key 格式：`t('detail.*')` 或 `t('player.*')`

**扩展建议**
- OCR 按钮可放在左侧工具区（与编辑相邻），或独立为右侧操作区的第二级菜单
- 新增 i18n key：`ocr.entry` / `ocr.processing` / `ocr.success`


## 域2：视频播放器 UI

**现有结构**
- 播放器主组件：`src/components/media/player/VideoPlayer.vue` (编排层、持有 <video>)
- 控制条组件：`src/components/media/player/VideoControlBar.vue` (纯 UI，line 1–138)
- 进度条组件：`VideoSeekBar.vue`
- 诊断面板：`VideoDiagnostics.vue`

**控制条按钮布局（VideoControlBar.vue）**
- 左组：播放/暂停 + 时间读数（line 71–86）
- 右组：音量 / 倍速 / 循环 / 字幕 / PiP / **截帧** / 全屏（line 90–136）
- 截帧按钮：`line 124`（Camera 图标，emit 'capture-frame'）
  ```vue
  <UiIconButton :label="t('player.captureFrame')" :disabled="captureBusy" @click="emit('capture-frame')">
    <Camera :size="18" />
  </UiIconButton>
  ```

**TimelineScrubberCanvas.vue 用途**
- `src/components/media/TimelineScrubberCanvas.vue`：**图库日期导航时间线**（非视频播放）
- 用途：按月/年密度条或日历时间滚动图库（Minimap 式交互）
- 与 OCR 功能无关

**视频帧提取现状**
- 已有截帧能力：`useVideoFrameCapture` composable (`src/composables/player/useVideoFrameCapture.ts`)
- 流程：canvas.drawImage(video) → canvas.toBlob(PNG) → 对话框选定路径 → IPC `SAVE_FRAME_PNG`
- 后端处理：`src-tauri/src/ipc/player_commands.rs` 的 `save_frame_png` 命令

**前端截帧流程（参考实现）**
```typescript
// useVideoFrameCapture.ts:61–110
const canvas = document.createElement('canvas')
canvas.width = el.videoWidth
canvas.height = el.videoHeight
const ctx = canvas.getContext('2d')
ctx.drawImage(el, 0, 0, width, height)
blob = await canvas.toBlob('image/png')
// 对话框选路径，IPC 上传
```

**扩展建议**
- OCR 按钮可添加到控制条右组（在截帧/全屏之间）
- 可复用当前帧画布：`canvas.toBlob()` 后直接送入 OCR 后端（无需重复取帧）
- 或直接用 `<video>.currentTime` + Media Foundation `read_frame_at` 在后端取帧


## 域3：AI 基建（推理运行时与工作流）

**推理运行时**
- ONNX Runtime (ort) 版本：`v2.0.0-rc.12` (`crates/scrollery-ai-core/Cargo.toml:22`)
- 执行引擎：DirectML (GPU) + CPU fallback
- 张量库：ndarray v0.16
- 文本分词：tokenizers v0.21

**Worker 进程架构**
- 二进制：`crates/exotic-workers/ai-worker/src/main.rs`
- 运行模式：**长驻子进程**（不是库）
- IPC 协议：stdin/stdout 帧通信（stdout = 协议帧，stderr = 日志）
- 通信层：`exotic-protocol` crate（定义 Frame/WorkerErrorCode/EmbedBatch 等）
- 启动与通信：由 `src-tauri/src/ai/worker_client.rs` 管理

**AI Core 层（推理核）**
- 位置：`crates/scrollery-ai-core/src/`
- 功能：CLIP 图像编码 + 人脸检测(SCRFD) + 人脸嵌入(ArcFace)
- 特性：`inference` feature 控制编译（T16 准备拆离 ort）
- 模型管理：registry 注册表 + 模型加载初始化

**图片输入流程**
```rust
// crates/exotic-workers/ai-worker/src/batch.rs:1–50
// 路径：cache_key → {ai_cache_dir}/{key[..2]}/{key}.webp (cache hit)
//      或 source_path 回退解码常见格式(jpeg/png/webp/bmp/gif/tiff)
// 解码限制：MAX_SOURCE_FILE_BYTES = 512MB
// 输出：DecodedImage (pixel vec + width/height)
```

**批处理接口**
- EmbedBatch：文本/图像编码批处理
- FaceDetectEmbed：人脸检测+嶌嵌入批处理
- 批处理支持逐项化失败处理（一项失败不连坐整批）
- Blob 格式：f32 LE 连续排布，维度校验 EmbedDimMismatch

**调用示例**（IPC 命令层）
- `src-tauri/src/ipc/ai_commands.rs:68–80`（#[tauri::command] 宏注册）
- 错误处理：`join_err()` 统一映射任务失败

**扩展建议**
- OCR 需要新增专用推理路径（PaddleOCR / TrOCR / Tesseract 等）
  - 若用 ort 模型：可复用 ai-core 的 SessionPool 机制
  - 若用独立库（tesseract）：需单独初始化 + 进程管理
- 图片输入可复用 DecodedImage 结构体 + image crate 解码
- Worker 侧新增 OCRBatch message type 与 worker_client 通信


## 域4：视频帧提取后端

**Media Foundation 后端**
- 位置：`src-tauri/src/video/media_foundation.rs`
- 用途：Windows 原生视频解码（无 FFmpeg）
- 关键设计：XVP(视频处理器 MFT) 内直接转格 RGB32 + 缩放（性能优化）

**按时间戳取帧 API**
- 函数：`read_frame_at()` (`media_foundation.rs:673`)
- 入参：`seek_100ns: i64`（100纳秒单位时间戳）
- 出参：`DecodedImage`（RGB32 像素数据）
- 超时护栏：`READ_SAMPLE_TIMEOUT` 异步读取（避免损坏视频死等）
- 异步模式：`IMFSourceReader::ReadSample()` + `OnReadSample` 回调

**关键帧提取**
- 函数：`keyframes()` (`media_foundation.rs:152`)
- 功能：提取 n 个关键帧，每帧缩放至指定高度
- 用途：生成缩略图精灵表(sprite)
- 调用栈：`src-tauri/src/derive/video.rs` → `src-tauri/src/thumbnail/generator.rs`

**缩略图管线（可复用）**
- 入口：`src-tauri/src/thumbnail/generator.rs` (ThumbConfig, generate_thumb)
- 支持档位：THUMB_TIERS = [64, 128, 256, 512, 1024] (`generator.rs:29`)
- 原子落盘：`write_atomic()` (.tmp + rename，防崩溃裂图)
- 缓存路径：`thumb_path()` / `ai_cache_path()`

**与 OCR 的关联**
- 当前帧取法1：使用前端 canvas.toBlob → IPC 上传（已有截帧）
- 当前帧取法2：前端传当前时间戳 → Media Foundation 后端取帧（可复用 read_frame_at）
- 单帧 OCR：不需要 keyframes sprite，仅需单帧 DecodedImage


## 域5：IPC 惯例与错误处理

**命令注册方式**
```rust
// src-tauri/src/ipc/ai_commands.rs:68–80 (例)
#[tauri::command]
pub async fn detect_ai_provider(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    tokio::task::spawn_blocking(move || -> Result<...> {
        // 业务逻辑
    }).await.map_err(|e| AppError::Internal(format!(...)))?
}
```

**错误类型与 Serialization**
- 类型：`src-tauri/src/error.rs` 的 `AppError` enum
- 实现：自动 `serde::Serialize` (derive)
- 稳定 code：某些变体包含 `code: &'static str` 字段（Exotic, Reveal）用于分流
  ```rust
  #[error("exotic error [{code}]: {message}")]
  Exotic { code: &'static str, message: String },
  ```
- 返回类型：`Result<T>` = `std::result::Result<T, AppError>`
- 前端对应：IPC 错误自动包含 `code` + `message` 字段

**权限声明**
- 位置：`src-tauri/capabilities/` (JSON 格式)
- 文件：`default.json` (示例)
  ```json
  {
    "identifier": "default",
    "permissions": [
      "core:default",
      "dialog:allow-open",
      ...
    ]
  }
  ```
- 注意：Tauri v2 使用 ACL，详见 `@tauri-apps/cli/schema/acl/`

**IPC 命令常量**
- 位置：`src/constants/ipc.ts`
- 模式：`IPC.XXX_YYY = 'xxx_yyy'`（蛇形命名）
- 前端调用：`invokeIpc<ReturnType>(IPC.COMMAND_NAME, payload)`

**扩展建议**
- OCR 错误新增变体：`OcrModelNotLoaded(String)` 或 `OcrProcessing(String)`
- 新增 IPC 命令：
  - `EXTRACT_TEXT_FROM_IMAGE`: { itemId, imagePath } → { text, confidence }
  - `EXTRACT_TEXT_FROM_VIDEO_FRAME`: { itemId, timeMsec } → { text, ... }
  - `OCR_STATUS`: 获取模型状态、下载进度


## 域6：前端惯例（状态、事件、通知）

**剪贴板写入先例**
- 无 Tauri 剪贴板插件集成
- 使用原生 API：`navigator.clipboard.writeText(text)` (`src/views/LogWindowView.vue`)
- OCR 文字可直接 `clipboard.writeText()` 复制到剪贴板

**Toast 通知先例**
```typescript
// src/composables/player/useVideoFrameCapture.ts:57
const toast = useToastStore()
toast.addToast('success', t('player.captureSaved'))
toast.addToast('error', t('player.captureFailed'))
```
- 存储：Pinia `toastStore` (类型化消息队列)
- 消息类型：'success' | 'error' | 'info' | 'warning'
- i18n key 格式：`t('domain.action')`

**长任务进度事件惯例**（derivationStore 模式）
```typescript
// src/stores/derivationStore.ts:57–81
// 流程：IPC 命令启动任务 → 定时轮询状态 → 取消后停止轮询
const POLL_INTERVAL_MS = 1000

async function fetchVideoStatus() {
  const s = await invokeIpc<DerivationStatus>(IPC.DERIVATION_STATUS, {
    kinds: videoKinds(),
  })
  // { pending, processing, done, error, isRunning, active }
}

// 启动任务
await invokeIpc(IPC.START_DERIVATION, { kinds, reset, operationId })
// 停止任务
await invokeIpc(IPC.STOP_DERIVATION, { operationId })
```

**进度状态结构**
```typescript
interface DerivationStatus {
  pending: number       // 未开始
  processing: number    // 处理中
  done: number          // 完成
  error: number         // 失败
  isRunning: boolean    // 全局流水线在跑
  active: boolean       // 该 kind 有任务在途
}
```

**Channel 流式进度**（Channel 模式，另一路）
```typescript
// src/stores/aiStore.ts (模型下载)
const ch = new Channel<ModelDownloadProgress>()
await invokeIpc('download_model', { ... }, { ...eventHandlers })
```

**扩展建议**
- OCR 进度事件：新增 IPC 状态查询 `OCR_STATUS` → 返回 `{ busy, progress, ... }`
- 通知文案：添加 i18n key，如 `ocr.extracting` / `ocr.complete` / `ocr.failed`
- 长任务操作：遵循 derivationStore 模式（启动、轮询、取消），避免 UI 阻塞


---

## 快速索引（路径定位表）

| 功能域 | 主要文件 | 关键行 | 用途 |
|--------|--------|--------|------|
| 图片工具栏 | ContentViewer.vue | 125–247 | .detail-controls 底部浮层 |
| 编辑按钮 | ContentViewer.vue | 180–186 | PencilLine 图标，可参考样式 |
| 视频控制条 | VideoControlBar.vue | 1–138 | 所有播放器按钮组织 |
| 截帧按钮 | VideoControlBar.vue | 124 | Camera 图标 |
| 截帧实现 | useVideoFrameCapture.ts | 61–110 | canvas.toBlob + IPC 流程 |
| 推理运行时 | Cargo.toml (ai-core) | 22 | ort v2.0.0-rc.12 |
| AI Worker | ai-worker/src/main.rs | — | 子进程长驻，stdin/stdout 通信 |
| 图片输入格式 | batch.rs | 1–50 | DecodedImage，cache_key/source_path |
| 视频帧取帧 | media_foundation.rs | 673 | read_frame_at(seek_100ns) |
| 关键帧提取 | media_foundation.rs | 152 | keyframes(n, height) |
| 错误类型 | error.rs | 10–100 | AppError enum + Serialize |
| IPC 命令 | ai_commands.rs | 68–80 | #[tauri::command] 宏 |
| 权限声明 | capabilities/default.json | 1–25 | ACL 权限清单 |
| Toast 通知 | useVideoFrameCapture.ts | 57–80 | toastStore.addToast() |
| 进度轮询 | derivationStore.ts | 57–81 | POLL_INTERVAL_MS 模式 |
| IPC 常量 | src/constants/ipc.ts | — | IPC.XXX_YYY 命名约定 |

---

## 关键设计建议

1. **图片 OCR 入口**：ContentViewer.vue `.detail-controls__left` 区增加 OCR 按钮（与编辑相邻）

2. **视频当前帧 OCR**：
   - 选项 A：复用现有截帧流程（canvas → blob → IPC）
   - 选项 B：前端传时间戳 → 后端 Media Foundation 取帧（减少前端逻辑）

3. **推理后端**：
   - 若用 PaddleOCR/TrOCR (ort 模型)：复用 ai-core SessionPool
   - 若用 Tesseract：独立初始化，需设计错误恢复机制

4. **进度与通知**：
   - 采用 derivationStore 模式（定时轮询 OCR_STATUS）
   - Toast 通知成功/失败，避免 UI 阻塞

5. **剪贴板集成**：
   - 提取结果自动 `navigator.clipboard.writeText(text)`
   - 或右键菜单补充「复制文字」选项

---

摸底完成。共 6 域、路径精确定位、无计划外发现。
