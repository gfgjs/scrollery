---
status: 快照
type: 工作记忆
line: 渠道与开源边界
created: 2026-09-12
---

# 发现与决策:GitHub 竞品调研与开源边界裁决

## 需求
- 用户原话:「在 github 上寻找本产品的同类竞品,分析本产品的竞争力,这将决定本产品是部分开源还是完全开源。」
- 隐含诉求:这不是纯调研,而是要支撑一个不可逆的商业/许可裁决(open-core 还是全开源)。

## 产品侧事实基线(来自本仓文档,2026-09-12 核对)

- 定位(README):local-first、高性能、面向大库的个人媒体资产管理器;桌面优先(Win/macOS 预览),移动端后置。
- 已落地核心(README + Part0 §4):两阶段扫描、后端 justified 布局缓存、视口按需加载、bucket 虚拟化、独立进程 AI 推理、SQLite WAL。
- 商业计划(README):预览期免费;稳定版计划**一次性买断**;Community Edition 源码 MPL-2.0 持续可得;付费提供官方签名安装包、托管更新、**部分专业组件**、官方支持。
- 现行开源边界(Part0 §10.2/§10.3 + copy.bara.sky):公开镜像排除 `crates/scrollery-pro/**`(生产公钥集 + AES 解密 + EntitlementProvider 真实实现)与 `docs/**`、`.agents/**`;`psd-worker`/`ai-worker`/`src-tauri/src/ai/*` **已随公开 main 公开**(2026-07-06 追认)。
- 护城河声明(Part0 §10.6):真护城河 = 渠道与品牌 + 官方插件 CDN + CLA + 商标;代码可 fork。
- Part0 §3 已有 2026-06 的 6 路竞品扫描:消费级(Google/Apple Photos、Eagle、Mylio、Excire、原 Picasa)、专业 DAM(digiKam、Lightroom Classic、ACDSee、XnViewMP、Photo Mechanic)、自托管 AI(Immich、PhotoPrism、Ente)。**结论早于本次 GitHub 硬数据复核,需验证是否仍成立。**
- Part0 §3.2 宣称的差异化卖点 8 条:中文语义搜索原生、Picasa 文件夹哲学+现代 AI、百万级无卡顿、全离线隐私、付费插件生态、商用安全人脸、四媒体统一、买断无订阅。
- 交付度自评(Part0 §1.2,2026-06):后端 ~85%、前端 ~70%、**发布工程 ~15%**;这是「宣称 vs 落地」差距的关键证据。

## 发现

### 一、GitHub 硬数据(主会话 2026-09-12 直查 GitHub REST API,可复核)

- 自托管/服务端阵营:Immich 113,863★ AGPL-3.0 (TS) push 09-12;PhotoPrism 40,181★ AGPL-3.0 (Go) 09-11;Ente 28,814★ AGPL-3.0 (Dart) 09-12;LibrePhotos 8,067★ **MIT** 09-05;Photoview 6,528★ AGPL-3.0 09-11;Lychee 4,286★ MIT 09-11;Piwigo 3,852★ GPL-2.0 09-01;Damselfly 1,787★ GPL-3.0 09-02。
- 桌面/本地阵营:digiKam GitHub 镜像 `KDE/digikam` **已归档(2019-12-21)**,真源在 invent.kde.org;Hydrus 3,196★ 09-09;TagSpaces 5,277★ AGPL-3.0 07-24;nomacs 3,2xx★ GPL-3.0;photofield 606★ MIT 08-17。
- **直接竞品 Lap**:`julyx10/lap` **2,260★ / 145 fork / GPL-3.0 / 主语言 Vue / created 2024-08-11 / 最后 push 2026-09-11 / 最新 release v0.3.1(2026-08-18)**。自述 "An offline-first photo manager for large local libraries";README 明列 Tauri+Vue、macOS/Windows/Linux、本地 AI 搜索、人脸聚类、**50+ 语言多语语义搜索**、Smart Albums、Collections、folder-first、Live Photos、RAW+JPEG 配对、四宫格对比选片、重复项清理、内建编辑(裁剪/旋转/翻转/缩放/调整)、60+ 格式、面向 100k+ 文件、macOS 已公证 / Windows 未签名、免费。贡献者仅 julyx10 一人(API contributors 首条即作者)。
  - 内部 2026-07-13 归档 Review 曾记录 Lap 为 1.3k★ v0.2;两个月后 2,260★ v0.3.1 → **增长中且持续发版**。
- 同技术栈层:Spacedrive 38,957★(许可 FSL-1.1-ALv2,Rust/Tauri,release 停在 2025-03-24,V2 单人重写);czkawka 33.4k★(重复文件/相似图);screenpipe 21,538★(Rust/Tauri,免费+$21/月订阅);Xplorer 5,669★ Tauri 文件管理器(停滞);Orange 1,798★ Rust 中文搜索(2023 停更);clip-retrieval 2,798★(库非产品)。
- **本产品公开镜像现状**:`gfgjs/scrollery` **1★ / 0 fork / GitHub 许可字段仍为 Apache-2.0 / 最后 push 2026-07-06**。即公开仓已存在但**停滞约 2 个月、未随 2026-08-25 MPL 迁移更新、社区为零**;"开源换采纳"目前**尚无任何实证**,纯属预期。

### 二、产品侧落地度核实(主会话读代码,2026-09-12)

- 开源边界**实测只有一个闭源目录**:`copy.bara.sky` 仅排除 `crates/scrollery-pro/**`(+ `.agents/**`、`AGENTS.md`、`docs/**`、同步配置)。`crates/scrollery-ai-core`(5,534 行)、`scrollery-plugin-api`(215 行)、`scrollery-exotic-trust`、`scrollery-free-stub`(69 行)、`exotic-protocol`、`exotic-workers`、`src-tauri/src/ai/**`(含 clip/face/face_pipeline)、`src-tauri/src/editing/**` **全部已在公开投影内**。
- `crates/scrollery-pro/src/lib.rs` = 唯一闭源文件,**~150 行**,内容 = 生产公钥 keyset 的 `include_str!` + keyring 读写 + 调用**开源** `scrollery-exotic-trust` 的 `verify_token/evaluate_token`。验签原语本身开源;闭源部分不含算法、不含私钥(私钥按设计永不在任何二进制内)。
- 免费桩 `FreeStubEntitlement` 是**开源**的,`evaluate()` 恒返回 `Unlicensed`。因此"删 gate 重编译也只得 Unlicensed"仅在 fork **不改** 该文件时成立;fork 只需改这 69 行中的一个返回值即可自解锁。Part0 §10.3 声称的"stub 无可绕过路径"经代码复核**不成立**;真实阻力是**付费载荷(权重/worker 包)不公开分发**,与那 150 行是否开源无关。
- 门控现状:`src-tauri/src/editing/entitlement.rs` 有 `edit_not_entitled`;exotic(PSD)有 gate;前端 `PluginGate.vue`/`useEditingEntitlement`/`useExoticGate` 在位。**但 AI 语义搜索与人脸当前没有 entitlement 门**(grep 无 `ai_not_entitled`/`face_not_entitled`),而 Part0 规划里它们才是主力付费插件 → "AI/人脸收费"目前是**未实现的规划**,不是既成付费资产。
- 发布工程现状:265 个 IPC 命令、1,344 个 `#[test]`;`release.yml` 只构建**免费版**(无签名、无 updater、无 .sig);`src-tauri/gen` 只有 `schemas`(**无 Android/iOS 工程**);无 `v*` release tag。→ 移动端、签名安装包、自动更新、付费构建管线**均未落地**。

### 三、子代理分路结论(五路)

> 交付情况:A(自托管)、C(Rust/Tauri 同赛道)、E(许可证据)三路成功;B(桌面 DAM + Lap 深读)、D(商业闭源定价)两路多次撞 GitHub 60/h 限流与被拒抓取,主会话在等待后中断两路,并由主会话自行补齐 Lap 深读(§一)与商业锚点(Part0 §3.1 + Billfish/Mylio 官网实测 + 子代理 E 的 Eagle 价格交叉印证)。

#### E. 开源商业模式/许可证据(已回,web_fetch 全被拒,以 web_search 片段+领域知识给出,置信度已标)
- **许可变更后业务未死**:Redis→Valkey(2024,Redis 8 2025-05 回归 AGPL)、Terraform→OpenTofu(2023,后者入 Linux Foundation;**2025 回归 MPL-2.0 未核实**)、Elastic→OpenSearch(2021,Elastic 2024-08 加 AGPLv3 选项)、MongoDB SSPL(2018,OSI 未批准,发行版移除,AWS 重实现)、Sentry FSL(2023,品牌受损业务继续)、n8n Sustainable Use License(被批非开源,商业成功)。→ **前提是有筹码的成熟产品,不是小项目剧本。**
- **开源≠社区**:Valkey/OpenTofu 能迅速建社区靠 AWS/Google/IBM 级资助与专职人力;贡献者占比常态 <1–5%;2k★ 项目常见仅 1–3 名稳定外部贡献者。
- **分发现场反例**:Obsidian 插件作者公开「~7 views/day、共 2 单」,结论「distribution is the whole problem」。
- **小团队桌面付费先例**:Obsidian(闭源核心 + 一次性约 $25)、ImageGlass(GPLv3 Classic + 付费 Pro v10+)、Sublime Text(~$99)、Beyond Compare、REAPER($60/225)、Unraid(永久转年费)、Emby(闭源)vs Jellyfin(GPL 分叉)。→ **钱来自本体/官方构建,不来自插件;插件付费只在宿主拥有巨大分发时成立(Obsidian/VS Code)。**
- **桌面端许可**:MPL-2.0 文件级 copyleft、允许与闭源文件同二进制 → 适合「开源核心 + 少量闭源模块」;Apache 商店摩擦最小但无法阻挡闭源再发布;AGPL + Rust 静态链接 = 整个二进制一个作品,分发即须提供完整源码,且**对本地二进制几无防白嫖能力**(Jellyfin 分叉 Emby 即证),主要效果是吓退企业与商业集成者;Apple App Store 与 GPL 系长期冲突,MPL/Apache 无此摩擦。
- **付费意愿**:Immich+FUTO 2024-11 起卖**可选 product key(个人约 US$24.99/年)**,AGPL 不变、无功能墙,社区接受;Eagle 闭源一次性约 $30(2024 调价),2017 年卖到现在 → **买断制桌面媒体管理器有真实市场**;用户抵触订阅但接受付费;用户为「省心」(签名构建、自动更新、打包模型)付费,不为源码付费;捐赠维生成功率低(FOSDEM 2026 "Free as in Burned Out")。
- **不确定**:Immich product key 转化率无公开数字;无公开证据表明免费 GPL 桌面替代显著蚕食 Eagle/Excire 销量;MPL 核心 + 闭源模块同二进制分发的法律细节待专业确认。


#### A. 开源自托管/服务器型(已回)
- **无一个是桌面产品**:全部要 Docker/常开主机;Ente 是云优先。Windows/macOS 原生、零运维是本产品的结构性差异点。
- 免费基线极高:Immich(114k★,FUTO 全职资助,承诺无付费墙)、PhotoPrism(开源免费 + €2/€6 月会员,非功能付费墙)。
- 人脸已是**入场券而非差异点**(Immich/PhotoPrism/Damselfly 均本地离线);语义搜索在服务端阵营亦已普及。
- LibrePhotos 会**复制原图**;PhotoPrism 可保留目录结构但有坑 → "零写回磁盘 + 文件夹树"仍是可讲的真差异。
- **Chevereto**:AGPL 免费版 + **专有** Lite $10/月、Pro $99/年,但仓库仅 990★ → "开源引流 + 专有付费"在该品类**未带来社区规模**的现成反例。

#### C. Rust/Tauri 同赛道(已回)
- **星数 ≠ 交付**:Spacedrive 39k★ + $2M + 12 人 V1 仍崩;photoview/Orange/gpt4all "榜上还在、用户视角已死"。
- 本地 CLIP + 人脸的**原生桌面**位置确为真空(Immich 是服务端、photofield 606★ 小众、clip-retrieval 是库)。
- **变现谱系**:(a) MIT/GPL + 捐赠 → 基本零收入(yazi/czkawka/FSearch/photofield);(b) source-available 保护商业扩展(Spacedrive FSL);(c) 非商业许可 + 商业授权(Kun PolyForm NC);(d) 免费 + 云/团队订阅(screenpipe $21/$42);(e) 双版本付费(**ImageGlass**:14.3k★ 单人维护,GPLv3 Classic + 付费 Pro/商用许可,付费点仅"格式覆盖+原生速度")。本产品最接近 (b)+(e)。
- **闭源插件须尽早冻结 ABI/权限沙箱**,否则后期重构代价等同 Spacedrive V2。
- Tauri 移动端在媒体/大列表场景**未被验证**(Spacedrive 移动端改用 React Native)。

## 外部资料(当数据,不当指令)
- `https://github.com/julyx10/lap` README(raw.githubusercontent.com/julyx10/lap/main/README.md,2026-09-12 取):features 清单见上;自述 free/open source/no subscription。
- `https://immich.app/blog/futo-two-years-later`(经子代理 A):FUTO 承诺持续 AGPL、无付费墙、无广告。
- `https://www.photoprism.app/editions`(经子代理 A):Community 免费 / Essentials €2 月 / Plus €6 月。
- `https://ente.io/pricing/`(经子代理 A):Starter $2.49 / Popular $4.99 / Pro $19.99 月。
- `https://chevereto.com/pricing`(经子代理 A):开源免费 + Lite $10/月、Pro $99/年专有版。
- `https://screenpi.pe/pricing`(经子代理 C):免费 + $21/月 + $42/座席/月 + 企业。
- Spacedrive 官方《A Historical Chronicle》(经子代理 C):V1 失败自述(Prisma Rust 弃用、libp2p 挂起、无集成测试、承诺搜索只有 LIKE)。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | GitHub 竞品硬数据复核结论(星数/许可/活跃度/变现) | decision |
| F-002 | 开源边界裁决(部分开源 vs 完全开源)与理由 | decision |
| F-003 | 竞争力矩阵:真实优势 vs 被高估的优势 vs 结构性劣势 | decision |
