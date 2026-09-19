---
id: 2026-07-01-completed
status: active
type: rolling-status
line: 全局
created: 2026-07-01
---

# Scrollery · 已完成项详注归档(completed.md)

## 演示打码（2026-09-16）

- 顶栏一键切换并同步从本地存储恢复初值；仅支持 Canvas 画廊，空结果不隐藏关闭入口。
- 真实缩略图请求、解码和缓存保持原状；仅图片绘制施加尺寸相关强模糊，按格裁剪，DPR 滤镜半径与逻辑扩边分别计算，操作覆盖层保持清晰。
- 文件树普通/粘性行、原生路径提示、拖动标签、Canvas 目录分组头和信息浮窗使用自然演示名称。同一实体本次运行编号稳定；名称和元数据仅做显示替换，不改变业务数据。
- 开启立即收起悬停卡并作废在途准备，暂停视频与滑动预览，关闭后恢复；独立 DOM 画廊、原图查看器和详情页不在范围。
- 子代理验证：165 文件 / 1838 测试通过，含 31 项新增行为测试；vue-tsc、18 个改动文件 ESLint 通过。主会话审查并完成浏览器合成图三档视觉及开关恢复验证。
- 即时强模糊有性能代价：400 格/60px/DPR1 合成绘制、含同步读回、三次中位数为 11.30ms → 32.60ms；真实 WebView2 图库完整交互与滚动验收留在 [滚动状态](status/演示打码.md)，不宣称真实 FPS 已验证。未进行完整构建或依赖安装。
- 实施过程：[归档](worklogs/2026-09-16-演示打码/closeout.md)。

> **本文是 [todo.md](todo.md) 的详注归档**,只装 ✅ 已完成项的 `▸ 详注`——沿革时间线、分叉裁决
> (无人值守自决理由)、真机病历、教训。**从 todo.md 剪切而来,零删减**;日常无需加载,溯源时才查。
> todo.md 各节表格保留一句话结论 + commit,并在原详注位置留指针指回本文对应分节。
>
> **建档日期**:2026-07-05(随 todo.md 轻量化拆分)。新完成项的详注今后追加至本文对应分节。

> **⚠️ 维护约定(2026-07-08 二次拆分立,防复发)**:「详注」不止显式的 `### ▸ 详注` 块——**任何 ✅ 完成行,若其表格「现状结论」单元格写成了多提交交付史(沿革 / 分叉裁决 / 病历 / 诚实边界),同样算详注**。规矩:**完成即归档**——写完成行时,就把交付史落到本文对应分节(表格行只留一句话结论 + commit + 「详注 X-N」指针),不要在 todo.md 表格里堆长文事后再拆。度量 todo.md 是否膨胀**看字节 / token 而非 `wc -l`**:超长单行会让行数彻底失真(K 节曾 363 行看着没事,单个 R2 单元格实为 4605 字)。**进行中项(⬜/🔄/🔶/🔨)的 pending 上下文留在 todo.md,不归档**;仅归档其已交付子项的详注。

---
## 已收口索引(自 todo.md 迁入 2026-08-24)

> 📦 汇总已移交 [completed.md](completed.md) 的收口条目(交付波/审查/核证/状态更正)。每条含移交指针与**仍在案的活项**(⏸真机/待裁/挂账/余项)——活项随指针保留、不随正文归置;失收口即失效,不得删除。
>
> 📦 **2026-07-17 增补(缩略图档位重定 + 画廊行高滑杆改造)**:已提交 `b554aa5`(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-档位行高。**⏸真机**:重启后全量生成跑完无 panic + 行高吸附手感。
>
> 📦 **2026-07-17 增补(画廊重排滚动位置恢复)**:交付史已归档 → [completed.md](completed.md) ▸ R-画廊重排。**⏸真机**:大库切排序/分组/拖窗宽,视口顶部项钉在原位;列表顶部切排序留顶。
>
> 📦 **2026-07-18 增补(审查 9 项修复)**:四批修复全部落地(`efec238`/`0d1234a`/`305d4b6`/`8b0220f`),交付史已归档 → [completed.md](completed.md) ▸ R-审查9项。遗留 F-03 故障注入/F-07 真源回退矩阵未自动化——已归备份测试硬化专项(U-14/U-15)。
>
> 📦 **2026-07-19 增补(方案 A 导出整理成果,阶段 1-3 落地)**:核心链路交付(相册/视图工具栏入口已由 U-2 `5a97bd4` 接线,2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-导出成果。**⏸真机验收**:文件夹选择器/预检警告/进度与取消/刷新后恢复/打开导出目录/10 万+ SelectAll/离线盘/库内目标确认。
>
> 📦 **2026-07-19 增补(方案 A 导出线深审修复,3 批全交付)**:15 项已修(3 批 `c082e51`/`76ea910`/`2a24e99`),交付史已归档 → [completed.md](completed.md) ▸ R-导出深审。**3 项语义/设计级留待用户裁**:取消检查粒度为整文件级/进程崩溃遗留 staging 无启动清扫/磁盘满时明细是否保留;⏸真机验收未变(代码走查+单测,未 GUI 验证)。
>
> 📦 **2026-07-17 增补(无缝 minimap 轴)**:交付,交付史已归档 → [completed.md](completed.md) ▸ R-minimap轴。**⏸真机验收**:无缝开→右轴 minimap 出/拖·点·滚轮手感/chevron 收起重启记忆/54 万库深滚性能。本项即批量 15 项线遗留裁决 **D-003(#1 无缝下时间轴)的落地答案**。
>
> 📦 **2026-07-19 深审与渲染模式设置**:交付史已归档 → [completed.md](completed.md) ▸ R-minimap深审。**⏸真机验收**:54 万库深滚、触屏/macOS/iOS 位图解码。
>
> 📦 **2026-07-20 开线、2026-07-21 全线收官(日志能力重构,S1..S6 已落地)**:已推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-日志重构。**挂账**:docs 门 frontmatter 基线红本线占 5 文件,用户裁决转本线收口自办,不代修。

> 📦 **2026-07-21 增补(span 埋点+worker 日志汇入)**:全五阶段收官,已推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-span埋点。**⏸真机 GUI 验收**(not automated):跑一次派生→分析标签 TopN 出行;跑 AI 分析→日志窗口可见 `target=scrollery::worker` 行。
>
> 📦 **2026-07-20 增补(图片编辑开源包混合升级 v2 全部七阶段落地)**:已推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-图片编辑v2。**⏸真机验收**:Cropper.js 触摸手势/广色域真实照片调色目视/50MP 级拖动帧率;**已知全仓现状缺口**:CI 无 macOS/iOS/Android 覆盖。**挂账**:docs 门 frontmatter 基线红 3 文件 4 处,用户裁决转本线收口自办。
>
> 📦 **2026-07-21 增补(无人值守清账波)**:六项落地 + 接续两段(U-* 采纳落地/planning 盘点波)全交付,交付史已归档 → [completed.md](completed.md) ▸ R-清账波。**挂起未变**:U-4(随画廊/canvas 线)/U-8(随版本升级波)/U-9(需专项)/U-10(随 DB 波);⏸GUI 真机验收清单见三件套 progress.md。
>
> 📦 **2026-07-22 增补(AI/face 流水线根治线)**:五 commit 全落 + 真机实证(2026-07-22 用户确认),已推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-AI根治。二期候选 F-029 见 [status/AI与人脸流水线根治.md](status/AI与人脸流水线根治.md)。
>
>
> 📦 **2026-08-11 增补(F-01 发货闭包断链修复——ai-worker 收口)**:交付史已归档 → [completed.md](completed.md) ▸ R-ai-worker闭包。**残余**:enhance-worker(模型清单仍 PENDING 占位,就绪后按同模式补 externalBin)与 video-worker(prod 打包未就绪)仍属 F-01 断链,归 F-003 跟踪。
>
> 📦 **2026-07-25 增补(全仓代码注释清理精简线)**:四 commit 落 dev 并推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-注释清理。**新待裁 1 项**:英文散文注释行 515 行/84 文件(疑似漏删双语对 150 处)+ 99 个 .rs 自指文件路径头留账,重启须新决策(明细与扫描器路径在三件套 findings.md F-h,数已实测勿重扫)。

> 📦 **2026-07-25 增补(超长文件拆分施工线)**:A 详案 10 + B 简案施工全落并推 origin/dev(2026-08-13 核证),交付史已归档 → [completed.md](completed.md) ▸ R-超长拆分。**derivations.rs 按 D-454 明示跳过未施工**(真 user WIP 红线未解除,待用户裁归属);⏸GUI 手测清单待批(各阶段复核文档内)。


> 📦 **历史更新日期日志(2026-07-02→08-14)**:已归档 → [completed.md](completed.md) ▸ R2-更新日志。本文头部自 2026-08-23 起不再累积日期叙事。
> 📦 **2026-07-05 全面源码审查快照**:已归档 → [completed.md](completed.md) ▸ H 节,另见 ▸ 详注 B1-1(2026-07-05 ③b 落地:「B1 状态板整表翻绿,唯真实生产公钥一行待外部签发机基建」+ 保留 KeyringLicenseStore 双实现并存裁决)。
> 📦 **2026-07-19 图片编辑开源包混合升级方案(落盘时未施工)**:已归档 → [completed.md](completed.md) ▸ R-图片编辑方案。方案已由 v2 七阶段施工承接(D-112「编辑整体=付费高级功能」已裁,E0–E6 全落地);残余=付费 feature 复用能力抽象,见「收口挂账」#7。
> 📦 **2026-07-25 最近一周代码实现深度审查(发布建议:阻断)**:已归档 → [completed.md](completed.md) ▸ R-深审07-25。**复证仍成立未修**:F-03/F-04/F-05/F-06/F-10/F-13;耐久候选 F-002(FRESH INSTALL 冒烟)/F-003(配置层确定性测试)/F-004(增强状态机测试)——处置待主线排期,跟踪于 [status/全仓深度review与直修.md](status/全仓深度review与直修.md)。
> 📦 **2026-07-31 第三轮全仓深审+无分叉直修(`504f209`)**:已归档 → [completed.md](completed.md) ▸ R-深审07-31。**残余待裁**:enhance/video worker 无生产 bundle/fresh-install 闭包;配置跨进程事务、layout latest-wins、错误类型化、增强 FIFO、dev 审计与 bundle 余量(D-001..D-008)——跟踪于 [status/全仓深度review与直修.md](status/全仓深度review与直修.md);三件套保留施工态 [planning/2026-07-31-全仓代码深度审查与直修/](planning/2026-07-31-全仓代码深度审查与直修/task_plan.md)。
> 📦 **2026-08-03 UI 开发加载/关闭链路修复(`0c9ac89`)**:已归档 → [completed.md](completed.md) ▸ R-UI链路08-03。**待 GUI 真机回归**;过程见 [worklogs/2026-08-03-UI加载失败排查/](worklogs/2026-08-03-UI加载失败排查/task_plan.md)。
> 📦 **2026-08-13 全仓未完成工作代码核证回写**:已归档 → [completed.md](completed.md) ▸ R2-核证08-13。**仍在案**:`codex/fix-ci-raw-worker` 分支 9 commit(含 RAW F-06 交叉构建修复)仍未合入 dev(git 2026-08-23 实证)。
> 📦 **2026-08-13 审查 P0/P1 四项直修(已提交 `a7cf607`/`f05631c` 等)**:已归档 → [completed.md](completed.md) ▸ R2-P0P1直修08-13。**留账**:18 项 P2 级 findings + 1 项待裁(migrate 坏布尔翻转默认 true 键——刻意+已单测但语义不等价)见[报告文末](reviews/2026-08-13-审查P0P1四项直修报告.md);**⏸真机**:最小化 5min+ 后任务栏 ✕ 须弹框、DB 锁失败后库目录无残留。
> 📦 **2026-08-14 完成/可收口任务代码核证回写(25 条核证表 + 三处划线订正 + 两处文档残留修正 + 同日 43 线收口执行)**:已归档 → [completed.md](completed.md) ▸ R2-核证08-14 / R2-核证表08-14。**余项挂账 18 行**(逐行明细见归档表「余项」列):md阅读器(⏸真机)、底栏重构(⏸5 项)、视频封面(⏸软解回退)、阅读器UI(⏸6 项)、根文件夹显隐(⏸+push 待批)、缩略图进度(⏸4 步)、图片编辑v1(集成测试/真机缺口已诚实录)、数据备份恢复(⏸恢复交换)、前端动画重构(⏸§P5 5 条)、应用配置重构(待裁 D-c01.. 5 条+⏸)、07-23 深审与直修(⏸4 项;J5/J6 未确证)、窗口化沉浸(⏸+push 待批)、大图浏览器(⏸坏图复验)、审查应用配置(修复待裁)、第二轮深审(⏸rust-linux CI+push)、minimap 解耦(⏸)、canvas 设置项(⏸观感)、画廊大图首开(⏸低速磁盘);另 7 行可收口/已修(两日修改审查/施工规格集/未完成工作梳理/上线前三功能方案/free 最短路径/AI 语义搜索/Spec15 深化)。
> 📦 **2026-08-16 Canvas 缩略图加载渲染性能线收官**:已归档 → [completed.md](completed.md) ▸ R2-Canvas08-16。新登记 2 项立项候选见上「进行中」。
> 📦 **2026-08-18 画廊冷启动无限重排(状态更正与验收)**:已归档 → [completed.md](completed.md) ▸ R2-稳定化08-18。Windows WebView2 四项验收全部通过;记录见 [worklogs/2026-08-18-恢复后画廊稳定化/](worklogs/2026-08-18-恢复后画廊稳定化/task_plan.md)。
> 📦 **2026-08-18 续作收官(Canvas 生命周期 + 查看器返回闪烁)**:已归档 → [completed.md](completed.md) ▸ R2-路由覆盖层08-18。Windows WebView2 连续往返/侧栏/快捷键验收通过;裁决 [decisions/2026-08-18-查看器路由覆盖层呈现裁决.md](decisions/2026-08-18-查看器路由覆盖层呈现裁决.md)。
> 📦 **2026-08-21 入库扫描元数据读取性能优化**:已归档 → [completed.md](completed.md) ▸ R2-扫描08-21。余项见上「进行中·扫描线余项」。
> 📦 **2026-08-23 扫描线 P1 修复与对照线优秀部分吸收**:已归档 → [completed.md](completed.md) ▸ R2-扫描线08-23。余项见上「进行中·扫描线余项」;过程见 [worklogs/2026-08-23-扫描线P1修复与tmp吸收/](worklogs/2026-08-23-扫描线P1修复与tmp吸收/task_plan.md)。
> 📦 **2026-08-23 批次 2(正文 ✅ 行瘦身 + 整节收官)**:整节收官=queries 节 → [completed.md](completed.md) ▸ **Q 节**(⏸ 余:§8.4 运行时 smoke真机)+ 07-05 审查快照 H1/H3/H4 → ▸ **H 节**;✅ 行瘦身(交付史归档)=▸ **J-progress**(批 1-4/增量/加固对/P2 池)+ ▸ **G3-小决策/联网预取/G3-NOTICE** + ▸ **M-施工批**(07-10 施工批)+ ▸ **N-60px源** + ▸ **O-Canvas热路径/性能面板** + ▸ **S-定稿** + ▸ **U-全档**(后端大文件 7 行;⏳ 余:真机 smoke+可选 P1-b)。所有活项(⏸/⏳/⬜/🔨)均随指针保留于正文未动。

---

## A. GitHub 两仓拓扑落地(已收官)

> (状态板整迁自 todo.md,2026-07-12 文档治理「整节收官整节搬」;全节 ✅ 收官——末条独立 `picasa-next-pro.git` 归档由用户 2026-07-12 完成。表内仓名为历史快照,2026-07-06 已随 R2-7 改名 scrollery-private / scrollery,见记忆「两仓拓扑与命名」。)

Pattern C:现仓已转私有 = canonical 超集;公开仓 = Copybara 单向投影。详见
[Part6_3c_Copybara同步配置草稿.md](refactor_2026/Part6_3c_Copybara同步配置草稿.md) 与记忆「两仓拓扑与命名」。

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | 定私有仓名 = `picasa-next-private`;公开 = `picasa-next` | 用户已建两空仓 |
| ✅ | 本地 `origin` 重定向 → `picasa-next-private.git`(不加 public 远程) | 我已做,commit 环境 |
| ✅ | pro 并入私有仓:`git add crates/picasa-next-pro/`(4 文件,无 target) | commit `44ae960`;占位 keyset 替换属 ③b |
| ✅ | copy.bara.sky 的 origin/destination url 填真实地址 | 我已改草稿 §5 |
| ✅ | **push 本地 → 私有 canonical** | 已建立(2026-07-02 实证:origin/dev 与 origin/main 均达 `56fb3d8`;私有 CI 在 `d1430dc` 全绿=R0-1 门控实跑生效) |
| ✅ | 私有仓 GitHub 设默认分支(main 或 dev) | 已设 main(2026-07-02 gh api 实证) |
| ✅ | 归档/弃用独立 `git@github.com:gfgjs/picasa-next-pro.git` | **用户 2026-07-12 完成**;防 pro 两处 canonical 漂移——独立 -pro.git 已归档/弃用,pro 此后仅存私有 canonical 超集内 |
| ✅ | 公开仓 `picasa-next`:**保持空**,勿手推;待 ③c Copybara 首次同步 | baseline 已建并重建干净(2026-07-02,`666da7f`)→ **▸ 详注 A-1** |
| ✅ | **重建私有仓 + 轮换 deploy key**(Public 事故谨慎处置) | 12 步流程已交付;2026-07-02 确认全部完成勾销 → **▸ 详注 A-2** |

> 📦 本节 ✅ 已完成项的 ▸ 详注(沿革 / 分叉裁决 / 病历 / 教训)见下方 A-1 / A-2。

### ▸ 详注 A-1:公开仓 `picasa-next` 保持空 → 首次同步 baseline

- **待办语义**:公开仓保持空、勿手推,待 ③c Copybara 首次同步。
- **baseline 已建(2026-07-02)**:sync-staging 单 squash commit、bot 作者;顶层树核验无 `pro` / `copy.bara.sky` / `sync-oss.yml`。
- ⚠ 因 plan-docs 排除扩大 + Cargo.toml 标记块启用,须 force **重建一次**干净 baseline → **✅ 已重建**(2026-07-02,`666da7f`,实录见 C 节收尾行)。

### ▸ 详注 A-2:重建私有仓 + 轮换 deploy key(Public 事故谨慎处置)

- **背景**:用户拍板 2026-07-02;12 步流程表已交付(会话 2026-07-02)。
- **流程**:旧仓改名腾位 → 新建同名 **Private** 空仓 → push dev/main → 公开仓换新 deploy key + 新仓重建 secret → dispatch 验证 → 归档旧仓。
- 🔴 **顺序陷阱**:改名后、同名新仓建立前**勿 push**(redirect 会导去旧仓);SHA 不变 → Copybara 增量无缝、无需重 init_history。
- **进展观测(2026-07-02)**:`OSS_SYNC_DEPLOY_KEY` 已于 03:39 更新、03:41 dispatch 验证通过(key 轮换步已发生;03:35 恰落新旧 key 空窗致 sync green-skip 一次,无害);仓库重建 / 旧仓归档未见执行痕迹。
- **✅ 勾销**:2026-07-02 用户确认相关工作已全部完成。
---

## B. Part6 红线收尾(③b / ③c,建议对齐 v0.1 变现启动)

### ▸ 详注 B1-1:③b 变现物理下沉落地(2026-07-05,`7a428e6`)

- **裁决(用户在环,方案 A)**:**保留 KeyringLicenseStore、不删源、开源默认不切 FreeStub**——推翻原「迁移+删源」施工写法(Part6 §3.9.1/§3.8 正文已同步回写横幅)。理由:最小破坏、可回滚;红线不受损(开源侧内置仅占位集、无密钥价值,真钥仅 pro);保开源可用性(fork 可自建签发链)。
- **落地形态**:pro 入 workspace member(根 Cargo.toml 标记块内)+ src-tauri 标记块 dep(刻意非 optional——feature 门控会让 `dep:` 字面泄进公开 [features] 表)+ swap 点 `default_entitlement_provider` 标记块早退构造 `DirectEntitlement`(解析失败 fail-closed 降 FreeStub);公开镜像经 Copybara 剥离标记块回退开源装配。pro 去 `[workspace]` opt-out、删嵌套 Cargo.lock。
- **四盲点(施工前审查抓出,均已补)**:
  1. **Cargo.lock 投影污染**——私有 lock 的 pro 条目会被原样投影,oss-gate ② 硬红且公开 release.yml `--locked` 需要 tracked lock;修=copy.bara.sky 增 lock 剥离 transform(pro 包块 + 主包依赖行),投影 lock 保持 pro-free 且钉版本。
  2. **内测键回归**——swap 后若 DirectEntitlement 只认 pro 占位 json,内测 license 验签即断;修=pro build.rs 镜像 exotic-trust 同款编译期注入(**同变量** `PICASA_EXOTIC_KEYSET_FILE`),内测链 build-internal-installer.ps1 零改动存活,registry/license 两信任根不分叉。
  3. **pro 独立 crate 残留**——`[workspace]` opt-out 不摘则双 workspace 根报错;嵌套 lock 转死物删除。
  4. **debug dev 旁路断链**——`PICASA_EXOTIC_DEV_KEYSET` 原经 trusted_keyset 同时管激活;swap 块内镜像同条件分支(`DirectEntitlement::new(trusted_keyset())`)保 dev 商店 E2E 激活链;Release 无此分支(SEC-02)。
- **验证(仅本地)**:cargo check / fmt --check / clippy --all-targets -D warnings / test **470 全绿**(含新增 swap 冒烟:装配出 provider 标签=direct,实证标记块路径 + 占位 keyset 可解析;pro 套件随 workspace 收编入测试面)+ NOTICE --check 绿(pro 为 path crate 不入第三方清单,402+95 不变)。
- **剥离模拟(公开树预演)**:`git -c core.autocrlf=false archive HEAD` 逐字节导出 → glob 排除(pro/copy.bara.sky/sync-oss.yml/plan-docs)→ 复刻两式标记剥离 + lock 剥离(正则与 copy.bara.sky 同构)→ 断言恰 3 文件 + 1 lock 命中、禁词零出现、lock 零 pro → `cargo check --workspace --locked` **exit 0** + members 无 pro。`--locked` 通过 =「剥 pro 两条目 ≡ 剥离树重生 lock」的逐位等价实证。
- **病历(模拟首跑假阳性)**:一跑 3/4 标记文件未剥离——根因双杀:git archive 默认吃 autocrlf 把导出树洗成 CRLF + **JS 正则 `.` 连 `\r` 也不匹配**(RE2 的 `.` 只排 `\n`,两引擎方言差)。`git ls-files --eol` 实证 index 全 LF → 真 Copybara 读 blob 字节无恙,纯模拟器保真度问题;修=导出加 `-c core.autocrlf=false` 达字节级保真。教训呼应 NOTICE 病历:**复刻型验证必须对行尾与正则方言自免疫——「复刻了意图」≠「复刻了字节」。**
- **真管线实跑收官(2026-07-05 当日,承接上文剥离模拟)**:私有 CI 全绿(run 28741947688)→ 私有 main ff `5449ca1..23bdd32` → sync-oss 实推成功(run 28742635698,**非 NO_OP**:5 transform 全执行=标记剥离+lock 剥离首跑+三禁词 verify 全 PASS)→ 公开 sync-staging `e663e22`(GitOrigin-RevId=`7a428e6`,docs commit 因 plan-docs 全排除不改投影)→ **投影树三处抽查逐字节精确**(根 Cargo.toml 无 pro member 且标记注释块整块剥净 / Cargo.lock 零 pro / mod.rs 仅剩 KeyringLicenseStore 开源装配)→ oss-gate 双 job + drift-alarm 全绿 → 公开 main 快进提升 `b3cbbdb→e663e22`(gh api PATCH 非 force,required checks 同 SHA 已绿)。**结论:四层防线(glob/标记剥离/lock 剥离/禁词+构建 gate)全部经真管线 CI 实证;本次亦是 baseline 后首次稳态提升,「gate 绿 → api ff」流程跑通。**
- **剩余**:真实生产公钥(受控签发机,外部基建;替换 pro 占位 json 或商业流水线注入,归 Part8)。另注:内测签发链脚本(exotic-issue-license / internal-registry / lib/exotic-signing)随本次提升入公开树——推送前安检实证零内嵌密钥(密钥从未跟踪的 `.internal-signing/` 加载),且与「fork 可自建签发链」裁决姿态自洽;若日后欲收回,copy.bara.sky 加 scripts 排除即可(需新决策)。

### ▸ 详注 B2-1:私有仓落地 `copy.bara.sky`(§5)

- 已落仓库根;相对草稿 §5 有三处实证修正:FORBIDDEN 收窄避注释假阳性 / push=sync-staging / 排除整个 `refactor_2026`。
- **2026-07-02 CI dispatch dry_run 实跑全绿**,经 8 轮迭代:olivr 镜像 + 尾参 copybara / `verify_match` 用 `paths` / `+` 拍平 / `ignore_noop` 包裹 / `reversal=[]`。
- **3 条 FORBIDDEN verify 全 PASS** = 红线剥离实证有效;修正实录见草稿顶部 ⚑ 横幅。

### ▸ 详注 B2-2:`.github/workflows/sync-oss.yml`(§7,方案 A)

- 用户认可**方案 A**,已落地并 **CI 实跑验证**。
- 双凭据(`file://` 私有读 + SSH 只写公开)均认证通过。
- green-skip 守门 + `init_history` / `dry_run` / `force` 入参 + **exit 4=NO_OP 按成功处理**(内部-only 推送常态;实证:自动同步后手动重跑即 NO_OP)。
- YAML 已校验。

### ▸ 详注 B2-3:发布 gate `oss-gate.yml`(§6)

内容:staging 分支 + 剥离树 `cargo build/test` + members 断言 + gitleaks。

- **落地**:`.github/workflows/oss-gate.yml` 随投影同步公开仓;`if github.repository` 守门只在公开仓跑——私有仓 dispatch 显示 **Skipped** 即守门生效。
- **公开仓实跑迭代**:
  - **一跑(红)**:全树 grep 扫 `.md` 假阳性 → 范围对齐 `verify_match`。
  - **二跑(红)**:GNU grep include/exclude「最后匹配获胜」,exclude 写在 include 前被压掉 → gate 自指命中 env FORBIDDEN;修 = 排除移到 include 后。
  - **三跑**:禁词 / lock 重生 / Rust / apt 首过,挂 win32 resources → 平台拆分 `tauri.windows.conf.json`(`750817b`)。
  - **四跑**:深入 src-tauri 曝 frontendDist 编译期校验 + wic_engine Linux 编不过(**既有跨平台缺口非剥离缺陷**)→ gate 迁 **windows-latest**(基准=canonical 平台)+ 占位 dist + rust-cache。
  - **五跑**:windows 迁移实证成功,挂资源校验(resources 指向 node_modules 内 ONNX 二进制)→ 补 setup-node + `npm ci`(贡献者流程环境复原,非前端质量门)。
  - **六跑(里程碑)**:check 强制门 + test 429 全过 + members 过;仅挂 gitleaks-action@v2 的 windows 安装 bug(拼 tar.gz、官方 Windows 资产是 zip → 404)→ 拆并行双 job(gate=windows Rust,gitleaks=ubuntu)。「全绿」= 两 job 都绿,分支保护 required checks 勾两个。
- **提升 staging→main 已完成**(2026-07-02,推已绿 SHA + rulesets required checks 方案,实录见 B2-5)。

### ▸ 详注 B2-4:公开仓分支保护(只收 sync-bot)+ 反向漂移检测(§8)

- **2026-07-02 落地**,采 **rulesets 双规则**(用户仓无经典 restrictions,rulesets 等价达成):
  - `main`:禁删 + 禁 force push + required checks 三项(gate / gitleaks / drift-alarm);提升 = 推已在 sync-staging 验绿的同 SHA,天然放行。
  - `sync-staging`:restrict update 仅 DeployKey bypass(只收 sync-bot,owner 误推也拦)。
- **drift-alarm workflow**(私有 `8a201c6`,随投影进公开仓):push 断言 HEAD 作者=sync-bot,非 bot 即红;bot 推送两跑绿(7s / 10s)实证不误报。

### ▸ 详注 B2-5:首次 dispatch dry_run → init_history 建 baseline + 重建/提升/保护收尾

- **稳态已实证**:baseline 已建成;push:main 自动同步已实证(干净 HEAD `dc005d2`:plan-docs 全排除生效、公开 Cargo.toml 零禁词)。
- **gate 首跑红三修**:`verify_match` 的 `**.ext` 根目录盲区 / grep 范围对齐 / plan-docs 全排除 + Cargo.toml INTERNAL 标记块。
- **机制修正**:`--force` 不重写既有 baseline、`--init-history` 遇已有标记被忽略 → 抹旧脏 baseline(`103f87a3` 含 todo.md)须**先删公开 sync-staging 分支**再重建。(过期记录:曾疑 gate 于 `dc005d2` 仍红,实则修复早已生效,`b96eaa8` 即绿。)
- **收尾实录(2026-07-02 全链路收官)**:
  1. dev 全量提升 main(用户拍板,`56fb3d8`)
  2. 等自动 sync 完(30s 增量绿)
  3. 临时分支顶默认位(GitHub 禁删默认分支,须换位)
  4. 删 sync-staging → dispatch init_history + force
  5. **干净单 commit baseline `666da7f`**(sync-bot 作者;树核验无 plan-docs / pro / copy.bara.sky / sync-oss.yml)
  6. 三 check 全绿(gate 5m18s 含 cargo check + test --workspace、gitleaks 6s、drift-alarm 7s)
  7. 建 main@`666da7f` + default 改 main + 撤临时分支 + rulesets(见 B2-4)
- ⚠ **脏 baseline 遗留**:`103f87a3`(含 todo.md)已不可达,但 SHA 直址在 GitHub GC 前仍可访问——彻底清除需删建公开仓(deploy key 须重配)或联系 GitHub Support,由你裁决。
- **内容盘点(2026-07-02 SHA 直址实测,树未截断)**:27 份 plan-docs——todo.md、两份 license_distribution 变现设计(v1 弃用版 + v3 part3)、`_deprecated` 评审/施工纪律文档、implementation/exotic 计划、踩坑记录;**`refactor_2026/` 自始被排除不在其中**;另根 Cargo.toml 含 pro 注释块。**无 pro 源码、无生产密钥**(exotic-keyset.json 为开源占位)。

### B2. ③c Copybara 同步管线(已收官)

> (状态板整迁自 todo.md,2026-07-15「整节收官整节搬」;末条人工核对由用户 2026-07-15 完成后全节 ✅。逐项 ▸ 详注见上方 B2-1…B2-5。)

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | 私有仓落地 `copy.bara.sky`(§5) | dispatch dry_run 实跑全绿(8 轮迭代),3 条 FORBIDDEN verify 全 PASS → **▸ 详注 B2-1** |
| ✅ | `.github/workflows/sync-oss.yml`(§7,方案 A) | 双凭据认证通过,green-skip + NO_OP 处理已落地 → **▸ 详注 B2-2** |
| ✅ | 公开仓建**只写** deploy key + 私钥存私有 secret `OSS_SYNC_DEPLOY_KEY` | 用户已建;CI 实跑中 SSH 对公开仓认证成功即证 |
| ✅ | 发布 gate:staging 分支 + 剥离树 `cargo build/test` + members 断言 + gitleaks(§6) | `oss-gate.yml` 落地,经 6 跑迭代达里程碑(拆 windows/ubuntu 双 job)→ **▸ 详注 B2-3** |
| ✅ | **`Cargo.lock` 重生纳入 gate**(私有 lock 含 pro,须重生公开 lock) | gate 已预置(lock 无 pro 断言 + 重生);**③b 落地后投影侧同步就位(2026-07-05)**:copy.bara.sky 增 lock 剥离 transform(pro 包块 + 主包依赖行),剥离模拟以 `--locked` 实证「剥离 ≡ 重生」逐位等价 |
| ✅ | 公开仓分支保护(只收 sync-bot)+ 反向漂移检测(§8) | 2026-07-02 落地 rulesets 双规则 + drift-alarm workflow → **▸ 详注 B2-4** |
| ✅ | 首次 dispatch dry_run→init_history 建 baseline + 重建/提升/保护收尾 | 干净单 commit baseline `666da7f` 全链路收官 → **▸ 详注 B2-5** |
| ✅ | 首发前人工核对:公开树 vs 预期开源快照 diff 一遍(§8 末条) | **用户 2026-07-15 完成核对**(你侧动作,权限层留人工)。此前已抽查 `666da7f` 根目录 + workflows + crates(无 docs/pro/同步配置,drift-alarm.yml 就位);~~CLAUDE.md / CLAUDE_中文对照.md 在公开树——2026-07-02 你已裁决可公开~~ **2026-07-12 你已推翻该裁决**:CLAUDE.md 入 copy.bara.sky 排除面(内部纪律+docs/ 死链),对照文件已删,核对时按「不应存在」断言 |

### B3. ③c 开放问题(已收官,§10)

> (状态板整迁自 todo.md,2026-07-12「整节收官整节搬」;四项开放问题全部拍板消解,无独立详注块。)

| 状态 | 待办 | 阻塞源 / 备注 |
|---|---|---|
| ✅ | 公开仓历史:续用现有 vs 重开 | 已消解:公开仓为全新空仓,init_history 直接建全新 baseline |
| ✅ | `docs/refactor_2026/` 是否入公开镜像 | **2026-07-02 扩大为 `docs/**` 全排除**:todo.md / license_distribution / `archive`(原 `_deprecated`)会话记录同含变现与红线策略,且 gate 首跑实证 todo.md 携禁词字面;要公开的技术文档日后移 docs/ 放行 |
| ✅ | 同步频率:push 触发 vs 定时/手动 | 已落地 push 触发 + concurrency 串行 |
| ✅ | Copybara 运行环境:GitHub-hosted docker vs self-hosted | **实测定案 `olivr/copybara`**(Google 无官方镜像,copybara#137;原「荐官方 google/copybara 镜像」系误判),GitHub-hosted |


---

## C. Part6 其他剩余(后置 / 待前置)

### ▸ 详注 C-1:Part6 T3–T8 框架层 G1–G6(✅,T8 除外)

内容:协议 / Coordinator 通用化 / 长驻 session / GpuLimiter / model blob / 低优先级。

- **T3–T7 已随 Part4 T10–T15 交付**(2026-07-04 对账收账,Part6 §4 表已同步):
  - T3 协议 = Part4-T10 同施工
  - T4 Coordinator 通用化 = T13(`647891b`)
  - T5 长驻 session + Worker 泛化 = T15(D3 修正:握手 5s 不动,300s 归 SessionInit)
  - T6 GpuLimiter = T11(D2 修正:令牌全留主进程、协议零扩展)
  - T7 model blob = T12
- **T8 mac/Linux worker 低优先级 defer**(随 T19 mac/Linux 适配波;Windows BELOW_NORMAL 已有)。

### ▸ 详注 C-2:Part6 T12–T13

- **T12 ai/face worker 接入插件平台(暂缓有据)**:框架前置已解除(Part4 worker ✅),但**分发面待 Part7/Part8**(ai-worker 现随主程序同目录分发,T17 裁决⑦;加密模型面待 T11 AES 后置)。
- **T13 多渠道字段预留 ✅**(2026-07-04 `5d1b1ff`):
  - Catalog/Registry 渠道字段(serde default 旧数据兼容)
  - InstallSource 枚举 + PluginDeliverySource 三交付源(直销实装 / Steam·Store stub)
  - `exotic_plugins.entitlement_source` 列(**V12** 迁移,既有行回填 `'direct'` 语义为真)
  - install_staged_zip 接渠道参 + lib.rs Steam 启动占位
  - 验证 = clippy 默认 + channel-steam 组合零警告、channel-msstore check 过、lib 371 全绿(仅本地)
- **T13 分叉裁决(无人值守)**:
  1. 非 direct 渠道 **fail-closed**(channel_unsupported,先于一切副作用),而非按 §8.4 预铺「跳 Registry 验签保 hash 复核」弱路径——安全敏感分支不留无消费者死代码,Part8 实装随真实 Steam/Store 交付一起带测试落地。
  2. 列版本走 V12(§4 表原文「随 V11 或并入 V10」已被占用,过时)。
  3. 前端投影 InstalledExoticPlugin 不加字段(无 UI 消费,Part8 需要时再暴露)。
  4. repair/rollback 的 record_from_installed 暂回填 direct(现唯一渠道,Part8 改保留 DB 原值)。
  5. PluginDeliverySource 现阶段仅承载 `channel()` 判别,不预设拉包方法(同 T15 裁决④哲学)。

### ▸ 详注 C-3:`tauri build` release 路径验证(target 迁根后)

- **静态**:conf 无 target 依赖(frontendDist / resources / icon 皆相对 src-tauri 指同级,externalBin 不存在)。
- **实证 `tauri build --no-bundle`**:Finished release 2m29s,二进制落 **workspace 根 `target/release/picasa-next.exe`**(非旧 src-tauri/target),Tauri 经 cargo metadata 自动定位,dist 前端产物齐。
- **顺带修** Vite watch EBUSY(`dbf7287`,同源迁移副作用)。

### ▸ 详注 C-4:完整安装器打包(WiX MSI / NSIS)验证

- **状态 ✅**(2026-07-05 随 Part7-T17 dry-run 实证):公开投影树 lite 变体,windows runner 自动拉取 WiX/NSIS,MSI + NSIS 双产出 + SHA256(run 28726569655);商业变体 / 本机打包随 T16。
- **与 target 迁移无关**:bundle.resources 路径 target-独立,已静态验;需装 WiX/NSIS 工具链,发布前单独验,非迁移风险。
- ⚠ **语义反转(2026-07-04 Part7-T6)**:bundle 已不再声明 ONNX/DirectML 资源(`tauri.windows.conf.json` 整文件删,dev DLL 走 `.cargo/config` `[env]` 指 npm 包)——打包验证时**核对 bundle 内容物不含 4 个 ONNX DLL**;另 perf conf 顶层 `"//"` 注释键不被严格 schema 接受,首次启用 perf 变体前须删。

### ▸ 详注 C-5:公开树 Linux/macOS 构建适配

- **状态 ✅**:Linux 半边 + gate 回迁(2026-07-05);macOS 半边留 Part3-T8 mac 门控 / mac 环境,defer 不变。
- **已完成子项**:
  - windows dep 加 target 限定 ✅(R0-2)
  - `#[cfg(windows)]` 门 wic_engine 及其注册点 ✅(R0-2)
  - ~~各平台推理资源各建 `tauri.<platform>.conf.json`~~ **勾销=无对象**:Part7-T6 已整删推理资源声明,bundle 零平台资源差异。
- **Linux 面全链实录(2026-07-05)**:
  1. 静态审计五处 Windows 面(wic_engine 模块级 / media_foundation 模块级 / volume_probe 函数级 / DXGI 块级 + None 兜底 / worker 优先级双臂)+ Part4-T16 迁 host 的 DXGI 探测全规范。
  2. 私有 ci.yml 增 **rust-linux job**(check + test,`0d08f40`)——常驻 Linux 编译面回归守卫,兼 gate 回迁前置实证(私有超集绿 ⇒ 公开剥离子集绿)。
  3. 首跑 380/382:当年 gate 试跑「15 错」已被 R0-2/T16 清尽,仅剩 coordinator 两测假红——夹具用内置真实 Catalog 测运行条件,PSD 平台清单(win-x64/mac-arm)不含 Linux,「应当运行」断言因 UnsupportedPlatform 假红、负向断言因错误理由通过;修法 = 注入 `current_target_triple()` 平台的最小 Catalog 解耦(`00037b4`),复跑全绿。
  4. **oss-gate 回迁 ubuntu**(`5449ca1`,同步改写 2026-07-02「跑 windows」fork 决策注释为解除记录)+ 提升激活(私有 main→`5449ca1`,sync 绿,公开 staging `b3cbbdb`):ubuntu gate 首跑绿 5.8min(冷缓存) vs windows 暖缓存 7.4min,缓存暖后差距更大;gitleaks 与 gate 归一 ubuntu;公开 main 已提 `b3cbbdb`(同 SHA 三 checks 放行)。
- **备注**:本项与 T17 dry-run 共两次例行提升均记于此,提升已流程化。


---

## D. Part5 剩余

> (状态板整迁自 todo.md,2026-07-10 文档治理「整节收官整节搬」)

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | Part5-T11 插件商店**前端 UI** | 增量1 数据层 + 增量2 PluginStoreView(`a68faa0`)交付;dev registry 全链路基建已解锁(`961360d`)→ **▸ 详注 D-1** |
| ✅ | Part5-T12 插件 gate + 购买引导 | 数据层 + PluginGate + 详情覆盖层接线全交付(`05b5c01` 等);缩略图格 gate 有意 defer → **▸ 详注 D-2** |
| ✅ | Part5-T13 离线 UX | ①④已就位,本会话补齐 ②③(后端 `6583331` + 前端已知卷面板);rescan/relink 有意 defer → **▸ 详注 D-3** |
| ✅ | Part5-T19 store 去重 | 已完成(S5 configStore 单源化 + S6 useAnalysisController 抽取);Options→Composition 纯风格重构可选未做(高风险、无运行时验证、价值低) |
| ✅ | Part5-T20 布局切换 | 轨道 A 已完成(galleryLayoutSource + Grid/Justified 后端 uniform-packing + 前端切换) |
| ✅ | Part5-T18 巨组件拆分 | 干净抽取交付(useMediaDragToFolder `5da1c7a` 等);batch-ops/context-menu 深耦合有意不抽 → **▸ 详注 D-4** |

> 📦 本节 ✅ 已完成项的 ▸ 详注(沿革 / 分叉裁决 / 病历 / 教训)已归档 → [completed.md](completed.md)。

### ▸ 详注 D-1:Part5-T11 插件商店**前端 UI**

- **主体已交付**:后端命令均已注册(非阻塞);增量1 数据层(`useExoticStore` + 13 spec)+ 增量2 PluginStoreView(`a68faa0`:独立视图 `/plugins` + 侧栏入口,目录刷新 / 处理进度 / 插件卡安装升级修复激活卸载,复用 ExoticActivateDialog)。
- **~~dev registry 占位域名无真实数据 → 安装链路未 E2E~~ → 已解锁**(2026-07-03 `961360d`,dev registry 全链路测试基建):
  1. `scripts/exotic-dev-registry.mjs` dev 签发工具(Part8 D1 本地原型)——dev Ed25519 密钥对 + keyset、psd-worker 签名 zip(store 法)、签名 index(seq 单调),产物落 `.dev-registry/`(gitignore 含私钥不入仓、cargo clean 不波及)。
  2. host 三处 **debug-only** 旁路(SEC-02 姿态,同 EXOTIC_PSD_WORKER_PATH):download 引擎 `file://` 传输(只换传输,size/sha256 校验保留)、registry scheme 白名单放行 `file://`、`exotic::trusted_keyset()` 统一信任根入口支持 dev keyset 注入(builtin 三消费点收敛)。双重门控 = `#[cfg(debug_assertions)]` 编译期 + 环境变量运行期显式开启,**Release 构建旁路整体不编入**(cargo check --release 零警告实证)。
  3. installer.rs `#[ignore]` 产物核验测试:生产同套校验器全链(keyset→index 验签→zip verify_and_extract)**实跑通过**。
- **用法**:跑生成器 → 同 shell 设 `PICASA_EXOTIC_DEV_FILE_URLS=1` + `PICASA_EXOTIC_DEV_KEYSET=<dev-keyset.json>` + `PICASA_REGISTRY_BASE=file:///<repo>/.dev-registry` → tauri dev(授权 gate 另加 `--features exotic-dev-fixtures`)。
- **待用户在环:商店 GUI E2E**(刷新目录 → 列出 PSD → 安装 → gate/激活 → 卸载;注意先卸载 DB 里的孤儿安装记录——2026-07-03 发现 exotic 安装目录整体消失而记录仍在,即 psd hash_mismatch 重试噪音根源)。真实 CDN + 生产私钥仍归 Part8。

### ▸ 详注 D-2:Part5-T12 插件 gate + 购买引导

- **全交付**:增量1 数据层(`f65635f`)+ 增量2 PluginGate(`e9dc6f7`)+ 增量3a useExoticGate 数据层(`d127424`)+ 增量3b 详情覆盖层接线 gate + ExoticActivateDialog(`05b5c01`)。
- **主触点 = 详情覆盖层**(低回归加性分支)。
- ⚠ 缩略图格 gate **有意 defer**(虚拟滚动高回归 + 小格 UX 差);dev catalog 空 → 接线未覆盖 E2E,验证限 vue-tsc + 单测 + 审查。

### ▸ 详注 D-3:Part5-T13 离线 UX

- **已就位**:①灰显 + 角标(MediaThumb)+ ④事件联动 + 详情覆盖层重连恢复(`cac6599`)。
- **本会话补齐 ②③**:
  - 后端(`6583331`):加 `AppError::VolumeOffline` 变体(接入 get_companion_video_url)+ 卷管理命令(list 含在线态·媒体数 / rename / forget,DB DAO 早备,+4 测试)。
  - 前端(`<hash>`):已知卷面板(设置页,在线徽章 + 内联改名 + 忘记确认)+ VolumeOffline 弹「请插入设备」。
- **有意 defer**:per-volume rescan(映射多 scan_root)/ relink 缺失文件(content_hash,Part2)。
- **验证**:cargo test 277 + vitest 103 + build 绿;⚠ 卷插拔依赖真实设备,未 E2E。

### ▸ 详注 D-4:Part5-T18 巨组件拆分

- **干净抽取已交付**:useGridFlipReflow + useViewportDimPriority + **本会话 useMediaDragToFolder**(拖图到文件夹树,`5da1c7a`,MediaGrid 1798→1655)。
- **剩余有意不抽**:batch-ops / context-menu / row-height anchor 均**深耦合 / 交互密集**——抽 = coupling-relocation 退化设计 + 改滚动/拖拽核心无法运行时验证(记忆判定)。
- ✔ **拖图抽取已人工手测通过**(左键移 / Shift 复制 / 右键落点菜单 / 幽灵跟手 / 高亮 / 尾随 click 抑制),行为等价性运行时确认。


---

## D2. 特色功能实验 · 横向滚动画廊(H-Lab)

### ▸ 详注 D2:H-Lab 滚轮手感演进(v1 → v1.6,六轮手测)

> 一条自研滚轮动画的迭代史:自研 → 撤回原生 → 再复活自研 → 收敛到帧积分。中间态均已被后续版本推翻,此处完整保留每轮根因与修法。

- **v1(`e0720c6`,2026-07-02)**:三横向布局算法(paged / lanes / columns)+ 横向虚拟滚动 + 实验室视图。
  - Rust `layout/horizontal.rs` + 独立 `hcache` + 2 IPC(零生产耦合,生产仅 4 处最小接触);前端 `/hgallery-lab` 路由 + `useHVirtualScroll`(滚轮转译)+ 参数即调即算控制条 + 工具栏烧瓶入口。
  - 验证:cargo test 319 + clippy + vitest 160 + vue-tsc + build 全绿(**仅本地**);⚠ 滚动/滚轮/键盘 UI 行为不在自动化内,待手测。

- **v1.1(`554405d`)一轮手测**:① paged 每屏未拉满视口高;② 横滚掉帧。
  - ① layout_paged 两阶段重构(断行决策取对数失真小者 + 整页行高缩放贴底,cover 裁切吸收小幅纵横比偏差,末页 <0.6H 不拉伸)。
  - ② 滚轮平滑动画(目标值 + rAF 指数趋近,外部滚动让位)+ 去逐格合成层(translate3d/will-change → left/top + contain,掉帧主因:可视 40-50 逐格层 vs 生产逐行 10-15 层)+ 滚动中关格子 pointer-events。
  - 验证:cargo test 320 + clippy + vitest 160 + vue-tsc + build 全绿(**仅本地**);流畅度为观感指标,待用户复测。

- **v1.2(`4c47af1`)二轮手测**:逐格慢滚「一波一波」脉动致眩晕。
  - 滚轮动画由**指数趋近**(25%/帧,单格输入前快后慢速度尖峰 = 波浪根源)改**定时长线性重定标**(起点=当前位、目标累进、恒速走完 wheelAnimMs;稳定逐格输入链成近恒速,对齐原生模型)。
  - 时长做成实验滑杆 80-320ms(默认 160)供实测定值;二轮手测同时确认 v1.1 流畅度修复有效。
  - ⚠ 三轮手测现方向乱跳后曾整体撤销(v1.3);五轮定性:缺陷在**未钳制 t 的实现**而非模型,模型于 v1.5 复活 + 锁测。

- **v1.3(`295ce89`)三轮手测**:滚轮向下画面左右乱跳(方向不确定)。
  - 根因 = 自研重定标动画中 rAF 回调时间戳可**早于**输入侧 performance.now() 时钟,插值系数 t<0 写出目标**反方向**位置(滚越快回跳越大)。
  - 按用户裁决**撤销全部自研滚轮动画,回归原生**:onWheel 只做轴转译 + `scrollBy({behavior:'smooth'})`(与键盘翻页同一原生动画器,方向单调由浏览器保证);删 wheelAnimMs 滑杆 / i18n 键 / eslint 白名单 'ms',净 −55 行。
  - 验证:eslint + vue-tsc + vitest 160 + build 全绿(**仅本地**)。⚠ 裸 scrollBy(无簿记)被四轮手测推翻,见 v1.4。

- **v1.4(`47d7f01`)四轮手测**:极快连滚从极慢开始、滚很短即停。
  - 根因 = 裸 scrollBy smooth 的**取消语义**:每次调用丢弃上段动画未走完的剩余距离、从当前位置重起(WebView2 实测不累积),快速连滚被连环取消吞掉滚动量 + 每次重启落回 ease-in 起步段。
  - 修复 = **目标累积簿记 + 原生 scrollTo smooth**(唯一状态 = 一个可空目标值,零 rAF/时序/插值;target += dy 后 scrollTo,以原生动画器复现原生滚轮重定目标语义)。防陈旧目标两道边界:仅目标在滚动方向前方才累积(方向正确性由构造保证)+ 滚动空闲 160ms / 非滚轮导航即作废。
  - 验证:eslint + vue-tsc + vitest 160 + prettier + build 全绿(**仅本地**)。⚠ 原生动画器 ease-in-out + 重定标零速起步被五轮手测推翻(启动缓入迟滞),见 v1.5。

- **v1.5(`694e78d`)五轮手测**:快速启动缓入迟滞跟手差。
  - 根因 = 原生程序化平滑滚动是 ease-in-out、重定标零速起步、且浏览器无缓动控制 API——**原生路线触顶**;同时定性三轮方向乱跳为**未钳制 t 的实现 bug 而非模型缺陷**(当时整体撤销属误判)。
  - **复活定时长线性重定标**:新 `createWheelAnimator` 模块级工厂(raf/时钟可注入,R2-5 seam 纪律),t 双端钳制 + 方向前方性累积 + 定时长必然终止三道构造性约束。
  - **补确定性锁测 10 个**(线性插值 / 钳制 / 乱序时间戳方向单调 / 累积 / 反向重基 / 外源重同步 / 边界 / stop / null 容器——三次翻车命门路径按「测点由风险决定」补锁)。
  - 验证:eslint + vue-tsc + **vitest 170**(+10)+ prettier + build 全绿(**仅本地**)。⚠ 锚点插值实现被六轮手测推翻(极快启动顿滞),见 v1.6;常规节奏六轮确认基本正常。

- **v1.6(`c33d797`)六轮手测**:极快启动顿一下再正常(常规启动已正常)。
  - 根因 = 锚点插值每次输入把插值锚拉回当下,输入频率高于帧率(飞轮 2-8ms 格间隔 < 16ms 帧)时每帧有效插值时间塌缩为几 ms、位移仅剩余距离 1-3%——极快起手形同冻结,节奏回落后恢复。
  - 修复 = **帧积分**:输入只更新「累积目标 + 截止线」,位移按帧间隔积分 Δx = 剩余 × 帧dt / 剩余时间;无输入时与锚点插值数学等价(**既有 10 锁测不改一字全过**),高频输入不再偷走帧时间。
  - +1 锁测「高频输入速度不塌缩」(首帧 37px vs 旧 10px,断言精确值);验证:eslint + vue-tsc + **vitest 171** + prettier + build 全绿(**仅本地**)。

- **手动验证清单(plan §5)**:三模式切换 / 平滑横滚 / 缩略图批量请求 / 参数防抖 / 极端样本 / **极快启动无顿滞即时满速 + 启动跟手 + 方向恒正确 + 慢滚不晕 + 连滚不丢量**(七轮复测重点,现为帧积分式线性重定标)/ A 页底贴满(复测)。需带真实图库跑 `tauri dev`,用户在环;一轮(掉帧+页高)、二轮(波浪脉动)、三轮(方向乱跳)、四轮(连滚吞量)、五轮(启动缓入)、六轮(极快启动顿滞)反馈均已修。


---

## F. 审查修复(2026-07-02 Fable5 审查产出)

> (状态板整迁自 todo.md,2026-07-10 文档治理「整节收官整节搬」)

> 任务详情/修法/验收见 [审查修复与收敛计划](refactor_2026/2026-07-02-审查修复与收敛计划.md)(§3 任务表 + §4 文档修缮清单);
> 证据见 [审查报告](reviews/2026-07-02-Fable5全面审查报告.md)。本节为滚动状态源,完成即勾并回填 commit。

| 状态 | 任务 | 现状结论 |
|---|---|---|
| ✅ | R0-1 CI test 门控(cargo test + vue-tsc/vitest 入 ci.yml,扩 PR/dev 触发) | `d9c0436`;顺带修私有 CI 连红(Rust job 补 npm ci 材料化 DirectML 资源,同 oss-gate 五跑药方)。本地预演 check --locked/clippy -D warnings/test 全绿;**CI 实跑已实证**(2026-07-02:dev 每推必跑,`d1430dc` run 28579149102 全绿) |
| ✅ | R0-2 WIC 注册进 EngineArena(解锁 HEIC)+ windows dep target 限定 + cfg(windows) 门控 | `038051d`;arena 分发顺序锁测试 + env 门控真实解码测试(Nokia HEIF 样张本机实跑通过)。Linux 唯一硬错误面(wic_engine)消除,gate 回迁 ubuntu 待本改动经同步落公开仓后(C 节行);GUI 级手测 ✅ 2026-07-06(HEIC 缩略图+查看器大图正常) |
| ✅ | R0-3 store_layout 版本号入写锁临界区 | `3c2a4d1`;+8 线程并发回归测试 |
| ✅ | R0-4 缩略图三处改 tmp+rename 原子落盘 | `c9c43de`;write_atomic 助手 + 3 单测(成功无残留 / 原子顶掉截断旧文件) |
| ✅ | R0-5 SOFT_DELETE/RESTORE 迁 invokeIpc + 失败 toast | `94ab03a`;删除失败不进暂存集防前后端状态分叉;MediaGrid 28 个 lint 报错均存量(归 R1-7/R2-3) |
| ✅ | **R1-1 entitlement 激活族收敛到 swap 点** | `e2723cb` + pro 侧 `4997bf1`;方案(a):activate/deactivate 入 trait(fail-closed 默认实现)+ ActivationInfo(Debug 脱敏)+ activation_unsupported 错误码;命令层三处直构改走 `state.entitlement_provider()`。337 测试全绿(仅本地)。**③b 解除阻塞**;验收 grep 唯一命中=swap 点本身(字面归零属 ③b 下沉时) |
| ✅ | R1-2 S4 收尾:批量命令后端迁 SelectionDescriptor | `718a1e8`;5 命令迁描述符 + 分块批量写 + wire 契约定形(变体字段 camelCase + recent_only 补漏)+ 前端 toBackendDescriptor/useViewDescriptor;340 测试全绿(仅本地)。**范围修正**:materializeIds 不删——移动/复制/加收藏夹/删除暂存撤销/框选基线确需具体 id(注释已改写职责);语义搜索视图回退物化;删除走 Explicit 描述符(暂存 UX 需 id,SelectAll 深迁移留 T18) |
| ✅ | R1-3 IPC 命令 spawn_blocking 迁移 | 18/18 文件全部完成;CI 门控测试首跑即抓 3 处前批漏网 → **▸ 详注 F-R1-3** |
| ✅ | R1-4 InstallError 补 code() + 泛码透传 | `1c98833`;install_failed 改全变体细码透传(Manifest 委托 PackageError::code);rollback/uninstall 保持步骤级码(本就可区分);+1 锁测试,341 测试全绿(仅本地) |
| ✅ | R1-5 布局缓存失效契约显式化(**裁决变更**) | `ca22c66`;原「删除后 bump version」作废——与 T18 暂存删除 UX 冲突(bump 令暂存窗口取行失效滚动破图);改为 cache.rs 契约正文注 + 2 条 SQL 安全网锁测试(SelectAll 排除已删 / 批量写对已删 no-op) |
| ✅ | R1-6 T16 方案B 里程碑决策 | **用户已拍板(2026-07-02):话术不降级 → 方案B 正式排期**,第三态消除;实施项见下行 |
| ✅ | **T16 方案B 实施**(bucket 分段虚拟化) | **全量收官(2026-07-06)**:B0-B3.2 全落地 + 真机验收 ✅,bucket 已转默认引擎(`9c8a61c`);B3.2.1 闪烁修复(`d56a3e0`)后四点轻量回归真机全过(冷启动默认 bucket+自研条/拇指单次淡出/回退零位移/双引擎卡片交互一致) → **▸ 详注 F-T16** |
| ✅ | **Part2 布局重排提速**(S1/S2/S4+S1.1 起) | 百万库交互重排 5-7s → ~500ms(10 倍+);冷启动 MISS 用户裁决搁置 → **▸ 详注 F-Part2** |
| ✅ | R1-7 i18n 清扫 + ESLint 裸字符串规则 | `3d6f2d8`;实测 431 处/43 文件,locale +297 键,bare-strings 归零 → **▸ 详注 F-R1-7** |
| ✅ | R1-8 可访问性底线(卡片键盘激活 / dialog 语义 / aria-label) | `8215629`;卡片 role=button + 双弹框 dialog 语义 + 107 图标 aria-label;键盘流程手测 ✅ 2026-07-06(Tab 焦点环/Enter·Space 激活/弹框 Esc+焦点不逃逸) → **▸ 详注 F-R1-8** |
| ✅ | R2-1 文档修缮工程（W1–W12 正文回写,含废止 overlay 定 Copybara 单轨） | `dc8da36`;**12 项/8 文件/66 处**(原估「10 处」)——Part2 方案A→B/T9 时区/T5 三环三大横幅 + 回声清理;Part6+Part0 overlay **正式废止**(Copybara INTERNAL 剥离单轨,与 Part6_3c 对齐);完成度快照补过时头 + 推翻清单;W8 主体前期已回写仅补具名 shm 指向;canonical 守卫绿 |
| ✅ | R2-2 死代码清理（rowCache/fetchRows/justifiedLayout.ts;VectorStore 去留决断） | `fb3bedd`;前端死链三件套 + GET_LAYOUT_ROWS 常量 + justifiedLayout.ts(112 行)删净,显示路径零变更(itemPatchSignal 机制保留);**裁决:VectorStore 标 dormant(有主)**——启用条件(Part4 worker 化接线 / 达 ANN_THRESHOLD 或 p95>300ms)写入模块头注,不得以「未使用」清理;vue-tsc + vitest 109 + cargo test 全绿(仅本地),ESLint 33 存量零新增 |
| ✅ | R2-3 MediaGrid 16 处 `as any` 清零 + 存量 33 lint 清零 → eslint 上 CI | `1d030a0`;`as any` 全清 + v-memo 删除修隐患 + ci.yml 加 ESLint 硬门(CI 已实证)→ **▸ 详注 F-R2-3** |
| ✅ | R2-4 uiStore 9 个启动配置读并入 GET_STARTUP_CONFIG | `f7e26f9`;后端批 4→14 键(含 first_launch),uiStore 单次 startupConfigPromise 共享给 App.vue——**启动配置 IPC 11 次→1 次**;各键解析守卫原样保留(含 pinned back-compat 时序);顺手修 R2-2 漏网 clippy doc_lazy_continuation + `33e0de2` 清 crates/* fmt 存量漂移;eslint/tsc/vitest 109/cargo test/clippy 全绿(仅本地) |
| ✅ | R2-5 补测:useVirtualScroll 坐标数学 / useRequestQueue 槽位 / supervisor 生命周期 | `7695740`;**+63 测试**(30+21+12)→ **▸ 详注 F-R2-5** |
| ✅ | R2-6 Medium/Low 扫尾(全表 UPDATE 分批 / 目录树聚合 / stats 合一 / 裸 invoke 余量等) | `63f5511`;六项全落 + 7 characterization 测试 → **▸ 详注 F-R2-6** |

> 📦 本节 ✅ 已完成项的 ▸ 详注(沿革 / 分叉裁决 / 病历 / 教训)已归档 → [completed.md](completed.md)。

### ▸ 详注 F-R1-3:IPC 命令 spawn_blocking 迁移

**18/18 文件全部完成。**
- **命名清单**(`6ba13ff`)+ **批次 1-3**(`fb3f1e8` / `f5fc18a` / `e3b184e`):blocking.rs 共享助手 + 12 文件。
- **尾批(本次)**:
  - ai_commands:launch/pause/stop 标志位落库、engine 重载、GB 级模型 fs::copy、下载 sha256 整读、registry 组装 fs 扫描。
  - scan_commands:add_root 卷探测 syscall、clear_database 的 VACUUM + 删缓存目录。
  - file_ops_commands:move/copy/relocate/hard-remove 整体下沉——SQL 与 fs 交织改 std::fs 单闭包、move_directory 子树事务 + 缓存重定位、trash 回收站 syscall。
  - thumbnail_commands:批查 + exotic 路由段。
  - exotic 安装族:install 解包/hash、repair 逐文件复核、rollback 目录换回、uninstall remove_dir_all;resume 统一置于 join 后防 paused 泄漏。
- **断言落地为 CI 门控测试**(blocking.rs `ipc_commands_keep_rusqlite_off_async_workers`,「最近标记」启发式 tripwire)——**首跑即抓 3 处前批漏网**(set_rating/set_color_label 直锁 db_writer、download_face_model 配置读)已修。
- **验证**:344 测试 + clippy -D warnings 全绿(仅本地;CI 由本次 push 验证)。

### ▸ 详注 F-R1-7:i18n 清扫 + ESLint 裸字符串规则

`3d6f2d8`
- **实测 431 处 / 43 文件**(报告 ~217 为模板口径);三阶段多 agent 工作流(14 清单 → 单点合并写 locale → 15 并行改写,文件分区互斥)。
- **locale +297 键**(402→699,zh/en 逐键对齐)。
- **顺带修存量 bug**:contextMenu 等 5 缺键致 UI 渲染键路径、`t('x') || '中文'` 回退因缺键返回键路径 truthy 从不生效(8 处删净)。
- `vue/no-bare-strings-in-template` 上规 bare-strings 归零 + 新增 localeIntegrity.spec(键树对齐 + 字面键存在性,防回潮);router 切语言即时刷新标题。
- **验证**:vue-tsc 0 err + vitest 109 全绿(仅本地)。
- **有意保留**:语言自名 2 / console 7 / 未渲染 history label 6 / classicMode 模式名 1。
- **lint job 上 CI 移交 R2-3**(存量 33 错清零后)。

### ▸ 详注 F-R1-8:可访问性底线(卡片键盘激活 / dialog 语义 / aria-label)

> 🔴 **2026-09-04 已推翻**:应用户要求移除读屏/ARIA 语义(aria/role/alt/live region 与 reduced-motion 适配);键盘激活、焦点陷阱与 inert 焦点隔离保留。本节仅作历史沿革留存。

`8215629`
- **媒体卡**:role=button + tabindex + 按类型 aria-label + aria-pressed,Enter/Space 复用点击语义(`.self` 防内嵌按钮冒泡双触发,v-memo 零变更,焦点环走 reset.css 全局 `:focus-visible`)。
- **弹框**:Confirm/CloseConfirm 双弹框 dialog 语义 + 自研 useFocusTrap(入框 / Tab 循环 / 焦点归还,纯逻辑 4 单测)+ CloseConfirm 补 Esc。
- **图标按钮**:25 文件 107 个补 aria-label(镜像 title 键,零新键)。
- **验证**:vue-tsc + vitest 109 + bare-strings 0 全绿(仅本地)。
- **键盘流程手测待 GUI 会话**:Tab 进网格见焦点环 → Enter 开详情 → Esc 关 → 选卡删除弹确认 → 焦点自动落「取消」→ Tab 环内循环 → Esc 关焦点归还;NVDA 报读卡片类型 / 按钮名。

### ▸ 详注 F-R2-3:MediaGrid `as any` 清零 + 存量 33 lint 清零 → eslint 上 CI

`1d030a0`
- **`as any` 全清**:LayoutRow 本是判别式联合,模板 4 处直删 + script 判别式收窄,原案 normalRows 访问器不需要。
- **v-memo 删除 = 修隐患**:嵌套 v-for 内 memo 缓存槽被外层各行共享从不生效;deps 缺 item.id,grid 模式同尺寸空卡有 vnode 串位风险,模板留注防回加。
- thumbhash 死变量清 + decode 游标副作用保留;eslint ignore 补 `target/**`(workspace 化后 81 个 codegen 伪错)。
- ci.yml 加 ESLint 硬门(含 bare-strings 防回潮)。
- **验证**:lint 0 + vue-tsc 0 + vitest 109 + build 净(仅本地 → **CI 已实证**:`d1430dc` run 28579149102 全绿,含 eslint 硬门)。

### ▸ 详注 F-R2-5:补测 useVirtualScroll 坐标数学 / useRequestQueue 槽位 / supervisor 生命周期

`7695740`,**+63 测试**(30 + 21 + 12)。
- 前两者 node 环境零 DOM / 零生产改动(仅 resolveSafeMax 加 export);useVirtualScroll 为 **pre-T16 特性化锁定**(含刻意保留的 wart,T16 重写时整体作废)。
- supervisor 引 **ChildHandle trait seam**(1:1 委托,env-gated 冒烟继续覆盖真 Child)锁 kill / reap / Drop / shutdown 四分支。
- 三路只读 agent 侦察产出契约,逐条源自代码实读。
- **验证**:vitest 160 / cargo 302 / clippy / fmt 全绿(仅本地)。

### ▸ 详注 F-R2-6:Medium/Low 扫尾(全表 UPDATE 分批 / 目录树聚合 / stats 合一 / 裸 invoke 余量等)

`63f5511`,**六项全落**:
- **分批 UPDATE 签名改收 `&Mutex<Connection>`**(竞争在 Rust Mutex 层,批间放锁;sync 加不一致谓词常态零写)。
- 目录树 tagged-CTE 去相关化。
- stats 单扫描 FILTER 合一(口径逐字保留)。
- exotic token 毒锁 4 处统一 into_inner。
- sha256 五处收拢 utils::hash。
- mediaStore/uiStore 裸 invoke 23 处清零(消同文件半迁移漂移)。
- +7 characterization 测试;cargo 309 / clippy / fmt / tsc / eslint / vitest 160 全绿(仅本地)。

### ▸ 详注 F-T16:T16 方案B 实施(bucket 分段虚拟化)

- **状态 ✅ 全量收官(2026-07-06)**:B0–B3.2 全部落地并真机验收 ✅ 通过;bucket 已转默认引擎(`9c8a61c`,方案 A = 回退开关),行模板已抽公共组件(`19c8845`);B3.2.1 闪烁修复(`d56a3e0`)后四点轻量回归 2026-07-06 真机全过。归 Part2。范围见 [T16 评估](refactor_2026/T16_方案B_bucket分段_可行性与改造范围评估.md)。

**沿革(阶段时间线)**

- **B0**(`73cbab6`)→ **B1**(`bf30e5c`,语义边界 + IntersectionObserver,**已废**)。
- **B1 真机反馈**(7 万项 / 上千子文件夹):滚动条远跳白屏 1-2s、选择模式卡顿。**四根因诊断**:
  1. 取数风暴无优先级,终点段排队尾。
  2. 飞掠段应答 leave 后落地成幽灵挂载、DOM 无界驻留。
  3. 内联函数 ref × 数百段 × 每滚动帧 observe churn。
  4. 整段挂载无上界,date 单月段可达数万 px。
- **B1.5 整体重写**(`2e36b76`,用户拍板 2026-07-04:按实际高度等分段,弃语义边界)——**现行形态**:
  - 等高算术分段(SEGMENT_PX=4000 ≈ 2-4 屏;段边界 = 0,S,2S… 与日期/目录语义无关,get_bucket_rows 半开区间天然支持任意边界、后端零改动)。
  - 三种分组(date/folder/none)**统一覆盖**(B1 不覆盖 none 的洞随之补上)。
  - 可见段 = 纯算术(scrollTop/S)→ **IntersectionObserver 与全量占位 div 整体删除**(根因 ③④ 结构性消除),只渲染愿望窗口 1-3 段。
  - **愿望清单 + 单飞取数泵**(至多 1 个 IPC 在途,出队按距视口中心最近重挑,飞掠段自然跳过、终点段最先取——修根因 ①)+ 应答落地三重复核(同代 + 同对象,离窗应答一律丢弃——修根因 ② 幽灵挂载)。
  - loading 段骨架条纹替代白屏;段大小恒定 → 超大单桶(Immich #28861)构造性消失,**B3「二次分段」整项作废**。
  - 沿 B1 保留:一键开关双引擎(方案 A `enabled?()` 门控零删除)、切换滚动位保持、可视项 patch 统一 activeRows()。
  - **验证**:vue-tsc / ESLint / vitest 187 / cargo 373 / clippy / build 全绿(仅本地)。**B1.5 真机验收 ✅ 通过**(2026-07-04 用户确认:恢复流畅瞬出)。
  - 遗留分工——**B2 剩**:FLIP 静默降级、useViewportDimPriority 仅方案 A、行模板抽公共组件;**B3 剩**:>16.7M 段级坐标映射(粗粒度 ~10M,与细段取数层正交)。

- **B2**(`d084f28`,2026-07-04):审计结论 = 选区/拖拽从设计上引擎无关、零改动(区间走 flat_ids、命中走 data-item-id/data-dir-id、幽灵命令式定位)。实际迁移仅两处:
  1. FLIP 根引擎感知(bucket 用段容器 bucketContentRef)+ **whenSettled 段稳定屏障**(bucket 行数据在版本换代后异步回填,FLIP 的 Last 快照须等愿望窗口段全部落地否则动画静默失效;error 段视为稳定不无限等、引擎停用释放等待者防悬挂)。
  2. useViewportDimPriority 行源引擎感知(bucket 喂已挂载段行)。
  - +3 whenSettled 测试(vitest 190 全绿,仅本地)。**B2 真机验收 ✅ 通过**(2026-07-04:框选跨段 / Shift-range / 拖图 / 删除动画)。

- **B3**(`7f8092b`,段级坐标映射,解除总高上限):>16M 进映射态——spacer 封顶 16M,段物理位 = start − anchorDelta。滚动语义分层:
  - 局部 1:1(滚轮/惯性零干预,保 B1.5 手感)。
  - 单事件大位移(Δp > max(3屏, 6000))全局线性重锚(拇指 ≈ 库内比例)。
  - 停稳原子偿债(同帧改 delta + scrollTop,内容零位移仅拇指归位)。
  - scrollToLogicalY 统一五处跳转入口(scrubber / 文件夹 / 锚点 / 缓存恢复 / 引擎切换),scrollCache 在 bucket 模式改存逻辑 y;+8 映射态测试,首跑抓获并修复「边缘偿债同步段污染当帧逻辑位」时序 bug(测点写负载形态的红利)。
  - **B3 映射态首轮真机验收 ❌ 边缘不合格**(2026-07-04:到顶/到底还能继续滚、一跳一跳出内容、显示不全)。

- **B3.1**(`cce30c1`,边缘重修):两根因 = ① 重锚只认单事件巨跳,慢拖滚动条被误判 1:1 → 拇指到底而逻辑远未到底;② 物理钉边「立即偿债」在手势进行中改 scrollTop,与拖拽/惯性互搏 → 反复弹跳 + 愿望窗口抖动(骨架闪烁 = 显示不全)。修法:
  - **输入源分类**:wheel/touchmove/滚动键盖 1:1 印记(印记内手势局部 1:1),无印记滚动链 = 滚动条拖动 → **逐事件**全局比例重锚(拖到边 = 逻辑边,构造上无钉住);手势链沿用起点分类(惯性/smooth 动画不误判)。
  - 物理钉边后原生无 scroll 事件,由 onWheel 推锚差 1:1 续滚到真逻辑边缘、到边硬停;**删「立即偿债」**——偿债仅停稳后,手势中零 scrollTop 写入;Home/End 有意不盖印记,走巨跳兜底恰落逻辑边界。
  - 映射态测试 8→11(慢拖比例回归锁 / 钉顶钉底续滚与硬停 / 程序化印记;vitest 201 全绿,仅本地)。
  - **B3.1 在环验收(真机,dev 通道)**:devtools 执行 `localStorage.setItem('picasa.debug.bucketSpacer','2000000')` 后刷新 → 7 万项库即触发映射态——验:① 滚动条快拖 + 慢拖到底/到顶落点 = 库底/库顶,到边即停;② 拖动中拇指位置 ≈ 库内比例;③ 滚轮/触摸板手感与非映射态无差,滚到物理边后内容平滑续滚至真边界并硬停;④ 停稳后拇指微调而内容纹丝不动;⑤ Home/End 精确落顶/落底;验完 removeItem 恢复。
  - **百万库真环通道(2026-07-04)**:mock_data 扩展 `--dirs/--days`(目录 = seq%N 与日期 = 连续块正交分布;天内偏移压在 target/days 秒小窗,不跨自然日 → 每日条数精确均匀、时区无关),已灌 `Mock 1M` root(C:/MockPhotos1M:100 万条 / 1000 目录 × 每目录恰 1000 / 1000 日期 × 每天恰 1000,SQL 按 UTC + 本地双口径实证,cache_key 零重复);总高 ~30M px,**免 debug 覆盖即真实进映射态**,三种分组均有 1000 桶可测。**分叉记录**:采 DB mock 而非生成百万真实文件——分钟级灌库、零图片磁盘、独立 scan root 应用内可整库删除;缩略图为占位(文件不存在),不影响 T16 所测的布局/滚动/取数路径,真实缩略图形态仍以 7 万真库为准;缩略图队列嫌吵可复跑加 `--thumb-status failed`。
  - **B3.1 真机复验 ✅ 基本通过**(2026-07-04,百万库映射态)→ **遗留**:急速滚动时原生拇指反复回跳(用户初疑滚动条高度重算)。诊断:非高度重算(spacer 恒定)——1:1 滚动令原生拇指超前其比例位,停稳偿债拉回 scrollTop 即回弹;debug 覆盖 2M 未清时 ratio=15 回弹 93%(表现「回到顶」),清除后 47% 仍可感(用户实测证实两级幅度);原生拇指只跟容器物理 scrollTop/scrollHeight,浏览器无百分比映射钩子,保留原生条无法根治(映射态「内容 1:1」与「拇指恒等比例」不可兼得,偿债即两套坐标系的对账)→ **用户拍板自研**。

- **B3.2**(`4ec898b`,自研逻辑滚动条):bucket 引擎隐藏原生条(`--scrollbar-width` 槽位转对称右 padding,引擎切换零布局位移;onMounted 宽度计算改实测双侧 padding 对齐 contentRect 语义),MediaScrollbar 按 currentLogicalY / 逻辑总高渲染纯百分比拇指——与画廊逐帧同步、**永不回跳**,偿债退化为纯后台余量管理零感知。
  - 拖拽 pointer capture + 抓点保持 + rAF 节流,轨道点击直达定位可无缝转拖,跳转统一走 scrollToLogicalY 即时落点;百万级拇指钳 32px 最小高、位置仍按行程比例(几何纯函数 + 6 单测含 round-trip 互逆)。
  - 引擎硬化:偿债定时器竞态守卫(回调先于 clearTimeout 入队且期间有新滚动 → 放弃本次偿债)+ wheel 边缘续滚记入 lastScrollTs。方案 A 不动(非平移态原生条本就准);vitest 207 全绿(仅本地)。
  - **B3.2 真机验收 ✅ 通过**(2026-07-04,用户评价:比原生滚动条更流畅)→ **遗留小疵**:滚动结束拇指深浅频闪。

- **B3.2.1**(`d56a3e0`,显形滞回修复):根因 = 驱动显形的 isScrolling 在收尾抖动——惯性尾梢稀疏事件超 150ms 复位阈值、停稳偿债内部 scrollTop 写又翻一轮;修法 = 显形立即、隐没延迟 700ms 合并,信号再抖也只有一次淡出。

**收尾落地(用户拍板「开始收尾工作」)**

1. **bucket 转默认引擎**(`9c8a61c`):uiStore 默认 true,启动读取语义反转为「显式 `'false'` 才回退方案 A」(历史显式 `'true'` 与未配置新装置都落 bucket),i18n 去「实验」标注。
2. **行模板抽公共组件 MediaGridRow**(`19c8845`,清账原 defer 项):两份逐字节对齐的行模板(分隔符 + 卡片)抽为单源,宿主 handler 以函数 props 直通(签名零重复),两引擎仅 offset-y(段起点 / renderAnchor)与 row-will-change 有别;样式单源于 MediaGrid:行根类靠子组件根节点继承父作用域属性直接命中,内部类 10 条换 `:deep()` 穿透(scoped 编译特异性不变,视觉零差异);孤儿 import 清除。vitest 207 / vue-tsc / ESLint / build 全绿(仅本地)。
3. **轻量回归(真机)✅ 2026-07-06 四点全过**:① 滚动结束拇指只淡出一次不闪;② 冷启动(不动设置)即 bucket 引擎 + 自研滚动条;③ 设置关闭开关 → 回退方案 A 原生条、布局零位移;④ 两引擎下框选 / 拖拽入文件夹 / 删除动画 / 右键 / 悬停等卡片交互原样(抽组件回归面)。

### ▸ 详注 F-Part2:Part2 布局重排提速(S1 / S2 / S4 + S1.1 → S3.x)

- **状态 ✅**;触发 = 百万库启动 / 滑块 / 窗宽 / 开关时间·目录轴每次 3-5s 重排(2026-07-04 用户报告)。
- **病根**:compute_layout 每次全量重跑三段 O(N) 流水线(19 列 × 1M 行双 JOIN + 逐行路径拼接的 SQL → 逐项 chrono 格式化 + 字符串克隆的布局 → 三份 1M 索引物化 + 旧代 drop),且**取数与几何耦合**——滑块/窗宽/轴切换不改 WHERE 却每次重查;DB 副本实测 date 轴 2.5s、folder 轴 5.1s(rel_path 字符串排序)。

**第一波:S2 消脂 + S1 取数缓存 + S4 防抖**(`c33af8b` + `8ebc47e` + `04678c7`)

- **S2 消脂**(`c33af8b`):查询去 dir_path/dir_name 逐行拼接(只取 dir_id,标签经新增 query_dir_labels 10³ 级映射还原);分组边界整数化 GroupMark(date=UTC 日序数 / folder=dir_id,标签仅边界构造,10⁶→10³ 次分配);删死代码 clone;compute_* 泛型化 `&[I: Borrow<LayoutItem>]`。
- **S1 取数缓存**(`8ebc47e`,治本):新增 `layout/items_cache.rs`——
  - 命中键 = filter canonical JSON + 全局 data_version + 序形态;datetime(默认)家族存基准序(sort_datetime DESC, id DESC,经 query_layout_items_canonical 免 JOIN 取回),三轴 × 两向全部内存派生(恒等 / 整体反转 / dir_rank 全键排序)→ **滑块/窗宽/轴切换/时间轴跳转零 SQL**,folder 轴从 5.1s SQL 字符串排序变亚秒内存排序;filename/similarity 按 SQL 序原样缓存;ai_search 视图不缓存。
  - **序等价契约(刚性)**:derive_order 必须与 view_to_sql 的 SQL ORDER 逐项等价(否则 SelectAll/flat_ids 选区漂移)——dir_rank 用「rel_path 去重排秩,同 rel_path 同秩」复刻 SQLite BINARY 序,对拍测试锁 3 轴 × 2 向(fixture 含双根同 rel_path 并列 / 大小写 / Unicode / 同秒 id tiebreaker);谓词单一事实源(push_query_body 中缝拆出 push_where_predicates/push_order_by,View/SelectAll 路径 SQL 逐字节不变)。
  - **失效契约**:AppState.data_version,成员/几何/顺序写路径**提交后** bump(软删恢复 / 文件操作 / 扫描每批 + 收尾 / enricher 三段 / 相册 / 人物归属 / 卷可用态 / 清库,fast_scan 加 on_batch_committed 回调);纯展示写走双缓存 patch 不 bump(缩略图/favorite/rating/color——filter 敏感视图由 items_cache 内部整体失效;可视区尺寸回填 patch 带 0×0 守卫)。
  - **顺手修复**:rating/color 此前不 patch 布局缓存(滚出滚回回退)→ D3 泛化 patch 双缓存成对。锁纪律:两把缓存锁绝不同时持有。HIT/MISS 结构化耗时日志(query/total 拆分)落 tracing。
- **S4 滑块防抖**(`04678c7`):拖动只更新本地显示值,停手 250ms 一次性写 store(原实现每 20px 步进触发全量重排,300ms 防抖只防了持久化)。
- **分叉记录(无人值守授权下自决)**:① ai_search 视图不缓存(每搜索整表重写,失效面不值);② enricher 批提交选 bump 而非 patch(批内还改 sort_datetime 顺序,patch 不完备;代价 = 补全阶段缓存冷,即今日常态);③ 厚载荷列(thumb_path 等)保留在常驻行,几何/载荷分离(S3,内存减半 + 命中再提速)留 Phase B 视真机数据定。
- **验证**:cargo 380(+7:items_cache 6 + 序对拍 1)/ clippy / fmt + vue-tsc / ESLint / vitest 207 / build 全绿(仅本地)。
- **在环(真机,百万库)**:① 冷启动首算(MISS)耗时与日志 query/total 读数;② 拖尺寸滑块停手后单次重排,亚秒;③ 窗口拉伸停止后亚秒;④ 开关时间/目录轴亚秒(日志应报 HIT);⑤ 时间轴 scrubber / 文件夹跳转如常;⑥ 收藏/评分/色标后滚出滚回不回退;顺带回归 T16 收尾清单四项。

**S1.1 真机回归修复**(`0b54018`,2026-07-04 首轮真机回报:HIT 全命中但目录轴下滑块/窗宽/轴切换仍 5.5-7.3s,date 轴同库 367ms)

- **病根** = derive_order folder 轴直接对 `&LayoutItem` 排序,每次比较 2 次哈希查秩 + 2 次 ~200B 大结构体随机访存,1M 项 ≈ 2000 万次比较 ≈ 5-7s(date 轴恒等派生零成本,故 367ms = 布局段健康证明)。
- **修法**:① 装饰-排序-还原(一次线性扫描抽 (rank,ts,id,idx) 进 32B 紧凑元组连续数组再排,比较器零哈希零随机访存);② 排序置换 memo 进缓存体(patch 不触碰排序键,随缓存体 MISS 换代自动失效;同轴同向后续交互免排序 O(N) 还原);③ HIT 日志拆 derive/layout 分段并带轴向。
- 新增 memo 命中/替换测试,序对拍仍绿,cargo 381 / clippy / fmt 零告警(仅本地)。
- **在环复测注意**:②③④ 项须在目录轴开启下复测,预期首次切目录轴 derive 数百 ms、同轴后续交互 derive ~0ms,日志读 `axis= / derive / layout` 分段即可定位。
- **真机验收通过(2026-07-04 二轮,1M 库)**:多次测试交互重排平均 ~500ms(首轮回归读数 5.5-7.3s、原始报告 3-5s → **10 倍+**),用户拍板「可继续推进」。

**Phase B(S3 几何/载荷分离)开工**(分叉自决,无人值守授权)

- **依据**:① S3 本为计划内挂账项「视真机数据定」,数据已到:交互耗时地板落在布局段 make_row_item 克隆风暴(1M × 5 次堆克隆/次重排)+ 旧代 drop(store_layout 换代 5M 次 free,不计入 HIT 日志但计入体感);② 1M 库厚载荷(thumb_path/thumbhash/三小串)在 items_cache 与 layout rows 双份驻留 ~数百 MB,S3 内存减半;③ 慢机/更大库下 500ms 会线性放大。
- **落地**(`d97e20b`,2026-07-04):布局行常驻从 ~200B + 5 段堆载荷/项 瘦身为 32B/项(SlimRowItem = id + x/w/h);厚载荷单份驻留 items 缓存,get_layout_rows/by_y/bucket 三命令出口经 hydrate_rows 按可视区拼装 HydratedRow(serde 形状与原 LayoutRow 逐字段一致,**前端零改动**);重排消除 1M × 5 堆克隆 + 旧代 5M free,新增 store_layout 换代计时日志(索引物化 + 旧代 drop 这段隐性成本此前不在 HIT/MISS 日志内)。
- **架构简化**:D3 布局侧 patch 四组(thumb/favorite/rating/color)整体退役——滚出滚回新鲜度由出口拼装天然保证,patch 单点化 items_cache;exotic sink 双 patch 同步单点化,PipelineDeps 删 layout_cache 死字段。
- **契约变化(已沉淀记忆)**:items 缓存获双重身份(命中源 + 布局行载荷源)——① 视图敏感写从「整体置 None」改「降级 reusable=false」(置 None 会饿死出口拼装破图),值仍 patch 保旧布局展示即时;② ai_search 快照驻留但 reusable=false(只作载荷源不参与命中);③ hydrate 查无 id → 占位行项(几何保留行形不塌,thumb_status=0 骨架,覆盖布局/快照换代竞态与清库瞬态,换代自愈);④ items_cache::invalidate 硬清仅限布局同步清空场景(clear_database 先清 layout 再清 items,已核对顺序)。
- **验证**:cargo 381 / clippy(lib+tests)/ fmt / 全包 check 绿(仅本地)。
- **在环(真机 1M,Phase B 验收)**:① 滑块/窗宽/轴切换重排耗时(预期 ~500ms → 明显下降,读 HIT 日志 layout 段 + 新增 store_layout 日志);② 滚动取行载荷完整(缩略图/星级/色标/收藏/时长角标/离线置灰);③ 缩略图后台生成中滚动即时可见;④ 收藏/评分/色标改后滚出滚回不回退(出口拼装新通路);⑤ ai 搜索视图取行正常(新驻留路径);⑥ grid 宫格模式与 T16 bucket 滚动回归;⑦ 内存占用对比(任务管理器,预期显著下降)。

**S3.x 逐刀提速(2026-07-04 多轮真机反馈驱动)**

- **S3.1 幂等去重**(`08bb2c7`):Phase B 首轮真机回归(用户报告)——多次刷新内存 500-900MB 波动后回落 550MB、体感反比 B 前慢;日志显示每次刷新**两次完整重排**(两 HIT + 两 store_layout,v66→v67 连发 ~1.4s)。
  - 诊断:同参数 compute_layout 被前端连发——候选触发对 = 挂载 compute + {totalItems watcher(loadStats 返回)| resize 防抖(300ms,与日志时间轴吻合)},mediaStore 在飞合并只排队不比对参数;Phase B 前用户只见 1 条 HIT(DEDUP 日志将使一切重复触发显形)。
  - 修法 = **后端幂等短路**:快照可命中(同 HIT 守卫)且 gen_key(filter + 轴三元组 + 几何 + 布局模式 + dv)与现行布局代一致 → 直接返回现行摘要,免重排且**版本不换代**(bucket 段表免虚假重建——双换代此前每刷新两次段表重建 + 重取数,是体感变慢的另一半)。安全性:成员/序变更路径全 bump dv(软删/恢复/扫描/卷插拔/人脸/收藏夹逐一核对)、展示敏感写降级 reusable=false,任一即破幂等;ai_search 天然不参与。
  - 验证:cargo test 383(+2)/ clippy -D warnings=0 / fmt / 全包 check 绿(仅本地),前端零改动。**真机验收通过(二轮)**:刷新两触发均 DEDUP ~0ms(v1 unchanged 连续两条);交互重排剩余成本 = layout 253ms + store_layout 390ms(用户点名优化后者)。
- **S3.2 换代段提速**(`8adf282`):① id_to_flat/id_to_idx 换 FxHashMap + 预扫容量(1M 整数键 SipHash 逐项插入 + 多轮扩容重哈希占换代大头,内部索引无 HashDoS 面;items 侧 MISS 换代同享);② 旧代移出写锁 + 堆释放卸后台线程(写锁窗口只剩取号 + 指针交换,不再阻塞滚动并发读锁);③ store_layout 挪进 spawn_blocking(1M 索引物化不占 tokio worker;锁纪律不变,该段不持 items 锁)。R0-3 锁内取号契约保持。
  - 验证:cargo test 383 / clippy=0 / fmt / 全包 check 绿(仅本地),前端零改动。**真机三轮**:store_layout 390→112ms,用户仍嫌与「数十 ms」预期有差——剩余大头 = 1M 次 FxHashMap 插入在 ~24MB 表上的**随机访存**(哈希已便宜、访存未便宜)。
- **S3.3 密集直址表**(`58421cc`):自增主键 id 域紧凑(max_id≈N)→ id→flat 索引改 `Vec<u32>` 直址(u32::MAX=空位,1M 库 4MB 驻 L2/L3,构建 = 顺序读 + 缓存友好随机写,较哈希插入快一个量级);稀疏域(max_id>4N+1024,大量删除后的老库)自动退 FxHashMap,双形态查询等价(测试锁定);附带索引内存 24MB→4MB。
  - 验证:cargo test 384(+1)/ clippy=0 / fmt / 全包 check 绿(仅本地)。**真机四轮达标**:store_layout 42-51ms(用户「符合预期,可继续推进」),store 段收官(390→112→~46ms 三刀:哈希算力 → 扩容重哈希 → 随机访存)。
- **S3.4 layout 段组间并行化**(`643d797`):两算法结构同构(整数 GroupMark 定组界 / 组内顺序打包 / 组间零依赖)→ 抽公共骨架 layout_groups_parallel(段表 O(N) 轻扫 → rayon 并行逐组打包,组内 y 局部 0 起,commit_row/emit_separator 闭包原文复用 → 组高前缀和缝合);顺序等价:y 均整数和,「基线 + 局部」与顺序累加**位级一致**,新增多组缝合特征化测试(grid + justified 双轴精确 y 序列)锁定;none 分组单段无并行收益(行打包链式依赖),如实维持原耗时。
  - 验证:cargo test 385(+1)/ clippy=0 / fmt / 全包 check 绿(仅本地)。**真机五轮**:用户反馈仍不理想——compute_layout 随行数走高、地板 ~150ms。本地 1M 合成基准立证(release/20 核,新增 `#[ignore] bench_layout_1m`):算法 + 并行本身 18-31ms@10-50 万行——差距在三个顺序残余。
- **S3.5 三刀**(`01816c4`):① commit_row 每行 2 临时宽度 Vec(真机行密度 = 数百万次分配)→ 单缓冲跨行复用、每行分配归零;② median_measured_aspect 每次 O(1M) 重算 → OnceLock 缓存进 items 快照(只依赖项集;尺寸回填有意不失效,漂移仅影响 0×0 占位形状);③ get_summary 每次全行扫描 → 分隔符/月桶随 store_layout 同遍物化,摘要退化 O(分隔符数) 克隆(DEDUP 同收益)。基准同机对比:501k 行 30.8→22ms、107k 行 18→11ms;分配约束更紧的机器收益更大。
  - **真机六轮达标**:layout 段降入 150ms 内;但用户报冷启动 MISS query 段 1.6s→6.2s。

**冷启动 MISS query 段回归(S3.6 / S3.7,最终搁置)**

- **S3.6 启动期 WAL 截断**(`27e38e0`):查证非代码回归(该窗口 db/ 仅一行测试构造点、query 计时起止未动);主嫌疑 = 跨会话状态:WAL 检查点此前仅挂正常退出钩子,dev Ctrl+C/强杀后 WAL 带全天管线写量跨会话累积,首个 1M 查询在巨型 WAL 上逐页查找(次嫌疑:高内存测试挤掉 OS 页缓存 → 首启磁盘冷读)。修法 = create_write_connection 内(唯一连接时刻)wal_checkpoint(TRUNCATE) + 前后尺寸/页数日志——根治 WAL 债并自带诊断证据。
  - **判决(七轮)**:WAL 日志一行未见 + query 稳定 6.6s(跨启动一致)→ WAL 假设部分证伪,但**日志不可见另有其因**(tracing 订阅器在 create_write_connection 之后才初始化——日志级别配置存 DB,顺序不可倒置,checkpoint 的 info!/warn! 被静默丢弃)。
- **S3.7 真病根实锤 + 修复**(`05d937c`):canonical 查询 ORDER BY 吃 idx_media_sort = 索引序逐行随机回表;白天管线把 1M 行 thumb_path/thumbhash/EXIF 回填后表体积数倍膨胀,1M 随机页访问在 64MB 页缓存上颠簸 = 稳定 6.6s(1.6s 是瘦表时代同一查询;与今日布局改动无关)。EXPLAIN 实证仅去 ORDER BY 不够(规划器仍选 partial index,谓词与其 WHERE 逐字匹配)→ 修法 = 去 ORDER BY + 默认全量视图 unary `+` 压制(实证退化顺序全表扫);选择性视图不含压制、各自索引照常;基准序内存置换排序补齐(同 S1.1 DSU 手法,~100ms 量级),与 SQL 序逐项等价。checkpoint 挪至 tracing init 后、管线拉起前。
  - 新测试:计划锁定(EXPLAIN 断言默认视图无 idx_media_sort / 目录视图保持索引;谓词措辞漂移即红)+ 序语义;序对拍在环。验证:cargo test 387(+2)/ clippy=0 / fmt / 全包 check 绿(仅本地)。
  - **修复有效性实证**(`77298a7`,八轮):用户清库后 query 恢复 1363ms 并正确指出瘦表不能证明修复(正式版不可能删库)——本地合成 1M 胖表(thumb/hash 全填 + 时间戳与 rowid 去相关,303MB)同库对跑:old(索引序回表)2564ms vs new(扫 + 排序)659ms = **3.9×**,全程热缓存(冷缓存差距更大);序等价 1M 相邻对全量断言过。基准入库 bench_canonical_fat_table_1m(`#[ignore]`)。注:commit 信息误写「388 过」,实为 387 过 + 3 ignored(基准不计入)。
- **用户裁决搁置(九轮)**:「此问题耽搁太多进度,先搁置,继续推主线」——终局确认降级为**被动观察项**(日后表自然变胖后顺带看一眼冷启动 MISS query 即可,不再占主线轮次;胖表对照基准已实证修复有效,残余风险低);**Phase C(冷启动流式首屏)随之不启动**。
- **主线切换记录**:Part2/Part4/T16 均已收官或仅剩用户在环项,按 todo.md 优先级转投 **Part7-T11 + §3.6.5 渠道物理门控**(🔴 P0、卡 Store 上架、前置 T6/T7-T9/T10/T12 全 ✅、原型已验证、可全程本地验证)。


---

## G. 全景扫描剩余项(2026-07-02 新增)

### G1. 可立即开工(状态板整迁自 todo.md,2026-07-12「整节收官整节搬」)

> 七项全 ✅;各行详注见下方 G1-1…G1-7。

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | **Part4-T6 persons.model_name 隔离** | 已实施(`fa0e951`,328 测试仅本地)→ **▸ 详注 G1-1** |
| ✅ | **Part2 content_hash 三环接线** | 已实施(`8aa7003`,Part2 文档 10 处正文回写)→ **▸ 详注 G1-2** |
| ✅ | **Part7-T5 版本号单一事实源** | 已实施(`ff589da`)→ **▸ 详注 G1-3** |
| ✅ | Part7-T1 余量(tags `v*` 触发 + smoke job + 一致性门控) | 已实施(`ff589da`);smoke job 已实跑 ✅(2026-07-05 dispatch)→ **▸ 详注 G1-4** |
| ✅ | **Part7-T10 渠道 feature 骨架** | 已实施(`bc2706d`,五组合验证)→ **▸ 详注 G1-5** |
| ✅ | Part3-T6 余项(reconcile_orphan_gc + all_cache_keys + 周期任务) | 已实施(`21fa53a`)→ **▸ 详注 G1-6** |
| ✅ | 文档卫生集中回写(六处) | 六处全部回写完成 → **▸ 详注 G1-7** |

### ▸ 详注 G1-1:Part4-T6 persons.model_name 隔离

内容:两命令 + 名册查询全面按模型过滤 + cosine 维度护栏 + 清陈旧注释 + T_t3 四测。

- **已实施**(2026-07-02 `fa0e951`,328 测试全绿仅本地)。
- **分叉裁决**:① 激活设 verified 对拍门(SCRFD 未对拍轨拒绝激活,防静默算错);② 切换不 eager 重载引擎(人脸流水线用户显式启动,避免 CLIP 缺失连累);③ FaceProfile/FaceModelInfo/前端类型加 verified 字段。
- UI 切换入口未接(归 Part5 设置页)。

### ▸ 详注 G1-2:Part2 content_hash 三环接线

内容:(mtime,size) 判据 + SuspectChanged 零写 + 批事务外指纹 + resolve 定案(touch/失效)三环全落。

- **已实施**(2026-07-02 `8aa7003`,Part2 文档 10 处正文同步回写)。
- **分叉裁决**:① **实施而非删列**(设计经 8 轮核验,删列 = 推翻有据设计违 E 节);② **SHA-256 替代 BLAKE3**(离线 cargo 缺 crate,存储带 `sha256:` / `sha256s:` 算法前缀,换算法安全,联网后可升级);③ idx_media_hash 不再算死索引口径(列已激活,索引留未来 by-hash 臂)。

### ▸ 详注 G1-3:Part7-T5 版本号单一事实源

内容:根 [workspace.package] 锚 + src-tauri/free-stub 继承 + 删 tauri.conf.json version(cargo check 实证 Tauri 回退 CARGO_PKG_VERSION)+ sync-version.mjs 四模式 + CI --check 门控。

- **已实施**(2026-07-02 `ff589da`)。
- **分叉裁决**:dirty-tree 检查由 --check 等价覆盖(--check fail ⇔ write 后有 diff),T16/17 落地再加 --write 链;identifier 锁定断言不做,随 R2-7;继承面按 §3.2 字面 = src-tauri + free-stub,plugin-api 等内部件保自有版本。

### ▸ 详注 G1-4:Part7-T1 余量

内容:tags `v*` 触发 + smoke job(tauri build --no-bundle → PICASA_SMOKE_TEST=1 断言 exit 0,钩子实证在 lib.rs RunEvent::Ready)+ tag 与版本锚一致性门控。

- **已实施**(2026-07-02 `ff589da`)。
- **分叉裁决**:冒烟挂 **tag / 手动 dispatch** 不挂日常 push(完整 build windows runner 10 分钟级,T16/17 建成后迁入其中)。
- ~~⚠️ smoke job 本体未实跑~~ → **✅ 已实跑**(2026-07-05 无人值守清账,workflow_dispatch @ `b1b2db2`,run 28731562158 四 job 全绿):tauri build --no-bundle 19m52s + 冒烟断言步 **7s 过**(release exe 在 hosted windows runner 真实拉起、达 RunEvent::Ready 即 exit 0——GUI 应用在 runner 会话可开窗实证);tag 一致性步在 dispatch 场景正确 skipped(条件门对偶面顺带验证)。

### ▸ 详注 G1-5:Part7-T10 渠道 feature 骨架

内容:三 feature(direct 入 default)+ lib.rs compile_error! 双守卫(两两互斥 + 零渠道)。

- **已实施**(2026-07-02 `bc2706d`,五组合验证:三正例绿 + 双负例精准触发)。
- **分叉裁决**:channel-direct 定稿形态 `["dep:tauri-plugin-updater"]` 因离线缺 crate 先空占位(注释已标,T7 联网预取后补);Provider stub 接线归 T12。

### ▸ 详注 G1-6:Part3-T6 余项

内容:reconcile_orphan_gc(epoch 守卫 + 16hex 识别 + best-effort)+ all_cache_keys(HashSet 对账)+ lib.rs 周期任务。

- **已实施**(2026-07-02 `21fa53a`)。
- **分叉裁决(节奏,原 G3 小决策)**:**首跑启动后 5min**(错开 PRAGMA optimize)→ GC → LRU 收敛,此后每 24h;软删行缓存受保护不 GC(可恢复);「全量重建完成后即时触发」未接事件,留微余项。

### ▸ 详注 G1-7:文档卫生集中回写(六处)

对象:审查计划 §3 R0/R1 行状态、选区设计稿「设计待审核」头、T20 设计「未动代码」、Part1 §6 复选框 + CURRENT_VERSION=10 残留、Part3 任务表 T11 与复审「HDR」标签矛盾、Part0 §11.3 ⏳。

**六处全部回写**:
1. R 计划 R0-1~5 补 ✅ + commit(镜像 R1 行格式)。
2. 选区设计稿头改「已实施并交付」。
3. T20 设计补轨道 A(A1/A2)已交付 + §4 几何决策已拍板 + 轨道 B 逐项对照(useMediaDragToFolder / TimelineScrubber / useViewportDimPriority / useGridFlipReflow 已交付,batch-ops / context-menu 有意不抽)。
4. Part1 §6 复选框据实勾选(T9 联网阻塞留空)+ 确认 CURRENT_VERSION 注解本已随 R2-1 补全。
5. Part3 + 完成度审查文档同源「T11 HDR」误标订正为「T11 ai_thumb 联动」(全仓 grep 无 HDR 设计内容,系早期草稿错标)。
6. Part0 §11.3 **实测 3 处非 4 处**(ci.yml / showToast / data-theme,已实测代码确认全部完成,标注与 todo.md 计数口径修正)。

### ▸ 详注 G2-1:Part4 worker 化主线

内容:三份 §8.2 critical 设计 → T9.5 VRAM 原型(合并 vs 分离拍板输入)→ T10-T19 全段(协议 / GpuLimiter / model blob / Coordinator 通用化 / 推理迁 worker / 去 ort / 控制面留主 / ai_thumb 喂 CLIP / 低优先级)。

**前置(拍板 + critical 设计 + T9.5)**

- **2026-07-02 用户拍板:现在全线启动**(改变 §11.4.1「v0.1 后 fast-follow」既定排期);与 C 节 Part6 T3-T8 同波互为生产/消费;为 Part7 <10MB 前提。
- **三份 critical 设计 ✅**(`c7b9691`):D1 两级载荷通道(明文直传路径,shm 仅 AES 后启用)/ D2 🔴 修正 §8.2.2 帧方案 → 令牌全留主进程、协议零扩展 / D3 🔴 修正 §8.2.3 → 握手 5s 不动、300s 归 SessionInit 请求。
- **T9.5 ✅ 实测**(vram_probe:合并 361.6MB@B/16,HQ 上界仍 <4GB → 数据支持合并,正式拍板随 T20 崩溃率半边)。

**T10 协议扩展 ✅**(2026-07-03,连带 Part6-T3 同施工;双 feature 组合 fmt/clippy/test 全绿仅本地;psd-worker 同波重编译)

- PROTOCOL_VERSION 1→2、RequestBody 扩 SessionInit/SessionClose/EmbedBatch/FaceDetectEmbed、WorkerErrorCode +4、capability 常量 + catalog 枚举扩 embedding·face_detect_embed(两端对拍测试)、Success/Failure item 字段 Option 化(thumbnail 线上形状不变,有 wire-shape 测试锁定)。
- **T10 分叉裁决(无人值守)**:
  1. 发现 D3 §3 与 Part6 §3.2.1a(字段权威 + 6 单测原型)冲突——D3 转述 Part4 G 行草稿未核对权威源(「转述须核对权威源」之病又一例),仲裁 = **三源合并**(载运按 D2/D3:op 级零新帧、GpuToken 不进协议;字段按 §3.2.1a:多角色句柄 / EmbedBatch 逐项化 / ai_cache_dir 随 SessionInit;完整性按 D1:逐模型 len+sha256+models_root),D3/Part6/Part4 六处已挂修正横幅。
  2. 修正 §3.2.1a:嵌入向量出 JSON 走 Success 帧 blob(128×768d JSON ≈2.5MB 撞 MAX_JSON_LEN 1MiB + 违「JSON 只放控制字段」天条)。
  3. FaceDetectEmbed 具体化(cache_key? / source_path? 回退对、det_score_thresh 快照、几何 JSON / 嵌入 blob、质量分 host 派生不进协议)。
  4. 快照补 face_profile_id?、SessionReady 补 face_embed_dim?(合并 session 双 profile)。
  5. session_id 不进批量 op(严格串行下冗余)。

**T11 GpuToken ✅**(2026-07-03,三绿仅本地)

- host 侧 GpuToken/GpuPermit 落 exotic/limiter.rs——薄封装复用 BackgroundHeavyLimiter 公平队列(DRY,分叉裁决:不复制骨架、独立类型防 CPU/GPU 池混用 + 两 permit 类型补 `#[must_use]`),额度恒 1、FIFO、取消感知、RAII(panic 展开路径有测试),AppState.gpu_token 共享实例;「先 CPU 后 GPU」顺序天条以类型文档锁定;协议/worker 零改动(D2);acquire 接线随 T13/T15。

**T12 model blob 分发 ✅**(2026-07-03,三绿仅本地)

- RegistryEntry.model_blobs[](serde default 向后兼容 + 数据面校验:HTTPS/sha256/size 0~8GiB/kind/文件名白名单)+ fetch_model_blob(幂等跳过、.part 断点续传保留、校验失败必删、LargeFile 档)+ 安装命令 1.5 步先 blob 后 zip、落 models 目录(D1 Level A 直接可用)。
- **T12 分叉裁决(无人值守)**:① 「豁免压缩比」落地解释 = blob 不进 zip → zip-bomb 检查天然不适用,InstallLimits 零改动(自身防线:HTTPS + size 精确封顶 + sha256 + 白名单 + 8GiB 上限);② ModelBlob 增 file_name 字段(§3.7.1 四字段无落盘名);③ blob 排 zip 之前(失败零清理面,就位权重内容寻址、重试复用不回滚);④ 大文件 .part 失败保留供续传(与 fetch_package 小文件即删刻意不同)。

**T13 Coordinator 通用化 ✅**(2026-07-03 `647891b`,三绿仅本地)

- PluginDescriptor 注册表(capabilities 从 Catalog 取;worker_id/uses_gpu 为运行时支持信息,Part8 manifest 扩展后转全数据驱动)+ 调度循环拆双层(注册表外环 + 单(插件,能力)drained 内环,PSD 硬编码全出调度路径)+ 非 thumbnail 能力显式跳过(T15 接缝,届时按 uses_gpu 走 CPU→GPU 双取)+ per-op timeout 表(coordinator::op_timeouts 四档 + 不变量测试,pipeline::TASK_TIMEOUT 收拢)+ **P0-3 落地**(verify_installed_integrity 协议版本前置比对,protocol_mismatch 码 = needs_reinstall 语义 + 装时合法升版后被拒单测;DB 标记/自动重下载留 Part8)。
- 分叉裁决:CAPABILITY_STR 删除、CAPABILITY 降 `#[cfg(test)]`;timeout 表未消费档位 allow(dead_code) 注明 T15。

**T15 推理核心迁出 ai-worker ✅**(2026-07-03 `6d1ab57` + `403b10f`,三绿仅本地,428 测试双组合;T14 AES 随 ④ 后置跳过)

1. **共享核 picasa-next-ai-core**:engine/clip/face/profile + face_profile 注册表 / provider / DecodedImage / 自有 AiError 迁出;src-tauri 六模块退化再导出薄壳 + error.rs From<AiError> 收敛,全库调用点零改动;进程内路径保留 = T16 过渡双活前提。
2. **ai-worker 合并单 worker**(T9.5 数据,拍板随 T20;Ready 声明 embedding+face_detect_embed):SessionInit 两段式(validate_and_resolve 纯校验可单测:models_root 归属 / 角色完备 / 逐模型 len+sha256(D1 §3)/ Named 拒至 AES ④ → load 声明即必须就绪)、EmbedBatch(cache_key 白名单防越界 / 逐项不连坐 / Ok 项序进同帧 blob f32 LE / 维度红线整批 EmbedDimMismatch)、FaceDetectEmbed(cache/source 回退对 + det_score_thresh 快照覆盖)、空闲自杀 timer(最后一帧后 300s exit(0),D3 §4④)。
3. **host 泛化**:WorkerConn.run_request(RawOutcome)+ run_thumbnail 退化为其 + 专属校验、validate_embed/face_batch_output(「不信任 worker」延伸到 v2:同序同长 / 逐项核对 / blob 精确长度)、Supervisor session 快照 + init_session + close_session(kill 清零 §4⑤)、pipeline trait 拆分 WorkerTask/ThumbnailWorker/EmbedWorker(C5)。
- **T15 分叉裁决(无人值守)**:① 共享核走独立 crate 而非复制进 worker(T16 双活 + DRY,薄壳手法同 ③a);② SessionInit 要求 CLIP 成对必备 + face 成对可选;③ 批超限回 MalformedInput(host bug 早暴露);④ WorkerFactory 不加 embed 方法(T17 按实际派发器形态定,避免无消费者抽象);⑤ host 侧空闲卸载 idle timer 接线随 T17(close_session 接缝已立)。
- 🔴 **发现 v2 协议缺口:无文本编码 op**——T16 删 ort+tokenizers 后语义搜索查询向量无处生成(§8.6「文本塔 worker 内硬编码 CPU」印证文本属 worker 职责),T10 三源合并的三份设计均未列此 op;裁决 = T17 补 additive op(如 EncodeText;v2 无对外部署、psd-worker 兜底臂同波扩),不动 PROTOCOL_VERSION。

**T17 控制面留主进程 ✅**(2026-07-03 `4ef4f71` + `3867a47`,双特性组合 436 全绿仅本地)

1. **EncodeText op**(v2 additive 不升版,T15 缺口收口):RequestBody::EncodeText{texts} + SuccessBody.text_embed(count 双向核对),向量走 Success 帧 blob(texts 序 × embed_dim × f32 LE),全批原子(文本编码无逐项 IO 失败模式)、不新增 capability;worker 端 SessionState 增 tokenizer(声明即必须就绪:vocab 缺失=ModelLoadFailed)+ handle_encode_text(批 1..=64 / 单条 ≤8KiB 防御);psd-worker 兜底臂同波扩;host validate_encode_text_output(count+blob 双校验)。
2. **AiWorkerClient 句柄**(ai/worker_client.rs → AppState.ai_worker):spawn 死亡重建(exe=主程序同目录 + PICASA_AI_WORKER_PATH 覆盖)/ ensure_session(SessionDescriptor 快照比对,不符先 close 再 init,D3 §4②)/ embed_batch·encode_text(op_timeouts + validate_* 消费);**硬止损编码进 client**:同批至多 2 次尝试(进程级异常/输出违例 → 重建重发一次;SessionExpired → 重 init 一次;terminal Failure 不重试);SessionInit 模型完整性 len+sha256 由 host 现算 +(len,mtime)备忘(GB 级不重算);7 单测(mock EmbedWorker)。
3. **worker 派发路径**(ai/worker_pipeline.rs):Producer/Writer 与进程内同源复用(控制面同一套代码),中段 = 单派发线程攒批 → CPU permit → GPU 令牌(D2 顺序天条)→ EmbedBatch;缺 ai_cache 项 host 预检跳过(保持 Processing 下次运行恢复,T18 闭合缺口,有日志不静默);逐项 retryable 跳过 / terminal 标 Error / 批级致命终止本轮;运行结束即 close_session。
4. **接线面**:pipeline.rs 按 `ai_backend` 分流(缺省 inproc 零变化)+ 提取 resolve_batch_size(三处共用);search.rs 拆 semantic_search_with_vector(两后端共用打分半段);semantic_search_cmd worker 分支走 EncodeText;start/restart 在 worker 后端跳过进程内引擎 init(防双份 VRAM);EmbedWorker trait 补 session();op_timeouts 扩 SESSION_CLOSE(30s)/ ENCODE_TEXT(30s)。
- **T17 分叉裁决(无人值守)**:① EncodeText 全批原子 + count 核对 + 不新增 capability;② worker 会话必备 tokenizer;③ 缺缓存项 host 预检跳过而非现场生成(缓存生产=T18 职责,不越界);④ 模型 sha256 host 现算 +(len,mtime)备忘;⑤ host「空闲卸载」收敛为运行结束即 close_session、不设独立 idle timer(D3 §4④ 正文已回写);⑥ face_pipeline 派发不在本波(随 e2e 波接线,FACE_DETECT_EMBED 档位留 allow);⑦ ai-worker exe=主程序同目录 + env 覆盖(签名/插件包分发随 Part7)。

**worker e2e 验收 ✅**(程序化,2026-07-03 `54ee6fe`,本机实测)

- 新增 src-tauri/src/bin/worker_e2e.rs harness(仿 T9.5 vram_probe 先例)——真实模型(cn-clip-vit-b16 fp16 双塔 + vocab)+ 真实 ai_cache(110 张均匀采样 8)双后端同输入对拍:进程内 AiEnginePool 参考 vs 真实 spawn 的 ai-worker 子进程(与 `ai_backend=worker` 生产同一实现)。
- **结果全过**(EXIT=0,clippy 修复后终版二进制复验):图像 8/8 + 文本 3/3 余弦全 1.000000(两侧共用 ai-core 同套 preprocess/推理,B/16 固定 batch=1,数值逐位一致——将来任何 <1.0 偏差即真实回归);检索 top-1 三查询全一致且分数相同;close→重 init 自洽 1.000000。
- **时延**:进程内引擎加载 1.1s;worker 冷启动首查 1.8s(spawn + 握手 + sha256 ~0.4GB + SessionInit + EncodeText,启动税 ~0.7s 可忽略,推翻「host 现算 sha 拖慢冷启动」隐忧);重 init 1.4s(sha 备忘命中);EmbedBatch 8 张 1.13s。
- **e2e 分叉裁决(无人值守)**:① e2e 以程序化 harness 达成主证据;② harness 不碰真实用户 DB——含 DB 状态机/让步的全管线 GUI 快测降为 **T16 动工 gate 的最后一格**(用户在环一次:app_config 表设 `ai_backend=worker` 后跑一次分析+搜索);③ 进程内参考侧 tokenizer 直接 from_profile 构造(与 worker 端 SessionState 同构)。
- 验证:双特性组合 436×2 全绿(仅本地;harness 为 bin 目标不增测数)。

**face_pipeline worker 派发接线 ✅**(2026-07-03 `9ecee43` 接线本体 + `a9c7751` harness Phase E,双特性组合全绿仅本地)

1. **协议 additive(不升版)**:FaceItemResult::Ok 补 width/height(worker 实际解码尺寸)——几何是解码图像素坐标而解码在 worker 端,host 归一化/quality 派生不可用预测尺寸(舍入误差);serde(default) 容旧帧,host 校验 0 尺寸=违例(锁测试);同波补 handle_face 批上限防御(对称 embed,Boy Scout)。
2. **AiWorkerClient::face_detect_embed**(FACE_DETECT_EMBED 档去 allow;维度取 SessionReady.face_embed_dim,缺失=违例);**matches() 超集放宽**:spec 无 face 可复用带 face 合并会话(face 边际 VRAM ~96MB,T9.5 实测)——否则 face 运行期搜索把会话抖成 close→init 循环;mock 2 新测(超集匹配矩阵 + face 批切换与尺寸透传)。
3. **face_pipeline worker 运行器**:Producer/Writer 与进程内共用(同 T17 手法),中段 = 攒批 → resolve_face_decode_source(与进程内共用同一函数:thumbnails 档位预测短边 ≥640 才用缩略图)→ CPU permit → GPU 令牌(D2)→ FaceDetectEmbed → faces_to_records(从 detect_and_embed_one 提取的共用映射,两路径落库语义逐位一致);源格式白名单(worker 纯 image crate):exotic 原图(heic/raw 等)项跳过保持 Processing(已知过渡缺口,有日志);运行结束即 close_session;孤儿恢复提取 recover_orphaned_face_items 共用。
4. **命令层**:start/restart_face_analysis worker 分支跳过引擎加载(防双份 VRAM)+ 就绪检查收进 spawn_blocking(rusqlite 硬化);face_loaded worker 语义 = face_enabled + 激活轨双 onnx 在盘(active_face_profile 提 pub(crate) 同源判定)。
5. **harness Phase E 实测全绿**(EXIT=0):8 样本两侧脸数逐张一致(4 脸 / 4 零脸),bboxΔ=0.00000、嵌入 cos=1.000000、解码尺寸一致;会话切换(CLIP-only→合并)实测;超集复用实测 0.11s 无重 init。
- **face 波分叉裁决(无人值守)**:① additive 补实际解码尺寸而非 host 预测;② 派发源与进程内共用决策但仅派 worker 可解格式,exotic 原图跳过为已知过渡缺口;③ matches 超集放宽;④ face_loaded 就绪语义 worker 分支不触引擎;⑤ fingerprint=item_id + 阈值(行为参数入指纹,协议明文);⑥ face 批派发不预检 ai_cache(face 源=缩略图档位/原图,与 CLIP 的 ai_cache 预检语义无关)。

**T18 ai_thumb 喂 CLIP ✅**(2026-07-03 `893b1d2`,双特性组合 442×2 全绿仅本地 + harness 回归 EXIT=0)

- **触发背景 = 用户 GUI 首测暴露**:切 worker 后分析立即结束、全部跳过——诊断 DB∩盘=1/110(磁盘 110 个 ai_thumbs 几乎全是旧 mtime 键孤儿),库 1554 项 ai_cache 覆盖率 ≈0;进程内路径三级回退(ai_cache→缩略图→原图)从不暴露,worker 只认第一级。
- 按 §3.8 规范原文(「无命中回退派生原图产 ai_cache」)落地:① 提取 generate_ai_cache(derive/image.rs,run_ai_thumb 与 worker 派发共用一份实现;2 单测:原子无 tmp 残留 + 幂等逐字节不变、缺源 Err 不留文件);② **顺带修红线**:run_ai_thumb 原 std::fs::write 直写最终路径违反「派生产物一律原子落盘」,改 write_atomic(generator.rs 提 pub(crate));③ worker dispatch_batch:CPU permit 提前至生成段前(现场派生=重 CPU 解码,须在后台池配额内,D2 序不变)→ 缺缓存现场派生 → 派生失败标 Error(同进程内解码失败语义,不连坐整批)→ GPU 令牌 → EmbedBatch;瞬态跳过语义保留。
- **T18 裁决(无人值守)**:① 现场派生在派发线程串行(首跑全库性能次优 = §3.8 明文接受;快路径=缩略图顺带产出/派生侧预产);② 派生失败=Error 不重查;③ 教训固化(memory e2e-selfconsistent-blindspot):e2e 自洽采样 ≠ 覆盖率验证,对拍数据源必须与生产键连接验证。

**T16 准备件 ✅(S1 地基 + 拆除设计)**(2026-07-03 `e770d99`,双特性组合全绿仅本地)

- ① 依赖面盘点:host 直接 ort 仅 2 处(error.rs From + ai_commands ort::init),tokenizers 直接使用为零(纯 Cargo 残留)。
- ② **ai-core inference feature 地基**(缺省开 = 零行为变化):纯件外移(embedding.rs 字节序三函数 / face_types.rs DetectedFace+quality,clip/face 原位 pub use 再导出保路径)+ clip/engine/face/provider 模块级 cfg 门 + AiError::Ort 臂同门 + 六依赖全 optional;**`--no-default-features` 实测可编译**(依赖图仅剩 serde/thiserror/tracing,ort/tokenizers 整树消失),+face-noncommercial 组合同绿——host 摆脱 ort 的路径已实证打通。
- ③ **拆除设计文档** docs/refactor_2026/2026-07-03-Part4-T16-去ort拆除设计.md:全盘点表 + S0-S5 分步顺序(S0=用户 GUI gate / S1=本地基 ✅ / S2=host 切 worker-only / S3=harness 黄金向量改造 + vram_probe 删 / S4=Cargo 收口 + cargo tree 无 ort 为证 / S5=文档回写)+ 三拍板(vram_probe 删而非迁;ai_backend 配置退役=保键忽略值;status provider 字段:SessionReady additive 补 provider/gpu_name 回声)。

**T18.5 worker 派发性能回归修复 ✅**(2026-07-03 `8941eda`,双特性 443×2 全绿仅本地 + harness 全相位过)

- **触发 = 用户 GUI 实测**:切 worker 后 1550 张 10s→5min 且重跑同慢(排除首跑派生因素)。
- **定位(harness 实测)**:EP 无辜——双后端 cos=1.0、Phase A/B 单张成本同为 ~110-145ms;根因 = **worker 化丢掉进程内路径的并行结构**(in-proc=rayon 预处理池 + WIC GPU 解码喂单推理线程;worker 路径=host 单派发线程 + worker 端串行解码/预处理,CPU 段 ~100ms/张 串行硬扛,推理仅 ~6ms/张)。
- **修复四件**:① worker handle_embed 全批并行解码 + 预处理(std::thread::scope + 原子游标领活,零新依赖;≤16 线程,预处理张量 600KB/张全批驻留内存安全);② handle_face 分块并行解码(块=4 兼作解码图驻留上限;推理仍串行);③ ai-core preprocess_image/preprocess_decoded 像素循环扁平化(ndarray 逐元素索引 → 平坦 slice 写,dev opt-0 热点;算术逐位不变,对拍 cos 仍 1.000000);④ host dispatch_batch 现场派生 rayon 并行化(对齐 in-proc 并行语义)。
- **证据**(harness 新增 Phase B2 吞吐相位,64 张单批同机对照):修复前 112ms/张、×1550 外推 173s;修复后 **13ms/张(8.6×)、外推 21s**(debug 构建,release 更快);全对拍/检索/face/超集复用仍全绿。
- **T18.5 拍板**(①③ 已被用户二测改判,现行见 T18.5b/c):① ~~并行度上限 embed=16/face=4~~ → **用户拍板:解码并行度探测机器核数、不设固定上限**(decode_threads=min(逻辑核数,任务数);face 块=4 保留但语义澄清为内存上限,常量更名 FACE_DECODE_CHUNK);② host CPU permit 保持「1 批=1 槽」记账,worker 多线程由 BELOW_NORMAL 优先级让路前台;③ ~~剩余差距接受不再深挖~~ → **用户二测质询(22s 但 GPU 仅 35%/CPU 仅 50%)后改判:当波补 T18.5b 流水重叠**;④ 发现 dev profile 未给依赖开 opt(`[profile.dev.package."*"] opt-level=2` 涉全局构建时间留用户拍板)。

**T18.5b/c 批内流水重叠 + 并行度放开 ✅**(2026-07-03 `40138b7`,双特性 + test 443×2 全绿仅本地 + harness 全相位 EXIT=0)

- handle_embed 改「解码线程池喂有界 sync_channel(容量 2×threads 封顶在飞张量内存)+ 推理侧攒子批(≤16)边收边推」——此前相位交替(先全解码再全推理)使批耗时=两段之和、GPU/CPU 利用率互为镜像(用户实测 35%/50% 即病征),重叠后=两段取大;批级失败提前 drop(rx) 解除解码线程 send 阻塞(防 scope 卡死);blob/结果序契约不变。
- **证据**:B2 吞吐 13→**8ms/张**(64 张 0.52s),×1550 外推 **13s**(用户 GUI 实测轨迹 5min→22s→预期 ~12-15s;in-proc 基准 10s ≈ 推理节拍 ~6ms/张 的地板);对拍全绿 cos=1.000000。

**T16 去 ort 全段收官 ✅(S0-S5)**(2026-07-03,`ef17d42` S3a 黄金导出 / `3d67321` S2+S3b 切 worker-only / `7acccd5` S4 Cargo 收口;每步双特性 + test 443×2 全绿仅本地 + harness 全相位 EXIT=0)

- ① **S0 gate = 用户 GUI 三测 14s 通过**;② S3a 黄金向量入库(8 图 + 3 查询 + face 自有键集含 1 有脸锚点,sha16 防错拍);③ **S2 host 切 worker-only**:删 pipeline 预处理池 / 推理线程 / 解码源决策、face_pipeline 进程内 runner、search 编码半段、ensure_engine_initialised/ort::init、state.ai_engine、engine 薄壳(净删 ~1300 行);ai_backend 配置退役(保键忽略值仅日志);薄壳重塑(clip→embedding 纯件 / face→face_types 几何 / provider→host 自持 DXGI VRAM 探测,调用路径全不变);SessionReady additive 补 provider/gpu_name 回声(拍板③落地:worker 填探测结果,host persist_provider_echo 写回既有 config 键,status 命令读法零改动);AiTask/PendingAiItem 瘦身;④ S3b harness 黄金对拍(Phase A 装载 + sha16 失效检测 + 黄金键防漂移断言;--export-golden 改 worker 侧快照语义)+ 删 vram_probe/face_smoke/test_ort_load 三个进程内历史工具(git 史可找回);⑤ **S4 Cargo 收口**:src-tauri 删 ort/tokenizers/ndarray 直依赖、ai-core 改 default-features=false、AppError::Ai 改携带字符串(IPC code 稳定)、AiError 挂 non_exhaustive(workspace feature 并集把 Ort 臂带回时通配臂兜住,两种构建形态零警告)、ai-core 头注「ort 两处同步」义务解除。
- **证据**:`cargo tree -p picasa-next -e normal` 零 ort/tokenizers/ndarray(=tauri build 单包形态);黄金对拍全相位 cos=1.000000 + face 锚点 bboxΔ=0。
- **T16 拍板补录(无人值守)**:① examples face_smoke/test_ort_load 随进程内推理面删除;② 黄金 face 段自有键集 + 全零脸自动补扫;③ AiError non_exhaustive 解 feature 并集匹配两难;④ GUI 状态命令(detect_ai_provider / clip_loaded / reload_ai_engine)前端契约零改动。
- **下一段(在环):用户 GUI 四测**——T16 删了大片 host 命令层,请一轮回归:分析 + 语义搜索 + 人脸分析 + 设置页模型信息显示(provider/GPU 名应在首次分析或搜索后刷新);过后 Part4 剩余 = T19 mac(defer 至 mac 环境)/ T20 合并 vs 分离正式拍板 / face exotic 原图覆盖拍板。

**T16-R1 GUI 四测回归:face 批超时修复 ✅**(2026-07-03 `9947921`,clippy 零警告 + workspace 443 全绿仅本地 + harness 全相位 EXIT=0)

- **四测结果 = AI 分析 ✅ / 人脸 ❌**——1554 张启动 ~2min 后 supervisor 以 timeout 击杀正常工作的 worker,worker_client 重试同批必然再超时 → 硬止损终止整轮。
- **根因两条**:① face 派发直接复用 CLIP 的 VRAM 自适应 batch=64 配 FACE_DETECT_EMBED 固定 120s——人脸源可为全尺寸原图(dev opt-0 解码单张秒级),64 张批必然超时;而 face 推理逐图进行(YuNet 固定 640 输入),大批对吞吐零收益、纯增超时敞口;② worker handle_face 只把「解码」并行化,letterbox(全尺寸→640 缩放,与解码同级昂贵)留在串行推理段——**「迁 worker 丢并行结构」同病第二例**(memory 已固化补 ④ 超时缩放条目)。
- **修复三件(无人值守拍板)**:① face_pipeline FACE_DISPATCH_BATCH=16 派发上限(与 CLIP batch 解耦);② op_timeouts::face_detect_embed(n)=60s+6s×n 按项数缩放(定位=假死检测器而非性能指标,宁可宽松——用户取消走 cancelled 回调即时生效不受超时影响);③ handle_face 块内全并行:每项在线程内跑完 解码→letterbox→检测→嵌入 整链,GPU session 由池锁自串行,FACE_DECODE_CHUNK=4 驻留封顶语义不变。
- **证据**:face 锚点 bboxΔ=0.00000 / cos=1.000000、B2 吞吐 9ms/张无回退、ai-worker.exe 已重建。
- **独立事件:psd-worker hash_mismatch 30s 重试噪音(与本波改动无关)**——install_root(Roaming\com.picasanext.app\exotic)整目录不存在而 DB 安装记录仍在,复核第一步「缺 package-manifest.json」即失败(归类 hash_mismatch);目录为何消失未考(dev 噪音低风险)。处置 = 商店卸载重装 exotic-image-psd,或 dev 旁路 EXOTIC_PSD_WORKER_PATH=target\debug\psd-worker.exe。

**T16-R2 face 慢根因取证 + 计时插桩 + 修复三方案**(2026-07-03 `bc4b6a2` 取证;2026-07-04 修复)

- **取证**:用户回报 face 能跑但极慢(体感 100 张 2-3 分钟)。推理确在 GPU(directml,face detect/embed 各 2 session),慢在 CPU 解码——resolve_face_decode_source 要求缩略图短边 ≥640 而档位顶格 960 长边(16:9 短边仅 540),大多回退**全尺寸原图** + worker 纯 image crate opt-0 解码 + FACE_DECODE_CHUNK=4 封顶;旧进程内路径用 ImageEngine(WIC 原生)+ ResizeHint::ShortEdge(640) 解码期降采样,故 worker 化前不这么慢——**「迁 worker 保并行结构」第三实例:解码引擎与解码期降采样也是结构**。
- 外证(联网调研):immich 喂 1440px 预览 JPEG 从不解原图(官方文档明文),检测输入同为 640;YuNet 640² ~5-15ms CPU、SFace ~5ms/脸——推理解释不了 1.5s/张;ORT 官方:DirectML 同 session 禁并发 Run、多 session 为认可并发模式(现结构已是)。**多 worker 实例裁决:不采**(复制 CPU 解码瓶颈 + 每实例一份模型 VRAM + gpu_token 本就互斥)。
- **修复三方案(2026-07-04 用户拍板「按你的建议推进」,已全部落地;B=`8972995` / A+C=`0c8e263`)**:
  - **A = host 侧 WIC ShortEdge(640) 预解码到 face 缓存**(与 CLIP ai_cache 模式同构,治本;顺带消 exotic 原图跳过缺口;release 同受益):face_thumbs/{prefix}/{hex}.webp(FACE_CACHE_SHORT_EDGE=640,与 336 的 ai_thumbs 分目录),派发批三级定源(缩略图档位 → face 缓存 → 短边 ≤640 小原图直派)+ 缺缓存批内 rayon 现场预解码(镜像 CLIP T18/T18.5,CPU permit 提前到预解码段前保 D2 顺序)。
  - **B = `[profile.dev.package]` 仅给解码 crate 开 opt-level=2**(18 个外部解码/压缩链 crate;不动本地 crate 增量编译,dev 立竿见影;全局 `"*"` 覆盖仍留拍板)。
  - **C = A 落地后 FACE_DECODE_CHUNK 4→探测核数**(640 级源 RGBA ~2MB/张,封顶前提消失)。
  - **衍生裁决**:① worker 走 source_path 绝对路径而非协议 cache_key(零协议改动,信任语义同缩略图源);② 预解码失败(WIC+image 双引擎都败)标 Error 而非保持 Processing;③ face_thumbs 纳入 LRU 预算与 "ai" 类清理、统计并入 ai_thumbs 类目,cache_files_for_key 8 产物 + 对账 GC 五子目录;防呆:未来 detect_size>640 模型不吃偏小缓存(face_cache_applies 运行期防护)。
- **插桩实测坐实取证**(用户回传):单项均值 解码 2487-4051ms / 检测 1705-4799ms / 嵌入 ≈0,批墙钟 17-36s/16 项。验证(仅本地):clippy 双侧零警告 + picasa-next lib 369 绿 / 1 忽略 + ai-worker 14 绿 + exe 重建。
- **在环复核 ✅(2026-07-04 用户实测)**:1554 项全 face 缓存源(缩略图 0 / 原图 0),均 33ms/项、30 项/s——对比修复前 ~1.5s/项约 **45×**,用户确认「速度快很多了」。

**T20 合并 vs 分离拍板 + Part4 收官**(2026-07-04 无人值守按授权记录,可逆)

- **维持合并单 ai-worker**——依据三半边齐:① T9.5 VRAM 实测(合并 361.6MB@B/16,HQ 上界 <4GB);② 崩溃率半边实测补齐(T15 落地以来全程零 organic crash——worker 死亡均为 host 超时击杀(T16-R1 已修)或 app 关闭 disconnect);③ 超集会话复用收益(face 边际 96MB,分离则 face 运行期搜索必然 close→init 抖动)。
- 按「开发期不冻结契约」**不冻结**:PluginDescriptor 注册表与协议均已支持多 worker,若崩溃遥测显现隔离需求可低成本改分离。
- **Part4 收官状态**:剩 T19 mac(defer 至 mac 环境);face exotic 原图覆盖**已被 T16-R2 方案 A 顺带收敛**(heic 等经 host WIC 预解码入 face_thumbs);worker vs in-proc 残差(14s vs 10s)有意延后(三测 14s 未破 15s 门槛)。

### ▸ 详注 G2-2:Part4 P0/P1 散件(T1 / T2 / T7 / T8 / T9)

- **T1 ✅**(`19de56c`):cfg 全链隔离 + 双组合验证 + 合规断言测试;商业流水线符号扫描断言留 T16/17 兜底。
- **T2** 渠道过滤守卫:需 pro,← ③b。
- **T7 ANN**(dormant 有主):50 万向量或 p95>300ms 触发;与 Part1-T11 同线。
- **T8 缓存持久化半件 ✅**(`2630c80`):磁盘 L2——冷启动免联网 + 断网陈旧兜底 + 原子落盘;自托管 Registry 半件留基建。
- **T9** CLIP 自导出 + 自托管:合规 🟠,待基建。

### ▸ 详注 G2-3:Part3 尾件(T10 / T11 / T12)

- **T10 ✅**(`6e9134b`):store_doc_thumbnail 加 page_count 参 + document_meta 直写(pdf=numPages / svg=subtype-only)。
- **T11** ai_thumb 联动:← Part4 / Part6。
- **T12 ✅ 代码先行**(`b620662`):批管线 deferred 专用小池解耦 decode worker;§3.5.2 limiter 下限实测已先落 state.rs:218。⚠ **滚动流畅度回归待 GUI 手测**——用户拍板代码先行、验收前不算 Done。

### ▸ 详注 G2-4:Part7 主体 T4 / T6-T17 + §3.6.5

- **状态**:发布工程现状 ~15%。多项已 ✅(T6 / T7-T9 / T11 / T12 / T17),剩 mac 矩阵 + 证书链 T13-T16。
- **补充对账**:T18 identifier 断言随 R2-7;T3 oss 边界断言的「pro 未入仓」半边已被两仓拓扑推翻,cargo tree 半边由 oss-gate members 断言覆盖。

**已交付子项**

- **mac 矩阵**:← Part3-T8 mac 门控全量。
- **T6 去 ort 资源层 ✅**(2026-07-04 `40508f8`):bundle 剔除 4 DLL(实测 ~63MB),dev 改 .cargo/config `[env]` ORT_DYLIB_PATH 指 npm 包(onnxruntime-node 移 devDependencies,真删随 Part8),CI 双 workflow 撤材料化补丁,删净 target DLL 后 worker_e2e 全相位实证(仅本地;bundle ls 核对留 T16/T17)。
- **updater 三件 T7-T9 ✅**(2026-07-04 `7b226d0`):optional dep 绑 channel-direct(msstore 树零 updater 实证)+ dev minisign 密钥(.release-keys gitignored,公钥入 conf)+ createUpdaterArtifacts 落 overlay conf(签名 env 不绑架本地构建);裁决:前端包/capability 有意不加(Rust 驱动零 IPC 面),endpoints `.invalid` 占位待 Part8;**人工半边**:私钥入 CI secrets + 首发前密钥转正(G3 基建行)。
- **T2 ✅**(已随 R0-1 交付)、**T3 ✅ 形态修订**(断言职责归 oss-gate,「pro 未入仓」前提被两仓拓扑推翻;2026-07-04 对账)。
- **T12 Provider stub ✅**(`a6a05e5`):fail-closed 桩 + 组合根工厂 cfg 选择,四渠道组合可编译;entitlement_source 消费留 Part6-T13。

**channel 物理门控 T11 + §3.6.5 ✅**(2026-07-04 `b05d0eb`)——四维物理排除 + 联合扫描全落地:

1. **conf 面** = 基座物理清空 plugins.updater 迁 tauri.direct-release.conf.json(conf 合并只能加不能删,Store 渠道安装包内嵌 conf 必须基座天生干净),lib.rs 注册按「context 最终配置是否携带 plugins.updater」判定(dev / 无 overlay 构建不注册——插件 init 对缺配置硬失败,行为无损;msstore/steam 不建空 overlay)。
2. **Rust 面** = KeyringLicenseStore / fetch_package / fetch_model_blob / install_exotic_plugin 命令体 cfg 门 channel-direct(install 留 channel_unsupported 稳定错误码桩保 IPC 面;registry 浏览=数据面、repair/rollback/uninstall=本地生命周期,全渠道保留;keyring **crate** 不随门——storage/proofread API Key 属通用能力非 DRM)。
3. **扫描面** = scripts/verify-channel-bundle.mjs(conf/capability/dist 三面;内置 14 断言正反 selftest 防「只会 PASS 的空壳」)+ ci.yml 双接线(rust job cargo tree 第四面:store 渠道无 updater + direct 树反向守卫;frontend job 生产 bundle 后扫,原型实证须 minify 后扫、不扫函数名)。
- **分叉裁决(无人值守)**:§3.6.5 ②③ 降级为扫描守卫(T7 落地后前端零 updater 包 / capability 零 updater 权限,无消费者不预设抽象,同 Part4-T15 裁决④)、④ pro feature 转发随 ③b、AES 半边随 ④(现无 AES 代码);Part7 §3.6.5 已按正文回写红线挂落地修订横幅。
- **验证(仅本地)**:clippy -D warnings 三渠道组合全绿 + cargo test --workspace 387 过 + 3 ignored + cargo tree 三向断言 + 扫描 44 文件 PASS + dist 污染探针红绿实证;direct-release 全构建(.sig 产出 + updater 注册实跑)留 T16 流水线验收。
- **T11 断言 CI 首跑红 → 修复 ✅**(`dfd2fcb`,CI 复跑绿):GitHub shell bash 注入 pipefail,「echo 大文本 | grep -q」在 grep 命中早退时上游收 SIGPIPE(141)→ 管道整体非零 → 正向断言(期望命中)误红;排除 ANSI 色码 / cargo 1.96.0→1.96.1 / lockfile 三嫌疑后定位管道竞态(本地无 pipefail 跑不出)。**对偶面更险**:oss-gate 两处 forbidden 扫描(keyset 泄漏 / members 含 pro)同型——真命中时管道 141 令 if 为假,泄漏被静默放行;四处全改 herestring(`out=$(cmd); grep -q X <<< "$out"`,双向免疫),本地 pipefail 全脚本彩排过,教训入记忆 shell-chain-guards。

**待做子项(证书 / 流水线)**

- 双平台签名 T13-T14(需证书)/ worker 签名产线 T15 / commercial release 流水线 T16(需 T13-T15 证书链)。

**oss-release T17 ✅**(2026-07-04 `0d2b710`)

- .github/workflows/release.yml:公开仓 tag `v*` 正式发布 / workflow_dispatch dry-run(构建 + 校验 + 工件上传不发布,**兼作 C 节「完整安装器打包 WiX/NSIS 验证」实跑面**);github.repository 守门只在公开仓执行(私有 tag 归 ci.yml 冒烟 + T16,双轨物理隔离,零签名凭据);硬门三连 = tag 版本锚一致 + CHANGELOG 小节提取 + 渠道合规扫描,发布 SHA 复跑 cargo test,产物 MSI+NSIS+SHA256SUMS。配套 CHANGELOG.md(发布说明单一事实源)+ scripts/extract-changelog.mjs(缺小节/空小节硬失败,正反路径本地实测)。
- **分叉裁决(无人值守)**:① 公开仓历史 = squash 同步提交,§3.7.1「conventional-commits 自动生成」在公开侧不可用 → 改「私有维护 CHANGELOG → 同步公开 → 提取版本小节」,git-cliff 类自动化留 T16 私有流水线;② 免费构建 = 默认 tauri build(default 已含 lite;§3.7.1 的 `--profile lite` 无对应 cargo profile,系设计稿未落地项);③ 发布说明缺失 = 硬失败(倒逼发布纪律)。
- **dry-run 实跑验收 ✅(2026-07-05,用户拍板 dev→main 提升后全链执行)**:私有 main 56fb3d8→2036134(快进)→ 私有 CI 绿 + sync-oss 绿 → 公开 sync-staging `2dc2cf1`,oss-gate strip-tree 7m25s 实跑绿(herestring 修复后 forbidden 扫描首次真实投影树取证)+ gitleaks 绿 + drift-alarm 绿 → 公开 main 666da7f→2dc2cf1(gh api 快进,ruleset 同 SHA 三 checks 天然放行)→ dispatch dry-run(run 28726569655)27m20s 全绿:**MSI(WiX light)+ NSIS(makensis)双安装包产出**(Picasa Next_0.1.0_x64_en-US.msi / _x64-setup.exe)+ SHA256SUMS 双条目 + 工件上传;gh release list 为空 = 发布路径未触发,dry-run/tag 出口分流实证;公开 CI@main 绿。
- **T17 全收官**;顺带勾销 C 节「完整安装器打包验证」(本项目首次任何环境的 WiX/NSIS 实跑)。
- **§3.6.5 非 Rust 产物物理排除 + 四维联合扫描 ✅**(2026-07-04 随 T11 落地,见上;卡 Store 上架的面已解除)。

**内测发行链 + 内测安装包 ✅**(2026-07-05,用户指令「功能完整内部测试安装包,最大化跑通主线,手测/产品名不阻塞」)

- **三断点打通**:① 编译期 keyset 注入(exotic-trust build.rs 装配 OUT_DIR,构建设 `PICASA_EXOTIC_KEYSET_FILE` 即替换信任根——正是占位集 `_note` 预告的「发布前由发布流水线替换」机制提前落地;运行时零旁路,SEC-02 不动摇)② registry base 编译期默认值(`option_env!("PICASA_REGISTRY_BASE_DEFAULT")`,装机用户零配置;运行时 `PICASA_REGISTRY_BASE` 仍最高优先)③ 内测签发端(`scripts/lib/exotic-signing.mjs` 共享签名库(dev 生成器同源重构,行为不变)+ `exotic-internal-registry.mjs`(release worker + 真 HTTPS package_url)+ `exotic-issue-license.mjs`(payload.sig token,默认永久)+ `build-internal-installer.ps1`)。
- **发行源**:公开仓 `gfgjs/picasa-next-registry` raw 直链(exotic/v1 三件套;`.gitattributes * -text` 钉死签名工件字节精确——git 文本规范化会改写签名字节,首推时的 LF 警告即此坑的预警);**分叉(用户拍板 2026-07-05)**=新建公开仓 raw URL 方案(备选=公开镜像仓 Releases/自有托管;psd-worker 源码本在公开树,编译产物公开托管零泄漏,工件由内测键签名与生产信任根隔离)。
- **无人值守分叉裁决**:内测 keyset=「占位集+内测键」**超集**(注入态下 builtin_keyset_parses 断言仍成立)/内测键 `*-internal-2026-07` 与 dev 键、未来生产键三层隔离/index 60 天有效期(过期只展示不允新装,重签续期)/`.internal-signing/` gitignored,私钥仅签发机本机、不入 CI。
- **验证(仅本地)**:注入机制双向实证(注入缺 release-2026-01 的 keyset → builtin_keyset_parses **预期红**证明嵌入生效;去 env → trust 26 测绿证明默认路径逐位等价)+ dev 链重构回归(dev_registry_artifacts #[ignore] 绿)+ 内测产物过生产校验链(internal_registry_artifacts #[ignore] 绿,HTTPS 条目零 dev 开关=装机形态)+ 实网一致性(raw 三件套下载 SHA256 与本地签名产物逐字节一致)+ 安装包注入三重取证(build 日志 cargo:warning 留痕/exe 内嵌 release-internal-2026-07/exe 内嵌内测发行源 URL)+ vue-tsc、vitest 207、eslint(4 脚本)、clippy -D warnings、cargo test --workspace 全绿。
- **产物**:`target/release/bundle/msi/Picasa Next_0.1.0_x64_en-US.msi`(39.5MB,d60e3c52…)+ `nsis/Picasa Next_0.1.0_x64-setup.exe`(9.6MB,387d3bce…);PSD 永久 token 存 `.internal-signing/token-psd.txt`。runbook + 内测者指南:`docs/2026-07-05-内测安装包与内测发行链.md`。
- **边界(如实)**:安装包未代码签名(SmartScreen 提示,T13-T15 未至)、无 updater 配置(基座 conf,lib.rs 按 context 判定不注册)、信任根为内测键**不得当正式渠道分发**;生产 keyset/CDN/签发机仍归 ③b/Part8(B1 表相应行未动,仅补机制注)。安装后 GUI 全链(商店刷新→激活→安装→PSD 出图)待真人走查——本项交付的就是这份可走查的安装包。
- **实测病历 #1(2026-07-05,已修)**:真机首刷即 `rollback_rejected`——dev(tauri dev)与装机 release **同 identifier 共享 appdata 防回滚基线**(`exotic/registry/index.seq`),此前 dev E2E 已把基线推过 2,内测 index `sequence=1` 被正确拦截;更深的结构性脆弱=计数器状态文件 gitignored、换机重签从 1 重启 → 已装设备**永久**拒收。修法=两生成器 sequence/package_sequence 改**生成时刻 epoch 秒**(无状态天然单调;展示版本号仍用小计数器),重签重发并实网验证(seq=1783230441,三件套 SHA256 逐字节一致);装机侧无需重装,刷新即愈。教训:防回滚基线是**设备级**共享状态,任何签发源(dev/内测/生产)的序号空间必须全局单调,计数器文件担不起这个契约。
- **真机走查主流程 ✅(2026-07-05 用户确认)**:刷新商店 → 贴 token 激活 → 安装 → PSD 出缩略图全链跑通。
- **实测病历 #2(已修 `66ff8ba`)**:处理进度永久卡 60/62——孤儿租约恢复只存在于 pipeline 步骤 0,而「是否启动 pipeline」由 has_ready(只认 pending/retryable)决定:硬杀/panic 遗留的 stale processing 行使二者**互锁**,恢复代码永不可达,30s 重试时钟每轮撞墙。修法 = Coordinator 每次 wake 评估前清扫过期租约(仅回收超 LEASE_TTL 者,活实例 renew_loop 免疫;迟到结果由 `status=1 AND lease_owner` 条件更新兜底不双写)+ 回归锁测试。教训:**自愈代码不能只挂在「要被救的路径自己启动之后」**——启动条件恰被待救状态压住时,自愈永不可达;装机侧装新包重启即自愈(Startup wake 清扫)。
- **实测病历 #3(已修 `66ff8ba`)**:①激活成功后卡片仍显激活按钮——商店卡片从不消费 entitlement,修为按已装行取授权态、authorized 显「已激活」徽章;②进度区是挂载时刻冻结快照——后端每批都发 `exotic:status-changed` 而前端无人监听,补监听(500ms 节流)。教训:后端广播的事件要有消费端清单意识,「发了没人听」与「没发」在 UI 上等价。
- **病历 #2/#3 修复真机复验 ✅ 通过**(2026-07-05 用户确认「符合预期」:覆盖安装重建包 `d6560306`/`64aaf6cb` 后,卡住任务自愈、进度实时刷新、激活徽章正确)。至此内测走查:安装 → 商店刷新 → 激活 → 装插件 → PSD 出图 + 三病历修复全部真机闭环;**升级链路**(可升级检测 → 升级安装 → worker 版本失效重做)为商店主功能仅剩未走查腿,registry v2 发布随后解锁。
- **实测病历 #4(已修 `b428a0b`)**:v2 升级走查报「进度未重置」——**该现象本身是正确行为**(走查清单预期有误:失效键是 worker 自报 `CARGO_PKG_VERSION` 而非 manifest 版本,v1/v2 worker 字节相同,同解码器不该重做);但顺藤摸出真缺口——版本失效只在 pipeline 步骤 2 执行,启动条件 has_ready 只认 pending/retryable,**真换解码器的升级**完成时任务全 done → pipeline 不启动 → 失效永不可达,旧缩略图陈旧到碰巧有新任务为止(与病历 #2 同构:自愈码挂在被救路径的启动条件之后)。修法 = PluginInstalled/Startup/UserRequested 三类 wake 允许免 has_ready 起**一轮**对账(首轮消费防空转,其余门控照常,周期时钟排除防 30s 白 spawn)+2 测试;psd-worker 0.1.0→0.1.1 零功能 bump 发 registry v3(seq=1783245885)供真机验证失效重做。教训:①走查预期须从失效**键**推导而非从直觉;②同构缺陷(「自愈不可达」)修第一处时应全库扫同族——病历 #2 修租约清扫时若同扫「还有什么只活在 pipeline 启动后」,#4 当场就会浮出。
- **商店进度「展开查看详情」功能(用户点单,`c62182c`)**:进度条旁新增展开面板,可看逐文件处理明细(文件名/格式/相对路径/状态点/尝试次数/错误码 hover 全文),四桶筛选 chips(待处理/处理中/完成/失败,计数取自摘要),实时跟随 `exotic:status-changed`(复用 500ms 节流),「加载更多」每页 50 封顶 200。全栈:DAO `list_exotic_task_details`(桶白名单谓词 + status 优先级排序 + 分页)→ IPC 命令(bucket 校验/limit clamp/spawn_blocking)→ Vue 面板 + i18n 11 键。实现要点:数据流动时 offset 追加页会漂移 → 「加载更多」改为**重取整窗**(幂等,offset:0 窗口扩容)。
- **病历 #4 修复 + 升级链路 + 详情面板真机复验 ✅ 通过**(2026-07-05 用户确认「测试通过」:包 `c62182c`/`e2259903` + registry v3 走查——升级 1.0.4 后 worker 字节真变 → 版本失效对账触发、进度真重置重做;展开详情面板逐文件明细/筛选/实时滚动正常;已激活徽章保持)。至此**内测走查全链闭环**:安装 → 商店刷新 → 激活 → 安装插件 → PSD 出图 → 升级失效重做 → 详情面板,病历 #1-#4 全部真机复验收账。
- **前端 thumbhash 解码器补全(2026-07-05,`1f13562`,承接 G3 订正的「可选补全非决策项」)**:原 TS 解码器不止「仅亮度近似」——格式解析本身多处偏离(第二 header 按 4 字节读、AC 按整字节而非跨通道连续的 nibble 流读、缺 P/Q scale、`r=l+p+q` 换算公式错误),缩略图未就绪时的模糊占位实为失真灰块,`thumbhashToAverageColor` 纯色占位同病。补全 = 逐行对齐 evanw/thumbhash 官方算法(与后端编码用的 thumbhash crate 同源),含色度 AC ×1.25 饱和度补偿、LPQ→RGB 正确换算、alpha 重建、`clamp 后截断`对齐 Rust `as u8`。
  - **验证方法(跨语言金标对拍)**:Rust 侧加 `#[ignore]` 金标生成器(五张确定性合成图——横/纵/带 alpha/纯色/极小,经 crate 编码+自带解码器产参考像素与平均色,写成 `src/utils/thumbhash.golden.ts`),前端 14 测逐通道 ±2 容差对拍(f32/f64 余弦差 ×255 截断后最大 ±1)+ 均值差 ≤0.5 防系统性偏色 + 畸形/截断输入与 Rust Err 行为对齐(返回 null)。金标生成器挂在主 crate 测试模块内,与生产编码器共享 Cargo.lock 版本,crate 升级时金标同版本再生。仅本地验证:vitest 221 全绿 / vue-tsc / eslint / clippy / cargo test 468。

### ▸ 详注 G3-R2-7:R2-7 产品名拍板 + 三辖区商标 FTO + tauri identifier 锁定

> 📦 2026-07-12 从 todo.md G3 表「阻塞源 / 备注」单元格全文迁移(零删减);pending(公开 main 提升 / 域名占位 / FTO 委托 / tauri dev 冒烟)留 todo.md G3 行。
>
> 待办原文:🔴 **R2-7 产品名拍板 + 三辖区商标清权 FTO(US/EU/CN)+ tauri identifier 锁定**

- **全局最高杠杆死锁**:Part7-T18 / Part8 D10-D13 / 官网 / 上架全线前置;审查计划口径「拖延成本单调上升」。
- **2026-07-05 初筛完成**(5 路 agent 全景筛,[报告](decisions/2026-07-05-R2-7-产品名初筛与改名面盘点.md)):Mnemo/Lumeo/Tessera 🔴 淘汰(Mnemo 遭同名同品项 US 申请+微软商店同名 AI 照片应用双杀)、Arclive 🟡弱、**Foteca 🟡净=唯一推荐送 FTO**;'Scroll' 族经判据否决;⏰ foteca.com 2026-07-08 到期宜 backorder;改名爆炸半径已盘点(identifier/keyring=两状态坐标,趁内测期改最廉);第七轮候选判据已硬化(crates/org/.app/渠道站内四可得);
- **第七轮(卷轴方向,用户拍板)已筛**(6 路):英文 Handscroll 🟡净=两轮商标面最净 / Scrolio·Scrollery 🟡中 / Emaki·Enscroll 🔴,中文**长卷 🟢 唯一绿灯**(卷轴=资金盘污名+繁体≈滚动条 / 绘卷撞太吾峰值 / 手卷双品类劫持);~~推荐 Handscroll/长卷~~→**第八轮(用户口味坐标=Scrollery/画卷)再筛 5 路:Scrolleria 🟢 三轮英文唯一全绿**(六硬门全过双域皆空)/Rollery 🟡(波兰语通用词跑偏)/Scrollary·Unfurl 🔴(前者双域 5 天前被抢=名字正被他人启用);中文深筛:画卷=好词非好商标(App Store 同类目精确同名在营+ASO 裸词不可赢,留作文案语素),**卷廊 GREEN 倾向**(零在用实体,好商标需养词);
- **终选推荐 Scrolleria/卷廊 + 画卷留 slogan**,次选 Scrollery/画卷;⚠ 拍板当天须注册 scrolleria.com+.app(scrolio/scrollary 双双被抢在前);
- **✅ 2026-07-06 终选拍板 = Scrollery / 画卷**(用户选定心头好,接受生态税;当日追加筛查 MyScrolls 🔴 否决——ZeniMax 在权标准字 SCROLLS+习惯执法史,见初筛 §7.6);
- **FTO brief 终稿已出**([brief](decisions/2026-07-06-R2-7-Scrollery-FTO-brief.md),含拍板当天占位六项——scrollery.app 07-06 复核仍空 + GitHub org 变体 + @scrollery scope + crates 裸名 404 空闲——及三辖区律师问题清单 + 可转发英文版);剩余:用户占位 + 委托 FTO;
- **✅ 改名施工代码面已完成(2026-07-06,用户拍板提前于 FTO,红灯风险知情)**:identifier=`com.scrollery.app`/productName=Scrollery/crate 族 scrollery-*/KEYRING 收敛单常量/CHANGELOG 迁移条目,剥离模拟四层防线全绿(三真雷当场修:禁词 `\b` 词边界/门禁配置扫描排除/模拟器 glob 仿真),474+221 测试全绿(**仅本地,CI 欠费待补**);
- **✅ 仓库改名+URL/guard 批次+D10 翻 active 已收官(2026-07-06 当日)**:三仓原地改名 `gfgjs/scrollery-private`/`scrollery`/`scrollery-registry`(gh api 301 实证;未建 org),九处同批(copy.bara.sky 两 URL+四 guard+注释+两 registry base)+本地 remote set-url;D10 门禁 active=true 且 selftest+实扫全绿;剥离模拟新树复跑四层防线全绿(投影树 check --locked 1.74s);
- **✅ 全链 CI/同步实证收官(2026-07-06 双自托管 runner,billing 已纯可选)**:终局 run 28796485291 四绿(rust-linux 首绿于 dev-box-wsl)→你推 main→sync-oss 自托管首推成功→公开 sync-staging `7d5aa5a`(投影自 b1cba5d)→oss-gate+gitleaks+drift-alarm 全绿;
- **仅剩公开 main 提升一步(你侧,权限层留人工)**;NOTICE 门首次真实拦截(主题线依赖漏账)已补 b1cba5d;剩:tauri dev 冒烟(你侧);详见[施工计划 §7 实录](decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md)+[CI 运维手册](runbooks/2026-07-06-CI自托管runner运维手册.md)

### ▸ 详注 G3-小决策:2026-07-02/05/06 三组小决策清零(face 校验 B / thumbhash 订正+补全 / 缓存清理节奏)

> 📦 2026-08-23 从 todo.md G3 表「小决策」✅ 行全文迁移(零删减;行内仅留一句话结论+指针)。

- **Part4-T5 face 校验——已拍板选项 B 并实施(2026-07-06)**:size 快检入 `face_variant_installed`(+4 测试,LFS pointer/截断识破),全量 sha256 仅显式修复动作([决策 brief](decisions/2026-07-06-决策brief-渠道·开源边界·face校验.md) §3)~~(现「只按存在判定」有据)~~。
- **🔴 2026-07-05 审查订正**:~~Part3 thumbhash 修 vs 删(残缺 stub 全仓零消费,待 Part5 拍板)~~——**原表述系误标**。源码实证:thumbhash 是**完整 + 被消费**特性(后端 `generate_thumbhash` 接入 generator/exif_thumb/exotic sink 4 派生路径 + DB 存储 + 前端 10 文件消费),**无「删」的对象**;真实残留仅**前端 JS 解码器**(`src/utils/thumbhash.ts`)为仅亮度近似(跳过 P/Q/alpha AC 系数,色彩保真降级但功能可用),属**可选补全非决策项**——**✅ 已补全(2026-07-05,`1f13562`,含跨语言金标对拍,详见 completed.md)**。
- **缓存清理节奏**:~~(待裁)~~——**已裁并实施**(2026-07-02 无人值守授权,见 G1-6 Part3-T6)。
- 均为「无人值守不擅自推翻有据设计」型,拍板后实施均为小任务。

### ▸ 详注 G3-联网预取:2026-07-02 联网预取全量放行(simsimd 等六包)

> 📦 2026-08-23 从 todo.md G3 表「联网预取」✅ 行全文迁移(零删减)。

- **已完成(2026-07-02,你拍板全量放行)**:scratchpad 隔离工程 cargo fetch,442 包锁定并全量入本地缓存(simsimd 6.5.16 / usearch 2.25.3 / updater 2.10.1 / steamworks 0.13.1 等)+ npm cache add @tauri-apps/plugin-updater;**零侵入主仓**(未向依赖树加任何 crate,后续波次离线 cargo add 即命中)。

### ▸ 详注 G3-NOTICE:2026-07-05 NOTICE + SBOM 生成落地(无人值守清账)

> 📦 2026-08-23 从 todo.md G3 基建/法务清单「NOTICE + SBOM」条目全文迁移(零删减;该条已完成,清单其余条目为活项留 todo)。

- 生成 `NOTICE.md`(tracked;rust 402 + npm 95 第三方包;范围 = 发货物闭包:picasa-next+ai-worker+psd-worker 三树 normal 并集 × win 发货目标 + npm 生产依赖闭包,dev 依赖不分发不计入)+ `target/sbom/picasa-next.cdx.json`(CycloneDX 1.5,untracked 发布工件,接入发布流水线归 Part7-T16)。
- 脚本:`scripts/generate-notice.mjs`(零依赖 + 内置 selftest,仓例同 verify-channel-bundle)。
- ci.yml rust job 挂 `--check` 新鲜度门(lockfile 变更未重生成即红;**CI 实证** run 28732292304 @ `9170dc7` 三 job 绿,门输出「✓ NOTICE.md 新鲜」)。
- **病历**:门 CI 两跑红均为环境陷阱而非依赖差异——① `localeCompare` 的 ICU collation 随环境 locale 分叉(runner en-US vs 本机 zh-CN,下划线/连字符混排包名重排)→ 排序改纯码点比较;② ci.yml 的 `CARGO_TERM_COLOR=always` 强制 cargo tree 向管道吐 ANSI 色码,dedupe 标记 `(*)` 被染色击穿剥离正则 → 采集端 `--color never` 钉死 + 解析端剥 ANSI 纵深防御;另预防性修 ③ autocrlf 检出 CRLF vs 生成串 LF → 比对前行尾归一化。三类各有本地模拟复现(CRLF 转写 / CARGO_TERM_COLOR=always 注入)。教训:**生成-比对型校验门必须对环境自免疫**(行尾/排序 collation/工具着色),「本地幂等」不构成「跨环境确定」的证据。
- **许可谱系实查**:无纯 GPL/LGPL;5×MPL-2.0(cssparser 族/option-ext,弱 copyleft,未修改使用,NOTICE 已列明)+ jszip `(MIT OR GPL-3.0-or-later)` 双许可(经 epubjs 引入)**择 MIT 即合规**——脚本按「择一也须显式确认」从严旗标,NOTICE Review notes 留人工确认位。
- **边界**:归属清单非法律意见,上架前 license 兼容性终审/完整 license 文本捆绑(cargo-about 级)/渠道政策核验仍属人工环节,不因此免除。

---

## H. 2026-07-05 全面源码审查快照(结论 + 文档卫生待清理)

> 📦 2026-07-15 子节收官搬迁:H2 状态板整迁自 todo.md(零删减)。H1 取证表 / H3 架构性小观察 / H4 审查边界仍留 todo.md H 节(H3 含未接线的 `[profile.lite]` 与 schema 债等活项)。
>
> 📦 2026-08-23 H 节整节收官搬迁:H1 / H3 / H4 增补自 todo.md(零删减)。H3-2 `[profile.lite]` 未接线观察与 H3-3 schema 债(G4 已录)仍为活项,跟踪在 todo.md G4 节;H 节全文亦见 [reviews/2026-07-05-完成度审查快照.md](reviews/2026-07-05-完成度审查快照.md)。

> **2026-07-05 增补**:H 节 = 当日**全面源码审查快照**(见 [2026-07-05-完成度审查快照.md](reviews/2026-07-05-完成度审查快照.md)),独立实跑取证(`cargo check/test`、`vue-tsc`、`vitest`、`cargo tree` 全绿)+ 5 路子系统只读 agent 对账;结论 = 声称 ✅ 项几乎全部证实为真,唯查出 7 处文档 / 注释偏差(H 节表)。G3 thumbhash 项按「正文回写红线」当场订正(原「残缺 stub 全仓零消费」系误标)。
>

### H2. 查出的文档 / 注释偏差(7 处 · 已收官)

> 均为**注释 / 文档滞后于已推翻旧设计**(违「正文回写红线」),功能均正确。**2026-07-05 当日已全部清理完成**(用户指令;改后 `cargo check -p picasa-next` + `cargo clippy --all-targets -D warnings` 复验绿,stale 短语全库 grep 归零)。
>
> (表内 `picasa-next` 包名 / `crates/picasa-next-pro` 路径为**历史快照**——2026-07-06 已随 R2-7 改名为 `scrollery` / `crates/scrollery-pro`,见 [改名施工计划](decisions/2026-07-06-R2-7-改名施工计划-Scrollery.md);按「归档存证原样」不改写。)

| 状态 | 偏差 | 位置 | 修法 |
|---|---|---|---|
| ✅ 已回写 | thumbhash 误标「残缺 stub 全仓零消费」→ 实为完整 + 被消费特性 | `todo.md:800` | G3 订正 + 🔴 横幅 |
| ✅ 已清 | Part4 注释债 5 处仍描「双活 / 按 ai_backend 分流」(T16 已推翻);尤引用**已删除的 `state.ai_engine` 字段** | `worker_client.rs:8`、`worker_pipeline.rs:2,47`、`face_pipeline.rs:122,514,538` | 改写为「T16 后 worker-only 唯一路径」;死字段 → `ai-worker 子进程` |
| ✅ 已清 | pro 注释「永不 git add,被 .gitignore 排除」与现行 `.gitignore:52`(pro 已一等 member 跟踪)矛盾 | `crates/picasa-next-pro/Cargo.toml:2` | 改为「永不入公开仓 + Copybara glob 剥离」 |
| ✅ 已清 | 注释称「`[profile.lite]` 无对应 cargo profile」实际 `Cargo.toml:102` 已定义,且该体积 profile **从未被任何构建路径调用**(关系 <10MB) | `.github/workflows/release.yml:10` | 改为「profile 存在但本工作流不调用,接线属开放决策(见 H3)」 |
| ✅ 半清 | CHANGELOG.md 无 `[0.1.0]` 小节 → 现打 `v0.1.0` tag 会被 extract-changelog 硬门挡下 | `CHANGELOG.md:11` | `[Unreleased]` 已填真实首版特性清单;**`[0.1.0]` 切分 + 定日期属发布动作,留发布时**(2026-07-15 归档复核:该活尾巴由 CHANGELOG 自身「发布流程约定」+ `[Unreleased]` 头提示跟踪,不需 todo 另挂) |
| ✅ 已清 | README「known scroll bug」指向已废弃 `archive/perf_hardening_plan_v2.md`(T16 bucket 已替换);`ort` 仍列主后端(已迁子进程) | `README.md:11,20,23` | ort→ai-worker 子进程 / 坐标平移→bucket 虚拟化 / 指针→refactor_2026 |
| ✅ 已清 | `2026-07-02-Fable5全面审查报告` 称批量命令「仍收 Vec<i64>」已过时(5 写命令已迁 SelectionDescriptor) | 该审查文档 | 整张「宣称即险」表加一条过时横幅(四项差距均已收敛) |

### H1. 独立取证(客观信号,全绿)

> (2026-08-23 自 todo.md H 节整节收官搬迁,零删减。)

| 取证项 | 结果 |
|---|---|
| `cargo check --workspace` | ✅ exit 0,9 crate 全过 |
| `cargo test --workspace` | ✅ exit 0,~464 测试 0 失败(主包 387+4 ignored) |
| `vue-tsc` + `vitest run` | ✅ exit 0,207 测试全过 |
| `cargo tree -p picasa-next` 去 ort | ✅ 主包零 ort/tokenizers/ndarray(仅 ai-worker 载) |
| 后端 `TODO/FIXME/unimplemented!` | ✅ 零命中 |

**总评**:文档诚实度高——声称 ✅ 项几乎全部证实真实落地、无空壳 / 桩冒充;实跑测试数与声称逐字吻合。真正剩余工作集中在已如实标记的 ③b / Part8 / 证书链 / mac 几大块(快照 §2),受 R2-7 产品名死锁与外部基建阻塞。

### H3. 架构性小观察(proactive)

- ~~USE_PIPELINE 死代码(`thumbnail_commands.rs:24` + 两处 `if !USE_PIPELINE` 恒不执行)可按 Boy Scout 顺手清(G4 有意 defer)~~ **(🔴 2026-07-10 过时:已运行时化为 A/B 开关待实测裁决,见 todo.md N 节)**。
- `[profile.lite]` 定义未接线:免费版 release 走 `release` profile 而非体积优化 profile,若求 <10MB 极致裁剪须明确决策。
- schema 债 4 项(G4 已录)确认仍在。

### H4. 审查边界(未覆盖,需相应环境)

GUI 手测 / CI 真实运行(smoke·updater·.sig·bundle 内容物)/ 真机性能倍数 / mac / 联网——均无法从源码 + 本地实跑核实,快照 §5 已如实标注。

---

## I. 前端 UI 优化与多主题系统(2026-07-06 立项)

> 📦 2026-07-12 整节从 todo.md 收官搬迁(零删减);⏸ 真机残留三项(主题三态 / 迁移手测、Win11 标题栏着色、设置页逐项对照)留 todo.md I 节。

> 设计文档: [2026-07-06-前端UI优化与多主题系统.md](designs/2026-07-06-前端UI优化与多主题系统.md)(normative)。
> 拍板: 传统色五套双语名家族(墨 Ink/素 Porcelain/宣 Xuan/玄 Obsidian/黛 Dai);宣=全味道签名主题;范围全选。
> 核心约束: 一切主题的画布区守低饱和中性色(感知学红线);主题=纯 CSS 变量覆写文件+注册表元数据(商店级解耦,本轮不做 loader/manifest)。

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | S1 地基: 状态模型三键+迁移 / data-theme 单源 / 三态修复 / FOUC / 字体本地化 / 标题 favicon | `bf79f0b`;vue-tsc/eslint/vitest 221/cargo check 全绿(仅本地);GUI 三态+迁移手测待 S4 后统一 |
| ✅ | S2 主题基建: themes/ 迁移(porcelain/ink) + 注册表 + canvas·徽章·texture token + 契约测试 | `95d741a`;契约 6 断言入库,vitest 227 全绿(仅本地);内容照搬零视觉回归 |
| ✅ | S3 主题家族: xuan → obsidian → dai(每套对比度脚本留证) | `24bd718`;check:contrast 五套硬门槛全过(宣 accent 4.69/黛 7.62/玄 7.04);顺带修素存量缺陷 sidebar-active-text 4.21→5.93;色值锚点实取 zhongguose.com;纸纹观感待 GUI |
| ✅ | S4 切换 UI: ThemePicker + 外观 segmented + 侧栏三态 | `6a79937`;侧栏三态已随 S1;vitest 227 全绿(仅本地);GUI 手测批 2026-07-06 用户走查基本通过 |
| ✅ | 月白 Moonlight 干净清凉主题(GUI 手测「发脏」反馈追加,第 6 套) | `fa4f2ef`;冷偏灰+纯白面+发丝线+轻阴影配方,虹蓝/靛青锚点;六套门禁全过 |
| ✅ | 素/墨反哺「干净配方」(用户拍板同意) | `3e3cfd2`;素冷偏移+层级拉开+边框脆化+阴影减重,墨 elevated 拉开+边框脆化;玄/黛按裁决不动;六套门禁全过 |
| ✅ | Windows 标题栏真彩跟随主题(GUI 手测发现「未跟着变化」) | `5e66ef5`;根因=原实现只有明暗二态;DWMWA_CAPTION_COLOR/TEXT_COLOR 刷主题 chrome 色,前端 getComputedStyle 运行时取值(外置主题自动生效),Win10 自动降级;**着色效果待 Win11 真机确认** |
| ✅ | S5 批1: 硬编码色第一批(MediaThumb/Toast/AppShell)+图标残留+reduced-motion+骨架屏 | `399d710`;主题新增 4 token 六套齐键;骨架屏未触虚拟滚动引擎结构 |
| ✅ | S5 批2: 高可见组件收敛+「彩底白字」缺陷修+幽灵 token 清剿 | `a5cbe3e`;战果: 彩底#fff 暗主题不可读系统性修复(→text-inverse);幽灵 --color-bg-base 无 fallback 渲染透明 ×4(活 bug)/--color-danger ×3 全清 |
| ✅ | S5 尾: 交互态组件级补齐(工具栏/缩略图选中) + 空状态引导文案 | `9159b59`;.btn-icon/.chip 按压态+缩略图选中 accent 环+主题卡 hover/按压;空库出「添加文件夹」主按钮(事件复用 FoldersSection 完整 addRoot);顺修全局 .btn-primary 彩底白字 |
| ✅ | S6 设置页收敛: 注册式声明重构(行为零变化,单独 commit) | `bb03d53`;settingsMap 全声明式注册+SettingRow 外壳+五卡遍历渲染+DynamicSettingControl 按 control 类型分派(625 行链→3 张绑定表),净 -367 行;顺修 bucketScroll 钉住不显示/pin-btn --color-primary 幽灵 token/设置页彩底#fff ×3;**设置页逐项 GUI 对照待手测** |
| ✅ | 热修: MediaThumb 豁免注释「星号贴斜杠」拼出注释终止符致 vite 编译失败 | `e41e2d5`;教训=vue-tsc/eslint/vitest 均不过 SFC 样式解析,S5 尾起 vite build 纳入每批证据链 |
| ✅ | 真机反馈: 点击主题卡片即时生效(跨明暗分组联动外观模式) | `8d0729c`;原实现点卡片只写槽位不切外观,浅色下点深色「玄」须再点一次「深色」才可见;新增 pickTheme——非当前生效分组则显式切明暗模式,当前分组只换主题保 system 态;四门全绿(vitest 451/451) |

---

## J. 2026-07-06 全库代码质量审查(新增待修池)

> 📦 2026-07-12 从 todo.md J 节 4 段 🟢 实施进度 blockquote 迁移(零删减);pending(P2 余池 / GUI 终证)与逐批表格留 todo J 节。

### ▸ 详注 J-progress:施工实施进度(批 1-4 + 增量 + 安全加固对 + P1-21 架构渐进)

> **🟢 实施进度(2026-07-06 无人值守收官)**:批 1-4 + P2 池已落地,27 项过验证(仅本地全绿)。提交 `59de6e0`/`a753e9b`/`ef1987c`/`021da68`/`1422a53`/`665ff81`。逐条状态见[审查报告 §5.5 修复状态总表](reviews/2026-07-06-全库代码质量审查报告.md)。**订正**:P1-25 ci.yml 守门系 agent 过时发现(四 job 早已带 `github.repository` 守门)。

> **🟢 增量进度(2026-07-07)**:承接首轮的 📋「独立 PR 类」defer,分文件/分主题增量收敛 6 项 + 1 fmt chore,均逐项过验证。提交 `cf9b947`(P1-11 useTauriListen)/`c135a66`(P1-16 IPC 裸奔簇)/`5ee79c5`(P1-13 嵌套 watch)/`5ae4fd1`(R11/R23 锁中毒统一)/`c1b8a5c`(fmt chore)/`90f7e1c`(P1-17 invokeIpc+ESLint)/`544959f`+`1891d8f`+`b0092b4`+`d62a0f5`+`7ec918a`+`c7b40ac`(P1-8 String→AppError 六文件)。**三大系统性问题清零**:Result<_,String> 命令契约、raw invoke 双轨、锁中毒三套策略。**剩余仅**:⏸ GUI 真机验证(P1-14/23/C1)、📋 P1-21 架构渐进、P2 余池。

> **🟢 安全加固对(2026-07-07,用户在环)**:承接 top-4 系统性发现「权限面偏宽」最后一块,收敛 2 项——`422a806`(P1-23 CSP 删 `'unsafe-eval'`:vite 开 vue-i18n JIT 编译 + 两处 pdf.js `isEvalSupported:false`)、`e42963f`(C1 assetProtocol 静态 scope 7 整盘 glob→仅 `$APPDATA/**`,运行时授权早已完整闭环)。**均属 ⏸ GUI 失效面:代码就位 + 静态过验(前端 build/vitest 235、后端 cargo check 全绿,仅本地),运行时终证仅真机可证,故不计「已修」,状态=「代码就位/⏸ GUI 终证」**。GUI 验收清单见[报告 §5.5 末](reviews/2026-07-06-全库代码质量审查报告.md)。

> **🟢 P1-21 架构渐进(2026-07-07,用户在环,两具名目标已落地)**:uiStore 上帝 store(~19 域)渐进拆分,报告点名的 **toastStore + viewStore 两刀均完成**。**`478c1dc` toastStore**:toast 域(状态零耦合)独立成 `stores/toastStore.ts`,27 消费文件迁移,清 18 处闲置 `ui`;副产品消 `uiStore↔scanStore` 循环 import warning 一条。**`aedca27` viewStore**:四互斥视图筛选域(activeSmartAlbum/Directory/Collection/Person + 四 setter,含 clearSelection 耦合)独立成 `stores/viewStore.ts`,8 消费文件迁移,清 3 处闲置 `ui`,uiStore 净减 63 行。迁移安全网=先删源域让 vue-tsc 报全残留调用点 + eslint 揪闲置 ui;store 实例名用全名避命名遮蔽。两刀均 vue-tsc/eslint/vitest 235/build 全绿(仅本地)。**uiStore 由 ~19 域降至 ~17;进一步拆分超出报告具名范围,列 Boy Scout 可选(不一次到位)**。

> **逐批要点(2026-08-23 自 todo.md J 节表格行压缩而来,防指针断链)**:批1(`59de6e0`)=motion 原子写/useViewIds token+spec/file_ops Drop 守卫/clear_all_thumbnails 三件套/人脸框 token/pendingUpdate/pointercancel;批2(`a753e9b`)=派生 panic_guard/EXIF 偏移+2 测试/clear_cache 复位 derivation/audio_covers 纳入治理(热路径 LRU 重建=📋 设计决策);批3(`ef1987c`+`021da68`)=state.rs 下沉+tripwire 扩域/dest 净化/R7/R18/前端双源/字号 uiScale 单源/groupBy persist;批4(`1422a53`)=删 tauri-plugin-fs 全链/删 Google Fonts/verify_match 扩 7 类型/SETTINGS_MAP satisfies+SettingKey,CSP unsafe-eval 与 assetProtocol 代码就位(`422a806`/`e42963f`)仅剩 GUI 终证;P2 池(`665ff81`)=死代码删/pdfjs 重试/basename/幽灵事件/LIKE 转义+测试/webdav 状态码。

### ▸ 详注 J-P1-4:P1-4 竞写修复 + da80c46 诊断订正(2026-07-07 真机确认 / 2026-07-10 根治)

| ✅ | **P1-4 竞写修复**(GUI 实测暴露,与安全对无关):图像调度器冲掉前端渲染的 pdf/svg 封面 | `659bff0`;根因=FullThumbGen 领 thumb_status=0 时卷入 pdf/svg→generator UNSUPPORTED_TYPE 回写 thumb_status=2/NULL 冲掉 DocThumbRenderer 已写好的封面(derivation 仍成功)。修法=防冲(EXCLUDE_FRONTEND_DOC_THUMB 三查询排除 pdf/svg)+ 自愈(启动 reconcile_cover_thumbs 从权威派生产物回填,幂等非破坏)。生产库副本实证 id=11 healed 指向实存 webp(24460 字节)。**✅ 真机确认(用户 2026-07-07:重 build 重启后 PDF 缩略图正常显示,无需清缓存重渲)**。~~video/audio/epub 同源竞写留跟进~~ → **✅ 已根治(2026-07-10 `903a98f`,N 节)**:写边界防降级——`update_thumb_result` 对失败写(status=2)条件化 `WHERE thumb_status NOT IN (1,3)` + items_cache 同语义守卫,滞后失败写对全部格式(含未来新增流水线)一律不可降级已有产物,查询侧排除(本行修法)降为纵深防御。见报告 §5.5 P1-4 行 |

| 订正 | ~~da80c46 附带的「status=3 卡死是 PDF 占位之因」诊断~~ | 该诊断经生产库地面真相**证伪**(全库无 status=3 行,PDF derivation 实为 status=2 成功)。真因是上行 P1-4 竞写。da80c46(reset 覆盖 status IN(2,3))仍是有效健壮性改进(真失败文档应可经清缓存重试),代码保留,仅其叙事更正 |



---

## K. 阅读器完善(2026-07-07 立项)

> 设计文档 [2026-07-07-阅读器完善方案.md](designs/2026-07-07-阅读器完善方案.md) 为 normative 权威。以下为 R1–R5 已交付项的完整交付史(沿革 / 分叉裁决 / 病历 / 诚实边界),从 todo.md K 节表格单元格剪切而来,零删减。todo.md 表格行只留一句话结论 + commit + 「详注 K-N」指针。

### ▸ 详注 K-1:R1 文本管线地基(Rust)

**待办**:**R1 文本管线地基(Rust)**:编码 seam(修 GBK 三链路)+ 分章规则集 + 两级分段 + 索引缓存表 + 2 新 IPC

**2026-07-07 无人值守交付 `558fd74`**:reader/{encoding,text_index,paragraph}.rs + schema V13 + get_text_book_index/get_text_chapter,31 单测;GBK/GB18030/Big5/SJIS/UTF-16 编码全绿,分章负向断言/伪章兜底/偏移对拍全绿。

**+ 手动编码覆盖 `db914b5`**(reader_book_prefs V14,补完 §5.1 P0)。GUI 手测(GBK 真机阅读/编辑链路)留 R2 接渲染器时做

### ▸ 详注 K-2:R2 统一渲染器切换(vendor foliate-js,十四提交)

**待办**:**R2 统一渲染器切换**:vendor foliate-js + BookReader + epub 迁移(图/TOC/%)+ SyntheticBook(txt/md)+ 排版基线 + 设置面板 v1 + loc1 定位器 + 删 epubjs

方案 §7-R2;结构性核心,收口带 GUI 硬门(§9)。**2026-07-07 无人值守推进(用户在环→无人值守)十四提交**:

- `52da98d` **R2-1** vendor foliate-js(钉 `78914aef`,剥 PDF 后端/13M blob→pdf.js stub;eslint/prettier/view.d.ts 就位;NOTICE 补 VENDORED_FRONTEND 登记 + 顺修 R1/R4 引入 crate 未同步 drift 402→417)
- `c807a71` **R2-2** BookReader(包 `<foliate-view>`,epub 分发从 EpubReader 切走;替换规则挂 `load` 事件、进度 `cfi:` 兼容;体积 353→77kB 瘦身 4-5 倍;**epub 运行期 ✅ 2026-07-07 真机四点全过**:开卷即图/翻页/滚轮/键盘/替换规则/旧 cfi 恢复)
- `ec2d063` **R2-7 核心** loc1 定位器算法(编解码 + 三级恢复 + 旧值兼容,21 单测;接线待消费方)
- `dd41a38` **R2-4** txt 入 foliate 管线:syntheticBook.ts(buildTextSyntheticBook,章=section,load()按需拉章→XHTML blob URL 懒加载即"滑窗",size=R1 charLen 当进度权重;浏览器 API 走 DI seam→node 环境可单测资源生命周期,17 测)+ BookReader textSource prop + DocumentViewer 路由 txt→BookReader(md 暂留 TextReader);进度走 foliate 原生 CFI(合成 book fake-CFI 确定性 DOM→跨重启可解析),旧 scroll: 比例迁 goToFraction;**txt 运行期 ✅ 2026-07-07 真机六点全过**(开卷/翻页/滚轮/键盘/替换/进度恢复)
- `b312f3f` **R2-3a** foliate 阅读流切换(翻页↔滚动,txt/epub 统一——同一 `<foliate-view>` 的 flow 属性,一次实现两格式;补回 R2-4 迁 txt 时丢的滚动阅读=回归修复;运行时 setAttribute 触发 render() 重排、位置 foliate 保持、无需 remount;滚轮在 scrolled 让位给容器原生滚动;持久化 doc_reader_flow;i18n readerFlow/flowPaginated/flowScrolled;**scrolled 运行期 ⏸ GUI 待真机**)
- `049bd73` **R2-5a** 排版基线(readerStyles.ts:buildReaderCSS 纯函数产出注入 CSS,5 单测;全局字号 19px/行距 1.75/CJK 衬线栈/图片自适应对全格式,txt 段首缩进 2em/段距/章标题居中用 `.sate-chapter` 隔离;挂载读主题 `--color-text-primary`/`--color-bg-primary` 解析值内联,setStyles 入 before 槽=低优先 → txt 全量生效、epub 书样式经 cascade 胜出**零回归**;foliate 每次新章自动重注;**观感 ⏸ GUI 待真机**)
- `4147903` **R2-5b** epub 自带↔智能排版切换(用户提出「某些 epub 无自带格式需切换」):同一 setStyles 换槽——**自带**=before 槽(书样式胜,默认零回归)/**智能**=after 槽 + `!important` + `body *` 通配覆盖字体行距 + 全局 `p` 缩进对齐(压过 epub 高特异性书样式);buildReaderCSS 加 override 选项(+2 单测);BookReader styleMode prop + watch 运行时切换(setStyles 后 foliate `#view.expand()`+ResizeObserver 自动重排、保位、不 remount);工具栏 epub 专属选择 + 持久化 doc_epub_style_mode;i18n styleMode/styleBook/styleReader;**智能排版对真实差 epub 实效 ⏸ GUI**)
- `27fca36` **R2-4b** md 入 foliate 管线(整篇单 section + 复用项目**零依赖 renderMarkdown**,修正早前「需 markdown-it/DOMPurify」判断):buildMarkdownSyntheticBook + buildMarkdownHTML(**text/html MIME** 因 renderMarkdown 出 HTML5 的 `<br>`/`<hr>`;外层 `.sate-md` class 作排版钩子)+6 单测;readerStyles 加 `.sate-md` 排版(标题/码/列表/引用/分割线,中性灰 rgba 边框兼容明暗);BookReader resolveOpenTarget 按 isMarkdown 分支(md 取 `GET_DOCUMENT_TEXT` 全文);DocumentViewer md 改路由 BookReader + 移除 TextReader 导入(**孤儿化,已不入包,R2-8 删**);**md 观感 ⏸ GUI**)
- `cf22862` **R2-6a** 排版设置面板(字号 12-32px/行距 1.2-2.4 步进旋钮,实时生效 + 全局持久化):ReaderSettingsPanel.vue(纯展示,emit change,值钳制 + 0.05 网格吸附防浮点毛刺)+ BookReader `typography` prop→buildReaderCSS + watch(deep) 重注(foliate setStyles 后自动重排、保位、不 remount)+ DocumentViewer 工具栏 Type 按钮开合面板 + 持久化 doc_reader_font_size/line_height + i18n;**实时调节观感 ⏸ GUI**)
- `f382230` **R2-6b** 排版旋钮扩展(字体族 衬线/黑体 + 栏宽 max-inline-size + **换主题实时重着色**:watch uiStore.resolvedThemeId 重读 `--color-*` 重注,因 iframe 不继承 app 变量)
- `e2ffa29` **R2-6c** 每书 编码/重排(txt):设置面板每书区(编码下拉 auto/UTF-8/GBK/GB18030/Big5/SJIS + reflow 开关);编码由后端从 prefs 自动读、reflow 经 get_text_chapter 参,改动 set_reader_book_prefs + reloadToken 重挂载重解码/重分段;简繁转换归 R4。**R2-6 设置面板完整(字号/行距/字体/栏宽/主题响应/编码/重排)**)
- `4d3ec41` **R2-3** 导航面板(全书进度页脚% + TOC 目录树):BookReader onRelocate 加 emit locate{fraction,tocLabel}(relocate 的 fraction 即全书 0..1)、ready 后 emit toc(book.toc)、暴露 goToHref(view.goTo 解析 href);TocPanel.vue(book.toc 扁平化 + depth 缩进渲染,避免递归组件);DocumentViewer 工具栏 List 按钮 + 进度页脚(百分比+章名)+ onTocNavigate→goToHref;txt=章目录/epub=原生 TOC/md=空(heading TOC 增强);**导航实效 ⏸ GUI**)
- `2b4b596` **R6 仿真翻页**(钦点必做):pageTurn 三档(关/滑动/仿真);slide/curl 启 foliate 内建 `animated` 属性(paginator.js:901 门控 300ms easeOutQuad 滑动),curl 额外叠 `.book-curl` overlay(卷边光影随方向扫过,纯 CSS 不依赖快照,prefers-reduced-motion 关);设置面板段控 + 持久化 doc_reader_page_turn;**诚实边界**:curl 是卷边光影近似,真·内容卷页受 iframe 快照约束(Chromium 不可靠)列为专项,默认 slide 稳妥;**动画观感 ⏸ GUI**)
- `5cc421d` **R2-8 退役**:删 TextReader/EpubReader(孤儿)+ 移除 epubjs 依赖(npm 清 30 包)+ 重生成 NOTICE(64 npm,无 epubjs);dist 实测无 epubjs 残留。

**R2 结构主体完整(渲染归一 txt/epub/md + 阅读流 + 排版基线/模式 + 设置面板 + 导航 + 翻页动画 + 退役旧渲染器),用户 2026-07-07 真机验收通过 ✅**)。

**裁决**:基础进度用 foliate 原生 CFI,loc1 canonical 精密重锚(跨字号/重排/简繁仍精准)拆为可单测的独立跟进(需 DOM-range↔canonical 偏移映射);md 复用已有零依赖 renderMarkdown(**修正:不需 markdown-it/DOMPurify 新依赖**),因其输出 HTML5 故 section 用 text/html MIME。

**余待**(用户已真机验收 R2 主体,以下转入后续 GUI 迭代,属增强/后续期,非本轮钦定范围;简繁 2026-07-07 已接线见 R4 行):loc1 精密重锚接线(R2-6c reflow/编码切换会改 canonical 令 foliate CFI 断→loc1 canonical 重锚此时才显价值;DOM-range↔canonical 映射纯函数可单测,但 range→段落映射须真机验)、md heading 派生 TOC(单 section 内锚点导航,需 renderMarkdown 加 heading id + resolveHref 带 anchor)、简繁转换(R4,convert_chinese 已前拉,需 load 事件 per-chapter convert)、真·内容卷页(R6 专项,受 iframe 快照约束)

### ▸ 详注 K-3:R3 设置与主题深化

**待办**:**R3 设置与主题深化**:每书 prefs 表 + 主题日夜配对/纹理 + contrast gate 扩展 + 排版全参数 + SETTINGS_MAP「阅读」节

方案 §7-R3(正文已回写交付说明)。**2026-07-07 无人值守四提交交付**:

- `cf... cb77414` **R3-a1 纯核**(utils/color.ts 色彩数学单一归宿:parseColorToRgb 唯一解析器 + isDarkColor 从 shikiHighlight 迁入 re-export + WCAG relativeLuminance/contrastRatio;themes/readerThemes.ts 阅读专属调色板 纸白/羊皮纸/护眼 light + 夜间/石墨 dark,与 app chrome 主题**分命名空间**,FOLLOW 哨兵=跟随应用零回归默认,日夜双槽同构 app theme_light/dark,normalizeReaderThemeId 挡跨槽误存;reader-theme-contract.spec.ts **契约 + WCAG AA≥4.5 对比度门禁** 防「好看但读不清」底色;readerStyles texture 选项 羊皮纸纯 CSS 径向渐变叠色 CSP 安全;+20 单测)
- `862c93b` **R3-a2 接线**(BookReader resolveReaderColors 据 app 明暗择日/夜槽 → 每书覆盖 kind 相符才生效 → 落主题色,FOLLOW/未知回落 app 颜色**逐像素零回归**;md 代码高亮按生效阅读背景明暗选码主题;ReaderSettingsPanel 顶部阅读主题选择器 只改当前明暗槽 + 每书「本书主题」;DocumentViewer 日夜槽 ref + appIsDark + 持久化 doc_reader_theme_light/dark + **每书主题纯显示层不重挂载**比对 canonical 字段;types ReaderBookPrefs.theme)
- `310b734` **R3-b 排版全参数**(readerStyles ReaderTypography += fontWeight/letterSpacingEm/textAlign/titleScale 各带默认零回归,override 分支字重不压 h1-h6;面板 字重段控/字距步进/对齐段控/章题缩放;DocumentViewer 四 ref + 持久化 + 启动 loader 守卫;+2 测;eslint allowlist += 'em')
- `7ba6d69` **R3-d 设置页「阅读」节**(ReaderSettingsSection.vue 独立组件循 NetworkStorage/KnownVolumes 惯例,日/夜两组阅读主题卡片 样张真实色对 + 「文」预览,读写同一组 doc_reader_theme_* 键)。

**两处偏离已回写正文**:①「宽度三档」由既有连续栏宽(R2-6b 480-1040)取代不另做;②「SETTINGS_MAP section」改独立 section 组件 + **范围收窄**(设置页只放日/夜主题配对=开书前就想定 + 唯一双槽处,字号/排版等边读边调项只留书内面板,规避前端双源漂移;SettingsSection 类型/SETTINGS_MAP 注册表未改)。

tsc/eslint/vitest 318(+22)/build 全绿(仅本地),主 bundle 490.77→494.19KB(+3.4KB=设置节组件)。**用户 2026-07-07 真机验收通过**(阅读主题日夜切换/羊皮纸纹理/每书主题/字重字距对齐章题/设置页阅读节)→ R3 ✅ 收口

### ▸ 详注 K-4:R4 功能面(三面板 / 简繁 / md 高亮 / auto-scroll / 沉浸)

**待办**:**R4 功能面**:TOC/书内搜索/书签 三面板 + 简繁(convert_chinese)+ md 插件位(shiki 懒载)+ auto-scroll + 沉浸模式

方案 §7-R4;**2026-07-07 简繁后端前拉**:ferrous-opencc + convert_chinese 命令 + 6 单测(s2t/t2s/s2twp 词汇转换实证),前端常量/类型就位。

**2026-07-07 简繁前端接线交付**(承 R2 收口):`zhConvert.ts`(DOM 文本节点整批转换,跳过 code/pre/script 子树,长度守卫防半程污染;2 纯函数 + 6 单测)+ BookReader `load` 事件挂点(替换规则后按 §5.13「替换→简繁」顺序施加,一章一次 IPC,纯显示层不改后端 canonical)+ ReaderSettingsPanel 档位下拉(关/s2t/t2s/s2tw/s2twp,`isTxt` 门控编码/重排、简繁对全格式显示)+ 每书持久化(`reader_book_prefs.zhConvert`,后端宽松 `serde_json::Value` 解析忽略此字段无副作用)+ **txt/md/epub 全格式通用**;切换走 `reloadToken` 重挂载(转换不可逆,让 load 按新档位对原文重转);tsc/eslint/vitest 293/build 全绿(仅本地),**运行期观感 ⏸ GUI 待真机**。

**2026-07-07 书内搜索交付**(`6ff46ea`):BookReader 暴露 foliate 原生 `search` 异步生成器 + `clearSearch`(命中跳转复用 goToHref,CFI/href 皆可解析)+ SearchPanel(回车提交因全书开销大不逐键 debounce/结果按章分组 label=章名/命中片段 pre+`<mark>`match`</mark>`+post 两行截断/打开即聚焦/状态行 搜索中%·无结果·命中计数)+ DocumentViewer 编排(流式收集 {progress}/{label,subitems}/'done',**代次令牌 searchGen 防换查询·换书旧迭代串档**,命中由 foliate Overlayer 在渲染视图自动高亮、关闭/换查询 clearSearch 清除,**TOC/设置/搜索三面板互斥**同一抽屉位)+ view.d.ts 收紧 search 契约类型;**三格式统一**(section.createDocument R2-4 已预埋)。**诚实边界**:匹配章源文本、非替换/简繁后显示文本(foliate 架构固有,简繁关时一致;命中高亮按 CFI 结构位置仍准)。未加单测(引擎 vendored 上游已测/编排层耦合组件难 node 测,已注明),tsc/eslint/vitest 293/build 全绿(仅本地),⏸ GUI。

**2026-07-07 书签交付**(后端 `28b7fb6` + 前端 `ad958e7`):schema **V15 reader_bookmarks**(`UNIQUE(item_id,locator)` 幂等 + `idx(item_id,fraction)` 按进度 + 事务化迁移)+ list/add/delete 命令(全走 read/write_blocking,tripwire `ipc_commands_keep_rusqlite_off_async_workers` 绿,**cargo 全 lib 450 测**含 round-trip 单测)+ BookmarkPanel(头部「+」加当前位置/条目 章名·%/悬停显删防误删)+ BookReader 暴露 `getCurrentLocation`(最近 relocate 的 cfi:/进度/章名快照)`/goToLocator`(剥 cfi: 前缀交 goTo,loc1 可扩展)+ **四面板 TOC/设置/搜索/书签互斥**(同一抽屉位)。locator 用 `cfi:` 与阅读进度同源(loc1 落地可平滑换);tsc/eslint/vitest 293/build 全绿(仅本地),⏸ GUI。**R4 三面板(TOC ✅/搜索 ✅/书签 ✅)全交付**。

**2026-07-07 R4 收尾三项交付**:**沉浸模式**(`5ce5d5d`,工具栏 v-show 隐藏 + 右上浮动退出 + Esc,进入收起所有侧栏)+ **auto-scroll**(`5ce5d5d`,BookReader 间隔 `view.next()` 定时器统一分页/滚动流,工具栏 ▶/⏸ 瞬态开关,设置面板 2-30s 步进调速全局持久化)+ **md 代码高亮**(`79e2f6f`,shiki 4.3.1 **JS 引擎懒载**——`createJavaScriptRegexEngine` 避 WASM 守 CSP,主包零增长/core+引擎+按语言全独立 chunk 仅打开含代码 md 才拉;renderMarkdown 代码块打 language-class,双主题按阅读页背景明暗选;isDarkColor 纯函数 +3 单测;NOTICE 重生 npm 64→109)。**R4 line 全部子项交付**(三面板 + 简繁 + md 插件位 + auto-scroll + 沉浸),**用户 2026-07-07 真机验收本批(沉浸/auto-scroll/shiki 高亮)通过 → R4 ✅ 收口**。

**md heading TOC 为额外增强(非 R4 line)已 defer**——需 foliate 单 section fragment 锚点解析 + splitTOCHref/getTOCFragment 对字符串 fragment 语义(触 TOCProgress 内部有回归风险)。tsc/eslint/vitest 296/build 全绿(仅本地,主 bundle 490KB 未因 shiki 增大),⏸ GUI

### ▸ 详注 K-5:R5 竖排竹简模式(R5-a 核心交付,R5 整体进行中)

**待办**:**R5 竖排竹简模式**(品牌签名):轴对换翻页语义 + 字体包下载位 + 真机矩阵验收

方案 §7-R5(正文补激活机制说明)。**2026-07-07 R5-a 竖排核心交付 `c85d69b`**:

- 勘查确认 **vendored foliate paginator 原生支持竖排**——getDirection 从 doc.body 计算样式读 writing-mode,`#vertical` 一路驱动分页/滑动 dx↔dy/滚动轴/snap/overlayer/dir=rtl(**轴对换翻页语义=渲染器内建,非自研**,大幅降 R5 风险);
- 激活点=paginator 每 section load 的 onLoad 先 setStyles 再 dispatch 'load'(均在 getDirection 前)。
- 实现:BookReader vertical prop(off/vertical-rl/vertical-lr)+ onLoad 把 writing-mode 落 doc.documentElement(可继承→body,getDirection 前就位)+ 横排零回归;面板版式 select(横排/竖排右起/竖排左起);DocumentViewer readerVertical ref + 持久化 doc_reader_vertical + **竖排↔横排切换唯一需 remount**(getDirection 仅 section load 探测,reloadToken++,位置由 CFI 恢复);i18n layoutMode 系。tsc/eslint/vitest 318/build 全绿(仅本地),主 bundle +0.2KB。

**余项(未起)**:

- 字体包下载位(重基建,复用模型库下载,设计留后续)/
- 竖排排版参数联动(行距→列距语义、logical 属性调优,§7-R5 明列为设备调优项)/
- **真机矩阵验收(§9,竖排 WebView2 实际渲染观感 + CJK 字体可用性,只能真机证)**。

**运行期 ⏸ GUI 待真机**

### ▸ 详注 K-6:2026-07-08 收尾波(折叠分组 + 缺陷修复四项)

**2026-07-08 收尾波(无人值守)**:

①**阅读设置面板折叠分组** `104a6a0`——新增 ReaderSettingsGroup(轻量可折叠 + localStorage 持久化;刻意不复用设置页 CollapsibleCard,以免污染其全局「一键全部折叠」协调器),面板重构为 主题/排版/翻页/本书 四组(主题、排版默认展开,翻页、本书默认折叠,持久化记住偏好),面板标题正名「阅读设置」,+4 组标题 i18n(localeIntegrity 校验双语对齐)。

②**缺陷排查修复四项** `e769f81`(两路只读子代理审查阅读器子系统 + orchestrator 独立核实;**竖排物理轴属性专审=干净**,证 R5-b 逻辑属性改造彻底):

- (a) remount 保位——简繁/编码/重排/竖排切换 reloadToken++ 重挂载前用 getCurrentLocation 刷新 initialPos,原会跳回开卷位(真回归);
- (b) escapeXml 剥 XML 1.0 非法 C0 控制字符——脏/误解码 txt 免整章 parsererror,码点循环规避工具管道转义坑,+1 测含空格/标点防误删回归线;
- (c) onTypographyChange 14 写降为只写变化键(去热路径写放大);
- (d) TocPanel activeHref 接线——当前章高亮由 BookReader locate 事件透传 tocItem.href。

**已报未修(低 · latent 且触发需 foliate 并发双请求同章未证 · 收益边际,测试季不动受测热路径,defer)**:txt section.load() blob URL check-then-act 竞态(并发 load 同章泄漏一个 URL)。

四门全绿(仅本地):tsc 0 / eslint 0 / vitest 321(+1)/ build 0,主 bundle 494.71KB。运行期(尤 remount 保位 / 折叠观感)⏸ GUI 待真机。

---

## L. 顶栏重构 · 自绘标题栏 + 上下文工具栏(Command 化)(2026-07-08 立项)

> 📦 2026-07-12 从 todo.md L 节各阶段单元格迁移(零删减);normative 权威仍是 [设计文档](designs/2026-07-08-顶栏重构-自绘标题栏与上下文工具栏.md)。todo.md L 表只留状态 + 一句话结论 + commit + 指针,进行中(🔨)项的 pending/⏸GUI 上下文留 todo。

### ▸ 详注 L-intro:立项裁决 + 施工前评审

> 设计文档:[2026-07-08-顶栏重构-自绘标题栏与上下文工具栏.md](designs/2026-07-08-顶栏重构-自绘标题栏与上下文工具栏.md)(normative,含四路调研取证 / 六层架构 / 数据契约 / 分阶段计划 / 风险 / GUI 验收清单 / OS 集成预留)。
>
> **用户 2026-07-08 三裁决**:①标题栏布局=**自适应单行**(内容页单行合并,密集网格视图保留第二行「🔴 2026-07-09 修订:改为画廊亦并入标题栏单行 + L5 溢出,见 §5 Phase G」);②Windows=**完整 frameless + Snap Layouts**(`tauri-plugin-frame` 走 `HTMAXBUTTON` 官方路径,mac=`titleBarStyle:Overlay` 保原生红绿灯)「🔴 2026-07-08 后续裁决修订:**Snap Layouts 本期 defer**(插件审计不过,frameless 自绘照常交付),见 Phase 1 行与设计文档 §2;2026-07-10 补此注对齐」;③查看器**统一为 shell 内路由 + `activeViewer` 单源**(看图台从 body 覆盖层迁路由,为未来「用 Scrollery 打开 / 默认图片浏览器 / 双击直达大图」的 OS shell 集成铺可寻址地基)。核心=六层解耦骨架(WindowChrome / viewerStore / CommandRegistry(VSCode 贡献点范式)/ ContextualToolbar / useToolbarOverflow / 自定义预留);用户自定义 + OS 集成本期只**做骨架预留**不出 UI。承接「开发期不冻结契约」:默认呈现集/分色/键位真机前不定死。
>
> **2026-07-08 施工前评审回写**(取证逐条核实全吻合,六层架构与分期不变,六处修正):①双击最大化系 Tauri 2.11.4 内建(drag.js `detail===2`,**勿自实现防双 toggle**);②新增设计文档 §4.6 capability 增量清单(`core:window:default` 只含 getter,`start-dragging/minimize/toggle-maximize/close` 等全须显式加);③**P4-5 网格保活与滚动恢复**(高风险补项:现状覆盖层刻意保活网格,迁路由须 KeepAlive/滚动锚点,全仓现状零 KeepAlive 零 scroll restore);④P5-6 键位同源(tooltip 与监听共用 registry keybinding);⑤plugin-frame 职责边界钉死(插件不画三键但注入点击/hover 脚本,倾向 vendor 剥注入自绑);⑥mac 补 `setTheme` 明暗同步(废 DWM 染色 ≠ 废明暗同步)。

### ▸ 详注 L-P0:Phase 0 地基

**待办**:**Phase 0 地基**(纯新增零回归):平台工具 `platform.ts`(引 `@tauri-apps/plugin-os`+`os:default` cap)+ `viewerKind.ts`(纯函数+单测)+ `viewerStore.ts`(activeViewer 单源)+ `commands/types.ts`(Command/Context 骨架)+ CSS 变量 `--titlebar-height`/`--titlebar-controls-inset`

**✅ 2026-07-08 交付**:五部件全落地。platform.ts=plugin-os 2.3.2 同步 `platform()` 缓存(非 Tauri 上下文 try/catch 降级 windows)+ Rust 侧 `.plugin(tauri_plugin_os::init())` + `os:default` cap;viewerKind.ts=`resolveViewerKind(mediaType,fileFormat)` 纯函数(md 升一等 markdown 类,**刻意无 unsupported**——不可渲染文档由分发层前置拦截,4 测);viewerStore.ts=`activeViewer` shallowRef 单源 + populate/clear/**patch/setImmersive** + **token 时序防御**(旧查看器迟到 clear 不误清新态,5 测建立 pinia store 测试范式);commands/types.ts=Command/CommandGroup/CommandContext/**CommandStores**(type-only `ReturnType<typeof useXxxStore>` 精确类型化 media/history/view/filter,零 any)。**验收全绿(仅本地)**:vitest 330(+9)/vue-tsc 0/eslint 0/cargo check exit 0(plugin-os 编译过)/cargo fmt 净。无 GUI 面

### ▸ 详注 L-P1:Phase 1 自绘标题栏 chrome

**待办**:**Phase 1 自绘标题栏 chrome**(L1):tauri.conf mac Overlay+trafficLightPosition / Win `decorations:false` + ~~`tauri-plugin-frame`(Snap Layouts)~~**🔴 Snap Layouts 本期 defer(2026-07-08 用户裁决,审计=单人维护/下载 3236/3 yank/JS 注入冲突六主题;设计文档 §9.1 已回写)——不引第三方、frameless 自绘照常交付、`--titlebar-controls-inset` 由 WindowChrome 自算** + `WindowChrome.vue`(拖拽区+平台分叉三键+**双击最大化系 Tauri 内建勿自实现**+红绿灯避让+P1→P3 过渡态=标题栏行暂空、旧 AppToolbar 留第二行)+ capability 按设计文档 §4.6 清单增补(`core:window:allow-start-dragging/minimize/toggle-maximize/close` 等均不在默认集)+ 废弃 Win DWM `set_window_theme` 染色链改 CSS(**mac 补 `setTheme` 明暗同步**)

**🔨 代码就位 ⏸GUI 待真机(2026-07-08)**:tauri.conf mac 三字段(decorations 保 true 供 trafficLightPosition schema 约束)+ 非-mac setup 运行期 `set_decorations(false)`(窗口 hidden 时翻,无原生栏闪烁)+ WindowChrome.vue(平台分叉:mac 留白避让红绿灯/非 mac 自绘 min·max·close 三键走 getCurrentWindow(),双击=Tauri 内建不自绑,close 复用 CloseRequested 自定义流零回归,inset 观测三键宽/mac 固定 78px)+ AppShell 改列向(titlebar 槽全宽置顶 + app-body 行)+ App.vue titlebar 槽 + 废 DWM 链(删 set_window_theme/hex_to_colorref/handler 注册/SET_WINDOW_THEME 常量,改前端 setTheme 明暗同步)+ i18n 三键键双语。**验收全绿(仅本地)**:cargo check/fmt 0、vue-tsc 0、eslint 0、vitest 330。**⏸GUI 终验**:Win10/11 拖动/双击(无双 toggle)/三键/八向 resize/最大化不越屏 + mac 红绿灯位置·主题 hover + 主题切换标题栏染色 + 全程无 ACL 拒绝。**已知过渡态限制**:MediaDetailOverlay(body 覆盖层)开启时盖住自绘标题栏(Win 窗口三键/拖拽被遮,Esc/Alt+F4 仍可用)→ Phase 4 覆盖层路由化解决

### ▸ 详注 L-P2:Phase 2 CommandRegistry + 网格命令化

**待办**:**Phase 2 CommandRegistry + 网格命令化**(L3 先行):`registry.ts`(注册/查询/when/执行+单测)+ `builtins/global.ts`+`grid.ts`(现有网格操作登记,复用 store action,**新增复制路径**)+ 收敛 MediaGrid/MediaDetailOverlay 两处右键菜单硬编码→注册表

**✅ 2026-07-08 交付**:registry.ts(factory+单例双出/query 按 when·group·order/run 守卫 when·isEnabled,8 测)+ context.ts(buildCommandContext DRY)+ builtins/global.ts(copyImage/showInExplorer/setWallpaper,**壁纸-仅图片条件单点定义**)+ contextMenu.ts helper(共享命令 + 宿主本地 move/copy 按 order 合并)+ index.ts registerBuiltins + main.ts 接线。**收敛 MediaGrid + MediaDetailOverlay 右键菜单**:共享项走注册表,move/copy 作本地 Command(捕获各宿主对话框实例,零回归);**行为对拍测试 5 项**(图/视/未知/无目标 × 菜单组成·顺序·壁纸条件)。**验收全绿(仅本地)**:vue-tsc 0/eslint 0/vitest 343(+13)。**三处 scope 调整**:①网格工具栏命令(undo/redo/全屏/布局/排序)**延后 P3** 随 GridToolbar 消费方登记(避免此刻猜 title/icon/group);②**复制路径 defer**(agent 实证全库无此能力=新增,需后端剪贴板文本 IPC + 取绝对路径,不在「收敛既有」范围);③**CommandContext 去 stores 字段**(原设计塞 stores 逼每次 ctx 构造实例化全部 store,含 window 副作用的 uiStore→node 测试炸;改命令 run/when 内直接 useXxxStore(),更惯用)

### ▸ 详注 L-P3:Phase 3 ContextualToolbar + 自适应单行 + 溢出

**待办**:**Phase 3 ContextualToolbar + 自适应单行 + 溢出**(L4/L5):`useToolbarOverflow.ts`(ResizeObserver+宽度缓存 Priority+)+ `ContextualToolbar.vue`(navigation 组=主按钮/其余=⋯溢出;a11y=role+roving tabindex+aria-label+tooltip 含快捷键)+ 现 AppToolbar 瘦身为 `GridToolbar.vue`(网格第二行)+ App.vue `#toolbar` 换 ContextualToolbar

**🔨 网格半边交付 ⏸GUI(2026-07-08)**:**P3-1** `useToolbarOverflow`(computeOverflowSplit 纯核 6 测 + ResizeObserver 缓存 composable,`6227e2d`);**P3-2** `builtins/grid.ts`(撤销/重做/全屏 nav 命令)+ `ContextualToolbar.vue`(标题栏内容槽,route-gate 判网格,navigation 组主按钮 + role/aria-label/aria-pressed)+ 三键从 AppToolbar 移除(键盘 Ctrl+Z/Y·F11 不动)。vue-tsc/eslint/vitest 349 绿。**关键相位发现(已回写 §5)**:ContextualToolbar 的**上下文切换依赖 P4**——activeViewer 要 P4 才 populate,图/视覆盖层无路由,故 P3 只能靠 route-gate 判「非内容页」渲染网格命令,**内容页上下文 + 溢出接线 + GridToolbar 完整瘦身 + roving tabindex 均与 P4/P5 耦合待续**。AppToolbar→GridToolbar 改名 defer(现文件即网格密集工具栏,改名属 cosmetic)

### ▸ 详注 L-P4:Phase 4 查看器统一为 shell 内路由

**待办**:**Phase 4 查看器统一为 shell 内路由**(L2 落地,A∪C):新增 `/view/:id`(`?path=` 预留外部文件)+ `ContentViewer.vue` 分发 + **迁 MediaDetailOverlay→ImageViewer/VideoViewer**(保留 useMediaDetail 内核,去 body Teleport,补 defineExpose ViewerApi)+ 各查看器 populate viewerStore + 沉浸模式统一 + 分发入口统一 `router.push('/view/'+id)` + **P4-5 网格保活与滚动恢复**(2026-07-08 评审补:现状覆盖层刻意保活网格,迁路由须网格路由 `KeepAlive`+`onActivated` 与虚拟滚动引擎再同步 或 store 滚动锚点恢复,否则最高频开合路径重挂丢位)

- 依赖 P3。
- **高回归 ×2**:useMediaDetail 迁移 + 网格保活(均先补 characterization test 锁行为;T16 教训:原生条只跟物理 scrollTop)。
- **⏸GUI**:图/视/文/音四类 shell 内渲染+翻页/缩放/信息/沉浸/前进后退+**返回网格保位不重排**。
- **🔨 迁移前安全网已就位(2026-07-08)**:`useMediaDetail.spec.ts` characterization test(缩放 ×1.25·×0.8 钳位/缩放模式 original·fit-width·fit-height 数学/cycleZoomMode 循环/fitToWindow 不放大/onWheel ctrl 门控/拖拽 scale>1 门控 + translate 更新 + stopDrag 停更,8 测,node + 最小 document 桩不引 jsdom)锁死内核行为,迁移中内核保留不变即零漂移。
- **P4-a 网格重挂载滚动恢复已交付 ⏸GUI(2026-07-08)**:勘查确认 `scrollCache`(模块级 Map,scrollCache.ts 头注本就为「重挂载恢复」而设)此前**仅在 layoutVersion watcher 读回**,组件重挂载(异组件路由返回,版本未变)走 onMounted 不触发该 watcher → 落回 scrollTop=0 丢位;覆盖层时代被「Teleport 保活网格永不卸载」掩盖。修复=onMounted compute 后 `await nextTick` 再读 `scrollCache.get(getViewKey())` 回设 scrollTop(bucket 走 scrollToLogicalY),与 watcher 恢复逻辑同构、幂等。
- **纯加法不碰覆盖层,顺带修好现存 /collections·/persons·/doc·/audio 跨组件路由返回丢位**;是「返回网格保位」的地基,先于看图台迁移落地以隔离虚拟滚动风险。滚动恢复系 DOM+异步+IPC 绑定,node 无 DOM 环境无有意义单测面,**靠 tsc/eslint + 357 测不回归 + 真机验收行为**(⏸GUI 验:滚动网格→开文档→返回,滚动位保持)。vue-tsc/eslint/vitest 357 绿(仅本地)。
- **P4-b 覆盖层→路由迁移已交付 ⏸GUI(2026-07-08)**:新建 `ContentViewer.vue`(cp 覆盖层再外科手术式改 delta,复用逐字节验证过的 useMediaDetail 内核)——去 body Teleport/backdrop、三层容器填充 .app-content;路由驱动(`/view/:id` 的 :id 为当前项单源,`loadFromRoute` 守卫防翻页 replace 重载,`navigate()` 邻接换项后 replace 同步 URL);close 走 router.back(无 back 记录回退根);
- **populate viewerStore(kind 由 resolveViewerKind 推,按 detailItem 变化 patch)+ defineExpose(ViewerApi)**;派发切换 MediaGrid.handleCardClick + FoldersSection.onFileClick 图/视→`router.push('/view/'+id)`;新增路由 + `routes.view` 双语键;
- **退役删除 MediaDetailOverlay.vue**(App.vue 去挂载/去 import,全仓无残留引用)。
- **设计偏离三项已回写正文§P4 横幅**:合成单 ContentViewer 不拆 Image/VideoViewer(DRY,图视共享 ~90%)/ 落 components/media / 沉浸留 P4-c;另顺修 onMoveCopyConfirm「未 await 恒判最后一张」时序 bug。vue-tsc/eslint/vitest 357 绿(仅本地),迁移逐场景静态推演正确但**运行期须真机验**(⏸GUI:开图/缩放/拖拽/翻页/信息/人脸/Live/exotic/右键/移动跳转/关闭返回保位/前进后退)。
- **P4-c 已交付 ⏸GUI(2026-07-08)**:①沉浸模式=图/视 `viewer.setImmersive` → AppShell 隐侧栏/工具栏/状态栏 + App.vue 隐 WindowChrome + ContentViewer 隐控制条,只留全屏图;浮动退出钮 + Esc 分层(沉浸→退沉浸/否则→关);
- **翻页保持沉浸**(viewerSnapshot 不含 immersive,patch 展开保留)、**离开自动复位**(clear→activeViewer=null→isImmersive 假);阅读器局部沉浸暂并存,统一待 P5(已回写§P4-3)。②ContextualToolbar 切 viewerStore 驱动:`isGrid = !hasActiveViewer && !doc/audio 前缀`——**修 P4-b 引入的「/view 误显网格命令」回归**;doc/audio 暂靠路由前缀兜底(P5 populate 后退纯 viewerStore)。③清理 MediaGrid 指向已删覆盖层的 isDetailOpen 残留守卫。vue-tsc/eslint/vitest 357 绿(仅本地)。
- **⏸GUI**:沉浸切换隐/显 chrome、Esc 分层、翻页保持沉浸、/view 标题栏不再显网格命令。
- **Phase 4 主体全交付(P4-a…P4-c),余「沉浸统一 + 内容页专属命令」并入 Phase 5**

### ▸ 详注 L-P5:Phase 5 各媒体专属命令补全 + 默认呈现集

**待办**:**Phase 5 各媒体专属命令补全 + 默认呈现集**(L3 扩册):`viewer-reader/image/audio/video.ts` 全操作登记(接现有函数与 ReaderApi;**看图台新增旋转**)+ 各 ViewerKind 默认 navigation 集(附录 B 候选,真机调优)+ DocumentViewer/AudioPlayer 移除自带顶部工具栏(操作已迁)+ **P5-6 键位同源**(tooltip 快捷键与监听共用 registry `keybinding` 常量;查看器级散点 keydown 收敛注册表分发,防「显示≠行为」漂移,现状全仓 ~9 处散点)

- 依赖 P4。
- **P5-2 图/视命令 + 顶栏点亮已交付 ⏸GUI(2026-07-09)**:`viewer-image.ts` 登记 7 命令(prev/next/zoomOut/zoomIn/cycleZoomMode/toggleInfo/toggleImmersive,全经 `ctx.activeViewer.api` 调 ViewerApi,when=kind image/video,navigation 组);ContextualToolbar 从「查看器渲染空」改为 **by `command.when` 谓词渲染**(网格命令 when=view'grid'、图/视命令 when=kind,同组共存零冲突;/doc·/audio 未 populate 仍前缀兜底抑制);+3 i18n 键(zoomMode/prev/next 双语);
- **单测 4**(when 过滤 + run 分发 + 缺 api 安全 + isActive)。vue-tsc/eslint/vitest 361(+4)绿。
- **过渡态**:与 ContentViewer 底部控制条**并存**(功能等价双入口),待 P5-5 收敛为单一控制面(反馈后定夺哪些留顶栏/底栏)。
- **P5 旋转已交付(03b8f47,2026-07-09)**:useMediaDetail rotation+rotate+containScale(含旋转适应数学=90/270 足迹宽高对换、钳 1 不放大,**未旋转恒得 1 零漂移**)+characterization+4;ContentViewer 底栏旋转钮(顺手,承用户双控偏好)+viewerApi.rotate+**旋转态隐人脸框**(bbox 投影假设未旋转,诚实边界);viewer.rotate 命令(顶栏共 8 命令);百分比无需改(rotation-aware scale 已携带)。
- **P5-6 键位同源已交付(abb97eb)**:commands/keybinding.ts(formatKeybinding 方向键→箭头·mod→⌘/Ctrl + normalizeEventKey '='归'+'·字母不敏感 + dispatchKeybinding 按 key 查 navigation 组 keybinding 分发,query 按 when 过滤→键位天然上下文感知)+ContentViewer.onKeydown 散点 if(e.key)收敛为一行注册表分发(Esc 留特殊)+ContextualToolbar tooltip 显快捷键(与 keydown 共用 command.keybinding 单源);+5 测。
- **P5-5 已按用户 C 决策修订=局部工具栏保留**(§P5-5 横幅 + 记忆 viewer-keep-local-controls)。vitest 370(仅本地)。
- **余项(按需/待定,顶栏克制下未必做)**:数据命令(收藏/壁纸/复制图像/资源管理器,需 id 操作)/reader·audio·video 命令册(点亮其他查看器顶栏,需 DocumentViewer populate viewerStore)/默认呈现集/溢出接线。
- **⏸GUI**:旋转各朝向适应不溢出+切图复位/键盘行为不变+tooltip 显快捷键/顶栏 8 命令生效。
- **P5 余项—点亮音频/阅读器顶栏已交付 ⏸GUI(2026-07-09)**:①音频(c2d7657)=AudioPlayer populate viewerStore(kind='audio',ViewerApi togglePlay/seekBy/close)+viewer-audio.ts(后退10s/播放暂停/前进10s,when=kind 'audio')+3 测+i18n audio.playPause;②阅读器(9a774bb)=DocumentViewer populate(kind=resolveViewerKind('document',fmt)→pdf/epub/text/markdown 单点推导,ViewerApi toggleToc/Search/Bookmarks/Settings+close)+viewer-reader.ts(TOC/搜索/书签/排版设置 4 命令,when=foliate epub·text·markdown→pdf 不显示,复用 doc.* i18n)+3 测。
- **video 已由 viewer-image 的 image||video 覆盖,无独立册**。
- **均按 P5-5「克制」只上关键命令**(音频音量/阅读器排版·主题·简繁·翻页动画·竖排·编辑等留局部工具栏双控);沉浸仍走各自局部态未并入 viewerStore(统一留后续);/doc·/audio 前缀兜底抑制随 hasActiveViewer 转真自然失效。门禁全绿 typecheck/eslint/vitest 426/build。
- **顶栏点亮观感⏸GUI 真机**。
- **阅读器沉浸并入 viewerStore 已交付(f7f2443,2026-07-09)**:immersive 局部 ref→可写 computed 代理 viewer.isImmersive/setImmersive,进入沉浸 App.vue 隐 WindowChrome+AppShell 隐侧栏/工具栏/状态栏(真·全屏,与图/视 P4-c 统一),Esc+浮钮双出口不变;解决阅读器点亮后「局部沉浸时标题栏仍显阅读器命令」毛边。门禁全绿 426。
- **余 Phase 5**:默认呈现集 + **溢出接线(⚠ 勘查发现:ContextualToolbar 溢出与画廊工具栏布局纠缠——二者共用 window-chrome__content 同一 flex 容器,grid 态把 ctx-toolbar 改 flex:1 会抢 AppToolbar 折叠区 flex 空间回归 G1-G9;需仅 viewer 态条件 flex + G3-G8 式真机迭代,非隔离改动,故暂缓)** / 音频沉浸(音频无沉浸,按需)/ 数据命令(收藏等需 id)

### ▸ 详注 L-PG:Phase G 画廊工具栏并入标题栏

**待办**:**Phase G 画廊工具栏并入标题栏**(折中并入 + L5 溢出,2026-07-09 用户裁决改 §2):画廊工具栏(标题/搜索/筛选/分组排序/行高/布局)搬进 WindowChrome 内容槽、撤 AppShell 第二行 header;可折叠控件接现成 `useToolbarOverflow`,尾部溢出进「⋯」;viewer 态仍走 ContextualToolbar

- **G1 正文回写 + 盘点已做(2026-07-09)**:回写设计文档 §2 表下横幅 / §3.5 / P3-3 / §7 + 本节;定优先级=标题·搜索常驻(计入 reservedWidth),布局→日期→颜色→评分→Live·收藏→分组排序→行高→图视 chips 按显示序从尾折叠。
- **关键勘查**:L5 `useToolbarOverflow` 引擎(ResizeObserver + 宽度缓存 + measure/remeasureKey)**早已整只建好**,此前从未接线,故本 Phase 主要是「搬迁 + 接线」非「造引擎」。
- **G2 结构迁移已交付 ⏸GUI(2026-07-09, e036787)**: AppToolbar 从 AppShell 第二行搬进 WindowChrome 内容槽(showGalleryToolbar 门控仅主视图, 内容页 /view /doc /audio 不显)+ 第二行 header 改 v-if 槽存在(无槽不渲染)+ 内容槽加 gap;富控件零改动搬迁(left / filters flex:1 / right 分布自洽不依赖父容器);顺带消除文档页历史双顶栏;门禁全绿(仅本地)typecheck 0/eslint 0/vitest 370/build。
- **G3 响应式溢出已交付 ⏸GUI(2026-07-09, a17713e→0553772→本次)**: 内容溢出即紧凑, 视图控件(行高/布局/分组/排序)折进「视图 ⋯」body-teleport 弹层;
- **机制偏离**=未用逐项 useToolbarOverflow(异质工具栏无天然「可用宽=容器宽」测量容器), 改内容溢出检测 ResizeObserver, 折叠行为一致机制更稳(§5 Phase G G3 + §3.6 已回写)。
- **溢出量法两次收敛**: ①窗宽阈值→scrollWidth(a17713e/57359a5), ②scrollWidth→getBoundingClientRect(2d0452a, flex+overflow:visible 下 scrollWidth 不报溢出致 ⋯ 仍不出)+ 决策抽纯核 decideCompact(0553772, 7 测)。
- **③折叠承载重构(本次, 修真机崩溃)**: 原「条件 Teleport(:disabled)进另一 body-Teleport 目标 单实例两处渲染」使 Vue runtime patch 踩空锚点, compact 一变即崩(Cannot read null nextSibling/insertBefore)+ 视图控件盖三键;改**抽 GalleryViewControls.vue + 双渲染**(内联 v-if=!compact / 弹层 v-if=compact, store 同步, 无实例搬运无嵌套 Teleport, 复用日期弹层单层 body-Teleport 模式), 崩溃致因结构性消除。避让/高度/主题打磨随真机迭代;门禁四件套全绿(仅本地)typecheck/eslint/vitest 377/build, 崩溃修复⏸GUI 真机终证。
- **④code-review 复审(08860a6, 8 路 finder high-effort)**: 落地 2 条零风险修(F1 溢出复核 watch 补 ui.groupBy/ai.isSemanticMode 依赖——修切 folder/语义模式时内联组内排序 select 溢到三键后不可达;F3 GalleryViewControls 补 onBeforeUnmount 清 rowHeightTimer);
- **余 6 条 defer 真机迭代**(需真机看实际命令集宽/弹层尺寸方能验证,拒盲改): ⓐctx-toolbar 测量耦合(checkOverflow 遍历 .window-chrome__content 全子含 ctx-toolbar,选择变命令集改宽却不触发复核)/ⓑnaturalWidth 滞回在紧凑态非视图内容变宽时失真抖动/ⓒ视图弹层定位魔数 220+无视口越界钳制+resize 不重定位/ⓓ弹层 backdrop(z-index200)盖窗口三键需二次点击/ⓔcheckOverflow layout thrash 无 rAF 节流/ⓕv-show menu 实例常驻可改 v-if 卸载 + 死 CSS .toolbar__sort 清理。
- **⑤G4 筛选 chips 逐项折叠交付 ⏸GUI(10e5ce7)**: 真机反馈「≈1200px 视图控件折后继续收窄, 筛选 chips 被逐渐遮住」(旧兜底=overflow-x:auto 隐藏滚动条裁切)。§5 原设计规划过 chips 逐项折、G3 只折视图控件块未接; 本 G4 补齐。用户裁决=**分组两个 ⋯**(chips 走 useToolbarOverflow 折进「筛选 ⋯」, 视图控件保留「视图 ⋯」整块折; 非「统一单 ⋯」——后者需移视图控件+异质菜单 G3 弃过)。抽 GalleryFilterChips.vue 内联/菜单双渲染 + useToolbarOverflow 首次真正接入(containerRef=.toolbar__filters flex:1 教科书场景)+ 引擎测量精度硬化(computeOverflowSplit 加 gap 参数+composable 读 gap/padding 校正, offsetWidth 不含 gap/clientWidth 含 padding 不校正末项被裁; 纯核 8 测)+ 「筛选 ⋯」body 弹层(backdrop v-if 关卸载 menu 实例承 F5)+ toolbar.filterOptions 双 locale; 门禁四件套全绿 vitest 379; 折叠观感/优先级(从尾: 清除→日期→颜色→评分→收藏→Live→视频→图片)/弹层竖排布局待真机调优。
- **引擎「早已建好从未接线」状态 G4 起终结**。
- **⑥G5 统一单测量重构 ⏸GUI(2648212)**: 真机反馈「缩窄时 chips 先折→继续缩窄视图控件折→右侧释放 flex 空间致已折 chips 弹回(振荡)」。根因=G4 两套独立溢出系统(chips 观 filters flex:1 / 视图控件 decideCompact 观整行)抢同一块 flex 空间。用户裁决=**统一单测量重构**: chips+视图控件并入一个 flex:1 容器 .toolbar__foldable(视图控件移到搜索框左侧), 一套 useToolbarOverflow 逐项折(视图控件 data-toolbar-item 放末→最先折)。单容器→折叠不改容器 flex 宽→可用宽单调→振荡从构造消失。viewControlsFolded=hasOverflow / filterChipsFolded=visibleCount<chipCount / 双 ⋯ 预留 76。
- **删 decideCompact 双系统**(appToolbarOverflow.ts+spec 死代码 −7 测 / compact/checkOverflow/naturalWidth/内容 RO)。vitest 372。
- **⑦G6 折叠动画 ⏸GUI(9579b48; G5 真机验收通过后做)**: v-show 硬切闪烁生硬 → max-width→0+opacity 收拢过渡(opacity 淡出遮 max-width 起点延迟)+ 折叠容器 gap:0 改 margin-inline-end 间距(折叠项 margin:0 随收拢无碎屑, 引擎测量含 margin)+ variant 分叉(内联 collapse class / 菜单 v-show)+ is-measuring 帧 :deep 禁过渡。vitest 372。时长/缓动/⋯ 按钮 pop/菜单内 chip max-width 待真机调优。
- **Phase G 顶栏重构主体 G1-G6 全交付, 余真机细调**。
- **G2 ⏸GUI 宽窗真机待验(单栏无第二行 / 控件不回归 / 内容页不顶画廊栏)**。
- **G2 真机后续两修(2026-07-09)**: ①拖拽回归修复(e9a1446, AppToolbar 结构容器 .toolbar__left/filters/right 补 data-tauri-drag-region——G2 铺满内容槽盖住裸露拖拽面致死区)②品牌 logo 从侧栏移入标题栏左端(32e9cc7, 新增 TitlebarBrand 常驻 + drag 锚点, 删 SidebarHeader;--sidebar-header-h 是手风琴粘性高度非本组件, 保留)。
- **⑧G7 折叠动画改 CSS Grid 收缩 ⏸GUI(989586a; G6 max-width「闪一下」修正)**: 根因=max-width 从上限降到内容实宽那段不可见空转。改 CSS Grid 折叠格(.fold-item grid 1fr→0fr 动画真实宽度全程 + .fold-item__inner overflow:hidden + translateX 横向收入, 0.28s), 测量对象移到 .fold-item 包裹层。
- **期间修 G6 真机崩溃(e349780)**: 给组件透传动态 :class 触发 Vue __vnode 踩空 → 改包裹 div 承载折叠 class 零透传 + 两个 ⋯ v-if→v-show; **纪律沉淀=折叠/动画可测项一律「包裹 div+props」承载不透传组件**。
- **⑨G8 视图控件拆独立折叠项 ⏸GUI(bd20c7b)**: 真机反馈「行高+分组排序整块折, 预期独立依次折」→ GalleryViewControls 拆 3 个独立 fold-item(行高 offset0/布局 offset1/分组排序 offset2, globalIndex=baseIndex+offset), 多根片段化(去单根 .toolbar__view-controls), 从尾折: 分组排序→布局→行高→chips。
- **⑩G9 两弹层展开布局打磨 ✅ 真机通过(多提交)**: 均由父弹层 :deep() 覆盖子组件 .fold-item 实现「菜单侧异形陈列、内联侧零影响」。视图 ⋯(eddb63b)=菜单专属标签(行高/版式/分组, v-if variant=menu 不参与测量)+space-between 两端对齐, +i18n layoutLabel/groupLabel。筛选 ⋯(迭代 4 版收敛至 b942270)=用户精确规格「5 文字 chip 同宽同高/间距恒定/星级·颜色占 chip宽×2+间距×1」逐字映射 3 列等宽 grid(文字 chip 占 1 列 repeat(3,1fr)、星级/颜色/清除 grid-column:span 2 自动并入中间 gap)+星级/颜色纵向 padding 真机定 4px 等高(37a489c)。
- **已知局限**=3 列等宽须固定弹层宽 252px 按中文标签定, 英文更长需调大(§9.7 待办)。
- **迭代教训**=中途为强对齐擅换两列网格反破坏排序+撑高控件属过度设计, 终版回「用户精确规格逐字映射 grid」才收敛(KISS: 先问清几何再落 CSS)。
- **Phase G 顶栏重构主体 G1-G9 全交付, G9 真机验收通过; 余 G2/G3/G5/G7/G8 折叠观感 ⏸GUI 宽窗真机细调**。
- **⑪弹层/溢出健壮性收尾(2ef79dd, 用户裁决「弹层/溢出健壮性收尾」)**: 落地 ④code-review defer 清单里纯代码、不依赖真机的项——ⓒ(positionMenu 把 left 钳进视口防近边缘越界 + 魔数 252/220 提具名常量 + resize 关闭已开弹层 + Esc 关闭 a11y)/ⓓ(三处 backdrop 顶部让出 --titlebar-height, 弹层开启时窗口三键 z-index 2 仍一次可点, 不被 z-index 200 backdrop 拦截)/ⓕ(删迁移遗留死码 .toolbar__sort)。
- **ⓐⓑⓔ 已失效不适用**(依附 G5 删除的 decideCompact/naturalWidth/checkOverflow 双系统)。ⓒ 余「英文标签下按 locale 切弹层宽」等仍 ⏸真机(§9.7)。本任务两文件 eslint 0/vite build ✅; ~~全量 vue-tsc 红系并行文件树会话在飞 WIP(非本文件)~~(2026-07-10 实测已归零,过时注划除)。

### ▸ 详注 L-审:2026-07-10 全线深审 + 修复批

**待办**:**2026-07-10 全线深审 + 修复批**(四路 agent 分域深读 + 关键结论亲核 + 门禁亲跑;详录=设计文档 §10):代码 2 HIGH/4 MED/9 LOW + 文档 8 处回写时效;溢出引擎/折叠数学/token 防御/右键收敛**零正确性 bug**,25 引用哈希全真

**四提交落地(仅本地验证,vue-tsc 0/eslint 0/vitest 全量 492 绿)**:`6478be1` C1 修复批(**allow-set-fullscreen 补权限**——此前 F11 每次吃 ACL 拒绝落 webview 兜底/Esc 守卫补 viewer.isImmersive 让行修「OS 全屏+文档沉浸」Esc 被吞/窗口操作空 catch 改 warn/setTheme 限 mac/goBack 三查看器统一 history.state.back);`03021b6` C2 命令 id 重命名(**用户裁决:开发期不冻结,按建议改**——§4.4 修订为 `<域>[.<子域>].<动作>`,reader/audio 七命令加子域段,泛查看器命令保两段;顺修 api 补 toggleImmersive+死注释×3);`6219aa0` C3 P5-6 补全(**eventToCombo 组合键分发+keybindingAliases 别名,网格 undo/redo/F11 收敛注册表消双源漂移**,AppShell F11 defaultPrevented 防双 toggle,+5 测);`a08636a` C4 沉浸标题栏 hover 滑入(**用户裁决「鼠标移入显示顶栏」**——修 frameless 沉浸期失去唯一拖拽区/三键,贴顶 ≤4px 滑入/mouseleave 滑走/e.buttons 防平移误弹)。**⏸GUI 待真机**:F11 无 ACL 拒绝/组合键与 tooltip 一致/沉浸 hover 手感与阈值/全屏+文档沉浸 Esc 分层。**遗留跟进**(设计文档 §10 池):ViewerKind 双分类器收敛(DocumentViewer 局部 kind vs resolveViewerKind,md 分歧 text/markdown)/roving tabindex/ContextualToolbar 溢出接线/spec 注册表隔离
### ▸ 详注 L-真机:2026-07-12→15 顶栏真机反馈批

| ✅ | **2026-07-12→15 顶栏真机反馈批**(2026-07-15 补登:本批交付时登记在 UIUX 重构线与 [experience.md](experience.md) §14,L 节漏登,现回填) | **✅ 9 提交,两条线用户真机确认(2026-07-15)**。**①形态裁决**:`5776ae4` 合并/分离一键切换(默认合并,详见 Phase G 行)。**②移窗线(推翻 P1 原设计)**:`6eccd08` 弃 `data-tauri-drag-region` 改 `useWindowDrag` 阈值委托(原属性不冒泡→按钮不在拖拽面、无阈值→点击/拖动不可分)+ `361ad30` 阈值解耦常量 5→3 + `523cd0c` 三路径消打滑(空隙即时移窗/按钮 2px+按住 300ms 降阈/卡片手柄零阈值)+ `3cae44e` **双击最大化改手动判定 `isDoubleClick`**(首击 `startDragging` 打断 click 计数 → Tauri 内建 `detail===2` 路径不可达,原「勿自实现」裁决失效)。**③「筛选/分组致折叠闪展再收」四层防线**:`3cae44e` suppressFoldAnim + `0282ff8` 同步命令式量尺 + `5328597` is-settling 过渡抑制(治分组,走 measure)+ `b8e9090` 根治(A `applySplitSettled` 统一不变量 + RO 比对 `window.innerWidth` 分真窗口 resize/兄弟宽变化;B 计数源头解耦——全库计数作 hidden sizer 撑最大宽 + 视图值 inline-grid 叠放,**因视图计数 ≤ 全库计数可证明为上界**)。**④** `bcc47ca` 拖行高致弹层横跳(`overflowRemeasureKey` 去 `gridRowHeight` 空转项)+ `deferWhile` 弹层开启期暂停重测 + 两溢出 ⋯ 互斥。经验沉淀见 [experience.md](experience.md) §14(`93a56dc`)。vitest 902/902(仅本地)。**⏸GUI 余**:RO 判别与 inline-grid sizer 的宽窗观感细调 |


### ▸ 详注 L-F11:2026-07-16 F11 沉浸顶栏唤出根治 + 相邻 IPC/CSP 安全审查批

| ✅ | **2026-07-16 F11 沉浸顶栏唤出根治 + 相邻 IPC/CSP 安全审查批** | **✅ 用户真机验收通过(2026-07-16)+ 后续 3 项安全修复本地验证**。**F11 根因**:tauri-runtime-wry 2.11.4 给 undecorated+resizable 窗口盖的隐形 drag-resize 子窗口在屏幕顶缘拦一条 ≈4px 输入带、全屏不撤,与 `CHROME_REVEAL_EDGE_PX=4` 骑线,是「移到顶有时出有时不出/横移必不出」的全部成因(F9 探针 pointerout(relatedTarget=null) 物证 + tauri-runtime-wry 源码互证,tao 三条早期嫌疑均已源码排除)。修法 `777824d`:F11 全屏区间 `setResizable(false)`(该 API 是该隐形子窗口唯一公开 detach 入口),配套 `AppWindowApi.setResizable` + ACL;`010b1e8` 拆除诊断探针 + 更正两处已证伪的根因叙述。**安全审查 #3/#4/#5**(阅读器 iframe 沙箱 + IPC 命令面复查中发现,#1/#2 已在更早会话段修复未单独登记):`cc43ef1` `open_directory` 补 canonicalize+is_dir 校验(此前零校验直呼 explorer/open/xdg-open,可被诱导转发 URL scheme 给已注册协议处理器);`617cc51` dev 环境补 CSP(核实 Tauri 的 CSP 注入只走 `get_asset()`、devUrl 场景完全绕过,新增 `vite-plugin-dev-csp.mjs` 从 prod CSP 派生仅追加 `ws:`,顺带把 index.html 内联防 FOUC 脚本外置为 `public/theme-snapshot.js` 使其不依赖 Tauri 专属的内联脚本哈希机制);`3b5381f` 记录「摘 epub iframe sandbox 的 allow-scripts」评估否决(VENDOR.md 禁改该 vendor 文件 + 运行时改属性无安全时机窗口 + WebKit bug 218086 自身推荐的 workaround 就是改用 CSP,与既有 `epubScriptGate.ts` 防线一致)。验证:cargo test --workspace / clippy -D warnings / vitest 1019 / vue-tsc / eslint / prettier(改动文件)全绿,仅本地非 CI。**遗留**:28 个本地未推提交~~待批 push~~**已推 origin/dev(2026-08-13 核证;当前 dev 领先 origin/dev 17 commit 属后续工作)** |


### ▸ 详注 L-裁决:2026-07-16 两项遗留裁决落地(分离模式工具栏常驻 + 收藏夹软删 limbo)

| ✅ | **2026-07-16 两项遗留裁决落地:分离模式工具栏常驻 + 收藏夹软删 limbo** | **✅ 两提交,本地全门绿(非 CI),⏸GUI 待 round11**。**①分离模式工具栏常驻(`038656f`)**:上轮只留一句「是否常驻」而提案未落盘,「常驻」有两种成立读法(沉浸态不隐 / 跨路由不消失),带代价列出后用户裁**「两者都要」**=「选了分离 = 这条 bar 永远在」;已知代价当场接受:**分离模式下 F11 不再是「无顶栏」纯沉浸画廊**(想要纯沉浸走默认合并模式)。三处非直译判断:**AppToolbar 有意不随 bar 常驻**(它挂 document 级 keydown 注册表分发,与 AppShell 为「总览/设置页无 AppToolbar」而加的兜底分发〔round10 #7〕同时在场 = 双执行)→ 做成「bar 常驻、内容按路由换」;**查看器路由不给槽**(要最大内容面积且自持局部控制条);**非画廊页渲染 route.meta.title 而非空条**。让位几何:标题栏沉浸态 fixed 铺视口顶 40px,其唤出期工具栏 translateY(40px) 与之拼成 40+48 栈,**恰好等于 sideChromeExtent('top') 的收起边界**(两条都带 data-chrome-top)→ useChromeReveal 一行未改;transform 而非 margin(唤出是高频 hover,不能推 .app-content 重排);z-index/transition 挂 --immersive 而非 --pushed(否则去类瞬间被后置兄弟 .app-content 盖住回程动画)。**②收藏夹软删 limbo(`99dce7d`,round10 查出、非用户报)**:`albums.deleted_at`(V18)只有写路径零读路径 → 5 秒 toast / 会话内 undo 栈是仅有捞回入口,重启即永久不可达而行永久保留。**取数改写了选项本身**:原选项隐含「媒体侧有 purge」,实测**媒体回收站同样永不 purge**(全库无按 deleted_at 老化的清理),真实差距只有**可达性**一条 → 用户裁**只补读路径、不加 purge**(补上即与媒体侧同构;加 purge 反让收藏夹保留策略严于照片)。`list_deleted_collections` 与 `list_collections` 的 deleted_at 判据严格取反(不重不漏划分 + 系统夹读侧同样过滤,单测锁死);有意不建索引(albums 数十行 vs `idx_media_trash` 服务百万行 keyset);排序单测直接钉死两个时间戳(`strftime('%s','now')` 取整秒,同测试内两删撞同一秒会退化 id 倒序兜底、**测不出真实排序键**);UI「已删除(N)」仅在有软删夹时出现。顺带更正 historyStore 头注与 CollectionsView.onDelete 两处现在会说谎的注释。验证:cargo test --workspace(主 lib 527/0/5)/ clippy -D warnings / rustfmt / vue-tsc / eslint(全仓)/ vitest 1019 全绿,仅本地非 CI。**⏸GUI**:F11 让位观感 / 跨路由不跳 / 非画廊页标题条 / 重启后「已删除」区与恢复闭环 |



---

## M. 2026-07-10 全库规范·优化审查(新增待修池)

> 报告 [reviews/2026-07-10-全库规范优化审查报告.md](reviews/2026-07-10-全库规范优化审查报告.md)(8 路分域 agent + orchestrator 亲核;取证基线 cargo test 543 / vitest 589 / clippy·fmt·tsc·eslint 全绿,仅本地验证)。旧账裁决:**C3 exotic dev 旁路正式关闭**(release 物理不可达取证);R16 降级🔵;R21 拆分优先级上调(列清单重复已酿成 P0-A3)。

### ▸ 详注 M-施工批:2026-07-10 施工批 1-4 + 缓办 2 项(用户解禁后补记)

> 📦 2026-08-23 自 todo.md M 节头部 blockquote 全文迁移(零删减);批 1-4 表格行与 ⬜/🔨 活项留 todo。

- **2026-07-10 施工回写(用户解禁后补记)**:同日无人值守施工批 1-4 共 12 提交 + 缓办 2 项(`f5398d1`/`16d7ddb`)全交付,提交链见各行(SHA 均经 git 核实存在)。批 1-3 全绿收官;批 4 性能卫生池点名项(B11/B16/B8/B9)已做,余 C4/C3/其余 C 池约 40 项 + 测试盲区 top 属 Boy Scout 续池。终验 cargo test 547 / vitest 615,**仅本地**。
- 报告侧「§5.5 式」修复状态回写**已补**(2026-07-10):报告文首补「已过时/已交付项」横幅登记(报告曾随目录入 `reviews/2026-07-10-fable5-AI人脸深审/`,2026-07-11 移出至 `reviews/` 根——单文件审查线名实归位)——P0 全清 + 批 1-3 收官 + 批 4 部分(报告结构 §0–§8 无字面 §5.5,该债实为「快照类文档须带时效」的横幅回写)。

---

## N. 缩略图生成流水线深审与优化(2026-07-10 立项)

> (2026-08-14 全量施工归档:todo.md N 节 ✅ 行的交付史整块移位至此,零删减;工作线文档 [2026-07-10-缩略图流水线深审与优化.md](designs/2026-07-10-缩略图流水线深审与优化.md);todo.md 本节保留 ⬜ 活项与指针。)

### ▸ 详注 N-深审主批:深审主批(写边界防降级等)

| ✅ | **深审主批**:写边界防降级(五流水线唯一写汇聚点 `update_thumb_result` 对 status=2 条件写 + items_cache 同守卫,**根治 video/audio/epub 同源竞写残留**=关闭 J 节 P1-4 跟进项)+ WebP 有损(image crate 仅无损系 5~10 倍体积陷阱)+ DocThumbRenderer 三重加固 + URL 构造收敛 + 死代码清理 | `903a98f`+`1f60ce4`;+6 测 |


### ▸ 详注 N-编码质量:编码质量用户可设(thumb_webp_quality)

| ✅ | 编码质量用户可设:`thumb_webp_quality` 1-99 有损/**100=无损(同走 webp crate VP8L)**;变更复位存量+封面派生行;AI/人脸缓存与雪碧图恒 80 解耦 | `a7079f2`;+2 测。⚠ 周边事故:webp/libwebp-sys 漏加 dev opt-level=2 覆盖致 dev 编码慢一个量级拖垮整机(另会话修 `1d91abd`);**教训=新增 C crate 必查 dev profile 覆盖清单** |


### ▸ 详注 N-defer池:defer 池六项清偿

| ✅ | **defer 池六项清偿**(用户钦定顺序 ⑦⑧⑤①⑥②):canvas 迟到 404 sig 毒化守卫 / thumb_size 复位漏封面派生行 / epub 无封面文本卡 / store_doc_thumbnail raw body IPC(JSON 数组每字节 ~4 字符→零膨胀) / **LRU 驱逐事件驱动复位(d503843 根治,启动 stat 扫描降 7 天频控兜底)** / canvas 状态机抽 canvasThumbState 纯模块+9 characterization 测 | `0597a19`/`a270160`/`6dbeacd`/`43911d9`/`6ff2841`/`8e8fc0d` |


### ▸ 详注 N-AB定案:生成引擎 A/B 定案(Part3-T13 改性)

| ✅ | **生成引擎 A/B 定案(Part3-T13 改性)**:USE_PIPELINE 运行时化(AtomicBool+开发者工具 toggle,产物同源仅调度不同)+ 公平性修 3 缺陷(F1 进度 IPC 零节流/F2 `filter_map`+`zip` **结果错位雷**/F3 方案二 Phase2 单线程)→ 真机 A/B → **删方案一** | `daab834` 前置 + **`cfe63d8` 定案**;用户真机实测**流水线 7.3s vs Rayon 直线 16-20s(慢 2.2~2.7 倍)**→ 方案二成唯一引擎、方案一 ~300×2 行+运行时开关全链退役;「方案一多核最好」直觉证伪 |

### ▸ 详注 N-60px源:60px 极密 Canvas 多档缩略图源(120/240/480)

> 📦 2026-08-23 自 todo.md N 节「60px 极密 Canvas 多档缩略图源」✅ 行全文迁移(零删减)。

- **2026-07-17 真机证实**:480px 冷区经缓存、预取、并发与单段解码实验仍无改善。按 `max(item.w,item.h) × DPR` 选择满足需求的最小现有档,缺档回退 480;新生成可评估同解码缓冲顺带派生,存量低优先级回填。实施前 A/B 生成吞吐、磁盘增量与冷滚帧时间。不把全部档位改成 128 的倍数,也不新增固定 60 档。
- **2026-07-17 防闪烁线复验补充**:关闸期前向预取修复落地后,4K+全屏+64px 快速滚动加载过程仍明显(用户裁边际收益不高、记档后期)——供给上限在 IO/解码侧,该残留由本项承接;A/B 可用 `scripts/bench/canvas-wave-bench.mjs`(注意 dev server 吞吐钳制边界,终裁靠真机)。
- **2026-08-16 定案**:服务选档(最小满足档,缺档回退 DB 路径)+ 生成档位×DPR 对齐已上线(`19cad9b`+`a510195`);真机 A/B 同区并发装载 P50 −21%(37.5→29.75ms),热缓存单件两档均 2-3ms(瓶颈在 64 槽排队非输入像素);**全库存量回填不做**(128 档中位 2.5KB/+145MB,渐进收敛);并发扩槽 64→128 A/B 不采纳(区域方差主导、判据未证明);实测详情见 [reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md](reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md) §9。

---

## O. 文件树与时间轴 DOM 膨胀治理(2026-07-09 立项)

> 📦 2026-07-12 从 todo.md O 节六个大格迁移(零删减);normative 权威 = [独立复核报告 v2](designs/2026-07-09-文件树与时间轴DOM膨胀-独立复核报告v2.md) §9 + [极密网格](designs/2026-07-09-画廊极密网格渲染方案.md) / [时间轴密度带](designs/2026-07-10-时间轴密度带canvas升级方案.md)。O 节其余短行(T0/量化/S1/date/展开折叠/loupe/键盘 slider)仍留 todo。

### ▸ 详注 O-canvas:画廊极密网格 canvas/优化专项

**待办**:**「另立项」= 画廊极密网格 canvas/优化专项**(承接上一行 §9.5「削不动除非 canvas=另立项」裁定)

- **2026-07-09 开题(本会话)**:分析文档 [2026-07-09-画廊极密网格渲染方案.md](designs/2026-07-09-画廊极密网格渲染方案.md) 落盘——三路联网调研(开源 Immich/PhotoPrism、闭源 Eagle/Google Photos、canvas/WebGL 技术 + webview 约束)+ T0–T4 方案谱系 + 跨端 canvas 取舍(⚠️ iOS/WKWebView 可能反向劣化)+ 测量协议。
- **对 v2 的细化**:v2「削不动」仅覆盖静态节点数(维度 A);用户报症状(快滚/进选择态卡顿)落在维度 B(每格 Vue 组件响应式重量)/C(进选择态响应式级联),二者未到地板、有**非 canvas 结构性解**(T0 容器单类+伪元素 / T1 轻量卡片)。
- **已交付测量探针** `useGalleryPerfProbe`(dev 门控 `scrollery.debug.perfProbe`,量化 FPS + selectEnterMs,补 v2 节点普查看不见的 B/C)+ `summarizeFrames` 纯核 6 测,MediaGrid 一行接线零热路径改动。门禁全绿(仅本地):vitest 432/vue-tsc 0/eslint 0。
- **🔨 G-T0 代码交付**(`caf9dd2`:compact 关覆盖层 + 两选择态 class 对 compact gate + compact 点击切选;消灭进选择态「建节点+全文档 recalc」主成本,残余廉价 re-render 归 T1;门禁 vitest 432/tsc 0/eslint 0,**⏸GUI 真机验收 + T0 前基线对拍**)。
- **🔨 G-T1+T2 代码交付**(`ea2da48`):`useThumbLoader` 单源(status 1/3/0 解码+自愈,MediaThumb/Compact/canvas 三方共用,清 §11 首号漂移风险)+ `MediaThumbCompact` 60px 轻量卡片(砍 useHoverPreview/徽章/评分/收藏/7 依赖 thumbInfoLines,压维度 B)+ MediaGridRow compact 分支 + **MediaGrid 选择态绑定短路** `compactCells?false:isSelectionMode`(compact 进选择态**整网格零重渲染**,消维度 C 于绑定处,补强 T0 §5.6 残余);T2=方案 A compact 缓冲 8→4/下限 240px + 逐行 will-change 收敛非compact(bucket 用固定段 margin 不经此;cv 已由显式 w/h 满足免加 intrinsic-size)。
- **🔨 G-T4 canvas「转正」代码交付**(`83a4e17`:MVP `1823454`→**混合架构**=canvas 纯绘 pe:none + **轻量 DOM 镜像层**;镜像格 data-item-id+role=button+aria+handlers → **框选/拖到文件夹/键盘/右键/读屏全复用现有链路零重建**;补画 availability 置灰+角标/pending-delete/播放三角/时长;真 LRU+限并发 12+404 自愈(修 MVP「裂图永久缓存」bug)+主题 watch 即时+iOS/线性坐标门控;
- **先审计 MVP 13 处妥协逐条处理,compact 与 DOM 逐项等价 → 同一标准对比成立**,详见方案 §9.7;非compact hover 预览/内联评分收藏/info-overlay 3 项 defer)。
- **🔴 G-T4 v2 修正(mirror-removal)**:用户实测 v1 canvas 反更卡,devtools 实证镜像层 **2658 节点**——每格一 `<div>` 把维度 A 重新引入,与立项目标背道而驰。
- **已推翻镜像层 → 零逐格 DOM**:canvas 单元素算术命中(`hitTestCell`)承点击/右键/pointerdown;框选给 `useSelection` 加 `setPointerIdResolver` 注入纯算术解析器(拖到文件夹的落点仍用侧栏真实 DOM 不受影响);节点回 MVP 级。逐格键盘/读屏 a11y 诚实降级为 DOM 模式专属(DOM 仍 a11y 完备默认);视觉/资源(availability/pending-delete/真LRU/自愈/主题/门控)v2 全保留。详见方案 §9.8。门禁全绿(仅本地)vitest 451/tsc 0/eslint 0/build 490KB。
- **✅ 滚动飞掠卡顿子线收口(2026-07-10,真机验收通过)**:收口文档 [2026-07-10-画廊滚动飞掠卡顿分析.md](archive/2026-07-10-画廊滚动飞掠卡顿分析.md)。T4 后暴露两滚动症状(快滚滑块跳 + 快滚白屏),**双模式复现·与节点数无关 → 根因在共享 bucket 引擎**(按需取数 + 惯性中渲染),白屏=取数载荷往返、滑块跳=段落地爆发抢主线程。A/B/C 决策(用户 2026-07-10 拍板先 B 再 A);交付三提交:**B `0a39e31`** 快滚甩滚低保真加载闸门(模块级 `useThumbLoadGate` 单例信号,`scrollVelocity`/`shouldDeferThumbLoad` 纯函数阈值 3px/ms +10 测,飞掠期抑制新缩略图 decode/请求启动、降速停稳补起,canvas「一屏直出」爆发受益最大;dropped 5-11→**0-1**、白屏基本消失)+ **甲 `da43dfa`** 急停快放行(GATE_RELEASE_MS 64ms 专用短去抖,比 150ms settle 早 ~86ms 出图)+ **乙 `8078d7c`** 载荷瘦身(核实三消费点对 thumbhash 只取平均色 → 后端 `average_color_hex` hydrate 时算好 `placeholder_color` 过桥替代数组,`LayoutRowItem.thumbhash`→`placeholder_color`,消灭 canvas draw 逐格逐帧均色计算 + 砍 ~28 字节数组过桥;复用 thumbhash crate ±2/通道与前端一致,金标 #7a7e80 锁 3 测;
- **保彩色质感非退化固定灰**;justified.rs/layout.ts 与并行会话 epochDay 共编经 hunk 级 git apply --cached 避让)。cargo 455 + vitest 478 + tsc/eslint 0 + build 492.79KB 全绿(仅本地)。
- **残留=按需架构固有边界**(DOM 逐格挂载 / 真图 decode / 方向性预取不做),收口不再攻。
- **🔨 canvas 2D 续挖批(2026-07-10, 外部专家四建议核实后按序施工)**: 核实结论=#2/#3 属实, #1 方向对但「数量级」仅内存维度成立(GPU canvas 纹理上传/mipmap 一次性摊销, 帧时间须 perfProbe 实测), #4 OffscreenCanvas+Worker 漏算函数 prop 不可过桥/rows 结构化克隆/palette 依赖主线程三成本 → 维持需数据立项。交付两提交: **`34c9be6`** alpha:false 不透明合成(合成器 opaque 快路径; 清屏改 --color-bg-canvas 设备像素空间整幅铺底防 dpr 取整漏黑边; 六主题 token 全有) + **`f40251f`** 解码期预缩放(fetch→createImageBitmap 换 new Image(): 经典路径 onload 只是字节到位, 首次 drawImage 才主线程同步解码——canvas 独有痛点, DOM 路径 await decode() 无此问题; 解码/缩放走解码线程池主线程零阻塞 + cover 裁到格纵横比 + 高度分桶只缩不放, 驻留内存 480px 全图→格尺寸约降一个量级, 且 ImageBitmap 不受 discardable 解码缓存回收(回收后重画不再主线程重解码); bitmapBucketH/bitmapPrepParams 纯函数 +9 测; 两级失效=数据签名硬失效/渲染规格 stale 续画(thumbSize 滑杆不闪占位); SVG 直显(status 3 web_safe_formats 含 svg)createImageBitmap 会 reject → 经典 Image() 回退逐字保旧行为; LRU 驱逐/覆盖/卸载显式 close(); CSP connect-src 已含 asset: 零配置变更)。#3 飞掠质量降档因 #1 已做而跳过; G-T3(decoding=async)的 canvas 侧已被本批取代, 余 DOM `<img>` 侧。vitest 508 全绿(基线 499+9 两侧实测对账)/tsc/eslint 0/build 成功(仅本地); ⏸GUI 真机(观感/内存/perfProbe dropped 对比/SVG 回退/滑杆跨桶重备)。
- **✅ 深审 F1-F4 回补批(2026-07-10, 用户批准, 四提交)**: F1 `8d69257` 程序化落点豁免速度采样(bucket onScroll 回传 internalScroll 消费信号+2测; 孤立跳落点即放行消 ~64ms 人为延迟, 相邻 <100ms 连续程序化跳=飞掠关闸; 两种都只重置采样基准; 方案A 直写 scrollTop 无信号仍被采样=已知残留由 64ms 释放兜底) / F3 `bdd83b1` 闸门滞回(关 >3 / 放行 <1.5 px/ms, 带内维持; shouldDeferThumbLoad 加 wasDeferred 记忆项, 测例改写+4; 慢滚恒出图契约不变) / F4 `cee653b` 卸载显式复位闸门+清 gateReleaseTimer/scrollTimeout(取代「靠泄漏定时器保放行」的侥幸正确, 防后人补 clearTimeout 钉死单例) / F2 `ea9c8f6` characterization 12 测(useThumbLoadGate 4 + useThumbLoader defer 状态机 8: 闸门拦启动/放行恰补一次/掠过卡不入队/数据迁移不丢补起/已出图豁免/404 自愈守卫/换代守卫/scope 无泄漏; node 方案=stubGlobal window 后动态 import+FakeImage 行为队列+effectScope, onBeforeUnmount cancelThumb 组件外测不到已标注边界)。vitest 525 全绿(43 文件)/tsc/eslint 0/build 成功(仅本地); F1 落点即时出图体感 ⏸ 真机。
- **⬜ 余(母线独立待决,与滚动线正交)**:真机对比(探针量化 canvas vs DOM 的 FPS+selectEnterMs)定 canvas 是否长期投入(补 defer 项+商店化)+ G-T3(decoding=async,增强)

### ▸ 详注 O-parity:画廊 Canvas 功能对齐 DOM + 滚动加载策略

**待办**:**画廊 Canvas 功能对齐 DOM 线 + 滚动加载策略线(2026-07-10 立项即交付)**

- **Canvas 功能面对齐 DOM ✅ 四阶段交付**:`0cb79b7`(悬停跟踪+rows deep watch 补乐观更新重绘源+选中态圆角/缩放 0.85/surface 遮罩/checkbox+cubicBezier 过冲动画,SelectionAnimTracker 快照纪律=离屏变化不回放)/`35ba0e8`(全格常显:占位 EXT/文本卡/ORIG·THUMB·大小·相似度·LIVE 徽章/信息浮窗 7 类行/常显星心/分隔行图标药丸 sticky/暂存删除整卡灰化;buildThumbInfoLines 抽 mediaGrid.helpers 与 MediaThumb 单源)/`7f6fc25`(**单例悬停卡**:mgc-hover-layer 与 canvas 同 sticky 坐标系,卡内直挂整只 MediaThumb 零复刻——收藏/星级/checkbox/信息浮窗/useHoverPreview 视频预览全同源;computeHoverRect 放大镜 k=max(1.06,min(140/短边,1.5 封顶));非 v1 逐格镜像,O(1) 节点)/`873ee30`(+33 纯函数测)。
- **真机迭代**:`12b2373` 移入闪烁三层修(预解码+占位延迟显形)+放大镜封顶+卡上 pointermove 穿透命中(卡不困指针)/`e507cda` 撤 100ms 意图延迟(不跟手)/`30a236c` 悬停卡黑纱重投影+白描边。
- **滚动加载策略**:`3a82a73`+`68e23e8` 视口外预取(像素余量 1.25×viewH×条目预算 96/48 双约束,大格吃满小格截断)/`29614a5` 闸门阈值随行高缩放(修滚轮慢滚占位墙:60px 保 3/1.5,行高翻倍阈值翻倍封顶 18/9)/`ae02b9d` 滑块拖拽链强制关闸。
- **🔴 未解:拖滑块「一闪一闪」**——`d532140` 回退到今晨基线用户实测**仍闪** → 根因不在本线四项(已 `3a51302` 复装全部);候选嫌疑=缩略图生成期 media_enriched 2s 节流 compute 整页重排/55k 库后台生成风暴;
- **用户将 fork 更早代码分离变量,悬置待其结论**。vitest 593 全绿(仅本地);⏸GUI:选中动画/悬停卡全套/预取零占位/闸门三分语义

### ▸ 详注 O-T1:T1 文件树虚拟化

**待办**:**T1** 文件树虚拟化(方案 B 共享滚动,a/b/c 全交付)

- **T1-a ✅ 交付(2026-07-09 本会话)**:FoldersSection 虚拟化——`.tree` 全高占位 + `.tree-layer`(translateY offsetY)只渲 visibleRows;复用侧栏共享滚动(computeTreeWindow 纯核 + 5 测);去逐行 sticky + 统一 ROW_H=28;M5 修 expandToNode 索引滚动(treeScrollTopForIndex + 3 测,3 调用点接线,保 smooth);折叠 v-show 挂 ResizeObserver 重算窗口。门禁全绿(仅本地):vitest 398/tsc 0/eslint 0。
- **✅ GUI 验收通过(2026-07-09)**:展开全部压测下 `.tree-layer` 恒 ~40 行、translateY 随滚动跟随、不空白(真机修 clientHeight 瞬态巨值 `bd1e7f1`)。
- **⏸GUI**:展开万级目录 DOM 恒定/快滚不空白/选中跳转滚到/reload 保展开态。
- **T1-b ✅ 交付(2026-07-09 `b8e49d7`)**:祖先链堆叠粘性目录头覆盖层(纯函数 `stickyHeaderChain` 回扫求祖先链+7 测,`.tree-layer` 改 absolute 让覆盖层不占位对 T1-a 零回归,relTop<=0 树顶不钉,点击滚到/箭头折叠);钉顶观感⏸GUI。
- **T1-c ✅ 交付(2026-07-09 `132bca7`)**:拖拽边缘自动滚动(指针贴共享滚动区上/下边缘启独立 rAF,速度随贴边加深,把视口外目标滚进 DOM,每帧重算落点,静止贴边持续滚)+ 清理虚拟化后失效的 `scrollToNode`(querySelector+scrollIntoView 恒 no-op)及 expandToNode 内两处调用,职责收敛为「只展开祖先」;拖拽手感⏸GUI。
- **T1 三子项全交付**;仅 ROW_H 单一 CSS 变量同源被运行时实测 rowH 取代(M4 消解,accordion 架构未动)

### ▸ 详注 O-T2:T2 单目录文件分页 + nodes shallowRef

**待办**:**T2** 单目录文件分页 + `nodes` 改 shallowRef(**百万级必做**)

**2026-07-09 交付**(`93e9f13`):useFolderTree 的 `nodes` 深 ref→shallowRef(修 CLAUDE.md 大数组红线);所有原地 mutate(splice/expanded/files 赋值)逐点补 `triggerRef`,collapse 类改「先置属性再 reassign」。分页 `FILE_PAGE=200`:后端 `list_directory_files` 加 `limit/offset`(None 退化全量,旧语义不变),前端每页多取一条探测 `hasMore`(mediaCount 是子树聚合不可用),`flattenFolderRows` 补 `MoreRow`「加载更多」行(+3 测),FoldersSection 渲染 + `loadMoreFiles` 追加(防重复点击,高度 28 保虚拟化)。门禁(仅本地):cargo check 0/vue-tsc 0/eslint 0/vitest 405/build 0。大目录分页手感⏸GUI 待真机

### ▸ 详注 O-dup:缺陷修复:并发 loadChildren 重复目录行

**待办**:**缺陷修复:并发 `loadChildren` 造成重复目录行**(Vue "Duplicate keys found during update: dXXXX" 控制台刷屏,用户 2026-07-09 截图报症)

**2026-07-09 修复**(`6373f90`):**根因**=groupBy=folder 滚动画廊时,FoldersSection 两个 watch(activeDirectoryId / scrolledDirectoryId)对同一未展开祖先并发 `expandToNode → loadChildren`;旧实现在 `await` IPC 完成前才置 `parent.expanded=true` 且无在途去重,两次调用都读到 `!expanded` 各 `splice` 一批相同子节点 → 拍平树出现重复目录行 → Vue 每帧 diff 报 Duplicate keys 刷屏(**T1-c `expandToNode` + T2 `shallowRef` 手动 mutate 模型共同放大竞态窗口**:读→异步取数→写之间无响应式屏障)。**修复**=`useFolderTree.loadChildren` 双防线:①在途去重表 `Map<parentId, Promise>`——并发同 parentId 复用同一在途 Promise 并**等待**(非早退,保证 resolve 后子节点已注入,不破坏 `expandToNode` 顺序展开祖先链前提)②`splice` 前 `existing` id 集合过滤兜底(「一个 id 在拍平树至多一行」硬不变量)。**回归网** `useFolderTree.spec.ts`(3 测:并发只注入一次且 IPC 仅一次 / 加载后再直调不重复 / 单次正常);门禁全绿(仅本地):vitest 451/vue-tsc 0/eslint 0。**⏸GUI**:滚动画廊控制台不再刷 Duplicate keys

### ▸ 详注 O-timeline:时间轴 canvas 密度带升级线 P1–P4

**待办**:**时间轴 canvas 密度带升级线(P1–P4)** — 推翻本表 2026-07-09「canvas 长期搁置」裁决

- **裁决更新(2026-07-10)**:原搁置对「数百离散标记」量级成立(canvas ~3% 舍入误差不抵 a11y/DPR 复杂度);但可视化升级到**日级密度带 + LOD 刻度 + 全轨渐变聚光 + 时间比例坐标**四者叠加后,标记数/视觉复杂度越过 crossover,canvas 成结构性正解(每帧 O(trackH) 与库规模解耦)。方案见 [2026-07-10-时间轴密度带canvas升级方案.md](designs/2026-07-10-时间轴密度带canvas升级方案.md)。
- **已交付(仅本地验证,观感⏸真机)**:原型+DOM/Canvas 一键切换 `a4da35b` → canvas 转正(清 6 处妥协,与 DOM 同标准)`fa7ff96` → **P1** item 坐标 + 三视觉(条/包络/热力/光谱)可切 `3699a97` → **P2** 全轨渐变聚光(source-atop 只染标记)`7d824fb` → 轴宽可调 + 滚动条滑块宽设置回归 + 半透明视窗把手(VSCode minimap 式)`eaa7974`/`35fe3a3`/`cb09bfa` → 滑块/视窗同高进设置页 `fbc1501` → **P3** time 日历坐标全链(后端 `epoch_day` + `buildTimeBand` 空日插值 + LOD 月/年刻度 + 逆映射指示线,坐标切换双门控)`d06a80d` → **深审修复批(2026-07-10 全线对抗性评审后,5+1 提交)**:`0efacdc` R1 time 坐标朝向自适应网格排序——修 🔴 ASC(工具栏常驻升序按钮一键可达)下 rowJumpY 反向单调击穿 logicalYToTimeFrac 二分=指示线全盘倒置,+5 测(ASC 回归/大库常态「天数>行数」同行去重/round-trip/span=0)/`8abebb1` R2 拖拽中浮层冻结(canvas 对 DOM 版 applyJump 的行为回归)+R3 time 坐标键盘浮层 item 空间错位改逆映射/`1734748` R4 最旧年恒有年标锚(单年库曾零年标)+R5 日历 guard 超限(≈108 年,一张损坏 EXIF 离群照即触发)由静默丢最新端改为显式截断+告警,年标 12px 去挤叠,+2 测/`1852f2c` R6 epoch_day 从 compute_*_layout 出发的端到端测点(含负时间戳 div_euclid 日界/folder None;此前仅手搓 LayoutRow round-trip)/`6988e53` R7 行色缓存消非 bars 每帧 ~700 次字符串分配(paletteVersion 驱动主题重算)+R8 徽标接 i18n(zh 单字/en 单字母);评审确认后端 epoch_day 链全对(div_euclid 负日界/camelCase 对齐/无持久化兼容问题)。终验(仅本地):vitest 499 全绿·cargo 全 lib 456 全绿·vue-tsc/eslint 0·vite build 过(主 bundle 494kB 持平)。
- **⬜ P4**:真机 Performance 抓 scroll/飞掠 paint timing 与 DOM 并排实测 → 定视觉/坐标留舍 + DOM 版去留(§12);须用户在环真机
### ▸ 详注 O-T0:T0 时间轴 folder 模式点轨吸附 + hover 分组名浮层

| ✅ | **T0** 时间轴 folder 模式点轨吸附最近 separator + 放开 hover 分组名浮层 | **2026-07-09 交付(本会话)**:纯 DOM 无架构改动;新增 `nearestSeparatorIndex` 纯核(二分吸附)+7 测,folder 模式点轨吸附真实分组边界(替代 `frac*totalHeight`)+hover 显文件夹名(解除 `v-if=hasMonths` 限制)。门禁全绿(仅本地):vitest 28(scrubber helpers)/vue-tsc 0/eslint 0。**⏸GUI**:folder 分组下点轨落点吸附、hover 显分组名。**张数未显**(separators 无 count 字段,需后端补或查表,defer) |


### ▸ 详注 O-量化:MediaGrid 常驻节点量化实测(devtools 实数)

| ✅ | 量化「全屏 + 极小缩略图」MediaGrid 常驻节点(devtools 实数) | **2026-07-09 实测**:3556 cell × 2.1 节点/cell = **网格 7294 节点**(compact culling 到地板,每 cell 仅根 div+img,削不动除非整网格 canvas 化=另立项);文件夹轴 2042(用户截图 `.tl-scrubber` childElementCount 坐实归属);日期轴 200+(日历月天然有界,长期不膨胀)。**结论**:网格是合法地板,文件夹轴 2042 是最大可削废浪费 |


### ▸ 详注 O-60px:60px Canvas 数千格加载驻留与任务风暴收口

| ✅ | **60px Canvas 数千格加载驻留与任务风暴收口(2026-07-17)** | `384e10e`:位图 LRU 由 2000 提至 12000 条，仍受 512MB 字节上限约束；预取按可见格数自适应并改为可取消 idle 分片，可见区与预取共用 64 个全局在途槽。真机终验：已加载区域明显减轻、接近无感，极速无顿挫；480px 全新冷区无改善，后续转 N 线多档源。全量 Vitest 88 文件/1161 测试、typecheck、ESLint、build 全绿。详见 [worklog](worklogs/2026-07-17-Canvas滚动缩略图无感加载优化/) |


### ▸ 详注 O-防闪烁:Canvas 快速拖动防闪烁 + 稍快速换图波根治

| ✅ | **Canvas 快速拖动防闪烁 + 稍快速换图波根治(2026-07-17)** | `ec2bbff` 四层演进:①快滚供给链(bucket 最新目标旁路 + 跨屏旧 fetch 取消);②六主题占位低饱和中灰;③根因实证=闸门关闭期视口外预取整体停取(滚轮连拨峰值越 engage、谷值困于滞回带,闸门钉死)→ 改仅前向 1.25 屏 + 分片预算按新启动数计 + draw 尾同步分片,headless A/B burst 域 coldCellFrames 4911→~530(−89%),开闸域零回归;④格缝双 token `--color-bg-canvas-gap` 修占位糊连/真图失分隔。真机:标准窗口通过;**4K+全屏+64px 极速域加载仍明显,用户裁边际收益不高记档后期——残留由 N 线「多档缩略图源」行承接**。沉淀 CDP 基准脚本 `scripts/bench/canvas-wave-bench.mjs`(评估边界:dev server 连接钳制,真飞掠/极密域结论不可用)。全量 Vitest 88 文件/1184、typecheck、ESLint、contrast、build 全绿(仅本地)。详见 [worklog](worklogs/2026-07-17-Canvas快速拖动防闪烁优化/) |


### ▸ 详注 O-slider:大库拖动 slider 文件树跳同名目录修复

| ✅ | **缺陷修复:大库拖动 slider 时文件树跳到靠后的同名目录** | **✅ 2026-07-14 修复**(`c2536e6`):folder layout 的目录键由 `rel_path` 改为 `(rel_path,directory_id)`，阻止跨扫描根同路径、同名文件按组内排序交错；文件树同步合并为串行 latest-wins，并在展开后、写 `scrollTop` 前校验代次。验证:Rust workspace 全绿；Vitest 71 文件/858 测、typecheck、ESLint、build、contrast 全绿。**⏸GUI**:用户 40 万图库拖动滑块终证 |


### ▸ 详注 O-S1:时间轴 folder/none 模式 DOM 降采样

| ✅ | **S1** 时间轴 folder/none 模式 DOM 降采样(桶聚合到条像素高,2042→~数百) | **2026-07-09 交付(本会话)**:`downsampleSeparators` 纯核(按 y 桶合并取每槽首个真实分隔符,稀疏区留白)+5 测;TimelineScrubber `displaySeparators` computed(SEP_SLOT_COUNT=300)替 `separators` 渲染,分组 <=300 零改动零回归;全量 separator 仍供 T0 吸附全精度。门禁全绿(仅本地):vitest 390/vue-tsc 0/eslint 0。date 轴天然有界不动。**⏸GUI**:folder 分组下轴点数 <=300、疏密可辨、点轨吸附/hover 分组名不回归 |


### ▸ 详注 O-date:时间轴 date 模式逻辑 y 比例定位 + 缩略图遮挡修复

| ✅ | 时间轴 date 模式改逻辑 y 比例定位(与滚动条同步)+ 修缩略图遮挡 | **2026-07-09 交付 + GUI 验收通过**(`f67be18`):月刻度/年份 top=b.y/totalHeight 与 MediaScrollbar 同源,文件多的月占更多轴空间(密度由文件数决定);跳转/hover 改比例;`visibleYearLabelSet` 防稀疏区年份挤叠(+4 测);timeline-sidebar-wrapper z-index 101 + 年份标签背景色描边,修缩略图遮挡。真机迭代产物(非原 §9 计划项)


### ▸ 详注 O-展开:文件树一键展开/折叠全部

| ✅ | 文件树一键 展开全部/折叠全部(压测辅助 + 实用) | **2026-07-09 交付**(`4940a3b`):useFolderTree expandAll(递归懒加载,串行防 splice 竞争)/collapseAll;顶栏 toggle 按钮(ChevronsUpDown/DownUp);i18n zh/en。用于把树展到最大行数验收虚拟化


### ▸ 详注 O-loupe:canvas scrubber 内建放大镜

| ✅ 复活 | ~~loupe 放大镜长期搁置~~ **已随 canvas scrubber 实现**(推翻本表搁置裁决) | canvas 版原型即内建放大镜(方案乙:光标最近 K=9 分隔符富标签列表,canvas `fillText` 绘制,date=每日 label+count / folder=每文件夹;`a4da35b`+`fb5d995`);`pointer-events:none` 磁化预览不抢焦,M2 交接闪断由此规避。time 坐标下放大镜中心按 `rowJumpY` 校正落在正确时段。观感⏸真机 |


### ▸ 详注 O-键盘:键盘 slider a11y

> 🔴 **2026-09-04 更新**:slider 的 ARIA 语义与 valuetext 已移除;方向键/Home/End/PageUp/Down 键盘步进保留。

| ✅ | 键盘 slider(role=slider + ↑↓逐 separator)a11y | **2026-07-09 交付**(`6fd7c1f`):纯函数 `stepScrubberIndex`(方向键/Home/End/PageUp-Down 算下一索引+clamp,+11 测)+ 轨道完整 ARIA slider 语义(role/orientation/valuemin-max-now/valuetext 年-月或分组名/aria-label)+ keyboardIndex 独立真值(避 emit→回传延迟)+ date 步月桶·folder 步全精度分隔符 + :focus-visible 轮廓 + i18n。门禁:vue-tsc 0/eslint 0/vitest 426/build 0;读屏实操⏸GUI |

### ▸ 详注 O-Canvas热路径:2026-07-13 Canvas 热路径优化

> 📦 2026-08-23 自 todo.md O 节头部 blockquote 全文迁移(零删减;真机大库 perfProbe 与 DOM 并排基准待办留 todo)。

- **2026-07-13 Canvas 热路径优化回写**(`a5d8e99`):画廊可见行/指针命中由线性扫描改为两级二分,滚动帧只遍历可见窗口;缩略图裁剪异常路径保证释放中间 `ImageBitmap`。右侧日期/文件夹轴新增离屏静态层缓存,滚动帧只合成静态层并绘制 O(1) active/hover/指示线;视窗拖拽 pointermove 合并至每帧一次。验证:Vitest 69 文件/843 测、typecheck、ESLint、build 全绿 + browser harness 双 Canvas/切分组/滚动/hover 通过。**仍保留**真机大库 perfProbe 与 DOM 并排基准待办,未以 18 项 harness 代替性能定案。

### ▸ 详注 O-性能面板:2026-07-13 全局低扰动性能面板

> 📦 2026-08-23 自 todo.md O 节头部 blockquote 全文迁移(零删减;用户真机终证留 todo)。

- **2026-07-13 全局低扰动性能面板**(`81cef21`):App 内 Ctrl+Shift+P 或设置→开发者工具可全局打开;默认静默录制会隐藏面板,支持高刷新率帧预算、P95/P99/错过 VSync、Long Animation Frame/Event Timing、Canvas draw/Bitmap/Image fallback/cache 指标、历史与 JSON 导出,并提供固定速度三趟画廊往返基准。全量 Vitest 855、typecheck、ESLint、build 全绿;绝对对比以 Release + 关闭 DevTools 为准,应用内浏览器自动验收因本机 ACL 沙箱故障未取得证据,仍需用户真机终证。

---

## 已完成(本会话,存档参考)

| 状态 | 项 | commit |
|---|---|---|
| ✅ | Part6 T2 R10 通用下载引擎 | `af1c676` |
| ✅ | Part6 workspace 骨架(6 member) | `6758119` |
| ✅ | Part6 T9/T10 EntitlementProvider 升格 + plugin-api/free-stub 叶 crate | `5f0953f` |
| ✅ | Part6 `get_plugin_entitlement` gate IPC | `dcff22b` |
| ✅ | Part6 ③a 信任根验签原语下沉 exotic-trust 去环 | `ba8a3de` |
| ✅ | Part6 ③① 组合根注入单点 swap seam | `ebb65b5` |
| ✅ | Part6 ③② pro 骨架(cargo 实证去环,gitignored) | (未提交=红线) |
| ✅ | Part6 ③c Copybara 同步配置草稿 | `6d38173` |

---

## P. AI 分析+人脸分析流水线深审与修复(2026-07-10 立项)

> (2026-08-14 全量施工归档:todo.md AI 人脸节 ✅ 行的交付史整块移位至此,零删减;报告 [reviews/2026-07-10-fable5-AI人脸深审/README.md](reviews/2026-07-10-fable5-AI人脸深审/README.md);todo.md 本节保留 ⬜/⏸ 活项与指针。)

### ▸ 详注 P-批1:批 1(GPU 槽泄漏 A1 等七项)

| ✅ | **批 1**:GPU 槽泄漏 A1/状态命令写副作用 A2/worker 恢复路径 W1·W2·W3/静默算错防线 K1(双防线)/A7 守卫/F1 merge 守卫/前端 U2·U3·U7 | 5 提交 `f68a3ae`/`57bbd34`/`6e23ddb`/`922028e`/`81537ae`;+6 测 |


### ▸ 详注 P-批2:批 2(X1 条件写等)

| ✅ | **批 2**:X1 条件写(P1-4 同款病灶 AI/face 半边关闭,顺删 3 个裸 UPDATE 死面)/F10 完成回调代次化+~~X2 face start sync~~(**2026-07-11 撤销**,见下)/F2 聚类并发防御+F3 孤儿对账(**V16 `faces.is_unassigned` 判别位**)/F6·F7 派生字段对账/F9·A11 失败面(errorItems+`retry_failed_*` 非破坏重试)/G1 误检桶首个写入口/U4 搜索代次 | 7 提交 `5a889f9`/`8c4ac80`(与前笔成对编译)/`3df7d75`/`45d9ba2`/`fea8f78`/`40196d0`/`4c51163`;+11 测 |


### ▸ 详注 P-事故三修:2026-07-11 凌晨环境事故三修

| ✅ | **2026-07-11 凌晨环境事故三修**:①删 src-tauri/.cargo/config.toml 失效 ORT 配置(**真根因**:R2-7 改名把硬编码绝对路径机械替换成死路径 `D:\photoapp\scrollery\...`,cargo 就近优先劫持 tauri dev,worker 装载线程无限阻塞;clean 前靠 target 里 DLL 存货掩蔽);②ai-worker 装 tracing 订阅者+启动指纹+收帧回执(此前 ai-core 日志全被丢弃,卡死零线索);③撤销 X2 start 路径 sync(无脸图无 faces 行,与 AI 侧不对称,每次「继续」全量重扫——外部专家报告指出并核实) | `15943d3`/`8d704b1`/`51dc09f`;伴随环境补件:worker 三 exe+ORT DLL 四件套复制回 target/debug、psd 插件从旧数据目录迁回(复核转 bad_signature 归 B 线);e2e 于 src-tauri CWD 冷启动 2.0s 实证;**2026-07-11 用户真机确认 AI/人脸恢复正常,事故收口**(经验沉淀 → [experience.md](experience.md) §5) |


### ▸ 详注 P-加固批:加固批 A+B 全交付

| ✅ | **加固批 A+B 全交付**(2026-07-11 用户批准施工,四提交 `8533cc5`/`c55e7b2`/`9b0b925`/`9e5e7d9`):**A-1** worker ORT dylib 自解析+快败(=池 d;`resolve_ort_dylib` 拒绝 System32 搜索回退+`preflight_ort_runtime` init_from 急切装载 60s watchdog——死路径「无限静默」变秒级典型错误码);**A-2** 协议 v3(专家建议 2 的协议级推广)=Progress 帧+ProgressBody+三阶段化错误码,worker SessionInit 流式化(装载线程+阶段回执+10s 心跳),宿主 `run_request` 静默限时(SESSION_INIT 300→90s 静默档+3600s 总上界)——**池 b/c 的倒挂病灶整体消解**(600s 降为段级后备);**A-3** psd-worker 核查=零 tracing 事件源,不装订阅者(控依赖膨胀);**A-4** face 完整依赖预检(=池 g 便宜版,CLIP 缺失指名报错不再白占 GPU 槽);**A-5** 路径卫生门禁 `scripts/check-path-hygiene.mjs` 挂 CI(嵌套 cargo config+配置盘符绝对路径拦截);**B-1**(=池 e)producer×2 读连接圈块作用域+status 命令单连接(`*_with` 变体族)+前端轮询 in-flight 合并;**B-2**(=池 f 改良)双 writer 时间驱动 flush「满批或 3s 先到先落」(否决固定 16/64——写事务放大 8-32 倍);**B-3**(=池 a)V17 `face_coverage` 覆盖表+回填双通道+「Done 必有账」事务级不变量+切轨/启动 sync 改覆盖真相+流水线启动自愈(X2 动机正版承接)+销账链(reset/源变) | 验证(**仅本地**):exotic-protocol 25+ai-core 2+worker 20+db 域 125+trust 27 测全绿,clippy -D warnings 0,worker_e2e v3 全链全绿,eslint/vue-tsc 0+vitest 615;GUI 观感面 ⏸ 真机(见下)。余:face 会话**真**解耦(g 全量版)待实际场景需要;P7=A14 仍随批 3 |


### ▸ 详注 P-e2e:e2e 黄金基线重生成

| ✅ | **e2e 黄金基线重生成**(用户拍板重置):`--export-golden` 经 worker 重导 8 图+3 查询+1 脸,复跑全绿(图像/文本余弦全 1.000000、top-1 三查询一致、face 几何/嵌入对拍过、重 init 自洽);协议 v3 改造后复跑仍全绿 | `7fa5ab5`;e2e 回归门恢复可用 |


### ▸ 详注 P-psd:psd bad_signature 根治

| ✅ | **psd bad_signature 根治**(真相=已装插件完好,debug 主程序从未被注入编译期信任根——恒占位集 fail-closed,彩排期旁路掩蔽、cargo clean 暴露,与 ORT 死路径同款延迟引爆):exotic-trust+pro 双 build.rs「debug 且本机有 `.internal-signing/internal-keyset.json` 即自取」,Release/CI/他机不受影响,SEC-02 不动;runbook §2 补行为变更 | `9e5e7d9`;`release-internal-2026-07` 键名于 debug exe 0→2 处实证;运行期插件复核 ⏸ 真机 |


### ▸ 详注 P-报告mv:报告目录收口 git mv

| ✅ | 报告目录已随 M/P 两线收口整体 `git mv` 入 `reviews/2026-07-10-fable5-AI人脸深审/`(保 `--follow`;README 索引已同步、消目录内双主线) | 治理规则「收口即处置」的合并处置点——已执行 |



---

## Q. `db::queries` 模块拆分(2026-07-16 立项,已收官)

> (2026-08-23 整节收官搬迁:状态板 5 行全 ✅ 自 todo.md 整迁,零删减;已批准设计 [2026-07-16-queries模块拆分方案.md](designs/2026-07-16-queries模块拆分方案.md);工作记忆 [planning/2026-07-16-queries模块拆分方案/](planning/2026-07-16-queries模块拆分方案/task_plan.md)。todo.md 本节留 📦 指针 + ⏸ 运行时 smoke 活项。)

> 已批准设计:[2026-07-16-queries模块拆分方案.md](designs/2026-07-16-queries模块拆分方案.md)；工作记忆:[planning/2026-07-16-queries模块拆分方案/](planning/2026-07-16-queries模块拆分方案/task_plan.md)。性质=纯结构重构，保持 `crate::db::queries::<symbol>` 公开路径、SQL 语义、事务边界与测试行为不变。2026-07-16 三轮 Review 与补充裁决通过：D-001～D-012 全部定案。**同日 U 线三次核实**:S 对 queries.rs 的代码写入已尽(S 仅余真机验收),P0 硬门在代码面满足;基线漂移 +343 行由 D-008 机制吸收;S/P3 新增符号归属已回写设计 §4.3;「search.rs 是叶子」前提被 `push_in_predicate` 推翻后,用户采纳 **D-012 方案 a**(P1 暂引 facade helper,P4 迁 layout owner 时顺改路径)。

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | 源码取证与拆分必要性判断 | 立项快照 `33e5846`=10804 行/211 pub fn/39 消费文件；二次 Review 当前=10885 行/485129 bytes/213 pub fn+18 公开类型/常量、125 测、41 消费文件、7 月以来 59 commit/5097 churn；数字只作带 commit 快照，P0 重建 manifest |
| ✅ | 方案 Review（D-001～D-012） | 保留 `queries.rs` facade，14 个 bounded context、一领域一 commit；跨域归属入 §4.3；新增 FS 树 DAO 同归 scan；test list 归一化对拍；S→T 串行施工硬门与 search 过渡路径已回写正文 |
| ✅ | P0 基线冻结 + P1 低耦合模块 | ba86cba 冻结基线(开工 HEAD ae69565,零漂移);P1 五 commit(e95a9a3/363b6c6/5537bd3/5516f2e/39607ce)落地 config/collections/documents/storage/search,search 按 D-012 方案 a 暂引 facade helper;出口门 manifest 对拍 695=695 |
| ✅ | P2/P3 流水线与 AI/人脸 | P2 四 commit(717bfb4/15301fb/cbac384/7e7d3a2)落地 thumbnail/metadata/derivations/exotic;P3 两 commit(2ef51ac/a4d5f9e)落地 ai/faces;每阶段门 clippy -D warnings + 全量 test 绿 |
| ✅ | P4/P5 scan/media/layout 核心 + 收尾 | P4-1 scan(c2b89ad)、P4-2 media(f55e48c);**项目主仓已由 D 盘迁至 C 盘**(fdead51,D 盘 NVMe 不稳定掉盘,迁移后 `git fsck`+四门+前端 vitest 全绿);P4-3 layout(**6ccea07**,最后最高风险块:mapper+canonical SQL+三 builder+view_to_sql/selection+四测试模组整体一次迁移,对 HEAD 原块机械 diff 仅五处登记改点=可见性升级×1/跨域测试定向 import×3/P4-2 遗留孤儿 doc 摘除归位×1;**D-012 方案 a 收尾**与 SELECTION_BATCH_CHUNK 两处过渡引用同 commit 顺改;`'localtime'` 时区债 F-007 登记随迁未修);P5 收尾(**8c7f7fb** facade 终态 37 行=顶层文档+14 mod+14 pub use,零业务 SQL;**三 manifest 终拍零 diff**:API 232=232/consumers 42=42/tests 总量 695=695+queries 末级名 138=138 归一化对拍+末级名重复 0;最终四门全绿主 lib 608/0/5 + docs 两门绿,release 编译烟测 EXIT=0(3m16s,release/scrollery.exe 产出),均本地非 CI);**余 = §8.4 运行时 smoke(⏸GUI 真机:启动/扫描/画廊排序筛选/favorite 往返/缩略图/文档合集/AI 人脸只读/卷与插件列表)** |

---

## S. 文件树显示范围与格式筛选(2026-07-16 立项)

> (2026-08-14 全量施工归档:todo.md S 节 ✅ 行的交付史整块移位至此,零删减;权威设计 [designs/2026-07-16-文件树显示范围与格式筛选.md](designs/2026-07-16-文件树显示范围与格式筛选.md);todo.md 本节保留 🔨 P4/⏸ 活项与指针。)

### ▸ 详注 S-定稿:方案定稿 + 13 项裁决(2026-07-16)

> 📦 2026-08-23 自 todo.md S 节「方案定稿 + 13 项裁决」✅ 行全文迁移(零删减)。

- **方案定稿 + 13 项裁决**:三轮复核(含独立会话复核);D-002 由「逐字节对拍」修正为 common-subsequence 稳定性(所有文件模式必然插入 FS-only 项,原不变量不可实现);D-013 新增(树节点路径身份 ≠ DB 实体身份)。

### ▸ 详注 S-P0:P0 可扩展注册表与契约测试

| ✅ | **P0 可扩展注册表与契约测试** | **三提交交付**:`b3321fd`(P0-a 内置表 66 项收敛为 `BUILTIN_FORMATS` 单一事实源,三平行 match 全派生,热路径 `OnceLock<HashMap>` O(1);17 测试)+ `3fc96cf`(doc_subtype 接回主干,**修 file_format 被当子类型写库**)+ `7b4c868`(P0-b 新建 `formats/` 合并层 + `FormatDescriptor` UI 投影 + 删前端死镜像;6 测试 + 双 offering fixture)。验证:`cargo test --lib` **546/0/5** + rustfmt + clippy `-D warnings` + vue-tsc 绿(本地非 CI);**golden 对拍经变异验证有效**(删项/值错两类都红)。⚠ 两条相邻发现见 F-012/F-013 |


### ▸ 详注 S-P1a:P1-a 文件树三态(后端)

| ✅ | **P1-a 文件树三态(后端)** | **六提交交付**:`cbc7527` `resolve_within_root` 路径安全地基(三层防线,10 测试全走真 FS)+ `a686dae` FS 数据源(隐藏判定/DB 等价比较器/惰性枚举,11 测试)+ `f5ea0e6` `reveal_tree_entry`(**opener 只加 Cargo 依赖调自由函数、插件完全不注册 → ACL 面为零**,比设计原定更紧;3 测试)+ `79f911a` 目录快照缓存(双维度封顶,12 测试)+ `f3498dd` `list_tree_entries`(TreeEntry 双身份分离 + D-002 端到端对拍)+ `c111e1a` 失效接线。验证:`cargo test --lib` **583/0/5** + rustfmt + clippy `-D warnings` 绿(本地非 CI)。**四处变异验证**抓出 4 个假测试(见 F-016) |


### ▸ 详注 S-F015:F-015 reveal 统一到 opener

| ✅ | **F-015 reveal 统一到 opener** | `6e8d1e7`:两套实现合一(`ipc/reveal.rs`),修 Linux 只 `xdg-open` 父目录**不选中**(改走 D-Bus `FileManager1.ShowItems`,失败回退 XDG portal)。**统一顺带引入新失败面必须同批处理**:旧 `.spawn()` 发射后不管、几乎不失败;opener 校验路径存在且同步等结果 → 「文件被外部删除」成常见真失败,而两处调用点原本一个静默吞、一个裸 `await` → 补 error toast + 中英文案(见 F-015) |


### ▸ 详注 S-P1b:P1-b 文件树三态(前端)

| ✅ | **P1-b 文件树三态(前端)** | 用户裁 **F-017 = 方案 A(就地放宽)**。**三提交**:`9166d3d` 路径身份收敛为唯一实现(`crate::tree::{node_key,parent_key,child_rel_path}`,DB 三命令 + FS 命令全从这里取键,**前端不推导** —— 否决「前端自己拼 `${rootId}:${relPath}`」:那是跨语言第二实现,分歧不报错只是同一目录被当两个节点)+ `b1b2227` 结构轴换 `nodeKey`(DB 模式零行为变化;实体轴〔拖拽/移动/复制/路由/滚动锚点〕原样不动——§4.1 本就不对 FS-only 开放)+ `e94ad70` 放宽 `id` 为可选(`vue-tsc` 精确列出 **12 处**)+ 能力边界成**类型谓词** `hasEntityIdentity`/`isOpenableInApp`(非 boolean——那会逼调用方 `id!` 断言把保护扔掉;谓词收窄使「先判后用」成编译期强制)。验证:`cargo test --lib` **589/0/5** + `vitest` **1033/0** + 六门净(本地非 CI);**14 项变异验证**(含哨兵自检)抓出 MF4 假测试。**补了既有零覆盖缺口**(`collapseNode`/`collapseAll`/`getDescendantKeys`/`toggleNode`)。**P1-b 已收官**(2026-07-16,再五提交):`89ce704` 树模型能表达「未知」(`mediaCount`/`hasChildren` 加 `| null` —— 0/false 是断言「没有」而事实是「不知道」;0 会渲出「0」角标读作「这里没东西」,`hasChildren:false` 会让真有内容的目录永远点不开)+ 可展开判据收敛 `isExpandable()`(原散 4 处,FS 模式要改的恰是它,散着改必漏且无编译/测试信号)· `31d1bc8` FS 取数适配层(IPC 加 `kind` 过滤使两模式结构平行、前端只一套模型;过滤在**取快照之后**且缓存键不含 kind → 「先拉目录再拉文件」复用同一次磁盘枚举)· `65b64fd` 取数按模式分发(三 loader 签名 id→节点:FS 取数只需路径身份,收 id 等于把「必须有库行」写死进签名;模式**参数化**而非内部读全局 → 选择器对话框「只能选 DB 目录」的约束成显式,默认 `registeredOnly` = 不传即明确要 DB)· `549861c` 三态菜单 + 本机持久化(uiStore 启动批 20 键——该值须在首次展开前就位,configStore 惰性 N 次往返来不及;模式 watch 兼作水合兜底,不赌顺序)+ 双击 reveal。**换签名顺带暴露三处**:①`loadChildren(parentId, rootId?, loadId?)` 后两参从无调用方传过 → 整段死代码;②在途去重表键仍是 DB id(D-013 第三处漏网,FS-only 目录 id 全 null → 全撞同一键 → 并发展开互相顶掉且不报错);③reveal 的 relPath 我自己拼了一遍 = 重造 Rust `child_rel_path`,而**两条产出侧本来都现成有它**(DB 侧算完塞进 node_key 就扔) → `DirFile` 补 `rel_path` 照抄下发。验证:`cargo test --lib` **594/0/5** + `vitest` **1061/0**(84 文件)+ 六门净(本地非 CI);**9 变异 + 哨兵自检**全按预期(含一条**预测并标注为不可证伪**的:分页游标改 `files.length` 反推 0 捕获 —— 后端 `slice_page` 恒有 `next_cursor = cursor + entries.len()`,二者今天逐值相等,构造不出合法分歧输入;留 `filesCursor` 的理由是解耦「翻页游标」与「适配后剩几条」,不是它今天不同)。**余**:⏸GUI 手感验收(菜单观感 / 切模式重载 / 双击 reveal / FS 模式大目录翻页) |


### ▸ 详注 S-P2:P2 一级媒体类型

| ✅ | **P2 一级媒体类型** | `dfcd0d3`:常显顺序 图片|视频|文档|音频|LIVE|收藏,chip 数 7/8 → 9/10(P3 的「格式…」再 +1 = 设计所载 10/11)。**三处描述符化**新建 `filterChips.descriptors.ts` 作唯一事实源，模板/`chipCount`/`overflowRemeasureKey` 全部推导。**有意不做全数据驱动模板**：各 chip 内容结构差异大(按钮/内联 StarRating/ColorLabelPicker/开弹层的日期 chip)，硬塞 v-for 是为 DRY 而 DRY；真正会错的不是 chip 长什么样，而是它**排第几、在不在、宽度受谁影响** —— 只收敛这三件事。**施工中发现并修的不对称**：URL 白名单仍是 `['image','video']` —— encode 写全(宽)/decode 按白名单过滤(窄) → 点「文档」再刷新**筛选静默消失**(URL 里明明写着 types=document)，且无任何报错。remeasureKey **逐值等价论证**：原键的 `hasActiveFilters` 描述的不是清除 chip 有多宽而是它**在不在**——故新键把「存在性」单列；键只用于 watch 触发重测、非缓存键，故**变化点等价即等价**，值不必相同。顺手归一 i18n(零成本项)：Live 原为裸字面量、视频原借 `sidebar.videos` → 统一 `toolbar.*`。12 条新用例；**变异验证 7 个 + 哨兵自检全部捕获**(含「日期 chip 漏声明 widthDeps」= P3 加格式 chip 时最可能重演的形态)。vue-tsc/ESLint 净；vitest **1075/0**(85 文件)。本地非 CI。**余** ⏸GUI：chip 折叠观感与两新 chip 的真机切分位次 |


### ▸ 详注 S-P3:P3 细分格式全链

| ✅ | **P3 细分格式全链** | **四提交**:`2afe1e1` 后端全链(MediaFilter/GalleryFilter 加 `file_formats` + `to_media_filter()` 下沉带过 + SQL **两个构造点**〔画廊 `push_where_predicates` / 搜索 `search_media`〕+ facet/registry 两 IPC)· `8161b12` 前端全链(filterStore/DTO/URL codec/echo-guard)· `d7200f1` 弹层纯逻辑(别名合组/大类兼容/跨维度剪枝)· `a334127` 弹层 UI + 格式 chip · `73c44bb` §8.2 非回归。**抽 `push_in_predicate` 而非抄第四遍**:那段**参数序号算术错了不报错**——占位符编号与 extras 推入顺序错位既不 panic 也不报 SQL 错,SQLite 按序号取到隔壁谓词的值,「筛 PNG」静默变成结果恒空;空列表不得退化成 `IN ()`(语法错,且语义是「筛出零条」与「不限」正相反)。**facet 契约**:`DISTINCT file_format` + 基础谓词,实测 **0.0ms** 走覆盖索引 → **不缓存不做失效**;不带 media_type(114ms 全扫)、不带 counts(216ms,唯一用途是预判结果数、点下去就知道);分组交前端拿 registry 配——大类归属是 registry 的知识不是 DB 的知识。**fixture 有意不照搬生产库**:设计已注明生产库 raw distinct == base distinct == 29 是**数据巧合**(被基础谓词滤掉的仅 4 行软删 jpg,而 jpg 在 base 中也在)→ 照搬则对「漏掉基础谓词」零区分度,故造出巧合不成立的形状。**画廊重算 watch 由手工枚举 7 字段收敛为单键 `apiFilterKey`**:`compute()` 喂后端的筛选部分**恰恰就是** `toApiFilter()` 的输出,两处抄一份;colorLabel 已这样漏过一次(T16 遗留)。含**一处有意行为变化**(只填日期一端不再触发那次可证明的空转,已在代码+用例双处写明)。**URL 两侧同尺**(设计 §8 点名):encode 原「store 有什么写什么」/decode 按白名单收 = 写宽读窄;格式形态 `[a-z0-9]{1,16}` 与后端 `is_valid_format` 同形,数量上界 64(每个扩展名 = 一个 SQL 绑定参数,不封顶即让 URL 决定 `IN (...)` 多长);有意不校验「是否已注册」(registry 是运行时并集、URL codec 是纯函数拿不到 Catalog;未注册扩展名只是筛出零条=诚实空结果)。**类型系统列出 4 处必改点却漏了第 5 处**:`galleryFilterSnapshotEqual`(echo-guard)只**读**字段不构造快照 → 加字段不是类型错;漏了则两快照被判相等 → 跳过 URL→store 回填 → 深链格式筛选**永不生效且无报错**。已补比对+专门用例+就地注释。**§8.2 非回归两条**(设计专为防抄错而写):filter_key 随 file_formats 自动改键(钉住前提——日后加 `#[serde(skip)]` 会让缓存跨筛选误复用且不报错)、filter-invariance 在**格式维度**成立(既有那条用 favorited 子集,两者证同一性质在不同维度上的成立,缺一不可)。第三条(patch 路径不纳入格式)无需改码:那三个字段有 patch 语义是因为运行时会变,file_format 不变。**token 闭环门抓到幽灵 token**:badge 底色写了六套主题**均无**的 `--color-bg-tertiary`,运行时因 fallback 永远生效而看不出问题(experience §19 正向兑现)。**修一处自己写进源码的 NUL 字节**:Map 键的空格成了 `\0`,**穿过全部质量门**(能编译/tsc/ESLint/23 测试全绿),唯一信号是 git 把文件当二进制;靠变异脚本报「锚点未命中」才发现。变异验证累计 **31 个 + 4 次哨兵自检**(后端 8 / 前端 8 / 纯逻辑 11 / chip 4 / §8.2 2),其中 3 个因锚点问题**改手工施加**,不留空断言。**P4 八门全绿(本地非 CI)**:cargo test --workspace(主 lib **607/0/5**)+ rustfmt + clippy --workspace -D warnings + vue-tsc + ESLint 全库 + vitest **1123/0**(87 文件)+ vite build + 两道 docs 门。**余** ⏸GUI 真机验收 |


### ▸ 详注 S-审查:施工审查 + 真机三问题定性

| ✅ | **施工审查 + 真机三问题定性(2026-07-16)** | 全量审查快照 → [reviews/2026-07-16-S线文件树与格式筛选施工审查.md](reviews/2026-07-16-S线文件树与格式筛选施工审查.md):**2 🔴**(R-01 FS-only 行五处 `null === null` 判等恒真 → 隐藏目录常挂选中/拖拽样式,**= 真机问题①根因**;R-02 `useGalleryQuerySync` watch 漏 `fileFormats` → 只选格式后刷新筛选静默丢失)+ **5 🟠**(R-03 FS 模式根行沿用 DB `hasChildren`/`mediaCount` → `hasChildren=false` 的根在 FS 模式整层 FS-only 子目录丢失且不可展开;R-04 loadChildren 无代际守卫,切模式在途响应可注入新树;R-05 应用内 8 个物理写点不失效树快照;R-06 adapter 丢 `hidden` 字段,隐藏项零视觉区分;R-07 FS 模式禁拖误伤库内目录)+ 14 🟡。**真机三问题定性**:①=真 bug(R-01+R-06);②隐藏目录下 .md 双击跳文件管理器 = **D-001/§4.4 有意行为非失效**,改善分 A(affordance,已做)/B(path-based 应用内只读打开,**单独立项待裁**)/C(伪 id 入库,否决);③拖拽 = 文件行**从未实现**(0c54d03 起仅目录行可拖,媒体移动走画廊卡片拖树)+ 默认模式目录拖拽链核验完好 + FS 模式被 R-07 误伤(已按修法 a 恢复);F-020~F-023 已登记三件套 findings |


### ▸ 详注 S-修复批:修复批 1~3 施工

| ✅ | **修复批 1~3 施工(2026-07-16)** | **三提交 + 一破窗**:`2188bff`(批1=真机问题①③根治:R-01 判等收敛 `sameEntityId` helper+spec 钉 null-null;R-02 watch 补源 + **新建 wiring 级 spec**〔`Record<keyof GalleryFilterSnapshot>` 类型完备门 + 逐字段变更→URL 必变,变异验证撤修复 2 红〕;R-06 hidden 透传+文字/图标 opacity 0.55 淡化〔不动背景——背景通道已被选中/拖拽占用〕;问题②方案A=文件行 `.selected` 可见选中态〔.kb-active 被 `:focus-visible` 门在鼠标路径恒不显示=「单击没反应」根因〕+未入库 tooltip 明示+移动端不发起;R-07a 后端 `TreeEntry.parentDirectoryId`=被列目录库行 id,**零新增查询**〔find_directory_id 结果复用〕,FS-only 仍被 `hasEntityIdentity` 拦)· `973709c`(批2=数据正确性:R-03 FS 模式根行 DB 轴字段按模式归一 null〔F-022〕;R-04 代际守卫回归〔F-023,变异验证有效〕+ loadRoots 换代清在途表 + finally 只删自己注册项;R-05 失效接线=`InvalidateOnWrite` Drop 守卫扩展〔含中途失败路径〕+5 无守卫命令显式清,**尝试即清不等成功**〔部分落盘也失真〕,统一全清〔≤8 快照封顶〕;R-11/R-12 顺并,设计 §4.2 正文回填触发点清单)· `13b727d`(批3=🟡:R-08 `AppError::Reveal{code,message}` 稳定码落 IPC code 字段,测试改钉序列化后 code;R-09 三消费点按码分流+移动端动作不出;R-10 展开失败 collapseNode 回退+onLoadError→toast;R-14 rustdoc 归位;R-17 格式 chip 挪至收藏后〔§5 字面序,spec 钉前七项〕;R-18 裁「切模式折回根」为有意行为,tree/mod.rs 注释+task_plan §二对齐;R-19 harness 三 fixture)· `d293b53`(破窗:T 线遗留 derivations.rs 测试模块后置项,clippy `-D warnings` 红,纯移动)。**记录不动池**:R-13(TOCTOU 残余,威胁模型外)/R-15(后端 formats 兜底,可选)/R-16(Catalog 刷新失效钩子,接线时再做)/R-20(R-02 已修,深链剪枝时序待真机再评)/R-21(测试可选加固)。**单独立项已于 2026-07-17 开工交付**(见下行)。验证:vitest 全量 **1146/1146**(新增 23)+ vue-tsc/ESLint 净 + cargo test --lib **608/0/5** + clippy `-D warnings` 净 + 两道 docs 门绿(本地非 CI);**四处变异验证**(R-02×2/R-03×2/R-04×1)全部抓红后恢复绿 |


### ▸ 详注 S-树内文件行:树内文件行打开与拖拽

| ✅ | **树内文件行打开与拖拽(2026-07-17 立项施工)** | 两个单独立项一次交付(工作记忆 [worklogs/2026-07-17-树内文件行打开与拖拽/](worklogs/2026-07-17-树内文件行打开与拖拽/task_plan.md),设计件同目录)。**①问题②方案B v1 = path-based 应用内只读文本预览**:后端 `get_tree_text_preview`(`resolve_within_root` 同 reveal 姿态 + 白名单 txt/md/markdown **取 canonicalize 后真实扩展名** + 1 MiB 截断 + lossy 转码 + `AppError::Preview{code}` 稳定码、message 不带绝对路径;6 单测含目录伪装/二进制伪装/截断)+ 前端 `FilePreviewDialog`(UiDialog 原语,`<pre>` 纯文本渲染**不解析 markdown**,代际守卫丢迟到响应,桌面端 reveal 逃生口)+ 双击分流(白名单→预览〔移动端同样可用——预览不依赖 opener〕,其余照旧 reveal)+ Enter 键盘可达 + tooltip 三分支 + harness fixture。**不并行改造查看器 mediaId 取数链**(弹层收窄,工程量与攻击面双降;与 D-001 不冲突——禁的是交系统默认应用执行,非应用内只读查看);S 线设计件 §4.4 有意行为段同步收窄标注。**②文件行拖拽 v1**:拖到树内目录=移动、Ctrl/⌘=复制,复用画廊「拖到文件夹」链(`history.moveMedia/copyMedia`→`relocate_media_items`,同 item id 保留→缩略图/AI 嵌入不失效 + undo/redo 现成 + 树快照失效已被 InvalidateOnWrite 覆盖)**零后端改动**;仅实体身份文件可拖(FS-only 不起手),`dragFileId` 与 `dragId` 分离(目录 id 与媒体 id 两个空间不可混),落点 `canDropFileOnDir`(文件无子树无环检测,拒落回原目录,父 id 未知放行靠后端 no-op 兜底),边缘自动滚动改回调注入(行为不变),`suppressClick` 补进 `onFileClick`(拖完可打开文件不误路由进查看器)。验证:cargo test --lib **615/0/5** + clippy `-D warnings` + rustfmt + vitest **1155/1155**(88 文件,新增 9)+ vue-tsc + ESLint 净(本地非 CI)。**余 ⏸GUI**:预览弹层观感/截断提示/二进制伪装呈现、隐藏目录 .md 双击预览、文件行拖拽手感(ghost/边缘自动滚动/undo 还原)、Ctrl 复制、FS-only 文件不可拖、移动端 .md 预览可用 |



---

## R. 滚动增补归档(2026-07-17—2026-08-11 交付波)

> (2026-08-14 全量施工归档:按「完成即归档」,todo.md 头部增补中已完成并核证的交付波叙事整块移位至此,零删减;todo.md 各留一行 📦 指针,活项(⏸真机/待裁/挂账)随指针保留。)
> (2026-08-23 二轮瘦身:同契约追加 R-图片编辑方案 / R-深审07-25 / R-深审07-31 / R-UI链路08-03 四条——原 todo.md 头部未标 📦 的审查/方案叙事。)

### ▸ 详注 R-档位行高:2026-07-17 缩略图档位重定 + 画廊行高滑杆改造

> **2026-07-17 增补(缩略图档位重定 + 画廊行高滑杆改造;~~本地改毕待提交~~**已提交 `b554aa5`,2026-08-13 核证)**:① **缩略图档位** `[120,240,480,960]` → **`[64,128,256,512,1024]`**(5 档)——前端 `THUMB_SIZE_TIERS` + i18n(新增 XS 标签)。② **画廊行高滑杆** 60–960/step20 → **64–1024/step16**,新增 5 档节点(对齐缩略图档位)拖拽**靠近自动吸附**(阈值 24px,`GalleryViewControls.vue`)。③ **默认档位** 480 → **512**(仍是有效档、短边 ≥288 ≥224,保「缩略图喂 CLIP」优化;前后端默认一并对齐)。④ **panic 根治**:全量/批量生成两条 IPC 路径直调 `decode/encode_media_step`、绕过 `generate_thumbnail` 的 `snap_to_tier`,旧库存量 `thumb_size`(如 480)非新档位 → `thumb_path` 断言 `Thumbnail size 64 is not a valid tier` panic 刷屏;修=两入口加 `snap_to_tier` 兜底 + **档位表立 `generator::THUMB_TIERS` 为唯一事实源**(原散落 generator/cache×3/file_ops/scan 共 5 处硬编码,是漂移出 bug 的土壤,现全引用)。⑤ `exif_thumb` 大/小档位分级阈值随档位重定(480→512、下限 120→128)。验证:`cargo test --lib thumbnail::` 32 过 0 败 + `cargo clippy --lib` 触及面零告警 + 前端 vue-tsc/ESLint/vitest 全绿。**须知**:换档位使旧档缓存作废需重产(既定高代价);老库 DB 存量 480 靠 IPC snap 走 512、生成不再 panic,但设置页 segmented 到用户重选前无高亮按钮(无害小疵,未强迁 DB)。真机验收:重启后全量生成跑完无 panic + 行高吸附手感。


### ▸ 详注 R-画廊重排:2026-07-17 画廊重排滚动位置恢复

> **2026-07-17 增补(画廊重排滚动位置恢复)**:改窗口/侧栏宽度、切分组/排序/组内排序/无缝分组/布局模式等整体重排后,画廊滚回原浏览位置(钉住重排前视口顶部项)。做法=现有「行高锚点」机制(capture pre-flush → `get_item_y_by_id` O(1) → `scrollToLogicalY`)触发面泛化,拾取逻辑抽纯函数 `pickReflowAnchor` 入 mediaGrid.helpers 单测;顺带修两处潜伏缺陷:① 锚点 400ms 壁钟清除 vs 大库 `compute_layout` 秒级 IPC 竞态(恢复前锚点被清静默退化,50万库恰是重灾区;修=定时器遇 `isComputingLayout` 续命)、② 持锚期间切视图旧锚污染新视图滚动位(修=锚点携带 viewKey,不匹配即弃)。顶部特判:vTop≤0 不押锚(排序翻转钉首项会把顶部浏览者甩到另一端)。filter/search/视图切换**有意不入**触发面(项可能消失,维持 scrollCache 兜底)。验证:vitest 88 文件/1188 全绿(含新增 4 测)+ vue-tsc + ESLint 触及面零告警。三件套:[worklogs/2026-07-17-画廊重排滚动位置恢复/](worklogs/2026-07-17-画廊重排滚动位置恢复/task_plan.md)。**⏸真机验收**:大库下切排序/分组/拖窗宽,视口顶部项应钉在原位;列表顶部切排序应留在顶部。


### ▸ 详注 R-审查9项:2026-07-18 审查 9 项修复

> **2026-07-18 增补(审查 9 项修复)**:外部 AI 审查报告([reviews/2026-07-18-昨日与今日代码修改审查.md](reviews/2026-07-18-昨日与今日代码修改审查.md))F-01..F-09 逐条对码核实**全部属实**,分四批修复全部落地(`efec238`/`0d1234a`/`305d4b6`/`8b0220f`):① 派生/缩略图 token 升 RunTokenSlot run-generation 纪律(停止→立即重启不再互杀)+ reset 失败传播;② relink 单事务落库 + asset scope 授权后置(add_scan_root 一并对齐)+ 前端重链接/重扫双错误边界;③ TXT 超长单行强制 scrolled 护栏(阈值/谓词 syntheticBook.ts 单源,后端行内切分因非 UTF-8 偏移风险裁不做);④ MF 流选择失败回滚 + 旋转写 latest-write-wins 串行化(新原语 utils/latestWrite.ts)。验证:cargo --workspace 731 过 + vitest 1234 过(新增 15 测)+ clippy/vue-tsc/ESLint 零告警。遗留:F-03 故障注入/F-07 真源回退矩阵未自动化(~~ai/face token 槽统一~~已于批5 `0bf5ebb` 落地,2026-07-19:字段升 RunTokenSlot+删共享代次计数器,终态门控保持 `!is_cancelled()` 不变,731 测持平)。三件套:[worklogs/2026-07-18-审查9项修复分批施工/](worklogs/2026-07-18-审查9项修复分批施工/task_plan.md)。


### ▸ 详注 R-导出成果:2026-07-19 方案 A 导出整理成果

> **2026-07-19 增补(方案 A 导出整理成果,阶段 1-3 落地)**:上线前三功能(导出/备份/图片简单编辑,B→A→C 顺序,B 已交付)之 A 施工完成核心链路。后端:`export::{core,naming,manifest}` 纯函数引擎(staging 内逐项 `.tmp`→rename、命名档 original/sequence/date、文件名合规化、manifest 原子写、库内目标判定用 `Path::starts_with` 组件比较而非字符串前缀)+ `ipc::export_commands`(preflight/start/status/stop,复用 B 线已落地的 `file_job_owner`/`RunTokenSlot` 门闩与 app 事件+快照姿态,新增 `filetime` 依赖显式回写 mtime)。前端:`exportStore`(事件订阅+快照恢复,镜像缩略图生成进度姿态)+ `ExportDialog.vue`(目的目录/命名档/冲突策略/manifest 开关/预检摘要/库内警告)+ `BackgroundFileJobIndicator.vue`(状态栏右区,终态弹 toast 含「打开导出目录」)+ 入口接线(选区工具条 + 右键菜单,统一 `selectionDescriptor()`)。施工前核实方案与当前 HEAD 的漂移并修正两处(错误码改用域共享 `file_job_busy`、模块拆分对齐 `backup/core.rs` 惯例)。验证:`cargo test --lib` 706→729 全绿 + clippy `-D warnings` 本次改动零告警;`vue-tsc`/`ESLint`/`vitest` 1234→1239 全绿(本地非 CI)。三件套:[worklogs/2026-07-19-导出整理成果施工/](worklogs/2026-07-19-导出整理成果施工/task_plan.md)。**范围裁剪**(未做,非遗漏):相册/当前视图工具栏入口~~未接线~~**已由 2026-07-21 U-2(`5a97bd4`)接线,2026-08-13 核证**(选区工具条+右键+相册/视图三入口齐);崩溃遗留 staging 目录的「启动时列出可清理」UI 未做(orphan 目录无害,可手动删)。**⏸真机验收**:文件夹选择器/预检警告/进度与取消/刷新后进度恢复/完成后打开目录/10 万+ SelectAll/离线盘/库内目标确认。


### ▸ 详注 R-导出深审:2026-07-19 方案 A 导出线深审修复

> **2026-07-19 增补(方案 A 导出线深审修复,3 批全交付)**:对 f74f852/e55a679 两提交做深审(后端+前端双路独立复核 + 亲读交叉核对),发现 P1×2/P2×7/P3×8 共 17 项,已修 15 项(3 批提交:`c082e51`/`76ea910`/`2a24e99`),3 项语义/设计级留待用户裁(取消检查粒度为整文件级、进程崩溃遗留 staging 无启动清扫、磁盘满时已积累单项明细是否保留)。要点:①终局目录 rename 撞名(同秒连续导出)未去重且唯独漏清理 staging;②`stop_export` 仅核对 job_id 未核对 status,存在误取消新任务的窗口;③前端快照回填(先订阅后拉快照)可被迟到的旧 running 快照倒退覆盖已应用的终态;④两处目录/文件命名空间碰撞(manifest 文件名未预占、tmp 名与最终名共用命名空间);⑤`ViewStale`(task_plan 原承诺项)此前未实现,现补齐重取选区+重预检；⑥`fetch_export_meta` 预检侧多查 tags/albums、start 侧多一次 clone。验证:`cargo test --lib` 全量 716 过(新增 13 例)+ vitest 全量 1241 过(新增 7 例),vue-tsc/eslint/clippy 干净。ExportDialog.vue/BackgroundFileJobIndicator.vue 无既有组件测试基建,竞态类修复为人工核对(已知缺口)。**⏸真机验收未变**,深审与修复均为代码走查+单测,未做 GUI 交互验证。


### ▸ 详注 R-minimap轴:2026-07-17 无缝 minimap 轴

> **2026-07-17 增补(无缝 minimap 轴)**:无缝分组/不分组下时间轴 scrubber 失据整列隐藏(原 uiStore「留决策项」),右轴槽位改挂 **VSCode 式 minimap 轴**(`MinimapAxis.vue`,96px):canvas 微缩内容预览(placeholderColor 色块即时 + 微缩略图停稳回填,canvasThumbState LRU 2048 条/8MB + 飞掠闸门)+ 视口框拖拽/轨道点击居中直达/滚轮转发,双引擎经 currentLogicalY/scrollToY 通吃;滑窗几何纯函数 `minimapAxis.helpers.ts` 10 项单测锁定(统一斜率 k 闭式 + 钳高端点吸附)。显隐由画廊 chevron 钮管、持久化为 app_config 第 23 键 `seamless_minimap`(默认开)。验证:vue-tsc/eslint 0 错 + vitest 1207 全绿 + cargo fmt/clippy/test 629 全绿。三件套:[worklogs/2026-07-17-画廊无缝minimap轴/](worklogs/2026-07-17-画廊无缝minimap轴/task_plan.md)。**⏸真机验收**:无缝开→右轴出 minimap、拖/点/滚轮手感、chevron 收起重启记忆、54 万库深滚性能。本项即批量 15 项线遗留裁决 **D-003(#1 无缝下时间轴)的落地答案**。


### ▸ 详注 R-minimap深审:2026-07-19 minimap 深审与渲染模式设置

> **2026-07-19 深审与渲染模式设置**:设置页「缩略图」新增 `Minimap 内容`=`仅色块（即时）`/`缩略图（加载会有延迟）`,持久化为启动批第 24 键 `minimap_render_mode`,旧配置缺省保持缩略图默认;仅色块会中止图片请求并释放 bitmap。深审直接修复 5 类确定问题:scrollbar 键盘 a11y、Wheel deltaMode 与同帧吞量、缓存目录变更未失效、模式切换迟到请求、loading chunk 迟到写回绕过 48 块上限。用户续裁已落地:日期/分组视图新增“时间轴 / Minimap”会话内切换（默认时间轴）,微图失败按 400ms/1200ms 最多额外重试 2 次且不触发主缩略图自愈。自动化:Vitest 全量 1258、Rust workspace test 831、vue-tsc/ESLint/build/cargo check 全绿;clippy 被非本线 `scan_commands.rs:96 manual_inspect` 基线告警挡住。三件套:[worklogs/2026-07-19-minimap审查与渲染模式设置/](worklogs/2026-07-19-minimap审查与渲染模式设置/task_plan.md)。**⏸真机验收**:54 万库深滚、触屏/macOS/iOS 位图解码。


### ▸ 详注 R-日志重构:2026-07-20/21 日志能力重构(S1..S6)

> **2026-07-20 开线、2026-07-21 全线收官(日志能力重构,S1..S6 已落地,未 push)**:三路 sonnet researcher 并行联网调研(A=Rust/Tauri 后端与性能、B=应用内日志 UI 先例、C=agent 友好格式与治理,全文在 [worklogs/2026-07-20-日志能力重构/attachments/](worklogs/2026-07-20-日志能力重构/task_plan.md))后落盘 [designs/2026-07-20-日志能力重构方案.md](designs/2026-07-20-日志能力重构方案.md),开线 `lines/日志能力重构.md`。核心结论:**保留 tracing 三件套内核不换库**(GitButler/Spacedrive 同栈实证均绕开 tauri-plugin-log);落盘改 JSONL 单一事实源 + 三 Layer 扇出(文件/dev 控制台/UI 环形缓冲);**性能硬要求落点**=废除每条 fsync 的 RealTimeDailyAppender、off 档经既有 LOG_RELOAD 热切(稳态 ~1ns/调用点 callsite 缓存)、流水线 target 归一默认降噪、进度不走日志走既有事件体系、WARN/ERROR 重复压缩自写;schema=信封(ts/level/target/session_id/operation_id/msg)+attributes(error.chain 数组防多行;operation_id 显式传参——span 不跨 spawn_blocking);UI=设置 debug 区入口+独立日志窗口(@tanstack/vue-virtual,MVP 流+过滤+搜索,P1 按 AppError.code 聚合,P2 rusqlite 直方图+诊断包);前端桥=logger.ts 封装替换约 100 处裸 console+onerror 兜底。**已裁(2026-07-20,D-305):Q1..Q8 全部采纳推荐项**,主动建议一并采纳;**S6 前置两项(2026-07-21):worker 子进程日志(ai-worker/psd-worker)现状已足够不额外汇入(D-308)、span 耗时 Top N 埋点范围继续跳过 defer 独立任务——两项均已于同日被用户新裁决推翻并由独立后续任务落地,见下 2026-07-21 增补**。**施工由 Sonnet 5 承担(D-306),S1-S6 全部完成**:cargo test 816 passed、clippy 零新增(唯一 pre-existing scan_commands.rs:96 与本线无关)、前端 eslint/vue-tsc 零错误、vitest 1328 passed;off 态相对 info/debug 吞吐差值 <1%(验收基准通过,数字见方案 §3.2)。defer:移动端桥、minidump、MCP 查询(span TopN 已由后续任务落地,不再 defer)。**~~唯一遗留:用户批准后 push~~ 已推 origin/dev(六 commit 均为 origin/dev 祖先,2026-08-13 核证)**。**挂账(2026-07-21 清账波移交)**:docs 门 frontmatter 基线红本线占 5 文件(designs/2026-07-20-日志能力重构方案.md、lines/日志能力重构.md、worklogs/2026-07-20-日志能力重构/attachments/research-A-rust-backend.md、research-B-in-app-ui.md、research-C-agent-format.md),用户裁决转本线收口自办,不代修。


### ▸ 详注 R-span埋点:2026-07-21 span 埋点+worker 日志汇入

> **2026-07-21 增补(span 埋点+worker 日志汇入——日志线两项 defer 的独立后续任务,全五阶段收官,未 push)**:用户新裁决推翻 S6 两项 defer——①span 埋点选**更大范围**(D-309:流水线批次+主要 IPC command 级);②worker 日志选 **IPC 转发进主 JSONL**(D-310,取代 D-308)。落地:`SpanTimer` 手动计时 RAII 守卫+普通结构化事件(D-311,target=`scrollery::span`,弃 FmtSpan——合成事件不经全局 dispatcher、RingBufferLayer/UI 收不到;duration_ms=f64 毫秒),流水线 run 级 3 处(`pipeline:ai/face/derive`,derive 挂 operation_id)+IPC command 分档 info 39/debug 7(D-312,终表在三件套 findings);前端 `aggregateSpanDurations`+日志窗口分析标签「span 耗时 Top N」表。worker 侧:`exotic-protocol::WorkerLogLine{lvl,msg,fields}` 行协议,ai-worker(自定义 FormatEvent 收编 tracing+22 处手写调用点定级)/psd-worker(10 处)全量结构化,supervisor 行缓冲扫描逐行转发(固定 `target: "scrollery::worker"`+worker 字段,非 JSON 行 WARN+unparsed 兜底,64KiB 环形缓冲+崩溃摘尾保留,D-313/D-314)。reviewer 深审 0 严重/1 警告/3 建议全修(batch.rs 漏收编/drain 错误路径 flush/duration_ms f64/context 臂合并),存疑裁 D-315(span_close 不加 outcome)。验证:cargo test --workspace 全绿(scrollery-lib 835/exotic-protocol 27/ai-worker 14/psd-worker 8)、clippy 零新增、宏基准 off 相对差 +0.40%/-0.19% <1%、前端 vue-tsc/eslint 零错误、vitest 1336 全绿。五 commit(2b65fc9/247f1f7/995f904/eae75e9/6822875)。三件套:[worklogs/2026-07-21-span埋点与worker日志汇入/](worklogs/2026-07-21-span埋点与worker日志汇入/task_plan.md)。方案文档 §3.4/§5/§6/§8 正文已同步改写。**⏸真机 GUI 验收**(not automated):跑一次派生→分析标签 TopN 出行;跑 AI 分析→日志窗口可见 `target=scrollery::worker` 行。**~~遗留:用户批准后 push~~ 已推 origin/dev(2026-08-13 核证)**。


### ▸ 详注 R-图片编辑v2:2026-07-20 图片编辑开源包混合升级 v2 七阶段

> **2026-07-20 增补(图片编辑开源包混合升级 v2 全部七阶段落地,阶段 7 验证收官)**:E0 预览资源链、E1 Cropper.js 2.1.1 裁剪、E2 拉直 ±45°、E3 真色彩三滑杆调色(moxcms f32 分条 CMS + 双端同源公式黄金向量)、E6 授权门全部施工完成并提交(v1 `e3020c8` → 授权门 `d83cb89` → E0/E1 `31f5bfa` → E2 `fb6b796` → E3 `0a8d0ab` → 阶段7收口)。阶段 7 整体验证:完整 CI 对应门禁本地实跑全绿(Windows `cargo fmt/check/clippy -D warnings/test` 781 例、经 WSL 复核 Linux `check/test` 773 例、渠道依赖树断言、NOTICE.md 重新生成且 strong-copyleft 0、前端 `lint/typecheck/test` 1280 例 `/build`、渠道合规扫描、`tauri build --no-bundle` + 开机冒烟 276ms 达 Ready);D-110 依赖门确认 imageproc 未反向打开 image 默认格式族;release 包体差值实测 +2.50 MiB(+6.07%,临时 worktree 对比 v1 基线);D-008 内存基准新增直调生产函数的 `full_v2` 探针模式,24/50/100MP 三档实测,调色 CMS 有标签路径相对 v1 几何基线仅 +2.7%~+9%,并测出 100MP+45° 拉直会被既有内存预算准入门在解码前拒绝(与既有单测一致,非缺陷,不翻案)。详细测量证据见三件套 `docs/worklogs/2026-07-19-图片编辑开源包混合升级施工/findings.md`。**⏸真机验收**(不可自动化,留待用户):Cropper.js 触摸手势、广色域真实照片调色目视一致性、50MP 级拖动帧率体感;**已知全仓现状缺口**(非本线新增):当前 CI 无 macOS/iOS/Android 覆盖,移动端交叉编译与授权门/触屏交互全未验证。全部改动~~尚未 push~~**已推 origin/dev(2026-08-13 核证)**。**挂账(2026-07-21 清账波移交)**:docs 门 frontmatter 基线红本线占 3 文件 4 处(designs/2026-07-19-图片编辑开源包混合升级方案.md、lines/图片编辑功能升级.md、reviews/2026-07-19-图片编辑开源包混合升级方案审查.md[status+type 两处]),用户裁决转本线收口自办,不代修。


### ▸ 详注 R-清账波:2026-07-21 无人值守清账波

> **2026-07-21 增补(无人值守清账波)**:全仓遗留自动清账,六项落地(六独立 commit):①scan_commands manual_inspect 修复(全仓 clippy -D warnings 复绿,此前唯一 pre-existing 基线告警);②useHoverPreview 3 处 convertFileSrc 收口 resolveAssetUrl——收口后全仓 convertFileSrc 直连清零;scanStore 2 处裸 listen 因 useTauriListen 需同步 effect scope 上下文、迁移方向不唯一,转待裁;③改缓存目录时同步 thumbCacheDir 模块缓存+epoch 在途竞态防护(深审证实旧行为改目录后模块缓存滞留旧值、架空 minimap 失效链;展示保持 OS 原生形态);④删 npm 死依赖 @tauri-apps/plugin-opener;⑤eslint.config.js 头注如实化(非类型感知);⑥.gitignore 裸 models 锚定 /models/。池条目时效核实(07-06 P2 余池抽样 R6/F5/F9/F10/F11 + 07-10 C 池 chore 档 C41-C48):F10/F11/C48 已被后续工作修复(报告未标),R6/F5/F9/C41/C43/C45/C46/C47(启用类型感知半边)仍成立但均需拍板或实测,维持池内。门禁:cargo fmt/clippy -D warnings/test --workspace + vue-tsc/eslint/vitest 全绿(本地非 CI)。另录基线红:cargo fmt --check 于 backup/export 域 5 文件(backup/core.rs、backup/restore.rs、db/queries/export.rs、export/core.rs、ipc/export_commands.rs)持续红,与本波无关不代修,归属备份/导出线收口。三件套:[worklogs/2026-07-21-无人值守清账波/](worklogs/2026-07-21-无人值守清账波/task_plan.md)。**接续(同日第二次会话,U-* 采纳落地)**:决策清单 U-1..U-10 中 6 项按原推荐方向采纳完成施工,六独立 commit:`cb696dd`(scanStore 富化裸 listen 迁 detached effectScope+useTauriListen)/`5a97bd4`(导出 album/当前视图两入口接线,两轮深审全采)/`e3fe2b4`(LocalFs::abs 组件级路径边界检+9 测)/`00f45bb`(删 MediaFilter 死码型,考据非有意双型)/`78a611b`(删 repo-wide format 脚本)/`473311c`(sync-oss deploy key always() 擦除步,CI 未实跑仅 YAML 语法级验证)。U-4(随画廊/canvas 线)/U-8(随版本升级波)/U-9(需专项)/U-10(随 DB 波)维持挂起未变。**用户裁决**:docs 门 9 处 frontmatter 基线红转两线各挂账(图片编辑线/日志重构线,见对应段落),不代修。验证:cargo clippy/test + vue-tsc/eslint/vitest 全量 1345 过(基线 1336→+9)全绿(本地非 CI);cargo fmt --check 与 docs 门基线红均维持不变、非本波引入(~~fmt 基线红~~已由 2026-07-25 注释清理线 `762d1e2` 转绿,2026-08-13 实测 `cargo fmt --all --check` exit 0)。⏸GUI 真机验收清单见三件套 progress.md。**接续三段(同日第三次会话,planning 盘点波)**:29 线盘点无一收口净;自动施工 2 项三件套时效回写(`fa624fe` 导出线入口落地注记 / `68b45e4` 批量15项 D-003 消解注记);新决策清单 U-11..U-16 落盘本波 findings.md;门禁基线复核=worklog-kit check 9 红/8 文件+fmt 5 文件红维持不变,退役 check_docs.mjs 的全仓 223 红证伪为假信号(~~处置待裁 U-13~~**已裁决落地 `08045d8`,2026-08-13 核证**)。


### ▸ 详注 R-AI根治:2026-07-22 AI/face 流水线根治线

> **2026-07-22 增补(AI/face 流水线根治线,五 commit 全落+真机实证)**:用户报「AI 分析/人脸识别点开始没跑」,根因链运行时栈定案=MF 同步 `IMFSourceReader::ReadSample` 对 0x80004005 mp4 错误路径丢事件死等(7 线程栈铁证)→僵死 MF 进程级工作队列→rayon 池占满→派生 scope 永不 join→derivation token 恒 running→AI 无限让步(debug 级零可见)+face 被 rayon 池饿死;mkv 是受害者非毒源(`MF_VIDEO_EXTS` 白名单一直排除)。交付:`f3939a5` beforeDevCommand 前置 ai-worker 构建(新 target 缺 worker 第二层死点)/`874f511` 可观测性三修(让步变化才 info+status 扩 waiting_on+前端等待原因+300s 看门狗)/`acdca33` V22 orphan_count 毒任务防线/`e670369` MF 异步回调+30s 超时+泄漏隔离(opus 深审 0 严重;D-003 双钉零触碰)/`3f15308` unsafe impl Send(MTA 论证,clippy 清零)。真机实证:重跑 5.9s `Derivation pipeline completed`,125 毒批全清(114 done+11 error),全量四门绿(cargo 970+vitest 1345+clippy+vue-tsc)。已知边界:30s 超时路径真机未被 exercise(毒 mkv 被白名单前置拦截,异步化本身即修复主体)、seek 不设护栏(归 F-029 video-worker 进程隔离二期)。psd bad_signature 确认迁移后重编译自取 keyset 已自愈。worklog:[worklogs/2026-07-22-AI与人脸流水线根治/](worklogs/2026-07-22-AI与人脸流水线根治/task_plan.md)(已收口);二期候选 F-029 见 [status/AI与人脸流水线根治.md](status/AI与人脸流水线根治.md)。真机 GUI 手测已通过(2026-07-22 用户确认)。commit ~~未 push~~**已推 origin/dev(2026-08-13 核证)**。


### ▸ 详注 R-ai-worker闭包:2026-08-11 F-01 发货闭包断链修复(ai-worker 收口)

> **2026-08-11 增补(F-01 发货闭包断链修复——ai-worker 收口,enhance/video 残余)**:用户真机报「build 后安装,跑 AI 分析日志报错找不到 ai-worker」。根因与 07-25 审查 F-01 一致并实证:externalBin 仅 raw-worker,ai-worker 只在 beforeDevCommand 编译;7z 拆包确认 MSI/NSIS 载荷均无 ai-worker.exe,运行期 `ai_worker_exe()`(worker_client/process.rs)同目录查找失败 → 日志 `AI worker 启动失败 | startup failed`。修复:①新增 `scripts/build-ai-worker.mjs` 并挂 beforeBuildCommand(release 编译 ai-worker → 按 host triple 落名暂存 externalBin → 复制 ORT 四件套 DLL 进 src-tauri/binaries);②`tauri.conf.json` externalBin 增 `binaries/ai-worker`、`bundle.resources` 声明 onnxruntime/DirectML/dxcompiler/dxil 四 DLL(ai-core `resolve_ort_dylib` 强制 exe 旁 DLL、拒绝回退 System32);③新增 `scripts/verify-bundle-content.mjs`(F-001 候选落地:内建 selftest,三层断言 staging/conf/7z 拆包载荷),接线 release.yml(拆包硬验,发布必过)、ci.yml smoke(staging+conf)、build-internal-installer.ps1。验证:本地重建 NSIS 拆包 15 条目含 ai-worker.exe+四 DLL,断言全过;新包安装目录布局 = scrollery.exe + ai-worker.exe + raw-worker.exe + 四 DLL 同根。**MSI 本地重建被卡死 msiexec(锁旧 MSI 文件,无活动事务)阻塞,CI 干净 runner 不受影响**。残余:enhance-worker(模型清单仍 PENDING 占位,就绪后按同模式补 externalBin)与 video-worker(prod 打包未就绪)仍属 F-01 断链,归 F-003 跟踪。三件套:[worklogs/2026-08-11-ai-worker发货闭包修复/](worklogs/2026-08-11-ai-worker发货闭包修复/task_plan.md)(已收口)。


### ▸ 详注 R-注释清理:2026-07-25 全仓代码注释清理精简线

> **2026-07-25 增补(全仓代码注释清理精简线,四 commit 落 dev ~~未推~~**已推 origin/dev,2026-08-13 核证)**:范围 = `src-tauri/src`、`crates`、`src`(排除 vendor)、`scripts`,689 个在范围文件约 31000 行注释。规则 1「英文行紧邻中文译文行 → 删英文」由确定性脚本执行,规则 2-7 与「精简」按域人工执行,保留红线 = 抑制指令 / Copybara BEGIN-END-INTERNAL / 决策锚点 / 测试钉定 / rustdoc-TSDoc 公开契约 / 外部 URL / 命令行与算例 / 并发与安全约束。主体 `d9b337f`(283 文件,+744/−3068,**净减 2324 行**)+ `6f1cd57`(顺带清基线红:eslint 排除 `.claude/**`、clippy err_expect×3)。**教训(重要)**:① 规则 1 脚本只按行邻接判定译文关系、不校验语义等价,误删过 Copybara 剥离标记 / cargo 跑法 / 算例 / 公式 / 编号项 / intra-doc 链接,经取证式审计(先机械抽取被删的非 CJK 注释行 + 上下文落盘,再逐条对拍)逐条修复;② **两份复核回执被查出编造发现**——引文在全仓 diff 中逐条 grep 均不存在,其中一条还被下游修复代理照抄进 `crates/scrollery-ai-core/src/engine.rs` 产生无谓改动、经证伪后删除。结论:审计结论必须落盘可对拍、引文须逐字可查,自称「已对抗核验」的回执同样要机械比对。**待裁 5 项已于同日第二会话全部处置**:fmt 5 文件既有债清掉(`762d1e2`,`cargo fmt --all --check` 由长期红转 🟢)、state.rs 与 fast_scan.rs 残留英文块落地(`ba3be1c`,机械校验非注释增删行为 0)、en-US.ts 的 `U-2` 工单号核实为复制 zh-CN.ts 既有注释非新编(无需改动)、ReaderSettingsSection.vue 核实无可删项(跳过正确)。**新待裁 1 项**:确定性扫描实测全仓仍有**英文散文注释行 515 行 / 84 文件**(其中疑似漏删双语对 150 处)+ **99 个 .rs 仍带自指文件路径头**,根因 = 上一轮脚本守卫「英文块须单行」使多行英文块整批逃逸——**同日已裁:第二轮暂不做,仅记录留账,重启须新决策**(明细与扫描器路径在三件套 findings.md F-h,数已实测勿重扫)。门禁:fmt / clippy -D warnings / cargo test --workspace / npm lint / vue-tsc / vitest 全绿(本地非 CI)。三件套:[worklogs/2026-07-25-全仓注释清理精简/](worklogs/2026-07-25-全仓注释清理精简/task_plan.md)。


### ▸ 详注 R-超长拆分:2026-07-25 超长文件拆分施工线

> **2026-07-25 增补(超长文件拆分施工线,A 详案 10 + B 简案施工全落,derivations 待裁,dev ~~未推~~**已推 origin/dev,2026-08-13 核证)**:承接方案线 [planning/2026-07-25-超长文件拆分方案/](planning/2026-07-25-超长文件拆分方案/task_plan.md)(45 件分层榜单 + 14 份详简案),另立施工线 [planning/2026-07-25-超长文件拆分施工/](planning/2026-07-25-超长文件拆分施工/task_plan.md),commit 区间 `ba3abde..<本次收口提交>`(dev 尖端)。CSS 首刀+铺开(6 Vue 文件 style 外置)、Rust 详案 5/5(layout/faces/scan/worker_client/lib.rs 按域 boot 化)、Vue 结构拆 6/6(MediaGrid/ContentViewer/DocumentViewer/FoldersSection/MediaGridCanvas/SettingsView 拆共 54 composable)、tierB Rust 简案 14/14(message/state/fast_scan/worker_service 测试类型迁出、schema 三纪元、EXIF 注入、worker 行日志、video 三件、models 六域、layout geometry+grid_pack、face_pipeline 四阶段)全部落地,均过独立复核(累计抓 2 处严重:FoldersSection computed 包一层掐断 triggerRef 回归、layout/faces 方案表述错误两处更正)。**derivations.rs 按 D-454 明示跳过未施工**——真 user WIP 红线未解除,待用户裁归属。P6 全量清算:A 段 7 门禁(fmt/clippy/cargo test/typecheck/vitest/build/eslint)除两处既有基线红(cargo test `error.rs` 单测 flaky、vitest `alignment-grid.contract.spec.ts` 等式② 1 例,均非本线)外全绿;B 段全角折损全量对拍(python FF01-FF5E 规整化 + difflib,覆盖清单全部文件族)发现并还原 1 处(`exotic/worker_log.rs` L187 `EOF` 注释行标点半角化)。子组件抽取(MediaGrid 3 件 + ContentViewer 4 件)因宿主 scoped 后代选择器约束跳过留账,待单独授权。**⏸GUI 手测清单**待批(各阶段复核文档内)。


### ▸ 详注 R-图片编辑方案:2026-07-19 图片编辑开源包混合升级方案(落盘时未施工;后由 v2 施工承接)

> **2026-07-19 增补(图片编辑开源包混合升级方案落盘,未施工)**:用户反馈方案 C v1 编辑太简陋,两路联网调研(一体化前端编辑器 / Rust 处理包)后落盘 [designs/2026-07-19-图片编辑开源包混合升级方案.md](designs/2026-07-19-图片编辑开源包混合升级方案.md),开线 `lines/图片编辑功能升级.md`。核心结论:**无一体化包可整体替换**(tui-image-editor 停更 4 年、filerobot 藏 React 运行时、Pintura 收费、fabric/Konva 壳=换底座重新自研),v1「前端参数+Rust 真处理」架构维持,升级零件:E1 交互层 vue-advanced-cropper(MIT,带 1 天准入 spike,失败回退自研打磨)、E2 拉直 ±45°(imageproc 任意角插值 + 内接矩形闭式)、E3 调色三滑杆(亮度/对比度/饱和度,**双端同源公式 + 黄金向量 fixture 对拍;禁 CSS filter 与 imageops 现成函数**——调研实证两者数学不等价)。施工硬门:D-008 内存基准复跑(fine rotate expand 最坏 ≈2× 面积)。待裁 U-1..U-6(曝光并入亮度/锐化不进/滤镜标注 defer/方案 C 阶段 5 遗留合并清)。**未施工,施工时建三件套**。**同日第 2 版修订**:外部审查([reviews/2026-07-19-图片编辑开源包混合升级方案审查.md](reviews/2026-07-19-图片编辑开源包混合升级方案审查.md))4×P1+2×P2 核实全部成立并全部落稿——F-01 色彩策略用户裁**真色彩管理重路线**(D-108:sRGB f32 工作空间,moxcms 首选/lcms2 对拍备选,P0-CM spike 硬门;零 adjust 走 v1 原路径字节级不变)、F-02 落 D-109 预览资源端点(Rust 产降采样 sRGB 位图,全链不吃全尺寸源,blob URL 消 taint;D-008 基准测法升级为整进程树)、F-03 落 imageproc `default-features=false` + D-110 准入门、F-04 拉直统一闭区间 [-45,45]、F-05 撤销 CSS filter 浏览器分歧论据(规范原文:Filter Functions 钉死 sRGB)、F-06 落 §9 来源表 + E1 改双候选同级 spike(vue-advanced-cropper 2024-06 起停更 25 个月 vs Cropper.js v2 2026-04 活跃);自查补一处:fast_image_resize 已是依赖非「日后引入」。**用户补充裁决 D-112:编辑整体=付费高级功能,free 版不提供,插件形态参考 psd**——复用 exotic 授权链(keyring token+keyset 验签+evaluate),前端 PluginGate、后端新稳定码 `edit_not_entitled` 真门,v1 入口施工首步挂 gate;定价/生产密钥归 Part8,口径不提定价。估算 5–7 → **8–11 天**。


### ▸ 详注 R-深审07-25:2026-07-25 最近一周代码实现深度审查(发布建议:阻断)

> **🔴 2026-07-25 增补(最近一周代码实现深度审查——发布建议:阻断)**:窗口 `247d05c..f97137e`(257 提交 / 402 非文档文件 / `+54312-3149`),全文见 [reviews/2026-07-25-最近一周代码实现深度审查.md](reviews/2026-07-25-最近一周代码实现深度审查.md),三件套 [worklogs/2026-07-25-最近一周代码实现深度Review/](worklogs/2026-07-25-最近一周代码实现深度Review/task_plan.md)。**13 项 finding(1 P0 / 5 P1 / 6 P2 / 1 P3),~~全部在当前 HEAD 复证仍成立,均未修~~——2026-08-13 核证订正:已修 6 项(F-01 半修=ai-worker 发货闭包 08-11 收口 `2564dc1`;F-02/F-07/F-09/F-11/F-12 已由第三轮直修 `504f209` 修),F-03/F-04/F-05/F-06/F-10/F-13 复证仍成立**。① **F-01(P0)worker 发货闭包断链**:`tauri.conf.json` 的 `externalBin` 只列 `raw-worker`,`ai-worker` 只在 `beforeDevCommand`、`enhance-worker` 两条路径都不构建,而 `ai/worker_client.rs:597-616` 与 `enhance/service.rs:1043-1063` 都只在主程序同目录找可执行文件——正式包的 OCR / 语义与人脸 AI / 增强全部拉不起 worker,`cargo test --workspace` 绿灯漏掉这一断链。② **F-02(P1)增强模型清单仍是 `PENDING_USER_REPO` 占位 URL(`sha256=None`/`size=0`)却被判 `manifestReady=true`**,设置页下载按钮已对用户开放,点击必走无效地址;与 F-01 叠加 = 增强子系统 fresh install 无一条可完成路径。③ **配置重构专项 2026-07-23 报的 6 项到当前 HEAD 无对应修复提交**,逐项复证仍成立(F-03 跨进程写锁缺失 + watcher 旧快照回退 / F-04 schema 无区间约束致 `derive_batch_size=0` 停流水线、巨值 `Vec::with_capacity` 异常分配 / F-05 `thumb_cache_dir` 空串当相对路径 + async 持锁同步 IO / F-07 漏 `ai_hq_cache_enabled` / F-08 重启提示被覆盖丢失 / F-09 迁移 `.ok()` 吞 SQLite 错误后永久 no-op)。④ **F-06(P1)RAW `externalBin` 在非 Windows 与交叉构建必然缺文件**(脚本按 rustc host 而非 build target 命名),macOS/Linux bundle 与 ARM64 包受阻。⑤ 增强链另有 F-10 队列无 FIFO、F-11 ingest 失败仍报 done、F-12 预览固定路径致缓存串图。⑥ F-13 提交区间 whitespace check 红(`enhance-worker/src/main.rs:624` EOF 空行)。**方法与限制**:只审已提交历史,工作区并行 WIP 全程未触碰、不纳入结论;因 WIP 污染故意不跑 cargo/vitest/build,P0/P1 全部建立在提交 blob + 调用链 + 打包配置的静态证明上——**F-01 这类发货断链只有真打包才能终局证伪**,已登记耐久候选 F-001(安装包内容断言进 CI)/F-002(fresh install 四链冒烟 + macOS/Linux 真实 `tauri build`)/F-003(配置层确定性测试)/F-004(增强状态机测试)。**修复顺序**:F-01/F-02 → F-03/F-04/F-05 → F-06 → F-07~F-12 → F-13。收口回写复跑三项头部数字全部精确吻合(见三件套 progress)。**处置待主线排期,本条只登记不施工。**
>


### ▸ 详注 R-深审07-31:2026-07-31 第三轮全仓深审+无分叉直修

> **2026-07-31 增补(第三轮全仓深审+无分叉直修,~~未提交~~**已提交 `504f209`,2026-08-13 核证)**:正式报告 [reviews/2026-07-31-全仓代码深度审查与直修.md](reviews/2026-07-31-全仓代码深度审查与直修.md)。本轮直修配置迁移吞错/非法布尔、增强占位 manifest 误开放与 ingest 吞错、前端多类异步旧响应、Unix asset URL、增强预览原子写/缓存、AI HQ 外改重启、timer、PostCSS 生产漏洞及已批准 H 定向脱敏；Windows 本地 Rust 1304 passed/10 ignored、前端 1525 passed，fmt/check/clippy/lint/typecheck/build/渠道/NOTICE/生产 audit 全绿。**🔴 发布仍阻塞(残余)**:~~`externalBin` 仅含 raw-worker，ai/enhance/video worker 没有生产 bundle/fresh-install 闭包~~(ai-worker 已由 2026-08-11 增补收口;**enhance/video worker 无生产 bundle/fresh-install 闭包仍成立**)；配置跨进程事务、layout latest-wins、错误类型化、增强 FIFO、dev 审计与 bundle 余量列 D-001..D-008 待裁。三件套:[planning/2026-07-31-全仓代码深度审查与直修/](planning/2026-07-31-全仓代码深度审查与直修/task_plan.md)，未获收口授权，保留施工态。
>


### ▸ 详注 R-UI链路08-03:2026-08-03 UI 开发加载/关闭链路修复

> **2026-08-03 增补(UI 开发加载/关闭链路修复,已提交 `0c9ac89`)**:固定 Tauri/Vite 开发端使用 `127.0.0.1:1420`，避免 localhost 的 IPv4/IPv6 解析不一致；前端关闭监听提前到首个启动 IPC 前，并增加心跳；Rust 在前端失联时允许原生关闭、托盘模式原生隐藏。详见 [UI加载失败排查](worklogs/2026-08-03-UI加载失败排查/task_plan.md)，待 GUI 真机回归。
>


## R2. 滚动增补移交归档 · 二期(更新日志与 2026-08-13—08-23 头部叙事)

> (2026-08-23 二轮瘦身:按「完成即归档」,todo.md 头部剩余历史叙事(更新日期日志/核证/状态更正/近期交付波)整块移位至此,零删减;todo.md 各留一行 📦 指针,活项(⏸真机/待裁/挂账/余项)随指针保留。)


### ▸ 详注 R2-更新日志:待办总览更新日期日志(2026-07-02—2026-08-14)

> **更新日期**:2026-07-02(G 节=当日四路 agent 全景扫描新增;当日晚间无人值守波清账 G1 全部六项,分叉裁决记录见各行备注;续波三项拍板——worker 化全线启动 / 联网预取全量放行 / Part3-T12 代码先行——G2 散件随之清账,见 G2/G3 各行;**2026-08-13:全仓未完成工作代码核证回写,过期状态断言已就地划线订正,见下方 2026-08-13 增补与 [reviews/2026-08-13-全仓未完成工作梳理与代码核证.md](reviews/2026-08-13-全仓未完成工作梳理与代码核证.md)**; **2026-08-14:完成/可收口任务全量代码核证回写——25 条已完成线新登记 + 3 处过期断言划线订正 + 2 处文档残留修正,见下方 2026-08-14 增补**)。「阻塞源」列写清「为何还不能做」,避免误动高回归/投机项。


### ▸ 详注 R2-核证08-13:全仓未完成工作代码核证回写

> **2026-08-13 增补(全仓未完成工作代码核证回写)**:57 agent 多代理审计(11 路发现 → 44 批对抗式对码核证,373 条声明全部独立对码,0 幽灵条目),报告全文 [reviews/2026-08-13-全仓未完成工作梳理与代码核证.md](reviews/2026-08-13-全仓未完成工作梳理与代码核证.md)。**下列订正均以代码/git 实测为准,已就地划线;确认未完成(180+)/仅真机可验(50)/已裁刻意(46)维持原状未改**。要点:①「未提交/未推/待合并」类声明大面积过期——07-31 直修已提交 `504f209`、视频格式扩展已合并 `821c290` 且已随 dev 推 origin、日志/span/编辑/AI/注释清理/超长拆分各线 commit 均已入 origin/dev;当前真正未推的仅 dev 领先 origin/dev 的 **17 commit**(git 实测);②07-25 深审 13 项 finding「均未修」已推翻 6 项(F-01 半修/F-02/F-07/F-09/F-11/F-12 已修 `504f209`),F-03/F-04/F-05/F-06/F-10/F-13 复证仍成立;③fmt 基线红已由 `762d1e2` 转绿(`cargo fmt --all --check` 实测 exit 0);④`codex/fix-ci-raw-worker` 分支 9 commit(含 RAW F-06 交叉构建修复)仍未合入 dev;⑤本轮回写只划掉已证伪的状态断言,不改历史叙事。
>


### ▸ 详注 R2-P0P1直修08-13:当日审查 P0/P1 四项直修

> **2026-08-13 增补(当日审查 P0/P1 四项直修,~~未提交~~)**:10 路 finder 审查(7691 行 diff)23 项经核证 findings,四项 P0/P1 已直修完毕(**工作树未提交**,commit 待用户指示),报告全文 [reviews/2026-08-13-审查P0P1四项直修报告.md](reviews/2026-08-13-审查P0P1四项直修报告.md)。① **P0 安装包断言必红**(verify-bundle-content.mjs):Tauri v2 MSI 资源匿名化为 `PathFile_<hash>` 无扩展名,按名比对必误报四 DLL 缺失(2026-08-11 worklog 曾把此误报当断言有效证据)→ 改 MSI 按**字节尺寸**匹配(缺项/截断均错开),selftest 补正反样本,实机 MSI 11 条目+NSIS 15 条目全绿;② **P1 ask 关闭静默直退**(lifecycle.rs):3s 心跳超时在启动首心跳前/主线程卡顿/隐藏页节流(隐藏 5 分钟+约 1 次/分)三类窗口静默绕过确认框 → `close_decision` 三分派 + ask 陈旧走**宽限任务**(恢复窗口可见→轮询心跳≤10s→恢复转交前端弹框/耗尽才原生销毁,`CLOSE_GRACE_PENDING` 防堆叠)+ 前端 visibilitychange/focus 即时补心跳;4 新单测,⏸真机手测(最小化 5min+ 后任务栏 ✕ 须弹框);③ **P1 mediaStore navContext 脱节**:openDetail(fromLayout) 清空移回 fetch 前(失败不残留)、navigateDetail 的 currentIndex 提交后置(在途窗口不超前于展示项),+4 spec 测;④ **P1 enhance ingest 失败孤儿文件**:失败闭包内 `remove_file(&claimed)` 回滚(与既有错误路径对称),⏸真机验证(DB 锁失败后库目录无残留)。门禁全绿:fmt/clippy -D warnings/cargo test --workspace/lint/vue-tsc/vitest 134 文件 1532 测/实机拆包断言。**留账 18 项 P2 级 findings**(manifest 门禁严于引擎/loadBuiltin 漏守卫/入场动画删除/CARGO_TARGET_DIR 硬编码/stale 守卫 5 路手搓等)与 **1 项待裁**(migrate 坏布尔翻转默认 true 键——刻意+已单测但语义不等价)见报告文末。
>


### ▸ 详注 R2-核证08-14:完成/可收口任务代码核证回写

> **2026-08-14 增补(完成/可收口任务代码核证回写)**:对 todo.md 与 planning/ 全部 56 线逐一核对「已完成但未更新」候选,以下 25 条**均经 git commit 实测 + 代码符号/文件存在性核证**(不采信文档自述),新登记于本节「2026-08-14 核证表」;3 处过期断言就地划线订正(降噪超分 J-1..J-8 已定案+P0 施工已开工 / UIUX 线 S7 已收官 / U-P1-b 前提失效);2 处文档残留同批修正(asbuilt-spec.md 计数 16→17、Spec15 三件套阶段 2 勾选)。确认未完成项(首次 AI 语义搜索仅诊断、修复待授权——useBucketVirtualScroll.ts:358 error 粘滞复证在场;OCR/ICC 线施工在飞;文档缩略图等 D-C/D-D)维持未登记。**收口(closeout + 迁 worklogs)未自动执行,均待用户授权**;「余项」含 ⏸ 真机 GUI 者,代码与本地门禁均已落地(非 CI)。
>


### ▸ 详注 R2-核证表08-14:核证表(25 条)+ 划线订正 + 文档残留修正 + 43 线收口执行

> **2026-08-14 核证表(25 条已完成线新登记)**
>
> | 线(planning 目录) | 核证证据(commit / 代码符号) | 状态 | 余项 |
> |---|---|---|---|
> | 2026-07-17-md阅读器巨型文档内存爆炸修复 | markdown.ts 断长跑+分片;端到端 7.9GB→177MB 对拍 | ✅ | ⏸真机 |
> | 2026-07-17-底栏重构-内容页动态文件信息 | StatusBarFileInfo.vue 在场(3 文件),47 测 | ✅ | ⏸真机 5 项 |
> | 2026-07-17-视频封面关键帧提取性能审查 | video/d3d.rs 在场;4K HEVC cover+kf 6.3× | ✅ | ⏸真机(软解回退) |
> | 2026-07-17-阅读器页UI优化 | DocumentViewer fold-item 折叠容器;1184 测 | ✅ | ⏸真机 6 项 |
> | 2026-07-18-两日代码修改审查 | 报告落盘;9 项已由审查9项修复线全落地(efec238/0d1234a/305d4b6/8b0220f) | ✅ | 可收口 |
> | 2026-07-18-根文件夹显隐 | f931db5(V20)+6d92779(V21,is_hidden 10 文件);637 测 | ✅ | ⏸真机 + push 待批 |
> | 2026-07-18-缩略图进度刷新丢失与视频派生独立控制 | full_thumb_gen_status 在场(5 文件);645 测 | ✅ | ⏸真机 4 步 |
> | 2026-07-19-图片简单编辑施工(v1) | editing/{metadata,geometry,naming,io,ingest}+edit_commands.rs;750 测 | ✅ | 集成测试/真机缺口已诚实录 |
> | 2026-07-19-数据备份与恢复施工 | backup/{core,manifest,restore,swap,dbread}.rs;5018c76/da83688/835fd73 | ✅ | ⏸真机恢复交换 |
> | 2026-07-22-前端动画重构 | 五批(8bee8e4/9f5ebb1/a60acfc/ecea926)+1f3ea06;route-fade 零残留 | ✅ | ⏸真机 §P5 5 条 |
> | 2026-07-22-应用配置重构-外置配置文件与热应用 | b216dcc/5b0b925;883 测 | ✅ | 待裁 5 条(D-c01..)+⏸真机 |
> | 2026-07-23-全仓深度review与直修 | 9 commit 03cd68c..cc88f8c;J1-J17 全落地 | ✅ | ⏸真机 4 项;J5/J6 未确证 |
> | 2026-07-23-窗口化沉浸模式-非全屏自动隐藏顶栏底栏 | 324453a;auto_hide_chrome_windowed 在场 | ✅ | ⏸真机 + push 待批 |
> | 2026-07-23-大图浏览器无闪切图与错误态修复 | useViewerImageSource 在场(3 文件);1414/1423 测 | ✅ | ⏸真机坏图复验 |
> | 2026-07-23-审查应用配置重构 | 报告落盘(4P1+2P2) | ✅ | 修复待裁(部分已由 504f209 修) |
> | 2026-07-24-全仓深度review(第二轮) | C1-C4(c6cc2c6/f0f5654/e8ebdf8/141cd32);H 项已落 61d125f | ✅ | ⏸rust-linux CI + push |
> | 2026-07-24-画廊轴minimap解耦与按钮迁底栏 | e1b1789;axis_mode 在场;C-1 已裁 | ✅ | ⏸真机 |
> | 2026-07-24-施工规格集(asbuilt-spec) | docs/spec 17 篇全产出+波3 收口;计数残留已修 | ✅ | 收口登记 |
> | 2026-07-24-全仓未完成工作梳理 | 清单+裁决点已交付 | ✅ | 可收口 |
> | 2026-07-17-上线前三功能方案(导出/备份/编辑) | A/B/C 三案均已由对应施工线执行 | ✅ | 方案线可收口 |
> | 2026-07-17-未完成工作梳理与free社区版最短路径 | 两批清单已出(complete) | ✅ | 可收口 |
> | 2026-08-03-canvas设置项生效检查与修复 | cb459c4;hoverScale 显式下发 canvas 绘制 | ✅ | ⏸真机观感 |
> | 2026-08-03-画廊大图首次打开慢与闪烁修复 | 2940600;viewerRouteLoader 空闲预取+去入场动画 | ✅ | ⏸真机低速磁盘 |
> | 2026-08-04-AI语义搜索栏UI优化 | ac07f9f;SemanticSearchPanel 三区重构 | ✅ | — |
> | 2026-08-07-Spec15深化-各功能详细工作流 | 475cb5a(含 Spec00 17 篇计数修正);Spec15=891 行 | ✅ | asbuilt-spec 计数已修 |
>
> **三条过期断言划线订正(同批)**:① 降噪/超分子系统线「未施工」→ **J-1..J-8 已 2026-07-24 定案(J-2 改用户自有 HF 仓)、P0 施工已开工**(spike A-D 全绿+批 1-3 落地,enhance-worker crate 已建,host service.rs 1210 行;批 4 在途),残余=模型清单 PENDING 占位(F-02)见下行正文划线;② UIUX 线「S7 未开始」→ **S7 已 2026-07-15 收官**(阶段 13,真机 round9 验收通过;S1 原语全建成含 UiSelect 2753777),见下行正文划线;③ U-P1-b「lib.rs 余 832 行,数周后重测 churn」→ **lib.rs 实测 368 行,e92e2b2 boot 化拆分(D-450)已实质完成,U-P1-b 前提失效视为被吸收**,见 U 线行划线。
>
> **两处文档残留修正(同批)**:docs/status/asbuilt-spec.md「16 篇…Spec00–Spec14…待主线提交」→ 17 篇(Spec00–Spec15)+已提交 dev;Spec15 三件套 task_plan 阶段 2 勾选 complete(475cb5a)。
>
> **2026-08-14 收口执行(同日,A+B 两级 43 线)**:43 线全部收口归档至 [worklogs](worklogs/README.md)(归档时统一冠 2026-08-14- 收口日前缀,2026-08-24 订正为各任务创建日)(closeout.md 逐候选处置全覆盖;三件套 frontmatter 转快照;worklogs/README 已登记 43 行;todo.md/designs/lines/spec/status/reviews 全部 planning 引用同批更新为 worklogs 路径)。**经验蒸馏 20 节入 [experience.md](experience.md) §26-45**(内存探针三坑/Channel 生命周期/就地覆盖三坑/view_rotation 烤入/MF XVP 陷阱/chardetng 采样/选择性暂存术/终态门控/大文件拆分方法论/嵌入预览优先/稳定码三胞胎等)。**真机 ⏸ 项与未决项按 closeout disposition 挂账**,集中登记于本段「收口挂账清单」;已落地候选处置为 completed→todo.md(核证表/既有增补为落点)。
>


### ▸ 详注 R2-Canvas08-16:Canvas 缩略图加载渲染性能线收官

> **2026-08-16 增补(Canvas 缩略图加载渲染性能线收官)**:按 [reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md](reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md) 修订版 §5 顺序完成阶段 0–5:测量脚手架 + 真机基线 + 多档源上线(上表已翻 ✅)+ 恢复缺口重试(`80aebb9`,队列内有界退避)+ 阶段 4 三项数据裁决(4a 维持反证 no-op / 4b 扩槽不采纳 / 4c 选中态批处理被 A/B 否决回退——「全选态 jank 44-60%」系测量实例伪影,健康实例逐格绘制贴满 vsync)。工作记忆归档 [worklogs/2026-08-16-Canvas缩略图加载渲染性能优化/](worklogs/2026-08-16-Canvas缩略图加载渲染性能优化/)。**新登记 2 项立项候选**:① app 空视图死态(页面 reload/组件重挂载后可入「0 个项目」不自愈,布局 width<100 放弃后无重试路径,仅重启可恢复;生产首挂载是否可触发待查);② app 窗口保存状态可损坏至屏外(实测 -21333,-21333/158×26,用户可遇「打开看不到窗口」;Win32 MoveWindow 可修但产品侧缺钳制/自愈)。


### ▸ 详注 R2-稳定化08-18:恢复后画廊稳定化(画廊冷启动无限重排修复状态更正与验收)

> **2026-08-18 状态更正与验收(画廊冷启动无限重排修复)**:原工作树中的 P0 实现未提交且已整体备份后回退；当时 `src` 以 `HEAD 0020ef3` 为准，不能再声明 presentation shell、尺寸 reset key 收窄或其 `151 files/1645 tests` 验证已经落地。恢复后的前端门禁为 `134 files/1535 tests`，本地画廊→查看器→返回已通过；用户已完成真实 Windows WebView2 的冷启动、查看器往返、侧栏后往返、筛选/分组滚动四项验收，结果全部通过。该恢复阶段未复现冷启动无限重排，故未重引入原混合补丁。记录见[恢复后画廊稳定化](worklogs/2026-08-18-恢复后画廊稳定化/task_plan.md)。
>


### ▸ 详注 R2-路由覆盖层08-18:查看器路由覆盖层呈现(Canvas 生命周期与查看器返回闪烁收官)

> **2026-08-18 续作收官(Canvas 生命周期与查看器返回闪烁)**:用户随后复现“查看器左侧栏开启后返回画廊”的视觉闪烁。Canvas 生命周期以 `35c4cff` 独立重做并通过真实 GUI 验收；侧栏几何守卫后仍有视觉空档，故用户否决任何以延迟关闭换稳定的方案。最终 `3efdee6` 保留 `/view/:id` 的 URL/历史/深链语义，但让画廊 DOM 常驻、以 `ContentViewer` 覆盖层呈现，关闭立即撤层。用户已在 Windows WebView2 验收连续往返、侧栏、快捷键并确认通过；决策见[查看器路由覆盖层呈现裁决](decisions/2026-08-18-查看器路由覆盖层呈现裁决.md)，过程见[Canvas缩略图渲染重做 worklog](worklogs/2026-08-18-Canvas缩略图渲染重做/task_plan.md)。
>


### ▸ 详注 R2-扫描08-21:入库扫描元数据读取性能优化

> **2026-08-21 增补(入库扫描元数据读取性能优化)**:已采纳审查建议分 5 阶段落地并逐阶段 commit(`a4c09ac7` 富化 keyset+V24、`40c5e172` prepare_cached+V25 删冗余目录索引、`8a20c635` quick 剪枝前置到 walker+前端重扫默认 quick、`e5749caf` seen 流式 TEMP、`ff82c5d4` eager HeaderBuf+阶梯头读+XMP 字节搜索)。本地验证:Rust lib 1077 passed/5 ignored/0 failed、vue-tsc/scanStore vitest/eslint/prettier/clippy 通过(计数为阶段5 时点 1.97 工具链实测,见 2026-08-23 增补的更正)。**余项**:真机大库导入与增量重扫 A/B(当前仅有 SQLite 简模证据,公共性能结论须等实测);quick 账本为会话级,重启后首扫仍全量。
>


### ▸ 详注 R2-扫描线08-23:扫描线 P1 修复与对照线优秀部分吸收

> **2026-08-23 增补(扫描线 P1 修复与对照线优秀部分吸收)**:双线对比评审(与 scrollery-tmp-23a5679@207af98 同题实现对照)发现并修复 2026-08-21 线 P1——单写连接上单表 `_mm_seen` 会被并发异根扫描清空、致收尾误标已见项 missing;按根隔离 + `SeenWriter` + 差集后 DROP(`6c275e7`)。同时吸收对照线优点:`226e7c5` 富化选批队列表化(folder/date+filename 序脱离 O(N²),代次后缀+RAII 根除其 DROP 竞态与内存滞留)、`1692fd5` walker 单次 symlink_metadata + XMP 首字节定位。**验证更正**:原"1077/5"为阶段5 时点旧值;当前 stable(1.98)实测 Rust lib **1094 passed/6 ignored/0 failed**、vue-tsc 通过、clippy 本线文件零新增警告(存量 11 条 `chunks_exact_to_as_chunks` 均在未触碰文件,系 1.98 新 lint 基线漂移)。**新登记余项**:①CI dev 分支自 2026-08-20 起既有红(自托管 runner build.rs 环境、Linux job 排队 24h 超时、docs-governance 对 2026-08-15 既有文档报红),与本线无关,须独立立项修复;②对照线独有的 live photo 目录分片流式与扫描期重排降频未移植(前者含 SELECT 游标中 UPDATE 隐性约束需单独设计),待需要时立项。过程见[扫描线P1修复与tmp吸收 worklog](worklogs/2026-08-23-扫描线P1修复与tmp吸收/task_plan.md)。

---

## U. 后端大文件拆分审查(2026-07-16 立项)

> (2026-08-23 施工全档归档:todo.md U 线 7 个 ✅ 行的现状结论剪切至此,零删减;已批准设计 [U 线设计件](designs/2026-07-16-后端大文件拆分审查与方案.md);工作记忆 [worklogs/2026-07-16-后端大文件拆分审查/](worklogs/2026-07-16-后端大文件拆分审查/task_plan.md)。todo.md U 线保留 ⏳ 真机 smoke / 可选 P1-b 两行与指针。)

> 性质=全后端 ≥900 行共 16 文件普查 + 三档裁决(拆分 4 批 / 观察 4 / 豁免 6)+ 受托对 T 线方案做时效性核查。2026-07-16 用户采纳代码审查修订版;**同日施工会话四批全落地(6 提交,开工 HEAD 1a7a34a,本地门全绿非 CI)**,余=真机 smoke(GUI)+ 可选 P1-b 待重测。

### ▸ 详注 U-全档:普查裁决 + T 线核查 + 四批施工(U-P1-a…U-P4-a)

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ✅ | 普查 + 结构测绘 + 三档裁决 + 设计件落盘 | 16 文件逐一实测(4 并行只读测绘);结论=**queries.rs 之外无第二巨石**,四批拆案全是「结构问题恰好住在大文件里」:U-P1 lib.rs(42 commits churn 王,注册清单+setup owner 下沉)/U-P2 ai_commands(**层次倒挂**:4 个 ai/* 核心模块反向 import IPC 层 helper;AI/face 共享 Channel 下载编排寄居单一命令文件)/U-P3 exotic worker+pipeline(AI 层跨层拿纯校验器与 EmbedWorker trait)/U-P4 thumbnail_commands(QoS 自包含块下沉;去重继续归 M 线测试盲区/C1/C2);观察档(models/doc/file_ops/face_pipeline)带触发条件、豁免档(supervisor/installer/worker_client/items_cache/justified/fast_scan)带理由与重审触发器——测试撑大型与内聚状态机本轮不拆 |
| ✅ | T 线时效性核查(受托) | 数字漂移(+343 行/+13 测)由 T-D-008 机制按设计吸收,方案主体成立;**结构性前提失效仅一处**=「search.rs 是叶子」被 `push_in_predicate`(layout↔search 共享 helper)推翻;用户已采纳 T-D-012 方案 a;`list_library_formats`/`push_in_predicate` 归属 layout.rs 已按 §4.3 判据回写 T 设计;T-P0 硬门:S 代码写入已尽可启动,残余风险=S 真机验收若返修 queries.rs 按既有「插队重建基线」条款处理 |
| ✅ | 结合代码审查方案复核 | [专项审查快照](reviews/2026-07-16-后端大文件拆分方案代码审查.md)交叉 J/M/P/N/T 与当前源码:修正 P1-b 为完整 `bootstrap::setup` owner;P2 改 `ai/runtime_config.rs` + `ipc/model_download.rs`(通用 download 零 Tauri Channel);P4 复用 M 线测试债;T-D-012 方案 a 审查通过 |
| ✅ | U-D-001~006 用户 Review | 2026-07-16 用户采纳代码审查修订版;顺序定为 P1-a > P2 > P3 > P4 > 可选 P1-b,各批独立可回滚;本轮零代码改动 |
| ✅ | U-P1-a 注册清单下沉 | 96162c6:186 命令宏清单迁 `ipc/registry.rs::handler()`,集合对拍 186=186;`handler<R>()` 泛型被 11 个收具体 `AppHandle` 的命令否决(E0277),收敛具体 `Wry`(语义等价);lib.rs 1,013→832 行,注册冲突面消灭 |
| ✅ | U-P2 层次纠偏 + IPC 下载编排迁移 | d169f03(7 helper 迁 `ai/runtime_config.rs`,ai 层反向 import 归零含注释)+ e3fc686(下载编排迁 `ipc/model_download.rs`,`download/` 零 Tauri Channel)+ 63a3fb3(A15:`.part` 同步 fs 改 tokio::fs,独立行为提交);ai_commands 1,191→847 行(设计预估 ~700–750,偏差属估算) |
| ✅ | U-P3 exotic/AI 接缝重整 | fccc9f0:outcome/validate/worker_traits 三模块 + `pub use` 保旧路径,消费方零迁移;两状态机零行改动;`--list` 末级名对拍 27=27,worker_e2e 编译绿;worker.rs 1,397→710 |
| ✅ | U-P4-a QoS 下沉 | b757203:QoS 块迁 `thumbnail/qos.rs`,预算数学拆 `thumb_cpu_budget_for(logical)` 纯函数+边界测试(608/0/5);P4-b 去重不施工继续归 M 线测试盲区/C1/C2 |
