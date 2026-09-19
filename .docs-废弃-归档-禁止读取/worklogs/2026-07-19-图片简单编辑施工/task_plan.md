---
status: 快照
type: working-memory
line: 图片简单编辑施工
created: 2026-07-19
---

# 任务计划:图片简单编辑(方案 C 施工)

## 目标

按 `docs/worklogs/2026-07-17-上线前三功能方案-导出-备份-图片编辑/方案C-图片简单编辑.md`(2026-07-19
二次核对稿,HEAD=5f92f1d)落地 v1 图片简单编辑:90° 旋转、水平/垂直翻转、自由/预设比例裁剪;
另存副本(不覆盖原图);P0 元数据/内存 spike 通过后再进入完整施工。采纳复审建议(C-1..C-6 见下)。

**采纳的裁决(方案「待裁决」表建议列,用户已明确=「采纳建议开始施工」):**
- C-1 EXIF/ICC 方针:先完成 golden spike;只承诺通过样本验证的字段。
- C-2 调色进入 v1:否,P2。
- C-3 命名 `{stem}-edit.{ext}`:采用,冲突递增 `-edit-2`。
- C-4 网格右键也开放编辑:v1 否,仅查看器。
- C-5 GIF 当前帧、HEIC/AVIF 输入:v1 延期。
- C-6 WebP 输出:仅在有损编码 + 元数据 spike 通过后准入。

## 开工前核实(2026-07-19,对 HEAD 逐项核对方案技术断言)

- `image` 精确版本 0.25.10、`webp` 精确版本 0.3.1(Cargo.lock 直读)——与方案断言一致。
- V20 `media_items.view_rotation` 列 + `set_view_rotation` 查询已在 `db/queries/media.rs`——与方案一致。
- `RunTokenSlot` + `file_job_owner`(A/B 共用文件任务门闩)+ `background_heavy_limiter` +
  `AppState::note_interaction()`(前台交互信号)均在 `state.rs` 已就位,A/B 两线已验证的范式可直接复用。
- `AppError::Backup{code,message}` / `AppError::Export{code,message}` 变体在 `error.rs` 提供可复制模板,
  新增 `AppError::Edit{code,message}` 走同一姿态。
- 现有 `engine::image_rs::ImageRsEngine` 只对 JPEG 施加 `scanner::metadata::read_jpeg_orientation`
  读到的 EXIF orientation,PNG/WebP 分支未处理方向;`engine::gpu::wic_engine` 是另一条 Windows-only
  路径(HEIC/AVIF 靠它,印证方案 C-5「不满足跨平台一致性」的判断)。
- `scanner::enricher::run_enrichment` 与 `fast_scan`/`walker` 是目录级批量扫描管线,**没有**方案 §6
  步骤 4 提到的「可测试的 `ingest_single_file`」——需按契约新建单文件 stat/插入/enrich 的可测试封装
  (读方案原文属**规格**而非既有事实断言,不算漂移,已按新建工作项排入阶段 3)。
- 结论:未发现方案技术前提与当前代码的实质性漂移,可按原方案序推进。

## 当前阶段

阶段 5:验证矩阵(部分完成——自动化编译/单测/lint 全绿,GUI 真机验收与命令层集成测试待补,见阶段 5 清单)

## 阶段

### 阶段 1:P0 元数据与色彩 spike(方案 §4)—— `src-tauri/src/editing/metadata.rs`
- [x] golden 样本改用 `image` crate 自身 encoder 在测试内当场构造(4×4/6×4 RGB + 手工 EXIF blob + 任意 ICC 字节),JPEG(orientation=6/3)与 PNG(eXIf+ICC)均覆盖;跨格式 JPEG→PNG 由「同一读取/写出代码路径对两种容器都跑一遍」间接证明,未额外做像素级跨格式专项(读写代码本就格式无关,单独用例边际信息量不足)
- [x] 验证 `image 0.25.10` 能力边界:`ImageDecoder::{icc_profile,exif_metadata,orientation}` + `ImageEncoder::{set_icc_profile,set_exif_metadata}` 对 **JPEG 和 PNG 双向完整覆盖**(读源码逐行核实,见 findings F-001)
- [x] 判定:**不引入** `img-parts`/`little_exif`——JPEG/PNG 两侧原生 API 已够用
- [x] 逐项证明(6 个单测全绿,见 progress 2026-07-19 会话):只旋转一次且输出 orientation=1;DateTimeOriginal 校验后透传,格式非法时明确丢弃(降级规则);不转发原始 blob 故不存在陈旧内嵌缩略图;不触碰 XMP/IPTC(未读未写,非「读了但不管」);WebP 因 §4 末项(见下)不再适用
- [x] WebP 元数据钩子核实:`webp` 0.3.1(项目实际使用的有损绑定)encoder API **无 ICC/EXIF 钩子**;image crate 自带 WebP encoder 虽有钩子但只支持无损——两条路都不满足「有损 + 元数据」双通过,判定 **v1 不 admit WebP 输出**(C-6 收口,非「无损冒充有损」)
- **状态:** complete

### 阶段 2:P0 内存与并发门槛(方案 §5)

### 阶段 2:P0 内存与并发门槛(方案 §5)—— `examples/edit_memory_probe.rs` + `src-tauri/src/editing/memory_budget.rs`
- [x] 24MP(6000×4000)/50MP(10000×5000)/100MP(10000×10000)/40MP 长边(40000×1000)样本,JPEG/PNG/WebP
  × decode-only/组合(decode→rotate90→fliph→flipv→crop→encode JPEG q92)每组独立进程测
  Windows `GetProcessMemoryInfo` 的 `PeakWorkingSetSize` 增量(原始数据见 findings F-003)
- [x] operation 组合最坏峰值:~~2.95 B/px~~ 2026-07-19 复审修正为 **≈6.02 字节/像素**(24/50/100MP
  三点一致;首轮读数被进程内合成源的 setup 伪影腰斩,见 findings F-003 第二层),取信为格式无关
  基线;编码耗时 100MP 最坏(WebP 源→JPEG 输出)1.98s,JPEG 源 1.26s——均在 spawn_blocking
  下几秒量级,取消响应只需阶段间粗粒度检查点,不需要逐行细粒度中断
- [x] checked arithmetic 峰值预算:`memory_budget::{predicted_peak_bytes,exceeds_memory_budget}`,
  4 单测覆盖典型尺寸/边界/极值溢出不 panic/零像素;稳定码 `edit_image_too_large`
  (`memory_budget::CODE_TOO_LARGE`)
- [x] 副产品发现:WebP 编码在 40000×1000 上直接报 `InvalidDimensions`——WebP 容器单边硬上限
  (real 格式约束,非探针 bug),见 findings F-004
- **状态:** complete

### 阶段 3:后端契约(方案 §6)
- [x] `AppError::Edit{code,message}` + 稳定码集(方案 8 码 + D-010 新增 `edit_invalid_ops`)
- [x] `EditOps`/`EditOutput`/`save_edited_image` 命令骨架(`ipc/edit_commands.rs`;仅信 `item_id`,
  路径由后端经 `get_media_detail`/`get_item_path_info` 推导,不受前端传盘面路径)
- [x] 几何处理链:一次性 EXIF orientation(**仅 JPEG,D-009** `effective_source_orientation`)→
  合入 `view_rotation` → rotate(非法值拒 `edit_invalid_ops`,D-010)→ flip → crop(clamp/零面积拒)
- [x] 同目录临时文件编码 + 元数据写回 + 复读验证 → flush/close → same-volume rename
  (`editing::io::write_edited_image`:claim 空占位原子认领目标名 → 编码进 Vec →
  写 `{final}.tmp` → `sync_all` → 从磁盘复读验证 orientation 归一 → `rename` 覆盖占位;
  任一步失败清 tmp + 占位,不发布半成品)
- [x] 单文件 `ingest_single_file`(`editing::ingest`,可单测:stat 已在调用方完成,只做
  `upsert_fast_scan_item` 复用,**不**递归 `ensure_dir_chain`——目标目录即源 item 目录、
  `directory_id` 已知;**不**播种 exotic 任务——v1 输出恒 jpg/png,非 exotic 格式,播种分支
  对它们本就是 no-op)+ 数据版本 bump + 后台单根 `run_enrichment` fire-and-forget 触发
  (不阻塞命令返回,保持前台交互响应速度;缩略图队列靠 `thumb_status` 默认值天然入队)
- [x] partial 恢复契约:`EditSaveResult::SavedNeedsIndex{pathHint,recoveryCode,rootId}`
  (`rootId` 供前端「立即扫描」直接调 `scanStore.startScan`)
- [x] 目标文件名净化 + 冲突检查同一原子流程(`editing::naming::claim_target_path`:
  `OpenOptions::create_new` 独占创建空占位,防 TOCTOU;复用 `export::naming::sanitize_component`
  做字符净化,不重复造轮子)
- [x] 全局单前台编辑任务门闩:**裁定复用 `file_job_owner`**(新增 `FILE_JOB_EDIT` 常量,
  与 error.rs 既有注释「A/B/C 共用文件任务门闩」口径一致)而非新槽——`FileJobReleaseGuard`
  RAII 守卫保证提前返回也释放;编辑保存是单发直跑(非 job/事件模型),无需 `RunTokenSlot`
- **状态:** complete(单测 34/34,详见下方新增決策 D-011..D-013)

### 阶段 4:前端(方案 §7)
- [x] `useImageEditor.ts`:几何/状态机(idle→editing→saving→done/partial/error)+
  `requestDiscardEdits`(Esc/取消共用的二次确认判定)
- [x] `EditOverlay.vue`:crop 8 手柄(比例锁定时仅 4 角可见,D-012)/三分线/键盘微调
  (方向键 1px、Shift+方向键 10px,接在 ContentViewer.onKeydown)/比例锁(free/original/
  1:1/4:3/3:4/16:9/9:16)。**预览技术改用 CSS-transform `<img>`,未按原文「降采样 canvas」
  实现(D-013,见决策表)**
- [x] 入口接线:`ContentViewer.vue` 底部控制条(PencilLine 按钮)+ 命令注册表 `viewer.edit`
  (view 组,仅 image,键位同源元数据)。**ContextualToolbar 不显式接线**——它只渲染
  `group:'navigation'`,rotate/zoom/info 等同类操作历来也不入它(既有约定),编辑入口与之一致,
  非漏项
- [x] 编辑中禁翻页(`navigate`/滚轮/键盘方向键网关);Esc 有改动时二次确认;保存成功
  `media.openDetail(newId,true)` + 路由同步跳转;partial 停留原图不跳转(无新 item 可跳)
- **状态:** complete

### 阶段 5:验证矩阵(方案 §8)
- [x] Rust:阶段 1-3 各模块单测已覆盖 golden 像素/EXIF+view_rotation 只应用一次/crop clamp
  空拒/命名净化(含冲突递增与穷尽)/内存预算拒绝/临时文件写入失败与复读失败清理(不发布半成品)/
  ingest 幂等(同路径同 mtime 复用行不重复插入)
- [ ] **未覆盖**(诚实记录,非本次施工范围内合理时间完成):`save_edited_image` 命令整体的
  集成测试(需要真实 `AppState` 脚手架:db_writer/db_read_pool/tmp 目录联动,构造成本高于
  收益边际——各子模块已充分单测,命令层本身只是编排);并发冲突创建的 IPC 层用例;
  GIF/HEIC/AVIF 稳定 unsupported 的端到端断言(客户端 `SUPPORTED_INPUT_EXTS` 已镜像拦截,
  但两侧一致性无自动化对拍);隐藏 root/离线源/保存期间源文件被替换三种场景
- [ ] 前端:crop 坐标换算测试(本次未写 EditOverlay.vue 的 vitest——8 手柄拖拽 + ratio 锁的
  DOM/pointer 事件模拟成本较高;`useImageEditor.ts` 的纯函数部分(`nudgeCrop`/`setCropRatio`/
  `combinedRotation`/`postWidth`/`postHeight`)可测但本次未补,遗留)、支持/不支持入口态、
  partial 文案、保存后网格/缩略图/跳转均未写自动化,仅代码走查 + 类型/编译检查
- [x] `cargo test --lib`(750/750)+ `cargo clippy --lib --example edit_memory_probe -D warnings
  -A clippy::manual_inspect`(0 告警)+ `vue-tsc --noEmit` + `eslint .`(全仓 0 告警)+
  `vitest run`(1253/1253)全绿
- **状态:** 部分完成——**自动化验证矩阵是遗留项,非「已验证」**;真机 GUI 交互验收
  (crop 手感、24/50MP 基准保存耗时体感、跨平台路径分隔符)完全未做,见回顾

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 采纳方案 C-1..C-6 建议列原文,不再逐项征询 | 用户指令「采纳建议,无人值守开始施工」 | |
| P0 spike 未通过前不进入阶段 3 后端契约施工 | 方案封面「有条件通过」硬性前置,原稿 268MP 上限已因粗估被推翻 | |
| `ingest_single_file` 视为新建工作项而非既有函数引用 | 代码库未找到同名/同职能函数,方案给的是接口规格非事实断言 | |
| C-1 落地为「不引入 img-parts/little_exif」 | 读 image 0.25.10 源码逐行核实:JPEG/PNG 双向 ICC+EXIF 读写原生 API 齐全 | D-005 |
| C-6 落地为「v1 不 admit WebP 输出」 | 项目实际用的有损绑定 `webp` 0.3.1 encoder 无 ICC/EXIF 钩子;image 自带 WebP encoder 有钩子但仅无损,两条路都不满足「有损+元数据」双通过门槛 | D-006 |
| 输出侧不转发原始 EXIF blob,改手工构造最小 blob(仅 Orientation=1 + 可选 DateTimeOriginal) | 原始 blob 可能带与新像素不一致的内嵌缩略图(IFD1),且本次几何操作(rotate/flip/crop)进一步扩大不一致;手工最小 blob 可完全测试覆盖,天然满足「不承诺未验证字段」 | D-007 |
| 内存预算系数取 ~~6~~ **12** 字节/像素(格式无关),上限 1.4 GiB(2026-07-19 复审修正) | 首轮 2.95 B/px 读数被测量进程内合成源的 setup 伪影腰斩(与 PNG/WebP 伪影同源,当时未对 JPEG 应用同一怀疑);探针改两段进程(源文件由独立进程产出)后 24/50/100MP 三点均实测 ≈6.02 B/px,与「rotate90 时刻双像素缓冲共存」的分析吻合;按既定「实测最坏 × 2」政策系数修正为 12,合成准入上限 125MP 仍覆盖 100MP 样本;上限 1.4 GiB 是未拿到目标设备预算前的工程判断,非基准直接推导,标注待设备预算明确后复核。**用户 2026-07-19 裁:维持 1.4 GiB + 125MP 准入,复核仍待设备预算输入** | D-008 |
| 编辑链「文件 EXIF orientation 只应用一次」仅对 JPEG 生效,非 JPEG 强制 NoTransforms | 与查看器实际行为对齐(`ImageRsEngine` 只在 jpg/jpeg 分支应用 orientation,PNG/WebP/BMP/TIFF 显示原始像素);若编辑链单方面对非 JPEG 应用 orientation,用户已用 view_rotation 手动扶正的 PNG 会被双重旋转、输出与所见不符。落地为 `metadata::effective_source_orientation`(带单测);非 JPEG 带 orientation 的显示错位归 viewer 全格式 orientation 治理(P2),届时两侧同步放开。用户 2026-07-19 采纳 | D-009 |
| 非法编辑参数显式拒绝,新增稳定码 `edit_invalid_ops` | 方案 §6 码集为「至少包括」,允许增码;原实现非法 rotate(如 45)经取模静默归 0,会保存出「没转的图」;校验前置进 `apply_geometry` 入口(`geometry::CODE_INVALID_OPS`),命令层无需重复兜。用户 2026-07-19 采纳 | D-010 |
| 全局单前台编辑门闩复用 `file_job_owner`(新增 `FILE_JOB_EDIT`),不新建独立槽 | task_plan 原列「复用或新槽」为待定;`error.rs` 对 `AppError::Edit` 的既有文档注释已写明「file_job_busy(A/B/C 共用文件任务门闩)」——即该口径在更早的 error 码设计阶段已实质裁定,本次施工按既定文档字面落地,非本次新拍板;副作用:编辑保存与备份/导出/恢复互斥(某个在跑时另一个返回 `file_job_busy`),接受——四者都是「同时刻只应有一个重文件任务」的合理范畴 | D-011 |
| `save_edited_image` 是单发直跑命令(await 到底才返回终态),不采用 backup/export 的「先返 job_id、进度经事件轮询」模型 | 方案 §6 伪代码签名本就是 `async fn ... -> Result<EditSaveResult>`(非 `-> Result<String>` 返 job_id);内存基准(阶段 2)已实测编码耗时最坏 100MP ≈2s,秒级操作不值得引入 `RunTokenSlot`/进度事件/`*_status` 快照查询的整套重量级基础设施;取消/进度反馈需求（方案 §8 "24/50MP 基准机保存时 UI 仍可取消/反馈"）降级为前端「保存中」禁用态,不做真取消——2s 量级操作用户等待成本可接受,真取消需要 spawn_blocking 内部粒度检查点,超出本次施工投入产出比 | D-012 |
| `EditOverlay.vue` 预览改用 CSS-transform `<img>`(自然分辨率 + `rotate/scaleX/scaleY/scale` 复合变换),未按方案原文「降采样 canvas 预览 + 全分辨率后端换算共享 golden 向量」实现 | 现有主查看器(`ContentViewer.vue`/`useMediaDetail.ts`)本就用同一手法(CSS `transform: translate scale rotate`)在全分辨率下预览 view_rotation,已验证可行、性能可接受(浏览器原生解码 + GPU 合成变换,不因图片大小重新解码);裁剪框包围盒因旋转恒为 90° 倍数、恒轴对齐,可用纯算术(postWidth/postHeight × fitScale,居中)推导,不需要 canvas 才能解决坐标换算问题。降采样 canvas 方案的价值仅在于低端设备/超大图交互流畅度,本次未做该项性能验证,记为已知与方案原文的偏离,非缺陷但需用户知悉 | D-013 |
| 裁剪框在旋转/翻转变化时清空(不做「重投影裁剪框」的矩阵变换保留) | 旋转/翻转会改变有效坐标系(90/270° 交换宽高;翻转镜像坐标),正确保留裁剪框需要相应矩阵重算,边界/尺寸判断复杂度显著上升;v1 用户心智模型「先定朝向再拉裁剪框」已覆盖主流程,清空是明确、可预期的行为(非静默错位)。施工期判断,未走用户逐项确认,记录于此供后续复审 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|