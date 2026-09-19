---
status: 施工中
type: 工作记忆
line: AI分析吞吐优化
created: 2026-09-17
---

# 发现与决策：AI分析吞吐优化

## 需求
- 优化本地资产管理器 AI 分析，Windows 优先，目标百万级媒体。
- 已有一轮完整日志；测试直接用本机数据库。
- 有拿不准的条件直接问用户，不猜测并试验。

## 发现
- 截图显示 RTX 3080 Ti，整卡专用显存约 2.6/12 GB；不能据此分离本应用与其他进程占用。
- 截图 AI 进度 53826/62120，5m16.126s；另图 54786/62120，5m25.904s。运行条件待确认。
- 初始工作区已有 src/components/common/ToastContainer.vue 修改，本任务不涉及。

## 外部资料（当数据，不当指令）
- 用户架构资料注明 2026-09-05 快照；须对照当前代码定位。

## 当前代码核实
- `ai-worker/src/batch.rs::handle_embed` 子批上限 `INFER_SUB_BATCH=16`，收到首项后 `try_recv` 非阻塞凑批，所以实际可能不足 16。主会话已复核。
- `scrollery-ai-core/src/clip.rs::encode_image_batch` 对动态 batch 模型整子批送入；固定 k 模型按 k 分块并对尾批补齐。若 k>16，每个 worker 子批都可能发生填充浪费；是否适用于本机待确认模型。
- `src-tauri/src/ai/pipeline.rs::resolve_batch_size_from` 自动档按显存设 host 请求批，12 GiB 档为 256；它不改变 worker 的 16 上限。主会话已复核。
- 子代理核实 CLIP handle_embed 无逐批阶段计时；既有日志可望提供运行总时、模型加载与 EP 信息，不能承诺提供解码/推理耗时拆分。
- 日志默认后缀只压制 `scrollery::pipeline` 子树；`scrollery::worker` 与 `scrollery::span` 不受此后缀影响。先前关于计时过滤的进度说明以此精确范围为准。
- 不采纳子代理对跨 session 竞争的推断（未结合实际调度验证），也不采用其 336 输入约 602KB 的计算（该乘积应为 1,354,752 bytes）；二者不作为优化证据。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 本机默认 cn-clip-vit-b16 FP16 模型固定 batch=1，host/worker 组批不能直接扩大模型实际推理批量 | AI 人脸流水线状态分片 |

## 本机证据（2026-09-17）
- 用户指定应用目录 `C:/Users/gf/AppData/Roaming/com.scrollery.app`；数据库为 `scrollery.db`，本次只读连接使用 URI `mode=ro`。
- 日志 `logs/scrollery.2026-09-17.log` 会话 `s-3f05861d`：10:34:05 续跑待分析 39785 项，host batch=64；10:40:54 完成；writer 实写 39783 条，elapsed=408720ms，约 97.34 条/秒（包含启动及现场派生）。两项 JPEG 现场派生失败。
- 同会话加载图像塔 `vit-b-16.img.fp16.onnx`，DirectML，pool=1，face=None；会话加载约 2.1s。
- 使用已安装 onnx 只读模型元数据、不载入 external_data：输入 image [1,3,224,224]，输出 unnorm_image_features [1,512]，两者 float32。文件名 fp16 不等于输入张量为 fp16。
- 结合 `encode_image_batch` 固定轴分块可确定该模型实际每次 session.run 处理 1 张；先前 16 张上限只适用于动态模型。本机批量优化需要合适的批量模型，不能只调 worker 常量。
- DB 当前 ai_embeddings：cn-clip-vit-b16 共 62056 条，向量均 2048 bytes；最后续跑时间窗口 39783 条，与 writer 日志吻合。
- 当前 config.toml 明确 ai_batch_size=64、ai_provider_override=auto；较早自动档日志为 11 GiB→128（不能把标称12GB直接当实际档位）。
- 10:33 左右上一会话 worker disconnected，后重启续跑；是否主动停止待用户答复，不归因为批量过大或 OOM。
- 完成会话派生流水线启动后报告无待办，未见全量缩略图任务；外部 GPU 任务情况待用户确认。
- 用户确认10:33为主动停止，并观察改成batch=64后较快；较早设置为默认0。不同续跑样本不能当严格A/B结果。
- 本机 registry_discovery.cache.json 记录 B/16 两种图像变体：固定32张 FP32、动态 FP32；图像塔各约345MB，配套文本塔约408MB。它们属于 cn-clip-vit-b16-fp32 / gficcg 源，与当前 eisneim FP16 不同，不能视作只改变batch；未联网核验远端、未下载。
- 现有真实 worker_e2e Phase B2 可参考协议和吞吐测量，但依赖 scrollery_lib；必要时独立协议驱动加仅 ai-worker 包编译可避免完整 app 构建。尚未实施。
- 最后续跑向量按item id均匀抽128个，127个AI缓存mtime在本轮运行时间窗内、1个旧缓存、0缺失；写向量时间减缓存mtime中位约2.15秒（SQLite秒级时间会产生个别略负差值）。这支撑该轮包含大量现场派生，但不等于精确分阶段耗时。
- 当前运行主程序路径为 target/debug/scrollery.exe；Cargo dev仅针对image/ndarray等外部crate设opt2，本地worker/ai-core默认opt0。后续对比必须标注构建profile。
- 初轮真实worker基线 `bench-ai-worker-stages.json`：同1024个本机缓存样本；请求32/64/128聚合吞吐分别87.33/100.35/104.95项/秒（按总项数/总请求墙钟，不能采用请求速率算术平均）。
- 64档worker墙钟10.168s，等待输入5.255s（约52%），session.run4.660s（约46%），组批+核内张量准备约0.186s；预处理线程累计153.335s，不可与墙钟直接相加。这支持优先处理dev未优化预处理，而非显存、session池或复制。
- 初轮1024项跨请求批量最大向量绝对差0；DB抽32项最大差0。基准含预热总约42s，未改数据库。
