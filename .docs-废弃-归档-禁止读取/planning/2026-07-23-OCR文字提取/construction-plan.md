---
id: 2026-07-23-construction-plan
status: active
type: plan
line: OCR文字提取
created: 2026-07-23
---

# OCR 文字提取(B′ 路线)施工计划

> architect 产出(23 轮),主线复核后落盘。任务卡 T1–T11 + 五波并行分组。
> 前提:D-418..421 已拍板,本计划不复议。

## 主线复核修正(优先级高于下文正文,冲突处以本节为准)

1. **cls 模型不共用(与 model-assets.md 对齐)**:PP-OCRv5 mobile/server 各带专属 textline 方向分类模型(`ch_PP-LCNet_x0_25_textline_ori_cls_mobile` / `ch_PP-LCNet_x1_0_textline_ori_cls_server` 系),已非旧版 `ch_ppocr_mobile_v2.0_cls`,两档各钉各的。据此:
   - T2 `ocr_profile.rs`:`cls_file` 两档互异;仅 `dict_file` 共用。`cls_input_hw` 按档位填,取值以 `model-assets.md` 载明为准;若未载明,以 PaddleOCR 官方该模型配置为准并在注释标注出处,golden 对拍最终定案。
   - T4 单测「两档 cls/dict dest 相同」改为「仅 dict dest 相同,cls 互异」。
   - 下文 T2 默认文件名清单中 cls 行以本节为准;最终文件名仍以 model-assets.md 为单源。
2. **锚点漂移**:recon-codebase.md 的编辑按钮锚(180-186)已漂移,现行 ContentViewer.vue:197-203;本计划行号为 architect 今日实测,implementer 以本计划为准,再漂移以语义锚(代码内容)定位。

---

## 0. 从属决策(实施中不再摸底)

- **D-OCR-1**:OCR 会话独立于 CLIP 会话——协议新增 `OcrSessionInit/OcrSessionClose/OcrBatch` 三个 additive op(不动 PROTOCOL_VERSION,先例 EncodeText),worker 内 CLIP 与 OCR 两个会话槽并存互不干扰。不复用 `SessionInit`(它把 CLIP 双塔钉为必备角色,会迫使 OCR 用户先装 370MB CLIP)。
- **D-OCR-2**:一期 OCR 三模型 **EP 恒 CPU**。理由:v5 rec 是 SVTR(transformer 系),与 engine.rs 坑9(DirectML 对 BERT int64 Gather 静默算错)同型风险;模型小(mobile 档合计 20-30MB),CPU 延迟可接受。DirectML 待二期对拍后再启。
- **D-OCR-3**:视频帧 PNG 不走协议帧 blob(supervisor 发送面 `Frame::control` 无 host→worker blob 通道,见 `src-tauri/src/exotic/worker.rs:240`),host 落临时 PNG(`*.tmp`+同卷 rename)后以 `source_path` 派给 worker,用完即删。
- **D-OCR-4**:OCR 结果(文本+四点框+置信度)走 Success 帧 **JSON**(结构化小数据,先例 FaceDet 几何走 JSON),blob 恒空;worker 侧设逐项行数/字节 cap 防撞 MAX_JSON_LEN。
- **D-OCR-5**:商店门控走 catalog offering 新增 `distribution: "builtin"` 语义——无安装包,`availability_of` 对 builtin 跳过安装态检查,直接验 license(无 token → AvailableUninstalled,有效 → Authorized)。不依赖 Part8 私有 registry 发空包。
- **D-OCR-6**:门控 UX 采用「按钮常显 + 未授权点击引导」:未授权 → toast + 跳插件商店;模型未下载 → toast + 跳设置页 OCR 分节。不隐藏按钮(保可发现性,与 edit 按钮「显式不支持文案」哲学一致,见 ContentViewer.vue:195-196 注释)。
- **D-OCR-7**:一期不做大图 tile 切片、不做 OCR 结果 DB 持久化、不做批量 OCR 任务化;det `limit_side_len` mobile=960 / server=1280 作为大图精度的一期答案。

---

## 1. 改动清单(任务卡 T1–T11;T8 并入 T7)

### T1 — exotic-protocol:OcrBatch 协议族(v2 additive)

**文件:`crates\exotic-protocol\src\message.rs`**

1. `RequestBody` enum(:35-87)新增三个 variant(插在 `EncodeText` 之后,`snake_case` tag 自动得 `"ocr_session_init"/"ocr_session_close"/"ocr_batch"`):
   ```rust
   /// OCR 会话装载(v2 additive):独立于 CLIP 会话(D-OCR-1),worker 双槽并存。
   /// 响应 = Success + SuccessBody.ocr_session。
   OcrSessionInit {
       session_id: u64,
       /// 角色 OcrDet/OcrCls/OcrRec/OcrDict 四件套,逐件 len+sha256(同 D1 §3)。
       models: Vec<ModelDescriptor>,
       /// worker 按 profile_id 从 scrollery-ai-core::ocr_profile 内建注册表取几何/阈值/文件名契约。
       ocr_profile_id: String,
       models_root: String,
   },
   OcrSessionClose { session_id: u64 },
   /// OCR 批(交互一次一图为主,批口面向未来)。响应 = Success + SuccessBody.ocr,blob 恒空(D-OCR-4)。
   OcrBatch { items: Vec<OcrItem> },
   ```
2. `ModelRole`(:141-146)新增 `OcrDet, OcrCls, OcrRec, OcrDict` 四变体(snake_case 序列化)。
3. 新增结构体(仿 FaceItem/FaceBatchSuccess 区块 :187-332):
   ```rust
   pub struct OcrItem { pub item_id: i64, pub cache_key: Option<String>, pub source_path: Option<String>, pub fingerprint: String }
   pub struct OcrSessionReadyBody { pub caps: Vec<String> }   // 恒含 "ocr_text"
   pub struct OcrBatchSuccess { pub results: Vec<OcrItemResult> }
   #[serde(tag = "status", rename_all = "snake_case")]
   pub enum OcrItemResult {
       Ok { item_id: i64, fingerprint: String, lines: Vec<OcrLine>, width: u32, height: u32 },  // width/height=解码图实际尺寸(quad 坐标系)
       Err { item_id: i64, fingerprint: String, code: WorkerErrorCode },
   }
   pub struct OcrLine { pub text: String, pub quad: [[f32; 2]; 4], pub confidence: f32 }
   ```
4. `SuccessBody`(:210-232)新增两个 `#[serde(default, skip_serializing_if = "Option::is_none")]` 字段:`pub ocr_session: Option<OcrSessionReadyBody>`、`pub ocr: Option<OcrBatchSuccess>`(线上形状不变性由既有测试 `success_body_thumbnail_wire_shape_unchanged` :567-593 扩两行断言保住)。
5. `capability` mod(:198-203)加 `pub const OCR_TEXT: &str = "ocr_text";`。
6. `RequestBody::item_id()`(:92-103)与 `input_fingerprint()`(:106-120)为三个新 variant 补 `=> None` 臂(批量 op 无单项语义,同 EmbedBatch)。
7. 测试(:427 起)新增:三 op tag 往返(`"op":"ocr_batch"` 等)、`OcrItemResult` status tag 往返、`OcrLine` 序列化形状、`capability::OCR_TEXT` 字面量锁、SuccessBody 新字段 None 不出线 + 旧 JSON 可解析、新 ModelRole 四变体 serde tag。

**版本兼容注意**(写进 variant rustdoc):additive 不动 `PROTOCOL_VERSION`;两端同仓同步分发,旧 worker 收到新 op 会 JSON 解析失败回 `internal_error`(main.rs:254-274 既有兜底),host 侧 ocr 路径只在新 worker 存在时可达,无实际混版窗口。

---

### T2 — scrollery-ai-core:OCR 管线自研

**文件:`crates\scrollery-ai-core\Cargo.toml`**
- 加依赖 `imageproc`(轮廓/最小外接旋转矩形/透视 warp;版本对齐仓内 `image` crate 的 major——先查 Cargo.lock 中 image 版本,imageproc 0.25 对 image 0.25),归入 `inference` feature 的依赖组(与 ort/ndarray 同门控)。

**文件:`crates\scrollery-ai-core\src\lib.rs`(:24-40)**
- 注册模块:`pub mod ocr_profile;`(纯契约,不门控——src-tauri 以 default-features=false 消费,同 face_profile 模式);`#[cfg(feature = "inference")] pub mod ocr;`。

**新文件:`crates\scrollery-ai-core\src\ocr_profile.rs`**(纯数据,零 ort;仿 face_profile.rs)
```rust
pub struct OcrProfile {
    pub id: String,                    // "pp-ocrv5-mobile" | "pp-ocrv5-server"
    pub display_name: String, pub description: String,
    pub det_file: String, pub cls_file: String, pub rec_file: String, pub dict_file: String,
    pub det_limit_side_len: u32,       // mobile 960 / server 1280(D-OCR-7)
    pub det_thresh: f32,               // 0.3
    pub det_box_thresh: f32,           // 0.6
    pub det_unclip_ratio: f32,         // 1.5
    pub det_min_box_side: f32,         // 3.0(原图坐标像素)
    pub cls_thresh: f32,               // 0.9
    pub cls_input_hw: (u32, u32),      // 按档位填,见主线修正节
    pub rec_input_h: u32,              // 48
    pub rec_max_w: u32,                // 3200(安全帽)
    pub rec_min_conf: f32,             // 0.5(低于即丢行)
    pub max_lines_per_image: usize,    // 1000
    /// 喂模型前是否 R/B 互换(PaddleOCR DecodeImage img_mode=BGR 惯例)。
    /// 默认 true;golden 对拍测试若证明 RGB 才对,单点改此处(见边界1)。
    pub swap_rb: bool,
    pub size_mb: u32,
}
pub const DEFAULT_OCR_PROFILE_ID: &str = "pp-ocrv5-mobile";
pub fn ocr_profiles() -> Vec<OcrProfile>;          // 两档;cls_file 两档互异(主线修正节),dict_file 共用
pub fn find_ocr_profile(id: &str) -> Option<OcrProfile>;
```
文件名缺省值:det `ch_PP-OCRv5_mobile_det.onnx` / `ch_PP-OCRv5_server_det.onnx`、cls 两档专属 PP-LCNet textline_ori 模型(名以 model-assets.md 为准,见主线修正节)、rec `ch_PP-OCRv5_mobile_rec.onnx` / `ch_PP-OCRv5_server_rec.onnx`、dict `ppocrv5_dict.txt`(共用)。注释注明:**最终文件名以 model-assets.md 为单源,改名只动本文件单点**(host 描述符、worker 校验、下载清单全部从这里取名)。

**文件:`crates\scrollery-ai-core\src\engine.rs`**
- `fn load_session_pool`(:527)从模块私有改 `pub(crate)`(ocr 模块复用它的池装载+超时+进度姿态);`fn build_session`(:656)不动。

**新文件(均 `inference` 门内):**

`crates\scrollery-ai-core\src\ocr\mod.rs` — 编排:
```rust
pub struct OcrEngine { det: SessionPool, cls: SessionPool, rec: SessionPool, dict: OcrDict, pub profile: OcrProfile }
impl OcrEngine {
    /// CPU EP(D-OCR-2),三池各容量 1(交互单发,控内存)。任一模型缺失/加载失败 → Err(会话语义:声明即必须就绪,同 session.rs:199-205)。
    /// 加载后做 dict 契约自检(见 dict.rs;类比 clip 坑8 vocab 校验)。
    pub fn init(models_dir: &Path, profile: &OcrProfile) -> Result<Self, AiError>;
    /// det→(crop 逐框)→cls→rec 全链;输出行按阅读序。
    pub fn recognize(&self, img: &image::DynamicImage) -> Result<OcrOutput, AiError>;
}
pub struct OcrOutput { pub lines: Vec<OcrLineOut>, pub width: u32, pub height: u32 }
pub struct OcrLineOut { pub text: String, pub quad: [[f32; 2]; 4], pub confidence: f32 }
```
错误:复用 `crates\scrollery-ai-core\src\error.rs` 的 `AiError` 既有变体;若无贴切变体则加一个 `#[error("OCR error: {0}")] Ocr(String)`(thiserror,域错误惯例)。

`ocr\det.rs` — 检测前处理+DB 后处理(参数全部从 profile 取):
- 前处理:RGB8 化 → 按 `swap_rb` 决定通道序 → `scale = min(1.0, limit_side_len / max(h,w))` → 目标边 `max(round(edge*scale/32),1)*32` resize(bilinear)→ `[1,3,H,W]` f32,`(px/255 - mean)/std`,mean `[0.485,0.456,0.406]` std `[0.229,0.224,0.225]`(与通道序同序应用);记录 `ratio_h/ratio_w` 供坐标回映。输入张量名从 `session.inputs()[0].name()` 动态取(仿 CLIP 兜底哲学,profile.rs:76-78 注)。
- 后处理(输出 `[1,1,H,W]` 概率图):`mask = p > det_thresh` → `imageproc::contours::find_contours` → 每轮廓:`imageproc::geometry::min_area_rect` 得四角 quad;短边 < `det_min_box_side*scale` 弃;score = quad 轴对齐包围盒内概率均值(fast box_score),`< det_box_thresh` 弃;**unclip**:`d = area(quad) * unclip_ratio / perimeter(quad)`,四条边各沿外法向平移 d,相邻平移线求交得新角(确定性四边形外扩,不引 clipper);clamp 到图内 → 除以 ratio 回原图坐标。
- 排序:按 quad 质心 y 分行(容差 = 框高中位数一半),行内按 x 升序。

`ocr\geometry.rs` — `get_rotate_crop_image`:目标宽=上下边长均值取整、高=左右边长均值取整;`imageproc::geometric_transformations::Projection::from_control_points`(quad→矩形四角)+ `warp`(bilinear)裁出;若 `h/w ≥ 1.5` 顺时针旋 90°(竖条转横)。

`ocr\cls.rs` — 方向分类:每 crop 等比缩至 profile.cls_input_hw 尺寸(左对齐零填充),`(px/255-0.5)/0.5`;**逐张跑**(cls 极小,毫秒级,不组批);输出 `[1,2]`,`argmax==1 && prob ≥ cls_thresh` → crop 旋 180°。

`ocr\rec.rs` — 识别 + CTC:每 crop `w' = min(rec_max_w, ceil(w·48/h))` 等比缩至 48×w',`(px/255-0.5)/0.5` CHW `[1,3,48,w']`,逐张跑(一期不组批,见已弃方案);输出 `[1,T,C]`;CTC 贪心:逐 t argmax(idx,prob),折叠连续重复、丢 blank(idx 0),字符 = `dict.char(idx)`;conf = 保留字符 prob 均值;文本空或 `conf < rec_min_conf` 丢行。

`ocr\dict.rs` — 字典:UTF-8 逐行读(容 BOM;空行保留为合法字符行,行尾仅去 `\r\n`);映射契约:模型输出 idx 0 = blank,idx 1..=N → dict 行 0..N-1;**契约自检**(engine init 时或首次推理时):rec 输出末维 C 必须 == dict.len()+1(无空格)或 dict.len()+2(use_space_char,此时 idx N+1 = `" "`),否则报模型/字典错配错误——这是坑8(词表错配→输出乱码)的 OCR 同型防线。

**单测(纯逻辑,CI 可跑,不依赖模型文件):**
- det.rs:合成概率图(全 0 中画一块 p=1 矩形)→ 断言产出 1 框、坐标±2px、unclip 外扩后大于原矩形;resize 几何(32 倍数、ratio 正确);排序(两行三框乱序输入 → 阅读序)。
- geometry.rs:合成纯色四边形图 warp → 断言目标尺寸与角点像素颜色;h/w≥1.5 旋转分支。
- cls.rs:合成 logits → 阈值/旋转决策表。
- rec.rs:合成 `[1,T,C]` logits → CTC 折叠/去 blank/conf 均值/低 conf 丢行,含「全 blank → 丢行」。
- dict.rs:临时字典文件 → 索引映射、C=N+1 与 N+2 两态、错配拒绝、BOM 容错。
- ocr_profile.rs:两档存在、det/cls/rec 文件名互异、dict 共用、DEFAULT 可解析。

**模型级 characterization(`#[ignore]`,本机手跑非 CI):**
- 新增 `crates\scrollery-ai-core\tests\ocr_golden.rs`:`#[ignore]` 测试,env `OCR_MODELS_DIR` 指向已下载模型目录,`tests/fixtures/ocr/` 检入 2 张小 PNG(一张中英混排印刷体、一张含 180° 倒置行,各 ≤100KB);断言识别文本包含预期子串。**这是通道序(swap_rb)与归一化参数的定案测试**——若断言不过,翻转 `swap_rb` 重跑,以过者为准回写 profile 默认值并在注释记录定案依据。

**速度基准(回答「官方档位差 30 倍不可信」):**
- 同文件加 `#[ignore] fn ocr_bench()`:env `OCR_MODELS_DIR` + `OCR_BENCH_IMG`(任选本机一张 1080p~4K 含文字图),对每档 profile 各跑 6 次弃首轮,打印 det/cls/rec 分段与端到端中位数 ms。运行:
  `cargo test -p scrollery-ai-core --release --features inference --test ocr_golden -- --ignored ocr_bench --nocapture`
- 实测数字回写三件套 findings;若 mobile 档端到端中位数 > 2s/图,把 server 档 UI 文案从「高精度可选」调为「高精度(慢)」并复议默认档(记录裁决,不阻塞施工)。

---

### T3 — exotic 门控:builtin offering + catalog 条目

**文件:`src-tauri\resources\exotic-catalog.json`(:4-18)**
- `offerings[]` 追加:
  ```json
  {
    "plugin_id": "exotic-ocr",
    "name": "OCR 文字提取",
    "media_kind": "image",
    "formats": ["ocr"],
    "capabilities": ["text"],
    "license_tier": "paid",
    "sku": "ocr-engine-2026",
    "platforms": ["x86_64-pc-windows-msvc"],
    "min_host_version": "0.1.0",
    "override_common": false,
    "store_url": "https://example.invalid/plugins/ocr",
    "distribution": "builtin"
  }
  ```
  (`store_url` 占位与 PSD 同姿态,上线前随商店域名统一替换;`"ocr"` 非真实文件扩展名,`is_valid_format` catalog.rs:323 的字符集校验可过,且永不与磁盘扫描的真实格式相遇,不进缩略图 router / useExoticGate 格式集的判定路径。)

**文件:`src-tauri\src\exotic\catalog.rs`**
- `RawOffering`(:112-136)加 `#[serde(default)] distribution: Option<String>`;`CatalogOffering`(:140-157)加 `pub builtin: bool`(装配时 `distribution.as_deref() == Some("builtin")`;未知值按 package 处理并 warn)。
- 测试区(:347-370)加:builtin 条目解析 → `off.builtin == true`;缺省 → false。

**文件:`src-tauri\src\exotic\mod.rs`**
- `availability_of`(:386-423):在 `let Some(rec) = installed else { return AvailableUninstalled }`(:402-404)**之前**插入 builtin 分支:
  ```rust
  // builtin offering(D-OCR-5):无安装包,跳过安装态门,直接验 license。
  if off.builtin {
      let Some(sku) = off.sku.as_deref() else { return Availability::InstalledUnlicensed; };
      return match self.licenses.evaluate(&off.plugin_id, sku, now_secs()) {
          LicenseStatus::Authorized => Availability::Authorized,
          LicenseStatus::Expired => Availability::LicenseExpired,
          LicenseStatus::Unlicensed | LicenseStatus::KeyringUnavailable => Availability::AvailableUninstalled,
      };
  }
  ```
  (平台/Host 版本/dev fixture 门保持在前——fixture `PICASA_EXOTIC_DEV_FIXTURE`/`with_authorized_fixture` :398-401 对 OCR 同样生效,dev E2E 免真 token。)
- `FormatResolution`(:72-82)加 `pub builtin: bool` 字段;`resolve_format`(:356-382)两处构造点填值(NoOffering 臂填 false)。
- mod.rs 既有 availability 测试矩阵为 builtin 加三例:无 token→AvailableUninstalled、fixture→Authorized、expired→LicenseExpired。

**激活链零改动**:`activate_exotic_plugin`(`src-tauri\src\ipc\exotic_commands.rs:346-381`)按 plugin_id 从 catalog 取 sku 验签存 keyring,不依赖安装行,对 builtin 直接可用。

---

### T4 — OCR 模型元数据 + 下载清单(URL 占位)

**新文件:`src-tauri\src\ai\ocr_registry.rs`**(仿 profile.rs 的 `static_fp16_b16_assets` :316-340 静态清单姿态,不走 HF tree 动态发现):
```rust
/// OCR 资产下载基址。None = 资产 URL 尚未钉定,下载命令回稳定码 ocr_manifest_unready。
/// 钉定后填 Some((primary_base, Some(mirror_base))),四文件清单即活。
pub const OCR_ASSET_BASE: Option<(&str, Option<&str>)> = None;

/// 某档位的完整下载清单(det+cls+rec+dict 四件;文件名/dest 单源取自 ocr_profile)。
/// size_bytes/sha256 随资产钉定回填(先 0/None;model_download 对 None sha 仅按大小校验,0 大小跳过校验)。
pub fn ocr_assets(profile: &scrollery_ai_core::ocr_profile::OcrProfile) -> Option<Vec<ModelAsset>>;

/// 档位安装判定:models_dir 下四文件均存在且非空(sha 深校验留给 worker OcrSessionInit)。
pub fn ocr_tier_installed(models_dir: &Path, profile: &OcrProfile) -> bool;
```
- `src-tauri\src\ai\mod.rs`:注册 `pub mod ocr_registry;`(先 `Grep "pub mod remote_registry" src-tauri/src/ai/mod.rs` 找同款声明行邻接插入)。
- 单测:`ocr_assets` 在 BASE=None 时回 None;伪 BASE 下四条 dest 与 profile 文件名一致、两档仅 dict dest 相同(主线修正节)。

**协调点**:`OCR_ASSET_BASE`、四文件 `size_bytes/sha256` 的回填靶点=model-assets.md;本计划所有其它部分不依赖其值(fail-closed:未钉定时设置页显示「清单未就绪」)。

---

### T5 — ai-worker:OCR 会话与批处理

**新文件:`crates\exotic-workers\ai-worker\src\ocr.rs`**
- `pub struct OcrSessionState { pub session_id: u64, pub engine: scrollery_ai_core::ocr::OcrEngine }`
- `pub fn validate_ocr_init(session_id, models: &[ModelDescriptor], ocr_profile_id, models_root) -> Result<(OcrProfile, PathBuf), InitError>`:镜像 session.rs `validate_and_resolve`(:60-133)——canonicalize root、`find_ocr_profile` 解析、期望角色表 `[(OcrDet,det_file),(OcrCls,cls_file),(OcrRec,rec_file),(OcrDict,dict_file)]` 四件必备、复用 `verify_descriptor`(session.rs:136-180,**改 `pub(crate)`**)做归属/文件名/len/sha256 校验、重复角色拒(:110-113 姿态)、不完备拒(:116-122)。
- `pub fn handle_ocr_batch(sess: &OcrSessionState, request_id: u64, items: &[OcrItem]) -> Frame`:
  - `const MAX_OCR_ITEMS: usize = 8;` 超限 → 整批 `MalformedInput`(仿 batch.rs:139-149)。
  - 逐项(串行即可,交互场景一批一图):图像加载镜像 `load_face_image`(batch.rs:380-403)但返回 `DynamicImage`——cache_key 优先(`cache_webp_path` batch.rs:110 复用)、source_path 回退(stat 拦 `MAX_SOURCE_FILE_BYTES` 512MB :27)、都缺 = MalformedInput;`engine.recognize` → Ok{lines,width,height};错误映射:读文件失败→IoError、解码失败→MalformedInput、推理失败→InternalError(逐项 Err **不连坐**,batch.rs 模块头契约)。⚠batch.rs 是他线 dirty 文件:优先不改它;若复用需改可见性,改为在 ocr.rs 本地最小复制该 helper 并注明镜像来源,勿动 batch.rs。
  - 逐项防御 cap:行数 > `max_lines_per_image` 截断+`log_warn`;单项文本总字节 > 512KB → 该项 `ResourceLimit`。
  - 装配 `SuccessBody { ocr: Some(OcrBatchSuccess{results}), ..Default::default() }`,`Frame::with_blob(...,Vec::new())`(blob 恒空,D-OCR-4);终帧前防御:JSON 序列化长度 > 900KB → 整批 `ResourceLimit`(理论不可达,双保险)。
  - 批诊断一行 `log_info`(项数/总行数/墙钟 ms,仿 batch.rs:540-551)。
- 单测:validate_ocr_init 的 happy/越界/sha 不符/角色缺/角色重复(照抄 session.rs tests :232-417 的 temp_dir/lay_model 手法,角色换 OCR 四件)。

**文件:`crates\exotic-workers\ai-worker\src\main.rs`**
- `mod ocr;`(:15-16 处)。
- 主循环状态(:236)`let mut sess` 旁加 `let mut ocr_sess: Option<ocr::OcrSessionState> = None;`,`handle_request` 签名(:543)加 `ocr_sess: &mut Option<...>` 参数(调用点 :284 同步)。
- `handle_request`(:543-599)新增三臂:
  - `OcrSessionInit` → **先 `preflight_ort_runtime(ORT_PREFLIGHT_TIMEOUT)`**(:49 常量复用;失败按 :406-408 的错误码映射回 Failure)→ `validate_ocr_init` → `OcrEngine::init` → 成功置 `*ocr_sess`、回 `Success{ocr_session:Some(OcrSessionReadyBody{caps:vec![capability::OCR_TEXT.into()]})}`;失败回 `ModelLoadFailed`(terminal)。同步处理、不发 Progress(CPU EP + 小模型,秒级;host 侧超时 180s 兜底,见 T6)。已有 OCR 会话时按切换语义先 drop 旧(仿 :378-385)。
  - `OcrSessionClose` → `ocr_sess.take()` 幂等回 Success(仿 :556-571)。
  - `OcrBatch` → `ocr_sess` 为 None 回 `session_expired(request_id)`(:602-611 复用),否则 `ocr::handle_ocr_batch`。
- `ReadyBody.capabilities`(:221-224)追加 `capability::OCR_TEXT.to_string()`。
- 注意 `session_caps`(:614-620)只描述 CLIP 会话,不动。

---

### T6 — 宿主推理通路:worker_client + validate + 超时表

**文件:`src-tauri\src\exotic\validate.rs`**
- 邻接 `validate_face_batch_output`(:178)新增:
  ```rust
  pub enum OcrItemOutcome { Ok { lines: Vec<exotic_protocol::OcrLine>, width: u32, height: u32 }, Err { code: WorkerErrorCode } }
  pub fn validate_ocr_batch_output(items: &[OcrItem], body: &SuccessBody, blob: &[u8]) -> Result<Vec<OcrItemOutcome>, String>
  ```
  校验(「不信任 worker」纪律):`body.ocr` 必在;`results.len() == items.len()`;逐项 item_id+fingerprint 与请求同序一致;Ok 项 width/height > 0、quad 8 个坐标 finite 且 ∈ [-1e4, 1e5]、`0.0 ≤ confidence ≤ 1.0`;`blob.is_empty()` 否则违例。任一不符回 Err(String)(触发 run_validated 弃实例重发)。
- 测试:合法通过/长度不符/id 错位/blob 非空/NaN quad/conf 越界各一例(仿本文件既有 validate_* 测试)。

**文件:`src-tauri\src\exotic\coordinator.rs`(op_timeouts mod :44-69)**
- 加 `pub const OCR_SESSION_INIT: Duration = Duration::from_secs(180);`(flat 无 Progress 心跳,须容 server 档冷载)与 `pub fn ocr_batch(items: usize) -> Duration { 60s + 30s*items }`;测试区(:569-588)补 `op_timeouts::ocr_batch(8) < PROGRESS_TOTAL_CAP`、`OCR_SESSION_INIT < PROGRESS_TOTAL_CAP` 断言。

**文件:`src-tauri\src\ai\worker_client.rs`**
- 新类型(SessionSpec :55-67 旁):`pub struct OcrSessionSpec { pub profile: scrollery_ai_core::ocr_profile::OcrProfile, pub models_dir: PathBuf }` + 组装函数 `pub fn build_ocr_session_spec(state) -> Result<OcrSessionSpec>`(读 config 键 `ocr_active_tier` 缺省 `DEFAULT_OCR_PROFILE_ID`,models_dir 取法同 `build_session_spec` :533)。
- `AiWorkerClient`(:111-116)加字段 `ocr_loaded: Option<String>`(在载 OCR profile_id;`with_spawner` :125-132 初始化 None)。
- **失效点**:`drop_worker`(:144-148)与 `ensure_worker` 重建路径(:151-161 spawn 成功后)各加 `self.ocr_loaded = None;`(worker 进程换代 = OCR 会话蒸发)。
- 新私有 `fn ensure_ocr_session(&mut self, spec, cancelled) -> Result<(), EnsureError>`:`ensure_worker` → `ocr_loaded == Some(profile.id)` 即零帧返回;否则组装 `OcrSessionInit`(descriptors 用既有 `model_descriptor(role, path, sha_cache)` :493 复用 sha 备忘,四角色四文件)→ 经 `EmbedWorker::run_batch`(worker_traits.rs:51-56,底层 `run_request` 通吃任意 RequestBody)以 `OCR_SESSION_INIT` 超时发送 → Success 且 `body.ocr_session` 有值即 `self.ocr_loaded = Some(id)`;Failure/三态错误按 `ensure_session`(:168-220)同款二分(ModelLoadFailed=Terminal、TimedOut/Disconnected/Protocol=Process)。
- 新公有 `pub fn ocr_batch(&mut self, spec: &OcrSessionSpec, items: &[OcrItem], cancelled) -> Result<Vec<OcrItemOutcome>>`:attempt 圈仿 `run_validated`(:223-320)——每轮 `ensure_ocr_session`;发 `RequestBody::OcrBatch`,超时 `op_timeouts::ocr_batch(items.len())`;Success → `validate_ocr_batch_output`,违例弃实例重试;`SessionExpired` → `self.ocr_loaded = None` 后重试(worker 空闲自杀/换代场景);retryable Failure 退避重发;terminal 直返;MAX_ATTEMPTS=2 硬止损。**不复用 `run_validated`**(它的 ensure 是 CLIP 会话)——新循环独立,注释注明与 run_validated 的同构关系。
- Mock 测试(:561 起 tests mod):仿既有 mock EmbedWorker 注入,覆盖:首发 init+batch 成功;worker 换代后 `ocr_loaded` 复位重 init;SessionExpired 重建一次;validate 违例弃实例重发。

---

### T7 — IPC 命令层 + AppError + 常量注册(T8 并入本卡)

**文件:`src-tauri\src\error.rs`**
- `AppError` enum(Exotic 变体 :90-91 邻接)新增:
  ```rust
  /// OCR 错误。code 稳定小写(前端分流,不匹配 message):ocr_unlicensed / ocr_license_expired /
  /// ocr_unavailable / ocr_model_missing / ocr_manifest_unready / ocr_invalid_input /
  /// ocr_decode_failed / ocr_engine_failed / ocr_worker_failed。
  #[error("ocr error [{code}]: {message}")]
  Ocr { code: &'static str, message: String },
  ```
- `impl Serialize`(:335-461)的 `(code, msg)` match 加 `AppError::Ocr { code, message } => (*code, message.clone())` 臂(照抄 Exotic 臂形状;message 只含展示文案,**不含**绝对路径/worker 内部串——命令层构造时遵守)。
- 测试区(:487 起)加 `ocr_error_surfaces_stable_code`(仿 preview :487-494,遍历全部稳定码)。

**新文件:`src-tauri\src\ipc\ocr_commands.rs`**(四命令,全部 `#[tauri::command]` + async + `spawn_blocking` 惯例,`join_err` 映射照 ai_commands.rs:34-41):
1. `pub async fn ocr_status(state) -> Result<OcrStatusDto>`:spawn_blocking 内(keyring evaluate 是阻塞系统调用)—— `state.exotic_host()`(state.rs:884)`.resolve_format("ocr")` 取 availability;两档 `ocr_tier_installed`;`ocr_active_tier` 配置读。DTO(camelCase serde):`{ availability, storeUrl, activeTier, manifestReady: bool, tiers: [{ id, displayName, sizeMb, installed }] }`。**不带 busy 字段**(交互忙态由前端 useOcr 单源持有,不做后端轮询——OCR 是秒级一次性动作,不套 derivationStore 长任务模式)。
2. `pub async fn ocr_extract_image(item_id: i64, state) -> Result<OcrResultDto>`:
   - spawn_blocking 内:门控 `ensure_ocr_authorized(&state)`(私有 helper:availability 非 Authorized → `AppError::Ocr{code:"ocr_unlicensed"|"ocr_license_expired"|"ocr_unavailable"}`);模型就位检查(`ocr_tier_installed` 不过 → `ocr_model_missing`);按 item_id 查源路径——**复用 reveal/show_in_explorer 命令同款的条目路径查询**(`Grep "MediaNotFound" src-tauri/src/ipc` 定位既有 helper,rusqlite 全参数绑定),canonicalize + 存在性校验;构造 `OcrItem { item_id, cache_key: None, source_path: Some(path), fingerprint: <随机 16hex nonce> }`;`state.ai_worker.lock()`(std Mutex,**全程在 spawn_blocking 内,不跨 await**)→ `ocr_batch(&build_ocr_session_spec(..)?, &[item], &|| false)`;单项 Err 映射:IoError/MalformedInput → `ocr_decode_failed`、ResourceLimit/InternalError → `ocr_engine_failed`;客户端硬止损错 → `ocr_worker_failed`(message 用户级文案,详情留 tracing)。
   - DTO:`{ lines: [{ text, quad: [[f32;2];4], confidence }], width, height }`。
3. `pub async fn ocr_extract_frame(data_base64: String, state) -> Result<OcrResultDto>`:门控同上;base64 解码(上限:解码后 ≤ 64MB,超限 `ocr_invalid_input`);落 `{cache_dir}/ocr_frames/{uuid}.png`(**先 `.tmp` 写再同卷 rename**,目录构造取法同 worker_client `build_session_spec` 的 ai_cache_dir 派生;命令开头顺手清理该目录下 mtime > 1h 的陈旧文件);`OcrItem{ item_id: 0, source_path: Some(tmp_png) }` 走同一 worker 通路;**finally 语义删除该文件**(成功/失败都删,best-effort warn)。
4. `pub async fn download_ocr_models(tier: String, on_progress: tauri::ipc::Channel<DownloadProgress>, state) -> Result<()>`:`find_ocr_profile(&tier)` 不识别 → `ocr_invalid_input`;`ocr_assets(..)` 为 None → `AppError::Ocr{code:"ocr_manifest_unready"}`;其余整段照抄 `download_model`(ai_commands.rs:752-844)/`download_face_model`(face_commands.rs:748 邻域)姿态:`secure_client(TimeoutPolicy::LargeFile)` + `model_download::download_assets`(`.part`+原子改名+断点续传+Channel 进度)。

- **文件:`src-tauri\src\ipc\mod.rs`**:注册 `pub mod ocr_commands;`(邻 ai_commands/face_commands 声明)。
- **文件:`src-tauri\src\ipc\registry.rs`**(:184-196 邻域):invoke_handler 清单加四命令。
- **文件:`src\constants\ipc.ts`**(:254-268 AI 组邻接)新增:
  ```ts
  OCR_STATUS: 'ocr_status',
  OCR_EXTRACT_IMAGE: 'ocr_extract_image',
  OCR_EXTRACT_FRAME: 'ocr_extract_frame',
  DOWNLOAD_OCR_MODELS: 'download_ocr_models',
  ```
- **新类型文件:`src\types\ocr.ts`**:`OcrLine/OcrResult/OcrStatus/OcrTier` 接口(strict,无 any),与后端 DTO camelCase 对齐。
- **capabilities 判定:不需动**——四命令是 app 自有 command(经 invoke_handler 注册,`core:default` 即可),不引入新 Tauri 插件;剪贴板走 `navigator.clipboard`(LogWindowView 先例),非 plugin-clipboard。此结论写进 ocr_commands.rs 模块头注释一行。

---

### T9 — 查看器前端:入口按钮 + useOcr + 结果面板 + 全部 i18n key

**新文件:`src\composables\useOcr.ts`**(模块级单例状态,先例 useExoticGate.ts:17-40 的模块缓存):
- 模块级 `ref`:`busy`、`panelOpen`、`result: Ref<OcrResult | null>`、`sourceLabel`(文件名/帧时刻,面板标题用);模块级 status 缓存(60s TTL,`resetOcrStatusCache()` 导出供下载/激活后失效)。
- `async function extractFromImage(itemId: number, fileName: string)`:先 `ocrStatus()` 判门——非 Authorized → `toast('info', t('ocr.unlicensed'))` + `router.push` 插件商店路由(路由名 `Grep "PluginStoreView" src/router` 取现名);`manifestReady/installed` 不过 → `toast('info', t('ocr.modelMissing'))` + push 设置页;通过则 `busy=true` → `invokeIpc<OcrResult>(IPC.OCR_EXTRACT_IMAGE, { itemId })` → 空行集 toast `ocr.empty`,否则开面板;错误按 `code` 分流文案(`ipcErrorMessage` 惯例,aiStore.ts:8 同款 import);finally busy=false。
- `async function extractFromVideoFrame(videoEl: HTMLVideoElement, fileName: string)`:门控同上;canvas 截帧段**逐行复用** useVideoFrameCapture.ts:69-95 的 drawImage/toBlob/SecurityError 防御(`blobToBase64` :34-49 抽为共享导出或本地复制,二选一,倾向从 useVideoFrameCapture 导出复用);`invokeIpc(IPC.OCR_EXTRACT_FRAME, { dataBase64 })`。
- `async function copyAll()`:`navigator.clipboard.writeText(lines.map(l=>l.text).join('\n'))` → toast `ocr.copied`;失败 toast `ocr.failed`。**仅用户点复制才写剪贴板**(契约:不自动覆写)。
- `function closePanel()`。

**新文件:`src\components\media\OcrResultPanel.vue`**:
- 消费 useOcr 单例;`v-if="panelOpen"` 深色浮层面板(样式承 `.detail-controls` 硬编码白系豁免惯例,VideoControlBar.vue:141-142 注);标题 `t('ocr.resultTitle')` + sourceLabel;主体 = `user-select: text` 的行文本区(`max-height: 60vh; overflow:auto`;行数 cap 1000,普通滚动即可,**无需虚拟化**——上限已钉,注释注明这是有界列表的显式豁免);底部按钮:复制全部(`Copy` 图标 + `t('ocr.copy')`)、关闭;逐行渲染 `line.text`(quad/confidence 一期不画框,数据已在 result 中留给二期高亮)。

**文件:`src\components\media\ContentViewer.vue`**
- import:`ScanText` 图标(:526 PencilLine 邻接)、`useOcr`(:470-483 composable 区)。
- 模板:编辑按钮块(:197-203)之后加:
  ```vue
  <UiIconButton v-if="detail.mediaType === 'image'" :label="t('ocr.entry')" :disabled="ocrBusy" @click="onOcrImage">
    <ScanText :size="18" />
  </UiIconButton>
  ```
- script:`openEditor`(:845)邻域加 `onOcrImage()`(调 `extractFromImage(detail.id, detail.fileName)`;detail 的 id 字段名以 useMediaDetail 现名为准)。
- 面板挂载:`<OcrResultPanel />` 放 face-overlay(:114-117)/EditOverlay 同层级(查看器根容器内,图像/视频两态共用——视频态触发也由本面板呈现,单例状态天然共享)。

**文件:`src\components\media\player\VideoControlBar.vue`**
- props(:16-36)加 `ocrBusy: boolean`;emits(:38-50)加 `(e: 'ocr-frame'): void`;截帧按钮(:124-126)之后插:
  ```vue
  <UiIconButton :label="t('ocr.entryVideo')" :disabled="ocrBusy" @click="emit('ocr-frame')">
    <ScanText :size="18" />
  </UiIconButton>
  ```
  (import ScanText 于 :8。)

**文件:`src\components\media\player\VideoPlayer.vue`**
- import useOcr;`captureFrame`(:209-211)邻域加 `function ocrFrame(): void { void ocr.extractFromVideoFrame(videoEl.value!, fileNameRef.value) }`(videoEl 空守卫);模板 `@capture-frame="captureFrame"`(:343)邻接加 `:ocr-busy="ocr.busy.value"` 与 `@ocr-frame="ocrFrame"`。

**i18n(本卡独占两份 locale 文件,T10/T11 的 key 也在此一次写齐,避免文件域冲突):**
`src\i18n\locales\zh-CN.ts` 与 `en-US.ts` 同步(localeIntegrity.spec.ts 强制 key 对齐):
- 新顶层 `ocr:` 节(edit: 节 :411 邻接):`entry`(提取文字)、`entryVideo`(识别当前帧文字)、`extracting`、`resultTitle`、`copy`、`copied`、`close`、`empty`(未识别到文字)、`failed`、`unlicensed`(OCR 插件未激活,前往商店)、`licenseExpired`、`modelMissing`(OCR 模型未下载,前往设置)、`captureTainted` 复用 player 现有 key 不新增。
- `settings:` 节追加:`ocrTitle`、`ocrHint`、`ocrTierMobile`(标准·约 30MB)、`ocrTierServer`(高精度·约 190MB)、`ocrActive`(使用中)、`ocrUse`(启用)、`ocrDownload`、`ocrDownloadComplete`、`ocrDownloadFailed`、`ocrManifestUnready`(下载源尚未上线)。
- `store:` 相关节追加:`builtinTitle`(内置能力插件)、`builtinNoInstall`(无需安装,激活即用)。
- **全部 UI 文案经 t(),零硬编码**(含面板按钮与 toast)。

**前端测试:**新增 `src\composables\__tests__\useOcr.spec.ts`(vitest,mock invokeIpc/toast/router):门控三分支(unlicensed 跳商店/model missing 跳设置/authorized 走提取)、空结果 toast、复制调用 clipboard.writeText、busy 互斥。

---

### T10 — 设置页:OCR 模型分节(预下载入口)

**新文件:`src\components\settings\OcrModelSection.vue`**(模板 = `FaceModelLibrary.vue`,同为「固定小模型组下载」形态;进度/下载调用姿态照 ModelLibrary.vue:260-284):
- 数据:`invokeIpc<OcrStatus>(IPC.OCR_STATUS)`;两档卡片:显示 displayName/sizeMb/installed/active;
- 动作:下载(`Channel<ModelDownloadProgress>` → `IPC.DOWNLOAD_OCR_MODELS { tier }`,进度条 pct 同 ModelLibrary.vue:251-255;`manifestReady=false` 时按钮禁用 + `t('settings.ocrManifestUnready')`);启用档位(见下方 configStore);完成后 `resetOcrStatusCache()` + 刷新。
- 未授权态:分节仍显示(预下载不设授权门——模型下载免费,识别命令才验 license;分节顶部小字提示当前授权态 + 商店链接)。

**文件:`src\stores\configStore.ts`**(:204-207 邻接)
- 加 `async setOcrTier(val: string)` 与对应 state 字段/loadConfig 读入(照 aiDownloadSource 全套三点:state、load、setter),配置键 `ocr_active_tier` 缺省 `"pp-ocrv5-mobile"`。

**文件:`src\views\SettingsView.vue`**
- `<FaceModelLibrary />`(:344)之后挂 `<OcrModelSection />`;import 区(:462)加声明。

后端配置键 `ocr_active_tier`:走既有 ConfigManager 通用 save_config 通路(与 `ai_download_source` 同层),schema 侧若有键白名单(`Grep "ai_download_source" src-tauri/src -g "*.rs"` 命中的注册表)同步登记。

---

### T11 — 插件商店前端:builtin 条目呈现

**文件:`src\types\exotic.ts`**
- `FormatResolution` 接口加 `builtin: boolean`(与 T3 后端字段对齐)。

**文件:`src\composables\useExoticStore.ts`**
- 加 `builtinOfferings` ref + `loadBuiltin()`:`invokeIpc<FormatResolution[]>(IPC.LIST_EXOTIC_FORMAT_RESOLUTIONS)` 过滤 `r.builtin`;并入现有 refresh 流(`loadRegistry/loadInstalled` :94 起的编排处同步调用);导出。

**文件:`src\views\PluginStoreView.vue`**
- 主插件卡列表 section(:28-122 之后)新增「内置能力插件」section(`t('store.builtinTitle')`):每条 builtin offering 渲染卡片——名称、availability 徽章(复用现有徽章样式)、`availableUninstalled/licenseExpired` 态显示「激活」按钮(**复用现有激活对话框组件**,`Grep "ActivateDialog" src/views/PluginStoreView.vue src/components` 取现名与调用姿态,传 pluginId 走 `IPC.ACTIVATE_EXOTIC_PLUGIN`)与 storeUrl 购买外链;`authorized` 态显示已激活 + `t('store.builtinNoInstall')` 说明行。激活成功后刷新 + `resetOcrStatusCache()`(import 自 useOcr)。

---

## 2. 并行分组

文件域两两不相交的组可并发;箭头 = 真依赖(编译/契约依赖),未列即无序约束。

```
波次1(三路并发):
  T1(crates/exotic-protocol)     T2(crates/scrollery-ai-core)     T3(src-tauri/src/exotic + resources)
波次2(依赖后并发,三路):
  T4(src-tauri/src/ai/ocr_registry.rs)      ← T2(profile 文件名单源)
  T5(crates/exotic-workers/ai-worker)       ← T1, T2
  T6(src-tauri: validate.rs/coordinator.rs/worker_client.rs) ← T1, T2
波次3(单路):
  T7(src-tauri/src/ipc/* + error.rs + registry.rs + src/constants/ipc.ts + src/types/ocr.ts) ← T3, T4, T6
波次4(三路并发):
  T9(src/components/media/* + src/composables/useOcr.ts + src/i18n/locales/*) ← T7
  T10(src/components/settings/* + src/stores/configStore.ts + src/views/SettingsView.vue) ← T7, T9(i18n key 已由 T9 落)
  T11(src/views/PluginStoreView.vue + src/composables/useExoticStore.ts + src/types/exotic.ts) ← T3, T7, T9(i18n)
  (i18n 两文件仅 T9 写,是三卡不冲突的前提,勿在 T10/T11 里补 key)
波次5:批末门禁 + 手测清单(phase-closer)
```

## 3. 边界情况(18 条)

1. **通道序(BGR/RGB)与归一化错配** → 识别全乱但不报错。防线:T2 golden 对拍测试是定案仪式,`swap_rb` 单点开关;未跑对拍前不得宣称管线可用。
2. **字典与 rec 模型错配**(坑8 同型)→ dict.rs 契约自检 C==N+1/N+2,不符即 ModelLoadFailed 级错误,拒绝带病服务。
3. **空结果**(无文字图):worker 回 Ok{lines:[]},前端 toast `ocr.empty`,不开面板。
4. **巨图**:源文件 >512MB 被 worker stat 拦(ResourceLimit);像素级大图经 det limit_side_len 降采样,内存有界;小字在 4K 截图上可能糊——一期已知限制(D-OCR-7),server 档 1280 缓解,tile 二期。
5. **竖排 CJK/180° 倒置**:cls 只治 180°;竖排列识别弱是 PP-OCR 已知界,不做承诺文案。
6. **视频帧超大**(8K):base64 解码 64MB 上限,超限回 `ocr_invalid_input`;正常 4K PNG(5-15MB)可过。
7. **canvas 污染**(SecurityError):useVideoFrameCapture 既有防御逐行复用,toast 后终止。
8. **AI 嵌入批在途时点 OCR**:`ai_worker` 是 std Mutex + worker 严格串行,OCR 请求最坏排队 ≈ 一个 EmbedBatch(120s 上限)。一期接受:按钮 busy 态 + `ocr.extracting` toast;不加抢占(写入面板注释)。
9. **worker 空闲 300s 自杀后首次 OCR**:`ocr_loaded` 随 drop_worker/重建复位 + SessionExpired 重试圈,用户无感(T6 mock 测试点名覆盖——CLIP 侧 W1 事故同型)。
10. **CLIP 会话切换/关闭不得误伤 OCR 会话**:worker 双槽独立;`set_active_model` 的 `close_session`(ai_commands.rs:721-725)只动 CLIP 槽。T5 加一条 worker 侧测试:SessionClose 后 OcrBatch 仍可服务。
11. **license 过期/撤销在面板打开期间**:门控按命令逐次验,已出结果不回收;下一次点击被拦。
12. **模型文件被篡改**:安装判定只查存在,真校验在 OcrSessionInit 的 sha256(session.rs 纪律)——校验不过回 `ModelLoadFailed`,命令层映射 `ocr_model_missing` 引导重下载(下载命令兼修复,`.part`+校验跳过已正确文件)。
13. **临时帧文件泄漏**:命令 finally 删 + 每次进命令清 >1h 陈旧文件;崩溃残留最多滞留到下次调用。
14. **`ocr_manifest_unready`**:URL 未钉定期间,设置页禁用下载并明示;识别命令在模型未装时回 `ocr_model_missing`——两条稳定码不得混用。
15. **逐项不连坐**:多项批中单项解码失败不拖垮整批(worker 纪律);一期命令恒单项,契约面向未来。
16. **MAX_JSON_LEN**:行数 1000 cap + 单项文本 512KB cap + 终帧 900KB 双保险,极端密集文字页降级为 ResourceLimit 而非协议断裂。
17. **dev 授权旁路**:`PICASA_EXOTIC_DEV_KEYSET`/fixture 对 builtin 分支生效(平台/版本门之后),dev E2E 不需真 token。
18. **i18n 完整性**:localeIntegrity.spec.ts 会拒收单边 key——T9 一次写齐两 locale。

## 4. 验证方式

### 施工中增量验证(implementer,每卡收尾即跑)

| 卡 | 命令 |
|---|---|
| T1 | `cargo test -p exotic-protocol` ; `cargo clippy -p exotic-protocol -- -D warnings` |
| T2 | `cargo test -p scrollery-ai-core --features inference` ; `cargo clippy -p scrollery-ai-core --features inference -- -D warnings`(golden/bench 两个 `#[ignore]` 不在此跑) |
| T3 | src-tauri 内 catalog/availability 过滤测试 + `cargo check --manifest-path src-tauri\Cargo.toml` |
| T4 | `cargo test --manifest-path src-tauri\Cargo.toml ai::ocr_registry` |
| T5 | `cargo test -p ai-worker`(包名以 crates\exotic-workers\ai-worker\Cargo.toml 为准);对应 clippy |
| T6 | `cargo test --manifest-path src-tauri\Cargo.toml exotic::validate worker_client coordinator` |
| T7 | `cargo check --manifest-path src-tauri\Cargo.toml` ; error:: 测试 ; `npx eslint src/constants/ipc.ts src/types/ocr.ts` |
| T9 | `npx vue-tsc --noEmit` ; eslint 改动文件 ; `npx vitest run src/i18n/localeIntegrity.spec.ts src/composables/__tests__/useOcr.spec.ts` |
| T10/T11 | `npx vue-tsc --noEmit` ; eslint 改动文件 |

Rust 格式:各卡收尾对**所属 crate** `cargo fmt`(⚠仓内已知坑:cargo fmt 传文件参数仍会重排整 crate——直接 crate 级跑并只提交本卡路径,勿吸无关重排)。

### 批末门禁(phase-closer,implementer 不跑)

- `cargo test --workspace --quiet`;`cargo clippy --workspace --all-targets -- -D warnings`
- `npm run lint` ; `npx vue-tsc --noEmit` ; `npx vitest run`(全量)
- 对照 `.github/workflows/ci.yml` 补跑遗漏面;本地 vs CI 结果分开陈述
- **本机手测(非自动化)**:① 模型就位后跑 golden 对拍(定案 swap_rb)与 ocr_bench(记录分段中位数进 findings);② dev keyset 下商店激活 → 图片提取 → 面板复制;③ 视频帧提取;④ 未激活态引导跳转;⑤ 模型未下载态引导;⑥ 空闲 6 分钟后再 OCR(自杀重建无感)。
- 资产 URL 钉定前,download_ocr_models 端到端下载验证顺延到回填 commit(剩余清单登记,勿默默漏)。

## 5. 已弃方案

- 扩展现有 `SessionInit` 挂 OCR 角色——CLIP 双塔是必备角色,迫使 OCR 用户先装 370MB CLIP;弃。
- `OcrBatch` 逐请求携带模型描述免会话——每次重建 session,交互延迟与设计噪音双输;弃。
- 图片走 ai_cache webp 作输入——ai 缓存 ≤640 级,文字分辨率不足;图片恒走 source_path;弃。
- 视频帧走后端 read_frame_at——与 D-421 冲突,seek 精度/色调映射与所见帧有差;弃。
- 帧 PNG 走协议帧 blob——supervisor 发送面无 request-blob 通道,扩它波及所有 worker 通路;弃。
- OCR 文本走 Success blob——结构化小数据,FaceDet JSON 先例在前;弃。
- rec 组批推理——交互单图收益近零;二期批量 OCR 再做;弃(一期)。
- DirectML EP 一期启用——rec SVTR 与坑9 同型静默算错风险;弃(D-OCR-2)。
- 后端 busy/进度轮询(derivationStore 模式)——OCR 秒级一次性动作,前端单源 busy 足够;弃。
- 经 Part8 私有 registry 发空安装包——引入无意义包管理面,builtin 一处分支即达;弃(D-OCR-5)。
- 隐藏未授权按钮——损可发现性;弃(D-OCR-6)。
- 自动写剪贴板——契约明令禁止;弃。
- Tesseract / Windows.Media.Ocr / OneOCR 逆向 / ocrs-cjk——research-web.md 已排除且 D-418 拍板;弃。
- 直接依赖 oar-ocr crate——D-419 拍板自研;弃。
- OCR 结果持久化 + 批量 OCR 任务化 + 搜索接入——二期立项范围,本线不碰 DB schema;弃(一期)。
