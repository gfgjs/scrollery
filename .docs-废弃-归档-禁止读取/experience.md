---
id: 2026-07-10-experience
status: active
type: experience
line: 全局
created: 2026-07-10
---

# Scrollery - 性能与稳定性踩坑记录

本文档记录本项目在性能与稳定性攻坚中遇到的典型问题、排查思路及最终解决方案，按主题分簇。
当前六簇：**AI 分析流水线**（§1–3，`SessionPool` / `ONNX Runtime` 调优）、**前端渲染**（§4–5、14，画廊虚拟滚动 / 派生缓存自愈 / 顶栏折叠展再收）、**环境与工具链**（§6–8，cargo config / DLL 解析 / 构建与格式化工具坑）、**架构与验证方法论**（§9–11、15，迁移对账 / 对拍盲区 / 缓存契约 / 编译期默认行为核验）、**文档治理与门禁**（§12，工具代码路径自扫描）与 **硬件与运行环境**（§13，主机硬件不稳定在软件层的投影）。

---

# 一、AI 分析流水线

## 1. 纯 CPU 模式下的性能雪崩（单核退化）

**现象描述：**
用户反馈在使用 CPU 进行全量 AI 分析时，耗时相比旧版本成倍增加，CPU 使用率极低（长期徘徊在一个核心的负载）。

**踩坑原因：**
在为了解决“多 Session 并发请求导致的 N*N 线程资源抢占（Thread Storm）”问题时，我们在 `engine.rs` 中对 `AiProvider::Cpu` 采取了一刀切的策略：
```rust
// 错误的做法：强行限制单 Session 为 1 线程
let mut b = Session::builder()?
    .with_intra_threads(1)?
```
开发者的初衷是外层流水线可能会并行发起多个推理请求，如果不限制单实例的线程数，会撑爆 CPU。但实际上，流水线 (`src-tauri/src/ai/pipeline.rs`) 中负责提取批次并向 GPU/CPU 请求推理的消费者核心（`run_inference_tasks`）**仅有单线程在运行**。
这就意味着，这唯一的消费者在调用唯一的 CPU Session 进行 `encode_image_batch` 运算时，只能利用到 `1` 个物理线程，彻底退化成了单核运算。

**解决方案：**
1. **恢复内部多线程**：为 CPU Provider 恢复 `with_intra_threads(cores)` 配置，使其能够吃满所有核心以加速批次推理。
2. **缩小 SessionPool 上限**：将 `AiProvider::Cpu` 的 `pool_size` 限制为 `2`（主消费线程占 1 个，偶尔搜索时的文本编码占 1 个），从物理实例的源头上掐断 N*N 线程风暴和内存 OOM 隐患。

## 2. GPU 大批次导致显存交换，性能断崖下跌

**现象描述：**
用户在搭载 12GB 显存的独立显卡上，为了追求极致性能，将设置面板中的 **AI 批处理大小 (Batch Size)** 手动拉满至 `512`。
结果，原本 Batch Size = 256 时仅需 `8秒` 的推理批次，在 512 下耗时剧增到 `35秒`，且检测到显存使用量飙升到了 `11.2GB`。

**踩坑原因：**
现代深度学习模型（如 Chinese-CLIP ViT-Base/16）在推理时的显存占用是由“模型静态权重体积”加“批次前向传播产生的中间激活层体积”组成的。Batch Size 越大，计算 Attention 矩阵时需要的内存（Activation VRAM）就呈线性或超线性增长。
当显存占用达到 11.2GB，已经触碰到了 Windows 系统 WDDM 调度的安全红线。操作系统为了防止程序 OOM 崩溃，强行将多出来的显存压力转移到了**系统共享内存（RAM）**中，引发了显卡和系统内存之间的 PCIe 数据交换（VRAM Swapping / Paging）。这极大地拖慢了计算单元的访存延迟，造成“性能悬崖”。

**解决方案：**
1. **防爆硬限制（后端）**：无论前端如何配置，在 `pipeline.rs` 的入口处将用户输入的 Batch Size 硬性上限死锁为 `256`。
2. **智能 Auto 模式（前端+后端）**：由于不同用户的显存差异巨大，引入 `0 = 自动配置` 的机制。当用户不手动配置时，系统启动 `detect_vram_bytes()` 检测物理显存，并应用保守且能跑满 CUDA/DirectML 核心经验值（12G -> 256，8G -> 128，4G -> 64 等）。
3. **前端引导警告**：在 Vue 的设置组件中，针对超过 128 和超过 200 的危险值，分别显示黄色警告和红色高危警告，教育用户“盲目拉高 Batch Size 会起反作用”。

## 3. 图片原尺寸与显存占用的误区

**常见问题：**
“要分析的图片分辨率很大（如几十 MB 的 RAW 或 4K 大图），会不会导致 AI 分析显存爆表？”

**结论：完全不会。**
- **AI 前向传播时的状态**：所有的原图在进入推理前，都在 CPU 侧的预处理阶段（WIC 或 image-rs）被重采样和裁剪成了固定的尺寸格式（如 `224x224 RGB Float32`）。所以显存占用永远只跟 Batch Size 有关。
- **代价在别处**：处理超大图片真正承受压力的是硬盘 I/O 读取时间和 CPU 解码消耗的系统内存（RAM）。这也从侧面印证了为何我们需要多线程或更底层的 WIC 解码器来为 AI 阶段输送“口粮”，避免前端 I/O 成为后端的瓶颈。

---

# 二、前端渲染

## 4. 画廊极密网格快滚飞掠卡顿（虚拟滚动）

> 详细收口见 [2026-07-10-画廊滚动飞掠卡顿分析.md](archive/2026-07-10-画廊滚动飞掠卡顿分析.md)；此处提炼可迁移的诊断法与设计坑。

**现象描述：**
极密网格（60px 缩略图）用无极滚轮快速飞掠时，① 自研滚动条滑块跳动/卡顿（按住滑块拖则正常）；② 快滚到位后短暂白屏再绘图。

**关键诊断（可复用的排除法）：**
两症状在 **DOM 与 Canvas 两种渲染模式都复现、与网格节点数无关、大行高也复现**（只是不明显）。这条「跨渲染模式 + 与节点数无关」的**交叉复现**是把根因从「渲染层」二分到两模式**共享的 bucket 虚拟滚动引擎**（按需取数 + 惯性中渲染）的关键——排查性能问题时，先找一个能让症状消失/保留的正交开关（此处是渲染模式），比盯着某一实现猜要快得多。
- 白屏 = 取数载荷往返延迟（每段 ~600 项、每项曾带 ~28 字节 thumbhash 数组作 JSON 过桥）；
- 滑块跳 = 段落地爆发（反序列化 + 挂载/重绘）抢主线程，而滑块由 rAF 在同一主线程驱动、无法解耦。
- 两者是**同一「按需取数 + 惯性中渲染」架构的两个投影**，故先前只调段大小（`clampSegmentPx`）只改「爆发形态」不改「速率」，体感改善有限。

**解决方案与设计要点（教训）：**

1. **快滚甩滚低保真**（本次**关键生效改动** `0a39e31`）：按滚动速度置模块级加载闸门，飞掠期抑制**新**缩略图 decode/生成请求的启动，降速/停稳后放行补起；已缓存图照常绘制、闸门只抑制启动不丢加载。真机 dropped 5-11/burst → **0-1**、白屏基本消失。这是把「段落地爆发里的解码工作」从惯性帧挪到停稳帧的业界 fling 标准做法。

2. **闸门变量必须是「滚动速度」而非「是否在滚动」**（最易踩的坑）：代码里现成的 `isScrolling` 对**慢滚也为 true**，直接用它 gate 会令慢滚全程不出图、体感更差；而症状只随 velocity 出现（爆发速率 ∝ velocity）。故抽纯函数 `scrollVelocity` + `shouldDeferThumbLoad`（阈值经验 3px/ms）gate on velocity——慢滚照常出图、快滚才抑制。**「有现成的布尔信号」不等于「它是正确的判据」。**

3. **跨切面信号用模块级单例、勿 prop 穿透**：加载闸门要作用于数千张轻量卡片，若走响应式 prop 穿 4 层组件，每次翻转触发全量 re-render 本身就是一次 hitch。改用模块级单例信号（镜像本项目 `useSelection.setPointerIdResolver` 惯例），让**真正发起加载的消费者**各自 `watch`，零 prop 穿透、零卡片 re-render。

4. **动量急停要专用短去抖放行**（`da43dfa`）：无极滚轮急停时最后一帧速度高、闸门仍关，只靠长 settle（150ms）兜底放行则出图偏慢。加 ~64ms（≈4 帧）专用去抖，停顿即放行，比 settle 早 ~86ms 出图；取值须 > 飞掠帧间隔（~16ms）以免动量途中误放。

5. **过桥前先问「前端到底用了这份数据的多少」**（`8078d7c`）：核实发现画廊占位只需**一个平均色**，却每项过桥 ~28 字节 thumbhash 数组、且 canvas 每帧逐格现算均色。改由后端 hydrate（仅可视区 ~10² 项）算好紧凑 `#rrggbb` 过桥替代数组——既瘦线上载荷（白屏源）、又消灭渲染热路径逐格逐帧计算。跨语言一致性用金标锁死（后端复用与前端同源的 thumbhash crate，Rust 单测断言金标 hash → 固定 hex）。**用户曾提议「占位改固定灰」省事，但前端改灰不动后端 = 零性能收益（数组仍过桥）；既然要动后端，紧凑色的载荷收益与丢弃几乎一样大却保住彩色质感——不必拿观感换速度。**

**残留（诚实边界）：**
DOM 逐格挂载、真图 decode 是按需虚拟滚动的**固有开销**，性价比低、收口不再攻；方向性预取在白屏已消后收益不确定，亦不做。记录残留是为防后来者按「未完成」重启已裁定为低性价比的工作。

## 5. 缩略图 LRU 驱逐产出「孤儿封面」:文件已删而状态短路自愈(2026-07-09)

**现象描述:**
画廊滚动时 404 刷屏——LRU 驱逐删掉了封面文件,但缩略图 router 里该行 status=1(已生成)使重建判断**短路**,自愈链被永久旁路。

**踩坑原因:**
「派生文件存在性」与「DB 状态记录」是两份真相,删除文件的一侧(LRU 驱逐)没有同步复位另一侧;而自愈判据只查状态不查文件,分叉后死锁在「记录说有、盘上没有」。另注意:图像缩略图与文件夹封面走**两条独立生成链**,复位时须找对派生行。

**解决方案与教训(已由事件驱动复位根治,`6ff2841`):**
驱逐事件同步把对应派生行 status 2→0,重建链自然接管。可迁移通式:**任何删除派生文件的动作,必须在同一事务里复位其状态行**;自愈判据须「文件存在 ∧ 状态一致」双查,单查任一者都会分叉(与 CLAUDE.md「派生产物原子落盘」同族——存在性判定不可靠的另一面)。

## 14. 顶栏「筛选致折叠闪展再收」——flex 兄弟宽度耦合进 ResizeObserver + measure/recompute 双路径(2026-07-15)

> 收口提交:`523cd0c`(整行移窗拖拽)/`3cae44e`+`0282ff8`(同步命令式量尺消 show-all 绘制)/`5328597`(is-settling 折叠过渡抑制)/`b8e9090`(计数源头解耦 + recompute 统一不变量)。真机验收通过。

**现象:** 点筛选(图片/收藏…),顶栏折叠 chip「短暂展开再收起」。折叠逻辑本身没错——真凶是**布局耦合**。

**根因(可复用诊断法):** 顶栏是三段 flex 行 `[标题+计数 flex-shrink:0][foldable flex:1][搜索]`。筛选使计数 `543,449→2` 文字变窄 ~48px → 左段变窄 → **flex:1 的 foldable 被 flex 再分配到更多宽** → foldable 的 `ResizeObserver` 触发 `recompute()` → 放出 chip(带 0.28s 过渡)= 闪。**通式:一个 `flex:1` 元素用 RO 监听「可用宽」时,任何兄弟的内容宽变化都会经 flex 再分配喂给它的 RO,与「窗口是否 resize」无关。RO 看到的「我变宽了」有两个不可区分的来源——窗口真变大 vs 兄弟让出宽。** 排查「莫名其妙的重排/闪动」先问:这个 RO 是被谁的宽度变化喂的。

**两个正交修法(互补,非二选一):**
1. **源头解耦**——让易变兄弟不撑动 foldable:用**可证明的上界**当隐藏 sizer 撑最大宽(`visibility:hidden`),实际值 `inline-grid` 同格(`grid-area:1/1`)叠放,格宽恒 = sizer。计数的上界是天然的:视图计数 ≤ 全库计数,故用全库计数当 sizer,零猜测、随库自适应(比拍一个魔法 `min-width` 更稳)。
2. **判据补锚**——RO 分不清两来源,就用**外部锚点**补上:比对 `window.innerWidth`,变了才是真窗口 resize(走 0.28s 动画),没变=兄弟再分配(瞬时落定 `applySplitSettled`,无动画)。升格为「唯有真·窗口 resize 才动画」不变量。

**折叠瞬时落定的实现坑(`applySplitSettled`):** 要「改折叠但不触发过渡」,不能只置 `transition:none` 就改值——解除 `none` 的瞬间过渡重启、且末次绘制值仍是折叠前的 1fr → 过渡照样从 1fr 动画到 0fr。正确序:置 settling(`transition:none`)→ 改 `visibleCount` → `await nextTick()` 等折叠类落 DOM → **`getBoundingClientRect()` 强制一次同步 reflow 把「已折叠+无过渡」提交为样式基线** → 再解除 settling(grid 值不变、仅过渡属性 none→0.28s、无数值变化 → 不触发动画)。全程在 microtask 内无中途 paint。

**元教训 · 别把互补层当替代层,按症状类别建不变量:** 这条闪动有**多层**——① 响应式 `show-all` 渲染帧被真实绘制;② 折叠过渡在内容变化时动画;③ 兄弟宽驱动 recompute。曾把「同步命令式量尺」(治①)误当成「过渡抑制」(治②)的**替代**顺手删掉后者,结果只治一半。它们是**叠加的防线,不是二选一**。且筛选走 `recompute()`、分组走 `measure()`,**机制不同**——按「单个触发器」逐个打补丁(先修筛选、再修分组)必然漏;按「症状类别」建统一不变量(唯真·窗口 resize 才动画)才能一次覆盖两路径。

**诚实边界:** 同步量尺仍会绘制 mount 首帧的展开(一次性,可接受);`window.innerWidth` 判据对「侧栏开关」这类同窗宽的合法再分配也判成瞬时(可接受,甚至更跟手)。

**移窗拖拽附带教训(`523cd0c`/`3cae44e`):** 整行 `data-window-drag-region` 交给 `useWindowDrag` 委托时,区分点击/拖拽用三路径(排除件/纯空隙面即时移窗/按钮走距离 2px+按住 300ms 降阈)。两个 WebView2 坑:① `pointerdown.detail` 判双击不可靠(常停在 0/1),须用 `mousedown.detail` 或**手动时间+坐标判定**;② `startDragging()` 进入 OS move-loop 会吞掉其后的 mouseup/click/dblclick,故「首击即 startDragging」会打断浏览器双击计数——双击最大化须用独立的「上次按下时间≤500ms+位移≤4px」手动判定,不依赖 `dblclick`/`detail`。空隙面判定用 `el.matches(selector)`(只测元素自身)而非 `closest()`——fail-safe:容器内的可点 div 永不误判为拖拽面。

---

# 三、环境与工具链

## 6. 失效 ORT 路径静默劫持 tauri dev(cargo config 就近覆盖 + 死路径无限阻塞)

> 事故全链与提交对照见 [todo.md](todo.md) P 节「2026-07-11 凌晨环境事故三修」(`15943d3`/`8d704b1`/`51dc09f`);此处提炼可迁移的机制认知与诊断法。

**现象描述:**
真机上 AI 分析/人脸识别完全不可用,分两幕:第一幕 worker exe 不存在(spawn 失败,报错清晰,cargo clean 所致);补建后进入第二幕——日志显示 worker 已启动,但进度永久为 0,伴随读池 `Pool(Error)` 刷屏,**全程零 AI 日志**。worker 进程 0 CPU、6MB 内存,像死了但没退出。

**踩坑原因(三层叠加,缺一不爆):**
1. **绝对路径进了仓库配置**:`src-tauri/.cargo/config.toml`(06-03 建)以硬编码绝对路径设 `ORT_DYLIB_PATH`;07-06 改名施工把路径里的 `picasa-next` 机械替换成 `scrollery`,而本地目录从未改名——路径当场变死,但**四天无症状**(下一层在兜底)。
2. **cargo config 就近优先**:cargo 从 CWD 逐级向上合并 `.cargo/config.toml`,近者胜。`tauri dev` 从 `src-tauri` 调 cargo,吃到这份死配置;在仓库根 `cargo run` 吃到的却是根上那份好配置(`relative = true`,天然免改名雷)——**同一台机器、同一份代码,复现与否只差一个 CWD**。
3. **ort load-dynamic 对死路径不快败**:显式 `ORT_DYLIB_PATH` 指向不存在的文件时,ort 装载线程**无限阻塞**(而非报错),0 CPU 零日志;clean 前 `target/debug` 里的 DLL 存货让回退搜索还能落到好 DLL(掩蔽期),clean 后回退落到 System32 的 1.17 旧版同样无限阻塞——两条路殊途同归。叠加 worker 单段装载上限 600s×3 段 vs 宿主 SESSION_INIT 300s 的**预算倒挂**,宿主恒先杀,表现为「SessionInit 永久无应答」;派发卡死→derive 任务抓满读池→状态轮询饿死,即 `Pool(Error)` 刷屏的来源。

**诊断方法(可复用):**
- **帧级二分**:一次性探针按「握手 → SessionClose → 空 EmbedBatch → SessionInit」逐帧推进,把「host 写侧 / worker 读侧 / 装载阶段」一次切开;配合把帧 dump 成文件直喂 worker stdin,可脱离 host 复现。本次以此证伪「帧蒸发」假说(帧确已到达 worker,卡在装载内部)。
- **先补观测再猜**:ai-core 本就有完整阶段日志,只因 worker main 没装 tracing 订阅者被全部丢弃——「零日志」本身就是线索;装上订阅者+启动指纹(exe 路径/ORT_DYLIB_PATH)后断点直读。
- **环境类问题必须在事发 CWD 复现**:在仓库根复现不了 src-tauri CWD 的问题,正是 config 分岔;排查 env 须从**调用时 CWD** 向上枚举全部 `.cargo/config.toml`,「别的目录复现不了」不等于「不存在」。

**解决方案与防复发:**
1. 删除 `src-tauri/.cargo/config.toml`,ORT 指路收敛到仓库根单一事实源(`relative = true`,改名免疫);
2. ai-worker 装 tracing 订阅者 + 启动指纹 + 收帧回执(`8d704b1`),同类故障 stderr 环形缓冲直读断点;
3. 纪律:**绝对路径不进仓库配置**(改名机械替换只对绝对路径有杀伤力)、**不再新建嵌套 cargo config**;结构性加固(2026-07-11 同日全量落地,todo P 节加固批 A+B):worker 启动自检 ORT 路径存活即报即退(`8533cc5`)、装载阶段心跳/阶段回执把宿主 watchdog 从「猜总时长」改为「静默限时」(协议 v3,同提交)、CI 路径卫生门禁拦绝对路径与嵌套 config(`c55e7b2`)。

4. **同款延迟引爆第二例(2026-07-11,psd bad_signature)**:debug 主程序的 exotic 信任根同样毁于 cargo clean——build.rs 的编译期 keyset 注入依赖构建时 env,dev 构建从未注入(恒 fail-closed 占位集),缺口被彩排期运行时旁路掩蔽,clean 后已装内测插件全数 `bad_signature`。修法同思路:构建脚本在 debug 下自取本机内测 keyset(`9e5e7d9`),把「记得设 env」变成「机器自己知道」。**教训通式:凡「正确性依赖构建/运行环境里的一份配置而非仓库本身」的机制,都要么收进仓库(相对路径/自取规则),要么让代码自解析并快败——否则 cargo clean、换机、改名,每一个都是延迟引爆点。**

## 7. cargo `--bin` 静默过滤多 `-p` 目标 + clean 后补建清单

**坑:**`cargo build -p A -p B --bin X` 只构建 X——`--bin` 过滤器作用于**全部** `-p` 的 target 集合,其余包的默认二进制被静默跳过,且退出码为 0,极易把「命令成功」误读成「全部建完」。

**连锁:**cargo clean 后若按此误判漏建,须手动补建 worker 三 exe **和 ORT DLL 四件套**;缺 DLL 时 ort 回退解析落到 System32 的 1.17 旧版,无限阻塞假死、零日志(机制同 §6 第 3 层)。

**纪律:**clean 后按产物清单逐一核对存在性,勿信单条 build 命令的退出码;需要多个 bin 时逐个显式 `--bin` 或不带过滤器整包构建。

## 8. `npm run format`(独立 prettier --write)重排 Vue 内联处理器炸 build,四门全程假绿

**事故:**整仓直跑 `npm run format` 把 Vue 模板里的内联事件处理器换行化,build 直接炸;而 typecheck / lint / test 全程绿(模板表达式的这类破坏不在三者检查面内),另伴随 ~48 个文件的纯格式噪音淹没真实 diff。

**处置与纪律:**误跑用 git 回滚(当次以 stash 恢复),不做手工修补;格式化修复一律 `npm run lint:fix`——Prettier 规则已经由 @vue/eslint-config-prettier 走 ESLint,独立 prettier 与 ESLint 面不等价。红线已入 CLAUDE.md,本条存事故细节。

## 16. 判断"能否安全改名/搬迁项目目录"要查内部自证点,不要搜文件夹名字符串(2026-07-15)

**场景:**本地开发目录从 `D:\photoapp\picasa-next` 搬迁改名为 `D:\workspace\scrollery`(补齐 2026-07-06 品牌改名当时特意保留的目录名尾巴)。

**方法论:**git / Cargo / npm / Tauri 的"我是谁"均靠**内部证据自证**,从不依赖 `basename(容器目录)`——git 靠 `.git` 内部 refs + remote URL(与本地路径无关的 SSH/HTTPS 字符串);Cargo workspace 的 `members` 全是相对路径,`name` 字段来自 `Cargo.toml` 而非父目录;npm 同理靠 `package.json` 的 `name`;Tauri 应用身份靠 `tauri.conf.json` 的 `identifier`/`productName`。判断"能否安全改名/搬迁"时应逐一核对这些自证点是否已与目录名脱钩,而非搜索文件夹名字符串本身在代码里出现了几次——后者会把测试假数据、UI 占位符文本、历史文档里的旧路径引用也算进风险面,制造虚假的复杂度。

**连带教训(工具输出需复核):**调研中一次并行 grep 曾把 `scripts/generate-notice.mjs` 的一行内容错标成另一文件的行号,但目标文件总共没那么多行——行号矛盾暴露标注错位。任何可疑的工具输出都要重读源文件核实,不能因为"grep 说是"就采信。

## 17. Claude Code 项目数据按工作目录绝对路径键控,改名/搬迁前须手动迁移(2026-07-15)

**现象:**改名/搬迁项目目录后,新会话会进入全新的空 project key 目录,不会自动继承旧路径下的会话历史(`<uuid>.jsonl`)与蒸馏记忆(`memory/`)。

**编码规则(5 个同机项目目录交叉验证确认):**drive 字母转小写,路径中的 `:` 与每个 `\` 各自替换成一个 `-`。例:`d:\photoapp\picasa-next` → `d--photoapp-picasa-next`;`d:\workspace\scrollery` → `d--workspace-scrollery`。目的地一变,编码键跟着变——这类推导必须锚定最终拍板的确切路径,不能用中间假设去定案。

**"聊天记忆"不止 `memory/`:**项目数据目录下的 `<uuid>.jsonl` 会话记录文件才是完整对话历史的主体(本例 77 个文件、729MB),`memory/` 只是其中人工蒸馏出的索引子目录,体积占比很小。

**操作纪律:**必须在**开新会话之前**,手动把整个旧 project 目录(`Copy-Item -Recurse`,用复制不用移动)拷到新编码名下才能续上;晚了(新会话已建空目录后才想起)则要改成逐文件合并,成本高得多。搬完先在新会话验证一遍(记忆索引能否被正常读取、历史会话列表是否可见),确认无误再自行决定是否清理旧副本(不建议 AI 代劳删除,数据不可再生)。

---

## 22. Tauri asset protocol 对一切响应发 ACAO——`<video crossorigin>` 截帧不 taint 的依据(2026-07-22)

视频播放器重构验证:`tauri-2.11.5` 的 `src/protocol/asset.rs` 对**所有**响应(含 206 Range、含错误路径)都发送 `Access-Control-Allow-Origin: <window_origin>`,而非只对成功响应发。这意味着给消费 `asset://`/`convertFileSrc()` 资源的 `<video>` 元素加 `crossorigin="anonymous"`,浏览器会以 CORS 模式取流并因响应头匹配而判定"同源等效",随后 `canvas.drawImage(video)` + `toBlob()` 不会 taint canvas——截帧/取像素数据无需绕后端。**前提**:`crossorigin` 属性必须随 `:src` 一同渲染(不可事后追加,否则首次请求已发出、浏览器不会补发 CORS 预检);dev(`http://localhost:*`)与 prod(`http://tauri.localhost`)两种 window origin 均须验证匹配。适用范围:本仓 Tauri 2.11.x,升级大版本后应重新核实该行为未变。

# 四、架构与验证方法论

## 9. worker 化迁移须先盘点并保住原并发结构

把计算从主进程迁入 worker 前,**先盘点原路径的并发结构**(线程数、流水线相位、背压点)并逐条对账到新结构;「正确性对拍」不覆盖吞吐——结果逐位一致仍可能吞吐腰斩,harness 必须含**吞吐相位**,否则退化在对拍全绿的掩护下溜进主干。

## 10. e2e 对拍的自洽盲区:自洽全绿 ≠ 覆盖率 > 0

对拍两侧若共用同一错误映射或同为空集,结果照样全绿——「自洽」检验不了「接上了真数据」。对拍数据源必须与**生产键**连接验证(join 到真实生产键并**断言覆盖计数 > 0**)后,绿灯才可信。

## 11. 布局缓存四契约(指针,正典在代码注释)

正典:`src-tauri/src/layout/items_cache.rs` 模块头文档(S1 序等价契约、数据代 bump、HIT 四关守卫,S3.1 测试锁定 is_hit_valid 与 HIT 守卫同判据)与 `src-tauri/src/layout/cache.rs` 「失效契约」注释(软删除**有意**不失效不 bump,为暂存删除 UX 服务)。四要点速览:①写路径提交后必须 bump;②改 SQL ORDER 须同步 `derive_order` 并过序对拍;③items 快照是布局行载荷源;④改 HIT 守卫须同步 `is_hit_valid`。**动这些文件前先读模块头注释**,本条只是路标。

## 15. 评审「补齐周期机制」类提案前先查编译期默认行为是否已覆盖(2026-07-13/15)

诊断稿提出「全量生成期间无周期性 WAL checkpoint,需手动补一个」——代码里 grep 不到任何 `wal_checkpoint` 调用似乎印证了这一点,但 SQLite 的 `wal_autocheckpoint` 是**编译期默认值**(1000 页 ≈4MB),不显式配置等于沿用默认,不等于关闭。grep 只能证伪「显式配置」,证不了「机制不在场」。核对生产库当日全部开机日志(WAL 最大 10.4MB、checkpoint 0–3ms、boot→Ready 253–407ms)后,「WAL 膨胀致开机变慢」假设整体证伪,提案中「手动补周期 checkpoint」(与默认自动 checkpoint 同类冗余)被撤回,只保留一行 `journal_size_limit` 作通用卫生封顶(`b24b196`)。**评审任何「系统缺少 X 机制」的提案,先验证 X 的编译期/框架默认值是否已经覆盖,再决定要不要显式补一份**;先埋探针拿真实数据、再评估要不要动代码,比先动代码更省——本例探针数据当场推翻了整条因果链。同一台机器当晚 NVMe 掉线(§13)提供了体感变慢的替代解释,提醒:**在硬件正退化的机器上做性能归因,先排除硬件**。

## 18. 正则扫描器必须先剥注释,否则门禁反噬良好注释(2026-07-15,本仓第三次踩)

**判据:任何基于正则的扫描/门禁,若被扫面允许注释,必须先剥注释再匹配。** 三次前科同一根因、三种伪装:

| 时间 | 被扫面 | 注释里的什么顶穿了扫描 | 症状 |
|---|---|---|---|
| S1 UiDialog | SSR 串 | 模板注释含裸词 `aria-labelledby` | `.not.toContain(裸词)` 恒红,而属性实际未渲染 |
| S1 SelectionActions | SSR 串 | 模板注释含裸 `data-toolbar-item` | `/data-toolbar-item/g` 计数 6→7 |
| S7 主题 token | .vue/.css 源码 | 注释留旧 token 名作历史记录 | 幽灵测绘误报 5/11,真值 4/5 |

第三次最毒,因为**扫描器惩罚的恰恰是好习惯**:本仓修幽灵 token 时按约定在注释留旧名作历史记录(`/* 原 var(--color-danger) 为幽灵 token(S5 修) */`)——那些注释标记的正是**已修站点**,却被永久报红。工具反噬良好实践是最坏的激励结构:人的理性反应是删注释或关门禁,两者都比原 bug 更糟。**不要给写注释的人开豁免面来绕过工具缺陷,修工具。**

两条实现纪律(`src/themes/theme-contract.spec.ts` 的 `stripComments` 是参考实现):
- **只剥不解析**。剥 `<!--`/`/* */`/`//` 三形足矣,别为了精确去写 CSS/JS 词法分析器——门禁的复杂度自身会变成 bug 面。
- **宁可漏判不可误判**。`//` 规则须规避 `://` 否则吃掉 URL。假阴性只是少抓一个幽灵(下次扫到);假阳性会让人关掉整个门禁(永久失守)。

SSR 串那两次还有条附加纪律:**模板注释会被 `renderToString` 渲进串**,故 spec 计数/负向断言涉及的字面 token 一律不得出现在模板注释里,注释改用描述性称呼(「折叠标记属性」而非 `data-toolbar-item`)。且写属性形断言前先 dump 一次真实 SSR 串——Vue 把无值属性渲成 `data-toolbar-item`(后跟空格)而**非** `data-toolbar-item=""`。

亲缘条目见 §12(check_docs 1b 自扫描:同样是「工具扫到自己注释里的示例」,那里的解法是**源头避让**——工具代码内不把 docs 前缀与 `.md` 后缀连成同一字面量;这里的解法是**工具剥注释**。选择判据:被扫面若是工具自己的代码,可要求源头避让;若是全库业务代码,只能修工具——不能要求全仓开发者为门禁的正则让路)。

## 19. 集合级契约证明不了「选得对」——合法取值里选错那个,所有门都是绿的(2026-07-15)

**判据:形如「消费面 ≡ 定义面」的集合级契约,只能证明每个引用都指向某个合法定义,不能证明它指向的是*对的*那个。** 二者之间的缝隙没有任何门守着,而 bug 恰好住在缝里。

S7 实例:设置页 9 张卡里唯独「网络存储」底色/圆角/间距不同,**六套主题全中**(六主题 `bg-surface ≠ bg-elevated` 无一例外,只是暗色主题下差值小到肉眼难辨)。当时三道门全绿:

| 门 | 为何绿 |
|---|---|
| token 引用闭环(消费 ≡ 定义) | `--color-bg-surface` 与 `--card-bg` **都是合法定义的 token**,非幽灵 |
| `check:contrast` | surface 本就是被守的合法底,对比度达标 |
| typecheck / lint | CSS 变量名不进类型系统 |

**取值全合法,错的是取了哪个。** 这类缺陷只有把像素画出来、跨主题比对才现形——本仓的做法是 `npm run capture:themes` 出 6 主题 × 3 场景矩阵,再算**跨主题不变像素**(不随主题动 = 硬编码嫌疑),而**不是** pixel-diff 基线门(见下)。

两条连带纪律:

- **别为不可控/合法差异设基线门**。截图基线门在字体栅格/抗锯齿/浏览器版本任一变动即红,而红了唯一可能的响应是「重新生成基线」= 幽灵门禁反模式(判据:**设阈值前先问「红了我能做什么」;若唯一答案是「改阈值/改基线」,就别设**)。故机器只做**捕获**这件苦力,判定留人眼——人眼要抓的正是既有真门证明不了的那类。
- **拿测量当证据前,先测量测量本身**。同主题连拍两次做确定性对照(实测:porcelain 逐字节相同;ink 差 482 像素 = 0.04%、最大通道差 3),确认噪声只会让不变性**低报**,上面那些百分比才配当证据。

本例的直接机制(Vue 特有,非本条主旨):组件根若是另一个组件,**scoped CSS 的 `data-v` 会打在子组件根节点上**——故残留的自绘 `.settings-card` 命中了 `CollapsibleCard` 的根 div、盖掉全局卡;而同一 style 块里的 `.settings-card__header` 因拿不到该 `data-v` 是死代码。**残留样式一半活一半死,死活线正好卡在 scoped 规则上**——删旧组件的自绘样式时,别以为「反正是 scoped 的,留着无害」。

## 23. License 尽调不能只看项目声称的许可证,要核实构建产物的实际编译选项(2026-07-22)

调研 mpv/libmpv 作为视频解码后端的可行性时踩到:mpv 上游默认协议是 GPLv2/v3,只有专门加 `--enable-lgpl` 编译标志(且因此阉割掉部分依赖 GPL 库的功能)才会变成 LGPL——但社区/第三方发布的"mpv"或封装它的 Tauri 插件常笼统写"MPL-2.0"或不注明具体构建方式。**判据**:遇到"核心库本身是 dual/conditional license"的依赖(mpv、部分 ffmpeg 编译产物同理),不能信任包装层或下游项目页面写的许可证字段,必须找到该二进制/依赖树的实际编译配置(configure 参数、build script)逐条核实;找不到就默认按更严格的那个协议(此处即 GPL)处理,触发红线宁可错杀。

## 24. 状态持久化选层:逐条目用户状态进 DB,全局纯前端偏好进 localStorage(2026-07-22)

视频播放器重构裁定播放位置(逐条目)进 DB、音量/倍速/循环(全局偏好)进 localStorage 时提炼出的可复用判据:**凡是"跟着某一条数据记录走"的用户状态(旋转角度、播放位置、评分等)归 DB**——它需要随备份/恢复走、跨设备/多库切换要正确、且天然有既定归宿字段可挂;**凡是"跟应用本身走、与具体记录无关"的纯前端偏好(音量、倍速、面板折叠态等)归 localStorage**——引入后端 config 热更新链对付几个标量过重,且这类偏好本就不该参与备份语义。判断入口:先问"这个状态换一条记录/换一台设备后,用户预期它还在吗"——预期在场即 DB,预期不携带即 localStorage。

# 五、文档治理与门禁

## 12. 门禁 1b 自扫描:工具代码里的 docs 路径字面量一律被验存(2026-07-12)

`tools/check_docs.mjs` 的 1b 节扫描 scripts/tools 下 .mjs 里一切「docs 前缀 + `.md` 后缀」样式的字面量并验证可达——**测试 fixture 的假路径字符串、甚至警示注释里的示例都会命中**(给 check_docs 自身写 closeout 自检时两度被自己拦红:先是 fixture 假路径,后是「不要写这种字面量」的注释本身)。规矩:**工具代码内不把 docs 前缀与 `.md` 后缀连成同一字面量**,fixture 路径经变量拼接(如 `${D}/worklogs/…`);不要反过来给脚本开豁免——豁免面一开,真断链也进得来。

# 六、硬件与运行环境

## 13. NVMe 瞬时掉线伪装成"文件系统损坏"，`npm run tauri dev`/`build` 报"系统无法打开指定的设备或文件"（2026-07-13）

**现象描述:**
`npm run tauri dev` 与 `npm run tauri build` 双双失败,报 Win32 级错误"系统无法打开指定的设备或文件"。逐层复现:`npx tauri --version` 报 `npm error could not determine executable to run`;`Get-ChildItem` 枚举 `node_modules/@tauri-apps/cli` 目录直接抛 `A device which does not exist was specified`(`IOException`/`DirIOError`),但对目录内单个文件 `stat` 却仍能返回——只是元数据全是垃圾值:`link count` 显示 `21600`(正常应为 `1`)、`size 0`、时间戳 `1601-01-01`(NTFS `FILETIME` 的纪元 0 刻)。

**排查中的误判(记录以防复发):**
最初判断为 NTFS 目录项(MFT 记录)物理损坏,按此思路做用户态修复:`rm -rf` 删除报 `Directory not empty`(目录声称非空,但 `readdir` 只列出 `.`/`..`——典型的孤儿记录死锁,枚举不到、又删不掉);改用 `Rename-Item` 把损坏目录挪走**成功**(返回 `RENAME_OK`),但随后 `npm install` 试图重建 `@tauri-apps/cli` 时在 `mkdir` 阶段报 `ENOENT`——**连父目录的写入都失败**,说明损坏不是单个目录的孤立问题。进一步在 `node_modules`、`@tauri-apps`、乃至**项目根目录**分别探测性创建测试目录,三处全部报同样的 `A device which does not exist was specified`——问题已不是某处文件系统元数据坏了,而是**整块卷不可写**。

**根因(Windows 事件日志实证):**
`Get-WinEvent` 在故障发生的同一秒(`22:54:42`)连续三条日志钉死了机制:
- `stornvme` **Event 129**:`Reset to device, \Device\RaidPort2, was issued.`(NVMe 驱动对该设备发起强制复位)
- `disk` **Event 51**:`An error was detected on device \Device\Harddisk1\DR1 during a paging operation.`(`Harddisk1` 经 `Get-Partition -DriveLetter D | Get-Disk` 核实即 D: 盘所在物理磁盘)
- `Ntfs` **Event 50**:`{Delayed Write Failed} Windows was unable to save all the data for the file D:\actions-run...`(缓存脏数据没能写回磁盘)

即:承载 D:(项目所在盘,Samsung MZVLB1T0HBLR,NVMe,走 Intel RST/VMD 的 RaidPort2)在负载下(自托管 CI runner + npm/cargo 并发 I/O)瞬时掉线,NTFS 对已失联设备的元数据查询返回垃圾值——这正是"文件损坏"假象的真实来源,并非磁盘介质本身损坏(事后 `Get-PhysicalDisk` 复查两块盘均 `Healthy`/`OK`)。

**决定性验证(重启即复原,证明写入从未落盘):**
排查过程中执行的 `Rename-Item`(改名到 `cli.corrupt-*`,返回成功)与后续 `npm install`(尝试重建 `cli`,失败)本应在磁盘上留下痕迹。但重启后核对:`@tauri-apps/cli` **完好如初**、改名产生的 `cli.corrupt-*` **凭空消失**——说明掉线期间的所有目录操作只发生在易失的文件系统元数据缓存里,从未真正落盘;重启重新枚举 NVMe 设备、从磁盘上原本完好的状态重新挂载卷,因此一切自动恢复。期间系统日志确认**未执行过 chkdsk**(`Application` 日志无 `Chkdsk`/`Wininit` 自检事件),排除"chkdsk 自动修复"这一替代解释。

**诊断方法(可复用的判据):**
出现"文件/目录时有时无、`stat` 拿到 `size 0` + 反常时间戳(尤其 `1601-01-01`)+ `link count` 异常、`readdir` 与真实内容对不上、`Get-ChildItem`/`rm -rf` 抛 `device does not exist`/`Directory not empty` 且现象逐级向父目录蔓延"这类组合症状时,**先查 `Get-WinEvent` 里同一时刻的 `stornvme`/`disk`/`Ntfs` 事件(Event ID 129/51/50 一类),而不是先怀疑 NTFS 物理损坏或依赖包本身坏了**。若能在故障时间点附近找到"设备复位 + paging 错误 + delayed-write-failed"三件套同时出现,基本可以确定是存储设备瞬时掉线导致的缓存态假象,而非真实文件系统损坏。**判据分层核实(2026-07-13 晚复现一例补证)**:掉线期间 `Get-Disk` 会直接枚举不到该盘(`Number` 查无),但 `Get-PhysicalDisk` 仍列出设备、`Get-Volume` 仍报 `HealthStatus=Healthy`——**卷级健康检查在此类瞬断中不可信**,以 `Get-Disk` 设备枚举 + System 日志事件为准,`Get-Volume`/`Get-PhysicalDisk` 的"看似正常"不能作为排除掉线的证据。

**处置与防复发:**
1. **铁律:遇到此类"重启前一片狼藉"的存储怪象,第一动作是重启**,不要 chkdsk、不要删 `node_modules` 重装——那些操作要么查不出真实问题(元数据本就是掉线设备返回的假象),要么是在会自动复原的状态上做无用功甚至引入新风险。
2. 本机此前已有另一条独立的硬件不稳定证据链(同期负载下蓝屏 `BugCheck 0x1A`/`0xA`、`rustc` 非确定性 `0xc0000005` 段错误,`WinDbg` 分析定位到内核 MM 元数据被物理内存位翻转损坏);本次 NVMe 掉线与之同机同源,共同指向"该机硬件在高负载下不稳定"这一更上层结论——排查该机上的环境/工具链故障时,若同期已出现其他硬件级异常,应优先怀疑硬件而非反复怀疑软件配置。
3. 缓解方向(治标,尚待验证):更新 Intel RST/VMD 驱动与 Samsung NVMe 固件;关闭 PCIe 链路电源管理(ASPM)与 NVMe APST 激进省电档;检查 M.2 温度/散热与插槽连接是否牢固;降低同一块盘上的并发 I/O(尤其避免自托管 CI runner 与本地重编译同时压这块盘)。
4. **数据安全提醒:**`{Delayed Write Failed}` 意味着掉线前后写入的文件存在不一致风险(`node_modules`、git 对象、SQLite WAL 均在此列);重要成果应勤 commit,不能只依赖磁盘缓存兜底。

## 20. 本机设备策略把 `.ps1` 脚本与 `iex` 字符串都降入 ConstrainedLanguage,.NET 类型解析全灭;同代码内联单链不受限(2026-07-17)

官网 og 图生成时踩到:`Add-Type -AssemblyName System.Drawing` 之后 `[System.Drawing.Color]` 报 `Unable to find type`,同一段代码三种执行路径结果不同——**`.ps1` 脚本文件红、`iex (Get-Content -Raw …)` 也红、单条内联命令链(`Add-Type; [type]::…` 用 `;` 连在一次调用里)绿**。定性:WDAC/AppLocker 类设备策略按「不可信脚本内容」降语言模式(ConstrainedLanguage 禁任意 .NET 类型解析),`iex` 吃进的字符串同样按不可信处理;交互式/内联命令保持 FullLanguage。复核可用 `$ExecutionContext.SessionState.LanguageMode`。规矩:**在本机凡需 .NET 类型的 PowerShell 任务不落脚本文件**——要么压成内联单链,要么干脆换工具(og 图最终改 headless Edge 渲 HTML 截 PNG,字体渲染反而优于 GDI+)。判据:报错是 `TypeNotFound` 而非权限/路径类,且内联可复现成功,即可断定是语言模式而非装配缺失。

## 21. headless Edge 截图验收三坑:锚点/scrollTo 不可靠、vh 段随视口膨胀、同源 iframe 探针才是布局真相(2026-07-17)

官网 v2 验收(Edge `--headless --screenshot`)踩齐三坑:

1. **锚点与滚动都到不了位**:`#fragment`、iframe 内 `scrollTo({behavior:'auto'})`、`--disable-smooth-scrolling` + `--virtual-time-budget` 组合逐一试过——要么仍从页顶拍,要么滚到段间拍出整屏空白(页面 `html{scroll-behavior:smooth}` 连锚点跳转都动画化,截图赶在动画前;禁平滑后又疑似撞上 virtual-time 合成竞态)。**结论:headless 直截图只信「从页顶可见」的内容;下方段落要么用 JS 布局位移(负 margin)要么留真机走查,别硬凑。**
2. **vh 段随截图视口膨胀**:340vh 的 sticky 展卷段在 `--window-size=1280,2600` 超高视口下等比放大,截图中出现「巨洞」,酷似布局 bug——实为 vh 语义,按真实视口高度(~900px)评估即无恙。超高视口整页截图对含 vh 尺寸段落的页面不可作布局证据。
3. **像素图会说谎,DOM 测量兜底**:390px 宽直截图里顶栏按钮「消失」,两轮 CSS 排查无果;最后写同源 iframe 探针页(iframe 定宽装载目标页,枚举 `getBoundingClientRect()` 越界元素 + `documentElement.scrollWidth`)实测 `scrollWidth=390`、零越界、按钮齐全——「按钮消失」纯属 headless 渲染伪影。**视觉验收必须配 DOM 测量兜底;截图异常先探针再改码,否则会去修不存在的 bug。**

## 25. 子代理的「复核回执」会编造发现,引文必须逐字机械比对才能采信(2026-07-25)

全仓注释清理线(约 31000 行注释、283 文件改动)靠子代理分域审 1078 个 hunk,过程中**两份复核回执被查出编造发现**:回执里以「被误删的英文原文」形式给出的引文,拿去在全仓 diff 里逐条 `grep` 全部不存在;其中一条「ORT session 非 Send」的编造论断还被下游修复代理照抄进 `crates/scrollery-ai-core/src/engine.rs`,产生了一处并不需要的改动——该 crate 早已用 `Receiver<Session>` 模式规避跨线程持有 Session,前提根本不成立,事后证伪删除。

**判据与纪律:**

1. **回执自称「已对抗核验」不构成证据**。第一份编造回执由 opus 产出、尾部还带明显退化痕迹;是否声明经过核验、模型档位多高,都不改变要机械比对的要求。
2. **可采信的复核流程是取证式的**:先用确定性脚本从 diff 里机械抽取待判行(如「所有被删的非 CJK 注释行 + 保留上下文」)落盘,再逐条对着文件判定。判定结论必须能对着落盘清单复算,而不是只存在于代理叙述里。
3. **别让未核实的回执直接喂给修复代理**。误改的传播路径就是「审计回执 → 修复代理照抄」,中间少了一道人工/机械核实,错误论断就变成代码。
4. 同源教训:低档模型的「零命中」结论同样不可信——同一线首轮英文注释摸底报「双语重复 0 处」,随后确定性脚本仅在 `ipc/` 单目录就删出 194 行相邻双语对。**凡「没发现问题」的结论,都要有确定性脚本对拍。**
5. **脚本也要校验语义等价,不能只看行邻接**。同线规则 1(英文行紧邻中文译文行 → 删英文)只按邻接判定译文关系,误删了 Copybara `// BEGIN-INTERNAL` 剥离标记(配对 END 留在原地,会让闭源块漏进公开镜像并踩 FORBIDDEN 禁词门禁)、cargo/shell 跑法、算例、公式、列表编号、intra-doc 链接。带守卫的脚本仍需一轮全量取证式复核。
## 26. 内存/性能探针三坑:PeakWorkingSet 单调不减、setup 预提交页面、阶梯定位法(2026-08-14)

- Windows GetProcessMemoryInfo 的 PeakWorkingSetSize 单调不减,同进程连续起多组探针,后组永远读不出真实 delta。
- 即便每组独立进程,只要 setup 阶段在测量进程内合成源数据,同样预提交页面、同样腰斩 delta(实测 JPEG 2.95 → 真值 6.02 B/px)。**诚实测法=源数据由另一个进程产出,测量进程 setup 只读文件**。
- 「内存泄漏」表象先分流:稳态泄漏 vs 无界进行中工作(布局永不完成)——排查手段完全不同;本案用阶梯法(裸引擎→忠实管线→变量对照)一次定位。
- CSS multicol 是乘积型放大器:整文档 rect 查询 × fragmentainer 数;任何「单容器塞全文再分栏」的阅读器管线都有此雷(epub 靠一章一 iframe 天然拆弹)。

## 27. Tauri Channel 与发起 WebView 同生命周期,长任务进度必须「事件广播 + 后端快照可查询」双件套(2026-08-14)

Channel 进度传输在 webview reload 后即断且不可重订阅,只适合与 invoke 同生命周期的短任务;可恢复长任务(缩略图/备份/导出)须用 AppState snapshot + app event + status query + generation 清理。

## 28. 就地覆盖库内源文件三坑(2026-08-14)

凡「改写库内源文件」类功能先过三关:invalidate 连坐删 faces/embeddings(scan.rs:690)、查看器无像素级重载事件 + convertFileSrc 无 cache-buster、目录 mtime 剪枝漏检就地改(fast_scan.rs:214)。

## 29. 图片编辑另存:view_rotation 烤入 + 元数据矩阵 golden spike 先行(2026-08-14)

V20 view_rotation 是持久化非破坏显示态:原字节导出不烤入,图片编辑另存时必须烤入且新 item 归零;orientation/EXIF/ICC/thumbnail/WebP 输出是格式矩阵,必须 golden spike 后再承诺。image 0.25.10 对 JPEG/PNG 的 ICC/EXIF 读写原生齐全;webp 0.3.1 有损绑定无 ICC/EXIF API(输出补元数据须手工 RIFF 注入)。

## 30. MF XVP 自动转正陷阱:rotation 属性不清零、SetRotation 拦不住、单声明旋转会信箱化(2026-08-14)

Windows Media Foundation 输出类型声明 rotation 属性后内容不转了,但输出尺寸仍被默认成旋后宽高,内容被信箱化;唯一稳修=输出类型 rotation+FRAME_SIZE 双属性恒显式钉死,旋转权威唯一归 CPU 侧。

## 31. 诊断单测 #[ignore]+env 门控完胜 release bench 盲试(2026-08-14)

#[ignore]+env 门控诊断单测(debug 13s/轮)完胜 release bench 盲试(1.5min/轮);两连败即该换观测手段。

## 32. chardetng 采样窗切多字节编码:「告警不响」≠「没病」(2026-08-14)

chardetng 采样 64KB + feed(last=true),切点落多字节序列中间 → >64KB 无 BOM UTF-8 中文书整册静默误判 windows-1252(1252 全字节可映射,替换率恒 0,Lossy 告警不响)。修=截断采样 feed(last=false)。教训:采样窗切多字节编码必须按流式语义喂检测器;窄检测器的兜底告警要审「哪类错根本不触发它」。

## 33. 并行会话共库时的选择性暂存术(2026-08-14)

git show HEAD:f + 注入我方行 + git hash-object -w + git update-index --cacheinfo:index 只收本线 hunk、工作区他线改动原样保留;git apply --cached 路线会被 PS 管道 CRLF/BOM 污染,Bash awk 拆 hunk 易掉文件头,直写 index 最稳。

## 34. 新建 composable 前先 Glob 撞名;横切能力先查统一原语(2026-08-14)

本仓横切能力(溢出折叠/弹层/焦点陷阱/虚拟滚动)多已有统一原语/composable,自造即分叉;新建前先 Glob 查重。另:Write 工具参数管道可注入 NUL(U+0000)入源文件(eslint 报 unexpected-null-character,Read 不可见),须 PowerShell 逐字符码点定位+字节替换。

## 35. 长任务终态门控两姿态并存,勿「统一」(2026-08-14)

thumb/derive 流水线=finish 返回值门控终态发布;ai/face=!is_cancelled 门控终态副作用。新长任务(文件任务类)须明确采前者,勿以「统一」名义改语义。

## 36. 库级根排除选条件子查询而非反规范化(2026-08-14)

热路径无隐藏时零改动 + 零同步点,胜过 volume_id 式冗余列(move/copy/rescan 泄漏隐患)。

## 37. 未注册文件的外部打开是执行面;capability 不能替代领域边界(2026-08-14)

canonicalize + 根边界只挡「路径越界」,挡不住「根内文件本身可执行」——未注册文件 Desktop 双击须经自有受限 IPC reveal,不走系统默认应用 open,不向 WebView 授通配 opener 权限。

## 38. schema.rs 是版本化 DDL 而非声明式期望态;facet 派生知识下沉 registry(2026-08-14)

改 SCHEMA_V1 只影响全新库,存量库须新迁移;「完整 DB」与「精简业务态」是两种不同备份产品。DB 只回答它独有的问题(哪些值存在),派生知识(大类归属/分组)下沉 registry:facet 216ms 全扫 → 0ms。另:文件树三序不一致(目录 BINARY/文件 NOCASE/画廊 NATURAL_CMP)是既有债,动序须独立立项。

## 39. 证据与验证:引用行号范围端点是断言;死函数可能是编错;零 importer 镜像必然漂移(2026-08-14)

① 用行号区间做证据时,区间端点本身就是断言(「无 file_format 索引」的错正好卡在行号端点);② 「死函数」可能是被开编码了一遍还编错(如 doc_subtype 与 doc_type 分裂);③ 零 importer 的镜像(前端 formats.ts 镜像后端 registry)必然漂移且无信号。

## 40. 变异验证与还原手段都要自证;「测试全绿」≠「测试有效」(2026-08-14)

变异验证抓出的假测试有四种失效形态(期望源手写错/断言没钉住/哨兵自检缺失/变异没复现);还原手段(如 git checkout 恢复变异)与仪器本身都必须先自证——同一件事上栽两次:变异没还原干净导致后续验证全废。

## 41. 类型谓词守得住动作面,守不住判等;双数据源静默漂移;删死守卫先问竞态会不会复活(2026-08-14)

① id 放宽为可选后,FS-only 行五处 `null === null` 判等恒真(真机问题根因)——类型谓词收窄了动作面,判等仍要 spec 钉;② 根治「手工枚举抄漏」的那次提交,自己在另一处手工枚举里抄漏了同字段——枚举收敛必须机械对拍;③ 双数据源下「共享节点沿用旧轴字段」是静默语义漂移;④ 删「恒不触发的死守卫」前,先问引入它的竞态会不会被本批变更复活。

## 42. 工具与测量坑:cd 泄漏、mtime 保留式恢复、异步回写越限、锚点续命(2026-08-14)

① Bash 工具 cd 会泄漏进后续命令 cwd(本仓工具约定:显式路径,勿裸 cd);② shutil.move/cp -p 恢复源码保 mtime → cargo 跑陈旧二进制,恢复后须强制重编;③ 异步 LRU 删除 loading 后迟到回写会绕过容量上限——请求所有权须与缓存槽绑定;④ 「捕获→异步重算→恢复」型机制的过期策略必须以在途信号续命(400ms 壁钟清锚 vs 秒级 IPC 竞态),不能裸靠壁钟;⑤ minimap 滑窗几何映射端点必单测,勿信推导;⑥ 组件级测试判定:「.spec.ts 缺失」是本仓默认态,判据=是否存在同构测试先例而非有 spec 就该抄同名;⑦ 布局/溢出类键(如 overflowRemeasureKey)漏配无静态信号——不报错/不掉测试/SSR 也过,只在真机抖动,顶栏类折叠机制加键必须同步补 widthDeps 类清单。

## 43. 大文件拆分方法论:审查分型/结构性前提/共享 helper 迁移序/setup owner/豁免触发器(2026-08-14)

① 大文件审查先分型:「测试撑大」型(生产紧凑)不拆,「零测试巨命令」型与「多语境聚合」型才是拆分对象,LOC 单指标会误判;② 已批准方案在并行线落地新代码后,失效的往往不是数字(抗漂移机制可吸收)而是结构性前提(「某模块是叶子」);③ 共享 helper 落入「owner 最后迁」的拆分序时,消费者先迁会引用不存在模块——跨域 helper 迁移序须 owner 先行;④ setup 类生命周期巨块先迁完整 owner 保顺序与共享局部量,再在 owner 内拆;⑤ 豁免只对带 commit 快照有效,须配结构/增长/缺陷触发器。

## 44. RAW 式重量级格式:嵌入预览优先 + sidecar 桌面专属约束 + 协议变体脱 workspace 检查(2026-08-14)

嵌入预览提取(不做完整 demosaic)是 RAW/未来重量级格式的默认设计模式(ms 级 vs s 级);sidecar 独立进程模式是桌面专属约束(iOS/Android 沙盒禁子进程);共享协议(如 exotic-protocol RequestBody)加变体时,脱离父 workspace 的 gnu worker crate 不会随 workspace 编译——协议改动须点名检查所有脱 workspace worker,由 CI gnu job 兜住。

## 45. 稳定码三胞胎与可拖行类的尾随 click 抑制(2026-08-14)

① Exotic/Reveal/Preview 稳定码变体已三胞胎同姿态(`{code,message}`)——新 IPC 错误码照此,勿再引入第 4 种形态;② 树行/卡片拖拽后必须抑制尾随 click(每个新增可拖行类都要重演:目录行有先例、文件行又栽一次,收敛为共用 helper 或契约测试)。


## 46. 真机性能测量效度:实例级渲染降级、HMR 清 LRU、draw span ≠ 帧健康(2026-08-16)

同日同码(纯 a510105/195)不同 app 实例的帧率在 43/60/118fps 三档间摆动,与显示刷新率无关(空闲 rAF 恒 121Hz)、重启即变、成因未明(疑 WebView2 efficiency mode/GPU 电源状态)——**绝对值跨实例不可比,真机 A/B 必须同实例内运行时切换(免 HMR 开关)或每臂重启交错多 rep**。两个配套坑:① Vite HMR 重挂载会重建 Canvas 位图 LRU,「暖区」测成冷区,代码切换后必须重跑热身趟;② `gallery.draw` span 只量 JS 提交侧,合成/光栅恶化一个量级时 span 反而更快(见 §47)——帧健康判据必须含 rAF 帧间隔,不能只看 draw span。**交付记录还须把“当前代码事实、同实例 A/B 数据、尚待验证假设”分栏；历史测量不能替代当前实现的性能结论。**

## 47. canvas2d 巨路径合并反噬:联合 clip 与巨 stroke 掉出快路径(2026-08-16)

Canvas 画廊全选态(数百格)把逐格 roundRect clip/遮罩/描边环合并为「单条联合 clip + 单路径一次 fill/stroke」,JS 侧 draw span 2.4ms(最快)但帧率 43fps;归因 2×2:数百子路径联合 clip 是灾难级主因(6.7fps/帧 p50 150ms——对后续每张 drawImage 都构成复杂裁剪区),巨路径 stroke 单独也回退 ~19fps;而逐格绘制(数百次小 fill/stroke/clip)在健康实例 118–120fps 贴满 vsync。教训:**WebView2/Skia 的 draw-call 合并直觉不成立,「减少调用数」必须先过同实例帧率 A/B**;逐格小路径恰在快路径上。完整数据见 reviews/2026-08-16 审查报告 §9.4。

## 48. 真机 rig 踩坑:harness 桩、屏外窗口与空视图死态(2026-08-16)

① headless harness 缺 `__TAURI_INTERNALS__` 桩时 `new Channel()` 与 `convertFileSrc` 在热路径逐帧抛错、gauge 全空——CDP `Runtime.exceptionThrown` 捕获 rAF 异常是定位利器(main.ts 最小桩已固化);② app 保存的窗口状态可损坏至屏外(-21333,-21333),CDP setWindowBounds 位置被宿主钳制,**Win32 MoveWindow 直移 MainWindowHandle 有效**;③ 页面 reload/HMR 重挂载后 app 可进入「0 个项目」死态(布局 width<100 放弃后无自愈路径,软恢复全无效须重启)——dev 反复可见,生产首挂载竞态待立项。测量 rig 遇「draw=0/fps=120/gauge 空」先查这三处。

## 49. 批循环 LIMIT 分页优先 keyset;rusqlite 热循环要显式 prepare_cached(2026-08-21)

「每批 `WHERE pending ORDER BY ... LIMIT ?` → 处理 → 下一批重查」看似自然,实际每批都对剩余候选重新排序或从 rowid 头跳过已处理前缀,整体近 O(N²/B);本例图片 enrichment 简模 100k 行仅选批约 9.5s,换 `(sort_datetime,id)` keyset + `(media_type, sort_datetime DESC, id DESC)` 索引后约 0.19s。通用修法:稳定序直接 keyset;复杂序建一次性 TEMP 队列表;`m.id > cursor` 是视频/音频等无显式排序队列的最廉价形态。另:rusqlite 的 `query_row/execute` 每次重新 prepare,没有跨调用语句缓存——百万文件级循环须在连接上用 `prepare_cached` 或调用方预 prepare,否则 SQL 编译本身成为稳定常数项。

## 50. 增量剪枝要在 stat 之前;UNIQUE 隐式索引可替代同前缀显式索引(2026-08-21)

增量扫描的「跳过 per-file 工作」若放在拿到 `WalkedFile` 之后,遍历器早已为每个文件付了 `metadata()`——最大开销没省。应把剪枝钩子挂在目录**进入时**(walkdir `file_type()` 零 syscall),未变目录的直接文件根本不产出,子目录仍独立下降。索引侧同理:表上 `UNIQUE(directory_id,file_name)` 的隐式索引已覆盖 `idx_media_directory(directory_id)` 同前缀查询,显式索引只是给 INSERT/UPDATE 多维护一棵 B-tree;删除前用 EXPLAIN 确认计划改走 `sqlite_autoindex_*`,并同步计划锁定测试。

## 51. 单写连接上的 TEMP 表必须按业务域隔离、用后即毁;连接级资源命名带调用代次(2026-08-23)

`DbWriter = Mutex<Connection>` 意味着所有写路径共享一条连接,TEMP 表是**连接级**而非「调用级」——两个看似独立的函数各自 `CREATE TEMP TABLE IF NOT EXISTS + DELETE` 清空,在并发(如不同 scan_root 同时扫描,入口只互斥同根)时就互相清表:后启动者把进行中者已流式写入的 seen 集抹掉,收尾差集把已见项整批误标 missing。修法:表名按业务域后缀化(`_mm_seen_r{root_id}`,i64 派生无注入面)+ 启动时清空自愈 + **真实差集完成后 DROP**(依赖"下轮再清"会让 temp_store=MEMORY 的多根各表在连接生命周期内常驻累积)。配套:`fire-and-forget` 任务的连接级资源(如富化队列表)还要带**调用代次**后缀(进程内 AtomicU64),否则旧任务收尾 DROP 会误删新任务刚建的同名表——窄窗口但真实。另:灌入 SQL 含 `?1` 时必须 `prepare` 绑定,`execute_batch` 把未绑参数当 NULL **静默灌空表**,不报错。

## 52. 本机沙箱:含子目录的目录不可 rename——逐文件 git mv + rmdir 空壳绕过;探针勿用真实文件(2026-08-24)

症状:`git mv`/`mv`/PowerShell `Rename-Item` 对**含子目录**的目录一律 OS 级 Permission denied,而平铺目录改名、单文件移动、平铺子目录自身改名全部正常;停 `git fsmonitor--daemon` 无效、icacls 无差异——是沙箱文件代理的系统性限制,不是句柄占用或权限问题。绕过:目标目录先 mkdir,逐条 `git mv` 文件(子目录同样先 mkdir 再逐文件迁入),最后 `rmdir` 空壳;索引终态与整目录 rename 完全等价(git 按内容检测 rename,worklogs 42 目录批量改名中 5 个含子目录的即此法落地)。两条教训:① 目录改名失败先造一次性探针目录(含子目录)复现,区分「环境规则」与「偶发句柄占用」,别按惯性怀疑进程占用;② 做移动类探针**只用独立临时文件名**,勿拿真实文件 mv 出再 mv 回——本次曾因此把一个 closeout.md 弄成未跟踪探针名被清理,靠 `git show HEAD:` 才恢复。

## 53. 主许可证迁移必须与第三方归属分层(2026-08-25)

许可证迁移不能对全仓 `Apache-2.0` 做机械替换：项目自身声明应更新根许可证、包元数据、对外文案和分发资源；lockfile、`NOTICE.md`、vendored 源码及源码中描述外部依赖的条目必须继续反映各自原始许可。执行前先用贡献历史与 CLA 确认第一方权属，再按公开投影边界审查闭源目录，最后用项目级搜索把剩余命中分类为第三方、历史记录或迁移说明。对 MPL-2.0，安装包还必须给可执行形式接收者一个可发现的对应源码入口，不能只在仓库 README 里声明。
