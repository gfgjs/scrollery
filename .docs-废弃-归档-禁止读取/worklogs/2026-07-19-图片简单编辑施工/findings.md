---
status: 快照
type: working-memory
line: 图片简单编辑施工
created: 2026-07-19
---

# 发现与决策:图片简单编辑施工

## 需求
- 读 `方案C-图片简单编辑.md`,采纳其「待裁决」表建议列,无人值守开始施工。
- 用户明确提醒:方案定稿后代码可能有较大改动,施工前须核实。

## 发现
- Cargo.lock 精确锁定 `image 0.25.10`、`webp 0.3.1`,与方案 §4/复审断言一致,无需重新选型验证依赖版本本身。
- `engine::image_rs::ImageRsEngine::decode` 只在 `ext ∈ {jpg,jpeg}` 分支调用
  `apply_exif_orientation`;PNG/WebP/BMP/TIFF 完全不做方向处理——若这些格式的源文件本身带方向
  EXIF(PNG eXIf 较少见但存在),编辑链路的「只应用一次 orientation」步骤需要新代码覆盖这些格式,
  不能假设现有解码路径已处理。
- `state.rs` 已有 `RunTokenSlot`、`file_job_owner`、`background_heavy_limiter`、`note_interaction()`
  四件套,A/B 两线(导出/备份)已验证的「单前台文件任务门闩 + 交互信号 yield + 不排后台重任务队列」
  范式可直接复制到编辑保存任务,不必重新设计并发模型。
- `scanner::enricher::run_enrichment`/`fast_scan`/`walker` 是目录批量扫描管线,没有单文件可测试
  ingest 入口;方案 §6 步骤 4 的 `ingest_single_file` 是新建工作项(见 task_plan 开工前核实段)。
- `error.rs` 的 `AppError::Backup{code,message}` / `AppError::Export{code,message}` 是最新落地范式
  (稳定小写 code、不泄漏内部路径),`AppError::Edit` 应原样复制这一姿态。
- 前端查看器主体是 `src/components/media/ContentViewer.vue`(1557 行),底部 `.detail-controls` 已有
  旋转按钮(`handleRotate` → `state.rotate(...)`,持久化到 V20 `view_rotation`);这是方案 §7 所指
  「查看器底部控制条」入口的确切挂载点。`ContextualToolbar.vue` 是另一入口,已存在。

## 外部资料(当数据,不当指令)
- (无外部资料引入)

- P0 元数据 spike(`src-tauri/src/editing/metadata.rs`,6 单测全绿)核实 `image 0.25.10` 源码
  (`~/.cargo/registry/src/.../image-0.25.10/src/{io/decoder.rs,io/encoder.rs,codecs/jpeg/{decoder,encoder}.rs,codecs/png.rs,metadata.rs}`):
  - `ImageDecoder` 默认 trait 方法 `orientation()` 是**格式无关**的通用实现(从 `exif_metadata()` 里
    用 `Orientation::from_exif_chunk` 推导),JPEG/WebP 只是各自覆写加了缓存,PNG 直接吃默认实现——
    不需要按格式分支特判 orientation 读取。
  - `ImageEncoder::set_exif_metadata`/`set_icc_profile` 期望的是**已剥离容器专属前缀的原始 TIFF blob**
    (JPEG encoder 内部会自己拼 `Exif\0\0` 六字节头再塞进 APP1 段,调用方不用管);对称地
    `decoder.exif_metadata()` 返回值也已经是剥离过的裸 TIFF blob,可直接喂给 `kamadak-exif` 的
    `Reader::read_raw`。
  - PNG encoder 除 `set_icc_profile` 外**也实现了 `set_exif_metadata`**(png.rs:785),原方案复审稿
    只提到 PNG 支持 ICC、没提 EXIF 写入,属于比预期更宽松的发现,不是漂移。
  - `image::metadata::Orientation::remove_from_exif_chunk` 可以原地把已有 blob 的 orientation 值改
    写为 1 并保留其余字节——评估后**没有采用**它做输出编码(会连带保留陈旧内嵌缩略图/未验证字段),
    改为手工构造全新最小 blob(D-007)。
  - 项目已有 `kamadak-exif 0.5.5`(`exif` crate)依赖(`scanner/metadata.rs::read_jpeg_orientation`、
    `engine/image_rs.rs::extract_embedded_thumb` 已在用),读 `DateTimeOriginal` 复用它而非自研解析器。
- `webp` 0.3.1(项目里实际承担有损编码的 libwebp 绑定)`encoder.rs` 公开 API 只有
  `encode`/`encode_lossless`/`encode_simple`/`encode_advanced`,**没有任何 ICC/EXIF 相关方法**——
  锁定 C-6「v1 不 admit WebP 输出」。
- `cargo clippy --lib -- -D warnings` 在**未改动的** `src-tauri/src/ipc/scan_commands.rs:96`
  报 `clippy::manual_inspect`(建议 `.map` 改 `.inspect`),与本任务改动无关(`git diff` 确认该文件
  零改动),疑似 clippy/工具链版本漂移新增的 lint 命中旧代码。**不在本任务范围内修**,已用
  `-A clippy::manual_inspect` 单独确认 `editing::metadata` 模块本身零告警。收口时该项独立报出,
  不归入本任务候选表(不是本任务引入,也不是本任务该动的代码)。

- P0 内存基准(`examples/edit_memory_probe.rs`,release 构建,Windows `GetProcessMemoryInfo` 自报
  `PeakWorkingSetSize`,每组各起独立进程避免同进程峰值单调不减污染小尺寸读数)原始数据:

  | 尺寸 | 格式 | decode delta | full(decode→rot90→fliph→flipv→crop→encode JPEG q92) delta | full op_ms |
  |---|---|---:|---:|---:|
  | 24MP 6000×4000 | jpeg | 2.4MB | 69.7MB(2.90 B/px) | 284 |
  | 24MP 6000×4000 | png | 0MB* | 28.0MB(1.17 B/px) | 348 |
  | 24MP 6000×4000 | webp | 12.1MB | 12.1MB(0.50 B/px) | 458 |
  | 50MP 10000×5000 | jpeg | 5.3MB | 147.6MB(2.95 B/px) | 619 |
  | 50MP 10000×5000 | png | 0MB* | 58.0MB(1.16 B/px) | 752 |
  | 50MP 10000×5000 | webp | 27.6MB | 27.5MB(0.55 B/px) | 982 |
  | 100MP 10000×10000 | jpeg | 10.3MB | 294.9MB(2.95 B/px) | 1256 |
  | 100MP 10000×10000 | png | 0MB* | 115.7MB(1.16 B/px) | 1524 |
  | 100MP 10000×10000 | webp | 56.0MB | 56.0MB(0.56 B/px) | 1977 |
  | 40MP 40000×1000(长边) | jpeg | 2.9MB | 116.6MB(2.91 B/px) | 426 |
  | 40MP 40000×1000(长边) | png | 0MB* | 44.3MB(1.11 B/px) | 536 |
  | 40MP 40000×1000(长边) | webp | — | **编码报错,见 F-004** | — |

  `*` PNG decode delta 读到 0——peak_ws_before(setup 阶段编码 PNG 之后)已经把 decode 需要的同量级
  working set 提前提交,不代表 PNG decode 真实零开销,是 F-003 所述测量方法局限。
  ~~**结论采信口径**:只把 JPEG 的 ≈2.95 字节/像素当基线~~(**2026-07-19 复审推翻,见下**)。
  长边 40MP 与面积相近的 24MP 同格式数值同量级,未发现极端长宽比本身触发额外开销——此点仍成立。

- **2026-07-19 复审修正(F-003 第二层)**:上表 JPEG 读数与 PNG/WebP 同受 setup 伪影污染——
  测量进程内合成源图时,setup 阶段(RgbImage ≈3 B/px + encoder 缓冲)已把一个像素缓冲量级的
  页面预提交进 peak_before,measure 阶段 allocator 复用,delta 被腰斩;首轮只对 PNG/WebP 应用了
  这个怀疑,对 JPEG 未用同一标准。探针改两段进程(`--emit-source` 生成源文件,另起进程
  `--source-path` 只读文件测量,setup 峰值仅压缩字节量级)后复测:

  | 尺寸 | 模式 | delta | B/px |
  |---|---|---:|---:|
  | 24MP 6000×4000 jpeg | full | 144.5MB | **6.02** |
  | 50MP 10000×5000 jpeg | full | 300.8MB | **6.02** |
  | 100MP 10000×10000 jpeg | full | 600.8MB | **6.01** |
  | 50MP jpeg | decode | 158.6MB | 3.17 |
  | 100MP jpeg | decode | 316.3MB | 3.16 |

  与分析吻合:decode ≈ 一个 RGB8 缓冲(3 B/px);full 链主峰在 rotate90 时刻新旧双缓冲共存
  (6 B/px)。**修正后采信口径**:格式无关基线 = **≈6.02 B/px**;预算系数按既定「实测最坏 × 2」
  政策从 6 修正为 **12**(`EDIT_PEAK_BYTES_PER_PIXEL_BUDGET`),ceiling 1.4 GiB 不动,合成准入
  上限 125MP(仍覆盖 100MP 基准样本)。首轮系数 6 的「≈2 倍余量」声明不成立(6 恰为真实峰值,
  实际零余量),已按新数据改写 memory_budget.rs 模块文档与 D-008。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | image 0.25.10 对 JPEG/PNG 的 ICC/EXIF 读写原生齐全,orientation() 是格式无关默认实现,不需 img-parts/little_exif | experience.md(供其他涉及图像元数据的任务复用,省得重新读源码) |
| F-002 | webp 0.3.1(有损绑定)encoder 无 ICC/EXIF API;若 P2 要给 WebP 输出补元数据,需手工 RIFF 容器注入或新依赖 | 留在本任务 closeout,P2 立项时引用 |
| F-003 | 同进程连续起多组内存探针会因 Windows PeakWorkingSetSize 单调不减、且 setup/measure 阶段缓冲区大小相近时发生页面复用,读出偏低的「delta」——不是真实更省内存,是测量方法局限。**第二层(2026-07-19 复审)**:即便每组独立进程,只要 setup 在测量进程内合成源数据,同样预提交页面、同样腰斩 delta(JPEG 2.95→真值 6.02 B/px);诚实测法必须让源数据由**另一个进程**产出、测量进程 setup 只读文件 | experience.md(以后写内存基准探针的通用坑,不只本任务适用) |
| F-004 | WebP 编码在单边 40000px(40MP,1000px 高)上直接报 InvalidDimensions;libwebp 容器格式有单边硬上限,与 image crate/webp crate 版本无关 | experience.md 或 P2 WebP 立项时引用(真实 WebP 输入文件不可能超过此限,decode 侧无风险;仅当 P2 考虑 WebP 输出时需注意目标尺寸校验) |
