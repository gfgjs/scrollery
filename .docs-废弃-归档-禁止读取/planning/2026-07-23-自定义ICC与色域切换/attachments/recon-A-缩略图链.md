---
id: 2026-07-23-recon-A-缩略图链
status: active
type: working-memory
line: 自定义ICC与色域切换
created: 2026-07-23
---

# 摸底报告：缩略图生成链

## 1. 引擎路由

**EngineArena 构建**  
src-tauri/src/engine/mod.rs:35–41（`EngineArena::phase1()`）注册两引擎，**注册顺序即分发优先级**：
- **ImageRsEngine**（第一）：JPG/PNG/WebP/BMP/GIF/TIFF — Phase 1 格式保持既有行为
- **WicEngine**（仅 Windows，第二）：HEIC/HEIF/AVIF/ICO — 新增格式兜接

**路由逻辑**  
src-tauri/src/engine/mod.rs:45–47（`engine_for()`）取首个 `can_handle()` 命中。平台条件：
- 非 Windows：image-rs 认的 7 格式走 image-rs，其余无引擎 → UnsupportedFormat
- Windows：Phase 1 仍由 image-rs 先行（测试 L57–61 锁定），heic/heif/avif/ico 由 WIC 兜接（测试 L67–73）

---

## 2. cache_key 组成

**计算位置**  
src-tauri/src/utils/hash.rs:29–40（`compute_cache_key(rel_path, file_name, file_mtime)`）

**输入源**  
| 分量 | 来源 | 说明 |
|------|------|------|
| `rel_path` | 扫描根内相对路径 | 若项目在根级则空字符串 |
| `file_name` | 文件基本名称 | 不含目录 |
| `file_mtime` | Unix 时间戳 | 文件最后修改时间 |

**构成**  
```
input_string = "{rel_path}/{file_name}|{file_mtime}"  // rel_path为空时前缀"/"
cache_key = xxh3_64(input_string) as i64
```

**版本/参数位**  
❌ 无。修改生成算法后**旧缓存不会自动失效**（需手工清理或升级版本号）。

---

## 3. encode_media_step 数据流

**函数包裹**  
src-tauri/src/thumbnail/generator.rs:461–469（`encode_media_step(item_id, cache_key, decoded, config)`）

**精确数据流**  

| 步骤 | 行号 | 说明 |
|------|------|------|
| 入口 | L472 | `encode_media_step_inner()` 原子操作 |
| AI 缓存 | L487 | `maybe_write_ai_cache()` — 一次解码两份产物 |
| **RGBA 调整** | **L496–501** | **`resize_to_rgba()`** — decoded.pixels 内存直操 → RgbaImage |
| **ThumbHash** | L508 | 从调整后的 RGBA 生成占位图 |
| **编码起点** | **L510** | **`encode_as_webp(&rgba_img, config.webp_quality)`** — WebP 编码 |
| 落盘 | L516 | `write_atomic()` 原子写 |

**像素变换最自然插入位置**  
**L496 前**（`resize_to_rgba()` 前，即 `maybe_write_ai_cache()` 后）：此时 `decoded.pixels` 仍为原始 RGBA 缓冲，可直接变换而无需多次分配。

---

## 4. EXIF 内嵌缩略图处理

**使用状态**  
✅ EXIF 内嵌缩略图**被用作缩略图来源**（快速路径）。

**代码路径**  
src-tauri/src/thumbnail/generator.rs:424–440（`try_cpu_decode()` 内 `try_exif_thumb()` 调用）

**处理流程**  
1. src-tauri/src/thumbnail/exif_thumb.rs:11–74（`try_exif_thumb()`）
   - L17：`engine.extract_embedded_thumb()` 提取 IFD1 JPEG 原始字节
   - L21：`image::load_from_memory()` 解码内嵌 JPEG
   - L29–40：仅应用 **EXIF Orientation 旋转**（无 ICC 处理）
   - L71：`encode_as_webp()` 编码后直接返回

**ICC 处理性质**  
❌ EXIF 快速路径**不走 `encode_media_step`**，而是在 `try_cpu_decode` 中直接返回 `DecodeResult::Ready`。故 EXIF 内嵌缩略图**无 ICC 色域变换机会**。

---

## 5. DecodedImage 非缩略图消费方（清单）

| 消费方 | 文件位置 | 用途 | 行号 |
|--------|---------|------|------|
| AI 分析 | src-tauri/src/derive/image.rs | CLIP 模型输入 | L132 |
| 视频关键帧 | src-tauri/src/video/media_foundation.rs | 关键帧采集 | L111/L152 |
| 文档封面 | src-tauri/src/derive/doc.rs | 文档封面提取 | (需查) |
| 音频封面 | src-tauri/src/derive/audio.rs | 音频封面提取 | (需查) |
| ThumbHash | src-tauri/src/thumbnail/thumbhash.rs | 占位图生成 | (缩略图管道内) |

**波及面**  
字段增删直接影响所有 5 个消费方的编译 + 运行时行为。

---

## 6. image-rs 引擎的 ICC Profile 可得性

**ImageReader 路径**  
src-tauri/src/engine/image_rs.rs:31–36（`ImageReader::open()` → `.with_guessed_format()` → `.decode()`）

**icc_profile() 可得性**  
❌ **不可得**。原因：
- `ImageReader::decode()` 返回 `image::DynamicImage`（像素数据）
- `DynamicImage` 无 `icc_profile()` 方法（image crate 0.25 不暴露 ICC 数据）
- L95 调用 `.to_rgba8()` 后 ICC 信息彻底丢失

**替代方案**  
需独立解析 ICC profile（外部库 `rgb/icc/color` 等）或升级 image crate 版本。

---

## 顺手发现

| 位置 | 问题 | 建议 |
|------|------|------|
| 无 | — | — |

