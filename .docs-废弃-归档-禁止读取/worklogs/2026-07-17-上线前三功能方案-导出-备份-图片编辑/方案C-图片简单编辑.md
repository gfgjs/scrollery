---
id: 2026-07-17-方案C-图片简单编辑
status: active
type: design
line: 上线前三功能方案-导出-备份-图片编辑
created: 2026-07-17
last-verified: 2026-07-19
---

# 方案 C:图片简单编辑

> 复审基线:方案提交 `247d05c` 之后的当前代码。契约立场:开发期可推翻;本稿为**有条件通过**的待裁决方案,完成元数据与内存 P0 spike 前不得进入完整施工。
> 2026-07-19 二次核对:对 HEAD(`5f92f1d`)复核依赖断言(image 锁 0.25.10、有损 WebP 走独立 `webp` crate、V20/V21、前台交互信号与 `background_heavy_limiter`)，结论与工程量不变。

## 1. 结论与目标

- 「简单编辑」只含 90° 旋转、水平/垂直翻转、裁剪;定位是看图时顺手处理,不是修图软件。
- **v1 只另存副本,不覆盖原图**。原 item、原文件和已确认的人脸/AI 状态保持不变。
- 当前实现已新增 V20 `media_items.view_rotation`。编辑器必须从该显示旋转起步,保存时把最终几何效果烤进新文件;新 item 的 `view_rotation=0`。
- 施工前先用固定样本证明 EXIF orientation、ICC、内嵌 EXIF 缩略图和 WebP 元数据策略可闭环,再锁定输出格式。
- 施工前以真实峰值内存基准决定像素上限,不再采用原稿约 268MP/约 1GB RGBA 的危险静态上限。

## 2. 为什么 v1 不做就地覆盖

当前就地覆盖仍有三项系统性风险:

1. 源文件变化会触发派生数据失效,可能连带清除人脸、AI 状态与用户确认结果。
2. 查看器按同一路径读取全尺寸图,缺少可靠的像素版本/cache-buster 契约,可能继续显示旧缓存。
3. quick scan 的目录 mtime 剪枝对部分就地变化仍存在漏检窗口。

另存副本让原数据不变,新文件按新 item 入库,可把以上风险从 v1 主链移除。就地覆盖留在 P2,只有失效豁免、查看器重载和扫描一致性三项均解决后才准入。

## 3. v1 范围与格式门槛

### 3.1 几何能力

- rotate 90° 步进(顺/逆时针);
- flip 水平/垂直;
- crop 自由比例,以及原比例、1:1、4:3、3:4、16:9、9:16;
- 编辑会话内支持重置,不保存编辑历史。

几何顺序固定为:

1. 解码并只应用一次文件 EXIF orientation;
2. 合入原 item 的 V20 `view_rotation` 与本次 rotate;
3. flip;
4. crop。

crop 坐标属于前端最终预览坐标系。后端按原始全分辨率复算,边界 clamp,零面积返回稳定错误。

### 3.2 输入与输出格式

- v1 输入只承诺跨平台 `image` 解码主链可稳定处理的静态 JPEG、PNG、WebP、BMP、TIFF。
- 动画 GIF 暂不承诺。「取当前帧」需要查看器帧号与后端解码帧之间的新契约,当前代码没有该数据,不得假装保存用户正在看的帧。
- HEIC/AVIF 当前主要依赖 Windows WIC 路径,不满足 Windows/macOS/iOS/Android 一致性目标,先放 P2。
- JPEG、PNG 为必选输出。WebP 只有在 §4 golden spike 证明有损编码后可可靠写回目标元数据时才进入 v1;否则 WebP 输出延期,不能用无提示的无损编码冒充质量滑杆。
- 默认:JPEG/WebP 静态源选 JPEG 质量 92;PNG/BMP/TIFF 选 PNG。用户可显式切换已准入格式。

另存位置为原图同目录,默认 `{stem}-edit.{ext}`,冲突时递增 `-edit-2`。前端只传 `itemId`、操作与输出选项,路径由后端基于数据库 item 推导、canonicalize 并校验仍在已授权 root 内。

## 4. P0 元数据与色彩 spike

不能直接把「引入 `img-parts`」当成已验证结论。当前 `image 0.25.10` 已暴露 decoder 的 orientation/EXIF/ICC 读取和部分 encoder 的 EXIF/ICC 写入能力,而现有 WebP 有损编码走另一条 crate 路径。先用最小 spike 决定是否需要 `img-parts`、`little_exif` 或格式专用写回。

固定 golden 样本:

- JPEG orientation=6,同时含 DateTimeOriginal、ICC 与内嵌 EXIF thumbnail;
- PNG eXIf + ICC;
- WebP EXIF + ICC;
- 至少一个跨格式 JPEG→PNG/JPEG 与 PNG→JPEG 样本。

验收必须逐项证明:

- 预览与输出只旋转一次,输出 orientation 为 1 或已移除;
- DateTimeOriginal 与 ICC 按声明保留,无法跨格式表达的字段有明确降级规则;
- 不复制与新像素不一致的内嵌 EXIF thumbnail;
- 不承诺未验证的 XMP/IPTC 保留;
- WebP 若保留为输出,质量 1–100 的文件大小/视觉结果确实变化,且元数据写回后仍可解码。

元数据必须在**同目录临时文件**中完成编码、注入和复读验证,成功后再 flush/close 并 same-volume rename。不得先发布最终文件再修改元数据,否则原子写契约失效。

## 5. P0 内存与并发门槛

全尺寸编辑至少同时占用解码缓冲、变换结果、编码缓冲和前端预览资源,仅按 `width*height*4` 估算会严重低报。施工前以 24MP、50MP、100MP 和超长边样本测量:

- 各格式 decode/rotate/crop/encode 的峰值 RSS;
- operation 组合最坏峰值;
- 编码耗时、取消响应与前台交互卡顿。

后端先读取尺寸和 decoder `total_bytes`,使用 checked arithmetic 计算保守峰值预算,在大块分配前拒绝。最终像素/字节阈值由基准和目标设备预算共同决定,写入常量与单测,稳定码为 `edit_image_too_large`。

编辑保存走 `spawn_blocking`,全局只允许一个前台编辑任务。它通过现有前台交互信号让后台派生任务 yield,但不排在 `background_heavy_limiter` 后等待;否则用户点击保存可能被后台缩略图/AI 长时间阻塞。

## 6. 后端契约

新命令只接受服务端可校验的数据:

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditOps {
    pub rotate: u16,          // 0|90|180|270,相对当前所见顺时针
    pub flip_h: bool,
    pub flip_v: bool,
    pub crop: Option<CropRect>, // 最终预览坐标系中的源像素矩形
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditOutput {
    pub format: EditFormat,
    pub quality: Option<u8>,
    pub file_name: Option<String>,
}

#[tauri::command]
pub async fn save_edited_image(
    item_id: i64,
    ops: EditOps,
    output: EditOutput,
    state: tauri::State<'_, AppState>,
) -> Result<EditSaveResult, AppError>;
```

处理链:

1. 按 `item_id` 查询 canonical source path、root、当前 `view_rotation` 和文件身份,不接受前端传盘面路径。
2. 在 blocking worker 中解码、应用一次 EXIF orientation、合入 `view_rotation` 和本次几何操作。
3. 对临时文件完成编码、元数据写回、复读验证、flush/close,再原子 rename 到唯一目标名。
4. 调用可测试的 `ingest_single_file` 做单文件 stat/插入/enrich,新 item 明确以 `view_rotation=0` 入库;触发现有数据版本/媒体事件与缩略图队列。
5. 返回 `saved { newItemId }`。若第 3 步已成功但第 4 步失败,返回**部分成功** `savedNeedsIndex { pathHint, recoveryCode }`,UI 告知文件已保存并提供「立即扫描」。不得返回普通失败诱导用户重复保存副本。

目标文件名需要去除路径分隔符、保留名、尾随点/空格和控制字符;冲突检查与创建必须在同一原子流程内,避免 TOCTOU。若目标 root 当前被隐藏,V21 仍允许对已打开 item 保存副本,但保存后的导航/列表是否可见必须如实提示。

新增 `AppError::Edit { code, message }` 或等价结构化变体,保持稳定码且不泄露内部路径/错误字符串。至少包括:

- `edit_source_unavailable`;
- `edit_format_unsupported`;
- `edit_decode_failed` / `edit_encode_failed`;
- `edit_image_too_large`;
- `edit_crop_empty`;
- `edit_target_conflict` / `edit_io`;
- `edit_saved_needs_index`。

## 7. 前端设计

- 入口放查看器底部控制条和 ContextualToolbar;仅对已支持的静态图片启用。GIF/HEIC/AVIF 显示明确的暂不支持原因。
- `EditOverlay.vue` 负责呈现,`useImageEditor.ts` 负责几何与状态;避免把复杂矩阵逻辑塞回查看器主组件。
- 打开编辑时,预览初始状态包含当前 item 的 `view_rotation`,确保用户看到的起点与查看器一致。
- 大图预览使用降采样 canvas,后端按全分辨率执行;前端以归一化矩形/显示矩阵换算为最终预览坐标,并用同一 golden 向量测试。
- crop 提供 8 手柄、比例锁、三分线和键盘微调;rotate/flip 后立即重投影。
- 状态机:idle → editing → saving → done/partial/error。saving 禁止重复提交;partial 明确区分「文件已落盘、尚未入库」。
- 编辑中禁翻页。Esc 退出时,仅在有改动时二次确认。
- 保存成功跳转新 item;V21 隐藏 root 或 partial 时不强行跳转到不可见 item。

## 8. 验证矩阵

### Rust 自动化

- rotate→flip→crop 组合 golden 小图逐像素断言;
- EXIF orientation 与 V20 `view_rotation` 组合,证明只应用一次且新 item 为 0;
- crop clamp/空拒、文件名净化、冲突并发创建;
- §4 全套 EXIF/ICC/thumbnail golden;
- WebP 输出门槛(若准入):质量行为、元数据写回与复读;
- 内存预算 checked arithmetic、超限在分配前拒绝;
- 临时文件失败不发布最终文件;元数据失败仍不发布;
- 文件成功但 ingest 失败返回 partial,重试扫描不会创建重复文件/item;
- GIF、HEIC、AVIF 走稳定 unsupported 错误;
- 隐藏 root、离线源、保存期间源文件被替换。

### 前端自动化与手测

- crop 坐标换算覆盖缩放、原 `view_rotation`、本次旋转、双向翻转;
- supported/unsupported 入口状态和 partial 文案;
- 24MP/50MP 基准机保存时 UI 仍可取消/反馈;
- 保存后网格出现、缩略图生成、查看器跳转;隐藏 root/partial 不错误跳转。

## 9. 工作量(复审估算,未测)

| 工作项 | 估算 |
|---|---:|
| P0 元数据/格式 spike + 内存基准 | 1.5–2 天 |
| 后端编辑、原子落盘、单文件 ingest | 2.5–3 天 |
| 前端 overlay/crop/状态机 | 2–2.5 天 |
| 自动化与跨平台手测 | 1–2 天 |
| **合计** | **7–9.5 天** |

原稿 5.5–6 天未计入格式元数据闭环、V20 显示旋转、内存基准和 partial recovery,现作废。

## 10. 待裁决

| # | 问题 | 建议 |
|---|---|---|
| C-1 | EXIF/ICC 方针 | 先完成 golden spike;只承诺通过样本验证的字段 |
| C-2 | 调色进入 v1? | 否,P2 |
| C-3 | 命名 `{stem}-edit.{ext}` | 采用,冲突递增 |
| C-4 | 网格右键也开放编辑? | v1 否,仅查看器 |
| C-5 | GIF 当前帧、HEIC/AVIF 输入 | v1 延期,先补跨平台帧/解码契约 |
| C-6 | WebP 输出 | 仅在有损+元数据 spike 通过后准入 |

## 11. P2 池

调色三滑杆、就地覆盖、GIF 当前帧、跨平台 HEIC/AVIF、批量旋转、无损 JPEG 旋转、编辑历史。就地覆盖的准入条件保持为:派生数据失效豁免、查看器像素版本重载、扫描盲区修复全部完成。
