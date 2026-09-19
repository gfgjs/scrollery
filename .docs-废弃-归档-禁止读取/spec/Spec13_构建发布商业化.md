---
id: 2026-07-24-Spec13_构建发布商业化
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec13-构建发布商业化

> 本篇讲 Scrollery 的**构建变体 / 分发渠道 / CI / 更新 / 许可基建 / 商业化**的 as-built 现状。服务读者：要改构建矩阵、接入新渠道、或搭建商业发布流水线的人类工程师，以及只读本篇 + 所引 `path:line` 就要复现该构建/发布链路的低能力 LLM 代理。产品名一律 **Scrollery**（`identifier` = `com.scrollery.app`，`src-tauri/tauri.conf.json:4`），文中不出现旧代号。读前建议先扫一遍 [Part7 发布工程](../refactor_2026/Part7_发布工程.md) §1（目标范围）与 [Part8 商业化与分发](../refactor_2026/Part8_商业化与分发.md) §1（目标范围）——但**本篇正文以现码为准**，两篇计划文档只在「为什么这样设计」处被引用，不复制其论证。

---

## 1. 概览

Scrollery 是单 Rust workspace（`Cargo.toml`）+ 单前端（`package.json` + Vite）+ Tauri v2 打包的桌面应用。三面职责：

- **构建**：Cargo feature 矩阵决定二进制里编译进什么（体积变体 lite/perf、发布渠道 direct）。
- **发布**：CI（自托管 runner）做质量门禁；两条互不相通的 Release 流水线——公开仓 `oss-release`（`.github/workflows/release.yml`，免费版）与私有仓 `commercial-release`（**待核实**：Part7 §7 阶段 E T13-T18 计划稿存在，仓内未见 `.github/release.yml` 或等价私有专属 workflow 文件，判断为**计划未落地**，见 §5）。
- **商业化**：核心免费 + 插件买断，许可基建（License token 验签 + OS keyring 存储）已闭环，签发端/支付/上架三者仍是**计划**（Part8 as-planned，§7）。

**代码位置**：

| 目录/文件 | 职责 |
|---|---|
| `Cargo.toml`（根） | workspace 成员声明（显式逐一列出）+ `[profile.*]` |
| `src-tauri/Cargo.toml` | `[features]` 矩阵（渠道/变体/合规隔离/devtools） |
| `src-tauri/src/lib.rs` | updater 条件注册（`:131-137`）+ `BUILD_VARIANT` 常量（`:57`） |
| `src-tauri/tauri.conf.json` | 基座配置（productName/identifier/窗口/CSP/bundle） |
| `src-tauri/tauri.direct-release.conf.json` | direct 渠道 overlay（updater pubkey + endpoints） |
| `src-tauri/tauri.perf.conf.json` | perf 变体 overlay（当前与基座一致，占位） |
| `src-tauri/capabilities/*.json` | Tauri v2 权限声明（`default.json`/`logs.json`） |
| `package.json` scripts | `tauri:build:lite`/`tauri:build:perf`/`tauri:build:direct-release` |
| `.github/workflows/ci.yml` | 私有仓提交门控（rust/rust-linux/frontend/smoke 四 job） |
| `.github/workflows/release.yml` | **公开仓** OSS 免费版发布流水线（文件名易误导，内容是 `oss-release`） |
| `.github/workflows/oss-gate.yml` | 公开镜像侧构建+测试门（Copybara 投影后跑） |
| `.github/workflows/sync-oss.yml` | 私有→公开镜像同步（Copybara） |
| `crates/scrollery-plugin-api` | `EntitlementProvider` trait + DTO（叶 crate） |
| `crates/scrollery-exotic-trust` | Ed25519 验签原语 + `verify_token`/`evaluate_token`（License 纯函数） |
| `src-tauri/src/exotic/license.rs` | 授权 provider 实现：keyring 直销 `KeyringLicenseStore`（`:43`）+ 未授权回退 `FreeStubEntitlement`（`:137`） |
| `src-tauri/src/exotic/mod.rs::default_entitlement_provider`（`:249`） | 组合根：授权 Provider 装配单点（keyring 直销；keyset 解析失败降级 fail-closed 回退桩） |
| `src-tauri/src/editing/entitlement.rs` + `ipc/edit_commands.rs` | 图片编辑插件的授权查询/激活/撤销命令 |

**在整机中的位置**：

```
Cargo feature 矩阵 ──决定──> 编译进哪些代码/依赖(体积变体与合规隔离)
      │
      ▼
cargo build / tauri build ──产出──> 平台安装包(msi/nsis/dmg 等，signCommand 待落)
      │
      ├─ CI(ci.yml，私有仓，自托管 runner)：rust/rust-linux/frontend/smoke 四门
      │     └─ smoke 仅 tag/dispatch 触发：--no-bundle 开机冒烟
      ├─ oss-release(公开仓 release.yml)：免费版 tauri build → gh release
      └─ commercial-release(计划，Part7 §7 阶段E)：注入 pro → 签名 → 公证 → 直销发布(待核实/未落地)
      │
      ▼
运行时：EntitlementProvider(keyring 直销；keyset 失败降级 fail-closed 回退) ──验证──> License token(keyring 存储) ──放行──> 插件/编辑功能
```

---

## 2. 数据模型/配置面

### 2.1 Cargo.toml features 矩阵（`src-tauri/Cargo.toml:245-286`，动笔前已回验行号）

| Feature | 类型 | 定义 | 说明 |
|---|---|---|---|
| `default` | — | `["custom-protocol", "lite"]`（`:248`） | 未加 `--no-default-features` 时的隐式组合 |
| `custom-protocol` | 基础 | `["tauri/custom-protocol"]`（`:249`） | Tauri 生产构建必需 |
| `devtools` | 诊断 | `["tauri/devtools"]`（`:252`） | Release 默认无 inspector，按需显式开启 |
| `exotic-dev-fixtures` | 开发桩 | `[]`（`:257`） | 仅配合 `debug_assertions` 双门控生效，注入测试授权跳过真实 License |
| `lite` | 变体 | `[]`（`:262`） | 极致轻量，默认变体；Media Foundation 视频后端 lite/perf 两变体都在（`#[cfg(windows)]`） |
| `perf` | 变体 | `["ffmpeg", "netfs"]`（`:263`） | lite + FFmpeg sidecar + WebDAV 原生 |
| `ffmpeg` | perf 子项 | `[]`（`:266`） | FFmpeg sidecar 占位 feature（**待核实**：`installer.rs`/`installer` 层 glue 尚未实现，仅建立 feature 边界，见 `:265` 注释「P2 暂为占位」） |
| `netfs` | perf 子项 | `["dep:reqwest_dav"]`（`:269`） | WebDAV 原生 VFS，lite 不编入（依赖 OS 挂载/UNC） |
| `face-noncommercial` | 合规隔离 | `["scrollery-ai-core/face-noncommercial"]`（`:277`） | SCRFD/ArcFace 非商用轨，仅研究/自用构建显式开启 |

上表**不含任何渠道 feature**：`channel-direct`/`channel-msstore`/`channel-steam` 已全部删除（2026-09-15 消融 P16），`tauri-plugin-updater = "2"` 因此是**普通依赖**（不再 optional、不再绑定 feature）。

### 2.2 tauri.conf.json 三件套

| 文件 | 作用 | 关键字段 |
|---|---|---|
| `src-tauri/tauri.conf.json` | 基座（所有构建共用） | `productName: "Scrollery"`、`identifier: "com.scrollery.app"`、`app.security.csp`（单一字符串，无平台分档，见 §3.2/§4.2）、`app.security.assetProtocol.scope: ["$APPDATA/**"]`、`bundle.targets: "all"` |
| `src-tauri/tauri.direct-release.conf.json` | direct 渠道 overlay（`-c` 传入合并，Tauri 的 conf 合并**只能加/改，不能删**） | `plugins.updater.pubkey`（minisign 公钥 base64） + `endpoints`（`https://updates.scrollery.invalid/{{target}}/{{arch}}/{{current_version}}`，占位域名） + `bundle.createUpdaterArtifacts: true` |
| `src-tauri/tauri.perf.conf.json` | perf 变体 overlay | 当前**与基座一致**（纯占位，注释注明待 `pdfium`/`ffmpeg` 落地后补 `bundle.externalBin`/`bundle.resources`） |

`package.json` 对应三条构建脚本（`package.json:17-19`）：`tauri:build:lite`（`tauri build`，纯默认）、`tauri:build:perf`（`--features perf -c tauri.perf.conf.json`）、`tauri:build:direct-release`（`-c tauri.direct-release.conf.json`，不带 `--features`，因直销就是默认构建）。

### 2.3 渠道 cfg 门控：已全部退役

**源码中已无任何渠道 `#[cfg]`**：`ipc/exotic_commands.rs` 的 `install_exotic_plugin` 恢复单一定义（无 `channel_unsupported` 桩）；`exotic/mod.rs::default_entitlement_provider` 无渠道分叉（恒 keyring 直销，keyset 解析失败降级 fail-closed 回退桩，§3.6）；`fetch.rs`/`license.rs` 的渠道门控与 `installer.rs` 的交付来源参数同批移除。`lib.rs` 的 updater 注册保留运行期判据——编译进 context 的最终配置是否带 `plugins.updater`（§3.5）。

---

## 3. 关键流程与算法

### 3.1 构建变体（lite / perf）

```
lite（默认）= custom-protocol + lite
perf        = lite + ffmpeg(占位) + netfs(WebDAV 原生)
```

- `lite` feature 本身为空 gate（`[]`），视频后端（Media Foundation）在两变体下都编译（`#[cfg(windows)]` 而非 `#[cfg(feature = "lite")]`）——lite/perf 分野目前只体现在 `ffmpeg`/`netfs` 两个子 feature 上。
- `perf` 当前唯一确定落地的差异是 `netfs`（`dep:reqwest_dav`，编入 `StorageBackend::WebDav`，见 [Spec08 存储备份导出文件操作](./Spec08_存储备份导出文件操作.md)）；`ffmpeg` 仅建立 feature 边界，glue 代码**待核实未见**（`Cargo.toml:267` 注释「P2 暂为占位」）。
- `BUILD_VARIANT` 常量（`lib.rs:62-66`）在运行时把 `cfg!(feature = "perf")` 结果暴露给前端/埋点，供 UI 显示变体角标。

### 3.2 CSP 生产分档（交叉引用）

`app.security.csp` 全平台唯一一份配置（`tauri.conf.json:44`），当前**未按平台分档**，也不含 `tauri:` scheme——这是「规范定分档、实现未落」的已知 gap，详见 [Spec14 不变量与约定 §4.7](./Spec14_不变量与约定.md#47-csp-平台分档--规范定分档现状未落as-built-gap)（本篇不重复该节全文，只指认现状：项目仅 Windows 目标线在实跑，macOS/iOS/Linux 分档条款尚无落地对象验证）。开发态 CSP 由 `scripts/vite-plugin-dev-csp.mjs::deriveDevCsp()` 从生产 CSP 派生（追加 `ws:` 到 `connect-src`），单一事实源，不进生产构建。

### 3.3 发布渠道：唯一值 direct

**现状**：唯一发布渠道 = 直销（direct）。`src-tauri/src/lib.rs` 顶部原有的两段渠道 `compile_error!` 互斥守卫已删（2026-09-15 消融 P16）——空渠道 `channel-msstore`/`channel-steam` 一并退役后不再存在「选错渠道」的组合，互斥门失去守卫对象，正是该门与空渠道同批退役的原因。

**渠道 → 授权/更新**：

| Channel | 授权 | 更新 | Provider 装配位置 |
|---|---|---|---|
| 直销（direct，唯一） | License token（`LicensePayload` + `verify_token`/`evaluate_token`，`crates/scrollery-exotic-trust/src/license.rs`）| `tauri-plugin-updater`（普通依赖；无 overlay 的 dev/普通 build 不带配置块故不注册） | `exotic/mod.rs`：`license::KeyringLicenseStore`（`license.rs`，keyset 解析失败则降级 `license::FreeStubEntitlement`） |

**授权判定折叠**逻辑（`ExoticHost::availability_of` 全量流程）详见 [Spec09 插件平台与exotic §3.2](./Spec09_插件平台与exotic.md)，本篇只讲渠道选择本身；`activate_exotic_plugin`/`deactivate_exotic_plugin`（`ipc/exotic_commands.rs`，`ipc/registry.rs:131-132`）与图片编辑插件专用的 `activate_editing_feature`/`deactivate_editing_feature`（`ipc/edit_commands.rs:82-116`，`ipc/registry.rs:108-109`）都经同一 `EntitlementProvider::activate`/`deactivate` trait 方法，plugin id/SKU 是后端可信常量（如 `EDITING_PLUGIN_ID = "feature-editing"`、`EDITING_SKU = "editing-tools-2026"`，`editing/entitlement.rs:10,12`），前端只提交 token 字符串。

`updater` 插件的**条件注册**（`lib.rs:133-137`）：

```rust
let builder = if tauri_context.config().plugins.0.contains_key("updater") {
    builder.plugin(tauri_plugin_updater::Builder::new().build())
} else {
    builder
};
```

原因：`plugins.updater` 配置块只存在于 `tauri.direct-release.conf.json` overlay（`plugins.pubkey` 必填，插件 init 对缺配置硬失败）；普通 `tauri dev`/`tauri build`（无 overlay）不带该块，故不注册——`tauri dev` 与日常 build 不受影响，正式 direct 发布须走 `tauri:build:direct-release` 脚本（`package.json:19`）。基座 conf 保持天生干净（`lib.rs:124-129` 注释）由 `verify-channel-bundle.mjs` 双向断言守卫。

### 3.4 CI（自托管 runner，`.github/workflows/ci.yml`）

触发：`push`（`main`/`dev` 分支 + `v*` tag）、`pull_request`、`workflow_dispatch`（`ci.yml:14-20`）。四 job（均 `if: github.repository == 'gfgjs/scrollery-private'` 防公开仓误跑）：

| Job | Runner | 核心步骤 |
|---|---|---|
| `rust` | `[self-hosted, Windows, X64]` | `cargo fmt --check` → `cargo check --workspace --locked` → `cargo clippy --workspace --locked -- -D warnings` → `cargo test --workspace --locked` → NOTICE 归属清单新鲜度门（`generate-notice.mjs --check`） |
| `rust-linux` | `[self-hosted, Linux, X64]` | `cargo check`/`cargo test --workspace --locked`（守住 `cfg(windows)` 门控成果；无 npm 步骤故推理相关测试须 `#[ignore]`，见 `ci.yml:141` 注释） |
| `frontend` | `[self-hosted, Windows, X64]` | 版本单源检查（`sync-version.mjs --check`）→ 改名门禁 → 路径卫生门禁 → exotic 协议版本同步门 → 主题对比度门禁 → ESLint → vue-tsc → vitest → `npm run build` → **发布合规扫描**（`verify-channel-bundle.mjs`，扫 minify 后生产 bundle：基座 conf 无 updater 块 + direct overlay 配置完整 + capability/dist 无 updater 泄漏） |
| `smoke` | `[self-hosted, Windows, X64]`，仅 tag `v*`/手动 dispatch | `npx tauri build --no-bundle` → 设 `PICASA_SMOKE_TEST=1` 跑 exe，`RunEvent::Ready` 分支检测该环境变量即 `exit(0)`；setup 中途 panic（DB 迁移/reactor 等）永不到 Ready，冒烟即红 |

三个重 job 走本机自托管 runner（`dev-box-win`）省 Windows 2× 计费倍率；`rust-linux` 亦已迁自托管（WSL，`ci.yml:114` 注释）；`runner.environment == 'github-hosted'` 判据保留托管 fallback 逻辑（rust-cache/系统依赖安装只在托管环境跑）。

**公开镜像侧**另有 `oss-gate.yml`（对 Copybara 投影后的公开树跑 `cargo check/test --workspace --locked` + gitleaks 密钥扫描，见 [Spec09 §3.7](./Spec09_插件平台与exotic.md)）。源码检查 job 用 `TAURI_CONFIG` 清空 bundle 的 externalBin/resources，发行资源在打包流程校验；`sync-oss.yml` 仅在私有仓执行同步。

### 3.5 updater（直销发布面）

```
direct-release 构建 ──携带 plugins.updater 配置块──> tauri_context.config() 含 "updater" key
                                                          │
                                          lib.rs:133-137 条件注册 tauri_plugin_updater
                                                          │
                                          运行时 app.updater() API（Rust 侧驱动，无前端 ACL/无 JS 包）
                                                          │
                                          endpoints: https://updates.scrollery.invalid/{target}/{arch}/{version}
                                                          │（占位域名，待核实：生产 endpoint 未落）
                                          minisign 签名校验（pubkey 见 tauri.direct-release.conf.json:5）
```

设计取舍（`lib.rs:118-128` 注释）：现无更新 UI 消费者，故不开放 webview ACL 权限、不引前端更新相关 npm 包——IPC 面越小越好；Part8 落地更新 UI 时再定「Rust 驱动 + 事件通知」或「JS 驱动 + capability」两种形态之一。签名私钥/生产 endpoints 属敏感材料，`tauri.direct-release.conf.json` 内当前 pubkey/endpoint 域名（`updates.scrollery.invalid`）为占位——**待核实**：正式密钥轮换/真实域名尚未见落地记录。

### 3.6 许可基建与激活（License token 验签、编译期 keyset gate）

**验签链**（`crates/scrollery-exotic-trust/src/license.rs`）：

- `verify_token(token, keyset, plugin_id, sku, now)`（`:82`）：解析 `LicensePayload`（含 `not_before`/`expires_at`），核对 `plugin_id`/`sku` 匹配、时间窗（`now < payload.not_before` 即拒绝，`:149`）、签名合法（`VerifyingKeyset::verify_any`，**`KeyPurpose::License`**，与签 Registry/包的 `KeyPurpose::Release` 用途分离，`:128`）。
- `evaluate_token(token_opt, keyset, plugin_id, sku, now)`（`:161`）：`None` → `LicenseStatus::Unlicensed`；`Ok` → `Authorized`；`Err(Expired)` → `Expired`；其余 `Err` → `Unlicensed`（不区分具体失败原因，防信息泄漏给前端）。

**Provider 装配**（组合根单点，`exotic/mod.rs::default_entitlement_provider`）：

```
唯一渠道 = direct：
  keyset 解析成功 → license::KeyringLicenseStore::new(trusted_keyset())（验签 + OS keyring token 存储）
  keyset 解析失败 → license::FreeStubEntitlement（fail-closed，恒 Unlicensed，绝不放行）
```

**Token 存储**：OS keyring（`service="scrollery"`，`scrollery-plugin-api::KEYRING_SERVICE`），**DB 不存 token**——与 exotic 插件 License 共用同一存储面。`activate`/`deactivate` 在 `EntitlementProvider` trait 默认 fail-closed（`ActivationUnsupported`），仅真实渠道实现（direct keyring 直销）覆写；两者当前签名均为 `Result<(), LicenseError>`，**无 payload**——`ActivationInfo`/`enc_seed` 预留已删（2026-09-15 消融 P10），keyring/token 的敏感信息保护不变。

**编译期 keyset gate**：生产验签公钥集编译期内置（`trusted_keyset()`），无签名私钥入仓——**验签公钥可公开**（Part0 §10）。`PICASA_EXOTIC_KEYSET_FILE` 为**整组替换**式注入；本次归一保留既有 key ID、公钥、用途和有效期，轮换时须保留需继续信任的历史键。

### 3.7 打包与签名

**当前落地**：`bundle.targets: "all"`（`tauri.conf.json:53`），图标资源 `icons/*.png`+`.icns`+`.ico`；`bundle.createUpdaterArtifacts: true`（direct overlay）。

**未落地（计划稿，Part7 §3.4/§7 阶段E）**：`tauri.conf.json`/`tauri.perf.conf.json` 均**无** `signCommand`/`externalBin`/`codesign` 相关字段（已 grep 确认，`src-tauri/tauri*.conf.json` 无此类键），即 Windows Authenticode 签名、macOS codesign + notarize + staple、perf 变体 FFmpeg sidecar 的 `bundle.externalBin` 均未接线。详见 §5 边界与失败。

---

## 4. 契约与不变量

### 4.1 四层防护框架（Part0 §8 理由链）

复用 exotic Ed25519 框架的四层防破解纵深，**已完成层0（`installer.rs`/`registry.rs`/`license.rs` 的验签+防回滚+完整性复核，162 测试绿）**，层1（加密权重）/层1b（设备绑定）/层2（可选一次性在线激活）均为 **🔨 待做**（Part0 §8.2 状态列原样标注），且**当前代码无加密权重契约预留**——`ActivationInfo`/`enc_seed` 已随 dormant 预留删除（2026-09-15 消融 P10），AES 解密原语（`ring::aead`）全仓未实现、零调用，层1 实装时重新定契约。诚实强度评级（Part0 §8.3）：挡住 99% 脚本小子/keygen/共享；挡不住有动机的逆向工程师 patch、内存 dump、VM 快照克隆——这是纯本地离线软件防破解的共同上限，云端校验/kill-switch 已明确否决（与完全离线+隐私第一冲突）。理由链详见 [Part0 §8](../refactor_2026/Part0_总纲与产品定稿.md)，本篇不复制其论证表格，只指认「层0 已落地、层1/1b/2 未落地」这一 as-built 现状。

### 4.2 CSP 生产分档（现状全平台统一未落分档）

见 §3.2；权威条目在 [Spec14 §4.7](./Spec14_不变量与约定.md#47-csp-平台分档--规范定分档现状未落as-built-gap)。**为什么这是不变量而非纯 bug**：AGENTS.md 硬约束要求 macOS/iOS/Linux 构建须含 `tauri:` scheme，但项目当前仅 Windows 目标线在实跑——分档条款尚无「该平台真实构建」这一验证对象，故现状是「规范先行、实现按目标线开工顺序补齐」，不是遗漏后未修的缺陷。若日后误读成「已分档」会导致 macOS 首次构建时 CSP 缺 `tauri:` scheme 而资源加载失败。

---

## 5. 边界与失败

| 边界/失败 | 现状 |
|---|---|
| 平台签名（Windows Authenticode / macOS codesign+notarize+staple） | **未落地**：`tauri.conf.json`/`tauri.perf.conf.json` 均无 `signCommand`/`externalBin` 字段（已 grep 确认）；Part7 §3.4 有完整签名产线设计（EV 证书 + inside-out 逐级签 + worker sidecar 单独签名再装入 `.ppx`），标注为「私有 commercial-build」阶段任务，**未见落地记录** |
| macOS 构建矩阵 | **未完成**：Part7 §7 阶段E（T13-T18）多数任务标「需证书，私有 CI」；本仓无 macOS CI job（`ci.yml` 仅 `rust`(Windows)/`rust-linux`(Linux)/`frontend`(Windows)/`smoke`(Windows) 四 job，无 macOS runner） |
| 私有 commercial-release 流水线 | **待核实/疑未落地**：Part7 §7 阶段E task 5 提及「私有仓 `.github/release.yml`」，本次核实仅见公开侧 `.github/workflows/release.yml`（即 oss-release），未见私有专属 release workflow 文件；`ci.yml` 的 `smoke` job 仅做开机冒烟，非完整发布流水线 |
| `ffmpeg` feature | 仅建立 feature 边界，glue 代码待核实未见（`Cargo.toml:267` 注释自陈「P2 暂为占位」） |
| updater 生产 endpoint | `tauri.direct-release.conf.json` 内 `updates.scrollery.invalid` 域名格式为占位（`.invalid` TLD 保留用于示例，非可解析域名）；pubkey 是否为最终生产密钥待核实 |
| 更新失败（直销运行时） | **待核实**：本次核实未见前端更新 UI 消费者或失败提示的具体呈现路径（`lib.rs:118-123` 注释「现无更新 UI 消费者」自陈这一空白），更新检查走 `app.updater()` Rust API，失败如何呈现给用户尚无代码路径可指 |
| Release 双轨的隔离 | oss-release（公开仓）与 ci.yml smoke（私有仓）物理隔离，`if: github.repository == '...'` 双向 guard；oss-release 不使用任何签名凭据（仅 `github.token`），是「零凭据、零签名」的诚实发布，非缺陷 |

---

## 6. 重建指引

### 6.1 构建命令

```bash
# lite（默认，直销）
cargo build --release
npm run tauri:build:lite

# perf（直销 + WebDAV 原生）
cargo build --release --features perf
npm run tauri:build:perf

# direct 正式发布（携带 updater overlay）
npm run tauri:build:direct-release
```

### 6.2 feature 组合速查

| 目标 | Cargo feature 组合 |
|---|---|
| 直销 lite（默认） | `custom-protocol,lite`（隐式） |
| 直销 perf | `custom-protocol,lite,perf` |
| 非商用人脸研究构建 | 直销构建 + `face-noncommercial` |
| 开发期 exotic 端到端（无真实 License） | 任一 debug 构建 + `exotic-dev-fixtures`（**必须**配合 `debug_assertions`，Release 即使开启也无跳过授权的代码路径） |

### 6.3 CI 复现

本地无法完整复现自托管 runner 环境，但可单独跑 CI 同款命令验证：

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
node scripts/generate-notice.mjs --check
npm run lint && npm run typecheck && npm test
npm run build && node scripts/verify-channel-bundle.mjs
```

### 6.4 验收

- CI：`.github/workflows/ci.yml` 四 job 全绿（rust/rust-linux/frontend 常态跑；smoke 仅 tag/dispatch）。
- 发布：oss-release 走 `workflow_dispatch` dry-run 验证全链（构建+校验+产物上传，不创建 release）。
- **待核实**：本次摸底未逐条重跑上述命令验证通过（属撰写任务，不做施工验证），以最近一次门禁记录为准。

### 6.5 坑与教训（链 experience.md）

- Cargo feature 是叠加语义，「互斥」纯靠文档约定不可靠——多渠道路径真出现时须用编译期 `compile_error!` 强制；空渠道 msstore/steam 与那两道互斥门已于 P16 随渠道维度一并退役（没有渠道组合可组合时，守卫本身也是维护面）。
- workspace `members` 禁用 `crates/*` glob：会命中无 `Cargo.toml` 的目录且只展开一层，漏掉两层深的 worker crate（详见 [Spec09 §6.3](./Spec09_插件平台与exotic.md)，同一教训跨篇复用不重复摘）。
- Tauri 的 conf overlay 合并只能加/改不能删——updater 配置块必须放在 direct-only overlay，基座必须天生干净，否则 dev/无 overlay 构建会因基座带块而误注册 updater（`lib.rs:124-129` 注释）。
- 更多跨子系统通用教训见 `docs/experience.md`（工具/门禁/正则扫描类，非本篇专属）。

---

## 7. 商业化

### 7.1 模式

**核心免费 + 插件买断**：四媒体基础浏览（Win+mac）永久免费无试用期限制；AI 分析/人脸聚类/冷门格式三插件各自一次买断，可打包购买。理由链见 [Part8 §7.1-7.2](../refactor_2026/Part8_商业化与分发.md)（本篇不复制定价论证表格）。

**⚠️ 定价——计划/未定案**：Part0 §7.2/Part8 §3.6 所列具体数字（如 $12.9/¥89 等）均是**产品规划阶段的基准价**，未见任何签发端/支付网关/上架记录佐证其已成交生效——本篇如实标注为「计划」，不作为既定事实复述。已核实为 as-built 的部分：`EDITING_PLUGIN_ID`/`EDITING_SKU` 等 plugin id/SKU 字符串常量确已写入代码（`editing/entitlement.rs:10,12`），但这只是**授权判定用的稳定标识符**，不代表定价/支付链路已落地。

**已核实为计划中而非已落地**：签发基础设施（License 签发服务端）、支付渠道（FastSpring/Paddle、微信/支付宝）、商店上架（MS Store/Steam 商品页；代码侧渠道分支已于 2026-09-15 消融 P16 删除，当前仅直销）、层1 加密权重/层1b 设备绑定/层2 在线激活——均见 Part8 §2/§3 现状实测栏标注「零落地」或「🔨 待做」。

### 7.2 官网

`scrollery.app` 四页静态站已建成（据 [2026-07-17-官网纯静态无后端拍板.md](../decisions/2026-07-17-官网纯静态无后端拍板.md)，独立仓 `scrollery-site`，与本仓不同 git 树，本篇不核实其源码——as-built 状态以该决策文档「已按此落地（`b97ad36`）」的记录为准）：纯静态 HTML/CSS/JS，零框架零构建零跟踪，联系方式 `contact@scrollery.app`（只收不发）。技术取舍：内容型站点 SEO 优先，Vue SPA 需 SSR/预渲染才有等效收益，纯增复杂度；后端需求（表单/下载计数/newsletter/许可签发）逐项检视后均有无后端解法或明确划给产品侧服务（许可签发属 Part8，不属官网）。

---

## 8. 关联

- 上游正典：[Part7 发布工程](../refactor_2026/Part7_发布工程.md)（§2 现状实测一手取证、§3 设计方案 R1-R8、§7 实施提示词分阶段——渠道 cfg/CI/签名产线的架构决策论证在此，本篇不复制）+ [Part8 商业化与分发](../refactor_2026/Part8_商业化与分发.md)（§2 现状实测、§3 设计方案 C1-C8、§6 验收标准——定价/支付/上架的完整规划，本篇只摘「计划 vs 已落地」判据）+ [Part0 总纲与产品定稿](../refactor_2026/Part0_总纲与产品定稿.md)（§7 盈利定价基准、§8 防破解四层纵深理由、§9 分发渠道抽象设计）。
- 许可/exotic 授权机制细节（三份真相、Availability 九态、Registry 验签+防回滚、任务队列状态机）：[Spec09 插件平台与exotic](./Spec09_插件平台与exotic.md)（本篇只讲渠道/构建/发布，不讲 exotic 平台内部机制）。
- 编译门控总目录（CSP 分档 gap、`cfg(windows)` 门控理由等横向不变量索引）：[Spec14 不变量与约定](./Spec14_不变量与约定.md)。
- 进程模型/整机架构：[Spec00 产品与架构全景](./Spec00_产品与架构全景.md)。
- 官网决策记录：[2026-07-17-官网纯静态无后端拍板](../decisions/2026-07-17-官网纯静态无后端拍板.md)。
