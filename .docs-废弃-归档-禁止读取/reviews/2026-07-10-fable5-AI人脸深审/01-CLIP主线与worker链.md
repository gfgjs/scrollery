---
id: 2026-07-10-01-CLIP主线与worker链
status: snapshot
type: review
line: AI 人脸流水线深审
created: 2026-07-10
---

# 分册 01:CLIP 语义主线 + worker 进程链

> 快照 2026-07-10,dev `68e23e8`。编号与 [README 总表](README.md) 一致;W=worker 链,A=CLIP 主线/命令面。
> ⚠️ **时效**:W1/W2/W3/A1/A2/A7/A11/A13 已于同日修复(交付清单见 [README 文首标注](README.md)),本册对应段落为修复前现状记录。

## 1. 架构数据流(现状,亲读核实)

```
扫描入库(ai_status=0)
 → start:sync_ai_status_for_model(按激活模型向量覆盖重指 status;Error 项一并回 0 重试)
 → F5 GPU 分析槽互斥(gpu_analysis_owner,CLIP↔face 单持有者,同 owner 可重入)
 → run_pipeline_worker_blocking(rayon scope 三线程):
    Producer(512/批领取→标 Processing→AiTask 通道;每轮让步 scan>thumb>derive>exotic)
    Dispatcher(攒批≤batch_size,50ms 空闲刷 → CPU permit → 缺 ai_cache 现场派生(rayon,tmp→rename)
               → GPU 令牌(全局恒 1)→ 锁 ai_worker → EmbedBatch(worker 端批内解码/推理流水重叠))
    Writer(512/批 upsert ai_embeddings + 标 Done;失败批标 Error;每 flush 失效常驻嵌入缓存)
 → 搜索:EncodeText(worker)→ ensure_cache(整表→f16 常驻)→ rayon 点积 → top-k
        → DELETE+INSERT ai_search_results → 画廊 JOIN 呈现
worker 生命周期:spawn(握手 5s)→ SessionInit(len+sha256 逐模型,300s 档)→ 批请求(硬止损=重建重发一次)
               → 运行结束 close_session 释放 VRAM;worker 空闲 300s 自杀兜底
```

大 payload 结论(亲核):图像字节**从不过管道**(EmbedBatch 只传 cache_key,worker 自读缓存;face 批传 source_path 自读);嵌入回程走 Success 帧同帧二进制 blob(f32 LE),零 base64/JSON 膨胀;batch.rs 是**真批推理**(动态轴整批一次送入,固定 batch=k 分块+尾批补齐)。

## 2. worker 进程链发现(W)

### W1 🟠 worker 空闲自杀后 host `alive` 陈旧 + SessionInit 失败不入重试圈 → 间歇性一次失败 【亲证】
- 位置:[worker_client.rs:210](../../../src-tauri/src/ai/worker_client.rs#L210)(`ensure_session(...)?` 的 `?` 直接跳出 `run_validated` 的 attempt 循环)、[supervisor.rs:87,142-144](../../../src-tauri/src/exotic/supervisor.rs#L142)(`is_alive()` 只读旗标,不 `try_wait` 探活)、ai-worker main.rs:35(`IDLE_SELF_EXIT=300s`)。
- 链路:每轮流水线/切模型都 `close_session`(快照→None)→ worker 空闲 300s 自杀 → host 旗标仍 true → 下次 `ensure_worker` 误判存活 → `init_session` 写死管道 → Disconnected → `ensure_session` 返回 Err → `?` 冒泡,**MAX_ATTEMPTS=2 的重建重发完全没机会介入**。
- 影响:上轮分析结束(或一次搜索后)超过 5 分钟,用户再点「开始分析」→ 整轮 fatal;再点一次就好(kill_and_reap 已把旗标置 false)。「搜索→自杀→再搜索」路径反而能恢复(那时失败发生在 run_batch,落在 attempt 圈内)——两条路径行为不对称正是病灶。
- 建议(任一,①最小):① ensure_session 的进程级失败(TimedOut/Disconnected/Protocol)纳入 attempt 循环——失败即 `drop_worker()` + continue,仅数据类 Failure(ModelLoadFailed 等)冒泡;② `is_alive()` 补 `child.try_wait()` 探活;③ host 记录 close_session 时刻,超 IDLE_SELF_EXIT 未通信即预防性重建。

### W2 🟠 `FailureBody.retryable` 语义被整体丢弃 【亲证】
- 位置:[worker_client.rs:230-236](../../../src-tauri/src/ai/worker_client.rs#L230):除 SessionExpired 外一切 Failure `return Err`,不看 `fb.retryable`;对照协议 message.rs(InternalError/GpuUnavailable/IoError 均 default_retryable=true)与 worker batch.rs:248-256(一次瞬态推理失败 → 整批 InternalError,retryable=true)。
- 影响:DirectML 偶发内核错误/显存瞬时紧张一次 → host 直接 Err → dispatch 视为批级致命 → **整轮流水线终止**,且自然完成分支对 Err 同样清 `ai_analysis_active`,自动续传也不救。模块头注释「terminal Failure → 不重试」与实现不符(实现是「一切 Failure 不重试」)。
- 建议:Failure 臂改 `if fb.retryable { warn + 计 attempt 重试 } else { return Err }`;GpuUnavailable 可附加「下轮降 CPU EP」决策。

### W3 🟡 `SessionSpec::matches` 不比对 batch_size,worker 硬拒超限批 【亲证】
- 位置:[worker_client.rs:73-80](../../../src-tauri/src/ai/worker_client.rs#L73)(只比 arch/image_file/face_profile);ai-worker batch.rs:134-144 与 :424-434(`items.len() > sess.batch_size` → MalformedInput,terminal);SessionDescriptor 未存 batch_size。
- 触发:搜索先建会话(batch=X)→ 用户调大 `ai_batch_size` → 300s 内点开始分析 → 会话复用 → 首批 Y>X 项 → worker 拒 terminal → 叠加 W2,整轮终止且重试同死。
- 建议:SessionDescriptor 记 batch_size,matches 要求 `spec.batch <= desc.batch`;或派发侧以 `min(spec, desc)` 为攒批上限。

### W4 🟡 `AppState.ai_worker` 粗粒度 Mutex 跨整批持有 【子代理报告】
- 位置:worker_pipeline.rs:243-246、face_pipeline.rs:866-869(持锁跨完整批往返:EMBED_BATCH 120s 上限、face 60+6n 秒、首批叠加 SessionInit 300s 档);被阻塞方 ai_commands.rs:180/220/305(状态命令、语义搜索)。
- 影响:分析进行中状态栏轮询在 blocking 池排队等锁(每次占一根线程直到批结束);搜索最坏等一整批+冷加载;std Mutex 无公平性保证。
- 建议:①会话快照拆独立 `RwLock<Option<SessionDescriptor>>`(状态命令零等待);②状态命令 try_lock 失败回退 DB 配置值;③搜索批间插队为后续可选。

### W5 🟡 每次 SessionInit worker 端全量重算 GB 级模型 sha256 【子代理报告】
- 位置:ai-worker session.rs:175(每模型 `sha256_file`);对照 host 侧 worker_client.rs:83-88/441-451 有 mtime+len 备忘。每轮运行必 close→init:worker 进程存活期间下轮 init 就要对 ViT-L fp32(~1.6GB)+文本塔+face 双模型全量读盘哈希,数秒到十数秒纯浪费。
- 建议:worker 进程内按 `(canonical_path, len, mtime)` 备忘(进程生存期有效,重建自然重验,不降低「每进程至少全验一次」下限);此为对 D1 校验纪律的收窄,建议以附注形式回写裁决。

### W6 🟡 worker 错误码在 IPC 边界压平为 System 文本 【子代理报告,与前端 U20 互证】
- 位置:worker_client.rs:181-193/230-235(`AppError::System(format!("...[{code}]:{msg}"))`)。AppError 本身有手写 Serialize + 稳定 code(形式合规),但 `model_load_failed`(应提示重下载)、`gpu_unavailable`(应提示切 CPU)等 worker 子码只作为字符串嵌进 message,前端(utils/ipc.ts:9 注释自证)只能按 code='Unknown'/串匹配兜底。
- 建议:AppError 增 `AiWorker { code, message }` 变体;至少把 ModelLoadFailed 映射到 AiModelNotLoaded。

### W7 🔵 host 侧 frame reader channel 无界(supervisor.rs:106)
「不信任 worker」纪律下,失控 worker 可在 host 不 recv 的间隙持续写帧灌爆内存(每帧上限 65MiB、条数无限)。建议有界 channel(2-4),队满即判协议违例 kill。

### W8 🔵 取消 = 杀进程 + 用户取消被记为 pipeline error(与 A16 同族)
worker.rs:281-283(cancelled → Disconnected)→ kill;dispatch 层对取消引发的 Err 无差别 fatal → 日志记「流水线错误:…已取消」。暂停这种高频操作弃掉整个进程+会话,恢复需重 spawn+SessionInit(冷载分钟级)。建议:dispatch 拿到 Err 先查 token,取消则 Ok 收尾;批内取消可先等 1-2s 自然完成再 kill。

### W9 🔵 main.rs Hello 版本宽容分支是死代码(帧层已硬拒版本不符);W10 🔵 face 批回声指纹不含内容指纹(fingerprint=item_id+阈值,无 mtime;现同步单飞无害,记档防退化)。

### W11 💡 host 派生与 worker 推理批间无重叠(双缓冲缺失)
worker 内部解码/推理已流水重叠(T18.5b,做得好),但 host 侧「缺 ai_cache 现场派生」(首跑全库普遍路径)与上一批推理完全串行——批耗时=两段之和。可做批级双缓冲(本批发出 IPC 后即开始下一批派生;CPU permit 的 1批=1槽记账须拆分)。首跑吞吐预估提升明显(两段取大)。

### W12 💡 handle_face 每 chunk 重建线程组(batch.rs:452-480;handle_embed 已示范长驻 scope+原子游标,对齐即可);W13 💡 f32↔bytes 逐元素转换可 `bytemuck::cast_slice` 整段化(非热点,顺手)。

## 3. CLIP 主线发现(A)

### A1 🟠 GPU 分析槽泄漏:三个命令 cancel 后不释放 【亲证】
- 位置:[ai_commands.rs:554](../../../src-tauri/src/ipc/ai_commands.rs#L554)(reload_ai_engine)、:573(rebuild_embeddings)、:783(set_active_model)——均 `cancel_ai_analysis()` 而无 `release_gpu_analysis(GPU_OWNER_AI)`;pipeline 完成回调仅自然完成时释放(pipeline.rs:86-98);pause/stop 有释放(:456/:480)。
- 语义(亲证 state.rs:377-403):try_acquire 对**同 owner 可重入** → 泄漏后 CLIP 自身仍能重启,**face 分析被永久拒绝**(「语义分析正在进行,请先暂停」但实际什么都没跑),直到 CLIP 再自然跑完一轮或 pause/stop。face 侧对称命令 `set_active_face_model` **有释放**(face_commands.rs:598)——不对称即证据。
- 建议:三处 cancel 后补 release(带 owner 校验,误释放安全);或释放收敛进 cancel_ai_analysis(restart 先 acquire 再 cancel,可重入,语义兼容)。

### A2 🟠 双 VRAM 阶梯打架:get_ai_status 把「自动」钉死成小 batch 【亲证】
- 位置:[ai_commands.rs:242-262](../../../src-tauri/src/ipc/ai_commands.rs#L242) vs [pipeline.rs:213-251](../../../src-tauri/src/ai/pipeline.rs#L213)。resolve_batch_size 约定 0/缺省=VRAM 自动档(≥12GB→256/≥8GB→128);但 get_ai_status(状态栏轮询,必然先于任何分析)发现键缺失时按**另一套阶梯**(≥8GB→64)算默认并 **set_config 写库** → 此后自动档永不生效。12GB 机器本应 256,被钉死 64,批吞吐砍 3/4;读命令有写副作用本身也违背直觉。
- 建议:get_ai_status 只算展示值不落库;或统一走 resolve_batch_size 并保留 0=自动语义。

### A3 🟡 取消不等待旧 run 收尾:切换/重启与 Writer 尾部 flush 竞态 【子代理报告】
- 位置:pipeline.rs:310-317(取消后仍 flush 余批并标 Done,用旧 profile.id)+ ai_commands.rs:783-796(set_active_model cancel 后立即 sync)与 :410-431(restart reset 后旧 flush 可「复活」旧向量)。
- 危害被两层兜住:搜索/计数只看 ai_embeddings(按 model_name);**下次 start 的 sync 自愈**。残留:「切换后不点开始直接搜索」期间覆盖判断短暂错位;restart 静默少重算若干项。
- 建议:取消型命令等待 run 真正退出(完成时 Notify/generation 计数);或 Writer 在取消分支丢弃余批不标 Done(在途项有孤儿恢复兜底,语义一致更简单)。**注意 face 侧无 start-sync 自愈,同窗口危害更持久,见 X2。**

### A4 🟡 Producer 标记 Processing 失败仍继续派发(pipeline.rs:179-187;face 同构 face_pipeline.rs:206-215)【亲读印证】
失败只 warn 就发通道 → 下轮同批再取再发;持续写失败(磁盘满)时无限空转。建议改与查询失败同臂:break,本批保持 Pending 下次重来。

### A5 🟡 start 不 join 旧 run:孤儿恢复与旧 Producer 交错可漏件(自愈)【子代理报告】
新 run 的 recover(1→0)可能先执行,旧 Producer 随后把新领一批标回 1 发进已无消费者的旧通道 → 本轮观感「跑完仍剩 N 张」。下次 start 自愈。修法同 A3 的轻量 join。

### A6 → 并入 K1(见分册 02)。原提议「host 比对 desc.embed_dim vs profile」经亲证**无效**:SessionReady 的 embed_dim 来自注册表 profile(ai-worker main.rs:224),两边恒等,比对永不触发。错配模型场景真实存在,但唯一有效防线在 ai-core 输出形状断言。

### A7 🟡 semantic_search_with_vector 不校验查询向量长度 【亲读印证】
- 位置:[search.rs:147-153](../../../src-tauri/src/ai/search.rs#L147):打分内积按缓存 dim 索引 `q[k]`,无 `query_vec.len()==dim` 守卫。K1 场景(会话 dim≠缓存 dim)下 rayon 线程 panic → JoinError → 前端收笼统 Internal。同文件 cosine_similarity(:201-221)专门为同类问题加过防线并写明教训,这条新入口没继承。
- 建议:入口一行 guard,长度不符返回结构化错误。

### A8 🟡 get_pending_ai_items 每批全量排序(无索引支撑)→ 见 [04-性能路线图 P1](04-性能优化路线图.md)(face F19 同病)。

### A9 🟡 嵌入缓存装载峰值 ≈3× 稳态(search.rs:66-86 + get_all_embeddings 一次性 collect)→ 04-P2。

### A10 🟡 分析期间每 512 条 flush 整体失效缓存 → 边分析边搜反复全量重载 → 04-P3(vector_store 增量 upsert 是既定消费点,dormant 有主,勿另起炉灶)。

### A11 🟡 CLIP Error 项永远计入 pending,进度到不了 100%,且摘要无 error 计数 【子代理报告】
ai_commands.rs:213-216(pending=total−embeddings 行数);对照 face 侧特意用 IN(2,3) 让进度到 100%(queries.rs:3257-3269 注释明言)。两侧口径不一致 + 失败面对 UI 完全不可见。建议:摘要补 error_items,pending=total−done−error;失败清单/单项重试见功能缺口册。

### A12 🟡 count_embeddings_for_model 不滤软删项 → 软删后 pending 低估(queries.rs:2944-2951;saturating_sub 兜住不为负)。建议 JOIN media_items 过滤。

### A13 🟡 fetch_tree 裸 reqwest::Client::new() 无超时(remote_registry.rs:244)
半开连接时 list_model_registry/download_model 挂到 OS 层 TCP 超时(分钟级),磁盘 L2 兜底要等失败返回才触发。建议复用 `download::secure_client(SmallFile)`(顺带 HTTPS 强制+重定向加固)。

### A14 🟡 EMBED_BATCH 超时固定 120s 不随批缩放 【子代理报告】
coordinator.rs:52;对照 :62-64 face_detect_embed 已按项数缩放并留有 2026-07-03「固定超时对全尺寸批必然误杀」教训注。batch 上限 256 + CPU EP(无独显/override cpu)极可能超 120s → 超时 kill → 重发再超时 → 硬止损 → 整轮 fatal。同一学费不应付两次。建议仿 face 做 `BASE + PER_ITEM×n`;或 provider 回声为 cpu 时把有效 batch 钳小。

### A15 🔵 下载路径多处同步 fs 直跑 async fn(ai_commands.rs:865/1029/1075/1098/1109;GB 级 .part 删除/rename 慢盘可卡 tokio worker;tripwire 只锁 rusqlite 抓不到)。顺手改 tokio::fs 或包 spawn_blocking。

### A16 🔵 取消被当 fatal 错误上报日志(与 W8 同族,修一处即消两处)。

### A17 🔵 ai_search_results 全局单会话表,last-writer-wins,无代次护栏(search.rs:177-190)。单窗桌面可接受;前端配套问题见 U4(建议前端代次令牌先行,表加 query_hash 列待多窗需求)。

### A18 🔵 remote_registry CACHE 四处裸 `unwrap()`,与全库毒锁纪律(`unwrap_or_else(into_inner)`)漂移(remote_registry.rs:149/165/176/191)。

### A19 💡 vocab.txt 仅 size 校验(sha256=None,有 min_vocab 兜底):内容钉死的上游资产,可一次性人工核出 sha256 硬编码归零风险。

### A20 💡 import_ai_model 零校验裸拷贝(ai_commands.rs:525-545):连 .onnx 扩展名都不查,是 K1 错配场景的主要入口。至少校验文件名可被 arch_for_image_file 解析。

## 4. 红线合规核对(worker 链 + CLIP 命令面,亲核+子代理双查)

| 红线 | 结论 |
|---|---|
| async command 内 rusqlite 走 spawn_blocking | **合规**:ai_commands 全部 15 命令逐条核对通过;blocking.rs tripwire 测试覆盖 ipc/ 全目录。残留仅 A15 的同步 fs(非 rusqlite) |
| std::sync::Mutex 不跨 .await | **合规**:ai_worker/db_writer 全部调用点位于 spawn_blocking 闭包或专职阻塞线程;毒锁一律 into_inner 恢复(例外:A18 风格漂移) |
| IPC 错误结构化 + 稳定 code | **形式合规、语义退化**(W6):AppError 手写 Serialize、code 稳定;worker 子码被压平进 System 文本 |
| 派生产物原子落盘 | **合规**:ai_cache/face_cache tmp→rename 有单测;下载 .part→校验→rename;registry 磁盘缓存同纪律 |
| SQL 参数绑定 | **合规**:AI 段全部 params![],format! 只拼常量片段与占位序号 |
