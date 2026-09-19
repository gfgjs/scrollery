---
id: 2026-07-02-Part4-D3-长驻Session握手与Supervisor改造
status: active
type: design
line: Part4 worker 化
created: 2026-07-02
---

# Part4 worker 化 · D3:长驻 Session 握手与 Supervisor 改造(§8.2.3 落定稿)

> 状态:定稿(2026-07-02);**§3 字段形状经 T10 落地修正(2026-07-03,见 §3 横幅),实现以 `crates/exotic-protocol/src/message.rs` 为权威**。上游:Part4 §8.2.3 / §3.1.1-3.1.2、Part6 G1/G4/G6 接缝。
> 现状锚点全部经代码实测(exotic-protocol frame.rs/message.rs、worker.rs、
> supervisor.rs、coordinator.rs,行号见 §1)。

## 1. 现状锚点(实测)

- 握手 = Hello(host 先发)→ Ready,超时是 `WorkerConfig.handshake_timeout` 字段
  (worker.rs:54),值 5s 在 coordinator.rs:40 注入——**握手本身不涉模型加载**,
  psd-worker 起进程即回 Ready;
- 请求执行 `WorkerConn::run_thumbnail(req, limits, timeout, cancelled)` **已有
  per-call timeout 参数**(supervisor.rs:124-147),内部 100ms 取消轮询;
- Supervisor 串行:一实例一在途请求,request_id 严格匹配;
- `FrameType` 六值(Hello/Ready/Request/Success/Failure/Shutdown),`RequestBody`
  按 op 分派(现 Thumbnail/Metadata),`SuccessBody.metadata: Option<Value>` 预留;
- `WorkerErrorCode` 五值(UnsupportedVariant/MalformedInput/ResourceLimit/
  IoError/InternalError),`default_retryable()` 仅后二者可重试;
- `PROTOCOL_VERSION = 1`(frame.rs:27)。

## 2. 核心裁决:模型加载不进握手,而是首个「会话请求」

§8.2.3 原案把「握手超时不够 ORT 加载」当问题,给了「握手超时独立配置 120-300s」。
实测握手与加载本就无关——真正要设计的是**把慢加载塞进哪个协议阶段**。定稿:

- **进程握手维持 Hello/Ready + 5s 不动**(它只证明「二进制活着、协议兼容、
  capability 匹配」,快而恒定;改它反而丢失「起进程失败」的快速失败语义);
- **模型加载 = 显式 `SessionInit` 请求**(op 级,复用 Request/Success/Failure 帧,
  **不加新 FrameType**),其 timeout 独立:`SESSION_INIT_TIMEOUT = 300s`(冷加载
  ViT-L 1.7GB + DirectML 编译内核的上界;可配置,常量归 coordinator 同址);
- `SessionReady` **不是新帧**,就是 SessionInit 的 Success 响应(§8.2.3 ③ 的
  「Hello/Ready 扩展字段」方案弃用——它把会话状态耦合进进程握手,重载模型需
  重启进程,违背长驻语义)。

> 裁决记录:①握手 5s 不动、加载走请求(修正原案「握手超时 120-300s」的表述——
> 300s 归 SessionInit 请求);②SessionReady=Success(op),协议帧类型零新增。

## 3. 协议扩展(T10 施工面,归 exotic-protocol,Part6 协作)

> 🔴 **本节字段形状已被 T10 落地稿修正(2026-07-03)**:本稿撰写时转述 Part4 §3.1.2 G 行
> 草稿,未核对 Part6 §3.2.1a(第 8 轮核验的字段权威定稿,含 6 单测原型)——漏了**多角色
> 句柄 / EmbedBatch 逐项化 / ai_cache_dir 随 SessionInit** 三项。T10 施工按三源合并:
> **载运=本稿 §2**(op 级、零新帧、300s 归 SessionInit)+ **字段=Part6 §3.2.1a**(逐项化+
> 多句柄+缓存根)+ **完整性=D1**(逐模型 len/sha256 + models_root);并修正 §3.2.1a 一处
> (嵌入向量出 JSON 走 Success 帧 blob,MAX_JSON_LEN 数学,详见 Part6 §3.2.1a 的 T10 注)。
> **实现即权威**:`crates/exotic-protocol/src/message.rs`。落地形状概要:

```rust
// RequestBody 扩 op(serde tag 既有模式;概要,以 message.rs 为准):
SessionInit {
    session_id: u64,
    models: Vec<ModelDescriptor>,        // {role: ImageEncoder|TextEncoder|FaceDetect|FaceRecog,
                                         //  handle: Path|Named, len, sha256}(§3.2.1a × D1)
    model_profile: ModelProfileSnapshot, // {arch_id, image_file, text_file, batch_size,
                                         //  face_profile_id?}(§8.6;face 档为 T10 补充)
    models_root: String,                 // Path 归属校验根(D1 §3)
    ai_cache_dir: String,                // 受限缓存根(§3.2.1a ②)
    image_provider: String,              // "directml"|"cpu";文本塔 worker 内硬编码 CPU(§8.6)
},
SessionClose { session_id: u64 },        // 显式卸载(host 主导生命周期)
EmbedBatch { items: Vec<EmbedItem> },    // 逐项化:{item_id, cache_key, fingerprint}(§3.2.1a ③)
FaceDetectEmbed {                        // 同构逐项化:{item_id, cache_key?, source_path?, fingerprint}
    items: Vec<FaceItem>,
    det_score_thresh: f32,               // 检测阈值快照(行为参数,进指纹)
},
// 响应:SessionReady = Success + SuccessBody.session{embed_dim, face_embed_dim?, caps};
// 批量结果 = SuccessBody.embed / .face 逐项 Ok/Err(不连坐),嵌入本体走同帧 blob(f32 LE,
// 按 Ok 项序连续;host 校验 blob.len() == ok_count × embed_dim × 4)。
```

- `Capability` 目录扩 `"embedding"` / `"face_detect_embed"`(Ready.capabilities
  声明,握手校验即覆盖);
- `WorkerErrorCode` 增四值(G6,对齐 Part6 §3.2.2 canonical):
  `GpuUnavailable`(retryable——让步/换 CPU EP 由 host 决策)、
  `SessionExpired`(retryable——重发 SessionInit)、
  `ModelLoadFailed`(terminal——校验/文件问题,重试无意义)、
  `EmbedDimMismatch`(terminal——模型与 profile 不符,数据完整性红线);
- **`PROTOCOL_VERSION` 1 → 2**:枚举扩展对旧 worker 是未知 op → 与其运行期
  失败,不如版本门在握手即拒。psd-worker 同波重编译(C 节行 69 已预告此强制)。

## 4. Supervisor / 池改造（T13/T15 施工面）

> ✅ T15 半场已落地(2026-07-03,`6d1ab57`+`403b10f`):①(run_request 泛化+按 op 分派校验)、②(session 字段)、③(串行不动)、⑤(kill 清零会话)全量;④ 空闲卸载=worker 侧兜底自杀 timer(300s)已落 ai-worker,host 侧空闲卸载已随 T17 落地(2026-07-03 `3867a47`):收敛为**运行结束即 close_session**,不设独立 idle timer(§4④ 正文已按此回写)。

1. **`run_thumbnail` 泛化为 `run_request(op 无关)`**:现签名已含 timeout 与
   cancelled,改动 = 请求构造与输出校验按 op 分派(thumbnail 的 WebP 复核逻辑
   保留为 op=thumbnail 专属;embed 批的输出校验 = 维度×数量一致性);
2. **会话状态字段**:Supervisor 增 `session: Option<SessionDescriptor>`(记录
   已加载的 model_profile 快照)。派批前 host 检查:目标模型 ≠ 当前 session →
   先 SessionClose + SessionInit(切换语义,配 300s timeout);
3. **串行模型不动**:长驻 ≠ 并发——一 worker 一在途请求的不变量保留(批内并行
   由 ort intra-op 线程承担);吞吐扩展 = 池宽(现 MAX_POOL=2 对 AI 收敛为 1,
   GPU 令牌额度 1 使多池无益,见 D2);
4. **空闲卸载**:🔴 本项已被 T17 落地裁决简化(2026-07-03,原「host 侧 idle timer
   阈值 ~60s」方案不再实施):AI 管线的**运行结束(自然完成/取消)本就是唯一空闲
   边界**——进程内路径同样在运行结束时置空引擎,批间不存在「空闲但会话保温」的
   需求 → host 侧 = 运行结束即发 SessionClose(worker_pipeline 收尾),搜索触发的
   会话由后续运行的切换语义或 worker 兜底回收;worker 侧兜底自杀 timer(host 失联
   时不留 VRAM 僵尸)= 收到最后一帧后 300s 无活动 `exit(0)`(T15 已落);
5. **崩溃恢复**:沿用 kill_and_reap + pipeline CRASH_BACKOFF(500ms)重建;重建后
   首个动作必然是 SessionInit(session 字段随实例清零,天然正确);GPU 令牌经
   RAII 已释放(D2),无死锁面。

## 5. 与任务表映射

| 任务 | 消费本稿 |
|---|---|
| T10 | §3 协议扩展全量(与 D1 的 ModelHandle 合并施工)+ PROTOCOL_VERSION=2 |
| T13 | Coordinator 通用化时注入 per-op timeout 表(thumbnail 30s / session_init 300s / embed_batch 120s 起步值,常量集中一处)〔✅ 2026-07-03:`coordinator::op_timeouts` 四档 + 表内不变量测试(握手≪thumbnail<批≤SessionInit);pipeline::TASK_TIMEOUT 收拢引用〕 |
| T15 | Supervisor 泛化 + session 字段 + 空闲卸载〔✅ 2026-07-03:run_request(kill 语义同 run_thumbnail)+ SessionDescriptor 快照(kill 清零,§4⑤)+ init_session(Success 必须带 session 应答体,否则协议违例 kill)+ close_session(幂等);§4① 的 embed 批输出校验落 validate_embed/face_batch_output;worker 侧自杀 timer 已落 ai-worker,host 侧空闲卸载已随 T17 收敛为「运行结束即 close_session」(见 §4④ 回写)〕 |
| T17 | 派发消费〔✅ 2026-07-03:AiWorkerClient 消费 §4②(SessionDescriptor 比对→close→init 切换)+ §5 timeout 表(SESSION_INIT/EMBED_BATCH/新增 SESSION_CLOSE·ENCODE_TEXT 两档);§4④ 按「运行结束即卸」回写;硬止损=同批至多 2 次尝试(mock EmbedWorker 7 单测)〕 |
| e2e | 实测验证〔✅ 2026-07-03 `54ee6fe` `worker_e2e` harness:spawn+握手+sha256(~0.4GB)+SessionInit+首查合计 1.8s——「模型加载不在握手、握手恒快」(§2)实测成立;close→重 init 1.4s(sha 备忘命中不重算);双后端(进程内 vs worker)图像 8/8+文本 3/3 余弦全 1.000000〕 |
| face | 派发接线〔✅ 2026-07-03 `9ecee43`:AiWorkerClient::face_detect_embed 消费 FACE_DETECT_EMBED 档(去 allow);§4② 切换语义经 face 实测(CLIP-only→合并会话 close+init);**matches 超集放宽**:spec 无 face 可复用带 face 会话(face 边际 VRAM ~96MB,T9.5 实测),harness 实测合并会话服务 CLIP-only 查询 0.11s 无重 init;Phase E 4 脸 bboxΔ=0/cos=1.000000〕 |
| T_t(补测) | 握手仍 5s 快失败;SessionInit 超时→kill_and_reap→重建→重 Init 链路单测(mock ChildHandle 缝已在) |

## 6. 验收

- 冷启动:进程握手 <5s 完成,SessionInit 独立计时,ViT-B/16 加载在秒级、ViT-L
  分钟级均不触发误杀;
- 切换模型:SessionClose→SessionInit 链路 VRAM 前后差 ≈ 模型体量(T9.5 原型的
  测量方法复用);
- 崩溃注入(kill worker 于批中途):host 500ms 退避重建、重 Init、批重派,
  上层任务无丢失(status 机保证);
- 旧版 psd-worker(PROTOCOL_VERSION=1)被握手版本门拒绝,错误信息可诊断。
