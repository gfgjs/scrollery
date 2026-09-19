---
id: 2026-07-10-fable5-AI人脸深审-README
status: snapshot
type: index
line: AI 人脸流水线深审
created: 2026-07-10
---

# AI 分析 + 人脸分析流水线深度审查报告(总览)

> **快照日期:2026-07-10**。本报告结论以当日 dev 分支(HEAD `68e23e8`)为准;其结论被后续工作推翻时,须回头在本节补「已过时/被推翻项」标注。
> **性质:只读审查**——未改动任何代码;所有修法均为建议,待裁决后另行立项实施。
>
> ## ⚠️ 已过时/已交付项(2026-07-10 当日回补)
>
> - **U1 🔴(模态样式)**:已由并行会话 `e8820a4` 修复(index.css 全局 Modal 基座),先于本报告修复批;⏸GUI 真机终证仍待办。
> - **A13(fetch_tree 无超时)/ 04 册 P20**:已由并行会话 `a064c34` 修复(secure_client SmallFile)。
> - **§3 批 1 + 批 2 修复批已全量交付**(2026-07-10 同日施工,12 提交 `f68a3ae..4c51163`):
>   批 1 = A1/A2(f68a3ae)· W1/W2/W3(57bbd34)· K1(6e23ddb)· A7/F1(922028e)· U2/U3/U7(81537ae);
>   批 2 = X1 条件写(5a889f9)· F10/X2(8c4ac80,与前笔成对编译)· F2/F3+V16 判别位(3df7d75)· F6/F7(45d9ba2)· F9/A11(fea8f78)· G1(40196d0)· U4(4c51163)。
>   验证均为 cargo fmt/clippy/test + vue-tsc/eslint/vitest 全绿(**仅本地**);GUI 观感项 ⏸ 真机。
> - **X2 修复已于 2026-07-11 撤销**(`51dc09f`,外部专家报告指出并核实):start 路径 sync 以「有无 faces 行」为覆盖真相,而无脸成功图没有 faces 行 → 每次「继续分析」被误归 Pending 全量重扫。X2 发现重新开放,其自愈动机(A3/F11 竞态)改由「图×人脸模型」覆盖记录承接(todo P 节加固池 a);切轨路径(set_active_face_model)的 sync 语义正确、不受影响。
> - **文档回写批已交付**:Part4 §3.6 D1 横幅(含 §3.1.2 G4 行)、§3.5.3 D2 改写、D3 两处口径更新,与本标注同 commit。
> - **仍待裁决/未立项**:批 3 性能(按 04 册裁决待真机大库实测取数)、批 4 功能建设(05 册 §2,含人脸框校对入口等)、W3 之外的 🟡/🔵 未列入批次者。
> **验证环境声明**:纯静态精读 + 全仓 grep 交叉核验,未运行任何测试/构建/GUI(只读约束)。每条发现标注验证状态:【亲证】=主审逐行核实;【构造证实】=据亲读的相关代码结构推定成立;【子代理报告】=分域深审产出、主审抽查合理;【存疑】=需真机或实测裁决。

## 0. 范围与方法

**审查域**(约 1.4 万行):

| 层 | 文件 |
|---|---|
| host 控制面 | `src-tauri/src/ai/*`(pipeline / worker_pipeline / worker_client / face_pipeline / face_cluster / search / vector_store / remote_registry 等 14 文件) |
| IPC 命令面 | `ipc/ai_commands.rs`、`ipc/face_commands.rs`、`ipc/proofread_commands.rs`、`ipc/blocking.rs` |
| 推理核 | `crates/scrollery-ai-core`(clip / face / engine / profile / face_profile / embedding / provider 等 11 文件) |
| sidecar | `crates/exotic-workers/ai-worker`(main / session / batch)+ `exotic/{supervisor,worker,limiter,coordinator}` 相关面 |
| DB | schema V2/V8/V10 相关 DDL、queries.rs 全部 ai_/faces/persons 函数、models.rs 状态机枚举 |
| 前端 | aiStore / faceStore / personStore / useAnalysisController / PersonsView / SemanticSearchPanel / ModelLibrary / FaceModelLibrary / FaceAvatar / FaceApprovalPanel / 状态栏与工具栏相关面 |
| 设计对照 | `plan-docs/refactor_2026/Part4_AI与人脸插件化.md` + todo.md Part4 台账 |

**方法**:主审亲读 host 侧全部核心文件(~4k 行)建立独立基线 → 6 路分域只读深审并行(worker 链 / ai-core / CLIP 主线 / 人脸主线 / 前端 / 设计对照+状态机横切)→ 主审对全部 🔴/🟠 及关键 🟡 逐条交叉验证(含裁决两处子代理结论冲突,详见 K1、E1)。

## 1. 执行摘要

流水线整体工程质量**高**:worker 化架构(host 零 ONNX、协议帧字节级校验、硬止损重试、原子落盘、模型轨隔离、用户劳动保护契约)是认真设计并有测试锁行为的;红线合规(rusqlite 全走 blocking + tripwire 门禁、SQL 全参数绑定、派生产物 tmp→rename)基本全绿。本次深审的价值集中在四类:

1. **恢复路径与并发窗口**(最大伤害面):worker 空闲自杀后的「间歇性一次失败」(W1)、retryable 语义整体丢弃导致瞬态故障放大为整轮终止(W2)、GPU 分析槽在三个 CLIP 命令上泄漏导致人脸分析被永久拒绝(A1)、在途批与 SourceChanged 失效的陈旧落库窗口(X1,缩略图线 P1-4 同款病灶未复制条件写)、增量聚类与人工命令并发可静默丢弃整批聚类结果(F2)。
2. **静默算错防线的一个盲区**:错配模型文件时 ai-core 图像批输出按 profile 维度错切,长度恰好骗过 worker 维度红线与 host 全部校验,垃圾向量批量入库且搜索侧静默跳过(K1)——全链唯一能拦住它的位置在 ai-core 的输出形状断言。
3. **人脸功能面的完整性缺口**:误检桶(is_ignored)整条特性无写入口不可达、查看器人脸框纯展示无校对入口、Error 项只有销毁性全量重跑一档、审批面板模态样式实际不存在(U1,全场唯一 🔴)。
4. **规模化前的性能债**(百万级叙事对照):pending 取件 O(N²/批) 排序、list_likely_matches 全表扫描、嵌入缓存装载 3× 峰值内存、每 2s 轮询多条全表 COUNT、recluster O(F²) 无护栏——当前库量级(千级向量)全部无感,达到十万级前应按 [04-性能优化路线图](04-性能优化路线图.md) 分批清偿。

**文档侧**:Part4 主线交付与台账高度吻合;唯一违反「正文回写红线」的是 §3.6 GPU 协调段(正文仍规定被 D2 裁决否决的「会话级令牌+协议帧」方案),次为 §3.5.3 sha256 校验段(详见 [05-功能缺口与设计对照](05-功能缺口与设计对照.md))。

## 2. 发现总表

严重度:🔴 功能破损/必修 · 🟠 高(用户可见伤害或数据污染) · 🟡 中 · 🔵 低 · 💡 优化建议。详情见各分册(编号即锚点)。

### 🔴

| ID | 位置 | 一句话 | 验证 |
|---|---|---|---|
| U1 | FaceApprovalPanel.vue:5,286 / OnboardingWizard.vue:5 | 模态样式 `.dialog-overlay/.dialog-content` 仅存在于其他 5 组件的 scoped style,跨组件不生效——审批面板将以非模态流内 div 渲染 | 亲证(静态);真机终证待办 |

### 🟠

| ID | 域 | 位置 | 一句话 | 验证 |
|---|---|---|---|---|
| W1 | worker 链 | worker_client.rs:210 + supervisor.rs:87,142 | worker 空闲 300s 自杀后 host `alive` 旗标陈旧,SessionInit 进程级失败经 `?` 越过重试圈 → 隔 5 分钟再操作必失败一次(间歇性、难排查) | 亲证 |
| W2 | worker 链 | worker_client.rs:230-236 | 除 SessionExpired 外一切 Failure 按 terminal 处理,`retryable` 位整体丢弃 → 一次瞬态推理失败=整轮终止+清续传标志 | 亲证 |
| A1 | CLIP 命令 | ai_commands.rs:554,573,783 | set_active_model / reload_ai_engine / rebuild_embeddings 取消流水线但不释放 GPU 分析槽 → 人脸分析被永久拒绝(face 侧对称命令有释放,不对称) | 亲证 |
| A2 | CLIP 命令 | ai_commands.rs:242-262 vs pipeline.rs:213-251 | get_ai_status 用另一套 VRAM 阶梯算默认 batch 并**写库**,自动档(≥8GB→128/≥12GB→256)被状态轮询永久钉死在 64 | 亲证 |
| K1 | 推理核 | ai-core clip.rs:426-437 + ai-worker main.rs:224 | 错配模型文件时图像批输出按 profile 维度错切,长度恰好骗过全链校验 → 垃圾向量批量入库、搜索静默跳过、零告警;SessionReady 维度来自注册表故 host 侧比对无效,唯一修复点=ai-core 输出形状断言 + init 自检;主要入口=import_ai_model 零校验 | 亲证(两处代码逐行核实) |
| X1 | 状态机横切 | queries.rs:2965/3231(裸 UPDATE)+ queries.rs:646-670 | 在途批 vs SourceChanged 失效竞写:迟到 writer 无条件回写 status=2 + 旧 cache 向量永久留存,无自愈——缩略图线 update_thumb_result「条件写」防冲模式未复制到 AI/face | 构造证实 |
| F2 | 人脸聚类 | face_cluster.rs:208-283 + queries.rs:4233-4267 | 增量聚类「快照读→内存决策→写回」与 merge/reassign/unassign 无互斥:merge 删簇 → apply_face_clusters FK 违约 → **整批(≤512 项)聚类事务回滚静默丢失**;并有 lost-update 覆盖精确重算(recluster_faces 的守卫也是非原子 TOCTOU) | 前提亲证(FK=ON connection.rs:30) |
| F3 | 人脸聚类 | face_pipeline.rs:379-435 + face_cluster.rs:226 | 「孤儿未聚类脸」无自愈:硬崩溃丢 cluster_pending 或 F2 丢批后,face_status=Done 而 person_id=NULL 的脸**永不再被增量聚类**(get_clusterable_faces 只查本轮 item_ids),唯一救济=显式全量重聚类 | 亲证 |
| F1 | 人脸 DAO | queries.rs:3508-3596 | merge_persons 缺同模型守卫(reassign/create 都有)→ 跨模型合并产出混维垃圾质心 | 子代理报告(守卫不对称已核) |
| F7 | 人脸级联 | file_ops_commands.rs:143-153 + schema.rs:424 | 硬删媒体 CASCADE 删脸后 persons 派生字段无对账 → 计数虚高/封面悬挂/幽灵人物 | 子代理报告(CASCADE 亲证) |
| G1 | 功能缺口 | 全仓 grep | `persons.is_ignored` 误检桶:schema/保护逻辑/过滤全建好,但**无任何写入口**(IPC+前端均无)——整条特性不可达 | 亲证 |
| U2 | 前端 | aiStore.ts:54 | AI start/restart 的后端拒绝(GPU 槽被占/模型未装)只进 console,用户点了没反应无解释(face 侧同场景走 toast,不对称) | 子代理报告(onError 亲读印证) |
| U3 | 前端 | SemanticSearchPanel.vue:68-75 | rebuildEmbeddings 一键清空全库向量+打断分析,无确认对话框(侧栏同语义操作有 confirm) | 子代理报告 |
| U4+A17 | 前后端 | aiStore.ts:100-142 + search.rs:177-190 | 语义搜索无 in-flight 令牌 + 后端 ai_search_results 全局单表 last-writer-wins → 乱序应答污染结果/计数/加载态 | 子代理报告(两层互证) |
| U5 | 前端 | PersonsView.vue:164-184 / FaceAvatar.vue:32-53 | 人物封面用 CSS background 渲染:404 无 onerror、无 LRU 驱逐孤儿懒自愈(画廊线已修的已知病灶在人物墙重现)、无生成请求通路 | 子代理报告(与病灶记忆互证) |
| F16 | 性能 | queries.rs:3938-4039 | list_likely_matches 全表扫描+每行复制完整质心 BLOB+limit 事后截断——大库刚分析完(全部未确认)时数百 MB 级搬运 | 子代理报告 |
| F17 | 性能 | face_cluster.rs:518-594 | recluster_all 全量嵌入常驻内存(百万脸≈1GB)+最坏 O(F²),无任何规模护栏/预检 | 亲读印证 |

### 🟡(摘要,详见分册)

| ID | 一句话 |
|---|---|
| W3 | SessionSpec::matches 不比对 batch_size,worker 按会话快照硬拒超限批(terminal)→ 调大配置后 300s 内开始分析=整轮打死 【亲证】 |
| W4 | AppState.ai_worker 粗粒度 Mutex 跨整批持有(最长分钟级)→ 状态轮询/语义搜索排队;快照读可拆 RwLock |
| W5 | 每次 SessionInit worker 端全量重算 GB 级模型 sha256(host 有 mtime+len 备忘,worker 没有) |
| W6+E1 | 错误契约「形式合规、语义退化」:AppError 稳定 code 但 worker 子码被压平进 System 文本,前端只能串匹配 |
| A3/A5 | cancel 不 join 旧 run:与 sync/reset/孤儿恢复交错 → 少量误标/漏件(CLIP 有 start-sync 自愈) |
| X2 | face 侧 start 缺 sync_face_status 自愈(CLIP 每次 start 都 sync),误标 Done 的项在人脸轨无自愈路径 【亲证】 |
| F10 | 流水线完成回调无条件 cancel_face_analysis 可误杀刚重启的新一轮(token 无 compare-and-clear;CLIP 同构) |
| A4 | Producer 标记 Processing 失败仅 warn 继续 → 重复领取,持续写失败时空转(face 同构) |
| A7 | semantic_search_with_vector 不校验查询向量长度,K1 场景下 rayon 线程越界 panic 【亲读印证】 |
| A11+F9 | CLIP Error 项永远计入 pending(进度到不了 100%)且无 error 计数;face Error 项只有销毁性 restart 一档,无非破坏重试 |
| A14 | EMBED_BATCH 超时固定 120s 不随批缩放(face 已修同病),CPU EP 大批必误杀 → 整轮 fatal |
| F5 | 手动 merge 的成果可被下次 recluster 撤销(不置 is_confirmed + src 簇 rejections 随 CASCADE 蒸发) |
| F6 | recluster 后未触及的 named/ignored 人物 face_count/cover_face_id 陈旧(人物墙显示 N 张实际 0)【双重亲证】 |
| F11/F12/F13 | 切轨 sync 与在途 writer 小窗口 / Error 项残脸被 sync 翻 Done / 「脸+状态原子落库」注释与实现(两事务)不符 |
| F15 | proofread set/clear key 的 keyring 系统调用直跑 async 正文(同文件 get 侧已下沉,标准不一) |
| K2/K3/K5/K6/K7 | CLIP 模型缺失误降级人脸到 CPU / CPU 线程超订 / 动态 batch 冗余整批拷贝 / vocab 非 UTF-8 回退 CWD / provider 探测死分支+override 只认 cpu(联动 P1-14) |
| U6~U12 | 语义模式丢分组偏好 / IpcError 前缀文案 / personStore 写失败静默 / 状态栏无人脸进度 / 下载进度组件本地态 / 轮询乱序闪回 / 人物墙无懒加载 |
| X3 | 三处 Person 视图成员变化未 bump(reset_face_data / set_active_face_model / 流水线增量聚类) |
| P1~P5 | 性能族:pending 取件 O(N²/批) 排序(A8/F19)/ 缓存装载 3× 峰值(A9)/ 边分析边搜反复全量重载(A10)/ 每 2s 轮询多条全表 COUNT / F18 每 flush 重读全量 persons |

🔵/💡 级(约 20 条)详见各分册。

## 3. 修复优先级建议(供裁决)

**批 1|小改高确定性正确性修复**(均为几行到几十行,互不依赖):
A1(三处补 release)· A2(状态命令去写副作用+统一阶梯)· W1(ensure_session 进程级失败纳入重试圈)· W2(消费 retryable 位)· K1(ai-core 输出形状断言 + SessionInit 元数据自检)· A7(查询向量长度守卫)· F1(merge 同模型守卫)· U2(start/restart 失败 toast)· U3(rebuild 加 confirm)· U7(ipcErrorMessage 统一)· U1(模态样式提升全局层,顺带消 5 份重复)。

**批 2|需小设计裁决的正确性修复**:
X1(flush 前比对 cache_key 的条件写,对齐 update_thumb_result 模式)· X2(face start 补 sync)· F2+F3+F10 并发族(token compare-and-clear + apply_face_clusters 防 FK 炸批 + 「未聚类脸对账」幂等扫描;或裁决「分析运行中禁用人工归属命令」)· F7(硬删路径 person 对账)· F6(rebuild 尾部对账 UPDATE)· F9(retry_failed_face_items 非破坏重试命令,CLIP 侧同补 error 计数)· G1(is_ignored 写入口)· W3(SessionDescriptor 记 batch_size)· U4(搜索代次令牌)。

**批 3|性能(建议真机大库实测后按数据排期)**:见 [04-性能优化路线图](04-性能优化路线图.md)。

**批 4|功能建设(按用户价值排序)**:见 [05-功能缺口与设计对照](05-功能缺口与设计对照.md) §2。

**文档回写(独立小批,零代码)**:Part4 §3.6 横幅回写(D1)+ §3.5.3(D2)+ 两处口径更新(D3)。

## 4. 分册导航

| 文件 | 内容 |
|---|---|
| [01-CLIP主线与worker链.md](01-CLIP主线与worker链.md) | W1-W13、A1-A20 详解 + worker 链红线核对 + 架构数据流 |
| [02-人脸主线与推理核.md](02-人脸主线与推理核.md) | F1-F21、K1-K9 详解 + 聚类算法评注 + 模型/预处理流水线表 |
| [03-前端.md](03-前端.md) | U1-U21 详解 + 前端数据流 + 契约核验通过项 |
| [04-性能优化路线图.md](04-性能优化路线图.md) | 全部性能发现合并 + 分梯队路线图(含触发条件) |
| [05-功能缺口与设计对照.md](05-功能缺口与设计对照.md) | 功能缺口(去重合并、按用户价值排序)+ Part4 逐 T 对照表 + 有意 defer 清单 + 文档回写债 |

## 5. 值得肯定的设计(全域摘选)

- **「不信任 worker」贯彻到字节级**:同序同长校验、item_id+fingerprint 回声、blob 长度精确对账、帧层先校验后分配(65MiB 谎报不触发分配)、路径白名单双向收口。
- **模型轨隔离**(faces/persons 双表 model_name 对称)与**用户劳动保护契约**(pinned/anchored/free 三类语义 + rebuild 绝不动 name/is_named/is_hidden/is_ignored + plan_recluster 纯函数 9 测)。
- **增量路径对 rejections 传空集是可证明正确的**(新脸 id 不可能有拒绝记录 + CASCADE),不是侥幸。
- **踩坑记录即制度**:ai-core 头部 11 坑逐条带症状/根因/检查方法;「fp16 必须对拍余弦≥0.99」「文本塔强制 CPU 换正确性」等纪律直接沉淀在源码。
- **blocking.rs tripwire 测试**把 CLAUDE.md rusqlite 硬化条款变成可执行门禁;下载工程(续传/镜像/原子就位/路径纵深防御)质量高。
- **前端 useAnalysisController 参数化去重**、语义结果 DB 物化复用布局管线(阈值调节零重搜)、审批乐观更新。
