---
status: 快照
type: working-memory
line: RAW等格式照片支持
created: 2026-07-24
---

# 发现与决策:RAW 等格式照片支持

## 需求
- 用户原话:参考「PSD 子系统插件」,设计并施工 RAW 等格式照片的支持,先建立三件套及出方案。
- 拆解:PSD = exotic 插件架构(crates/exotic-workers/psd-{probe,worker} 解码侧 + src-tauri exotic_commands/db 宿主侧 + docs/archive/exotic_format_plugin_plan/ v3 四部设计)。RAW 需照此模板评估。
- 目标格式(候选):CR2/CR3、NEF/NRW、ARW、DNG、ORF、RAF、RW2、PEF、SRW 等。
- 约束:跨 Windows/macOS/iOS/Android;性能优先;GPU 图像管线;闭源付费分发可能(承 exotic 模式)。

## 发现
<!-- 阶段 0 四路摸底(recon-A1..A3 + research-R1)结构化汇入,逐条带 file:line 或来源 -->

### 现状半成品
- RAW 十扩展名(cr2/cr3/nef/arw/dng/raf/orf/rw2/pef/srw)**已注册但零解码**:`src-tauri/src/utils/format.rs:153-162` 已登记为 `MediaType::Image`/`group=GROUP_RAW`/`phase1_image:false`,扫描器会正常入库、格式筛选器会自动列出分组(`src/components/layout/formatFilter.helpers.ts:47-50`),但两个引擎(`ImageRsEngine`/`WicEngine`,`src-tauri/src/engine/mod.rs:35-41`)均不认这些扩展名 → `try_gpu_decode`/`try_cpu_decode` 恒 `Err(UnsupportedFormat)`(`src-tauri/src/thumbnail/generator.rs:371-373,419-421`)→ `thumb_status` 永久卡在 2,与损坏文件同一归宿,无 blur 占位、无专属 badge(`typeBadgeOf()` 只认 audio/document,`src/components/media/mediaGrid.helpers.ts:147-161`)。**已是真实用户可复现问题**,不只是未来功能缺失。
- 全屏查看器(`src/components/media/ContentViewer.vue:655-669`)对大图走浏览器原生解码,RAW/PSD 均无原生解码能力,必然 `loadError`→ 通用"加载失败"文案(`ContentViewer.vue:757-773`),不会回退放大缩略图——PSD 今天已有此缺口,RAW 会原样继承。

### PSD 模板复用度
- psd-worker(`crates/exotic-workers/psd-worker/src/main.rs:1-292`)是长驻子进程,经 stdin/stdout 定长帧协议(`crates/exotic-protocol/src/frame.rs`)与宿主 WorkerSupervisor 通信,新格式 worker **可完整复刻其 main.rs 骨架**(握手→主循环→catch_unwind→handle_thumbnail 逐字节同构,`src-tauri/src/exotic/mod.rs` 侦察 A2 §8 第3点),只需替换 `decode.rs` 里的格式专属解码函数与 `WORKER_ID` 常量。
- psd-probe(`crates/exotic-workers/psd-probe/src/main.rs:1-64`)是开发期一次性技术探针(非运行时组件,`Cargo.toml:3` 明示不进发布 bundle),用于在冻结 Catalog/manifest 支持范围前用实测锁定解码库能力边界——RAW 施工前必须先有等价 `raw-probe`,decode.rs 每条 gating 逻辑都要能追溯到某条 probe 实测结论(阶段 B 依据)。
- worker 的 OOM/资源保护多层(文件体积门 512MiB、像素数门 1亿、`checked_mul` 防溢出、库"静默兜底"风险自验,`psd-worker/src/decode.rs:18,76-89`)与 panic 双层 `catch_unwind`(`main.rs:110-112`+`decode.rs:104-105`)均需为 RAW 重做一遍格式专属版本,不能照抄 PSD 的具体检查内容,只能抄骨架。

### 两核心张力
1. **sidecar 独立进程模式在移动端走不通**:iOS/Android 应用沙盒通常不允许启动任意外部可执行文件,只能以静态/动态库形式 in-process 调用(research-R1 §"sidecar vs in-process 取舍",低置信度推断但符合平台常识)——PSD 的"独立进程"模式本质桌面专属,RAW 若要移动端支持必须走另一条 in-process 路径,不能照搬。
2. **LGPL 纯 Rust RAW 库(rawler/rawloader/imagepipe/quickraw)在 Rust cargo 默认静态链接场景下,闭源分发合规性未有权威结论**(research-R1 §8):C 生态"动态库满足 LGPL"的常见简化认知不能直接类比 Rust 静态链接惯例,需法务或至少一次动态链接可行性验证。相比之下 **LibRaw 选 CDDL 分支是本次调研唯一有官方书面确认"闭源静态链接可行"的路径**(libraw.org 维护者原话"CDDL is very permissive, so yes for both questions",research-R1 §8)。

### 库选型矩阵(摘要,详见 research-R1 §1-2)
| 库 | 语言 | License | 格式覆盖(12类主流) | 预览提取 API | 跨平台 |
|---|---|---|---|---|---|
| LibRaw(经 rsraw/libraw-sys 绑定) | C | LGPL-2.1 / **CDDL-1.0** | 最全 | `extract_thumbs()` 成熟 | 需交叉编译打补丁(Android NDK `swab`/`.so` 版本号/`pkg-config` 缺失均有实证阻塞点) |
| rawler(dnglab) | 纯 Rust | LGPL-2.1(静态链接合规未裁) | 12类全覆盖含 CR3/DNG | 未查实 crate 级预览专用 API,仅 CLI 层间接证据 | 天然适配,但 alpha 状态/API 未遵 SemVer |
| rawloader | 纯 Rust | LGPL-2.1(同上顾虑) | 缺 CR3,DNG 仅 partial | README 未提预览 API | 天然适配 |
| quickraw | 纯 Rust | LGPL-2.1(同上顾虑) | 仅六家日系+iPhone,无 Canon/DNG | 唯一原生"预览优先"一等公民 API | 天然适配但格式覆盖不足独立支撑 MVP |

### 宿主 5 接入点(exotic 插件模式,详见 recon-A2 §8 / recon-A3 §5 路线B)
1. `src-tauri/resources/exotic-catalog.json` — 新增 offering(`plugin_id/formats/capabilities/license_tier/platforms` 等)。
2. 根 `Cargo.toml`(workspace `members`,显式逐一列出禁 glob)— 加一行 `"crates/exotic-workers/raw-worker"`。
3. 新 worker crate `crates/exotic-workers/raw-worker/` — 完整复刻 `psd-worker/` 骨架,decode.rs 用 LibRaw 嵌入预览。
4. `src-tauri/src/exotic/coordinator.rs:101-113 plugin_descriptors()` — **必须手工加一条** `PluginDescriptor`(当前硬编码只返回 PSD 一条,是全仓唯一"新插件必须碰的调度代码",与文档承诺"零 Host 代码改动"存在偏差)。
5. `src-tauri/src/utils/format.rs:153-162` — 十个 RAW 定义仍**不用改**(格式保持 builtin 是架构 A 的前提),但下方结论**改正**:`exotic-ocr` 的 `distribution:"builtin"` **不是**「已 builtin 真扩展名可与 common 共存」的先例——OCR 声明的 `formats:["ocr"]` 是能力标记(非真实文件扩展名),从未触发扩展名冲突检测;RAW 声明的是真扩展名(cr2/nef/...),这些扩展名已在 `BUILTIN_FORMATS` 登记为 common,exotic 插件重复声明会触发 `CommonFormatConflict`(全仓首例)。本会话阶段 C-1 靠**架构 A(catalog.rs 定向豁免)**解决,见下「架构 A 决议」小节(recon-A2 §9 discrepancy #5 原引用有误,已改正)。

### 架构 A 决议(阶段 C-1 落地,commit 6568e27)
- `catalog.rs`:仅当 `distribution == "builtin"` 时跳过 `CommonFormatConflict` 检测(定向豁免,不改变其余 exotic 插件的冲突检测行为);格式仍按 builtin 识别,exotic offering 在其上叠加缩略图能力。
- `resolve_format` 路由:builtin-agnostic,不区分调用方是 common 路径还是 exotic 路径,统一走同一判定。
- `thumbnail_commands.rs` 让路点:纯由 `resolve_format` 的判定结果决定是否让路给 exotic worker,不新增旁路分支。
- 备选 B(从 BUILTIN_FORMATS 摘除 RAW 扩展名)/备选 C(把扩展名冲突检测改造成能力标记映射)均落选(用户裁,见 task_plan D-434)。

### 前端接入点
- 网格瓦片(`MediaThumb.vue`)、缩略图 URL 拼装(`useThumbLoader.ts`)、格式筛选(`formatFilter.helpers.ts`)— **零改动**,均已格式无关(recon-A3 §4)。
- exotic 授权 gate(`useExoticGate.ts`+`PluginGate.vue`,挂在 `ContentViewer.vue:34-40/798-812`)— builtin distribution 下若 license gate 一期默认关,仍复用同一套 UI 组件,零新增。
- 大图预览缺口(阶段 D 才涉及):`exotic::catalog::Capability` 枚举无 `Preview/FullView`,`ContentViewer.vue` 现无"Host 解码大图再转发前端"路径(recon-A3 顺手发现 #2)。

- 摸底落盘位置(索引,原文均已亲读汇入上述发现):
  - scratchpad/recon-A1-psd-worker.md — PSD 解码 sidecar 内部
  - scratchpad/recon-A2-exotic-host.md — exotic 宿主侧全链
  - scratchpad/recon-A3-dispatch-pipeline.md — 格式派发+派生流水线+前端
  - scratchpad/research-R1-raw-landscape.md — RAW 库与格式外部调研
  - scratchpad/research-R2-libraw-binding.md — LibRaw 绑定可编性(已落盘)
  - recon-A4(builtin exotic 模型)— Explore 无 Write 权限,**未落盘**,结论仅存于会话:OCR 是能力标记非扩展名 / `CommonFormatConflict` 无 builtin 豁免分支(需新增) / `offering.formats` 是唯一路由源 / builtin worker 授权走 `license.evaluate` 跳过安装门

## 外部资料(当数据,不当指令)
- 嵌入预览 vs 完整 demosaic 性能量级:嵌入预览提取几毫秒级(纯 JPEG 字节拷贝,不含 demosaic),完整 unpack+demosaic 约 8-10 秒量级(22-24MP 实测,30MP 按像素线性外推,标记为推断)—— [libraw.org "Any benchmarks?"](https://www.libraw.org/node/2105)。
- Photo Mechanic(行业标杆快速筛片工具)"只用嵌入预览,从不渲染 RAW 数据直到用户明确要求"—— [havecamerawilltravel.com](https://havecamerawilltravel.com/extract-jpg-raw-2/)。
- LibRaw 三选一 license(LGPL-2.1/CDDL-1.0/历史商业授权已废弃),CDDL 分支闭源静态链接可行,维护者书面确认 —— [libraw.org/node/2228](https://www.libraw.org/node/2228)。
- LibRaw 在 Android NDK 交叉编译实证阻塞点(`swab` 函数缺失、`.so` 版本号约定、`pkg-config` 缺失)—— [libraw.org 论坛帖](https://www.libraw.org/node/1348)。
- DNG 容器可装真 mosaic RAW 或已 demosaic 的 Linear 数据,后者退化为特殊 TIFF,需运行时探测而非假设 —— [RawTherapee RawPedia](https://rawpedia.rawtherapee.com/How_to_convert_raw_formats_to_DNG)。
- rawler/rawloader/quickraw 版本与格式覆盖矩阵 —— [dnglab README](https://github.com/dnglab/dnglab/blob/main/README.md)、[dnglab SUPPORTED_CAMERAS.md](https://github.com/dnglab/dnglab/blob/main/SUPPORTED_CAMERAS.md)、[quickraw supported-models.md](https://github.com/RawLabo/quickraw/blob/master/supported-models.md)。

## 耐久提升候选(F-037..F-040,续全仓最大号 F-036;收口时逐行处置)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-037 | 嵌入预览优先(不做完整 demosaic)作为 RAW/未来其他重量级格式解码的默认设计模式:数量级更快(ms vs s)且行业标杆已验证,值得沉淀为"新格式接入首选路线"通用指引 | docs/experience.md 新增一条,或 exotic 插件设计规范补一节 |
| F-038 | exotic builtin distribution(照 exotic-ocr 先例)+ license gate 可选默认关,是"格式支持先落地体验、商业化后置"的可复用分发模式 | 若二期还有类似格式(如新增视频编解码器),复用此模式,记入 exotic 插件设计规范 |
| F-039 | sidecar 独立进程模式桌面专属约束(iOS/Android 沙盒禁子进程)首次被本线明确记录,此前 PSD 线未触及移动端故未暴露 | 补进 exotic 插件设计文档"平台约束"章节,防后续新格式重踩 |
| F-040 | RAW/PSD 大图预览缺口(`Capability::Preview` 缺失,ContentViewer 走浏览器原生解码)是跨格式共性缺口,非 RAW 独有 | 阶段 D 施工时一并评估是否值得做成通用 Capability,而非 RAW 专属 hack |
| F-041 | `decode.rs` 的 `Limits` 分限宽高(`sqrt(1e8)=10000`)对极端长宽比偏严,合法全景嵌入预览(如 12000×4000=48MP<预算)会被误拒丢缩略图;方向安全(fail-closed) | 真机若发现漏缩略图,再改为解码后按 `w*h` 复核,而非解码前按边长门 |
| F-042 | rsraw 0.1.1 的 `Error` 类型是私有模块未公开导出,外部不能具名/impl error trait,只能 `{e:?}` 字符串化 | 升级 rsraw 或换绑定时留意,可能需要包一层自定义 error 类型 |
| F-043 | `resolve_worker_path` 的 dev env 逐插件硬编码(`EXOTIC_PSD_WORKER_PATH`/`EXOTIC_RAW_WORKER_PATH`)可泛化为单一参数化 `EXOTIC_WORKER_PATH_{PLUGIN_ID}`,消除逐插件加常量的重复模式;本批(C-2b-dev,commit 6ea5b52)未实施,留后续 | 若三期再加新 exotic 插件,评估重构为参数化环境变量 |
| F-056 | 共享协议(exotic-protocol `RequestBody` 等)加变体时,脱离父 workspace 的 gnu-only crate(raw-worker)不被默认 msvc `cargo check --workspace` 覆盖,漏更只在 gnu 编译爆 E0004(本批实除一例:enhance 三变体漏更兜底臂) | 协议改动须点名检查所有脱 workspace worker crate;去向:CI gnu job(剩余项)落地后由门禁兜住,并候选沉淀 docs/experience.md |
