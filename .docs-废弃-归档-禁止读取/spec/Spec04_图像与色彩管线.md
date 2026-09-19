---
id: 2026-07-24-Spec04_图像与色彩管线
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec04-图像与色彩管线

> 一句话:本篇讲图片简单编辑(旋转/翻转/裁剪/调色/另存)、EXIF/XMP 元数据提取、以及查看器渲染色域管理(sRGB/P3/DCI-P3/自定义 ICC)三个子系统的 as-built 机制;服务扩展编辑功能或色彩管线的工程师与编码代理;读前无需前置知识,本篇术语首次出现即定义。**正典 `docs/refactor_2026/Part3_缩略图派生与GPU引擎.md` 无色彩管理章节,本篇为该空白的净新增内容**,不是对 Part3 某节的重写。

## 1. 概览

三个子系统各自独立、通过共享的 `editing::color` 色彩变换原语耦合:

- **图片简单编辑**(`src-tauri/src/editing/`):对图库中的静态图片做旋转/翻转/精细拉直/裁剪/亮度对比度饱和度调色,输出为**同目录另存的新文件**(不覆盖原图),另存后单文件注册进 `media_items`。前台交互式命令,非后台 job。
- **查看器色域管理**(`src-tauri/src/viewer_color/`):查看器(ContentViewer)大图按用户在设置里选定的目标色域(sRGB/Display P3/DCI-P3/自定义导入 ICC)派生一份**嵌入目标 ICC** 的图片,替代直显原图,使 WebView2 在广色域屏幕上对该 profile 做正确的显示映射。
- **EXIF/XMP 元数据提取**:两处相对独立的读取路径——`src-tauri/src/scanner/metadata.rs`(扫描期批量 enrichment,写入 `image_meta` 表)与 `src-tauri/src/editing/metadata.rs`(编辑保存期只读源图少量字段用于输出侧最小 EXIF 构造,不落库)。

### 代码位置(关键文件)

| 文件 | 职责 |
|---|---|
| `src-tauri/src/editing/mod.rs` | 模块声明,10 个子模块 |
| `src-tauri/src/editing/geometry.rs` | 旋转/翻转/拉直/裁剪几何链,`EditOps` 契约 |
| `src-tauri/src/editing/adjust.rs` | 亮度/对比度/饱和度调色公式与执行 |
| `src-tauri/src/editing/color.rs` | moxcms ICC 色彩变换公共入口(编辑与查看器共用) |
| `src-tauri/src/editing/metadata.rs` | 编辑保存期源图元数据读取 + 输出侧最小 EXIF 构造 |
| `src-tauri/src/editing/io.rs` | 编码 + tmp→rename 落盘 + 复读验证 |
| `src-tauri/src/editing/naming.rs` | 目标文件名派生 + TOCTOU 安全的原子认领 |
| `src-tauri/src/editing/ingest.rs` | 编辑输出单文件注册进 `media_items` |
| `src-tauri/src/editing/preview.rs` | E0 编辑预览(raw IPC 包) |
| `src-tauri/src/editing/entitlement.rs` | 图片编辑高级功能授权门 |
| `src-tauri/src/editing/memory_budget.rs` | 保存/预览峰值内存预算与 100MP 级准入门 |
| `src-tauri/src/viewer_color/mod.rs` | 模块声明 + 稳定错误码集 |
| `src-tauri/src/viewer_color/target.rs` | 目标色域解析(`ViewerColorTarget`)+ profile 装配 |
| `src-tauri/src/viewer_color/render.rs` | 派生渲染主体(解码→CMS→编码→落盘) |
| `src-tauri/src/ipc/edit_commands.rs` | 编辑 IPC 命令(5 条) |
| `src-tauri/src/ipc/viewer_color_commands.rs` | 查看器色域 IPC 命令(4 条) |
| `src-tauri/src/scanner/metadata.rs` | 扫描期 EXIF/XMP 提取(kamadak-exif/quick-xml) |
| `src-tauri/src/db/queries/metadata.rs` | `image_meta`/`video_meta`/`audio_meta` DAO |

### 整机中的位置

```
前端 EditOverlay/ContentViewer(Spec11)
        │ invoke
        ▼
ipc/edit_commands.rs ──┬─→ editing::{geometry,adjust,color,io,naming,ingest,metadata,memory_budget}
                        └─→ editing::entitlement(授权门)
        │
        ▼(save_edited_image 成功后)
scanner::enricher::run_enrichment(后台单根,fire-and-forget)
        │
        ▼
db::queries::metadata(image_meta upsert)

前端 ContentViewer 大图
        │ invoke get_viewer_color_url
        ▼
ipc/viewer_color_commands.rs ──→ viewer_color::target(解析 config 两键)
                              └─→ viewer_color::render::ensure_derivative
                                        │
                                        ▼
                              editing::color::to_target_rgba8(共享变换原语)
                                        │
                                        ▼
                              thumbnail::cache 布局的派生缓存(10GB 单源 LRU)
```

## 2. 数据模型与状态

### 2.1 DB 表(关键列;全表定义权威见 [Spec01](./Spec01_数据层.md))

**`image_meta`**(`src-tauri/src/db/schema.rs:129-149`,主键 `item_id` 外键级联删除):

| 列 | 类型 | 说明 |
|---|---|---|
| `orientation` | INTEGER DEFAULT 1 | EXIF orientation 原始值(1-8) |
| `exif_datetime` | INTEGER | Unix 时间戳,取 DateTimeOriginal→Digitized→Modified 优先级链 |
| `exif_make`/`exif_model`/`exif_lens` | TEXT | 相机制造商/型号/镜头 |
| `exif_focal_length`/`exif_aperture` | REAL | 焦距(mm)/光圈(F 值) |
| `exif_shutter` | TEXT | 快门速度,`"分子/分母"` 字符串形式 |
| `exif_iso` | INTEGER | ISO 感光度 |
| `exif_gps_lat`/`exif_gps_lng` | REAL | GPS 十进制度 |
| `dominant_hue`/`dominant_sat`/`dominant_lum`/`dominant_hex`/`is_monochrome` | — | **schema 已建列 + 索引(`idx_img_hue`,`schema.rs:151-152`),但仓内未找到任何写入路径**——`ImageMeta::default()`(`src-tauri/src/db/models.rs:226-244`)使这些字段恒为 `None`/`false`,`extract_image_meta`(`scanner/metadata.rs:212-309`)不写它们。待核实(是否为未完成的主色调特性遗留字段,或已废弃);**不要**假设有主色调聚类逻辑存在。 |

对应 Rust 类型:`ImageMeta`(`src-tauri/src/db/models.rs:226-244`)。写入:`upsert_image_meta`(`src-tauri/src/db/queries/metadata.rs:14-49`,`ON CONFLICT(item_id) DO UPDATE`)。查询待 enrichment 项:`get_unenriched_image_ids`(`metadata.rs:294-304`,以 `image_meta` 行缺失为判据)。

**`media_items.view_rotation`**(`schema.rs:789`,V20 决策):用户在查看器手动施加的展示旋转,`0/90/180/270` 四态,默认 `0`。与编辑几何链中的 `EditOps.rotate` 是两个独立量,合并公式见 §3.1。与 `video_meta.rotation`(视频容器旋转元数据,详见 [Spec05](./Spec05_视频与音频.md))**正交**——`view_rotation` 是用户会话态,`video_meta.rotation` 是文件内嵌的相机/编码器写入值,两者字段名相似但归属完全不同的媒体类型与语义层。

**`video_meta`/`audio_meta`**:与本篇图像管线无直接关系,仅在 `db/queries/metadata.rs` 同文件内共同实现(DAO 层按域拆分而非按表拆分),权威归 [Spec05](./Spec05_视频与音频.md)。

### 2.2 编辑版本

**没有 `document_versions` 式的编辑历史表**。方案 C 的另存策略(见 §3.1)决定了图片编辑不追踪版本链——每次保存产生一个**新的、独立的 `media_items` 行**(`editing::ingest::ingest_single_file`,`src-tauri/src/editing/ingest.rs:34-50`),旧文件与新文件之间除文件名约定(`{stem}-edit.{ext}`)外无 DB 级关联。若前端展示"从哪张图编辑而来",只能靠文件名或调用方自行记录,**不是**数据库契约。`document_versions` 表(如存在)属文档阅读器域,与本篇无关,详见 [Spec07](./Spec07_文档与阅读器.md)。

### 2.3 状态归属

| 状态 | 归属 | 存储位置 |
|---|---|---|
| 图片编辑高级功能授权态 | `EntitlementProvider` trait 对象(`state.entitlement_provider()`) | keyring / 平台收据(具体实现属 exotic 授权域,详见 [Spec09](./Spec09_插件平台与exotic.md)) |
| `viewer_color_target`/`viewer_color_custom_id` | `ConfigManager`(单源,前端不传参) | `<app_data>/config.toml`,详见 [Spec12](./Spec12_配置状态日志.md) |
| 自定义 ICC 文件 | 文件系统 | `{app_data_dir}/config/icc/{16位hex id}.icc`(`viewer_color/target.rs:113-121`) |
| 查看器色域派生缓存 | 文件系统,与缩略图共用 LRU 记账 | `cache/viewer_color/{target_id}/{prefix}/{hex}.{ext}`(`viewer_color/mod.rs:15-16`;`thumbnail/cache.rs:158-176`) |
| 编辑保存的门闩(防并发文件任务冲突) | `AppState`(内存) | `FILE_JOB_EDIT` 常量,与 backup/export 共用同一 `file_job_owner` 占用位(`state.rs`,`ipc/edit_commands.rs:20`) |

## 3. 关键流程与算法

### 3.1 图片简单编辑(几何 → 调色 → 编码 → 落盘 → ingest)

固定链序(`editing/geometry.rs:4-10` 模块文档钉死,不得重排):

```
解码源图
  │
  ▼
① 应用一次文件 EXIF orientation(仅 JPEG,D-009,见下)
  │
  ▼
② 合入 view_rotation(DB)与本次 ops.rotate,取模 360 落回四态之一
  │  combine_rotation(view_rotation, ops_rotate) → geometry.rs:122-130
  ▼
③ flip_h / flip_v(image crate 原生 fliph/flipv)
  │
  ▼
④ fine rotate(精细拉直,-45°..45° 闭区间;imageproc rotate_about_center_no_crop
  │  展开后裁剪取「保持原宽高比的最大居中内接矩形」,geometry.rs:169-232)
  │  (0° 或缺省完全跳过,不进入 imageproc,保 v1 字节级直通契约,D-106)
  ▼
⑤ crop(前端最终预览坐标系里的矩形,越界先 clamp,clamp 后零面积拒绝 edit_crop_empty)
  │
  ▼
⑥ E3 调色(可选,D-107 链序最末;亮度→对比度→饱和度)
  │
  ▼
⑦ 编码(JPEG q1-100 或 PNG)+ 元数据写回(ICC + 最小 EXIF)
  │
  ▼
⑧ 同目录 *.tmp → 复读验证 → sync_all → rename(io.rs:96-155)
  │
  ▼
⑨ 单文件 ingest(directory_id 已知,跳过目录链递归,ingest.rs:34-50)
```

- **步骤①的 D-009 裁决**:文件 EXIF orientation 只对 JPEG 生效(`editing::metadata::effective_source_orientation`,`editing/metadata.rs:50-56`),与查看器实际行为(`engine::image_rs::ImageRsEngine` 只在 jpg/jpeg 分支应用 orientation)对齐——若单方面对非 JPEG 也应用,已用 `view_rotation` 手动扶正过的 PNG 会被双重旋转。
- **步骤④的展开尺寸公式**(`expanded_dimensions`,`geometry.rs:79-97`):`expanded_w = ceil(h·|sin θ|+w·|cos θ|)`,`expanded_h = ceil(h·|cos θ|+w·|sin θ|)`,**刻意**与 imageproc 0.27 内部实现同用 `f32`(而非更精确的 `f64`),因为这是内存预算的准入判断,必须与真实分配逐像素一致,不是追求数学精度。
- **最大居中内接矩形公式**(`maximum_aspect_inscribed_size`,`geometry.rs:101-117`):候选矩形 `s·w × s·h` 的角点逆旋转回原矩形得到两个 `s` 上界,取较小者,再向下取整。前端 `src/fixtures/straightenGeometryGolden.json` 是双端共享的黄金向量(`geometry.rs:556`)。
- **调色公式**(`adjust.rs:6-20` 模块文档钉死,前端 `src/composables/adjustFormula.ts` 双端同源,黄金向量 `src/fixtures/adjustGolden.json`):
  - 亮度 `b∈[-100,100]`:`k_b = 2^(b/100)`,`v' = v·k_b`(乘法系数,±100 恰为 ±1EV)
  - 对比度 `c∈[-100,100]`:`k_c = (100+c)/100`,`v' = (v−0.5)·k_c + 0.5`(**不采用** `image::imageops::contrast` 的平方映射,两者数学不等价)
  - 饱和度 `s∈[-100,100]`:`k_s = (100+s)/100`,W3C Filter Effects saturate 矩阵,luma 权重 `[0.213, 0.715, 0.072]`(仅在 sRGB 原色下成立)
  - 应用序钉死:亮度→对比度→饱和度;每步 clamp `[0,1]`;alpha 不动
  - 全零参数在命令层被 `effective_adjust`(`adjust.rs:68-70`)过滤为 `None`,完全不进入调色管线(D-106,保 v1 字节级不变)
  - 有 ICC 源:先经 moxcms(relative colorimetric)转 sRGB → f32 公式 → 按**源位深**量化回写;分条(strip,64 行一条,`STRIP_ROWS`)转换控内存峰值
  - 输出侧:调色后像素已在 sRGB,嵌显式 sRGB profile(`srgb_profile_bytes`,`adjust.rs:146-153`),**不回写源 ICC**(阅读器会按错误空间解释)
- **主入口**:`geometry::apply_geometry`(`geometry.rs:135-167`)+ `adjust::apply_adjust`(`adjust.rs:157-378`)+ `io::write_edited_image`(`io.rs:96-155`),命令层编排见 `ipc::edit_commands::run_save_edited_image`(`edit_commands.rs:212-380`)。

### 3.2 ICC 色彩管理(moxcms)

共享变换原语在 `editing::color`(`src-tauri/src/editing/color.rs`),供编辑链、缩略图链、查看器色域链三方复用:

| 函数 | 用途 | 失败行为 |
|---|---|---|
| `to_srgb_rgba8`(`color.rs:88-108`) | 编辑预览/保存链:投影到 sRGB | 失败返回 `Err`(`edit_decode_failed`) |
| `to_target_rgba8`(`color.rs:40-83`) | 泛化版:投影到任意 `target`(查看器色域复用) | 同上 |
| `project_rgba8_to_srgb`(`color.rs:211-252`) | 缩略图链路薄封装 | **绝不失败**——异常路径原样放行原始像素,只记 warn(D-410:缩略图可用性优先于色准) |

变换 intent 统一为 `RelativeColorimetric`(`color::options()`,`color.rs:24-29`),编辑预览/保存与查看器渲染共用同一常量,防止两条链的色彩结果漂移。

**无 ICC 源的处理**:业界惯例假定为 sRGB。`to_srgb_rgba8` 在此情形**原样返回不经 CMS 数值管线**(D-413,见 §4);`to_target_rgba8` 被以非 sRGB `target` 调用时(查看器渲染)则仍需真实投影(sRGB→target)。

**CMYK JPEG 边界**:`image` crate 的 JPEG 解码器已把 CMYK 样本转换为 RGB,若再把解码后的 RGB 缓冲按 CMYK layout 喂给 ICC 会二次误解通道;`color.rs:70-81` 显式判 `DataColorSpace::Cmyk` 并按"解码结果已是 sRGB"处理,moxcms 不参与该边界(D-412 曾在此有过破口,已修复:此前直接 `Ok(image.to_rgba8())` 却仍嵌入 target ICC 元数据,像素其实从未投影却谎称已编码,见 `color.rs:66-69` 注释)。

**缩略图色管**(A 批,归 [Spec03](./Spec03_缩略图与派生.md) 权威,此处只提一句):`project_rgba8_to_srgb` 把已解码 RGBA8 缓冲投影到 sRGB,用于缩略图生成管线。

### 3.3 查看器色域切换(B 批)

```
① 命中检查:同 (target_id, cache_key) 的 jpg/png 任一存在即返(render.rs:39-47,62-64)
② 动画 WebP 前置拒绝(image WebP 解码器对动画只出首帧,派生首帧误导用户)
③ 装配 target profile:CMS 变换对象 + 要嵌入输出的字节(同源,D-412,见 target.rs:70-110)
④ 解码源图 + 内存预算前置校验(100MP 级准入,超限拒 viewer_render_too_large)
⑤ orientation 烤入像素(仅 jpg,D-009 同款)
⑥ CMS 投影到 target(无 ICC 源假定 sRGB 仍转换)
⑦ 按 alpha 判定编码格式:有 alpha → PNG 无损;无 alpha → JPEG q92,嵌入 target ICC
⑧ 原子落盘(tmp + 同卷 rename,write_atomic)
```

主入口:`viewer_color::render::ensure_derivative`(`render.rs:51-137`)。**不经 `media_derivations` 记行**——命中判定只查 `exists()`,缺失即自愈重渲(与 motion_video 派生同姿态);mtime 变→`cache_key` 变→旧派生成孤儿,靠 `thumbnail::cache::reconcile_orphan_gc` 回收(`render.rs:6-7`)。

**目标色域解析**(`ViewerColorTarget::from_config`,`target.rs:45-56`):从 `ConfigManager` 的 `viewer_color_target` + `viewer_color_custom_id` 两键派生,前端**不传** target 参数(防前后端口径分叉)。`srgb` 或未知值 → `None`(零派生,直显原图);`custom` 但 id 非法/空 → 同样优雅回退 `None`,不落错误码。

**自定义 ICC 导入校验链**(`ipc/viewer_color_commands.rs:194-264`):`canonicalize + is_file` → 大小 ≤16MB(`ICC_MAX_BYTES`)→ `ColorProfile::new_from_slice` 解析 → `color_space==Rgb` → `profile_class==DisplayDevice` → 变换探针(`sRGB→candidate` 8-bit 变换,拦截 LUT 型不可变换 profile)→ 持久化到 `{icc_dir}/{16位hex id}.icc`(id = ICC 原字节的 xxh3,`profile_id_of`,`target.rs:38-40`)。同字节重复导入幂等。

**平台门控(D-414)**:移动端(android/ios)锁 sRGB——`get_viewer_color_url` 返回 `Ok(None)`(直显原图,`viewer_color_commands.rs:50-53`),import/list/delete 返回 `unsupported_platform`(`viewer_color_commands.rs:150-151` 等,先例 `ipc::reveal.rs:37-41`)。

### 3.4 EXIF/XMP 元数据提取

**扫描期批量 enrichment**(`src-tauri/src/scanner/metadata.rs`):

- 快速路径(`read_jpeg_orientation`,`metadata.rs:51-64`):只读 JPEG 的 Orientation 标签,供快速扫描即时判断宽高互换。
- 完整解析(`parse_exif_meta`/`parse_exif_meta_buf`,`metadata.rs:191-209`):用 `kamadak-exif`(`exif` crate)读全部 EXIF 字段,`extract_image_meta`(`metadata.rs:212-309`)统一提取 orientation/日期(DateTimeOriginal→Digitized→Modified 优先级)/相机厂商型号镜头/焦距/光圈/快门/ISO/GPS(度分秒转十进制度,南纬西经取负号)。
- **头缓冲优化**(`#10`,2026-07-17;阶段5 阶梯化):单次 `open` 读头进内存,EXIF/XMP/尺寸三消费者共用,避免同一文件多次 `open`(Windows 下每次 open 伴随 Defender 扫描开销);`read_header_buf_for_ext` 按格式阶梯读(JPEG 128KB、TIFF/HEIC/HEIF/AVIF 256KB、其余 64KB,`metadata.rs:header_buf_cap_for_ext`);缓冲不足以解出结果且文件确实更大时逐消费者回退整文件路径。
- XMP Motion Photo 检测(`detect_motion_photo_xmp[_buf]`,`metadata.rs:318-355`):扫描前 128KB 匹配 Google/Samsung 的动态照片标记字符串,与 EXIF 无关但共用同一头缓冲。
- TIFF 维度解析硬超时(`run_with_timeout`,`metadata.rs:27-40`):畸形 TIFF 可能无限阻塞,detached 线程 + `recv_timeout` 5 秒兜底,超时即放弃(泄漏一线程,优于永久阻塞 enrich 工作者)。

**编辑保存期只读**(`src-tauri/src/editing/metadata.rs`):只读三项(orientation/ICC/`DateTimeOriginal`),不落库,只用于:①决定几何链首步 orientation;②输出侧构造全新的最小 EXIF blob(`build_output_exif`,`editing/metadata.rs:93-98`)。**不转发原始 EXIF blob**——原因见 §4「编辑输出不透传原始 EXIF」。

### 3.5 并发/线程模型

- 编辑与查看器色域渲染的实际图像处理(解码/CMS/编码/IO)恒在 `tokio::task::spawn_blocking` 内执行,不占用 async worker(`edit_commands.rs:172-175`,`viewer_color_commands.rs:74-77`)。
- 编辑保存持 `FILE_JOB_EDIT` 门闩(与 backup/export 互斥,`try_acquire_file_job`/`FileJobReleaseGuard`,`edit_commands.rs:139-169`),防止并发文件级任务互相踩踏;RAII 守卫保证提前 `?` 返回也释放门闩。
- 查看器色域派生用 `(item_id, target_id)` 的 keyed lock 去重并发请求(`state.viewer_render_lock`,`viewer_color_commands.rs:66-83`),同一图同一 target 的并发请求只渲染一次。
- 编辑保存成功后的后台 enrichment(`scanner::enricher::run_enrichment`)是 `tauri::async_runtime::spawn` 的 fire-and-forget 任务,不阻塞命令返回(`edit_commands.rs:186-204`)。

## 4. 契约与不变量(施工红线)

### 4.1 IPC 命令(全量归 [Spec10](./Spec10_IPC与错误契约.md),此处摘要要点)

| 命令 | 模块 | 要点 |
|---|---|---|
| `get_editing_entitlement` | edit_commands.rs:72-78 | 查询授权态,同步 keyring 读取离开 async worker |
| `activate_editing_feature` | edit_commands.rs:81-102 | plugin id/SKU 后端可信常量,前端只提交 token |
| `deactivate_editing_feature` | edit_commands.rs:105-116 | 撤销授权,幂等 |
| `get_edit_preview` | edit_commands.rs:126-136 | 返回 raw `tauri::ipc::Response`(非 JSON,自定义二进制包头,见 §3.1 之外的 `preview.rs:29-54`) |
| `save_edited_image` | edit_commands.rs:150-207 | 前台交互式单发任务,直接跑完整链路后返回终态(非 job/进度事件模型) |
| `get_viewer_color_url` | viewer_color_commands.rs:46-85 | `None` = 直显原图 |
| `import_icc_profile` | viewer_color_commands.rs:146-157 | 校验链见 §3.3 |
| `list_icc_profiles` | viewer_color_commands.rs:161-169 | best-effort,坏文件跳过 |
| `delete_icc_profile` | viewer_color_commands.rs:173-181 | 连带清派生子树;若删的是当前选中 profile,复位 config 至 srgb |

### 4.2 稳定错误码(全量归 [Spec10](./Spec10_IPC与错误契约.md);编辑域 `AppError::Edit`,色域 `AppError::Color`,`error.rs:401-409`)

编辑域(`AppError::Edit { code, message }`):`edit_decode_failed` / `edit_invalid_ops` / `edit_crop_empty` / `edit_image_too_large` / `edit_encode_failed` / `edit_io` / `edit_target_conflict` / `edit_source_unavailable` / `edit_format_unsupported` / `edit_not_entitled` / `file_job_busy`(与 backup/export 共用占用码)。`edit_saved_needs_index` 是**成功响应内**的字段值(`EditSaveResult::SavedNeedsIndex`),不是 `AppError` 变体。

色域域(`AppError::Color { code, message }`):`icc_parse_failed` / `icc_not_rgb` / `icc_not_display_class` / `icc_transform_unsupported` / `icc_too_large` / `icc_io` / `icc_not_found` / `unsupported_platform` / `viewer_render_unsupported` / `viewer_render_decode_failed` / `viewer_render_too_large` / `viewer_render_io`(`viewer_color/mod.rs:25-38`)。

### 4.3 不变量清单

| 不变量 | 出处 | 为什么(违反会怎样) |
|---|---|---|
| **D-412 像素域 == 嵌入 profile**:CMS 变换与嵌入用同一 profile 对象/同一字节 | `viewer_color/mod.rs:8-9`,`viewer_color/render.rs` 测试 `adobe_rgb_to_display_p3_embeds_target_icc_and_matches_lcms` | 若变换用 A 而嵌入用 B(即便同名同色域),阅读该文件的下游程序会按错误 profile 解释像素,色彩失真且不可诊断;此前有过破口(CMYK 分支未投影却嵌入 target ICC,谎称已编码) |
| **D-413 编辑链短路留包装层字节不变**:`to_srgb_rgba8` 在无 ICC 源(以及 CMYK 边界)时原样返回,不经 CMS 数值管线 | `editing/color.rs:85-108`,`editing/color.rs:95-99` 注释 | sRGB→sRGB 恒等变换在离散 8-bit LUT 下**不保证**逐字节相等;若强制过一遍 CMS,v1 「无 ICC 图片编辑后字节不变」的既有契约会被破坏,回归测试会检测到但语义上是不必要的精度损失 |
| **调色 intent 与预览同 intent**:编辑预览、保存调色、查看器渲染统一 `RelativeColorimetric` | `editing/color.rs:21-29` | 若三处 intent 不同,同一张图在预览/保存/查看器三处显示的色彩会互相矛盾,用户无法信任"所见即所得" |
| **禁用 CSS filter 做调色** | 撰写规范 §分派卡范围要求(`editing/adjust.rs` 全部走像素级 f32 公式而非前端 CSS `filter: brightness()`等) | CSS filter 语义与本篇钉死的公式(尤其对比度的线性系数而非平方映射)不等价,会导致预览与保存结果不一致 |
| **moxcms `encode()` 时间戳区清零对拍** | `viewer_color/render.rs:372-382` 测试注释 | ICC header 24..36 字节(`creation_date_time`)秒粒度写入序列化结果,跨秒边界的两次独立 `encode()` 调用会产生字节级不同但内容等价的 profile;逐字节比较测试必须先清零该区,否则出现与内容无关的偶发失败 |
| **`view_rotation` 与 `video_meta.rotation` 正交**,不可合并成同一概念 | `db/schema.rs:782,789` | 前者是查看器用户会话态(图片+视频通用),后者是视频容器内嵌的旋转元数据(仅视频);混淆会导致视频旋转被应用两次或图片旋转误用视频语义 |
| **D-009 orientation 仅对 JPEG 生效**(编辑链与查看器渲染) | `editing/metadata.rs:43-56` | 与 `engine::image_rs::ImageRsEngine` 显示行为对齐;若对 PNG/WebP 也应用,用户已用 `view_rotation` 手动扶正过的图会被双重旋转 |
| **编辑输出不透传原始 EXIF blob**,只构造最小 EXIF(Orientation 固定 1 + 可选 DateTimeOriginal) | `editing/metadata.rs:10-14` | 几何操作后原始内嵌缩略图(IFD1)与新像素不一致;且不想连带保留未验证字段 |
| **v1 不 admit WebP 输出** | `editing/metadata.rs:8-10` | 有损 WebP encoder(`webp` crate)无 ICC/EXIF 钩子,`image` 自带 WebP encoder 又只支持无损;老实不给该选项优于用无损冒充有损 |
| **全零调色参数视同未提供(D-106)** | `editing/adjust.rs:68-70` | 保住 v1「无编辑意图 → 字节级直通」契约,避免不必要的解码-CMS-编码往返 |
| **100MP 级内存预算准入门**(编辑保存/预览/查看器渲染三处独立校验) | `editing/memory_budget.rs:25-28` | 实测峰值 ≈6.02 字节/像素(格式无关基线,两段进程测量法排除了 setup 阶段页面预提交的测量伪影),× 2 安全系数 = 12 字节/像素预算,上限 1.5GB(工程判断,非硬性设备预算);超限必须在大块内存分配**之前**拒绝,否则消费级设备可能因单次编辑保存 OOM |
| **fine rotate 内存预算按展开尺寸而非原图面积判断** | `editing/memory_budget.rs:48-60` | imageproc 的旋转展开(见 §3.1 展开公式)会先分配比原图更大的缓冲,若只按原图面积判断会漏判 45° 拉直时的实际峰值 |
| **目标文件名认领必须 TOCTOU 安全**(`create_new` 独占创建占位,而非先 `exists()` 再 `create`) | `editing/naming.rs:41-77` | 并发场景下"先检查后创建"之间存在竞争窗口,可能两次调用都通过检查后互相覆盖 |
| **落盘先 tmp 后 rename,任一步失败清理不留孤儿** | `editing/io.rs:96-155` | 硬约束(AGENTS.md 项目规则);半成品文件被误当作已完成编辑会导致数据损坏假象 |
| **`ingest_single_file` 跳过目录链递归与 exotic 任务播种** | `editing/ingest.rs:2-8` | 编辑输出恒落在源 item 同目录,`directory_id` 已知;v1 编辑输出恒为 jpg/png,非 exotic catalog 格式,播种分支等价于 no-op,跳过是性能优化非功能缺失 |
| **`viewer_color` 派生不经 `media_derivations` 记行,只查 `exists()`** | `viewer_color/render.rs:6-7` | 与 motion_video 派生同姿态;简化一致性维护,代价是命中判定必须能自愈(mtime 变化后旧文件靠孤儿 GC 清理而非显式失效) |
| **`viewer_color` 派生缓存目录段编码为 `display-p3`/`dci-p3`/`icc-{16位hex}`,profile id 须严格 16 位小写 hex** | `viewer_color/target.rs:30-40,58-65` | 防路径注入(先例 `thumbnail::cache::parse_cache_key_stem`);若放宽校验,自定义 id 可能被构造成 `../../` 路径穿越 |

## 5. 边界与失败

| 边界情况 | 处理 | 出处 |
|---|---|---|
| 100MP 级大图 / 超角度旋转的准入门拒绝 | 保存/预览前置校验峰值内存预算,超限拒 `edit_image_too_large`;**真实可达拉直角度约 7.2°**(100MP 大图在 45° 展开下会先被内存预算门拒绝,45° 上限只是参数域校验,并非该角度在所有尺寸下都可达)——这是既有准入门的自然结果,不是本篇新增的缺陷 | `editing/memory_budget.rs`;[Part3](../refactor_2026/Part3_缩略图派生与GPU引擎.md) 记有类似的 100MP+45° 无法同时满足的实测 |
| crop 矩形越界 | 先 clamp 到图像边界(非拒绝),clamp 后零面积才拒绝 `edit_crop_empty` | `editing/geometry.rs:234-247` |
| `rotate` 不在 `0/90/180/270` | 显式拒绝 `edit_invalid_ops`,**不静默取模归 0**(D-010,静默会保存出"没转的图") | `editing/geometry.rs:19-21,54-56` |
| 拉直角度非有限值(NaN/Infinity) | 校验拒绝,不能进入三角函数计算 | `editing/geometry.rs:60-75` |
| ICC profile 解析失败/畸形 | 编辑链:`edit_decode_failed`;查看器渲染:`viewer_render_decode_failed`(不同码,不透传编辑侧码);缩略图链路(`project_rgba8_to_srgb`):**不失败**,原样放行原始像素 + warn 日志 | `editing/color.rs`;`viewer_color/render.rs:112-120` |
| ICC 色彩空间与解码缓冲变体不符(如 Gray profile 配 Rgba 缓冲) | `to_srgb_rgba8`/`to_target_rgba8` 按变体分派,不匹配则 `_ => Err`;`project_rgba8_to_srgb` 前置显式判 `color_space`,不能靠类型匹配兜底(否则会误判为"不支持") | `editing/color.rs:198-200,225-231` |
| 动画 WebP 派生查看器色域 | 前置拒绝 `viewer_render_unsupported`(image WebP 解码器对动画只出首帧,派生首帧会误导用户看到静止画面) | `viewer_color/render.rs:66-72` |
| 自定义 ICC 超过 16MB / 非 RGB / 非 DisplayDevice class / 不可变换 | 逐项拒绝对应稳定码,不读入内存做进一步解析(大小检查在 `new_from_slice` 之前) | `viewer_color_commands.rs:194-244` |
| 自定义 ICC 被外部改坏成非 RGB | `resolve_profile` 二次防御性检查(导入时已探针,此处兜底) | `viewer_color/target.rs:100-106` |
| target=custom 但 custom_id 空或非法 hex | 优雅回退直显原图(`None`),**不落错误码** | `viewer_color/target.rs:49-52` |
| 删除当前选中的自定义 ICC | 复位 `config.viewer_color_target = "srgb"` + 清 `viewer_color_custom_id`(best-effort,写失败仅告警;此时文件已删,后续 `resolve_profile` 会自然返回 `icc_not_found` 供前端回退) | `viewer_color_commands.rs:352-362` |
| **WebView2 嵌 P3 ICC 真机限制** | 待核实(需真机验证 WebView2 是否正确读取嵌入 ICC 做 target→display 映射;若不成立,派生图对广色域屏的意义仅限于"色域裁剪已发生"而非"显示端二次映射") | 归属 GUI 真机验收项,未见仓内自动化测试覆盖此假设 |
| 单文件 ingest 失败但文件已落盘 | 返回**成功**响应 `EditSaveResult::SavedNeedsIndex`(非普通失败),携带 `path_hint`/`recovery_code`/`root_id`,前端提示"文件已保存,索引失败"+提供"立即扫描",不诱导用户重复保存副本 | `edit_commands.rs:56-69,369-379` |
| 源文件已删除/不在线/格式不支持编辑 | 分别拒 `edit_source_unavailable`/`edit_format_unsupported`,预览与保存两条路径独立校验(代码重复但语义须保持一致) | `edit_commands.rs:233-245,390-402` |
| 编辑并发文件任务冲突(与 backup/export 互斥) | 拒 `file_job_busy`,RAII 守卫保证释放 | `edit_commands.rs:160-169` |
| 无授权调用编辑保存/预览 | 后端真门放在阻塞主链入口(`entitlement::require_editing_entitlement`),绕过前端直接 invoke 也无法使用;拒 `edit_not_entitled` | `edit_commands.rs:218-220,383-384` |
| DynamicImage 未来新增变体(non_exhaustive) | 几何/调色两处兜底:先升 RGBA16 再走对应路径,**不静默退化 8-bit** | `editing/geometry.rs:215-225`,`editing/adjust.rs:354-374,454-465` |

## 6. 重建指引(从零实现)

### 依赖顺序

1. 先建 `editing::geometry`(纯函数,不碰 IO/DB,可用小图逐像素单测穷举)——几何链是后续所有步骤的基础。
2. 再建 `editing::color`(moxcms 变换原语),因为 `adjust`/查看器渲染都要复用它。
3. `editing::adjust` 依赖 `color`。
4. `editing::metadata`/`memory_budget`/`naming`/`io` 相对独立,可并行。
5. `editing::ingest` 依赖 DB 层(`db::queries::upsert_fast_scan_item`)。
6. `ipc::edit_commands` 编排以上全部,是最后一层。
7. `viewer_color::target` → `viewer_color::render`(依赖 `editing::color`/`editing::memory_budget`/`editing::metadata`)→ `ipc::viewer_color_commands`。

### 外部 crate / npm 包

| 依赖 | 版本 | 用途 |
|---|---|---|
| `image` | 0.25 | JPEG/PNG/WebP/BMP/TIFF 解码编码,ICC/EXIF 元数据钩子 |
| `imageproc` | 0.27 | `rotate_about_center_no_crop` 精细拉直算子 |
| `moxcms` | 0.8.1 | 纯 Rust ICC CMS 色彩变换(生产实现唯一依赖;`lcms2` 仅测试内做参考对拍) |
| `fast_image_resize` | 4 | E0 编辑预览的 SIMD 缩放(`Resizer` + `ResizeAlg::Convolution(FilterType::Bilinear)`) |
| `kamadak-exif`(`exif` crate) | 0.5 | 只读 EXIF 解析,扫描期 enrichment 与编辑期读取共用 |
| `quick-xml` | 0.36 | XMP Motion Photo 标记检测(字符串匹配,非结构化 XML 解析) |
| `xxhash_rust`(xxh3) | — | 自定义 ICC profile id 哈希(与缩略图 `cache_key` 同哈希族) |
| 前端 `cropperjs` | 2.1.1 | E1 裁剪交互(前端,详见 [Spec11](./Spec11_前端架构.md)) |

### 坑与教训

- 内存基准探针在测量进程内合成源图会被 allocator 复用页面导致读数系统性腰斩(见 `memory_budget.rs:8-13`);两段进程法(生成文件 + 只读文件测量)才是可信基线。参见 `docs/experience.md`(具体条目待核实——本篇未逐条核实与该记忆条目的对应关系)。
- moxcms `encode()` 的时间戳区(24..36 字节)会导致逐字节对拍偶发失败,断言前必须清零(见 §4.3)。
- `DynamicImage` 是 `non_exhaustive`,任何按变体分派的 `match` 都需要显式的"未知变体升位深"兜底分支,不能让 `_ => ...` 静默退化精度。

### 验收

- 单元测试文件(与源码同目录 `#[cfg(test)] mod tests`):`editing/geometry.rs`(几何链穷举,含黄金向量 `src/fixtures/straightenGeometryGolden.json`)、`editing/adjust.rs`(调色公式黄金向量 `src/fixtures/adjustGolden.json`,与 `src/composables/adjustFormula.ts` 双端同源)、`editing/color.rs`(moxcms vs lcms2 对拍,容差 ≤1-2 code value)、`editing/io.rs`(落盘/tmp 清理)、`editing/naming.rs`(TOCTOU/冲突递增)、`editing/ingest.rs`(内存 DB upsert 语义)、`viewer_color/target.rs`(`from_config` 全分支)、`viewer_color/render.rs`(D-412 特征测试:嵌入 ICC 字节级相等 + lcms2 参考容差 ≤2)。
- Gate 命令:`cargo test`(项目级,无子系统专属 gate;涉及 Rust 图像/色彩逻辑变更后至少跑受影响 crate 的 `cargo test -p <crate>` 或全量,依变更面而定,见 AGENTS.md「验证分层」)。
- 前端调色/几何公式对拍靠共享 JSON 黄金向量文件,双端任一侧改公式必须同步更新向量并让两侧测试都过。

## 7. 关联

- 上游正典:[Part3 §缩略图派生](../refactor_2026/Part3_缩略图派生与GPU引擎.md)(**无色彩管理章节**——本篇色彩管理部分为该空白的净新增,不存在"Part3 计划为 X、当前实现为 Y"的对照关系;Part3 涉及的缩略图 WebP/sprite 生成机制归 [Spec03](./Spec03_缩略图与派生.md))。
- 相关设计:`docs/designs/2026-07-19-图片编辑开源包混合升级方案.md`(方案 C,几何/调色/落盘契约的原始设计出处,D-号裁决多源于此);ICC 色域线设计文档(B 线,2026-07-23,查看器渲染色域——具体文件名待核实,仓内 `docs/designs/` 目录下未逐一核实确切文件名,以本篇 path:line 引用的代码注释为准)。
- 缩略图缩放/生成/LRU 缓存:[Spec03_缩略图与派生](./Spec03_缩略图与派生.md)。
- 视频旋转元数据(`video_meta.rotation`)与视频处理管线:[Spec05_视频与音频](./Spec05_视频与音频.md)。
- 前端 `EditOverlay`/`useEditPreview`/`ContentViewer`/`adjustFormula.ts`:[Spec11_前端架构](./Spec11_前端架构.md)。
- IPC 命令全表/错误码全表:[Spec10_IPC与错误契约](./Spec10_IPC与错误契约.md)。
- 数据表全量 schema:[Spec01_数据层](./Spec01_数据层.md)。
- 授权/exotic 平台(`EntitlementProvider`/`PluginEntitlement`):[Spec09_插件平台与exotic](./Spec09_插件平台与exotic.md)。
- 配置键值真源(`viewer_color_target`/`viewer_color_custom_id`):[Spec12_配置状态日志](./Spec12_配置状态日志.md)。
