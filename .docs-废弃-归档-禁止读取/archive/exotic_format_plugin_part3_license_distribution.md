# 冷门格式插件子系统 · Part 3 商业 | License & Distribution（P5–P6）

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic v2 分卷,被 v3.1 同名分卷取代(见同目录 exotic_format_plugin_plan/)。

> 本卷范围：**Ed25519 离线 license 验签 + 插件包签名/完整性 + 远程注册表发现 + 下载/解包/安装/激活**。
> 落完即可：购买→下载→激活的完整商业闭环；门控从 Part1 的桩换成真实 license 校验。
>
> 配套：总纲 v2（§5 决策 D2/D4、§7 许可、§9 风险 R1/R2/R6）+ Part1/Part2（已完成）。

---

## 本卷前置依赖 | Prerequisites

- **Part1 + Part2 完成**（host 路由、pipeline、dev_mode 端到端跑通）。
- 新增依赖：`ed25519-dalek`（验签）；`zip` 已在（解包）；`keyring` 已在（token 存储）；`reqwest`/`download_assets` 已在（下载）。
- 核验 `ipc/ai_commands.rs:958 download_assets`（断点续传 + sha256 + 镜像 + 进度 Channel）签名，下载直接复用。
- 核验 `ai/remote_registry.rs`（在线发现 + 10min 缓存 + 离线回退），注册表照此模式。

## 本卷完成定义 | DoD

1. 厂商侧签发工具产出 license token + manifest.sig；主程序内置公钥能验签。
2. `install_exotic_plugin`：下载包 → 校验 sha256 → 验 manifest.sig → 解包到 `plugins/<id>/`。
3. `activate_exotic_plugin`：验 license token → 存 keyring → host 刷新标 `authorized`。
4. 启动 worker 前**再校验** worker 二进制 sha256（防安装后替换，R2）。
5. `list_exotic_registry`：在线拉远程目录、离线回退本地已装，供前端插件市场。
6. 关闭 `exotic_dev_mode` 后，未授权 psd → 门控 NeedsPurchase；激活后 → Authorized → 自动处理。

---

## Phase 5 — License + 签名 + 包完整性 | License, signatures, integrity

### 5.1 三道安全闸 | Three security gates（R2，下载执行第三方二进制是最大安全面）

1. **传输完整性**：注册表清单含每文件 sha256 + size；下载后逐一校验（复用 `download_assets` 的 `sha256_matches`）。
2. **来源真实性**：`manifest.sig` = 厂商私钥对 `manifest.json` 的 Ed25519 签名；install 时用内置 `VENDOR_PUBKEY` 验签。**manifest 内记录各 worker 二进制 sha256**，故验签 manifest 即间接锁定二进制内容。
3. **执行前再校验**：每次启动 worker 前重比磁盘 worker sha256 与 manifest 记录值（防安装后被替换）。

> 即 manifest.json 须扩展记录 `bin_hashes`（target triple → sha256），供闸 3 使用。Part1 的 Manifest 结构在此补该字段（`schema` 升 2 或同 schema 加可选字段）。

### 5.2 License 校验 | `exotic/license.rs`（Ed25519 离线，D4）

**离线验签**（非在线激活服务器）：桌面应用、零运维、可断网、用户已选"独立插件包下载"。

- **签发（厂商侧，离线）**：Ed25519 私钥对 license 载荷签名 → token（base64）。私钥**绝不入库/入仓**。
- **校验（主程序侧）**：内置厂商**公钥**（编进二进制），验签 + 校 `plugin_id` 匹配 + 未过期。
- **存储**：通过的 token 存 **keyring**（service=`picasa-next-license`, account=`<plugin_id>`），不落明文 DB（复用 proofread keyring 先例）。

```rust
// src-tauri/src/exotic/license.rs
//! 离线 license 校验 | Offline license verification（Ed25519 公钥内置）。
use ed25519_dalek::{VerifyingKey, Signature, Verifier};

/// 厂商公钥（编译期内置）。私钥仅在签发侧，绝不入库。
/// keys/vendor_ed25519.pub 须在仓库（公钥可公开）；私钥离线保管。
const VENDOR_PUBKEY: &[u8; 32] = include_bytes!("../../keys/vendor_ed25519.pub");

#[derive(serde::Deserialize)]
pub struct LicensePayload {
    pub sku: String,
    pub plugin_id: String,
    pub subject: String,        // 购买者邮箱/订单号哈希
    pub issued_at: i64,
    pub expires_at: Option<i64>,// null=永久；非空=订阅到期
}

#[derive(thiserror::Error, Debug)]
pub enum LicenseError {
    #[error("license 缺失 | missing")] Missing,
    #[error("签名无效 | bad signature")] BadSignature,
    #[error("插件不匹配 | plugin mismatch")] PluginMismatch,
    #[error("已过期 | expired")] Expired,
    #[error("格式错误 | malformed")] Malformed,
}

/// 校验 token 对某插件是否构成有效授权。失败原因细分，便于前端区分。
pub fn verify(token: &str, plugin_id: &str) -> Result<LicensePayload, LicenseError> {
    // 1) base64 解码 → 分离 payload(JSON) 与 64 字节 Ed25519 签名
    // 2) VerifyingKey::from_bytes(VENDOR_PUBKEY).verify(payload_bytes, &sig)  防伪造
    // 3) payload.plugin_id == plugin_id  防张冠李戴
    // 4) expires_at 为空或 > now  防过期
    todo!()
}

/// keyring 存取 | keyring get/set。
pub fn store_token(plugin_id: &str, token: &str) -> Result<(), LicenseError> { todo!() }
pub fn load_token(plugin_id: &str) -> Option<String> { todo!() }
```

### 5.3 license token 载荷 | Payload（签名前）

```jsonc
{ "sku": "psd-engine-2026", "plugin_id": "exotic-image-psd",
  "subject": "<购买者邮箱/订单号哈希>", "issued_at": 1750636800,
  "expires_at": null }          // null=永久授权；非空=订阅到期
```

### 5.4 签发工具 | `tools/sign_plugin.rs`（厂商侧离线）
一个独立小工具（或 example）：① 用私钥对 manifest.json 签名产 manifest.sig；② 用私钥对 license 载荷签名产 token。私钥从环境/文件读，**绝不入仓**。配套生成密钥对的一次性脚本（`ed25519_dalek::SigningKey::generate` → 公钥写 `keys/vendor_ed25519.pub` 入仓、私钥离线存）。

### 5.5 防破解的诚实边界 | Honest anti-piracy boundary（O3/R6）
离线验签能挡"无 key 直接用"与"篡改 token"，但挡不住"逆向去掉校验调用"（任何本地校验皆然，商业软件通病，可接受）。增量手段（v2 可选，**v1 切片不做**）：license 绑机器指纹；worker 启动需主程序传入 license 派生密钥；关键解码逻辑下沉 worker。

### 5.6 验收
- 用签发工具产 token → `verify` 通过；改一字节 token → `BadSignature`；换 plugin_id → `PluginMismatch`；造过期 → `Expired`。
- token 存 keyring 后重启仍可 `load_token` 取回。

## Phase 6 — 注册表 + 下载/安装/激活 | Registry, download, install, activate

### 6.1 三段流程 | Buy → Download → Activate

```
①购买（外部商店/官网，O2）        ②下载插件包               ③激活验证
用户付款 → 收到 license token     list_exotic_registry      install_exotic_plugin
（含 sku+subject+签名）         → 远程目录拉清单            → 校验 sha256 + manifest.sig 验签
                               → download_assets 下载包    → 解压到 plugins/<id>/
                                                           activate_exotic_plugin
                                                           → license::verify（公钥验签）
                                                           → 存 keyring → host.refresh 标 authorized
```

### 6.2 插件包结构 | Package layout

下载包（zip）解开后落 `{app_data}/plugins/<plugin_id>/`：

```
{app_data}/plugins/exotic-image-psd/
├── manifest.json          # 清单（含 bin_hashes，§5.1 闸 3 用）
├── manifest.sig           # 厂商私钥对 manifest.json 的 Ed25519 签名
├── bin/
│   ├── x86_64-pc-windows-msvc/psd-worker.exe
│   └── aarch64-apple-darwin/psd-worker
├── icon.svg               # 插件市场/占位角标图标
└── LICENSE-3RD-PARTY.txt  # 第三方解码库许可（§7 合规留痕）
```

### 6.3 远程注册表 | `exotic/registry.rs`（仿 `ai/remote_registry.rs`）

远程 JSON 目录（HF/自有 CDN），列可购买/可下载插件：

```jsonc
// 远程目录 exotic-plugins/index.json
{ "schema": 1,
  "plugins": [
    { "id": "exotic-image-psd", "name": "PSD 图像引擎", "version": "1.0.0",
      "media_kind": "image", "formats": ["psd","psb"],
      "license_tier": "paid", "price_hint": "¥XX", "store_url": "https://…",
      "size_bytes": 7340032,
      "package_url": "https://…/exotic-image-psd-1.0.0.zip",
      "package_sha256": "…",
      "icon_url": "https://…/psd.svg" } ] }
```

```rust
// exotic/registry.rs
/// 拉远程目录 + 合并本地已装状态。发现失败 → 离线回退"仅本地已装"（仿 list_model_registry online:false）。
pub async fn discover(state: &AppState) -> Result<Vec<RegistryEntry>, AppError> { todo!() }
```

### 6.4 安装/激活命令 | `ipc/exotic_commands.rs` 追加

```rust
/// 下载并安装（不激活）：下载 zip → sha256 → 解包 → 验 manifest.sig → 写 exotic_plugins 表。
/// 复用 ai_commands::download_assets（断点续传 + 进度 Channel）。
#[tauri::command]
pub async fn install_exotic_plugin(plugin_id: String, on_progress: tauri::ipc::Channel<…>,
    /* state */) -> Result<(), String> { todo!() }

/// 激活：license::verify(token, plugin_id) → store_token(keyring) → host.refresh → authorized=1。
#[tauri::command]
pub async fn activate_exotic_plugin(plugin_id: String, license_token: String,
    /* state */) -> Result<(), String> { todo!() }

/// 卸载：删 plugins/<id>/ + exotic_plugins 行 + host.refresh（保留 keyring token 以便重装免再激活，或一并清，待 O 决策）。
#[tauri::command]
pub async fn uninstall_exotic_plugin(plugin_id: String, /* state */) -> Result<(), String> { todo!() }

/// 注册表发现（前端插件市场）。
#[tauri::command]
pub async fn list_exotic_registry(/* state */) -> Result<Vec<RegistryEntry>, String> { todo!() }
```

### 6.5 启动时门控真值化 | Wire real license at startup
Part1 的 `host.refresh` 授权集本卷换真值：对每个已装插件 `license::load_token(id)` → `license::verify` → 通过则入 `authorized` 集 + 写 `exotic_plugins.authorized=1`。`exotic_dev_mode=1` 仍可短路为全授权（开发期）。

### 6.6 worker 启动前再校验（闸 3 接线）
`exotic/worker.rs::spawn`（Part2）在 `Command::spawn` 前，比对磁盘 worker sha256 与 manifest `bin_hashes[triple]`；不符 → 拒启 + 标插件异常 + 提示重装。

### 6.7 验收
- 本地起一个静态 JSON 目录模拟远程 → `list_exotic_registry` 列出 psd 插件。
- `install_exotic_plugin` 下载（可用 file:// 或本地 http）→ sha256 + 验签通过 → 解包到位。
- `activate_exotic_plugin` 带有效 token → host 标 authorized → 关 dev_mode 后 .psd 自动处理出缩略图。
- 篡改包内 worker 字节 → 启动前 sha256 校验拦截。

---

## 新会话续作提示词 | Continuation prompt（Part 3）

```
任务：实施 plan-docs/exotic_format_plugin_part3_license_distribution.md（冷门子系统·商业 P5–P6）。
先读：总纲 v2（§5 D2/D4、§7 许可、§9 R1/R2/R6）+ Part1/Part2（确认已完成）+ 本卷全文。
前置依赖：Part1+Part2 完成（host 路由 / pipeline / dev_mode 端到端）。
新增依赖：ed25519-dalek（Cargo.toml）。zip/keyring/reqwest 已在。
必看源码（照其模式）：
  - src-tauri/src/ipc/ai_commands.rs:958 download_assets（下载复用，带进度 Channel）
  - src-tauri/src/ai/remote_registry.rs（在线发现 + 10min 缓存 + 离线回退）
  - src-tauri/src/proofread/mod.rs（keyring 存取先例）
施工顺序：P5 license.rs（verify/keyring）+ 签发工具 + manifest 补 bin_hashes → P6 registry → install/activate/uninstall 命令 → 启动门控真值化 → worker 启动前 sha256 闸。
DoD（本卷末）：购买→下载→激活闭环；关 dev_mode 后未授权占位、激活后自动处理；篡改二进制被拦。
约定：thiserror / 参数绑定 / 中英双语注释 / 中文 commit / 私钥绝不入仓（仅公钥 keys/vendor_ed25519.pub）。
注意：防破解只做离线验签（O3/R6 诚实边界），增量手段 v1 不做。
完成后：回到 Part4（exotic_format_plugin_part4_frontend_slice_hardening.md）。
```

## 本卷产出清单 | Deliverables checklist

- [ ] `Cargo.toml` 加 `ed25519-dalek`；`keys/vendor_ed25519.pub` 入仓（私钥离线）
- [ ] `exotic/license.rs`（verify + LicenseError 细分 + keyring store/load）
- [ ] `tools/sign_plugin.rs`（厂商侧签 manifest.sig + 签 token；生成密钥对脚本）
- [ ] `exotic/manifest.rs` 补 `bin_hashes`（闸 3 用）
- [ ] `exotic/package.rs`（zip 解包 + sha256 + manifest.sig 验签）
- [ ] `exotic/registry.rs`（discover：在线 + 离线回退）
- [ ] `ipc/exotic_commands.rs`：install/activate/uninstall/list_exotic_registry + 注册
- [ ] `host.refresh` 接真实 license（authorized 真值化）+ worker spawn 前 sha256 闸
- [ ] 验收：闭环 + 占位/激活切换 + 篡改拦截

