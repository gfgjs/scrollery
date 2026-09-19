---
status: 快照
type: working-memory
line: RAW等格式照片支持
created: 2026-07-24
---

# 任务计划:RAW 等格式照片支持

## 目标
参考 PSD 子系统插件(exotic 插件架构),为 Scrollery 增加 RAW 等相机原始格式照片的支持(缩略图/预览派生 → 画廊+查看器可见),跨 Windows/macOS/iOS/Android。**当前范围=摸底+出方案+建三件套;施工待用户批。**

## 当前阶段
阶段 A/B/C 全部完成(C-2b-prod 落地,commit fb5062f+c954865);剩真机/CI tail(见「剩余工作」)。

## 方案
一句话路线:exotic **builtin distribution**(照 exotic-ocr)+ 桌面 sidecar worker + LibRaw(CDDL)嵌入预览提取,移动端二期 in-process;分期 A-E 见下。

### 分期(DAG:A 独立即开 ‖ B 可并 A → C 依赖 B → D 依赖 C;E 二期)
- **阶段 A(P0)过渡体验修复**:网格 RAW badge + 占位态、查看器友好文案。独立、现在先做。
- **阶段 B(P1)raw-probe 探针**:新建 raw-probe crate + LibRaw 绑定 + 各厂商实测 + report + 桌面三平台交叉编译。可与 A 并行。
- **阶段 C(P2)raw-worker + 宿主接线**:复刻 psd-worker,decode.rs 用 LibRaw 嵌入预览;catalog.json builtin offering / coordinator plugin_descriptors / 常量,**不含**根 Cargo.toml 默认 member(见下「计划修正」);另需 catalog.rs builtin 叠加豁免 + mod.rs 授权 free 路径 + fast_scan.rs 播种谓词,接入点比原估多。依赖 B 落地。
- **阶段 D(P3 可选)大图预览**:`Capability::Preview` + Host 解码转发路径(PSD 一并受益)。依赖 C。
- **阶段 E(二期)移动端 in-process**。

## 阶段
<!-- 阶段翻 complete 时当场折叠为一行 -->

### 阶段 0(已完成):摸底 + 调研
完成:4 路摸底全落盘(recon-A1/A2/A3 + research-R1)——核心结论=RAW 十扩展名已注册但零解码器(真实用户可复现问题),PSD 骨架高度可复刻,两核心张力(sidecar 移动端不通/LGPL 静态链接合规未裁)促成 builtin+LibRaw CDDL 定案。

### 阶段 A(P0):过渡体验修复(commit e28c616,网格 RAW badge/占位+查看器文案,复核无发现)——**complete**

### 阶段 B(P1):raw-probe 探针(commit 45b66b0,rsraw=0.1.1 vendored LibRaw 在 x86_64-pc-windows-gnu 工具链 `cargo build` 绿,绕开 rsraw MSVC 硬 panic;vendor 真实解码实测 gated on `RAW_PROBE_SAMPLES` 未跑,mac/Linux 交叉编译未验)——**complete**

### 阶段 C-1(P2):架构 A + raw-worker 骨架(commit 6568e27,`cargo test --lib` 965 passed/0 failed,opus 深审两轮全过)——**complete**

### 阶段 C-2(P2):修挡死 + worker 交付
- C-2a(授权门 + 播种守卫,commit 4ad6341)——**complete**
- C-2b-dev(`resolve_worker_path` builtin 分支,dev 按 plugin_id 分发 env 路径,commit 6ea5b52)——**complete**
- C-2b-prod(tauri externalBin 跨工具链 sidecar 打包进发布包)——**complete**(commit fb5062f+c954865,cargo check debug+release 绿 / installer 单测 8 passed / `tauri build --debug` 出 MSI+NSIS 且 raw-worker.exe 复制到 app exe 同目录 / opus 深审 1P1 已修+增量核验过)
- **状态:** C-2 全部完成,prod 打包链闭合

## 剩余工作(真机/CI tail)
1. **CI**:新增 gnu job 编译 raw-worker(纳入 ci.yml 门禁,`--target x86_64-pc-windows-gnu` + mingw)。
2. **release 实跑取证**:release.yml 已埋 rustup target add + dlltool/g++ fail-fast 探测,下次 release 走一遍即证(runner 前提)。
3. **本机长期跑 `build:raw-worker`** 需将 WinLibs mingw64/bin 加入系统 PATH(实测不在默认 PATH,验证时临时前置)。
4. **staging 脚本 fail-soft 失败分支**仅静态 + `node --check` 证实,未运行时跑通(旧产物遮蔽)。
5. **样张厂商实测**:真实 RAW(CR2/CR3/NEF/ARW/DNG/ORF/RAF/RW2/PEF/SRW)经 `RAW_PROBE_SAMPLES` 跑 raw-probe + dev GUI 看缩略图质量。
6. **mac/Linux 交叉编译**(仓无 mac/iOS/Android CI,已知缺口非本线新增)。
7. **GUI 真机验收**:设 `EXOTIC_RAW_WORKER_PATH`,dev 跑真 RAW 库看缩略图/查看器。
8. 阶段 D(大图预览 `Capability::Preview`)/E(移动端 in-process)仍二期。

## 计划修正(本会话施工中证伪原假设,body 已改正非仅脚注)
- 原「raw-worker 加进根 Cargo.toml members」**错**——raw-worker/raw-probe 用 rsraw,MSVC 硬 panic,必须空 `[workspace]` 脱离父 workspace、gnu-only 单独编,**绝不作默认 member**(否则默认 msvc `cargo check --workspace` 破门)。
- 原「exotic-ocr builtin 先例证明 RAW 格式可与 common 共存」**证伪**——OCR 声明的是能力标记 `formats:["ocr"]`(非真扩展名),不是「声明已 builtin 的真扩展名」的先例;RAW 是全仓首例,触发 CommonFormatConflict,靠架构 A 定向豁免解决。
- 「宿主 5 接入点」修正:coordinator 需加 builtin 叠加豁免(catalog.rs)+ 授权 free 路径(mod.rs)+ 播种谓词(fast_scan.rs),比原估多;builtin 帧协议 worker 的交付是 novel(PSD 走安装验签、ai-worker 走直启 spawn,均非先例),C-2 待摸清。

### 阶段 D(P3,可选):大图预览
- [ ] `exotic::catalog::Capability::Preview` + Host 解码转发路径(PSD 一并受益)
- **状态:** pending(阻塞于 C)

### 阶段 E(二期):移动端 in-process
- [ ] 待一期(桌面)验证后评估 in-process 纯 Rust 方案
- **状态:** pending(二期,阻塞于一期整体验证)

## 已裁决决策
| 决策 | 定案 | 依据 |
|------|------|------|
| 分发模型 | exotic **builtin distribution**(照 exotic-ocr),license gate 可选、一期默认关(免费可用,基础设施保留) | recon-A2 §9 discrepancy #5:`exotic-ocr` builtin 先例已验证与安装态门分叉共存;用户裁定 |
| 平台/进程 | 桌面先行(Win/mac/Linux)+ exotic sidecar worker;移动端二期 in-process | research-R1 §"sidecar vs in-process 取舍":iOS/Android 沙盒禁子进程,sidecar 桌面专属;用户裁定 |
| 库 | LibRaw(CDDL),sidecar worker 内静态链接;规避 LGPL 未裁风险 | research-R1 §8:LibRaw CDDL 分支有官方书面确认闭源静态链接可行,LGPL 系(rawler/rawloader/quickraw)静态链接合规性未裁;用户裁定 |
| 解码深度 | 嵌入预览提取(毫秒级),完整 demosaic 二期 | research-R1 §3/§7:预览提取比完整 demosaic 快 2-3 个数量级,Photo Mechanic 行业先例验证可行;用户裁定 |
| 过渡修复 | 独立 P0,现在先做 | recon-A3 顺手发现 #3:RAW 半成品已是真实用户可复现问题,不等主线施工;用户裁定 |

## 关键决策(D-ID 续全仓最大号 D-426)
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| RAW 分发模型 = exotic builtin distribution | 照 exotic-ocr 先例,免安装包即可用+基础设施保留,license gate 一期默认关不阻塞体验 | D-427 |
| 平台/进程 = 桌面先行 sidecar + 移动端二期 in-process | sidecar 在 iOS/Android 沙盒天然不可行,拆分避免阻塞桌面交付 | D-428 |
| 库 = LibRaw(CDDL),sidecar 内静态链接 | 唯一有官方书面确认闭源静态链接合规的 RAW 解码库,规避 LGPL 系未裁风险 | D-429 |
| 解码深度 = 嵌入预览提取,完整 demosaic 二期 | 性能数量级优势(ms vs s)+ 行业先例(Photo Mechanic)验证可行,匹配"高性能资产管理器"定位 | D-430 |
| 过渡修复独立 P0 立即开工 | RAW 半成品已影响真实用户库(缩略图永久空白),不应等主线施工排期 | D-431 |
| 库/工具链 = rsraw=0.1.1 + x86_64-pc-windows-gnu 单独编 worker | 唯一在本机可编的 vendored LibRaw 路径,gnu 绕开 rsraw MSVC panic,worker 独立 sidecar 不与 host 共享 ABI 故可行 | D-432 |
| raw-worker/raw-probe gnu-only 脱离父 workspace,不作根 member | 避免默认 msvc build 撞 rsraw MSVC panic | D-433 |
| P0 冲突解法 = 架构 A builtin 叠加豁免 | 用户裁决;catalog.rs 仅 `distribution==builtin` 跳过 CommonFormatConflict,格式仍 builtin 识别 + offering 叠加缩略图能力;保留 Stage-A 韧性;备选 B(摘除 builtin)/C(能力标记映射)均落选 | D-434 |
| RAW license = free 无 sku,builtin 免费(承 D-427) | 需 availability_of 显式 free 判据(`license_tier=="free"` → Authorized,fail-closed),非「无 sku → Authorized」 | D-435 |
| raw-worker prod 交付 = tauri externalBin + staging 按打包 host triple 落名 | gnu 产物进 msvc 包,Tauri 按 triple 匹配文件名与内容工具链无关;prod 选路 current_exe 同目录,镜像 ai-worker 先例 | D-444 |
| staging 失败语义分级 | CI 硬失败(绝不容忍陈旧 sidecar 进发布物);非 CI 有旧产物 warn+跳过(保 dev 内环);非 CI 无产物硬失败附首配提示 | D-445 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
