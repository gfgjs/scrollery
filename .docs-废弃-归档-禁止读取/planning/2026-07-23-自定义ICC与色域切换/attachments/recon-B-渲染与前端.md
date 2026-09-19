---
id: 2026-07-23-recon-B-渲染与前端
status: active
type: working-memory
line: 自定义ICC与色域切换
created: 2026-07-23
---

# 摸底 B：查看器渲染与前端设置项

## 1. 编辑预览链回传形态

**输出类型**：
- `editing/preview.rs:57` 的 `render_edit_preview` 返回 `EditPreview` 结构体
- `EditPreview.into_packet()` (line 31) 打包为 **binary packet**：固定 28 字节 header (magic "SEP2" + format/version flags + 尺寸) + encoded 二进制体
- IPC 通路：返回的 `Vec<u8>` 包体直接序列化为 IPC 响应，**非 base64**（raw binary over Tauri channel）

**IPC 命令签名**：
- `ipc/edit_commands.rs:71` 无直接 `render_preview` 暴露的命令；预览在保存链内
- 完整保存命令：`pub async fn save_edited_image(...)` (edit_commands.rs §2-6 计划)
- 前端调用点：搜索结果无；编辑保存是同步单发任务，无预览 IPC（方案 §2 单遍编码）

## 2. 编辑保存链文件写法

**tmp + rename 模式**：
- `editing/io.rs:96` 的 `write_edited_image` 完整实现
- 锚点步骤：
  1. 编码到内存 `Vec<u8>` (line 104)
  2. 同目录创建 `.tmp` 文件 (line 112-120)
  3. 写入 + `sync_all()` (line 122-126)
  4. 复读验证 (line 80：`verify_round_trip`)
  5. `fs::rename` 覆盖占位文件 (line 130 以后)
  6. 任一步失败则清理 tmp 与占位 (line 127-129)

**关键约束**：先 tmp 后 rename，同卷原子提交（硬约束 CLAUDE.md）

## 3. ContentViewer 换源机制

**absPath 计算**：
- `src/components/media/ContentViewer.vue:590` → `const absPath = computed(() => (detail.value ? resolveAssetUrl(detail.value.absPath) : ''))`
- 来源：`detail.value` = `media.detailItem`（Pinia media store）
- 使用 `resolveAssetUrl` (utils/assetUrl.ts:4) 转换为 asset protocol URL

**换源接线点（先例）**：
- `liveVideoSrc` (ContentViewer.vue:1333) → `state.liveVideoSrc.value = resolveAssetUrl(path)` 同类逻辑
- 若要同一 item 在设置变化时换派生文件 URL：**修改 media.detailItem 的 absPath** 或引入新计算属性（如 `derivedAbsPath`），使 `absPath` computed 依赖新属性，自动触发 `<img>/:src` 更新

## 4. 设置项全链（以 thumb_webp_quality 为例）

**Rust 端定义**：
- `src-tauri/src/config/schema.rs:375` → `SettingDef { section: "thumbnails", key: "thumb_webp_quality", kind: SettingKind::UInt, default: "80", ... }`

**IPC 层**：
- `src-tauri/src/ipc/config_commands.rs:30` → `pub async fn get_app_config(key: String, ...) -> Result<Option<String>>`
- `src-tauri/src/ipc/config_commands.rs:109+` → `pub async fn set_app_config(...)` (在 edit_commands.rs 以外，见 registry.rs)

**前端 Store**：
- `src/stores/configStore.ts:60` → `this.thumbWebpQuality = await fetchInt('thumb_webp_quality', 80)`
- `src/stores/configStore.ts:143` → `async setWebpQuality(val: number) { ... await this.saveConfig('thumb_webp_quality', clamped.toString()) }`

**SettingsView 控件**：
- `src/components/settings/DynamicSettingControl.vue:59` → 下拉/输入根据 `spec?.control` 类型渲染（注册表驱动）
- 实际使用处有待查（Thumb 策略用 segmented 特例而非通用选择器）

**完整流程**：schema 定义 → `ConfigManager` 内存读 → IPC `get/set_app_config` → configStore 同步 → 派生流水线感知（useConfigFile.ts:73-96）

## 5. 派生文件基础设施

**缓存目录结构**：
- 缩略图：`{app_data_dir}/cache/thumbnails/{size}/{2-char-prefix}/{cache_key_hex}.webp`  
  锚点：`src-tauri/src/thumbnail/cache.rs:7-32` 的 `thumb_path()` / `thumb_db_path()`
- AI 缓存：`{app_data_dir}/cache/ai_thumbs/{2-char-prefix}/{cache_key_hex}.webp`  
  锚点：`cache.rs:65-89` 的 `ai_cache_path()` / `ai_cache_db_path()`

**前端 URL 构造**：
- `src/utils/thumbCacheDir.ts:18` → `getThumbCacheDir()` 单源缓存（惰性 IPC 取值）
- `src/composables/useThumbLoader.ts:37` → `buildThumbUrl(thumbStatus, thumbPath, cacheDir)` 纯函数
  - status=1: `${cacheDir}/thumbnails/${thumbPath}` 经 `resolveAssetUrl` 转 asset protocol
  - status=3: 原文件绝对路径
- `resolveAssetUrl` (assetUrl.ts:4) 最终调 `convertFileSrc` 转 asset protocol

**CSP + 作用域**：
- `tauri.conf.json:45-48` → `assetProtocol { scope: ["$APPDATA/**"] }` **已覆盖缓存目录**
- 派生文件在 `cache/` 下可直接被 webview 加载（无需额外权限）

## 6. 自定义文件导入先例

**搜索结果**：无现成的通用「用户选文件→后端持久化」命令（如模型/ICC/字体导入）。

**现有命名约定**：
- AI 模型下载采用专门流程 (`ipc/model_download.rs`)，由后端自动管理缓存位置
- Exotic 插件管理采用包管理体系（install/deactivate，`ipc/exotic_commands.rs`）
- File import 无现成基础设施

**建议路径**：  
若需 ICC 导入，新建 `ipc/icc_commands.rs`：
1. 前端选文件 → 后端 `import_icc_profile(file_path: String)` 
2. 后端：验证格式 → 复制到 `{app_data_dir}/config/icc/` → DB 登记路径
3. 后端 `get_icc_profiles()` 返回已导入列表

## 7. DerivationStore / start_derivation 概览

**Kind 枚举**：
- `src-tauri/src/derive/kind.rs:20-62` 定义 `DerivationKind` enum
- 当前 6 种：AudioMeta, AudioCover, VideoCover, DocThumb, AiThumb, VideoKeyframes
- 新增派生需修改此枚：`enum DerivationKind { ..., ViewerColorTarget, }` (假设新名)

**添加派生类型的波及面**：
- `derive/kind.rs`: 枚举 + `as_str()` 匹配 + `from_str()` 反向映射 (line 55-80)
- `derive/pipeline.rs` / `derive/mod.rs`: 后端处理逻辑（根据 kind 选择具体编码器）
- `src/stores/derivationStore.ts:23-24` / `composables/useDerivationAutoStart.ts`: 前端 kind 常量 + 轮询列表
- 由使用方（configStore.restartDerivation）按需调 `start_derivation(kinds, reset)` IPC
- `ipc/derive_commands.rs`: `pub async fn start_derivation(state: ..., kinds: Vec<String>, reset: bool)`

**表面**（无需深入）：  
通过 `kinds` 参数列表化控制派生流水线作用域；`reset=true` 清 DB 重新扫描待处理任务

---

## 后续接线建议

**小阶段**（ phase-next ）：
1. 确定派生存储位置：缓存目录已覆盖，可复用 `cache/derived/{viewer_color}/{...}` 并应用同类 LRU 门控
2. ICC 导入链：后端命令 + 前端选文件弹窗 + SettingsView 已导入清单
3. 新增 `viewer_color_target` 设置项：按 §4 模板添加 schema + setter + 前端 select 控件
4. 派生类型扩展：§7 四处并发改动（kind + pipeline + store + useDerivationAutoStart）

**无待决项**
