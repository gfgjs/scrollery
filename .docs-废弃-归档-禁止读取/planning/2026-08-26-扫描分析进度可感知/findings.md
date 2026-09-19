---
status: 施工中
type: 工作记忆
line: 扫描分析进度可感知
created: 2026-08-26
---

# 发现与决策:扫描分析进度可感知

## 需求
- 检查“添加文件夹 → 扫描文件 → 分析元数据”流水线相关代码。
- 完善“让用户可感知扫描分析过程”的实现方案，例如底栏显示文件总大小、已扫描文件大小。

## 发现
- 添加入口在 `src/composables/useFolderRootActions.ts:168-218`：选择目录 → 重叠检查 → `scan.addScanRoot()` → `scan.startScan()`；`add_scan_root` 本身只注册根目录、建顶级目录和卷绑定，不启动扫描。
- 同一条扫描也被侧栏重扫、重链接后的兜底重扫、复制目录后的新根导入和编辑后的重扫复用；方案应落在 `scanStore`/IPC 进度契约，而不是只给 `addRoot()` 加特殊 UI。
- `start_scan` 在 `src-tauri/src/ipc/scan_commands.rs:526-690` 中先同步等待 `run_fast_scan`，快扫完成后再 `spawn_blocking` 启动后台 `run_enrichment`；因此“扫描完成”不是整条流水线完成，前端必须保留 enriching 运行态直到 `enrichment:completed`。
- 快扫 `run_fast_scan` 在 `src-tauri/src/scanner/fast_scan.rs:413-773` 中以 500 项为批次流式遍历和入库，不预扫总数；实际进度事件 `:704-711` 的 `scanned` 是已处理媒体文件数、`total=0`，`current_dir` 当前发送为空字符串。
- 快扫每个 `FileInfo` 已有 `walked.file_size`（`fast_scan.rs:597-603`），因此不增加额外 stat 就能累加“已处理文件大小”；但这表示逻辑文件体积，不表示真实磁盘读取字节。
- 富化图片事件在 `src-tauri/src/scanner/enricher.rs:141-421` 统计待处理图片并按批发事件；视频/音频在 `:463-675` 各自重新从 0 累计，现有 `MediaEnrichedPayload` 只有 `rootId/enrichedCount/total`，不能形成跨类型连续总进度。
- 前端 `src/stores/scanStore.ts:23-29,198-289` 只保存每根目录的 `scanned/total/currentDir/status/isRunning`；`AppStatusBar.vue:9-16` 只显示“正在扫描…”，`ManagementSection.vue:60-94` 才显示按文件数的不确定/确定进度。
- `scan_roots` 已有 `scan_status/scan_progress/total_files`，但 `finish_scan_root` 写入的是 `inserted`（本轮新增数），且发生在富化之前；它不能直接作为新的总文件数/总字节数真源。
- 扫描 live 进度分成前端专属 Tauri Channel 与 app 级富化事件两条无序传输；`scanStore` 已有终态账本和 10 分钟 watchdog 防止幽灵运行态，但没有扫描进度快照恢复，WebView 刷新后会丢失当前前端进度。
- `AppStatusBar` 在查看器有活动文件时以 `StatusBarFileInfo` 替换扫描信息（`AppStatusBar.vue:9-16`）；这是一项现有产品取舍，若要求扫描期间始终可见，应改为在文件信息旁保留紧凑扫描徽标或提供可展开详情，而不是静默让位。
- UI/UX 检索命中三条与本需求直接相关的通用规则：多步骤过程应显示阶段/进度；长内容应省略并提供展开；动态计数应使用一个有上下文的 `role=status`/原子状态播报，避免裸数字频繁抢读屏焦点。Vue 专项检索无匹配，以下采用通用规则并结合现有 Vue 组件契约。

## 2026-08-26 性能复核

- 当前应用数据库只读核对：`root_id=14` 共 63,685 个媒体文件；其中图片 56,381、视频 2,453、音频 32、文档 4,819。本轮元数据候选为图片/视频/音频合计 58,866 个、493,297,360,580 字节。
- 新增的 `get_metadata_workload` 聚合查询在当前 70 MB 数据库上的查询计划使用 `idx_dir_root`、`sqlite_autoindex_media_items_1` 以及三个元数据表的 INTEGER PRIMARY KEY；5 次冷/热混合只读测量约 21–26 ms。日志中富化开始到候选统计完成约 22 ms，首个图片批次在开始后约 115 ms 发出，因此这条查询不是本次体感变慢的主因。
- 当前一次真实运行日志：快速扫描 63,685 项耗时 5,420 ms；元数据富化总耗时 42,123 ms。图片段 56,381 项从首个批次到最后一个图片批次约 37,765 ms；批次间隔中位数约 220 ms、P90 约 639 ms、最大约 2,074 ms，未呈现随已处理数量单调恶化的趋势。视频/音频段合计 2,485 项约占剩余 4.3 秒。
- 与当前未提交实现的差异对照没有发现解析循环、保留核线程池或批次算法变化；新增工作仅为一次候选聚合、每批读取/累加 `file_size`、扩展事件负载和轻量前端 computed。故当前证据不支持“底栏进度代码直接拖慢文件解析”。
- 观感变化有明确来源：旧逻辑的图片 `total` 只覆盖图片，视频/音频事件又各自从 0 重新计数；新逻辑把 58,866 个跨媒体候选统一作为分母。相同实际处理速度下，进度百分比会比旧界面更慢，但口径现在是连续且诚实的。
- 对当前真实数据库做只读抽样，并按 `parse_exif_meta_buf` 的实际分支条件统计：图片 56,381 个中有 41,830 个（74.2%）会在头缓冲解析失败且缓冲截断时回退到 `parse_exif_meta(path)`；这意味着至少会重新打开原文件走完整解析入口，部分容器还可能继续扫描到更后面的 chunk/segment。按扩展名看，PNG 为 16,035/16,615，JPEG（jpg/jpeg 合计）为 25,595/39,566，PSD/GIF/BMP/WebP 也全部或几乎全部命中。
- 该回退统计不是“必然重新读取完整像素数据”的字节统计，而是当前代码必然进入原文件解析分支的候选数；但它已足够确认是高收益热点。`kamadak-exif` 将“没有 EXIF”明确返回 `Error::NotFound`，当前代码却把所有错误统一按“可能截断”处理，因此无 EXIF 的 PNG/WebP/JPEG 也会触发回退。应先区分 `NotFound`、输入截断/损坏和真正需要扩大读取范围的情况，只对后者回退。
- 优化方向按收益排序：① 修复 EXIF 回退判定，并为 PNG/WebP 增加按 chunk 结构的有界查找，避免扫描像素 payload；② 保持现有正确性后再评估头缓冲上限和已知 `file_size` 复用，减少额外 open/stat；③ 最后用同一真实样本对比保留核池与完整核池，不盲目提高并发。新增候选统计查询、批量 file_size 累加和底栏 computed 均不是当前优先级。
- 本批已落地：JPEG 在 SOS/EOI 后直接判定无 EXIF；PNG/WebP 只读取 chunk 头并 seek 跳过图像 payload；BMP/GIF/PSD 等已知不支持容器直接短路；其余无法判定的 TIFF/HEIF/RAW 仍保留整文件回退，避免损失元数据。快扫 JPEG orientation 也复用同一判断。
- 同一真实库的只读 profile（root=14）显示：`HeaderBuffer=14,390`、`NoMetadata=39,709`、`FullFileFallback=2,108`、`Unsupported=174`、总计 56,381；回退比例从 74.2% 降至 3.7%，回退分支数量减少约 95%。该 profile 包含逐文件诊断调用，不将其总耗时直接当作生产 A/B 耗时；生产日志中的批次回退计数已补齐，下一次真实导入可继续观察总耗时。

## 2026-08-26 并发与磁盘占用复核

- 用户随后又执行了同一目录 `C:/More/0` 的 `root_id=16` 导入，工作量仍为快速扫描 63,685 项、元数据候选 58,866 项 / 493,297,360,580 字节；快速扫描 3,590 ms，元数据补全 31,543 ms，图片主段（到 Live Photo 配对前）约 27,348 ms，视频/音频及配对尾段约 4.2 s。
- 当前机器为 20 个逻辑处理器；`reserved_core_pool()` 使用 `available_parallelism()-1`，图片段实际已配置 19 个 rayon worker。最新图片批次间隔中位数约 145 ms、P90 约 556 ms、最大约 1,167 ms；CPU 约 50% 与磁盘活跃约 90% 的组合更符合 I/O 等待，而不是解析线程数量不足。
- 该 I/O 不是顺序大块读：每张图至少一次 `File::open` + `File::metadata` + 64/128/256 KB 头读；JPEG/PNG/WebP 的格式探测和低频完整回退还可能再次打开原文件。图片富化又按画廊日期/时间序取 500 项一批，序列与磁盘目录物理顺序不一致，容易形成大量跨文件的小读、open/stat/seek 和 Windows Defender 逐次检查。因此任务管理器的“磁盘 90%”不等于能达到 SSD 顺序基准的 5 GB/s。
- 额外内存不是当前直接瓶颈：每批只保留 500 个路径/结果，头缓冲上限为单文件 64–256 KB；19 个 worker 的头缓冲也只是数 MB 级。root=15→root=16 的重复导入中，快速扫描从 5,594 ms 降到 3,590 ms（缓存受益明显），但元数据只从 32,292 ms 降到 31,543 ms（约 2.3%），说明把更多 RAM 用作普通文件缓存不会等比例缩短元数据阶段。
- 当前最值得做的不是盲目增加线程，而是做小型 A/B：①把快扫已记录的 `file_size` 传入头读，尽量避免每文件再次 `metadata()`；②让 PNG/WebP 探测和完整回退复用已打开的文件句柄，减少二次 open；③将读取与解析拆成有界预取队列，比较 19/24/32 个 I/O reader 与 19 个解析 worker；④另测按目录/路径顺序处理，评估局部性收益及其对首屏优先级的影响。批大小 500→1000/2000 可作为低风险对照，主要验证事务和进度事件开销，不应预设它能解决磁盘带宽问题。

## 耐久提升候选补充

| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-006 | 元数据阶段已使用 19 个 worker；瓶颈更像每文件小块/多次打开/非物理顺序 I/O，不能仅凭 SSD 顺序带宽提高并发；路径顺序 A/B 待做 | partial |
| F-007 | 已落地复用扫描阶段 `file_size`、文件句柄与有界头读批次；I/O reader 数与批大小仍需真实导入 A/B | partial |
| F-008 | 快扫在 `start_scan` 返回、富化在后台继续；需要贯穿两阶段的 `runId` 计时与统一汇总字段，才能做端到端 A/B | implemented |

## 2026-08-26 并发与磁盘占用优化实现

- 图像富化现在先用独立的有界头读池读取每批文件头，再在原保留核解析池中消费；头读池按逻辑处理器数的 2 倍计算，封顶 32，解析池仍为 `available_parallelism()-1`。这针对的是多文件小块 IO 等待，不把 SSD 顺序带宽当作单文件顺序读目标。
- 头读复用快扫已记录的 `file_size`，避免新富化路径再次 `metadata()`；`HeaderRead` 同时保留文件句柄，PNG/WebP chunk 探测和无法判定时的完整 EXIF 回退使用该句柄，减少二次 open。旧的公开头缓冲接口保留精确 `file_len` 语义，避免影响快扫调用方。
- 图片批次改为 1,000，头缓冲与句柄生命周期仍限制在单批内；按扩展名头缓冲最坏约 256 MB，通常更低。视频/音频继续使用 500，避免放大单项更重的媒体探测。
- 这批只改变读取调度和资源复用，不改变 EXIF 回退分类的正确性策略；按路径排序暂未落地，因为它会改变当前按画廊视图顺序优先补全占位尺寸的体验，应单独做 A/B。

## 建议的最小实现方向
- 把“逻辑处理进度”定义为 `processedFiles/totalFiles` 与 `processedBytes/totalBytes` 两套同源累计值；`totalBytes` 在快扫单遍流式模型下初期未知，使用不确定进度条，完成/进入富化后以实际候选集合补齐总量。
- 进度 DTO 增加 `phase`、`processedFiles`、`totalFiles`、`processedBytes`、`totalBytes`、`currentPath`（或 `currentDir`）和稳定终态/错误字段；不要让图片/视频/音频各自重置同一组字段。
- 富化启动时一次性确定各类型候选文件数/字节总量，或由 `run_enrichment` 维护整根流水线累计；每个批次用候选项的 `file_size` 累加逻辑处理体积，完成事件只结束任务，不再覆盖进度口径。
- `scanStore` 继续以 `progressMap[rootId]` 为每根目录原子状态，并增加聚合 computed 给底栏；多根并行时按文件数/字节求和，阶段显示为“扫描文件 / 解析元数据”，不显示某个根的局部百分比冒充全局百分比。
- 底栏第一期建议显示：阶段 + `已处理体积 / 总体积`（总量未知时显示“已处理体积 · 正在统计”）+ 可选文件数；点击/悬停再展开根目录、当前路径、取消按钮和“不确定”说明。保留侧栏的每根进度条作为详细视图。
- 进度事件后端只在批提交后发送，并在前端按约 200–500ms 节流渲染/aria 状态更新；不得逐文件 emit，也不应把每次进度更新写入 DB。
- 若需要刷新/页面重建后仍可感知，新增 `SCAN_STATUS` 快照 IPC 或把当前运行状态放进 `AppState`，先订阅 app 级事件再取快照；不要复用 `scan_roots` 的旧 `total_files` 字段承载新语义。

## 推荐实现方案

### 1. 先固定进度口径

- “文件总大小”默认指本次会被 Scrollery 索引的受支持媒体文件大小之和，不是目录内所有文件大小；未知扩展名、被排除文件和无法访问文件不进入该口径。若产品要展示目录物理总大小，必须另做文件系统 inventory，不能复用媒体扫描计数。
- “已扫描文件大小”指扫描器已经完成当前批次入库提交的媒体文件 `file_size` 累加；它是逻辑文件体积，不是 EXIF/视频探测实际读取的字节数。
- 快速扫描会在 walker 层剪枝，未剪枝目录的文件才产生 `WalkedFile`。因此不能把快速扫描中的 `processedBytes` 直接与全根媒体总量相除。建议同时保留 `checkedFiles/checkedBytes`（已检查/确认的范围）和 `processedFiles/processedBytes`（本轮实际入库处理范围）；第一期底栏只展示当前阶段明确标注的那一组，避免产生虚假的百分比。
- 首次全量扫描采用单遍流式遍历时，扫描开始阶段准确 `totalBytes` 必然未知。底栏应显示“已处理 1.2 GB · 总大小统计中”，扫描结束后补齐总量；若要求从 0% 就显示准确总大小，再增加独立 inventory 阶段，但要接受二次遍历、额外 stat/IO 和更复杂的取消语义。

### 2. 统一后端进度快照

建议把现有 `ScanProgressPayload`/`MediaEnrichedPayload` 收敛到同一套阶段快照，字段可按以下语义设计（名称可按项目惯例调整）：

```ts
interface ScanProgressSnapshot {
  rootId: number
  runId: string
  phase: 'discovering' | 'scanning' | 'metadata'
  processedFiles: number
  totalFiles: number | null
  processedBytes: number
  totalBytes: number | null
  checkedFiles?: number
  checkedBytes?: number
  currentPath: string | null
  terminal?: 'completed' | 'cancelled' | 'error'
  errorCode?: string
}
```

- `runId` 是必要字段：当前停止/重扫会取消旧任务，但旧富化任务仍可能晚到发送终态；前端必须只接受当前 `runId`，否则旧事件会把新任务错误置为完成或覆盖字节数。
- 快扫每批事务提交后，从 `FileInfo.walked.file_size` 累加并发快照；事件必须发生在提交之后，保证“已处理”与数据库可见状态一致。不要逐文件 emit，也不要每次进度更新写 `scan_roots`。
- 快扫结束时确定本轮实际发现的文件数/字节数，并发出扫描阶段终态或下一阶段的初始快照。快速扫描剪枝范围另按 `checked*` 统计；如果第一期不实现 `checked*`，底栏文案必须明确为“本轮处理”，不要叫“全库总量”。
- 元数据分析启动时一次查询待分析图片、视频、音频候选的总数和 `SUM(file_size)`，随后由一个跨媒体类型的累计器递增。图片、视频、音频不能各自把 `enrichedCount` 从 0 开始复用；查询需使用 `COALESCE(SUM(file_size), 0)` 并绑定 root 参数。
- 元数据阶段的总量应是“待分析候选”总量，底栏显示“解析元数据 420 MB / 1.1 GB”，不要把扫描阶段和元数据阶段的字节相加后伪装成单一百分比；两阶段处理的集合和成本不同。
- 完成、取消、错误都必须发送一次带 `runId` 的终态；错误只暴露稳定 `errorCode`/`variant`，不把内部路径或原始错误字符串直接放进 IPC/UI。

传输上，最小改造可继续保留 `start_scan` 的 Channel，同时把富化事件扩展为同一快照格式；更稳妥的后续方案是所有 UI 进度统一为 app 级 `scan:progress` 事件，并在 `AppState` 保留活动任务快照，增加 `SCAN_STATUS` 查询供 WebView 刷新恢复。不要长期维护“快扫一条 Channel、富化一条 app event、字段口径各自不同”的双轨状态。

### 3. 前端和底栏交互

- `scanStore` 保存每个 `rootId` 的完整快照，并以 `runId` 做过期事件过滤；新增聚合 computed，对多个根目录分别求和 `processed*`/`total*`，只有所有参与根都有确定总量时才显示全局百分比。
- 新建一个紧凑的 `ScanProgressIndicator`，由底栏独立渲染，不让查看器的 `StatusBarFileInfo` 或选区停靠条把扫描状态完全替换掉。28px 底栏内展示“阶段 + 体积 + 文件数”，空间不足时只保留体积摘要，点击展开详情。
- 建议文案：`扫描文件 · 1.2 / 3.4 GB · 8,432 / 21,091`；总量未知时：`扫描文件 · 已处理 1.2 GB · 总大小统计中`；富化时：`解析元数据 · 420 MB / 1.1 GB`。当前路径在展开面板内省略显示，避免长路径挤压底栏。
- 展开面板显示每个根目录、阶段、文件/字节进度、当前路径、取消按钮和“不确定进度”的说明；侧栏保留每根目录的细节进度条。多个根并行时不要用单根百分比冒充全局百分比。
- 动态摘要使用一个有上下文的 `role="status"`/`aria-live="polite"`，前端按约 200–500ms 节流状态文本和视觉刷新；文件数量/字节数变化不应逐批抢读屏焦点。未知总量使用不确定进度样式，不显示 0% 或虚假完成率。

### 4. 推荐分批落地

1. **MVP：统一扫描阶段体积进度。** 扩展 Rust/TS DTO，快扫批提交后累加 `processedBytes`，底栏显示阶段、已处理体积、文件数和“总大小统计中”；同时补 `runId`，修复旧事件污染新任务的边界。
2. **第二批：统一元数据阶段。** 为图片/视频/音频建立统一候选总数/总字节查询和累计器，移除各类型独立重置；底栏从“扫描文件”切换为“解析元数据”，直到 `enrichment:completed` 才结束。
3. **第三批：快速扫描与恢复。** 增加 `checked*` 或等价文案，补活动任务快照 IPC/app state，支持 WebView 刷新恢复；此批再决定是否需要准确的首屏总量 inventory。

不建议第一批就引入全目录二次遍历或把所有中间状态持久化到数据库：它们会放大 IO、取消、异常恢复和数据库写入面，而当前需求首先是让过程可感知。

## 验收与测试

- 前端单测：未知总量不显示百分比；单根/多根聚合；零候选；扫描切换元数据阶段；取消/错误/终态；旧 `runId` 事件被忽略；字节格式化和大于 TB 的边界。
- Rust 单测：批提交后文件数与字节数单调递增；扫描终态补齐总量；图片/视频/音频元数据累计不重置；空候选、取消、失败均发送终态；payload 序列化字段使用 camelCase 且错误字段稳定。
- 手动回归：新目录首次导入、已有目录快速重扫、混合图片/视频/音频、多个根并行、扫描中取消后立即重扫、文件在扫描期间增删/离线、查看器打开、选区停靠、窄窗口、中文/英文、深色主题和 WebView 刷新。
- 性能验收：事件只在批提交后发送；前端状态更新节流；扫描线程不等待 UI；不新增逐文件 DB 写入；大目录内存仍保持批量级而不是收集全量文件。

## 本轮落地结果

- [x] `ScanProgressPayload`/`ScanCompletedPayload` 增加 `runId`、`processedBytes`/`totalBytes`；快扫批提交后累计 `WalkedFile.file_size`，并上报当前目录。
- [x] `MediaEnrichedPayload` 增加 `runId`、累计处理体积和候选总量；图片/视频/音频共享同一累计器，root=0 的画廊刷新哨兵保持非扫描语义。
- [x] `scanStore` 增加每根目录字节状态和多根聚合状态，过期 Channel/事件按 `runId` 丢弃；底栏新增 `ScanProgressIndicator`，在查看器/选区状态下仍位于底栏右侧。
- [x] 底栏支持中英文阶段文案、未知总量文案、逻辑文件体积格式化和 `role="status"`/`aria-live="polite"`。
- [ ] 未实现活动扫描快照 IPC、WebView 刷新恢复和 quick 剪枝目录的 `checkedBytes`；这些保持为后续批次，避免当前把“实际处理体积”误标为全根扫描体积。

## 本轮实现文件

- 前端：[src/stores/scanStore.ts](C:/workspace/scrollery/src/stores/scanStore.ts)、[src/components/layout/ScanProgressIndicator.vue](C:/workspace/scrollery/src/components/layout/ScanProgressIndicator.vue)、[src/components/layout/AppStatusBar.vue](C:/workspace/scrollery/src/components/layout/AppStatusBar.vue)、[src/types/ipc.ts](C:/workspace/scrollery/src/types/ipc.ts)。
- Rust：[src-tauri/src/scanner/fast_scan.rs](C:/workspace/scrollery/src-tauri/src/scanner/fast_scan.rs)、[src-tauri/src/scanner/enricher.rs](C:/workspace/scrollery/src-tauri/src/scanner/enricher.rs)、[src-tauri/src/db/queries/metadata.rs](C:/workspace/scrollery/src-tauri/src/db/queries/metadata.rs)、[src-tauri/src/ipc/scan_commands.rs](C:/workspace/scrollery/src-tauri/src/ipc/scan_commands.rs)。

## 关键风险/待裁决
- 是否接受首轮扫描阶段总大小未知：接受则保留单遍流式与低 IO；若强制首屏显示准确总大小，需要二次遍历/统计阶段或临时队列表，会改变扫描性能与取消语义。
- “已扫描文件大小”应定义为已处理文件的逻辑大小，不应定义为底层实际 read bytes；后者会因 EXIF 头读、视频探测和缓存策略不可比。
- 底栏与查看器文件信息、选区停靠条、沉浸隐藏之间需要确定优先级；建议扫描任务属于全局高优先级，至少保留一个紧凑状态徽标。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 扫描进度单遍流式、总大小初期未知；准确总量与低 IO 存在取舍 | design |
| F-002 | 富化图片/视频/音频现有事件口径会重置，需统一阶段快照模型 | design |
| F-003 | 底栏当前只显示简单扫描文案，且查看器页替换扫描信息 | design |
| F-004 | 当前新增候选统计查询仅约 21–26 ms；不应因体感变慢直接回滚进度实现 | no-promotion |
| F-005 | 真实库中 41,830/56,381（74.2%）图片命中“头解析失败后原文件回退”；已按格式完成回退分类与容器级有界查找，profile 降至 2,108/56,381（3.7%） | implemented |
| F-006 | 当前日志文件可正常创建和持续写入；重跑会话无 `start_scan`/扫描日志，且 `scan_roots.last_scan_at` 未变化，故本次问题是扫描命令未触发，不是日志初始化或写盘失败 | diagnosis |
| F-007 | `OnboardingWizard.pickFolder()` 只登记扫描根、不启动扫描；若“重新导入”从该入口执行，会出现无扫描日志的表现 | code-follow-up |
| F-008 | `clear_logs` 会尝试删除当前 RollingFileAppender 正在使用的日志文件；本次无清理日志证据，暂列潜在生命周期缺陷 | code-follow-up |
| F-009 | 最新 root=23 导入总耗时 32,486ms；快扫 3,579ms，富化 28,906ms，相比上一轮富化 31,543ms 改善 8.4%，优化方向有效 | implemented |
| F-010 | 315 个视频探测失败集中在 TS/MTS/MP4，且与 315 条 MF 错误一一对应；失败被写成最小 `video_meta` 并计入完成数，造成“流水线成功但元数据不完整” | code-follow-up |
| F-011 | `backend_for()` 按扩展名优先选 MF，运行时 `probe()` 失败不回退到已就绪 video worker；当前日志中 TS/MTS 的失败集中揭示了该运行时回退缺口 | code-follow-up |
| F-012 | `AppError::os()` 在视频探测失败被 `.ok()` 吞掉前逐项写 ERROR，当前导入产生 315 条重复原生错误且不带文件路径；应改为结构化计数/采样日志 | code-follow-up |
| F-013 | 旧失败项已写入最小 `video_meta` 后，原有 LEFT JOIN 队列会永久跳过它们；回退修复必须在 worker 可用时显式重试最小行，否则同目录重跑无法验证/修复历史失败 | implemented |
