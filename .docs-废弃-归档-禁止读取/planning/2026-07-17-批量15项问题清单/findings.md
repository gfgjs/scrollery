---
status: active
type: working-memory
line: 批量15项问题清单
created: 2026-07-17
---

# 发现与决策:批量15项问题清单

## 需求
- 用户攒 15 项问题/需求,要求按易到难规划,优先无人值守多解决多施工;需决策的留档跳过等用户回来。
- 完整清单见 task_plan.md「15 项清单」表。

## 发现

### #1 分组无缝模式(侦察:布局线)
- 现有 `groupBy:'none'` 只是视觉无分隔,但 SQL 走全局排序丢组聚合(src-tauri/src/db/queries/layout.rs:635-646)。用户要的=排序仍按组、打包按 none。
- 排序(push_order_by/derive_order)与打包分段(layout/justified.rs:293-349 layout_groups_parallel)可解耦:`LayoutParams` 加 `seamless: bool`,打包走单段+跳 emit_separator(justified.rs:512-516 组首分隔符、:541-552 组末强制断行)。
- 渲染层零改动(DOM MediaGridRow/Canvas drawSeparator 都是 rowType 数据驱动)。副作用:date 分组 monthBuckets 空 → 时间轴 scrubber 失据(MediaGrid.vue:556-561),需单独产月桶。
- 前端:uiStore 加开关 + GalleryViewControls 加 toggle + useJustifiedLayout.ts:119-130 透传。改动面:小-中。

### #6 扫描期等高布局重排(侦察:布局线)
- 双 reflow 源:① scanStore.ts:131-137 扫描期每 1000ms loadStats → totalItems 变 → MediaGrid.vue:1837-1845 全量 compute;② 富化 MEDIA_ENRICHED 每 2s 防抖回填真实宽高比 → 等高行几何全变。
- compute_layout 恒全量(ipc/layout_commands.rs:157-481);扫描期 data_version 每批 bump → 去重闸必不命中。
- 宫格 cell 不吃宽高比(justified.rs:571-577)→ 对两源均免疫。
- **推荐方案(a)**:扫描期自动切宫格。`scanStore.isAnyScanRunning`(scanStore.ts:36)现无人消费,接进 MediaGrid/useJustifiedLayout 算 effectiveLayoutMode,扫毕翻回时终算一次等高。改动小-中。
- 方案(b) 增量等高布局:不成立——排序按 sort_datetime DESC,新文件散插非尾部追加;全快照架构(items_cache+前缀和+bucket 段表)需大改。弃。

### #10 EXIF 解析慢(侦察:EXIF 线)
- 库=kamadak-exif 0.5;富化循环 scanner/enricher.rs:117-405,批=500(ENRICHMENT_BATCH,enricher.rs:33),已 par_iter N-1 线程池——"500张/1-2s"=一批耗时,I/O-bound 非 CPU。
- **瓶颈#1(成立)**:pass 内同一 JPEG 开 2-3 次文件——EXIF(metadata.rs:135)+ XMP Motion Photo 死读 128KB(metadata.rs:254-257)+ 尺寸再开(metadata.rs:90,占位项几乎全走)。预期 2-3× 提速,改动中。
- **瓶颈#2**:XMP 128KB 固定读过量(标记通常在前几 KB APP1)。改动小。
- **瓶颈#3(残留)**:逐项 get_item_path_info ×500 串行 JOIN 且持写锁(enricher.rs:200-213);批选 SQL 每批 ORDER BY d.tree_sort_key 无索引 filesort,500k 库约 1000 批近 O(N²)。批选 SELECT 直接带 r.path/d.rel_path/m.file_name 可删逐项查询,改动小-中。
- 不成立:每文件事务(已 500/批)、单线程(已并行)、解析 CPU(kamadak 极低)。
- 路线优先级:单次头读+内存复用(open 3→1)→ 批选带路径 → XMP 收敛 → 批选索引/keyset 分页。
- 哈希不读文件(utils/hash.rs:29-40 纯字符串 cache_key)——与 #9 去重评估相关:现无内容哈希。

### #4 查看器侧栏开关太显眼(侦察:UI 线)
- 目标钮=AppShell.vue:39-53 浮动 `.viewer-sidebar-toggle`(样式 :238-263),已用主题 token,但 34×34+82% 底+边框+投影在黑台上突兀。真沉浸态(viewer.isImmersive)下此钮已隐藏,只剩 ContentViewer.vue:1094-1121 `.content-viewer__immersive-exit`(硬编码深色玻璃,不随主题)。
- 修法:纯 CSS 克制化(降透明度/去边框投影/hover 显形)+ 顺带把 immersive-exit 硬编码色换 token。改动小。

### #5 拖拽手柄可选显隐(侦察:UI 线)
- DOM:MediaThumb.vue:34-46(v-if=isSelected);Canvas:MediaGridCanvas.vue:1003 drawHandle + :1454-1482,:1636 hitHandleAt(**隐藏时两处都要 guard,否则不可见仍可命中**)。
- 持久化范式:uiStore ref+setter → IPC.SET_APP_CONFIG;启动 StartupConfig 批量读回(前端 uiStore.ts:50-75,:420-488 + 后端 config_commands.rs 需同步加键)。改动中(文件多每处小)。

### #8 隐藏边栏时设置入口(侦察:UI 线)
- 唯一入口=SidebarFooter.vue:4-11 → router.push('/settings')。侧栏隐藏(uiStore gallerySidebarVisible:189)即不可达。
- AppToolbar 仅画廊路由挂载;**AppStatusBar 恒在**(statusbar__right :129-135)——加钮最稳位。或 toolbar__right + v-if 侧栏隐藏才显。改动小。

### #14 「文件夹」被挤剩「文」(侦察:UI 线)
- 根因:AccordionSection 头部 flex,标题 flex:1 min-width:0 可缩,FoldersSection.vue:5-34 actions 区 5 个控件(「全部」文字胶囊+4 图标钮)不收缩 → 窄侧栏标题先 ellipsis。仅 FoldersSection 有 #actions。
- 修法:标题 min-width 保底/图标间距收紧=小;低频操作收「⋯」菜单或「全部」改图标=中。

### #13 文档缩略图叠加文件名/标题/章节(侦察:UI 线)
- 三条渲染路:pdf/svg=前端 DocThumbRenderer 离屏烘焙(已是可见性门控后台泵);epub=后端 derive/doc.rs:30-117 取封面(未取 dc:title/TOC);txt/md/office=CSS 文本卡(MediaThumb.vue:47-60,无文字)。
- 元数据现状:document_meta 表无 title/chapters 列;txt 章节已有提取(text_index.rs build_index)但只在开卷按需(doc_commands.rs:501-524);pdf 标题/大纲可经 pdf.js getMetadata/getOutline(未取)。
- 全量方案=大(后端提取+schema 扩列+IPC+DOM/Canvas 双路叠加);**仅叠文件名=小-中一期**(数据已有,复用 showThumbInfo 范式)。

### #3 退出大图后画廊键盘失灵(侦察:键盘线)
- 根因:方向键滚动=浏览器对**聚焦滚动容器**的原生行为(.media-grid tabindex=0,MediaGrid.vue:12-25;onGridKeydown 只盖 1:1 印记不滚动)。关查看器 router.back → ContentViewer 卸载 → 焦点重置 body;onActivated(MediaGrid.vue:1765-1785)重挂 document 监听(故 ESC/评分活)但**不 focus 回 gridRef** → 方向键死。
- 修法:onActivated 补 `gridRef.value?.focus()`(守卫:焦点在输入框/查看器时不抢)。改动小。
- 教训:KeepAlive 全局监听挂 activated/deactivated 只覆盖 document 级键,覆盖不到元素焦点依赖的原生滚动——盲区。

### #12 Ctrl+W/ESC 退出/隐藏(侦察:键盘线)
- 后端零改动:hide_window/exit_app 已有(system_commands.rs:173-187,registry 已注册);closeBehavior('ask'|'minimize_to_tray'|'exit',uiStore:330-333)+ window-close-requested 分派(App.vue:322-330)完备。
- Ctrl+W:现无绑定;AppShell.onKeyDown 被 `!viewer.activeViewer` 挡,需独立全局处理;行为复用 closeBehavior 分派。WebView2 无默认 Ctrl+W,冲突低。
- ESC:已被 5 处强消费(FullscreenExitGuard 捕获阶段/ContentViewer stopImmediatePropagation/backBar/清选区/UiDialog)。做「隐藏程序」必须最后兜底,镜像 shouldDeferEsc 判据。**ESC 语义激进,建议默认只做 Ctrl+W,ESC 兜底做成可选开关或留决策**。改动中。
- capabilities 够用(走自定义 IPC 非 core:window ACL)。

### #15 Canvas 按住方向键周期卡顿(侦察:键盘线)
- 主因:bucket 分段边界周期重活。每滚过 clampSegmentPx(rowHeight×20,clamp 1000-4000px,useBucketVirtualScroll.ts:104-107)→ syncDesired 建 **reactive** 段+异步 GET_BUCKET_ROWS;批行落地 → Vue 全量代理化 + MediaGridCanvas.vue:1681-1688 `watch(props.rows,{deep:true})` 全窗遍历,同步开销集中边界帧;叠加预取计划重建+新行 getImage 突发。间隔=段高,恰合「每隔一段距离」。
- 深 watch 不能直接删:承接就地乐观 patch(收藏/评分/色标,:1678-1680 注释)。
- 修法候选:① 批行 shallowReactive/markRaw 降代理化+独立 patch 信号替深 watch;② 边界工作拆帧;③ 段预取更超前。正确性风险中等,改动中。
- 次因:闸门滞回翻转重绘、showThumbInfo 时 viewportMeta 批 IPC。

### #2 增量生成缩略图(侦察:后端线)
- 「全量」慢在前置整表 reset:thumbnail_commands.rs:490-495 `UPDATE ... SET thumb_status=0, thumb_path=NULL` 后全部重派发;生成器本已缓存命中短路(generator.rs:215-232),dispatcher 只取 thumb_status=0。
- 增量=复制 start_full_thumbnail_generation 去掉 reset;可选前置 stat 兜底(queries/thumbnail.rs:291-333 已有)。新 IPC+注册+按钮+store+i18n,零 schema。改动小-中。

### #7 文件夹迁移识别(侦察:后端线)
- **关键杠杆:cache_key=xxh3(rel_path/file_name|mtime)(utils/hash.rs:29-40),不含盘符/绝对路径** → 整根迁移后缩略图/AI/人脸产物文件名全部不变,可原样复用。
- 最小可用「手动重链接根路径」=中:① UPDATE scan_roots.path ② volumes 重绑(volume_probe 已能取卷 GUID,同卷换盘符本就复认) ③ 抽样 size+mtime 校验(复用 upsert Unchanged 判据 scan.rs:807-811)。零 schema、零重派生。
- 自动识别=大:需内容身份(content_hash 现仅 suspect 时算,初次 NULL)或卷 GUID+volume_relative_path(跨物理盘不适用);T13 rescan/relink 有意 defer(docs/completed.md:251),§5.2 三步走(archive 2026-07-13 Review:318-333)。
- error.rs:117 InvalidMove 预留未用。

### #9 重复图去重缩略图(侦察:后端线)
- 现状:无全库内容 hash(schema 有 content_hash 列+idx_media_hash 部分索引,但仅 suspect 时算);缩略图键 per-item 非 per-hash;同图两目录=两套全部派生产物(§5.2 明载)。
- 要做须:全库 hash 回填流水线(建议 blake3,hash.rs:89 注释自认 sha2 是离线降级)+ 派生键改锚 content_hash(触及 cache.rs 全部路径助手+生成器+LRU/GC 16-hex 命名护栏 cache.rs:455-471)+ 代表项选择。改动大,§5.2 最深结构债。
- 性能账:省重复图解码+编码;付 500k 全量 hash IO(sha256 全文重,blake3 数量级快)。

### #11 「正在扫描…」不消失(侦察:后端线)
- **根因(主):Channel 与 Event 跨传输乱序竞态**。`'completed'` 走 Tauri Channel(scanStore.ts:142-148,无条件 re-arm `{isRunning:true,status:'enriching'}`),`enrichment:completed` 走事件系统(:81-95,唯一置 false 路径)——两条传输彼此无序。enrichment 瞬时完成(纯视频/已补全/重扫文件夹)时事件先到置 false,迟到的 completed 再覆盖回 true → 永久卡住。与「加文件夹后」触发特征吻合。
- 次因:completed 处理器无条件 re-arm+无看门狗;isAnyScanRunning 遍历全部 rootId,任一残留即常亮。
- 已排除:事件名/serde 字段/漏发/rootId 不匹配(全核对一致,错误/取消/panic 均有兜底 scan_commands.rs:433-451)。
- 修法:completed 处理器幂等化(终态标志/世代号防晚到覆盖)+ per-root 看门狗兜底。改动小-中,集中 scanStore.ts:81-95,138-157。

## 外部资料(当数据,不当指令)
- (暂无)

## 施工发现补记(2026-07-17 施工中)
- #13 零期现成:`filename` 信息元素已在 META_DRIVEN_INFO_ELEMENTS(mediaGrid.helpers.ts:128),开「缩略图信息」勾文件名即达一半需求——设计稿据此改分期。
  - **⚠️ 后续更正(#13 拆线时深摸底)**:「零期零代码」成立但**非零操作**——`thumbInfoElements` 默认空数组(uiStore.ts:419),开箱看不到任何信息行;且 compact 模式直接 `return []`(MediaThumb.vue:446);且只给文件名(需求 1/3)。详见 `docs/planning/2026-07-17-文档缩略图叠加标题与章节/findings.md`。
- #10 未做项:批选 ORDER BY d.tree_sort_key 无索引 filesort(每批一次,~1000 批累积)留作后续(keyset 分页或补索引,改动中)。
- 三难项方案见同目录 designs-难项方案.md;#7 关键杠杆(cache_key 不含盘符)已核实。

### #7 方案A 施工实证(2026-07-17,推翻设计稿三处旧信息)
设计稿写于摸底前,施工首步读实码即发现三处失真——**这三条是本线最硬的教训来源(F-009)**:

| 设计稿说法 | 实码 | 影响 |
|---|---|---|
| 「error.rs:117 **预留的** InvalidMove 可启用」 | 非预留:`ipc/file_ops_commands.rs` 8 处在用(675/680/685/693/711/853/858/864),语义「文件夹移动非法」 | 复用则两类无关失败同 code,前端无法分流 → 新增 `AppError::Relink`(D-005) |
| 「复用 upsert Unchanged 判据(scan.rs:807-811),按 size+file_mtime 比对」 | 判据实为 **scan.rs:793,只比 `file_mtime`,size 不参与**(size 只在 mtime 已变后区分 SuspectChanged/SourceChanged);807-811 是 Unchanged 分支内的 UPDATE 参数行 | 不复用(吃 `&Connection`、要 directories 行先在、Unchanged 分支仍写库),自写纯比较器且**刻意保留 size+mtime 双比**(D-006) |
| (未提)`scan_roots.path` 有 UNIQUE;无 `update_scan_root_path` DAO | 确有 UNIQUE(schema.rs:44);DAO 缺失 | 新增 DAO + 命令层先查重给可读错误,否则撞出裸 `AppError::Db` |

另两条落地约束(设计稿未覆盖):
- **后端自触发不了兜底重扫**:`start_scan` 要 `Channel<ScanChannelPayload>`,只有前端造得出 → 前端补发。
- **新根须补 asset scope 授权**(旧路径授权对新路径无效,漏则迁移后图片经 convertFileSrc 全挂)+
  **卷绑定须重探**(D→C 换物理卷,不重绑则旧卷离线时缺失检测把整批本地文件误判 missing,C5 Piece1)。

抽样策略取 `ORDER BY RANDOM()` 而非 `ORDER BY id LIMIT n`:后者只取到最先扫到的目录,抽样集中一角,
校验不出「新路径碰巧有个同名子目录」这类假阳性。代价是 50 万行一次扫描+临时排序(百毫秒~秒级),
relink 是用户显式一次性动作,正确性优先。

### 顺带发现(非 15 项):cargo test 全量在 HEAD 基线即 3 红
- 现象:`exotic::fingerprint` / `sink` / `pipeline` 三测试 panic 于 `cache.rs:20 "Thumbnail size 480 is not a valid tier"`。
- 定责已核实(stash 验基线 + `git log -L 30,30:generator.rs`):b554aa5「档位重定」把 THUMB_TIERS 从
  `[120,240,480,960]` 改成 `[64,128,256,512,1024]`,**生产侧引用点全跟上了、测试里的裸 480 没有**。
  是**测试陈旧,非生产回归**。已修(76ad9ea,改绑事实源,D-007)。
- **CI 会挡门**:`.github/workflows/ci.yml:70` 跑 `cargo test --workspace` —— 只是 CI 因主机硬件不稳被冻结,
  故这红一直没露(见 F-007)。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 缩略图 cache_key=xxh3(rel_path/file_name\|mtime) 不含盘符/绝对路径 → 整根迁移全部派生产物可存活,relink 只需改 scan_roots.path+volumes 重绑 | designs(并入 #7 立项档)/experience |
| F-002 | Tauri Channel 与事件系统是两条无序传输:终态事件可被晚到的 Channel 中间态覆盖复活——前端状态机对跨传输消息须幂等(终态账本/世代号),纯事件序推理不可靠 | experience |
| F-003 | KeepAlive 下把 document 级监听换挂 activated/deactivated 只救 document 键;方向键/PageUp/Down 是浏览器对「聚焦滚动容器」的原生行为,查看器关闭焦点掉 body 即失聪——激活时须显式收回焦点(无主才抢) | experience |
| F-004 | Vue deep watch 的代价在「窗口整体遍历+响应式代理物化」且集中在数据落地那一帧:大挂载窗口(canvas/虚拟滚动)下改浅 watch+显式 patch 信号,把深响应式换成两条窄通路 | experience |
| F-005 | Windows 下 File::open 伴随 Defender 扫描,同文件多次 open 是隐形 I/O 大头;元数据类 pass 用「单次 open 读头缓冲+多消费者复用+截断回退」范式(metadata.rs HeaderBuf) | experience |
| F-006 | 「全量生成缩略图」慢的真因是前置整表 reset,生成器本有缓存命中短路——增量=同流水线去 reset,一个布尔分叉 | no-promotion 候选(commit 信息已载) |
| F-007 | **过滤子集测试是盲区**:长期只跑 `cargo test --lib <mod>::` 精准过滤,输出全绿给足心理安全感,而被过滤掉的部分可以红很久无人知——本次 3 红藏在 480 个被过滤的测试里。CI 本是第二道防线(`cargo test --workspace`)却因主机不稳被冻结,**两道防线失效窗口重叠**。推论:冻结 CI 期间,本地至少要周期性跑一次全量,否则等于零门 | experience(§ 验证纪律) |
| F-008 | **改「唯一事实源」常量时,须连测试里的硬编码副本一起搜**:b554aa5 改 THUMB_TIERS 梯,生产侧引用点全跟上了(cache.rs/snap_to_tier/file_ops),唯独测试里的裸字面量 480 没有引用关系、grep 常量名也搜不到 → 静默失配。测试断言里出现事实源的**值**而非**引用**,就是一颗定时雷 | experience(§ 事实源纪律) |
| F-009 | 设计稿写于摸底前 = 半成品:本线 #7 设计稿三条关键说法(InvalidMove 预留 / 复用 Unchanged 判据 / 判据比 size+mtime)施工时全被实码推翻。**设计稿的行号与"预留/可复用"类断言必须在施工首步复核**,不能直接当施工依据 | experience(§ 文档纪律) |
