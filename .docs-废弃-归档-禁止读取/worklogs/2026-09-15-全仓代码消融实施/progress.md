---
status: snapshot
type: working-memory
line: 全仓代码消融
created: 2026-09-15
---

# 进度

## 2026-09-15
- 已回读上一轮三件套/报告/状态并核实Git。产品起点仍e7db489b，未提交差异只有上一轮15份docs（3修改+12新增）。
- 新建实施三件套，用户裁决已写入。先提交分析与本轮施工起点，之后每个已验证批次中文commit。
- 尚未运行测试；代码尚未修改。

### 批1a 前端闲置实现
- P01–P04完成。主会话复核纯gate保留、阅读器仅注释/孤立类型变更、latestWrite改测可观察行为。
- 子代理实测：typecheck通过，163文件/1852例Vitest通过，相关12文件112例亦通过；未再重复运行。定向ESLint通过；BookReader未改模板有既有Prettier告警，未顺带修。
- 前端生产/类型/注释净减426 LOC，测试净减347 LOC，总净减773 LOC（含空行）。本批未做原生阅读器GUI实测，删除对象无生产调用，现行阅读链不改。

### 批1b Rust闲置实现与配置空维度
- P05/P09/P10/P11/P26完成，19个Rust文件合计新增30/删除542，净减512 LOC；其中vector_store整文件318行退出。
- cargo check --lib、check --tests offline locked、plugin-api check和定向rustfmt通过。定向运行75例通过（plugin-api2、config34、installer8、channel_stubs2、editing entitlement2、AI搜索27）；installer2例ignored，license过滤命中0例不算验签行为通过。未测原生keyring交互、未全量构建。
- 保留激活验签/凭据存储、fail-closed和安装完整性保护；配置只删无人使用Float与和hot反向重复的元数据，不改变重启事件字段。
- 规范文档同步正文。后续IPC由Godel独占注册表/常量，dedup由Popper独占查询闭包；资源代理转P15收窄当前连接管理；P18/P22滚动代理只改前端，后端bucket键后续统一。

### 批5 第三方资源
- P21完成：Vditor从532文件/22998343字节降至370文件/15136072字节，删除162文件/7862271字节（34.2%）。包含MathJax完整目录、开发类型声明、由包依赖提供的重复编辑器入口与method.js；保留method.min.js及实际动态资源。
- 浏览器资源fixture：27个URL/37请求全部200，无非预期404、加载失败、外部请求、页面或console错误。编辑器公式2/高亮1，打印同构preview公式2/高亮1/SVG3；MathJax无请求。主动访问5个已删路径返回404用于反证，不计运行失败。
- 未验证真实Tauri/Vite装配与工具栏打印端到端；不运行安装包构建。本批不计第一方LOC收益。
- 批1b代码commit：88f1d767；现行规范5文件由文档代理最后清理正文，另随文档批提交。

### 批3a 选择装配
- P19完成：删除动态选择模式注册表38行及专属测试72行，直接装配classicMode；SelectionMode/Intent/State和显式/全选非物化语义保留。
- 定向9文件97例Vitest通过（含Shift区间/全选/反选/拖动及真实消费组件）；旧注册符号引用归零。全量typecheck由滚动整合后统一，不重复。

### 规范同步
- 文档代理完成5文件第一轮同步，补充补丁两次因格式/上下文错误失败；到预算停止。主会话按已核实证据做少量定点整合：Part1 §3.4旧设计正文替换为当前检索入口，T8/T9/T11退役；Spec06失效测试入口与Part6/8当前激活签名/交付源描述同步。
- 历史数据与渠道的其他现行规范将在对应批次一并更新；不改归档审查快照。

### 后续结构基线
- 主会话已在Python SQLite临时内存库执行当前33段SQL，捕获33表/37显式索引/0触发器及字段默认值/外键；target/pruning-validation/schema-before.json仅作P23结构对照，不作运行时验收或产品兼容器。
- 后续代码量测量脚本target/pruning-validation/measure.py复用baseline-files.tsv既有分类，对新增文件单独列明分类；统计整个当前文件集合而非基线交集。
- 批1a 1be9aeb8、批1b 88f1d767、资源9242dcb8、选择77793647、规范cae36ea3均已本地提交，未push。

### 批4a 缩略图入口
- P20：产品generate_thumbnail/ThumbResultOrDeferred共36行删除，bench直接调用既有decode/encode/deferred入口增加25行，总净减11行；未新增产品适配层。
- 原产物完成时cargo check --benches offline locked通过，thumbnail相关48通过/1ignored、其中generator5通过，三文件rustfmt检查通过。之后其他IPC/去重在途修改导致全库check暂不可用，未将其当本批通过；整合后统一补一次bench检查。
- P25保留：两路径色彩投影与重采样不同（Bilinear vs解码引擎ShortEdge），共有编码/原子写已使用同一低层实现，再包一层无净收益。不扩大到ICC修复。

### 批2/3在途终审（22:35）
- 非去重IPC注册终稿261→232（减29）。Godel实测cargo check lib零error；layout cache13、cache/router/face/exotic/dedup82、layout/media148通过，4ignored；vue-tsc全仓通过。其后再发现search_commands只删注册未删模块，以及search DAO/trash keyset仅测试调用，已继续裁此确定闭包，不能声称已完整提交。
- 去重Popper实测DB16、IPC状态1、lens39通过（lens2ignored；交叉测试不加总为唯一例数）；主会话要求最后删除已无消费者DedupProtectionSummary整个folder模块，保留当前lens保护查询。
- P15已完成源码收窄，早期验收因在途Rust整合阻断，待统一补storage与netfs定向测试。主会话精算6文件新增171/删除532，净减361（最终以提交前diff为准），不采用代理自报矛盾数字。
- P16 Lovelace负责渠道源码/Spec13/Part6/Part8，exotic_commands已从Godel交接。P17要求thumbnail offering在Catalog唯一边界必须显式worker_id；OCR/Enhance无thumbnail可缺省。实际签名仍验原始bytes。
- P22 Ptolemy已删双引擎/前端bucket设置，Chrome cold/selectanim后测产物已落target；待完整报告与后端bucket字段移除。P12 H-Lab保留。
- P23 Ramanujan只准备DDL，尚未获源码写入通知；后续直接db/schema.rs单文件，删migration/schema三年代文件。schema_version=34仅格式标识，不支持已有不等版本；删scan_roots.backend_id与exotic_plugins.entitlement_source及纯透传字段，保留连接管理表与direct/free授权source_tag。
- P24由Godel接续，但需等当前批提交与P22交接uiStore后才写配置/旧theme/旧scroll兼容；DB schema代理不写config。

### 批6 前端单滚动路径
- P18/P22前端完成：删除useVirtualScroll及专属spec、GalleryLayoutSource透传层；统一bucket逻辑坐标/恢复/取行，删enabled开关、旧模板/映射分支和设置UI。DOM/Canvas及10M Canvas高度限制保持，H-Lab只改失效注释。
- 最终161文件/1809例Vitest通过，ESLint与vue-tsc通过。加入40M高度的35M远端恢复及总高缩水回非映射态语义测试。
- Chrome fixture对照cold144.1→142.9fps，selectanim143.7→144.1fps，coldCellFrames均0；只作功能/粗回归证据，不宣称提速。两轮均有已知harness无Tauri listener异常，不算原生UI全绿；WebView2/弱机未测。
- 后端bucket_segmented_scroll SettingDef与StartupConfig字段待P24同批删除（目前额外字段无前端消费者，不存在第二执行路径）。前端源码已冻结供P24交接uiStore旧theme段。

### 后端批2/3最终整合
- P06–P08/P13–P17代码闭包完成：29注册退出；旧搜索/回收站分页DAO及仅测试引用DTO退役；旧去重计划模块和专属731行开发基准整删。P15只保留连接测试函数与实际管理UI；P16只保留direct，移除全部channel feature/InstallSource/空桩，FreeStub移入实际license模块。
- P17 Catalog唯一边界拒绝thumbnail缺失/空worker_id，无thumbnail的独立服务允许缺省；Coordinator不再维护旧映射。原始bytes签名与keyring保护保留。
- 产品/工程配置合计63文件新增447/删除6621（净减6174；此数包含Cargo/CI配置，最终第一方LOC另按基线分账）。P13宿主源含嵌入测试净减3872，专属开发基准另减731，不冒充纯生产业务代码。
- 最终cargo check通过；Lovelace完整主库--lib 1308通过/9ignored；Godel queries232通过/1ignored；P15默认storage14、netfs WebDAV2通过；exotic192通过/2ignored；exotic-trust crate27通过；benches离线编译检查通过。交叉过滤不累加为唯一测试数。各Rust格式/最终diff已核，未做Tauri安装包、真机GPU/keyring/WebDAV服务器往返。
- 主会话额外删layout中明确为未来预留的unused pubuse（内部实现仍真实使用），不恢复测试专用重导出；测试从owner直接import。

### 最终批次接续点（4a542ab2之后）
- 当前源代码已提交至4a542ab2；按原基线口径工作区第一方255715 LOC/911文件，较264655/927净减8940 LOC/16文件（P23/P24写入前快照，不是最终结果）。
- P23实际实施已授权开始：Ramanujan 01a0a563-91e6-76a1-ab5c-09f9c981c3ae；70调用/30分钟。负责schema/backup/两死列及其Rust调用面，保留配置代理的文件；负责Spec01/Spec08数据备份章节/Part1。
- P24实际实施已授权开始：Godel 01a0a562-45b7-7a43-a93d-3173c05e7d42；45调用/20分钟。负责config目录、config/enhance IPC、主题/旧scroll、bucket后端残留及harness（含删needsMigration）。新configboot/load_or_init签名给P23，由后者同步lib/coordinator，避免竞写。
- 文档sidecar：Popper 01a0a556-edfc-7393-a810-264b60b5f4d0负责Spec10/15/02旧IPC与滚动描述；Leibniz 01a0a557-7413-7ce3-a2de-a2588d53c7dd负责Part7渠道；Lovelace 01a0a56a-41c2-7f81-99de-3d2aeb365c38负责Part8/Spec09/14/00及渠道滚动状态。全部只docs，无源码权限。
- Ptolemy前端源码/文档完成已关闭。主会话校正滚动报告为实际commit净减1390，删除未经全部真机证明的“功能超集”宣称，保留具体机制与验证边界。
- 最后尚需：P23/P24整合行为验证、changed Rust clippy/格式与前端定向检查、当前DDL结构对拍结果、同口径量化与完整实施报告、规范/rolling状态更新、三件套归档、中文commit。不得push/发布，不安装依赖/全量Tauri构建。

### 配置真实消费复核
- Godel发现17个doc_*阅读器偏好键实际依赖未知DB路由：useReaderTypography与DocumentViewer持续读写；若直接拒未知键会破坏当前偏好保存。
- 主会话裁定纳入既有STATE_KEYS，语义为应用状态及UI记忆偏好（与layout_mode/group_by/pinned_settings一致），继续DB单源；不扩TOML，不建第三清单。声明状态键19→36，实际持久键集合从79+19+17隐式=115变为77+36=113，不能误报新增17功能或宣称声明项全减少。
- get/set均拒绝两清单外未知键，代理继续完成路由与阅读器测试。配置原始字符串与TOML Item是两个真实外部边界，强行统一需额外装箱，保留各自解析。
- Part0/7文档代理因补丁锚点反复不符停止，已关闭；Popper接手限定旧渠道/enc_seed正文收尾。Lovelace同步Spec13仍残留的中间态与Part8定点描述；所有docs仍由主会话核对实际diff。

### 最终验证启动（23:15）
- Godel完成P24：设置77/状态及UI偏好36，前端46个实际配置键均在清单内，get/set共用私有ensure_state_key；没有公共KeyRoute包装。实测config过滤37通过（主会话后删1个重复自证测试，最终整库测覆盖）；reader/stores/themes 27文件344例Vitest通过，vue-tsc通过。
- 原npm run lint被9/12–9/14既有未跟踪.research-tmp里的第三方包触发4332错误（15文件），本批修改文件单独lint零错误。主会话确认目录早于任务，保留其内容；只追加命令行排除此Git已忽略目录再验，不修改ESLint配置。
- Lovelace接最终主库lib测试/clippy及本轮改过Rust文件的格式检查，日志写target/pruning-validation/final-*.log；不跑全量安装包或安装依赖。
- P23规范写权从Ramanujan移交Meitner（01a0a5a2-df88-76b2-a709-75efaa35cbac，commandcode/high），仅Spec01/08/Part1。Ramanujan继续源码闭包与实际DDL结构对拍、backup前端字段退役。Popper/Leibniz docs任务均已关闭；主会话补齐其遗留的几处当前渠道字段/任务说明。

### 最终源码验证与提交前终审
- P23完成单schema.rs：格式34、私有完整DDL、新库单事务、当前格式幂等、其余稳定错误拒绝；不做数据迁移。删scan_roots.backend_id与exotic_plugins.entitlement_source全链、旧migration与年代schema文件；备份仅当前格式，删needsMigration/UI提示/孤立CSS及中英文词条；旧错误码映射同步到restore_schema_incompatible。
- 直接从实际schema.rs建临时库对拍：33表/37显式索引，除两列及backend_id外键的计划删除外，列/默认值/PK/FK均一致，quick/fk检查正常。主会话再核37条索引完整token序列（含UNIQUE、列序、表达式、WHERE），全部一致；不是仅比对象名字。
- 最终主crate lib首次1284通过/9失败/9ignored；原因全为旧迁移函数顺带给裸测试连接注册排序规则。仅修selection_resolve与enricher测试连接，生产仍由connection统一注册。复跑1293通过/0失败/9ignored；clippy exit0且无源码warning，唯一build-script提示为调试信任根加载信息。
- Rustfmt对99个已改现存Rust文件首检，仅config_commands两处格式不符；主会话修后与新增schema.rs一并check通过。之后仅去掉roots测试模块重复cfg(test)一行，无行为变化。
- 前端终检vue-tsc exit0，backupStore6例通过；主会话核新增错误映射并补backupPresentation3例及这两文件ESLint通过。27文件344例reader/stores/themes与滚动阶段161文件1809例为分批证据，不重复累加唯一例数。
- 排除既有Git忽略.research-tmp的全仓ESLint exit0/558文件零错误；原不排除命令4332错误保留原始边界说明。没有删除该目录或改lint配置。
- Ramanujan/Lovelace源码与验证完成已关闭。只剩Godel终检回报、Meitner三份规范，主会话负责最终报告/同口径指标/滚动状态/归档。源码已冻结，未push。

### 收口
- 源码收口31cab793：同基线口径254057 LOC/906文件，净减10598/21；Rust模块声明531→515、trait20→17、struct590→532、enum137→129、TS interface359→351；根级32模块保持。IPC261→232，当前232前端常量与注册集合完全一致。
- final-metrics/final-surface/schema-verification三份JSON与实施报告已落reviews原任务目录；原baseline保持冻结。第三方净减162文件/7862271字节单列，不计入第一方LOC。
- Godel终检vue-tsc、备份store6例、局部lint通过；主会话补错误呈现3例及实际backupMessages词条文件lint通过。所有源码无未提交变更；最后规范与归档仅docs。
- Meitner完成Spec01/08/Part1现行正文，旧增量DDL代码块、后台字段接线与迁移任务指令删除。Spec08明确只有校验阶段只读，后续受约束的路径rebase仍写暂存库，不夸大“消除所有不可信DB风险”。历史取证行号不冒充精确现行定位。
- 候选F-101与D-101/102/103已逐项处置至closeout；计划迁worklogs，README/todo/status同步同一文档提交。所有子代理已关闭，不push。

## 回顾
- 有效做法：先追完整消费者闭包再删IPC；把第三方运行资源做动态加载验证；当前DDL核列/FK并单独对索引完整定义；源码与文档均更新单一事实来源。
- 教训：关键词或测试引用数量不能判死代码，GPU令牌/图像缓存色彩差异/17个阅读器偏好均需要真实消费者复核。大段补丁必须以当前边界定位，失败后重读真实内容，不沿过期行号盲改。
- 验证边界：库测与浏览器fixture不代表原生包、真机GPU/keyring/WebDAV或跨平台GUI通过；默认lint的旧临时目录问题单列，不用改配置掩盖。
