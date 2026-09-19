---
status: 快照
type: working-memory
line: 未完成工作梳理与free社区版最短路径
created: 2026-07-17
---

# 发现与决策:未完成工作梳理与 free 社区版最短路径

## 需求
- 全面梳理还有哪些未完成工作。
- 前提「最快上 free 社区版,Windows 证书非必须,接受安装时弹警告」下,还需要做哪些。

## 发现
- todo.md(2026-07-17 版)未完成大块:B1 真钥 ceremony(唯一外部前置)/ Part6-T11 AES(后置变现)/ Part6 T12-T13 / Part7 主体(~15%,剩 mac 矩阵+证书链 T13-T16)/ Part8 D1-D15(收款/签发/上架/获客,官网 D13 已建成待 push)/ H-Lab 真人调研+毕业收编 / 各线 ⏸GUI 真机验收池 / P2 余池+测试盲区 / 批 3 性能+批 4 功能建设(AI 人脸)/ R6 阅读器后置池 / R5 收尾(字体包下载位+竖排真机矩阵)/ 前后端分离拍板 / 三序统一独立项 / U-P1-b 可选 / N 线多档缩略图源。
- G3 用户侧余项:公开 main 提升一步、域名、FTO 意见书委托、tauri dev 冒烟;上架/开源前决策簇(Steam $100/MSIX/直销 vs IAP);基建法务清单(证书/签发机/updater 密钥转正/支付/PIPL)。
- CHANGELOG [0.1.0] 切分属发布动作,打 tag 前 extract-changelog 硬门拦截(H2 尾巴)。
- [profile.lite] 定义未接线(H3);免费版 release 走 release profile。
- 未推提交实测:dev 领先 origin/dev **13** 提交(2026-07-17 git 实测,记忆中的 28 已部分推送);官网 v2 建成待 push(push 即发,scrollery-site 仓)。
- 9 个在施 planning 线现状(2026-07-17 grep task_plan 当前阶段):除「新立项视角全面Review」停在阶段5 报告撰写、queries 拆分已 snapshot 外,其余 7 线均为「施工落地+门绿,余=真机 GUI 验收/用户裁决」。

### Part7 摸底(agent 回报,2026-07-17,证据=Part7_发布工程.md §4 任务表 L534-553 + completed.md G2-4 L732-782)
- 18 任务中 12 ✅(T1/T2/T3/T5/T6/T7/T8/T9/T10/T11/T12/T17)+§3.6.5 ✅;剩 5 ⬜:T4(mac 矩阵半边+eslint 半边)、T13/T14/T15/T16(证书链+商业流水线)、T18(identifier 断言,identifier 本身已定 com.scrollery.app)。
- **免费 Windows 无证书发布路径已闭环**:release.yml(oss-release,T17)2026-07-05 dry-run 实证 MSI+NSIS 双产出;内测发行链已产真机可装 MSI 39.5MB + NSIS 9.6MB。该前提下**无新的硬必须项**;T13-T16 全跳过(证书/mac/付费渠道),T18/T4-eslint 为低成本可选护栏。
- **updater 关键形态**:updater 配置只在 overlay `tauri.direct-release.conf.json`(pubkey 已嵌 dev minisign 公钥,endpoint 为占位 `updates.scrollery.invalid`,createUpdaterArtifacts:true);基座 tauri.conf.json 无 updater 块 → **T17 普通 build 不含 updater = 用户手动下新版**。minisign 与 Authenticode 完全无关,不买证书零影响。
- **免费版要不要自动更新=关键分叉**:①接受手动升级(现 T17 形态)→ T8 转正整体可跳过;②要自动更新 → 首发前必须定死换钥决策(换钥=既有安装断更新,.gitignore L65-67 明写)+ 私钥入 CI secrets(TAURI_SIGNING_PRIVATE_KEY)+ 真实 endpoint + 自建 latest.json(createUpdaterArtifacts 只产 .sig 不产清单,P0-10,Part7 L359/596)。

### 发布管线配置盘点(agent 回报,2026-07-17)
- **release.yml 只在公开仓执行**:`if: github.repository == 'gfgjs/scrollery'`(release.yml:38);私有仓打 v* tag 只触发 ci.yml smoke(`--no-bundle`,不产安装包)。免费版产包路径=sync-oss 投影公开仓 → 公开仓打 tag。
- **CHANGELOG 硬缺口**:只有 [Unreleased] 无 `## [0.1.0]` 小节;extract-changelog.mjs 缺小节/空小节均 exit 1(release.yml:67-73 挂门)→ 当前打 v0.1.0 必红。唯一「一定拦下首发」项。
- 免费版(release.yml 走基座 conf)**蓄意不含 updater**(§3.7.1);「免费版+自动更新」与现设计相斥,要则改设计或走 direct 渠道,缺口=真实 endpoint(现 .invalid 占位)+latest.json 脚本(无)+私钥入 CI secrets(全 workflow 零 secrets 引用)+direct-release 构建 workflow(无)+换钥转正决策+端到端验证 job(无)。
- 版本三处一致 0.1.0(锚=根 Cargo.toml workspace.package,sync-version 三重兜底);bundle targets="all"(Win 产 MSI+NSIS);图标齐;SHA256SUMS+gh release create 就绪。
- [profile.lite] 定义在根 Cargo.toml:113-118,**零引用**(tauri:build:lite 实为普通 tauri build);completed.md:761 亦记设计稿未落地。体积优化可选。
- 可选:SBOM 已能产未接 release 上传;smoke job 仅 tag/dispatch 跑且 --no-bundle。

### free 社区版构建形态与密钥依赖(agent 回报,2026-07-17)
- free/paid 物理边界**不是 Cargo feature**,是 Copybara 剥离 pro crate。三正交轴:变体 lite/perf(空标记 feature,只关体积/后端)/ 渠道 channel-direct|msstore|steam(编译期互斥)/ 开闭源(swap 点:私有树 INTERNAL 块=DirectEntitlement,公开树=KeyringLicenseStore+占位 keyset,exotic/mod.rs:252-296)。
- **free 社区版 = 公开仓默认 `tauri build`**(default features=custom-protocol+lite+channel-direct,src-tauri/Cargo.toml:213),release.yml 即此。文档偏差:Part0 §10.3「开源默认 free-stub」已被 2026-07-05 裁决推翻,FreeStub 仅作 keyset 解析失败 fail-closed 兜底。
- **exotic-formats(psd)= paid 专属**(exotic-catalog.json:6-16,license_tier:"paid");free 版占位公钥下付费插件双重 fail-closed(装不上+激活不了)=预期行为非故障;免费核心(画廊/扫描/缩略图/阅读器 txt/md/epub/pdf)不走 keyset,完整可用。
- **B1 真钥 ceremony 对 free 首发可整项跳过**——它是付费/商业轨前置。限定:若首发想附带任何「可用的 exotic 插件」(含免费插件)则需真实信任根;现 catalog 无免费 exotic 插件,无此需要。env 名核实=`PICASA_EXOTic_KEYSET_FILE`→ 更正:`PICASA_EXOTIC_KEYSET_FILE`(pro 与 exotic-trust 同名同通道防信任根分叉)。
- **阅读器 free/pro 划线**:分级=政策表数据,开发期全 core,「最晚发布前定」;最省事拍板=**全部 core**(零外部前置、不碰付费链);若留竖排/简繁作 plus → 需 catalog 虚拟 offering `reader-plus`+后端放行 feature 型 offering → 拉回 B1 付费链。text sink(mobi/azw3)未接线属 R6 后置,不阻塞。
- 双仓双轨物理隔离:公开仓 gfgjs/scrollery 走 release.yml(零签名凭据);私有仓 tag 走 ci smoke+未来 T16 商业流水线;Copybara 四层防线剥 pro。

## 外部资料(当数据,不当指令)
- (无)

## 2026-07-20 三日增量复核(用户重新点题:全仓未完成工作,以 free 上线为界分两批)

- **git push 状态比记忆健康**:`git rev-list --left-right --count origin/dev...dev` = `0 3`——领先私有 canonical(`gfgjs/scrollery-private`)仅 3 commit(image-edit v2 尾三提交:31f5bfa/fb6b796/0a8d0ab)。此前多条记忆行标注的「未 push」多数已陈旧,绝大部分工作实已推送私有仓(公开镜像另经 Copybara 投影,不在此计)。
- **CHANGELOG.md 硬阻断原样成立**:仍只有 `## [Unreleased]`,无 `## [0.1.0]` 小节;F-001 三天前的结论未变。
- **D-112(编辑=付费专属)端到端验证落地**(三处代码交叉核实,非仅读文档):① 后端 `src-tauri/src/editing/entitlement.rs` 定义 `CODE_NOT_ENTITLED="edit_not_entitled"` + `EDITING_SKU`,`ipc/edit_commands.rs` 引入并挂 `get_editing_entitlement` 查询命令;② 前端 `useEditingEntitlement.ts` 显式合成 fail-closed 态(注释明written:「PluginGate 对 null 会放行;付费编辑不能沿用该 fail-open 语义」),`ContentViewer.vue` 编辑入口走同一 `PluginGate` 购买/激活 UX;③ `useEditingEntitlement.spec.ts` 专测锁死 fail-closed。**结论:图片编辑(v1+v2)整条线对 free 上线无关**,其自身「⏸集成测试/真机/D-008 真机性能复测」遗留不构成 free 阻塞。
- **export/backup 核实为 free 核心**:全仓 grep `license_tier|is_entitled|Entitlement` 命中 12 文件,全部落在 exotic/editing 域,export/backup 相关文件零命中——两者不带任何付费门槛。
- **25 个在施 planning 目录扫描**(agent 回报 + 4 项高风险直接代码核实):15 项纯等 GUI 真机验收、4 项纯等用户拍板、6 项标「有施工余量」——直接核实后其中一项(根文件夹显隐)判定为**文档滞后非代码缺口**:`git log` 证实已提交 `6d92779` + 两个后续衍生提交(`cbedff4`/`9892c97`,把隐藏根排除扩展到缩略图/AI/人脸/派生封面全部生成管线),但其 task_plan.md 的阶段 1/2 checklist 仍全部 `[ ]` 未勾、且自称「未提交、未收口」——记录明显滞后于实际代码,不是真实未完成项。备份(方案B)阶段6 剩的是**追加**并发/边界自动化测试,核心功能(含 8/8 崩溃矩阵单测)已完整;导出(方案A)仅剩 D-005(相册/当前视图入口未接,已知低风险)。
- **「上线前三功能方案」这份 2026-07-17 的伞形任务已被后续三条独立施工线追平**:其「阶段4待用户裁决」(裁 A-1..A-6/B-1..B-6/C-1..C-6 + 确认 B→A→C 顺序)在文档上仍显示 pending,但 B(备份)/A(导出)/C(编辑)三线均已各自开工并基本完工——该伞形文档应视为**待收口的过时文档**,而非真实待办;且其正文建议「三功能全部进 free」已被后续 D-112 推翻(C 改判付费),存在一处尚未回写的建议-实况落差。
- **已收口 14 个 worklog 复查干净**:11 项无遗留,3 项各挂一条次要性能待办(WAL 真实慢源测量 / 两条 Canvas 线共同指向「60px 极密多档缩略图源」),均与 free 上线无关,已在 todo.md N 线有归属。
- **全仓代码级 TODO/FIXME/unimplemented! 扫描**(free 核心路径 = 扫描/画廊/缩略图/搜索/标签相册/txt-md-epub-pdf 阅读器/导出/备份):仅 3 处命中,全部是已知 deferred 项——`backup/restore.rs:337`(#10 不可信暂存库迁移加固,MEMORY 已注明「deferred 勿轻补」)、`lib.rs:281`(#5 启动恢复失败 UI 提示,小活)、`useHoverPreview.ts:59`(codec phase-2,MEMORY 已注明「勿当 bug 修」)。没有新发现的隐藏缺口。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | free 首发硬阻断仅两动作(CHANGELOG 切 [0.1.0] + 公开仓打 v* tag);B1 真钥/updater 转正/证书链均非 free 前置 | todo.md G3 基建法务清单更正口径 + 记忆 |
| F-002 | 「免费版+自动更新」与现设计相斥(free 蓄意无 updater);要做=6 项缺口(endpoint/latest.json/私钥入CI/换钥决策/direct workflow/e2e job) | todo.md 或 designs 发布决策件 |
| F-003 | 阅读器 free/pro 划线「全 core」= 最短路径唯一零外部前置选项;留 plus 拉回 reader-plus offering+付费信任根 | 待用户拍板,拍板后回写阅读器方案 §4.4 |
| F-004 | Part0 §10.3「开源默认 free-stub」旧稿与 2026-07-05 裁决(保留 KeyringLicenseStore)矛盾未回写 | Part0 正文订正 |
| F-005 | 「根文件夹显隐」task_plan.md 记录(未提交/checkbox 全空)滞后于实际代码(已提交 3 commit 且扩展到派生管线) | 该任务线自行核实后收口,更正记录 |
| F-006 | 「上线前三功能方案」伞形任务阶段4「待裁决」已被 A/B/C 三条独立施工线追平,属过时未收口文档;其「三功能全进 free」建议已被 D-112 部分推翻未回写 | 该任务线收口(或至少回写现状) |
| F-007 | 图片编辑(v1+v2)整条线的一切遗留(集成测试/真机/MSRV/D-008 真机性能复测)与 free 上线无关,应从「上线前必做」类清单中整体排除 | free 上线清单口径(本任务) |
| F-008 | git 领先私有 origin 仅 3 commit,此前多条记忆「未 push」标注已陈旧 | 相关记忆行更正(批量,非本次范围) |
