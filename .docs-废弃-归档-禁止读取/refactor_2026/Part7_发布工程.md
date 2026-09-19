---
id: 2026-06-27-Part7_发布工程
status: active
type: canon
line: refactor_2026
created: 2026-06-27
---

# Part 7 · 发布工程（Release Engineering）

> 系列定位：把「能在开发机跑起来的代码」变成「能门控质量、能签名、能热更、能直销分发」的可发布产品。
> 依赖：**Part 6**（workspace 与授权实现归一、`EntitlementProvider` 抽象、公开面门禁已建立）。
> 被依赖：**Part 8**（商业化分发在本 Part 的 CI 门控、签名/公证与发布流水线基础上做真实上架与官网分发）。
> 取证基线：本 Part 全部论断以 `文件:行号` / Part 节号一手核验为准；旧 plan-docs 与记忆仅作线索，不予轻信。

---

## §1 目标与范围

### 1.1 一句话目标

现状「后端 ~85% / 前端 ~70% / **发布工程 ~15%**」（Part0 §51）。本 Part 把发布工程从 15% 补到可交付：**质量门控闭环（162 测试进 PR 门）→ 统一版本与瘦身打包 → 平台代码签名/公证 → 热更新通道**。三条交付阻塞中本 Part 直接解两条——②「CI 无 `cargo test`」、③「`tauri-plugin-updater` 未引入」（Part0 §53；①epub 登记**未落地**——format.rs:63 扩展名列表仍无 `epub`，归 **Part3 / Part0 Wave0 P0**，非本 Part 闭合）。

### 1.2 范围内（Part7 认领）

| 编号 | 主题 | 一句话 |
|---|---|---|
| R1 | **CI 测试门控** | `cargo test` 进门控（162 测试对 PR 不再零拦截）；dev 分支纳入 CI 触发；前端 typecheck 维持 |
| R2 | **workspace 之上的 CI 适配** | Part6 建好 workspace 后，CI 改为 workspace 级 `cargo test --workspace`；oss/commercial 双 pipeline 落地为真实 `.yml`（断言定义归 Part6，**实现归 Part7**） |
| R3 | **版本号单一事实源** | `tauri.conf.json` / `Cargo.toml` / `package.json` 三处 version 收敛到一处驱动；上架前锁定 `identifier` |
| R4 | **打包瘦身（去 ort 善后）** | 删除 `bundle.resources` 的 4 个 ONNX DLL + `package.json` 的 `onnxruntime-node` + `Cargo.toml` 的 `ort` 依赖；验证 <10MB 核心 |
| R5 | **平台代码签名 / 公证** | Windows Authenticode（EV 优先）、Apple `codesign`+`notarytool`；**覆盖所有 sidecar worker 二进制**；凭据仅私有 CI |
| R6 | **热更新通道** | 引入 `tauri-plugin-updater` + 更新签名密钥对 + `createUpdaterArtifacts` + 更新源清单；与 exotic Registry 各司其职 |
| R7 | ~~多渠道构建变体~~ **已退役（P16，2026-09-15）** | 三渠道 cargo feature 与 CI 矩阵随未实现商店渠道整体退役。当前**唯一渠道 = 直销 `direct`**：`tauri-plugin-updater` 是普通依赖，按最终配置是否带 `plugins.updater` 决定注册，无 channel feature / `#[cfg]` 门控 |
| R8 | **发布产物与 Release 流水线** | tag(`v*`) 触发 → 构建 → 签名 → 公证 → 生成 updater 清单 → GitHub Release（free）/ 私有 CDN（付费 catalog） |

### 1.3 范围外（划给其他 Part，避免重复/留空）

- **workspace 建立本身 + 授权实现归一（`KeyringLicenseStore` 唯一 direct 实现 + `FreeStubEntitlement` fail-closed 回退 + 删 pro/free-stub 双 crate）**：归 **Part6 §3.9 / T10**。Part7 只**消费**该 workspace 并把公开面门禁**实现进** `.yml`。
- ~~`MsStoreProvider` / `SteamProvider` 真实实现 + 商店提交 + `winapp pack` / SteamPipe 脚本~~：三渠道（含 Provider 桩）已于 2026-09-15 整体退役（P16）。当前只保证唯一 `direct` 渠道可构建，无渠道变体。
- **最终改名执行**（repo/Org/域名/`identifier`/模块前缀的一次性替换）：归 **Part0/Part8 首次上架前**（Part0 §475）。Part7 只**锁定 `identifier` 不再变更**这一约束并在 CI 校验。
- **license 签发服务 / 定价 / 落地页 / 获客**：归 **Part8**。

### 1.4 红线（开源边界 × 发布安全，贯穿全 Part7）

1. 🔴 **签名凭据绝不入 public Actions**：Win EV 证书、Apple 证书/`notarytool` 凭据、updater 私钥、AES `enc_seed`、签发私钥——**仅私有 CI（secrets 隔离）**；public Actions 的构建无任何签名步骤（Part0 §10）。**验签公钥可公开**（非秘密）。
2. 🔴 **公开树与私有树同构**：两侧使用同一份 manifest 与依赖图，公开门禁 = 原锁 `--locked` 构建与测试 + gitleaks 密钥扫描；不再有「某 crate 不得入依赖树」式断言（Part6 T10）。
3. 🔴 **平台签名 ≠ exotic 验签**：本 Part 的 Authenticode/notarization 是 OS 级可执行签名（过 SmartScreen/Gatekeeper）；exotic 插件包的 Ed25519 验签（[installer.rs](../../src-tauri/src/exotic/installer.rs)）是另一套信任链，Part6 已闭合，本 Part 不动。
4. ~~**MS Store Policy 10.2.2**：`msstore` 变体的「下载 worker + AES 解密」路径物理不编译（cfg 门控）~~ —— 随三渠道退役作废（P16，2026-09-15）：无 Store 渠道即无该合规面，仅剩直销一个渠道。

---

## §2 现状实测（file:line 一手取证）

> 取证方式：4 路并行源码 recon + 关键点人工复核。所有论断带 `文件:行号`，区分「确认存在」「确认缺失」。

### 2.1 CI/CD 现状：只编译不测试、只护 main 不护 dev

[.github/workflows/ci.yml](../../.github/workflows/ci.yml) 是 `.github/` 下**唯一**文件（无 release workflow、无 CODEOWNERS、无 issue 模板）。解剖：

| 项 | 现状 | 行号 |
|---|---|---|
| 触发 | `push` / `pull_request` **仅 `main`** | ci.yml:3-7 |
| job `rust-check` | `windows-latest`：`cargo check` + `cargo clippy -- -D warnings`，`working-directory: ./src-tauri` | ci.yml:10-27 |
| job `frontend-typecheck` | `ubuntu-latest`：`npm install` + `npm run typecheck`（`vue-tsc --noEmit`） | ci.yml:29-45 |

**🔴 关键缺口（编号沿用取证报告 G1-G8）**：

| # | 缺口 | 等级 | 证据 |
|---|---|---|---|
| G1 | **dev 分支零 CI**：日常开发全在 dev，所有 Part 提交无任何门控 | 🔴 | ci.yml:5,7 仅 `main` |
| G2 | **无 `cargo test`**：CI 只 `check`+`clippy`，单元测试从不执行 | 🔴 | ci.yml 全文无 test |
| G3 | **`psd-worker`/`psd-probe` 不在 workspace**：即便加 `cargo test`，在 `./src-tauri` 下也只能覆盖 `src-tauri`+`exotic-protocol`（path-dep 拉入），worker 8 测试跑不到 | 🔴 | 无根 Cargo.toml；src-tauri/Cargo.toml:42 仅 path-dep exotic-protocol |
| G4 | **无 tag(`v*`) 触发 workflow**，无发布流水线 | 🔴 | 无 release.yml |
| G5 | **无 `tauri build`/bundle/codesign step** | 🔴 | ci.yml 无 bundle |
| G6 | 前端无 ESLint、无 vitest | 🟡 | package.json devDeps 无 eslint/vitest |
| G7 | 无 CHANGELOG / 发布脚本 / `scripts/` | 🟡 | 根目录确认缺失 |
| G8 | 平台割裂：rust 跑 windows、前端跑 ubuntu，无统一矩阵 | 🟡 | ci.yml:12,31 |

**测试数精确口径**（plan 全程以此为准，纠正「162」的笼统说法）：

| 位置 | `#[test]` 数 | 备注 |
|---|---|---|
| `src-tauri/src/` | **162** | exotic/crypto.rs(18)、worker.rs(14)、db/queries.rs(14)、mod.rs(13)、license.rs(12)、install.rs(12) 等 |
| `crates/exotic-protocol/src/` | **14** | frame.rs(12)、message.rs(2) |
| `crates/exotic-workers/psd-worker/src/` | **8** | decode.rs |
| **合计** | **184** | Part0/记忆惯称「162」= 仅 src-tauri 主体；G3 致 psd-worker 8 测试现状不可达 |

### 2.2 打包 / bundle / 版本：一个与「去 ort」冲突的炸弹 + 三处版本号失同步

[tauri.conf.json](../../src-tauri/tauri.conf.json) 关键字段：`productName "Picasa Next"`(行3)、`version "0.1.0"`(行4)、`identifier "com.picasanext.app"`(行5)、`bundle.targets "all"`(行59)。

**🔴 去 ort 善后的精确删除清单**（Part4 已定 AI 推理外置 sidecar、核心去 ort）：

| 删除目标 | 位置 | 说明 |
|---|---|---|
| `bundle.resources` 4 个 ONNX DLL | [tauri.conf.json:68-71](../../src-tauri/tauri.conf.json#L68-L71) | `onnxruntime.dll`/`DirectML.dll`/`dxcompiler.dll`/`dxil.dll`（来自 `node_modules/onnxruntime-node`）；删后 resources 变 `{}` 或整 key 删 |
| `onnxruntime-node` npm 依赖 | [package.json:24](../../package.json#L24) | DLL 来源，前端不再需要 |
| `ort` crate | [Cargo.toml:95](../../src-tauri/Cargo.toml#L95) | `download-binaries+copy-dylibs`（编译期拉 ORT DLL）；连带删行 99 注释备选 |
| `tokenizers`/`ndarray`/`half` | Cargo.toml:108/101/105 | 随 AI 整体移出核心（归 ai-worker crate，Part4 职责，Part7 验证核心不再含） |

> ⚠️ **体积数字不当实测引用**：取证估 DirectML+dxcompiler+dxil「~60MB」、ort「+20MB」均为**估计**。沿用 Part4 校准纪律——「DirectML 大头须 `stat` 实测、勿把估计当数字」；Part7 §6 验收以**实测产物大小**为准，不锚定估计值。

**🔴 版本号三处独立、无单一事实源**：

| 文件 | 字段 | 行号 |
|---|---|---|
| tauri.conf.json | `version` | 4 |
| package.json | `version` | 4 |
| src-tauri/Cargo.toml | `package.version` | 3 |

无 `workspace.package.version` 继承（无根 Cargo.toml）。升版须手改三处 → 极易漂移。

**identifier 锁定风险**：`com.picasanext.app`（含 `picasa` 词根）。Part0 §475 警告——`identifier` **上架后变更代价极高，首次上架前必锁定**；改名（§10.7）未完成前**不可提交 Store**，否则改名后 Store 记录与 identifier 不符须重新上架、丢评分历史。

**已就绪项**（无需新建）：`src-tauri/icons/` 16 个图标齐全（通用 PNG + `icon.icns` + `icon.ico` + MSIX `Square*Logo.png`/`StoreLogo.png` 全套，Tauri 打 MSIX 自动取用）；[tauri.perf.conf.json](../../src-tauri/tauri.perf.conf.json) 为纯注释占位（待 P4/P5 落 ffmpeg externalBin / pdfium resources），`package.json` 已有 `tauri:build:lite`/`tauri:build:perf` 脚本(行12-13)未接 CI。

### 2.3 workspace / sidecar 构建拓扑：worker 不走 externalBin，走「安装期下载 + 运行期验签定位」

**无根 workspace**：`D:/photoapp/picasa-next/Cargo.toml` 不存在；`src-tauri` 是独立单 package（src-tauri/Cargo.toml:1 `[package] name="picasa-next"`，无 `[workspace]`）。`crates/` 现两簇：

```
crates/
├── exotic-protocol/                 # 纯 lib，publish=false，host 与 worker 双端共享帧/消息/错误码
└── exotic-workers/
    ├── psd-worker/                  # [[bin]] psd-worker，path-dep ../../exotic-protocol(行19)
    └── psd-probe/                   # [[bin]] psd-probe，version 0.0.0，注释明示「不进发布构建」
```

src-tauri 仅 path-dep `exotic-protocol`（src-tauri/Cargo.toml:42）；**psd-worker/psd-probe 对 src-tauri 完全孤立**（无引用）。

**🔑 worker 二进制不走 `tauri externalBin`**——这是 Part7 打包/签名设计的关键前提。证据链：tauri.conf.json 无 `externalBin`；worker 经「独立 `cargo build` → 打进 `.ppx` 包 → 安装期下载/原子切换写入 `<AppData>/exotic/plugins/<id>/` → 运行期完整性复核后定位」。运行期定位链（file:line）：

| 步 | 函数 | 位置 |
|---|---|---|
| 1 | `exotic_dir = app_data_dir.join("exotic")` | lib.rs:287 |
| 2 | `exotic_install_dir() = exotic_dir.join("plugins")` | state.rs:422-424 |
| 3 | `plugin_install_dir()`（正则 `[a-z0-9-]{1,64}` 防注入） | install.rs:347-358 |
| 4 | `installed_worker_path()`（读 manifest 找 `kind=="worker"`） | installer.rs:244-257 |
| 5 | `resolve_worker_path()`（🔴 SEC-02 已修：`EXOTIC_PSD_WORKER_PATH` 跳验签旁路现 `#[cfg(debug_assertions)]` 门控、**仅 debug**；prod/release 走 `verify_installed_integrity`，失败→None） | installer.rs:265-277 |
| 6 | Coordinator 调用 `resolve_worker_path(exotic_install_dir, PSD_PLUGIN_ID, keyset, now)` | coordinator.rs:159-169 |

**对 Part7 的含义**：① worker 不在主包 bundle 里 → **平台代码签名须单独覆盖每个 worker `.exe`/`.dylib`**（Part0 §521「Notarization 覆盖所有 sidecar」），因为它们经 OS 执行、受 Gatekeeper/SmartScreen 检查；② worker 在 `.ppx` 内分发 → 签名时机在「构建 worker 二进制后、打入 `.ppx` 前」，与主包签名是两条产线；③ AI/face worker 未来同构（Part4/Part6）。

**Part6 已认领 workspace 边界**（Part7 不重复）——[Part6 §3.9](Part6_插件平台与exotic收尾.md) / T10：

| 项（已全部落地，Part7 只消费） | 状态 | 认领方 |
|---|---|---|
| 建根 `Cargo.toml`（members **显式逐一列出**，禁 `crates/*` glob，resolver="2"） | ✅ | **Part6 T10** |
| 授权实现归一（`KeyringLicenseStore` 唯一实现；未授权回退现由同文件 `FreeStubEntitlement` fail-closed 承担（`channel_stubs` 渠道桩已随 P16 退役删除）） | ✅ | **Part6 T10** |
| 公开面门禁定义（原锁 `--locked` 构建测试 + 密钥扫描） | ✅ | **Part6 T10** → **Part7 实现进 `.yml`** |

> Part7 从「已有根 workspace、授权实现已归一、公开面门禁已定义」的状态起步，职责是把门禁**实现为可运行的 `.yml`** + 补 test/build/sign/release/updater/渠道矩阵。

### 2.4 签名 / 更新 / 渠道地基：三者皆零，且 updater 依赖离线缺失

**① 平台代码签名——完全未配置**。tauri.conf.json 全文无 `windows.signCommand`/`certificateThumbprint`、无 macOS 签名子段、无 `bundle.publisher`；ci.yml 无签名 step。须与 exotic 应用层验签严格区分：

| 维度 | exotic Ed25519 验签（**已完成**） | 平台代码签名（**未配置**，Part7 建） |
|---|---|---|
| 目的 | 插件包内容完整性 + 来源授权 | 可执行文件来自已知发布者（OS 信任链） |
| 机制 | `ring` `verify_strict`（RFC 8032）+ 逐文件 SHA-256 白名单 | Win Authenticode 证书链；macOS Apple 签名 + Notarization ticket |
| 密钥 | 私钥离线/HSM；验签公钥随源码 embed（可公开） | Win EV 证书；Apple Developer ID Application 证书 |
| 防护 | 伪造/篡改 `.ppx`、keygen、装后替换 worker | SmartScreen/Gatekeeper「未知发布者」拦截 |
| 层 | 纯应用层（OS 不参与） | OS / App Store 强制 |
| 现状 | installer.rs 验签 + 162 测试绿 | tauri.conf.json 零签名字段、CI 零签名 step |

**② 热更新——完全未引入，且离线缓存缺失（🔴 可行性约束）**。`tauri-plugin-updater` 在 Cargo.toml/package.json/`src/`/capabilities 四处均无；`capabilities/` 仅 `default.json`（权限含 core/dialog/fs/shell/window-state，无任何 `updater:*`）；tauri.conf.json 无 `plugins.updater`/`pubkey`/`createUpdaterArtifacts`。

> 🔴 **离线障碍（人工核实 `~/.cargo/registry/cache`）**：缓存现有 8 个 `tauri-plugin-*`（dialog/fs/shell/window-state/opener/sql/clipboard/plugin）**独缺 `tauri-plugin-updater`**，其验签传递依赖 `minisign`/`zip-extract`/`self_update` 亦全无。结合记忆 gotcha「本机 crates.io 不可达，不能新增未缓存 crate」——**R6 在当前离线环境无法即时 `cargo build` 验证**，须联网拉取后方可编译。本 Part 对 R6 给「设计完备 + 标注待联网验证」，不强求当下编译通过。
> updater 自带 **minisign** 验签（`tauri signer generate` 生成 minisign 密钥对，pubkey 进 `plugins.updater.pubkey`），与 exotic 的 ring/Ed25519 信任链是**两套独立机制**，不复用。

**③ 渠道——已退役，唯一 `direct`（P16，2026-09-15 对账）**。三渠道 feature 已退役：`channel-direct`/`channel-msstore`/`channel-steam` 在 Cargo.toml 与 `src/` 全无，`channel_stubs.rs`、互斥/零渠道 `compile_error!` 守卫、`InstallSource` 渠道分支与 CI 三渠道依赖树断言同批删除。现有 features：`default=["custom-protocol","lite"]`、`lite=[]`、`perf=["ffmpeg","netfs"]`、`ffmpeg=[]`、`netfs=["dep:reqwest_dav"]`。`tauri-plugin-updater` 为普通依赖（非 optional、不绑 feature），运行时按最终配置是否带 `plugins.updater` 决定注册。

**Part0 渠道设计锚点**（已退役，仅存结论）：
> §9.3/§9.4 的三渠道设计（feature/`#[cfg]` 物理排除/AES/Registry 分支与 CI 三 job）已随 P16 整段删除，正文见 Part0 §9.3 说明；
> §9.6 时序中「上微软 Store → MSIX → Steam」的分阶段上架已退役为**不预埋**方向；当前只发布直销 `direct` + 正常 updater，Store/Steam 无实现、无 feature、无上架脚本。
> （原第三条重复表述已并入上两条，不再复述退役设计。）

**Part0 路线图中 Part7 全部条目**（行22/461/507/516/521）：CI 加 `cargo test`（4 行，P0）+ 建 workspace（归 Part6 实做）；`tauri-plugin-updater` + 签名密钥对；Win Authenticode + dev 触发；直销构建；macOS Notarization 覆盖所有 sidecar；CI 双 pipeline + 签名隔离。

---

## §3 设计方案

### 3.0 feature 维度（先把正交维度钉死，后续所有构建命令据此组合）

发布工程的复杂度原在「同一码库 → 多产物」。**渠道维度已退役（P16，2026-09-15）**：只有直销 `direct` 一种产物，不存在渠道 feature、渠道 `#[cfg]`、`InstallSource` 或渠道构建矩阵。

| 维度 | feature | 语义 | 归属 |
|---|---|---|---|
| **A 体积/后端** | `lite`(默认) / `perf`=`ffmpeg,netfs` | 包体积与 native 后端（MF vs FFmpeg、UNC vs 原生 VFS） | 已存在（Cargo.toml:142-149） |
| **B 开源边界** | 已收敛（无 feature） | ✅ 第一方公开源码统一 AGPL-3.0-only，公开与私有树用同一份 manifest 与依赖图；授权装配在组合根单点完成 | **Part6 T9/T10 已落地** |
| **C 分发渠道** | ~~`channel-direct`/`channel-msstore`/`channel-steam`~~ **已退役** | 唯一渠道 `direct`；无渠道 feature、无 cfg 门控、无 Provider stub | **P16 已删除**（原 Part7 建 feature+cfg、Part8 填 Provider 的部分整体作废） |

**现行产物**：只有直销产物一种——`lite`（默认）或 `perf` 可选构建，私有 CI 签名/公证，授权来源 `KeyringLicenseStore`（Ed25519 license + keyring），更新走**普通依赖**的 `tauri-plugin-updater`。公开自构建（public Actions）构建同一唯一形态，只是无官方签名与更新服务。

> **渠道退役后的口径**：不再有「同一码库多产物」的渠道维度；构建命令无需 `--no-default-features`，源码里也没有任何渠道 `#[cfg]`。Store/Steam 不做事先预埋，真要上架时按届时平台现状重评（见 §2.4③）。

### 3.1 CI 质量门控（R1+R2，解 G1/G2/G3/G6/G8）

**核心改造**：ci.yml 触发面 + workspace 级 test + oss 边界断言 + 平台矩阵。前提是 Part6 已建根 workspace（否则 `--workspace` 无意义、G3 不解）。

**3.1.1 触发面（解 G1）**：`push`/`pull_request` 增加 `dev`：

```yaml
on:
  push:
    branches: [ main, dev ]
    tags:     [ 'v*' ]        # 🔴 tags 须在 push 下，非 on 顶层（GitHub Actions 无 on.tags）；解 G4 触发 release.yml（§3.7）
  pull_request:
    branches: [ main, dev ]
```

**3.1.2 测试门控（解 G2+G3）**：rust job 改为 **workspace 级、根目录执行**：

```yaml
# working-directory 从 ./src-tauri 上移到仓库根（Part6 建 workspace 后根有 Cargo.toml）
- run: cargo test --workspace --locked        # 覆盖 src-tauri + exotic-protocol + psd-worker(解 G3) + 全部 crates
- run: cargo clippy --workspace -- -D warnings
```

> ⚠️ **G3 的根因是 working-directory 钉死 `./src-tauri`**（现 ci.yml:23）。Part6 建 workspace + Part7 上移目录到根，psd-worker 的 8 测试方可达。`--locked` 防 CI 改 `Cargo.lock`（离线/可复现）。

**3.1.3 公开面门禁（红线）**：公开树与私有树代码同构，故不再有「某 crate 不得入仓/不得入依赖树」式断言。公开面的把关回到通用机制——**密钥扫描（gitleaks）** + **公开树原锁 `--locked` 构建与测试本身**（任何漏掉的私有材料会让构建或测试直接红）。门禁的 `.yml` 形态见 `oss-gate.yml`。

**3.1.4 平台矩阵（解 G8）+ 前端门控（解 G6）**：

- rust job 用 `strategy.matrix.os: [windows-latest, macos-latest]`——macOS 因 objc2 FFI 桥（Part3/Part0 §5.5）必须真 mac runner 编译；Linux 非目标平台，rust 不跑。
- 🔴 **硬前置：macos-latest runner 当前编译必失败，须 Part3 mac 门控先落地**。实测 `windows` crate 是**无条件依赖**（[Cargo.toml:53](../../src-tauri/Cargo.toml#L53) 无 `[target.'cfg(target_os="windows")']` 包裹）、`engine/gpu/mod.rs:2` `pub mod wic_engine` 无 `#[cfg(windows)]`、`ai/engine.rs` 无条件 `use ort::...`——mac runner 编译主 crate 直接炸。**故本任务依赖 Part3「mac 三步走第一步=cfg 门控让 mac 可编译」完成**（`windows` crate 移入 target.cfg、WIC/MF/ort 相关 mod 加 `#[cfg(windows)]`）；在那之前 macos runner **仅跑 `cargo check`/clippy、不跑 `cargo test`**（退化方案，G8 暂不彻底）。
- 前端 job：保留 `npm run typecheck`；G6 增 `eslint`（可选，先 warning 不阻断，渐进收紧）。前端测试（vitest）非本 Part 必须，留 Part5 收尾——🔴 **P1-8（第 6 轮独立核验）**：Part5 须落**具体**前端测试任务（vitest 组件/store 单测 + 关键交互如稳定多选/拖放/撤销的回归），非仅口头延后；高交互功能仅靠人工验收风险过高。
- **联网说明**：CI 跑在 GitHub Actions / 私有 CI（有公网），可正常拉 `tauri-plugin-updater` 等——§2.4 的离线缓存缺失只约束**本机开发期验证**，不影响 CI 流水线设计与执行。

**3.1.5 双 pipeline 落地（实现 Part6 定义）**：
- **公开构建**（`.github/workflows/ci.yml` + `oss-gate.yml`）：test + clippy + 密钥扫描 + 前端，**无签名**。
- **发布构建**（私有 runner，`v*` 触发）：平台签名 → 见 §3.7。两者物理隔离，凭据只在私有侧。

**3.1.6 🔵 开机冒烟测试（boot smoke test，2026-06-28 已落地代码钩子）**：

单元测试在 `#[tokio::test]`（自带 reactor）下全绿、却不保证应用能真正 boot——本轮 coordinator 在同步 Tauri setup 钩子里 `tokio::spawn`（无 reactor）开机即 panic，184 测试一个都没拦住。补**开机接缝**门控：

- **代码钩子（已实施）**：[lib.rs](../../src-tauri/src/lib.rs) `RunEvent::Ready` 分支检测 `PICASA_SMOKE_TEST` 环境变量 → 应用「启动就绪」即 `app_handle.exit(0)`。setup 中途任何 panic（reactor / 迁移 / DB）→ 永不到达 Ready → 进程异常退出（101）。
- **CI 接线（Part7 待落 `.yml`）**：commercial/oss build 产物后加一步 —— `PICASA_SMOKE_TEST=1 ./picasa-next.exe`（headless，带超时），**断言退出码 == 0（非 101）**。挂在 `tauri build` 后、发布前。
- ⚠️ 须跑**构建产物**（frontendDist 内嵌，无需 dev server）；`tauri dev` 因依赖 vite devUrl 不适合无头冒烟。
- 归属：钩子在主程序（已交付）；CI 步骤随 §3.1 双 pipeline 落地。**这是 [Part0](Part0_总纲与产品定稿.md) §11.4.4「测试地基硬门槛」的第一项。**

### 3.2 版本号单一事实源（R3，解三处漂移）

三处 version（tauri.conf.json:4 / package.json:4 / Cargo.toml:3）现手动同步。Part6 建 workspace 后，定 **`Cargo.toml` 的 `[workspace.package] version` 为唯一事实源**，其余两处对齐：

- **Rust 侧**：根 workspace 定 `[workspace.package] version = "x.y.z"`，各成员 `version.workspace = true`（src-tauri 继承；exotic-protocol/psd-worker 可保自有版本，它们是内部组件不对外）。
- **`tauri.conf.json`**：🔴 **删除 `version` 字段**（不填）。Tauri 2 的 `version` 字段 deserializer **只接受 semver 字符串或指向 `package.json` 的路径**（`serde_json::from_str` 解析，tauri-utils-2.9.2/src/config.rs:3466）——**指向 `Cargo.toml` 会 JSON 解析失败、`tauri build` 终止**（初稿「支持指向 Cargo.toml」事实错误）。version 缺省时 Tauri codegen 回退 `env!("CARGO_PKG_VERSION")`（tauri-codegen context.rs），即从 `[workspace.package] version` 取——这才是真正的单源做法。
- **`package.json`**：前端 version 仅用于 npm 元数据，不进产物版本。🔴 **P1-9 闭环（第 8 轮核验）**：纯「相等断言」只 fail 不 propagate——单改锚点（workspace.package.version）后 package.json 仍停旧值 → CI 必 fail 而非自动同步，与「升版只改一处」矛盾。**定稿走脚本同步**：`scripts/sync-version.mjs` 从 `[workspace.package] version` 读锚 → 写回 `package.json.version`（幂等）；CI 跑 `--check`（dry-run：只比对、不一致 fail 并提示「跑 sync-version --write」）+ 发布时 `--write`。附 **dirty-tree 检查**（写回后 git 无残留 diff 才放行）+ **tag 一致性**（`v$VERSION` 的 $VERSION == 锚，防 tag 与版本漂移）。
- **`identifier` 锁定**：CI 加断言——`identifier` 一旦首次进入 release tag，后续变更须显式 override 标记（防误改；真正锁定时机是改名完成、首次上架前，见 Part0 §475）。

> 决策：**Cargo workspace version 为锚，tauri.conf.json 删 version 字段（自动取 `CARGO_PKG_VERSION`），package.json 由 `scripts/sync-version.mjs` 从锚生成/写回（非纯断言，P1-9）**。升版只改一处（workspace.package.version），`sync-version --write` 传导至 package.json；CI `--check` + dirty-tree + tag 一致性兜底。

### 3.3 打包瘦身：去 ort 善后 + 核心体积验证（R4，配合 Part4 worker 化）

Part4 已定「AI 推理外置 sidecar、核心去 ort」。Part7 负责**打包侧善后**——按 §2.2 删除清单移除 4 处，并验证 <10MB 弹性目标：

1. **删 `bundle.resources` 4 个 ONNX DLL**（tauri.conf.json:68-71）→ resources 变 `{}` 或整 key 删。
2. **删 `onnxruntime-node`**（package.json:24）。
3. **删 `ort` crate**（Cargo.toml:95 + 99 注释备选）；`tokenizers`/`ndarray`/`half` 随 AI 移入 ai-worker crate（Part4 owner，Part7 验证核心 `cargo tree` 不再含 ort/tokenizers）。
4. **Lite profile 已就绪**（Cargo.toml:180-185 `[profile.lite]` opt-level="z"+fat LTO+codegen-units=1+strip）——`tauri build --profile lite` 产出极致裁剪核心。

> 🔵 **T6 落地修正(2026-07-04)**:①资源声明实际位于 tauri.windows.conf.json(2026-07-02 平台拆分后,§2.2 的 tauri.conf.json:68-71 行号过时),已整文件删除;②onnxruntime-node **移 devDependencies 而非删除**——它仍是 dev 推理 DLL 的唯一来源(.cargo/config.toml [env] ORT_DYLIB_PATH 直指包内 DLL,tauri-build 资源复制机制退役;真删随 Part8 插件分发 DLL 后);③§2.2 表「half 随 AI 移出」不成立——half 为 host 检索面 f16 常驻向量缓存(vector_store.rs)的合法直依赖,保留;④CI(ci.yml Rust job / oss-gate.yml)的「npm ci 材料化 ONNX 资源」补丁同步撤销。

**体积验收纪律**（沿用 Part4 校准）：§6 验收以 `tauri build --features lite --profile lite` 后**实测主程序 + 安装包大小**为准；**不锚定** DirectML「~60MB」、ort「~20MB」等估计值——估计仅用于「删了能省」的方向判断，验收看真实 `stat`。

> ⚠️ **去 ort 的真实净体积**取决于：① ort DLL 是否真从产物消失（`copy-dylibs` 删除后）；② DirectML.dll 等是否还被任何残留路径引用。Part7 验收脚本须 `ls -la` bundle 内容物清单逐项核对，而非只看安装包总大小。

### 3.4 平台代码签名 / 公证（R5，解 G5 的签名维度，红线：凭据仅私有 CI）

**目标**：主程序 + **所有 sidecar worker** 过 Windows SmartScreen / macOS Gatekeeper。与 exotic Ed25519 验签正交（§2.4 表），二者并存。

**3.4.1 Windows Authenticode**：
- 证书：EV Code Signing 证书（OV 也可但 SmartScreen 信誉积累慢，EV 即时通过）。私钥在硬件令牌 / 云 HSM（Azure Key Vault 等）。
- 签名方式：tauri.conf.json `bundle.windows.signCommand`（Tauri 2 支持自定义签名命令，对接 `signtool` 或云签名 CLI），凭据经 CI secrets 注入。**public Actions 无此配置**（仅私有 commercial-build）。
- 覆盖物：主 `.exe` + NSIS/MSI 安装器 + **每个 worker `.exe`（psd-worker、未来 ai-worker/face-worker）**。worker 不在主 bundle（§2.3），须在「`cargo build` worker → 打入 `.ppx` 前」单独 `signtool sign`。

**3.4.2 macOS 签名 + 公证**：
- 证书：Apple Developer ID Application 证书。
- 🔴 **流程 = inside-out 逐级签名，禁 `--deep`**（P0-10，第 6 轮独立核验联网证实 Apple TN2206：「`--deep` is for emergency repairs and temporary adjustments only / not recommended」，正式分发须 inside-out 逐级签、Xcode 即如此）：**先签最内层** `.dylib`/framework/各 sidecar worker 二进制 → **再签外层** `.app` → **最外层最后签**；各级 `codesign --options runtime --timestamp`（含 Hardened Runtime，**不加 `--deep`**；`--deep` 仅保留给 verify 调试）→ `xcrun notarytool submit --wait` → `xcrun stapler staple`。
- 🔴 **覆盖所有 sidecar + staple 须有容器**（Part0 §521，P0-10 联网核实）：macOS 对每个可执行/`.dylib` 独立签名；worker 二进制（`.ppx` 内）须各自 codesign。⚠️ **裸 Mach-O 二进制无法直接 staple**（notarization 票据无处附着）——sidecar/worker 须先封入 `.app`/`.dmg`/`.pkg` 容器再 notarize+staple；可 zip 裸二进制提交 notarize、但票据贴不回裸 exe。故 mac 交付物形态统一为 bundle/dmg/pkg，杜绝「分发裸 exe + 期望 staple」。Hardened Runtime 下未签名 sidecar 会被 Gatekeeper 直接拒启。
- entitlements：若 Part4 开 CoreML（Part0 §5.5 建议 P6.5 平台签名后再开），须申请对应 GPU/ML entitlement。

**3.4.3 签名时机与产线**（关键，因 worker 不走 externalBin）：

```
[私有 commercial-build]
  ① cargo build worker (psd/ai/face)  ──signtool/codesign each──►  签名后 worker 二进制
  ② worker 打入 .ppx 包（manifest 含逐文件 SHA-256，Part6）──► Ed25519 签名 .ppx（应用层）
  ③ tauri build 主程序 ──signtool/codesign 主 exe + 安装器──► 签名后安装包
  ④ notarytool（macOS 主包 + 各 sidecar）► stapler
  ⑤ createUpdaterArtifacts + minisign 签名（§3.5）
```

> **两套签名各司其职**：步②的 Ed25519 是「插件来源/完整性」（防 keygen/篡改）；步①③④的平台签名是「OS 信任」（过 SmartScreen/Gatekeeper）。一个 worker 二进制**同时**被平台签名（OS 认）+ 装进 Ed25519 签名的 `.ppx`（应用认）。

> **🔑 两类 sidecar 的签名路径不同**（核实：现 `externalBin` 仅注释占位，[Cargo.toml:144](../../src-tauri/Cargo.toml#L144) / [tauri.perf.conf.json:3](../../src-tauri/tauri.perf.conf.json#L3)，无实际配置）：
> - **exotic worker**（psd/ai/face，走 `.ppx` 安装、不进主 bundle）→ 上方「单独签 + 装入 `.ppx`」产线（步①②）。
> - **perf 变体的 ffmpeg sidecar**（未来 P5，计划走 `bundle.externalBin` 进主 bundle）→ **随 `tauri build` 自动签名**（tauri 对 externalBin 一并 codesign/signtool），无须单独产线。
> 故签名脚本须**枚举两类**：externalBin 内的（tauri 管）+ `.ppx` 内的（自管），勿只覆盖一类（呼应 RK5 sidecar 漏签）。

### 3.5 热更新通道（R6，解 G5 的更新维度；离线标注：须联网拉依赖）

**目标**：主程序自动检查/下载/安装更新（直销渠道）。与 exotic 插件 Registry（Part6）各司其职——前者更新**核心 app**，后者更新**插件包**。

**3.5.1 依赖与密钥**：
- 引 `tauri-plugin-updater`（Rust）+ `@tauri-apps/plugin-updater`（前端）。capabilities/default.json 加 `updater:default`。
- `tauri-plugin-updater` 作**普通依赖**引入（唯一渠道 `direct`，无渠道 feature、无 optional 绑定、无 `#[cfg]` 门控）：`cargo build` 即直销产物且含 updater，不存在「哪些渠道进二进制」的分支问题。

- `tauri signer generate` 生成 **minisign 密钥对**：私钥（`TAURI_SIGNING_PRIVATE_KEY` + 可选密码）仅私有 CI secrets；公钥进 tauri.conf.json `plugins.updater.pubkey`。
- 🔴 **离线约束**（§2.4）：`tauri-plugin-updater` + `minisign`/`zip-extract` 不在本机 cargo 缓存，本机无法编译验证；**CI/联网环境正常拉取**。本 Part 给完整配置设计，标注「`cargo build` 验证须联网」。

**3.5.2 配置**：
```jsonc
// tauri.conf.json
"plugins": {
  "updater": {
    "pubkey": "<minisign 公钥>",
    "endpoints": ["https://<官网>/updater/{{target}}/{{arch}}/{{current_version}}"]
  }
},
"bundle": { "createUpdaterArtifacts": true }   // 生成 .sig 签名的更新产物
```
- ⚠️ **`createUpdaterArtifacts` 只产带 minisign `.sig` 的更新包，不生成 `latest.json`**（P0-10，第 6 轮独立核验联网证实 Tauri 官方：`tauri build` 仅产更新包+签名，唯 `tauri-action` 会生成静态 JSON）→ 直销渠道须**自建 `latest.json`/动态 endpoint**：发布脚本读各 `.sig` 内容、组装 `{version, notes, pub_date, platforms.{target}.{signature,url}}` JSON 上传 CDN/官网 endpoint。**不可假设 `tauri build` 自动产清单**（这正是 R6「updater 离线/自托管缺失」缺口的一部分）。
- endpoints 指向官网更新服务（直销）；返回版本/下载 URL/`.sig`。

**3.5.3 与 exotic Registry 的边界（不混用）**：

| | 主程序热更新（R6） | exotic 插件 Registry（Part6） |
|---|---|---|
| 更新对象 | 核心 app `.exe`/安装包 | 插件 `.ppx`（worker + 模型） |
| 验签 | minisign（`.sig`） | Ed25519（ring，`verify_strict`） |
| 触发 | app 启动/定时查 endpoints | 用户在插件商店点更新 |
| 单调性 | 版本号比较 | 签名 Registry index 单调防回滚 |

> 二者**密钥体系、验签库、更新源都独立**。不要让 updater 的 minisign 和 exotic 的 Ed25519 互相复用——它们防的是不同攻击面（app 投毒 vs 插件伪造）。

**3.5.4 渠道差异——已消失**：三渠道退役后不存在渠道分支，updater 对唯一渠道 `direct` **无条件生效**（普通依赖 + 运行期按配置注册）。原「Store/Steam 由平台分发更新、updater 须 cfg 门控」的差异随渠道一起退役。

### 3.6 ~~多渠道构建变体~~ —— 整段删除（P16，2026-09-15）

原三渠道 feature 定义（`channel-direct`/`channel-msstore`/`channel-steam`）、`dep:tauri-plugin-updater` 的 optional 绑定、互斥与零渠道 `compile_error!` 守卫、CI 三渠道矩阵与依赖树断言**均已删除**，正文整段移除。现行 `Cargo.toml [features]` 只有 `default = ["custom-protocol","lite"]`、`lite = []`、`perf = ["ffmpeg","netfs"]`、`ffmpeg = []`、`netfs = ["dep:reqwest_dav"]`；`tauri-plugin-updater` 是普通依赖（见 §3.5.1），源码零渠道 `#[cfg]`。

### 3.7 Release 流水线（R8，解 G4/G5/G7；oss/commercial 双轨）

**触发**：push tag `v*`（§3.1.1）。**双轨物理隔离**（凭据红线）：

**3.7.1 oss-release（public Actions，免费版）**：
```
tag v* (public repo)
  → cargo test --workspace（复用 §3.1 门控）
  → tauri build --features lite --profile lite   # 公开自构建：不含平台签名与官方更新服务
  → 产物：免费版安装包（无平台签名 或 自签测试证书）
  → gh release create：附安装包 + SHA-256 + CHANGELOG（解 G7，conventional commits 自动生成）
```
> 免费版**可不做平台签名**（用户首启 SmartScreen 提示，或后续用同一 EV 证书签免费版提信誉——成本权衡，Part8 定）。免费版**不含 updater 私钥签名的更新包**除非用同套 minisign（直销免费版可走 updater）。

**3.7.2 commercial-release（私有 CI，付费版，红线：凭据隔离）**：
```
tag v* (私有镜像/私有 runner，secrets 隔离)
  → checkout（公开树与私有树同构，无源码注入步骤）
  → 唯一产物 = 直销 direct（渠道 feature/矩阵已随 P16 删除，无 channel 组合）：
     ├ cargo build --release                     # default = custom-protocol + lite
     ├ 签名 worker 二进制（signtool/codesign，§3.4.3 步①）
     ├ 打包 unsigned .ppx + 计算 digest → 经签名 ceremony 取回 Ed25519 .sig 附加（🔴 私钥不入 CI，见 §3.7.3a；§3.4.3 步②）
     ├ tauri build -c src-tauri/tauri.direct-release.conf.json → 签名主 exe/安装器（§3.4.3 步③）
     ├ [macOS] notarytool + stapler（覆盖 sidecar，§3.4.2）
     ├ createUpdaterArtifacts + minisign 签 .sig + 传官网 updater endpoint
     └ Store/Steam 上架不预埋：真要上架时按届时平台现状重评
  → 发布：直销版→官网；付费插件 catalog→私有 CDN，URL 在 release-key 签名的 registry index（Part0 §437）
```

**3.7.3 凭据清单（全部仅私有 CI secrets，public Actions 无）**：

| 凭据 | 用途 | 存放 |
|---|---|---|
| Win EV 证书 / 令牌 PIN | Authenticode 签名 | 私有 CI secrets / 云 HSM |
| Apple Developer ID + notarytool API key | macOS 签名/公证 | 私有 CI secrets |
| `TAURI_SIGNING_PRIVATE_KEY`(minisign) | updater 更新包签名 | 私有 CI secrets |
| exotic Ed25519 私钥 | `.ppx` / Registry index 签名 | 离线/HSM（**不入任何 CI**，签名 ceremony 见 §3.7.3a） |
| AES `enc_seed` / license 私钥 | 模型权重加密 / license 签发 | 私有（Part8 签发服务；验签公钥可公开） |
| 发布构建的签名凭据 | 平台签名 / 更新包签名 | 私有 CI secrets |

> public Actions **只跑** test/clippy/typecheck/门禁（§3.1），**零签名、零渠道私料**。公开树与私有树代码同构（无源码剥离面）。

**3.7.3a 🔴 .ppx 签名 ceremony（P1-8，第 8 轮核验，消解「CI 签 .ppx」vs「私钥不入 CI」冲突）**：

§3.7.2 流水线含「Ed25519 签 .ppx」步，凭据表又定该私钥**不入任何 CI** → 同一私钥要在 CI 用却不能进 CI，逻辑互斥（且原引 §5.1 不存在=悬空）。消解：CI **永不持 exotic Ed25519 私钥**，签名经带外机制完成，二选一：

- **离线签名 ceremony（推荐，纯气隙）**：commercial CI 产 **unsigned .ppx** + 计算 `digest=sha256(.ppx)`，digest 作 artifact 上传；**气隙签名机**（持 Ed25519 私钥）拉 digest → 签 → 回传 `.sig`；CI 取回 `.sig` 附加到 .ppx。私钥全程离线、不触网、不入 CI。
- **远程 HSM signing API（半自动）**：CI 调 HSM 签名端点（mTLS + 短时令牌），HSM 内私钥签 digest 返回签名；CI 持的是**可轮换的 HSM 调用凭据**（非私钥本身），泄漏可吊销换发。
- **审计 + 重试 + 防替换**：每次签名留痕（签名人/时间/digest/版本/ceremony 方式）；签名服务不可达 → CI 重试 N 次后转人工；CI 取回 `.sig` 后**用对应公钥验签 .ppx**（确认 digest 与最终产物一致，防中途替换）。
- **Registry index 签名**同此 ceremony（同一 Ed25519 私钥）。

> 凭据表「exotic Ed25519 私钥：离线/HSM、不入任何 CI」与本节一致——流水线「Ed25519 签 .ppx」实为「CI 提交 digest → ceremony 取回 .sig」，私钥不在 CI。

### 3.8 设计小结（R1-R8 落点速查）

| R | 主题 | 关键改动落点 |
|---|---|---|
| R1 | CI 测试门控 | ci.yml:3-7 加 dev；rust job 改 `cargo test --workspace`（根目录） |
| R2 | workspace CI 适配 | working-directory 上移根；oss 边界断言（git ls-files + cargo tree）；matrix os |
| R3 | 版本单一源 | workspace.package.version 为锚；tauri.conf.json **删 version 字段**（自动取 `CARGO_PKG_VERSION`）；`scripts/sync-version.mjs` 从锚写回 package.json（非纯断言，P1-9）+ CI --check + dirty-tree/tag |
| R4 | 打包瘦身 | 删 4 处 ort（tauri.conf.json:68-71 / package.json:24 / Cargo.toml:95,99）；profile lite 验收 |
| R5 | 平台签名/公证 | bundle.windows.signCommand；macOS codesign+notarytool；**覆盖 sidecar**（§3.4.3 产线） |
| R6 | 热更新 | tauri-plugin-updater + minisign 密钥 + plugins.updater + createUpdaterArtifacts；唯一 direct；离线待联网验证 |
| R7 | 渠道预留已退役 | 当前只有直销，不再维护渠道 feature、Provider stub 或三渠道 CI |
| R8 | Release 流水线 | tag v* 触发；oss-release(public) / commercial-release(私有 CI，单一 direct 产物，无 channel matrix)；凭据隔离 |

---

## §4 任务清单

> 依赖标注：`← Part6` 指必须 Part6 对应任务先落（workspace/pro/EntitlementProvider）。`待联网` 指本机离线无法编译验证、须公网环境。

| # | 任务 | 落点 | 依赖 | 验收锚 |
|---|---|---|---|---|
| **T1** ✅ 已实施(dev 触发=R0-1;tag `v*`+冒烟 job=2026-07-02 `ff589da`) | ci.yml 触发面加 `dev` + tag `v*`;开机冒烟 job(§3.1.6)挂 tag/手动 dispatch——日常 push 不背完整 tauri build,T16/17 双流水线建成后迁入;附 tag 与版本锚一致性门控。冒烟 job 本体未实跑(须 CI 环境,dispatch 可验) | .github/workflows/ci.yml | — | dev push 触发 CI（解 G1） |
| ✅ **T2**(已随审查修复 R0-1 `d9c0436` 交付;2026-07-04 对账收账) | rust job 改 `cargo test --workspace --locked` + working-directory 上移仓库根 + clippy `--workspace`〔ci.yml 现行=check/clippy/test 三步全 --workspace --locked+仓库根,dev 每推实跑绿〕 | ci.yml:21-27 | ← Part6 workspace | 184 测试全跑（含 psd-worker 8，解 G2/G3） |
| ✅ **T3**(形态修订,2026-09-12 对账) | 公开面门禁。⚠ 原「pro 未入仓/不在依赖树」两半边随授权实现归一整体作废（不再有闭源 crate）；当今把关回到通用机制——**gitleaks 密钥扫描** + **公开树原锁 `--locked` 构建与测试**（见 `oss-gate.yml`） | `.github/workflows/oss-gate.yml` | ← Part6 T10 | 公开树构建测试全绿 + 密钥扫描零命中 |
| **T4** | 平台矩阵 `matrix.os: [windows, macos]` + 前端 eslint（warning 起步） | ci.yml strategy | — | mac runner 编 objc2 桥（解 G8/G6） |
| **T5** ✅ 已实施(2026-07-02 `ff589da`;dirty-tree 由 --check 等价覆盖,发布流水线 T16/17 落地时再加 --write 链;identifier 锁定断言随 R2-7) | 版本单一源：workspace.package.version 为锚；**删 tauri.conf.json 的 `version` 字段**（Tauri 回退 `CARGO_PKG_VERSION`，🔴 不可指向 Cargo.toml=JSON 解析失败）；🔴 P1-9 `scripts/sync-version.mjs --check/--write` 从锚写回 package.json（非纯断言）+ dirty-tree/tag 一致性 | 根 Cargo.toml、tauri.conf.json:4、`scripts/sync-version.mjs`、ci.yml | ← Part6 workspace | 改一处版本三处**自动**一致 |
| ✅ **T6**(2026-07-04 `40508f8`) | 去 ort **打包资源层**:删 4 DLL 资源声明(实际位于 tauri.windows.conf.json,整文件删;target 副本实测 ~63MB)+ onnxruntime-node **移 devDependencies 而非删**(dev 推理 DLL 唯一来源,.cargo/config.toml [env] ORT_DYLIB_PATH 直指包内 DLL,tauri-build 资源复制退役;真删随 Part8 插件分发)+ CI 双 workflow 撤「npm 材料化」补丁(Rust job 不再需要 node)。验证:cargo tree 零 ort/tokenizers/ndarray(half=host 检索 f16 缓存合法直依赖,§2.2 该行过时)+ 删净 target DLL 后 worker_e2e 全相位过(新指路链端到端实证,仅本地);bundle 内容物 ls 级核对留 T16/T17(本机无 NSIS/WiX) | tauri.windows.conf.json/package.json/.cargo/config.toml/ci.yml/oss-gate.yml | ← Part4 T16（Rust 依赖层先删） | bundle 内容物无 ONNX DLL（§6 实测） |
| ✅ **T7**(2026-07-04 `7b226d0`；2026-09-15 修订) | 引 `tauri-plugin-updater`(Rust，**普通依赖**——渠道退役后不再 optional、不再绑任何渠道 feature；离线预取命中 2.10.1)。⚠ **前端包与 capability 有意不加**(落地修订):更新检查走 Rust 侧 API(`app.updater()`),现无更新 UI 消费者,不开放 webview ACL 面;Part8 做更新 UI 时再定「Rust 驱动+事件」或「JS 驱动+capability」 | Cargo.toml、src/lib.rs | ~~待联网~~ 预取 | updater 为普通依赖 |
| ✅ **T8**(2026-07-04 `7b226d0`,dev 密钥) | `tauri signer generate` 密钥对落 `.release-keys/`(gitignored,私钥绝不入仓);公钥进 plugins.updater.pubkey。**剩余人工半边:私钥入私有 CI secrets + 首发前转正/轮换决策**(换钥=既有安装版收不到更新,须发布前定死;.gitignore 注有决策链) | tauri.conf.json plugins.updater、CI secrets | T7 | updater 配置完整 |
| ✅ **T9**(2026-07-04 `7b226d0`；2026-09-15 修订) | endpoints(`.invalid` 占位域名,真实更新源随 Part8 基建)+ updater 注册**不再做 `#[cfg]` 门控**(渠道退役,唯一 `direct` 产物即含 updater)。⚠ **createUpdaterArtifacts 落 overlay conf**(tauri.direct-release.conf.json + `tauri:build:direct-release` 脚本)而非基底(落地修订):置基底会使无签名 env 的本地构建直接失败。 | tauri.conf.json、发布脚本 | T7 | updater 配置完整且无渠道门控 |
| **T10** ❌ **已退役**(P16,2026-09-15 删除) | 原「渠道 feature：`channel-direct`(default)/`channel-msstore`/`channel-steam`（互斥）+ `compile_error!` 守卫」已整体删除；现行 `default=["custom-protocol","lite"]`，无渠道 feature | ~~src-tauri/Cargo.toml features 区、lib.rs~~ | — | 无渠道 feature 残留 |
| **T11** ❌ **已退役**(P16,2026-09-15 删除) | 原「DRM/updater/下载安装路径加 `#[cfg(feature="channel-direct")]` 物理门控」随渠道特征删除：无渠道 feature 即无需门控，唯一产物就是直销 | ~~src/exotic/*、updater 注册处~~ | — | 源码零渠道 `#[cfg]` |
| **T12** ❌ **已退役**(P16,2026-09-15 删除) | 原「工厂 `#[cfg]` 选 msstore/steam stub」随 `channel_stubs.rs` 删除；未授权回退改由同文件 `FreeStubEntitlement` fail-closed 承担 | ~~src-tauri/src/exotic/channel_stubs.rs~~ | — | 无渠道桩文件与工厂分支 |
| **T13** 🔴 | Win Authenticode：bundle.windows.signCommand 对接 signtool/云签；签名脚本（仅私有 CI） | tauri.conf.json windows、私有 CI | T16 | 主 exe + 安装器有有效签名 |
| **T14** 🔴 | macOS codesign（Hardened Runtime）+ notarytool + stapler；**覆盖所有 sidecar worker** | 私有 CI mac job | T16 | Gatekeeper 放行；sidecar 全签 |
| **T15** | worker 签名产线：`cargo build` worker → signtool/codesign → 打入 `.ppx` → Ed25519 签 | 私有 CI 步骤 | T13/T14、← Part6 .ppx | worker 二进制双签（平台+应用） |
| **T16** 🔴 | release workflow（私有，tag v* 触发，单一 direct 产物）：构建→签名→公证→updater→发布 | 私有仓 .github/release.yml | T6-T15 | tag 触发产出签名发行版 |
| **T17** | oss-release workflow（public，tag v*）：test→tauri build lite→gh release + CHANGELOG | .github/workflows/release.yml | T1-T6 | tag 产出免费版 + CHANGELOG（解 G4/G7） |
| **T18** | identifier 锁定 CI 断言（阻止改名前上架）；**改名执行本身归 Part0 §10.7 / Part8**（本任务不登记/不替换，只加断言） | ci.yml | — | identifier 变更须显式 override |

**关键路径**：T1/T2/T3（门控，无签名依赖，最先做）→ T6（去 ort，配合 Part4）→ T7/T8/T9（updater，普通依赖）→ T13/T14/T15（签名，需证书）→ T16/T17（release 编排）。T18 贯穿；原 T10/T11/T12 渠道骨架已退役，不在关键路径。

---

## §5 风险与缓解

| # | 风险 | 等级 | 缓解 |
|---|---|---|---|
| RK1 | **updater 依赖离线缺失**：`tauri-plugin-updater`+`minisign`+`zip-extract` 不在本机缓存，本机无法编译验证 R6 | 🔴 | 标注「待联网」；CI/联网环境拉取；本 Part 给完备配置设计，编译验证移交联网 CI（T7 验收锚=联网 `cargo build` 过） |
| RK2 | **workspace 是 Part7 多任务前置**：T2/T5/T11 全依赖 Part6 建好 workspace；Part6 未落则 G3 不解、版本单源无锚 | 🔴 | 任务表显式 `← Part6`；Part6 C5 已定 workspace 成员纳入与 `cargo build/test --offline` 全绿；Part7 T2 验收前确认根 Cargo.toml 存在 |
| RK3 | 渠道 cfg 风险项已退役 | — | 当前无商店构建与渠道分支；未来上架按实际实现重新评估，不保留空检查 |
| RK4 | **签名凭据泄漏**：EV/Apple/minisign 私钥若误入 public Actions → 供应链灾难 | 🔴 | 物理双轨（public 无签名 step）；凭据只在私有 CI secrets；CI 扫 public workflow 不含 secret 引用；exotic Ed25519/AES 私钥永不入任何 CI（离线/HSM） |
| RK5 | **sidecar 漏签**：macOS Hardened Runtime 下未签 worker 被 Gatekeeper 拒启，且测试机（已信任 dev 证书）测不出 | 🟡 | T14/T15 产线枚举所有 worker 强制签；§6 在「干净 mac」做 Gatekeeper 验收（非 dev 机） |
| RK6 | **去 ort 残留**：删 ort 后若残留路径仍引 DirectML.dll，瘦身落空且运行期缺 DLL 崩 | 🟡 | T6 验收=bundle 内容物清单逐项核对（非只看总大小）+ 干净机冷启动核心功能 |
| RK7 | **identifier/改名时序**：改名前误上架 → identifier 锁死，改名后须重新上架丢评分 | 🟡 | T18 CI 断言锁定；上架（Part8）前置「改名完成」硬门槛；Part0 §475 已警示 |
| RK8 | **版本字段**：~~指向 Cargo.toml~~ **已确认 Tauri 2 不支持**（config.rs:3466 仅 semver / package.json 路径）→ 定为**删 `version` 字段、回退 `CARGO_PKG_VERSION`** | 🟢 已解 | 删字段即可，无须联网验证；CI version-sync 断言兜底 |
| RK9 | **MS Store MSIX 风险**（Tauri #14935）：MSIX 路径未成熟 | 🟢 | Part0 §9.6 已定时序——先上 Store EXE 路径，MSIX 待上游修复后评估；Part7 只保证 msstore feature 可编译，不强推 MSIX |
| RK10 | **CI 时长膨胀**：`targets:"all"` + 多平台 + 多 channel 矩阵 → CI 慢 | 🟢 | PR 门控只跑 test/clippy（轻）；全矩阵 bundle 仅 tag release 时跑；按需限定 `targets` |

---

## §6 验收标准

**质量门控（R1-R2）**：
- [ ] dev 分支 push/PR 触发 CI（解 G1）。
- [ ] CI 执行 `cargo test --workspace`，**184 个测试全跑**（含 psd-worker 8、exotic-protocol 14，解 G2/G3），全绿。
- [ ] 公开树门禁生效：`oss-gate.yml` 在公开投影树跑原锁 `--locked` 构建/测试 + gitleaks 密钥扫描，全绿方可提升到 main。
- [ ] rust job 在 windows + macos 双 runner 编译通过（mac objc2 桥，解 G8）。

**版本与瘦身（R3-R4）**：
- [x] 改 `workspace.package.version` 一处 → `sync-version.mjs --write` 写回 package.json（🔴 P1-9，非纯断言）；tauri.conf.json **已删 version 字段**（取 `CARGO_PKG_VERSION`）；CI `--check` + tag 一致性生效〔✅ 2026-07-02 `ff589da`;dirty-tree 由 --check 等价覆盖,T16/17 再加 --write 链〕。
- [ ] `tauri build --features lite --profile lite` 后，**bundle 内容物清单无任何 ONNX/DirectML DLL**（`ls -la` 逐项核对，非只看总大小，解 RK6）。
- [ ] 核心 `cargo tree` 不含 `ort`/`tokenizers`/`onnxruntime`；干净机冷启动核心功能正常。
- [ ] 主程序实测大小记录在案（以 `stat` 为准，不锚定估计值；<10MB 为弹性目标，超出须说明大头）。

**签名/公证（R5）**：
- [ ] 直销付费版主 exe + 安装器有有效 Authenticode 签名（`signtool verify` 过）。
- [ ] macOS 版过 notarization（`spctl -a -vv` 放行）；**所有 sidecar worker 二进制均签名**（枚举核对，解 RK5），在干净 mac（非 dev 机）Gatekeeper 放行。
- [ ] public Actions workflow 文件**不含任何签名 step / secret 引用**（红线 RK4，grep 核对）。

**热更新（R6，部分待联网）**：
- [ ] （联网）`tauri-plugin-updater` 编译通过；`createUpdaterArtifacts` 产出带 minisign `.sig` 的更新包；`latest.json` 由**发布脚本自建**（非 `createUpdaterArtifacts` 产物，P0-10）。
- [ ] updater 按最终配置中的 `plugins.updater` 注册，普通开发配置不启用更新入口。
- [x] updater 已无渠道门控（渠道退役）：唯一 `direct`，普通依赖 + 运行期按配置注册。
- [ ] 端到端：旧版本检测到新版本 → 下载 → minisign 验签通过 → 安装（联网集成测试，可移交后续）。

**多渠道（R7）——已退役（P16，2026-09-15）**：原三条验收（三 feature 各自编译、msstore 依赖树无 updater、四组合可构建 + Provider `unimplemented!()`）随渠道 feature 与 Provider 桩删除而**整体作废**，无需再验；`cargo tree` 渠道断言亦从 `verify-channel-bundle.mjs` 移除。当前只需验收唯一 `direct` 渠道可构建、updater 正常注册。

**Release 流水线（R8）**：
- [ ] push tag `v*` 触发：oss-release（public）产免费版 + CHANGELOG（解 G4/G7）；commercial-release（私有）产签名付费版。
- [ ] 凭据全程仅私有 CI；public 与 commercial 物理隔离。

---

## §7 实施提示词（按依赖顺序分阶段，可直接投喂执行 agent）

> 通用前置：本机 crates.io 不可达，新增 crate 前先 `find ~/.cargo/registry` 确认缓存；`cargo build/test` 加 `--offline`。改动后中文 commit，**不主动 push**。开源边界——**签发私钥/密凭证/`enc_seed`/代码签名证书永不入源码或 public Actions**（验签公钥可公开）。

### 阶段 A — 质量门控（T1-T4，无外部依赖，最先做）

```
任务：补齐 CI 质量门控，解 G1/G2/G3/G8。前提确认根 Cargo.toml（workspace）已由 Part6 建好——若无，先停并提示「Part7 T2 依赖 Part6 workspace」。
改 .github/workflows/ci.yml：
1. on.push.branches / on.pull_request.branches 加 dev；加 on.push.tags ['v*']。
2. rust-check job：working-directory 从 ./src-tauri 上移到仓库根；run 改 `cargo test --workspace --locked` + `cargo clippy --workspace -- -D warnings`。
3. 加公开面门禁两 step（公开树原锁 `cargo test --workspace --locked` + gitleaks 密钥扫描），任一红即 fail。
4. rust job 加 strategy.matrix.os: [windows-latest, macos-latest]。
验收：dev push 触发；184 测试全跑（含 crates/exotic-workers/psd-worker 8 测试）；密钥扫描零命中。
中文 commit。
```

### 阶段 B — 去 ort 善后 + 版本单一源（T5-T6，配合 Part4）

```
任务：去 ort 打包善后（解 R4）+ 版本号收敛单一事实源（R3）。前提：Part4 已将 AI 推理移入 ai-worker crate。
1. 删 src-tauri/tauri.conf.json 第68-71行（4 个 onnxruntime/DirectML/dxcompiler/dxil DLL resources），resources 变 {} 或整 key 删。
2. 删 package.json:24 的 "onnxruntime-node"。
3. 删 src-tauri/Cargo.toml:95 的 ort 行 + 99 注释备选；确认 tokenizers/ndarray/half 已随 Part4 移出（若仍在核心则报告）。
4. 版本单源：根 Cargo.toml 加 [workspace.package] version；src-tauri 等成员 version.workspace=true；🔴 **删 tauri.conf.json 的 version 字段**（Tauri 自动取 `CARGO_PKG_VERSION`，**不可指向 Cargo.toml**=JSON 解析失败，config.rs:3466）；CI 加 version-sync 断言兜底。
验收：`cargo tree`（核心）无 ort/tokenizers/onnxruntime；`tauri build --features lite --profile lite` 后 bundle 内容物 `ls -la` 无任何 ONNX DLL；改一处版本三处一致。
中文 commit。
```

### 阶段 C — ~~渠道 feature + cfg 物理门控（T10-T12）~~ 已退役（P16，2026-09-15）

```
**不再执行**。三渠道 feature、`#[cfg(feature="channel-direct")]` 物理门控、Provider 桩选择均已删除（P16，2026-09-15）；现行命令面即 default：`cargo build`（= `custom-protocol`+`lite`），无需 `--no-default-features`。
中文 commit。
```

### 阶段 D — 热更新（T7-T9，待联网）

```
任务：引入主程序热更新（R6）。⚠️ tauri-plugin-updater + minisign 不在本机 cargo 缓存——本阶段须联网环境执行；离线只能写配置不能编译验证。
1. src-tauri/Cargo.toml 加 tauri-plugin-updater；package.json 加 @tauri-apps/plugin-updater；capabilities/default.json 加 updater:default。
2. `tauri signer generate` 生成 minisign 密钥对；公钥写 tauri.conf.json plugins.updater.pubkey；私钥进私有 CI secrets（TAURI_SIGNING_PRIVATE_KEY），绝不入仓。
3. tauri.conf.json：plugins.updater.endpoints 指官网更新服务；bundle.createUpdaterArtifacts=true。
4. updater 注册 + 检查更新命令**不加渠道 cfg**（唯一 `direct` 渠道，正常注册；运行期按最终配置是否带 `plugins.updater` 判定）。
验收（联网）：cargo build 过；createUpdaterArtifacts 产带 .sig 的更新包；latest.json 由发布脚本自建（非 build 产物，P0-10）。
中文 commit。
```

### 阶段 E — 平台签名/公证 + Release 流水线（T13-T18，需证书，私有 CI）

```
任务：平台代码签名/公证 + 双轨 Release 流水线（R5/R8）。红线：所有凭据仅私有 CI secrets，public Actions 零签名。
1. Win Authenticode：tauri.conf.json bundle.windows.signCommand 对接 signtool/云签 CLI；凭据 CI secrets 注入。
2. macOS：私有 CI mac job 做 codesign（--options runtime --timestamp）+ notarytool submit --wait + stapler；枚举所有 sidecar worker 二进制逐个签名（worker 不在主 bundle，须在打入 .ppx 前单签）。
3. worker 签名产线：cargo build worker → signtool/codesign → 打入 .ppx → Ed25519 签（应用层，Part6）。
4. release（私有仓 .github/release.yml，tag v* 触发，**无 channel 矩阵**）：`cargo build`（default = `custom-protocol`+`lite`）→ 签名 → [mac]公证 → updater artifacts → 发布（官网 / 私有 CDN）。原 msstore/steam 的 `winapp pack`/SteamPipe 随渠道退役取消。
5. oss-release（public .github/workflows/release.yml，tag v*）：test → tauri build lite → gh release + 自动 CHANGELOG（conventional commits）。
6. T18：CI 断言 identifier 锁定（变更须显式 override）。改名执行本身归 Part0 §10.7 / Part8，本任务只加断言阻止改名前上架。
验收：tag v* 触发双轨；signtool verify / spctl 放行；干净 mac Gatekeeper 放行所有 sidecar；public workflow grep 无 secret 引用。
中文 commit。
```

---

## §8 与其他 Part 的接缝（一致性自检）

| 接缝 | 约定 | 风险点 |
|---|---|---|
| ← **Part6**（workspace/授权实现归一/EntitlementProvider/stub） | Part7 消费，不重建；T2/T5/T11/T12 依赖其落地 | Part6 未落则 Part7 多任务悬空（RK2） |
| ← **Part4**（AI worker 化、去 ort） | Part7 做打包侧善后（删 resources/依赖）；体积验收配合 | Part4 未移出 tokenizers/ndarray 则核心仍含（T6 报告） |
| → **Part8**（license 签发、定价、落地页） | Part7 只交付唯一 `direct` 渠道（updater 普通依赖 + 正常注册）；MsStore/Steam Provider、渠道 stub、`winapp pack`/SteamPipe 已于 P16 退役 | 渠道维度已整体退出，§3.6 留作历史记录 |
| ↔ **Part0**（§9.3 渠道/§10 开源边界/§475 改名/§521 sidecar 签名/§436 commercial-build） | 全部锚点已对齐摘录 | identifier 改名时序（RK7/T18） |
| ↔ **exotic 信任链**（Part6 已闭合 Ed25519） | 平台签名与应用层验签**两套并存不复用**；updater minisign 第三套 | 三套密钥体系混用（§3.5.3 表已隔离） |

<!-- Part7 全文定稿待执行（terminal review 已并入）。 -->
