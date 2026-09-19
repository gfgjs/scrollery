---
id: 2026-07-02-horizontal-gallery-lab
status: active
type: experiment
line: H-Lab 横向画廊实验(D2)
created: 2026-07-02
---

# 横向滚动画廊实验室(H-Lab)——多模式布局对比的解耦架构

> 快照日期:2026-07-02。本文是「横向画廊」特色功能的**实验阶段**设计:目标不是定稿某一种布局,而是把多种候选布局以最低耦合成本做进同一个实验视图,供真人体验调研后再裁决(承接记忆「开发期不冻结契约——未经实测比较不定死」)。
> 范围:仅布局算法 + 横向滚动 UI。多选/收藏/评分/日期分隔符/时间轴 scrubber/坐标压缩等附加能力**全部显式推迟**(见 §6)。

## 1. 背景与候选模式

纵向画廊已有 justified(等高行)与 grid(均匀宫格)两种生产模式。横向画廊的三个候选:

| 模式 | 几何 | 已知风险(待实测证实/证伪) |
|---|---|---|
| A `paged` 分屏 justified | 页宽 = 视口宽 × factor(默认 1.2,故意露出不完整页驱动滚动),页内跑等高行装配,行数以页高恰纳为准 | 相邻页行高错位(结构性);阅读折返幅度大 |
| B `lanes` 等高泳道 | k 条固定等高泳道;**列主序**填充(item i → 泳道 i mod k,先上到下、再左到右),行内图片宽度随宽高比浮动 | 泳道游标漂移(长库下相邻时序项横向错开);「行极长」的心理感受 |
| C `columns` 转置 justified | 列宽浮动、列内同宽异高、列高恰满视口 | 相邻列的水平边界互不对齐,可能破坏一致性观感(用户已提出此质疑) |

三者共同点:输入都是按时间序的 `LayoutItem` 流,输出都是**沿 x 主轴单调排布的几何块**。这是解耦的支点。

## 2. 解耦裁决(本文的 normative 核心)

四道接缝,依次隔离「算法 ↔ 投递 ↔ 滚动 ↔ 渲染」:

1. **算法接缝**:每种模式是一个纯函数 `(items, HLayoutParams) → Vec<HBlock>`,模式即数据(`HLayoutMode` enum)。新增/删除一种候选模式 = 增删一个纯函数分支,不触碰任何其它层。
2. **投递接缝**:`HBlock { x, width, items[] }` 是唯一跨 IPC 契约;块仅是「取数分组 + 虚拟化单元」,`HItem` 坐标一律**全局绝对坐标**(x/y/w/h),渲染层不感知模式差异(甚至不感知块结构,直接 flatMap items)。独立缓存 `h_layout_cache`(AppState 新字段)+ 独立版本号,与生产 `layout_cache` 互不可见。
3. **滚动接缝**:新写 `useHVirtualScroll`(x 轴,含滚轮转译 deltaY→scrollLeft),**不改生产 `useVirtualScroll`**。刻意不移植坐标压缩(SAFE_MAX 平移模式):实验库规模(≲30 万项,逻辑宽 ≲1000 万 px)用不到;总宽超过 1000 万 px 时 UI 显式横幅告警「超出实验滚动上限」。某模式毕业转正时,再做「生产滚动器轴泛化 + 平移模式移植」的合并(届时 R2-5 的坐标数学测试参数化跑两轴)。
   **滚动性能基线(2026-07-02 确立,六轮手测收敛)**:① 滚轮转译动画 = **帧积分式定时长线性重定标**(`createWheelAnimator`,`useHVirtualScroll.ts` 模块级工厂,raf/时钟可注入):输入只更新「累积目标 + 截止线(末次输入 + 160ms)」,位移在帧循环里按帧间隔积分 `Δx = 剩余距离 × 帧dt / 剩余时间`——无输入时逐帧衔接即恒速线性,输入频率高于帧率时速度不塌缩(见墓志六轮)。选型论证:手测确立四条体验约束——不阶跃(一轮)/慢滚不脉动(二轮)/连滚不丢量(四轮)/启动即跟手(五轮);原生程序化平滑滚动(scrollBy/scrollTo smooth)是 ease-in-out 曲线、每次重定标从零速起步、且浏览器不提供缓动控制 API,结构性无法满足「启动即跟手」,自研动画不可避免;线性段 = 输入即满速(ease-out 级跟手)而无其速度尖峰(慢滚不脉动),是唯一同时满足四条的曲线。方向安全三道**构造性**约束:帧 dt 钳 0(时间戳乱序时原地 no-op 而非反向外插)/目标仅在本次滚动方向前方时才累积、否则从当前位置重算(每帧写值恒在 [当前位置, 目标] 区间内,反向输入与外源位移自动重基)/定时长必然终止(末次输入后 160ms,不与滚动条拖拽等外源滚动持续对抗)。动画器数学已入 vitest 确定性锁测(`useHVirtualScroll.spec.ts`:线性/钳制/单调/累积/高频不塌缩/重基/边界/终止),观感(跟手/眩晕)仍属手测。
   🔴 **已推翻方案墓志(附推翻原因,禁止无新证据回潮)**:一轮·裸 `scrollLeft += dy`(阶跃观感即掉帧)→ 二轮·指数趋近 25%/帧(输入瞬间速度尖峰,逐格慢滚成加减速脉动致眩晕)→ 三轮·线性重定标**未钳 t 实现**(rAF 帧时间戳可早于输入侧时钟,t<0 反向外插致方向乱跳;⚠ 当时把模型连同实现整体撤销属**误判**——缺陷在实现不在模型,五轮已复活模型)→ 四轮·裸 `scrollBy` smooth(取消语义丢弃上段剩余距离、WebView2 实测不累积,连滚吞量)→ 五轮·`scrollTo` smooth + 目标累积簿记(原生 ease-in-out 且重定标零速起步,快速启动缓入迟滞;浏览器无程序化滚动缓动 API——**原生路线在此触顶**,这也是三轮「尽可能回到原生」裁决的边界实证)→ 六轮·**锚点插值式**线性重定标(每次输入把插值锚拉回当下,输入频率高于帧率时帧 dt 塌缩为输入间隔,速度随输入率反常下降——极快启动顿滞形同冻结;修复 = 帧积分,输入不再触碰时间锚)。② 格子用 **left/top 定位,禁 translate3d/will-change**——逐格提升合成层(可视 40-50 个)是掉帧另一主因,生产网格仅逐「行」提层(10-15 个);绘入滚动内容层后滚动全程由合成器搬运。③ 滚动进行中(160ms 空闲判定)对格子关 `pointer-events`,抑制 hover 样式重算与悬停预览,对齐生产 isScrolling 纪律。
4. **渲染接缝**:复用 `MediaThumb`(缩略图状态机/thumbhash 占位/请求上抛是难点资产,必须复用),但实验视图用 scoped `:deep()` CSS 隐藏收藏/评分/勾选 overlay——「无附加能力」按用户指示是范围裁剪,不 fork 组件。缩略图请求走既有 `useRequestQueue`(批量 + 槽位生命周期已有 63 测锁定),结果直接 patch 实验视图自持的 visibleBlocks。**h 缓存有意不接生产的缩略图批量回写**(那要触碰 8 处生产回写点,违背解耦):缓存中 thumb_status=0 的项滚出再滚回后由前端重发请求,命中后端「已生成快路径」立即返回新状态并就地 patch——以少量 IPC 往返换零生产耦合,属实验期可接受的自愈方案(毕业合并时随滚动器一起统一)。

刻意接受的重复:horizontal.rs 内部自带一个 ~40 行的等高行装配器(A 模式页内用),**不抽象生产 `compute_justified_layout`**——实验期以「不动生产代码」优先于 DRY;毕业时再统一(届时按 Boy Scout 规则合并)。生产 justified.rs 仅做两处 `fn → pub(crate) fn` 可见性放宽(`aspect_ratio` / `median_measured_aspect`),零行为变更。

## 3. 算法规格

公共约定:宽高比 `ar = w/h`,沿用生产钳制 `[0.2, 5.0]`;0×0 未测量项用已测项中位 ar 占位;所有输出坐标取整(x/w round,主轴推进 ceil),与生产同款防亚像素缝。`viewport_height` 扣除内边距后为可用高 `H`;`gap` 同时用作项间距、泳道间距与页间距。时间方向由查询层 `sort_order`(asc/desc)承担,算法不感知。

### 3.1 A `paged`(分屏 justified)——2026-07-02 修订:两阶段 + 纵向 justify
> 🔴 本节 v1「行 y 累进、放不下开新页、页底留可变空隙」方案已被用户首轮手测反馈推翻
> (每屏必须拉满视口高),现行方案如下。

- 页宽 `PW = viewport_width × page_factor`(factor ∈ [1.0, 2.0],默认 1.2——露出残页作滚动线索,是刻意设计而非误差)。
- **阶段 1 全量装行**(与页无关):贪心等高行装配(目标行高 `target_row_height`,累加 Σar,行宽达 PW 即 commit,完整行缩放至恰满 PW,行高上限 2×target)。
- **阶段 2 分页 + 纵向 justify**:行依次入页;当下一行放不下时做**断行决策**——比较「压进本页整体压缩」`|ln 1/f_incl|` 与「挤到下页、本页拉伸」`|ln f_excl|`,取失真更小者(经典 line-breaking 代价)。封页时整页行高统一缩放到**恰满视口高**,取整像素差逐行 ±1 摊派,页底逐像素贴齐。
- 行高缩放不回改项宽(页宽已精确):引入的小幅纵横比偏差(断行决策约束下通常 ≤~25%)由前端 `object-fit: cover` 裁切吸收,与生产 grid 模式方图裁切同哲学。
- 末页 Σ自然行高 < 0.6×H 时不拉伸(顶对齐),对偶末行不拉伸规则。
- 页 i 的块:`x = i × (PW + gap)`,块内项坐标已折算为全局绝对坐标。

### 3.2 B `lanes`(等高泳道,列主序)
- 泳道数 `k = lane_count`(2–6,默认 3),泳道高 `LH = (H − (k−1)·gap) / k`,泳道 L 的 `y = L × (LH + gap)`。
- 项宽 `w = ar × LH`;各泳道独立游标 `cursor[L]`,项放置后 `cursor[L] += w + gap`。
- 指派策略(实验参数 `balance`):
  - `false`(默认,用户原案):严格列主序,item i → 泳道 `i mod k`,视觉列内严格时序。
  - `true`(漂移抑制变体):下一项放**最落后**(min cursor)的泳道——几何更整齐,代价是列内顺序轻微乱序。两者并列供调研对比。
- 块 = 一轮指派的 k 项(末轮不满),块 bbox 覆盖其全部项;各泳道游标单调 → 块 x 单调。

### 3.3 C `columns`(转置 justified)
- 与生产算法严格对偶:packing ratio 用 `1/ar`,贪心累加至 `target_col_width × Σ(1/ar) ≥ H_avail`(`H_avail = H − 列内 gaps`)即 commit;列宽 `CW = H_avail / Σ(1/ar)`,上限 `2 × target_col_width`(全景图钳制,列内容顶对齐、下方留白);末列不拉伸用 target 宽。
- 列内项高 `h_i = CW / ar_i`,四舍五入后把整数像素差从上到下逐项 ±1 摊派(对偶生产的行内宽度摊派),保证完整列恰满 H。
- 块 = 一列,`x` 累进 `CW + gap`。

## 4. 契约与文件清单

### Rust(src-tauri)
- `src/layout/horizontal.rs`(新):`HLayoutMode` / `HLayoutParams` / `HBlock` / `HItem` / `compute_horizontal_layout()` + 三算法 + 单元测试。
- `src/layout/hcache.rs`(新):`HLayoutCache = RwLock<Option<HLayoutCacheData>>`,`store_h_layout` / `get_h_blocks_by_x`(bbox 相交,线性过滤——块数 ≈ n/3,微秒级)/ 独立版本计数器(锁内递增,承接 R0-3 教训)。不含缩略图回写(见 §2-4 自愈方案)。
- `src/ipc/hgallery_commands.rs`(新):
  - `compute_h_layout(params) → HLayoutSummary { totalWidth, blockCount, totalItems, layoutVersion, computeMs }`——查询(`query_layout_items`,group_by 固定 "none")+ 布局同入一个 `spawn_blocking`(遵守「async command 内 rusqlite 一律 spawn_blocking」硬化纪律),错误走 `AppError`(命令边界错误契约)。
  - `get_h_blocks_by_x(leftX, rightX, layoutVersion) → Vec<HBlock>`,版本不符 → `AppError::LayoutNotReady`(复用现有稳定错误码,前端按码重算)。
- `src/state.rs`:+`h_layout_cache` 字段;`src/lib.rs`:注册 2 命令。生产文件仅此两处 + justified.rs 两个辅助函数的 `pub(crate)` 可见性放宽,无任何行为改动。
- 自定义命令经 `generate_handler` 注册即可从主窗口调用,与既有布局命令同路,无需新增 capabilities 声明(与现状一致)。

### 前端(src)
- `src/types/hgallery.ts`(新):HBlock/HItem/HLayoutSummary/模式参数类型(镜像 Rust serde camelCase)。
- `src/constants/ipc.ts`:+`COMPUTE_H_LAYOUT` / `GET_H_BLOCKS_BY_X` 常量(invokeIpc 常量强制)。
- `src/composables/useHVirtualScroll.ts`(新):x 轴可视窗口(rAF 节流 + 取数去重防竞态,抄生产 fetchId/边界框守卫模式)、滚轮转译(deltaY→`createWheelAnimator` **帧积分式定时长线性重定标**,deltaMode 归一,|deltaX|>|deltaY| 时让触摸板原生横扫,见 §2-3 性能基线;动画器为可注入 raf/时钟的模块级工厂,配套确定性锁测 `useHVirtualScroll.spec.ts`)、isScrolling 空闲判定、ResizeObserver(高度变化 → 上抛重算,因为视口高是布局输入)。
- `src/views/HGalleryLabView.vue`(新):实验控制条(模式切换 + 逐模式参数滑杆 + 时间方向 + 摘要/耗时)+ 滚动容器(spacer 撑总宽,层内 flatMap 渲染 `MediaThumb`,绝对定位)+ 键盘(←/→ 步进、PgUp/PgDn/Space 翻一视口、Home/End)。
- `src/router/index.ts`:+`/hgallery-lab` 懒加载路由;`AppToolbar.vue`:右侧 +1 个 FlaskConical 入口按钮;i18n 两 locale 补齐全部实验室文案键(R1-7「en-US 无中文」验收适用于实验视图)。

## 5. 验证矩阵

- Rust 单测(确定性几何,风险优先——坐标数学属「命门路径」类):
  - C:完整列恰满 H(摊派后逐像素)、列宽钳制、末列不拉伸、x 单调、gap 记账;
  - B:严格 mod-k 指派、泳道 y 精确、同泳道项不重叠、balance 指派最短泳道、块 bbox 覆盖全部项;
  - A:页高不超 H、页 x 步距、完整行恰满页宽;
  - hcache:x 区间取块(bbox 相交含边界相切)、版本守卫、空缓存分流。
- 前端:`vue-tsc` + `vite build` 通过;**滚轮动画器数学(钳制/单调/累积/重基/边界/终止)已入 vitest 确定性锁测**(五轮三次翻车后按「测点由风险决定」补锁);滚动观感/键盘导航等 UI 行为仍**不在自动化覆盖内**,按项目 DoD 校准记「手动验证步骤 + 编译通过」。
- 手动验证清单:① 三模式切换即时重排;② 滚轮竖滚驱动横滚(平滑动画)、触摸板横扫原生;③ 缩略图按可视区批量请求且滚回不重复请求;④ 参数滑杆改动 300ms 防抖重算;⑤ 空库/单张/全竖屏/全景样本不崩不裂;⑥ 滚动流畅度以生产竖直画廊为基准对照(六轮迭代史:一轮掉帧 → 去逐格合成层+平滑滚轮;二轮慢滚脉动眩晕 → 线性重定标;三轮方向乱跳 → 误撤模型回归原生;四轮连滚吞量 → 目标累积簿记;五轮启动缓入迟滞 → 复活线性重定标+锁测;六轮极快启动顿滞 → 锚点插值改帧积分,待七轮复测——重点:**极快启动无顿滞、即时满速** + 启动跟手 + 方向恒正确 + 慢滚不晕 + 连滚不丢量);⑦ A 模式每页底缘贴满视口底(末页除外)。

## 6. 显式推迟项(实验毕业后按裁决结果做)

分隔符与时间轴 scrubber(x 单调映射,A/C 天然支持、B 需泳道 flush 设计)、多选/收藏/评分等 item 能力接回、坐标压缩(>1000 万 px)与滚动器合并、scroll-snap 变体(A 的 factor=1.0 快照墙)、DESC/ASC 默认值裁决、分组(date/folder)支持、性能基准(10 万项 FPS/内存)。

## 7. 调研设计提示(非本次交付)

建议每位受试者用同一图库依次体验三模式 × {默认参数, 各 1 组极端参数},采集:主观一致性评分、找图任务用时、滚动里程/单位时间、口头报告。B 需额外观察「泳道漂移是否被察觉」(强制包含含全景图的样本段)。
