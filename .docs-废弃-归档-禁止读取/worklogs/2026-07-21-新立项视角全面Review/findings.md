---
status: snapshot
type: working-memory
line: 新立项视角全面Review
created: 2026-07-13
---

# 发现与决策:新立项视角全面Review

> 📦 2026-07-21 已收口归档(U-16):候选 F-001~F-008 处置见 closeout.md;正文冻结。

## 需求
- 用户原话要点:不看具体代码实现、忽略项目内固化方案;以"新立项/新建项目"视角深度全面 Review 产品设计/技术选型/架构方案/数据结构;发现的不足、产品建议、重构建议全部落报告;可用子代理联网调研同类开源/竞品;子代理模型限制:复杂=opus-4.8、简单=sonnet-5、极复杂才 fable-5;报告落盘。

## 发现

### 第一手抽查(2026-07-13,orchestrator 本人,证据=文件直读)
- **workspace 拓扑**:src-tauri 主 crate + `crates/{exotic-protocol, exotic-workers/{psd-probe,psd-worker,ai-worker}, scrollery-ai-core, scrollery-exotic-trust, scrollery-free-stub, scrollery-plugin-api, scrollery-pro(私有)}`;`[profile.*]` 在仓根 Cargo.toml。
- **产品域远超照片**(src-tauri/Cargo.toml 依赖面实证):图片 + 视频(Windows Media Foundation)+ 音频(lofty 封面/标签/歌词)+ 文档/EPUB/txt 阅读器(zip/chardetng/encoding_rs/regex/ferrous-opencc/similar;前端 pdfjs-dist+shiki)+ PSD(独立 worker)+ WebDAV 网络盘(netfs feature);另有 wallpaper/arboard/trash/lexicmp 桌面集成。
- 阅读器规划了**远程 AI 校对**(reqwest+rustls;keyring 存 API Key;注释提及后续 Ollama/WebDAV)。
- **feature 矩阵**:lite/perf × channel-direct/msstore/steam(compile_error! 互斥)× face-noncommercial(合规隔离)× exotic-dev-fixtures(debug 双门控);updater 仅 direct 渠道链接。
- **DB**:30 张表(media_items + image/video/audio/document 四类 meta 分表;阅读器 6 表;faces/persons/face_rejections/face_coverage;exotic 3 表;albums/tags/ai_embeddings/ai_search_results/volumes/storage_backends/scan_roots/directories/app_config);34 个索引;`CURRENT_VERSION=18`,顺序 `if version < N` 块迁移,版本存 app_config KV(非 PRAGMA user_version)。
- **文件名搜索=LIKE**,FTS5 仅为注释里的「Phase 3 将迁移」意向,未实现(src-tauri/src/ipc/search_commands.rs:16)。
- content_hash 列存在(schema/queries 共 27 处);2026-07-02 全景扫描曾判「死列零比对链路」,现状待 C 代理确认。
- **前端依赖极简**:无第三方 UI 框架(vanilla CSS variables);vue-i18n 在列;onnxruntime-node 仅 devDeps。
- **host 零 ort**:推理恒在 ai-worker 子进程(ai-core 为唯一 ort 载体);f16 嵌入常驻缓存,注释给出 1M×512 ≈ 1GB 内存预算。
- windows crate 为 cfg(windows) target 限定;WIC/D2D/D3D11/DWM/DXGI/MediaFoundation 全为 Windows 专属面(mac 后端未建)。

### 子代理C回报:数据层全景(2026-07-13,已抽验两处关键论断属实)
- **30 表全景**:核心=media_items(+15 个多为 partial 的索引)+ image/video/audio/document 四类 meta 1:1 分表(列存非 blob);策展=albums/album_items/tags/item_tags(tags **未见查询消费者**);AI=ai_embeddings((item_id,model_name) PK,BLOB f32 LE,默认 cn-clip-vit-b16 512 维)/ai_search_results(每搜 DELETE 全表重填);人脸=persons(centroid BLOB)/faces(一图多脸)/face_rejections/face_coverage;插件=exotic_catalog_formats/exotic_plugins/exotic_tasks(租约+覆盖索引);阅读器=reading_progress/doc_replacements/document_versions(类 git 快照树)/text_book_index/reader_book_prefs/reader_bookmarks;基建=scan_roots/directories/volumes(卷 GUID 插拔感知)/storage_backends(cred_ref→keyring 密码不落库)/app_config(KV 总仓,设置全在 DB 非 JSON)。
- **迁移**:V18,STEPS 顺序块,每块 DDL+版本号同事务原子提交(migration.rs:57),失败回滚可安全重跑;**单向无 downgrade,无 integrity_check**。PRAGMA:WAL+NORMAL+busy_timeout 5s+mmap 256MB;写单 Mutex,读 r2d2(4);启动 wal_checkpoint(TRUNCATE)。
- **文件身份=(directory_id,file_name) UNIQUE,无内容去重**;cache_key=xxh3(rel_path/name|mtime)→**移动/改名/touch 即全套派生工件作废重算**;content_hash 为「可疑变更」基线(sha256 全文≤64MB/抽样>64MB),仅 size 相同时比对,不参与去重。
- **时间轴无 epoch_day 列**:布局引擎内存派生 div_euclid(86400)(justified.rs:237,本人已验)。
- **热路径**:默认全库视图**故意 unary `+` 压制 idx_media_sort**(queries.rs:1073-1084,本人已验,注释含 EXPLAIN 实证:索引序随机回表 1M 实测 6.6s→顺序全表扫+内存排序 ~100ms)——**全集物化模型,非 SQL keyset 下推**;viewport 元数据批量 PK 查 ✓;文件名搜索 LIKE '%q%' 全表扫无 FTS5;语义搜索无向量索引,常驻 f16+rayon 暴力点积,VectorStore trait+ANN dormant(阈值 500k);回收站 keyset 翻页 ✓。
- **盘上工件**:app_data_dir 下 cache/thumbnails/{120|240|480|960}/{2-hex}/{cache_key:016x}.webp 四档;ai_thumbs/face_thumbs/sprites/motion_videos/audio_covers;models/;write_atomic(tmp+同卷 rename)全覆盖;LRU 三缓存共预算+对账 GC;thumbhash 存 DB 列。
- **可移植性红灯**:全部策展(persons 命名/collections/ratings/color_label/tags/favorites/书签)只存 scrollery.db,**零写回**;XMP 仅读(Live Photo 检测);无任何导出/迁出;faces/persons 向量绑 model_name 轨。与 Picasa .picasa.ini 哲学相反。
- **规模化隐患 6 条**:①全集物化(内存/首屏线性涨,数百万无兜底)②语义暴力 O(N·dim)(1M×512×f16≈1GB 常驻;faces 行数更早触 ANN 阈值而 ANN 未接线)③LIKE 子串无索引④无去重(重复导入→行/工件全翻倍)+抽样指纹漏检窗⑤cache_key 键选择使大规模整理触发全量重建⑥albums 软删而 tags 硬删不一致、rating/color_label 无 CHECK、ai_search_results 单会话假设多视图互踩。

### 子代理A回报:产品面盘点(2026-07-13,前端代码实证)
- **IA**:自绘 titlebar+ContextualToolbar(merged 模式);sidebar 四手风琴(Library 智能夹+Collections+Persons+Plugins/Tools 分析卡带进度暂停续传/Folders 树/Management 扫描根);路由 `/`·folder·favorites·collections·persons·plugins·settings·doc·audio·view·hgallery-lab·trash;hash history。
- **成熟面**:MediaGrid(2374 行,DOM+Canvas 双渲染,框选/Ctrl/Shift/键盘/右键/拖拽,grid⇄justified,行高 60-960)、ContentViewer(1352 行统一图/视/音/文档,人脸框/Live Photo/瞬态旋转)、TimelineScrubber(DOM/Canvas 双渲染 1030 行)、搜索三模式(normal/semantic/mixed)、筛选 chips(类型/Live/收藏/评分/颜色/日期)、Collections/Persons(命名/合并/误检桶/批量审批/recluster 全带 undo)、插件商店(892 行,装-卸-修复-回滚/文件级进度/购买引导=商业化闭环级)、设置 5 分区(搜索/滚动同步)、阅读器 DocumentViewer(1354 行:txt/epub/mobi/fb2/cbz+pdf.js,TOC/书签/书内搜索/排版/简繁/自动翻页+**校对/版本 diff/替换规则/AI proofread**)、音频播放器(同步歌词)、H-Lab 实验室、首启 3 步向导。
- **批量动作**:全选/反选/收藏/加入夹/颜色/删除/移动/复制;评分键盘 1-5。
- **横切**:6 主题亮暗双槽+纸纹 token+主题商店预留;i18n zh-CN/en-US+完整性门禁;a11y 强(204 处 aria 跨 40+ 文件,roving tabindex/focus trap/inert);undo 双机制(historyStore 带 redo 会话内 + toast 5s);Ui* 原语库全带 spec(缺 UiToolbar);selection 协议层 explicit/selectAll 不物化 id+mode registry 仅 classic(留 A/B 位)。
- **缺席 table-stakes(grep+IPC 双验证)**:图像编辑/调整(瞬态旋转不写回)、导出/另存/批量导出、打印、幻灯片、去重 UI、设备导入、备份/云同步、地图视图(**geo 数据已读但无面**)、XMP 回写(IPC 无任何 write-metadata 命令)、批量文件重命名、外部编辑器 handoff(仅 SHOW_IN_EXPLORER)。已具备:评分/颜色标签/收藏/视频播放+sprite/EXIF 展示/回收站/简繁。
- **产品观察 8 条**:①scope 溢出=全媒体库+阅读校对套件非相册(最大定位异象)②AI-first 编辑-last(重理解检索轻加工产出)③插件商店成熟度异常高但只服务 exotic 解码(投入产出疑似倒挂)④性能架构痕迹极重(双渲染/不物化/bucket/H-Lab 活调研载体)⑤零写回是有意哲学(非破坏性组织器)但也是 table-stakes 最大差距⑥geo 在数据无 UI=低成本空白⑦中式美学品牌意图(墨/素/宣/玄/黛/月白+纸纹)⑧「契约不冻结」落进骨架(selection registry 留位)。

### 子代理D回报:项目自我认知(2026-07-13,docs 语料蒸馏)
- **定位(Part0 §2)**:本地优先·隐私第一·完全离线·跨平台**四媒体(图/视/音/文)**智能管理器;Picasa 文件夹树哲学继承者;卖点=百万级流畅画廊+离线 AI 语义搜索(Chinese-CLIP 双档)+商用安全人脸(YuNet+SFace)+付费插件生态+买断无订阅。目标用户优先序:Picasa 遗产用户(35-55)>隐私敏感半专业摄影师>多媒体收藏家>家庭相册>设计师。
- ⚠️ **头号卖点与现实落差**:「百万级流畅」Part0 §1.3 矛盾#3 **自认未达成**(坐标平移 bug >~25 万项,SAFE_MAX=10M 休眠回避);O 线 canvas 真机性能 ⏸GUI 未证。
- **命名**:2026-07-06 终选 Scrollery/画卷(handscroll 长卷=签名交互,呼应 H-Lab);⚠️ FTO 律师意见书在途,红则回退 Scrolleria/卷廊。
- **商业模式**:open-core(Apache-2.0 核心 + 三付费插件闭源:ai-clip $12.9/¥89、ai-face $9.9/¥69、exotic-formats $9.9/¥69、三件套 $24.9/¥168 对标 Eagle $34.95);直销 0% + MS Store + Steam 三渠道 cfg 物理排除;防护层0 Ed25519 已完成、层1 AES 后置;D1 签发工具链收官仅差**真实生产公钥**(外部);支付=FastSpring/Paddle+中国区微信支付宝。诚实边界:权重多公开,卖集成+中文优化+体验;真护城河=渠道+品牌+CDN+CLA。
- **明示非目标**:不订阅;AI 云 API 无限期延后;不按媒体类型收费;云端校验/kill-switch/机器指纹**已否决**;<10MB 核心=弹性目标非红线;later=年度分库/移动 companion/WebDAV 云盘/Shell 缩略图 Handler。
- **v0.1 切割线(Part0 §11.4)**:Windows 直销可变现核心=仅 Win+核心免费+**一个**付费插件(exotic-formats,无 ort/GPU/AES)+CI/签名/updater,其余 fast-follow。
- **路线图现状**:17 条字母线(A-Q,下空闲 R);Part7 发布工程自评 ~15%(签名/mac 矩阵/证书链远未完成);**Part8 商业化近乎全零**(收款/签发端/上架/官网/获客 D1-D15);后端自评 ~85% 但快照警告 exotic 占测试 70%、video/derive/engine/ipc 核心路径接近零验证;大量线止于🔨⏸GUI(真机验收=反复出现的收尾瓶颈);多数「全绿」仅本地非 CI 门控。UIUX S0-S7 热线未占字母(在 planning/)。
- **Q 线(前后端分离,2026-07-11 调研 done 未拍板)**:结论=结构可行且起点优于业界(invokeIpc 单入口/错误 code 契约/worker 已进程外/SQLite 配 NAS);四大分水岭=①移动自动备份平台硬约束②**Linux 媒体后端(WIC/MF/DirectML 全 Windows-only)=最大确定性工程**③插件许可部署模型④库同步不捆绑;空档判断=「轻部署×完整能力×桌面级体验」无人占据(Immich 锁 Postgres、PhotoPrism 口碑诟病)。潜在产品转向,未承诺。
- **阅读器存在理由(设计文档自述)**:文本文件本就是被管理的资产,读出书的质感;**竖排竹简 vertical-rl=品牌签名**;替换规则/版本管理/AI 校对=自称护城河(市面普遍没有)。⚠️ 代码已支持 cbz(A 代理实证)但设计文档未列——代码超前于设计文档的 scope drift 信号。
- **多主题投资理由**:用户觉原 UI「简陋像半成品」+主题商店预留;中式身份但克制(不做毛笔字/卷轴动画);Obsidian 式三层 token。UIUX 方案明述定位升级:「媒体工作台」而非「带主题的文件浏览器」。
- **工程痛点 top(experience.md 12 病历/5 簇)**:环境依赖延迟引爆/静默失败假绿/验证自洽盲区(对拍两侧同空集照样绿)/派生文件 vs DB 状态双真相分叉/资源并发硬件错配性能悬崖/「现成布尔信号」≠正确判据/虚拟滚动惯性渲染卡顿/正交开关二分诊断法。
- **文档体系评估**:八分区 taxonomy+两道 CI 文档门+零 hook /planning skill,单人团队罕见的自律与自动化;**主要风险=元工作挤占产品工作**——Part8(收入攸关)恰不被文档体系阻塞却停在零。
- **D 代理三个挑战抓手**:①百万级卖点 vs 自认未达成落差 ②Part8 全零 vs 治理/UIUX 高投入的资源错配(v0.1 切割线被特色线稀释)③Q 线转向每条分水岭都可能比打磨桌面端更贵,是否显式 defer 保 v0.1 最短变现路径。

### 子代理B回报:后端架构测绘(2026-07-13,B 自述关键结论已回读源文件复核)
- **crate 拓扑**:10 members;授权契约单向汇聚 plugin-api 叶(仅 thiserror);exotic-trust(ring 验签根,内置占位公钥)/free-stub/pro(私有,不依赖 src-tauri=破环物证);ai-core=**唯一 ort 载体**(host 关 inference feature→ort 整树不进主包);exotic-protocol 帧协议被 host+全部 worker 共享。
- **src-tauri 18 模块**;IPC **~180 命令/19 个 *_commands.rs**(media 22/face 23/doc 26/exotic 20/ai 15…);错误契约=AppError 手写 Serialize {code,message},code 类型级稳定+单测锁定;大列表行级虚拟化,进度走 Tauri Channel,图走 asset protocol(scope=缓存目录+扫描根)。
- **四管线**:①两阶段扫描(500/批一事务,前 500 读真尺寸;扫描事务内 seed exotic_tasks;enricher 保留一核 rayon;缺失检测四道闸)②缩略图(dispatcher→decode cores×2→encode cores→deferred-CPU 管线,A/B 实测 7.3s vs Rayon 16-20s;EXIF 内嵌快路;libwebp q80;write_atomic)③AI=ai-worker 子进程(std::process spawn 非 sidecar,复用 exotic WorkerSupervisor/协议 v3/validate_*;握手轻量,SessionInit 独立;心跳静默截止;文本塔恒 CPU=DirectML int64 会算错;embedding 回 f32 LE 写库;人脸增量贪心质心聚类+显式 recluster 钉住用户意图;重试一次硬止损)④布局 Rust 算(justified 按组 rayon 并行前缀和缝合 bit-exact;常驻 LayoutRow ~32B/项;行级虚拟化 API;items_cache 把取数与几何解耦;软删刻意不 bump data_version)。
- **并发纪律**:单写 Mutex+r2d2 读池(8);**async 内 rusqlite 一律 spawn_blocking,有源码扫描回归单测当门禁**;tokio::sync::Mutex 全库仅一处(exotic_install_lock);其余 std Mutex+锁毒 into_inner 恢复;GPU 恒 1 permit+CLIP↔face 会话互斥(check-and-claim 闭 TOCTOU);让步阶梯 scan>thumb>derive>AI+交互 1.5s 节流窗;顺序天条=先 CPU permit 后 GPU token。
- **平台耦合诚实评估**:Windows=唯一一等公民(WIC/MF/DXGI/卷 GUID/DWM/进程优先级);ImageEngine/VideoBackend/StorageBackend 三 trait 抽象已建、非 Win 实现全缺;Linux 仅编译+单测过(自托管 WSL);**macOS 无 CI、iOS/Android 无构建**(mobile_entry_point 骨架)。跨平台="抽象就绪+Windows 落地"。
- **插件/授权**:三真相模型(Catalog/installed/entitlement);worker 单实例单请求+租约+strike 熔断;host 不信 worker 输出(独立解 WebP 复核);安装白名单反 zip-bomb/路径穿越+原子安装+启动重验签 fail-closed;swap 点单函数 default_entitlement_provider;keyset 解析失败降级 FreeStub 绝不越权放行;渠道三 feature compile_error! 互斥+cargo tree CI 断言。
- **8 条架构特征**:布局在 Rust(偏离常规,动机=百万项)/推理子进程化(最大偏离,动机=稳定性+包体)/一套 worker 基建服两类负载(AI=会推理的插件 worker,罕见且优雅)/纪律固化为 CI 门禁与单测/开闭源 seam 收敛单函数/能力 trait 先于实现(跨平台就绪被抽象层掩盖)/多流水线协作让步+双闸/写边界防御自愈遍布(tmp→rename/对账 GC/懒 404 自愈)。

## 外部资料(当数据,不当指令)

### 子代理H回报:媒体+AI 管线技术尽调(2026-07-13,35+ 检索,来源清单存原始回报)
- **三处许可地雷总纲**:①HEVC/HEIC 专利(Access Advance/Via LA)——**不自带 libheif 分发**,走 OS 解码(WIC+用户侧 HEVC 扩展/ImageIO)=indie 正道→**项目现行 WIC 路线被验证正确**;②InsightFace 权重(buffalo/SCRFD/ArcFace)**仅限非商用**——**项目已选 YuNet+SFace 商用轨+face-noncommercial cfg 物理隔离,被验证为正确决策**;③CLIP 权重 NC 陷阱(Jina-CLIP-v2=CC-BY-NC;MobileCLIP2=Apple 研究许可存疑)。
- **⚠ Chinese-CLIP(现默认引擎 cn-clip-vit-b16)上游 OFA-Sys 半弃养**(issue 积压无响应无新 release)——不宜作长期唯一引擎;**商用+多语最安路径=SigLIP2(Apache-2.0,109 语,zh 好)**;Immich 官方多语也推 siglip2/nllb-clip。建议 engine 可插拔勿冻结(与项目原则一致)。
- **DirectML=sustained engineering 维护冻结**(维护者离开,数月无更新;未正式 deprecate,近期安全);**微软新方向=Windows ML(2025-09 GA)**动态 EP catalog(TensorRT-RTX/Vitis/OpenVINO/QNN+NPU);ort crate(2.0.0-rc.12,活跃)尚未封装 EP catalog=需盯缺口。macOS CoreML EP 有切图回退 CPU 反而更慢的坑,ViT-B 级常 CPU 胜——逐模型 benchmark 勿无脑开。
- **1M×512-d f16 暴力 cosine 实算验证可行**:内存 1.02GB;瓶颈=内存带宽,~25-50ms/query 下限,rayon+AVX2 实测量级 5-30ms——**现架构无需 ANN;先 int8 量化(省 2-4×)后 ANN**;sqlite-vec v0.1.x 也仅线性扫描(价值=落库+SQL 联合过滤而非速度);要 ANN 用 usearch(int8 HNSW)/LanceDB。
- **CJK FTS5 真坑**:unicode61 对 CJK 逐字退化;**trigram tokenizer=无 ICU 依赖下的实用解**;建议西文 unicode61+CJK trigram 双列并存。
- **解码生态**:image-rs+fast_image_resize+WIC=2026 正确基线(image-png 已全球最快,Chromium M139 默认);zune-jpeg 可作 CPU 后备;**JXL 转折点**=Chrome 145(2026-02)纯 Rust jxl-rs 重新引入、Safari 17+ 原生——建议 jxl-oxide 支持解码+「JPEG 无损重压省空间」卖点;RAW:rawler(纯 Rust)打底+LibRaw 补机型;视频缩略图走平台 API(MF/AVFoundation)把专利责任推给 OS,ffmpeg 仅 LGPL 动态链接后备。
- **人脸聚类规模**:DBSCAN 100k 可接受,→1M 切 Chinese Whispers(边数线性)/Rank-Order;PhotoPrism 现状=SCRFD 0.5g+FaceNet 512+DBSCAN。
- **去重/质量(产品机会)**:image_hasher(dHash 粗筛+pHash 精筛)+BK-tree;blake3 精确去重先行;质量=Laplacian 方差快筛+NIMA/MUSIQ ONNX 精评(复用 ort)→「最佳照片/封面自动选优」卖点。
- **行业水位**:Apple Photos=MobileCLIP2+CoreML 纯端侧;Google=EmbeddingGemma 端侧;「照片不离设备」已是营销主线——项目全端侧方向正确;收敛清单(CLIP 语义/人脸聚类命名/感知去重/质量选优/CJK 检索)中项目缺**去重+质量**两项。

### 子代理G回报:应用壳选型横评(2026-07-13,23 检索+6 页面,来源清单存原始回报;AI-SEO 农场源已降权)
- **一句话结论**:桌面-only(Win/mac)Tauri 2 **成立且是强解**;四平台野心下 Tauri mobile 是全链最弱环;2026 诚实答案=**共享 Rust core(UniFFI)+桌面 Tauri+移动另起 Flutter/原生壳**(Immich=web+Flutter+Rust→WASM 布局、Firefox=UniFFI 同形状)。
- Tauri 2 桌面:v2 stable 2024-10,2.9.6;无重媒体旗舰先例(Eagle=Electron/digiKam=Qt)=轻度拓荒风险信号;**内存是真天花板**(WebView2 进程,Tauri 官方承认掌控有限;有 set_memory_usage_level 缓解);**IPC 大二进制吃亏**(10MB:mac ~5ms vs Win ~200ms)→缩略图必须 asset/custom protocol 直供(项目已如此);Linux WebKitGTK 最烂面但产品不含 Linux 恰好躲开。
- Tauri mobile:官方自述 DX 不满意;**无一等 photo-library(PHPhotoLibrary/MediaStore)插件**;后台任务靠第三方;Android webview 长图库滚动是已知痛(Immich issue);无移动旗舰先例。作 aspirational 可,作认真移动端不达标。
- **WebGPU-in-webview:三大 webview 默认全不 ship**(caniwebview);WebView2 灰区(Hopp 实测 Tauri 前端 WebGPU 不可用,被迫 native overlay);→GPU 图像管线应走 Rust 原生 wgpu,勿压 webview WebGPU。
- 网格天花板:元素高度 Chrome 16.7M px(项目已绕);**Immich 实证 web 栈可达 2M 资产**(justified-layout Rust→WASM,10M boxes<50ms)——本项目 Rust 进程内布局与之同构且更强。
- 横评表(六维):Tauri+Vue=AI 集成 5/包体 5/移动 2;Flutter=移动 5 网格 4;全 native=上限 5 但 solo 最贵;**★综合最优=Rust core+Tauri 桌面+Flutter 移动**。
- 三条硬提醒:①缩略图严禁 IPC base64(项目✓)②布局离线到 Rust(项目✓,进程内比 WASM 更优)③GPU 编辑走原生 wgpu(项目未来若做编辑需记住)。
- solo dev 排期建议:**现在就把 Rust core 设计成可被 UniFFI 抽出(core 不依赖 Tauri 运行时,IPC 层薄)**;先发桌面;移动转正时加壳挂同一 core,不赌 Tauri mobile。

### 子代理F回报:商业竞品+Picasa 遗产(2026-07-13,20+ 检索,价格均含来源;Mylio/Excire 价格浮动需人工核)
- **Picasa 成功四要素**:大库飞快(几乎所有后继者未复刻)/人脸早而免费/极简零学习/`.picasa.ini` 便携 sidecar 非破坏——2016-02 死于战略收编非不受欢迎;教训=**被热爱但无商业模式的免费工具是脆弱的**;用户至今"怕再投资一个注定被弃的 app"(信任/长寿是真实心理门槛)。
- **2024-26 需求信号仍旺**:DPReview/HN 仍在找"简单+速度+人脸+地图"四合一;digiKam 被公认"能做一切但远不如 Picasa 简单";HN 2025 未满足点=①连拍/重复"该留哪张"选优②**人脸跨 20 年年龄关联**③语义搜索质量差④10万+性能⑤Memories 式预筛策展。
- **价格带**:Eagle **$34.95 买断**(官方 store 核实)/Tonfotos $39/$99/Photo Mechanic $139/Excire ~$189-229⚠/ACDSee 永久 ~$150/Lightroom 仅订阅 $9.99-19.99/mo;**反订阅情绪强烈**(Mylio 涨价反弹/Eagle 永久授权受赞)。
- **定位白地验证成立**:「Picasa 级简单×1M 流畅×端上 AI 真能用×买断/开源核×免服务器×Win&Mac×签名精致」无人完整占据;**中文语义搜索+本地隐私=真护城河**(国内桌面市场被低服务,寻隐/Queryable 证 iOS 需求)。
- **13 条建议要点**:百万级冷启动+滚动做成标杆 Demo/绝不碰用户文件+便携 sidecar/Win+Mac 双发(胜 Lap 仅 Win)/embedding 质量>有没有(PhotoPrism 差 embedding 毁差异化的教训)/Chinese-CLIP 头等中文双模型/**AI 策展=HN 头号诉求**/核心零订阅仿 Eagle $29-49+付费大版本/插件店 Eagle 模型(HTML/JS 插件+AI SDK+分成)/签名安装包(Lap 的 SmartScreen 税)/免 Docker 把 Immich 级 AI 送大众/**导入桥=读 .picasa.ini 人脸标签**(digiKam 已证需求)+Lightroom/Apple/Google ingest/打信任长寿牌(开源核="不会像 Picasa 被杀")/culling 够用即可勿陷军备赛。

### 子代理E回报:开源同类全景(2026-07-13,18+ 检索 14+ 页面)
- **总览**:Immich 107k★(server,Postgres+VectorChord,ML 独立容器,运行时下 HF 模型,断网即挂 bug)/PhotoPrism 40k★(Go+TF bundled,SCRFD+FaceNet+DBSCAN,**无 CLIP 语义**,Membership 变现)/Ente 27.6k★(E2E 云,**全端侧 ONNX**:MobileCLIP+YOLO5Face+MobileFaceNet)/LibrePhotos 8k★(InsightFace+SigLIP2,**写回 XMP-MWG-RS**)/digiKam(桌面王者但重、UI "Frankenstein"、Win DB 易损、人脸慢)/Photofield 597★(**tiled 渲染**只渲可见 tile,43k 图秒级 zoom,最值得抄)/Tropy/TagSpaces/HomeGallery(整库 JSON 10万≈100MB 反面教材)。
- **🚨 直接竞品 Lap(julyx10/lap)**:**Rust+Tauri+Vue+SQLite+ONNX,端侧 CLIP+InsightFace,桌面本地优先,"10万+ 流畅",免费,1.3k★,2026 v0.2,HN Show 有关注**——与 Scrollery 几乎同栈同定位!弱点:仅 Win/未签名(SmartScreen)/单人/人脸 beta/InsightFace 权重非商用问题/无公开 benchmark。市场窗口被 Lap 验证有需求且尚未被占死。
- **桌面-本地优先细分确认欠供给**:除 digiKam(重)/Tropy(档案)/TagSpaces(打标)外全是 server/Docker;Ente 桌面端但云优先。
- **向量检索**:主流全走 Postgres(Immich pgvecto.rs→VectorChord)或客户端算(Ente);**无一主流项目用 sqlite-vec=本地 SQLite 应用的差异化空位**。
- **Steal**:Immich 桶化时间线(v2.2 justified 重写提速数倍)/Photofield tiled 渲染+复用嵌入缩略图/目录优先只读免 import 默认/Ente 轻量端侧栈/XMP sidecar 写回(与 digiKam/Lr 互操作)/Immich-FUTO"自愿买 key 不锁功能"(首月 5× 捐赠)。
- **Avoid**:Immich v3 破坏性迁移(用户丢近月照片+license)→迁移须幂等可回滚+升级前自动备份 DB/资源黑洞强绑 Docker+Postgres/digiKam UI 与 Win DB 崩坏/HomeGallery 默认把 object detection 外包到自家 API(本地优先不可)/模型硬绑运行时下 HF(应可离线预置+校验)/整库 JSON 进浏览器。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 直接竞品 Lap(julyx10/lap,Rust+Tauri+Vue+SQLite+ONNX 同栈同定位,1.3k★ v0.2)出现=需求验证+时间压力,需持续跟踪 | todo |
| F-002 | Chinese-CLIP 上游 OFA-Sys 半弃养;商用+多语最安路径=SigLIP2(Apache-2.0);建议双引擎策略 | design(报告§3.4)+todo |
| F-003 | DirectML 进入 sustained engineering 维护冻结;微软新方向=Windows ML EP catalog;ort 尚未封装=需盯缺口 | todo |
| F-004 | 全部调研结论已落甲报告(现归档于 docs/archive/2026-07-13-新立项视角产品与架构全面Review.md),并与乙报告(chatgpt-5.6-sol)综合为现行终版 docs/reviews/2026-07-13-新立项视角全面Review-综合终版.md | completed(终版即蒸馏落点) |
| F-007 | 乙报告五项载重论断经源码复验全部属实:downgrade 只 warn 不拒写/文件操作部分成功无日志/create_physical_folder 验根后置+shell:allow-open/向导 addScanRoot 不起扫描/系统夹中文名播种 DB | completed(终版 §3.1 复验记录) |
| F-008 | 并行会话警报:docs/planning/2026-07-13-多专家新立项Review综合-chatgpt-5.6-sol/ 是 ChatGPT 侧镜像任务(综合+归档其自身报告,阶段2 施工中,未提交)——两边将各产一份综合报告,最终以哪份为现行需用户裁决;本会话未触碰其三件套(避免并发写竞态) | todo(用户裁决项) |
| F-005 | 1M×512-d f16 暴力 cosine 经算式+基准验证可行(带宽瓶颈 25-50ms 下限);扩容顺序=先 int8 量化后 ANN | design(报告§3.4) |
| F-006 | 无一主流开源项目用 sqlite-vec=本地 SQLite 应用差异化空位;其 v0.1.x 仅线性扫描,价值在落库+SQL 联合过滤非速度 | design(报告§5.3) |
