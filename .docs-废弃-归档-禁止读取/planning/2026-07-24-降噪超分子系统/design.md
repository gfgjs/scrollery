---
id: 2026-07-24-design
status: active
type: design
line: 降噪超分子系统
created: 2026-07-24
---

# 降噪/超分子系统架构与实现方案(定案稿)

> 输入:本目录 `scratchpad/` 三份摸底(ai-pipeline-map / plugin-store-map / industry-research)+ task_plan.md;关键锚点由 architect 对照现行代码核实(下文 path:line 为设计当日实读)。对标 Topaz Photo AI,做成插件商店内付费子系统,用户可选模型。

## 主线裁决(2026-07-24)

- **A–I 全部采纳**:独立 enhance-worker、按需操作产 `-enhanced` 新文件、单 offering builtin+paid、5 模型商用许可安全短名单、512 tile 静态 shape + fp16、P0 零新表。
- **spike 为 P0 硬门**:SCUNet/DRUNet/FBCNN/Real-ESRGAN ONNX 静态 shape 自导出 + PyTorch golden 对拍 + DirectML 实测;不过即回炉 D 节短名单,不得带病进施工。
- **排期约束**:P0 施工清单 #5/#6/#8(exotic-catalog.json / catalog.rs / coordinator.rs)与并行 RAW 线同文件,须在 RAW 线该三文件落地后进场。
- **J-1..J-8 留用户裁决**(见 J 节),每点已附推荐项;本线交付=设计定案,施工另立批次。
- **2026-07-24 用户裁决:J-1..J-8 全按推荐定案;J-2 变更 = 模型托管改用户自有 HuggingFace 仓(仓名待用户补,批 6 资产钉定前需要)。P0 施工已开工。**

---

## A. Worker 拓扑

**结论:新建 `crates/exotic-workers/enhance-worker` 独立进程(目录模式照 raw-worker WIP 的 crate 布局,不依赖其未定内容);推理核心下沉 `crates/scrollery-ai-core/src/enhance/`,复用既有 ort engine/provider/session 校验骨架;GPU 互斥沿用 F5 GPU 分析槽(与 CLIP/人脸争同一令牌)。**

理由:
1. **串行阻塞不可接受**:ai-worker 严格串行、一次一请求(ai-pipeline-map §1.1)。超分单图 GPU 5–30s、CPU 1–3min(industry-research §1 Puget 口径),塞进 ai-worker 会把交互式 OCR(`ocr_extract_image` 画廊即时识别)饿死在队列后。
2. **崩溃隔离价值高**:超分是本仓最易触发 DirectML OOM/TDR 的负载(大 tile + 长跑)。独立进程崩了只损失当前 job;若同进程,连坐 CLIP 453MB 会话(plugin-store-map §7)重载,代价是分钟级。Supervisor 的 kill→wait→池补机制原样可用(`src-tauri/src/exotic/supervisor.rs`,ai-pipeline-map §1.2)。
3. **VRAM 争用靠槽不靠进程**:进程分离不省 VRAM,串行化才省。enhance job 执行期间持 GPU 令牌(coordinator.rs:306 「uses_gpu 走先 CPU permit 后 GPU 令牌双取(D2)」同款),CLIP/人脸管线自然让步;ai-worker 空闲自杀 300s(ai-pipeline-map §1.1)兜底释放其残留 VRAM。
4. **超时量纲完全不同**:enhance 需要「per-tile Progress 心跳 + 静默限时」模型(见 E 节),与 EMBED_BATCH=120s / OCR_BATCH 线性放宽(coordinator.rs:44-84)都不同,独立 worker 独立超时档最干净。
5. **会话槽**:ai-worker 已是 `sess` + `ocr_sess` 双槽(ai-worker/main.rs:240、547-554,D-OCR-1),再加第三槽会让「三会话 VRAM 共驻」成为常态风险。

调度归属:**不进 exotic 任务化管线**(照 D-OCR-7 先例,message.rs:240 注记「OCR builtin 路径不进 exotic 任务化,catalog::Capability 无对应变体豁免」)。enhance 是用户显式发起的交互任务,不由 `exotic_tasks` 扫描驱动;host 侧新建 EnhanceService 直接持 supervisor + worker client(worker_client.rs MAX_ATTEMPTS=2 重试模式,plugin-store-map §3)。

被否:
- **复用 ai-worker 加第三槽(OCR 模式)**——串行单请求让分钟级批阻塞交互 OCR,崩溃连坐 CLIP 会话,三会话 VRAM 叠加不可控。
- **进 exotic_tasks 任务化调度**——该表 `UNIQUE(item_id, plugin_id, capability)`(schema.rs:500-518)是幂等派生语义,同图不同模型/参数二次增强无法表达。

## B. 任务形态

**结论:P0 主路径 = 按需「增强」操作(用户选图 → 参数 → 出新文件),产物走编辑线先例落**同目录 sibling 新文件** `{stem}-enhanced.{ext}`,入库为普通新 media_item;不进 media_derivations,不建全库状态表;P0 job 队列为内存态。**

理由:
1. **产品语义**:对标 Topaz = 昂贵、选择性、参数化的用户操作,产物是用户资产。而 `media_derivations` kind 状态机(schema.rs:277-287)与 `ai_items` 是「全库自动跑」的语义——增强绝不能自动跑全库(单图数十秒 × 全库 = 灾难)。
2. **产物归宿有现成契约**:编辑线 C-3 裁决 `{stem}-edit.{ext}` + `claim_target_path` 原子认领(editing/naming.rs:2-3、41-43)、`*.tmp` 同卷 rename(editing/io.rs)、EXIF 保留(editing/metadata.rs)全部直接复用,仅 stem 后缀换 `-enhanced`。新文件经扫描自然收编,**导出链零改动**(新 item 即普通资产,export/core.rs 不感知)。
3. **P0 零新表**:交互 job 重启即丢可接受(用户重发);断点续跑是批处理规模化(P1)的需求,届时再加 `enhance_jobs` 表(见 G)。

混合分期:P1 在图片编辑器内加「增强」入口(输出仍走同一 job 管线);P1 批处理队列持久化。

被否:
- **derive/kind 派生**——自动全库语义 + 产物是缓存物 + `PRIMARY KEY(item_id, kind)` 无法表达参数化重跑。
- **ai_items 式全库状态表**——增强不是全库属性,pending 全库播种毫无意义。
- **EditOps 内嵌字段(与 adjust 平级)**——EditOps 是同步 CPU 像素链,分钟级 GPU 推理塞进 `save_edited_image` 会阻塞编辑保存;只复用其 IO/naming/metadata 层。

## C. 插件商店建模

**结论:单一 offering `exotic-enhance`(一个「增强」插件,插件内多模型资产可选装),`distribution: "builtin"`,`license_tier: "paid"`,`sku: "enhance-engine-2026"`,`formats: ["enhance"]` 虚拟格式(OCR `["ocr"]` 先例,exotic-catalog.json:23——过 `is_valid_format` 但永不与扫描相遇);catalog.rs `Capability` 枚举加 `Enhance` 变体(catalog.rs:50-61 + as_str,仅作展示/授权面,不接任务化,D-OCR-7 豁免先例)。**

- **offering 粒度**:一个插件、模型是插件内资产档位。模型注册表单源在 `scrollery-ai-core` 的 `enhance_profile.rs`(纯数据,照 ocr_profile.rs 先例,plugin-store-map §4),加模型不动 catalog、不发新 SKU。
  被否:**每模型一个 offering**——SKU/验签/购买流 N 倍爆炸,商店 UI 噪音,模型迭代被 catalog 发版锁死。
- **distribution**:builtin(D-OCR-5)。enhance-worker 编译随 app 发布,无安装包;`availability_of` builtin 分支(catalog.rs:137-139 注记)直接验 license;模型资产首用下载 + 设置页预下载(D-420 同型)。
  被否:**package 分发 worker 二进制**——Part8 私有 registry 未就绪,OCR/RAW 均已走 builtin,无理由开新路。
- **模型资产托管**:用户自有 HuggingFace 仓托管(J-2 定案 2026-07-24;仓名待补)**自导出 ONNX**,URL + 字节数 + sha256 逐件钉定进 `enhance_registry.rs`,主源 + mirror_url;每模型附 LICENSE/NOTICE 文本同仓分发(BSD-3/Apache-2.0/MIT 允许再分发但须附许可与署名)。
  被否:**第三方源直链(OpenModelDB/HF 社区转换版)**——来源不受控、许可核实链断裂、且 P0 短名单半数无现成 ONNX 本就要自导出。
- **下载/安装/卸载**:`download_assets()` 全复用(model_download.rs:断点续传/sha256/.part→同卷 rename/进度 Channel,plugin-store-map §2);安装到 `{models_dir}/{profile_id}/`;卸载 = 删目录 + 状态刷新。
  - **P0 实装偏离**:模型平铺 `{models_dir}/{id}-{prec}.onnx`(文件名全局唯一,卸载按档删两文件;reviewer 评估风险可接受,子目录制留 P1 再议)。

## D. 模型短名单(权重可商用组合)

全部出自 industry-research §2 商用安全短名单;「ONNX」列 = 现成可得性,无现成者进 P0 前置 spike 自导出(见 I)。

| 任务 | 档位 | 模型 | 许可 | ONNX 可得性 | 体量(权重) |
|---|---|---|---|---|---|
| 超分 | 质量档(默认) | Real-ESRGAN x4plus | BSD-3-Clause | 有社区版,仍自导出钉定 | .pth ~67MB,ONNX 同量级 |
| 超分 | 动漫/速度档 | Real-ESRGAN x4plus-anime-6B | BSD-3-Clause | 有社区版,自导出钉定 | ~18MB |
| 降噪 | 质量档(默认) | SCUNet(KAIR) | MIT | **无现成,自导出** | 中等,导出后实测钉数 |
| 降噪 | 速度/可控档 | DRUNet(KAIR) | MIT | **无现成,自导出** | 中等;σ 可变噪声水平 = 强度滑杆的天然载体 |
| JPEG 伪影 | 单档 | FBCNN | Apache-2.0 | **未核实现成,按自导出计** | 中等;QF 可控 = 强度滑杆 |
| 人脸修复 | — | **defer(裁决点 J-3)** | 无干净商用开源:GFPGAN 捆绑 StyleGAN2/DFDNet(NC)、CodeFormer S-Lab NC、RestoreFormer 依赖链未核实(industry-research §2 红线合集) | — | — |

- P1 候补速度档:SPAN / realesr-general-x4v3(轻量),**逐权重 LICENSE 核实通过后**再入册(industry-research §2:SPAN 权重许可未核实细则)。
- P2 候补高质量档:SwinIR/HAT(Apache-2.0)——transformer 算子导 ONNX + DirectML opset≤20 有实测风险(industry-research §3),且推理慢,故不进 P0。
- 每模型导出两份:fp32(CPU 用)+ fp16(GPU 默认档);均锁静态 shape(见 E)。
- 红线:MPRNet / CodeFormer / GFPGAN 禁止打包分发(industry-research §2)。

## E. 运行时与大图

**结论:统一 ort + 既有 provider 探测链(DirectML→CUDA→CoreML→OpenVINO→CPU,ai-pipeline-map §2.1);tile 512×512 静态 shape + 16px overlap 中心区拼接;GPU 默认 fp16、CPU fp32;per-tile Progress 心跳 + 静默限时;取消 = kill worker。**

- **DirectML 维护态风险与后路**:P0 模型全是 plain CNN(RRDBNet/UNet 系),opset 13–17 足够,距 DirectML 锁定的 opset 20 上限(industry-research §3)有余量,短期风险低。后路 = EP 选择已收敛在 provider.rs 单点,未来切 WinML 只动该层;这也是把 SwinIR/HAT 压到 P2 的理由之一。
  被否:**ncnn-vulkan 路线**——性能更极致但独立二进制生态,与既有 ort/协议/会话校验框架整合成本高,仅当某 EP 实测不达标再局部备选(industry-research §3 取舍)。
- **tiling**:导出 ONNX 时锁静态输入 `1×3×512×512`(动态 shape 在多 EP 明显更慢或不支持,industry-research §3);tile_pad=16px(≥官方建议 10px),融合 = 每 tile 只取中心 `512−2×16` 区域拼接,x4 输出 tile 2048²;边缘 tile reflect-pad 补齐;小于 512 的图 pad 到整 tile 单发。tile 尺寸与静态 shape 是**联动契约**:改 tile 必须重导模型。低 VRAM 降档(384/256)需对应导出多档静态 shape,P0 只出 512 一档 + OOM 时降 tile 重试留 P1(P0 OOM 直接报 `ResourceLimit` 转 CPU 建议)。
- **精度**:GPU fp16 默认(SR 场景安全档,industry-research §3);CPU fp32;int8 不上线(须盲测对拍后才可作为档位,同上调研结论)。
- **内存/显存预算**:显存峰值(512 tile、fp16/fp32)在 dev 机 bench 后钉数——**未实测,不给数字**。主存大头是输出画布:复用 editing/memory_budget.rs 准入门思路,新增**输出像素上限**准入:降噪/去伪影输入 ≤100MP(与编辑线 D-008 同门),超分按「输出像素 ≤ 上限」反推输入(4x 时输入上限 16MP)。**2026-07-24 bench 终钉(spike-D,裁决点 J-6)**:100MP/16MP 均为实测钉数,非草案;超限返回稳定错误 `enhance_input_too_large`。
- **进度与取消**:worker 每完成一个 tile 发 Progress 帧(协议已有帧型,ai-pipeline-map §1.3);host 侧 `ENHANCE_SILENCE` 静默限时 300s(收 Progress 即重置,SESSION_INIT 静默心跳量纲同款 coordinator.rs:48-53;容 CPU 单 tile 慢跑);前端进度粒度 = job 内 tile 计数 + 队列内 job 计数。取消 = host kill worker + 重启(模型仅数十 MB,重载秒级,代价可接受)。**2026-07-24 bench 终钉(spike-D)**:`ENHANCE_SESSION_INIT` = 90s、`ENHANCE_SILENCE` = 300s(coordinator.rs 实测钉数,非估算)。
- **SCUNet 精度例外**:GPU 档实装为 fp32(DML fp16 实测 PSNR 37.48dB 未达 40dB 门,`fp16_safe=false`,GPU 亦须加载 fp32 权重);其余 4 档(x4plus/anime-6B/DRUNet/FBCNN)`fp16_safe=true`。
  被否:**协议加带内 cancel 帧**——worker 严格串行只在请求间读 stdin,带内取消要改协议不变量,收益不抵。
- **CPU 兜底**:provider 落到 CPU 时 UI 显式提示「预计慢 4–10×」+ 首 tile 实测 × tile 数的耗时预估,绝不静默长跑(industry-research §4)。
- **产物路径**:worker 组装全图、编码(无元数据)写 host 指定的 `{work_dir}/{job}.tmp`(路径前缀白名单校验,cache_key 越界拒绝先例 plugin-store-map §4);host 做容器级 EXIF 注入(不重编码)→ `claim_target_path` 认领 → 同卷 rename。单点内存、无 per-tile 大 blob 过管道、崩溃无半成品。

## F. 用户可选模型 UX

**结论:Topaz 式「自动默认 + 手动兜底」(industry-research §4 三范式对比);P0 的「自动」是规则映射而非 AI 分析。**

- **商店**:`exotic-enhance` 走 builtin 卡片区(OCR builtin 单独展示先例,plugin-store-map §4)+ ExoticActivateDialog 激活流复用 + 未授权引导(D-OCR-6 同型)。商店侧**零新控件**。
- **模型管理(新 UI 形态①)**:设置页「影像增强」分节 = 模型卡片列表(名称/任务/体量/速度-质量标签/下载进度/删除),先例杂交:OCR 模型下载分节(plugin-store-map §5)+ aiStore `listModelRegistry` variants/`setActiveModel`(plugin-store-map §5 CLIP 先例)。摸底已确认商店无模型选择控件先例(ai-pipeline-map §6.2),此为本线主要新 UI 投入之一。
- **处理入口(新 UI 形态②)**:查看器/画廊右键「增强照片…」→ EnhanceDialog:
  - 任务勾选:降噪 / 超分 / 去 JPEG 伪影(可组合,执行序固定 降噪→去伪影→超分,FBCNN 前置链路见 industry-research §2);
  - 模型档:每任务默认档预选(「自动」= 规则映射:源为 JPEG 建议勾去伪影、小图建议超分等),手动可换;
  - 强度:DRUNet σ 滑杆、FBCNN QF 滑杆;
  - 前后对比预览:取画面中心(或用户点选点)512² 单 tile 快跑,before/after 分割拖杆(Dialog 内嵌,产物走临时缓存文件 + convertFileSrc,符合 assetProtocol 约束);
  - 输出:格式跟随源(JPEG/PNG),显示预计输出尺寸与耗时预估。
- **批处理**:P0 = 多选右键入队,顺序执行,进度落在既有进度面板形态(job 列表 + tile 进度);专门队列管理 UI(暂停/重排/持久化)归 P1。
- P1 Autopilot 真分析(噪声估计模型)、编辑器内入口;P2 多模型分屏对比(Gigapixel Compare 式)。

## G. 数据与状态

- **DB:P0 零新表**(B 节)。P1 `enhance_jobs` 草案:`(id PK, source_item_id FK, params_json, status, output_path, last_error_code, created_at, updated_at)`——注意与 exotic_tasks 语义区隔(参数化可重复,无 UNIQUE(item,capability))。
- **settings keys**(settingsMap,DynamicSettingControl 绑定,ai-pipeline-map §6.2):
  - `enhance_model_denoise` / `enhance_model_upscale`(每任务当前选档,默认 = 质量档)
  - `enhance_last_params`(JSON 快照,Dialog 记忆上次选择)
  - `enhance_output_format`(`follow_source`|`jpeg`|`png`)
  - 不进 STATE_KEYS 自动管线(无 auto_process 语义)。
- **协议**(exotic-protocol v2 additive,message.rs:35 RequestBody + :90-99 OCR 三臂先例):
  - `EnhanceSessionInit { session_id, models: Vec<ModelDescriptor> }`(ModelRole 加 `Enhance` 角色;复用 validate_and_resolve 前缀/字节数/sha256 校验,ai-pipeline-map §2.2;终态补 `work_dir`——输出路径白名单前缀,worker 侧 claim 前校验 tmp 路径落于此前缀内;`models_root`——模型归属根,校验 ModelDescriptor 路径未越界至该根之外)
  - `EnhanceSessionClose { session_id }`
  - `EnhanceRun { session_id, source_path, output_tmp_path, output_format, steps: Vec<EnhanceStep> }`(终态;`EnhanceStep { task, model_id, strength }` 逐 step 描述任务链,取代早期 `tasks/params` 扁平字段;Progress per tile;Success 带统计)
  - `capability::ENHANCE = "enhance"`(message.rs:236-241 常量区)
- **IPC**(新 `src-tauri/src/ipc/enhance_commands.rs`;错误全走 AppError serde::Serialize + 稳定 code):
  | 命令 | 参数 → 返回 | 错误 code |
  |---|---|---|
  | `enhance_status` | () → { availability, provider, models[] } | — |
  | `download_enhance_model` | (model_id, Channel<DownloadProgress>) → () | `enhance_download_failed` |
  | `delete_enhance_model` | (model_id) → () | `enhance_io` |
  | `enhance_preview` | (item_id, point?, params) → { beforePath, afterPath } | `enhance_model_missing` / `enhance_busy` / `enhance_input_unsupported`(解码失败归并)/ `enhance_not_implemented`(P0 预览占位) |
  | `enhance_start` | (item_ids, params) → job_id | `enhance_input_too_large` / `enhance_unlicensed` / `enhance_input_unsupported`(RAW 等,见 I;GPU 不可用同归此码) |
  | `enhance_cancel` | (job_id) → () | — |
  | `get_enhance_queue` | () → jobs+进度(或事件推送) | — |

  实现定案(P0 落地,2026-07-24):稳定 code 集共 9 个——`enhance_input_too_large` / `enhance_unlicensed` / `enhance_input_unsupported` / `enhance_model_missing` / `enhance_busy` / `enhance_download_failed` / `enhance_io` / `enhance_invalid_params` / `enhance_not_implemented`;原草案 `enhance_decode_failed` 归并入 `enhance_input_unsupported`,`enhance_gpu_unavailable` 归并入 `enhance_io`,不再单列。
  - Capabilities:预期同 OCR 先例「app 自有命令 core:default 覆盖、无需新增条目」(plugin-store-map §6),施工时核验一次。
- **前端**:`src/stores/enhanceStore.ts`(队列/进度/模型列表)+ `src/types/enhance.ts`;大数组无,常规 ref 即可。

## H. 分期

**P0(最小可卖闭环)**:单图/多选增强 → 新文件落库;三任务 × 5 模型;fp16 GPU + CPU 兜底;预览对比;商店付费门控 + 模型下载。

前置 spike(独立小批,照 raw-probe 探针先例 45b66b0):SCUNet/DRUNet/FBCNN/Real-ESRGAN ONNX 静态 shape 导出 + 与 PyTorch 输出 golden 对拍 + DirectML 实测——**此 spike 不过,P0 短名单回炉**。

P0 文件级施工清单(仿 OCR 14 文件模板,plugin-store-map §4):

| # | 域 | 文件 | 内容 |
|---|---|---|---|
| 1 | 协议 | `crates/exotic-protocol/src/message.rs` | Enhance 三臂 + capability::ENHANCE + ModelRole::Enhance + 结果体 |
| 2 | 模型契约 | `crates/scrollery-ai-core/src/enhance_profile.rs`(新) | EnhanceProfile{id, task, file, scale, tile, tile_pad, fp16} 注册表(纯数据零 ort) |
| 3 | 推理 | `crates/scrollery-ai-core/src/enhance/`(新:mod/tiling/chain) | 解码→任务链→tiling→ort→融合→编码 |
| 4 | Worker | `crates/exotic-workers/enhance-worker/`(新 crate) | 三臂 handler、per-tile Progress、输出路径白名单、空闲自杀 |
| 5 | Catalog | `src-tauri/resources/exotic-catalog.json` | exotic-enhance offering(builtin/paid/sku) |
| 6 | Catalog 解析 | `src-tauri/src/exotic/catalog.rs` | Capability::Enhance 变体 + as_str(catalog.rs:50-74) |
| 7 | 门控 | `src-tauri/src/exotic/mod.rs` | builtin 分支应零改,核验即可 |
| 8 | 超时 | `src-tauri/src/exotic/coordinator.rs` | op_timeouts::ENHANCE_SESSION_INIT / ENHANCE_SILENCE |
| 9 | Host 服务 | `src-tauri/src/enhance/`(新:service/registry) | 内存队列、supervisor/client 持有、GPU 令牌、claim+EXIF+rename、资产清单 URL+sha256 |
| 10 | IPC | `src-tauri/src/ipc/enhance_commands.rs`(新) | G 节七命令 + 错误 code |
| 11 | 下载 | 复用 `src-tauri/src/ipc/model_download.rs::download_assets` | 仅新调用路径 |
| 12 | 前端数据 | `src/stores/enhanceStore.ts` + `src/types/enhance.ts`(新) | 状态/队列/模型 |
| 13 | 前端 UI | `src/components/enhance/EnhanceDialog.vue`(新)+ SettingsView 分节 + 商店卡片(自动) | 入口/预览/参数/下载 UI |
| 14 | 资产工程 | 仓外 model-zoo 仓 + 导出脚本 + NOTICE | ONNX 导出、sha256 钉定、许可打包 |

P0 轮次量级:OCR 线(T1–T11)一日闭环为基线;本线多出新 worker crate、tiling 引擎、预览 UI、资产导出四块,估 **spike 1 批 + 施工 5–6 批,~150–220 implementer 轮(±40%)**。

**P1**:enhance_jobs 持久化 + 队列 UI;OOM 降 tile 重试;速度档模型(SPAN 等,许可核实后);编辑器集成入口;RAW 解码→增强衔接;Autopilot 噪声估计。
**P2**:人脸修复(许可解决后)、SwinIR/HAT 高质量档、int8 档(盲测对拍后)、macOS CoreML 平台线、WinML 迁移评估、多模型分屏对比。

## I. 风险与开放问题

1. **DirectML 维护态**(industry-research §3):P0 CNN 低风险;WinML 后路收敛在 provider.rs 单点;SwinIR/HAT 因 opset/算子风险压 P2。
2. **许可逐权重核实**:发布前每权重 LICENSE 原文存档 + 分发包附 NOTICE;OpenModelDB「平台来源≠许可安全」(industry-research §3);红线三模型禁入。
3. **ONNX 自导出算子风险**:SCUNet/DRUNet/FBCNN 无现成 ONNX——前置 spike 是 P0 硬门,不过即回炉短名单。
4. **RAW 线衔接**:增强输入要求「可解码位图」。P0 对 RAW item 返回 `enhance_input_unsupported`(稳定 code,前端引导);P1 待 RAW 线落地后接「RAW 解码产物 → 增强」链(顺序:解码在前,增强吃解码后位图)。设计不依赖 raw-worker WIP 的任何未定内容,仅共享 exotic-workers 目录模式。
5. **4K/100MP 大图**:输出像素上限准入(J-6 定数)+ 流式 tile;输出画布是主存大头,worker 单点持有;双写(worker tmp → host 注 EXIF → rename)为容器级操作不重编码。
6. **VRAM 共驻窗口**:enhance 启动时 ai-worker 会话可能仍热(CLIP 453MB),GPU 令牌只保证不并发推理、不回收显存;缓解 = ai-worker 300s 空闲自杀;极端低 VRAM 机型主动 SessionClose 归 P1。
7. **iOS/Android**:CoreML 成熟、NNAPI 已被平台层废弃 → XNNPACK 兜底 + QNN 高端(industry-research §3);移动端需更小模型档,P2 立项。
8. **性能数字全部待实测**:文献数字不可移植(industry-research §4),P0 施工含 dev 机 bench 步(tile 吞吐/VRAM 峰值/CPU 倍率)后钉入超时与准入数值。

## J. 用户裁决点清单(可直接答复)

**2026-07-24 定案:全部按推荐采纳,唯 J-2 托管源改用户自有 HuggingFace 仓。**

| # | 裁决点 | 推荐 | 理由 |
|---|---|---|---|
| J-1 | 付费档位 | 单付费 SKU `enhance-engine-2026`,builtin,一次解锁全部模型档 | 与 OCR/PSD 同型,避免模型级 SKU 爆炸;高级档拆分留到有真实付费数据后 |
| J-2 | 模型托管源 | 自建公开 model-zoo 仓(GitHub Releases)托管自导出 ONNX + LICENSE/NOTICE,主源+镜像,sha256 钉定 | 半数模型本就要自导出;第三方源许可合规链断裂;OCR 先例证明该分发链可用 |
| J-3 | 人脸修复 | defer(P2 前提 = RestoreFormer 依赖链专项核实通过或取得商业授权) | 调研结论:无干净商用开源(GFPGAN/CodeFormer 均 NC 链路) |
| J-4 | 视频超分 | 不做(不进任何期) | 工程量数量级更大、显存/时长不可控,与照片管理主线偏离 |
| J-5 | 命名 | 子系统「影像增强」/ plugin_id `exotic-enhance` / 产物后缀 `-enhanced` | 与「OCR 文字提取」命名风格一致;`-enhanced` 与编辑线 `-edit` 同构不撞名 |
| J-6 | 大图上限 | 降噪/去伪影输入 ≤100MP(随编辑线 D-008 门);超分按输出像素上限反推(4x 输入 ≤16MP)。**2026-07-24 bench 终钉(spike-D)**:100MP/16MP 均为实测钉数,非草案 | 输出画布内存是硬约束;实装边界测试见 enhance::service::tests::admission_direct_100mp_boundary / admission_upscale_4x_16mp_boundary |
| J-7 | P0 模型组合 | D 节 5 模型(x4plus/anime-6B/SCUNet/DRUNet/FBCNN);「自动」档 P0 为规则映射非 AI 分析 | 全部许可安全;真 Autopilot 需额外分析模型,P1 再上 |
| J-8 | 降噪格式边界 | 全格式开放(JPEG/PNG/TIFF),UI 提示已编码图伪影风险;不学 Lightroom 限 RAW | P0 时 RAW 链未通,限 RAW 会让降噪没有输入源;Topaz/ON1 通吃模式已验证可行 |

## 施工排期备注(主线)

- 与 RAW 线同文件冲突:P0 清单 #5/#6/#8(exotic-catalog.json、catalog.rs、coordinator.rs)当前为 RAW 线 WIP 修改中(git status M,catalog 已含 `exotic-image-raw` offering)。本线施工须在 RAW 线该三文件落地后进场。
