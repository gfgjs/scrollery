---
id: 2026-06-27-Part8_商业化与分发
status: active
type: canon
line: refactor_2026
created: 2026-06-27
---

# Part 8 · 商业化与分发（Commercialization & Distribution）

> 系列定位（**9-Part 收官**）：把方案 C（核心免费 + 三插件买断、四层防护、单一直销渠道）从「客户端已建的验签 / keyring 授权」变成「**可签发、可收款、可上架、可获客**」的真实分发——补齐**服务端 + 业务 + 官网**这一层。第三方商店渠道属未来路线（当前无任何渠道代码/字段/feature 预留，见 §1.3/§2.1）。
> 依赖：**Part0**（§7 盈利定价 / §8 防护四层 / §12 命名）、**Part4**（SCRFD 合规隔离——签发/打包须与之对齐，第7轮终审补对齐 §0.1；加密权重属未来研究，当前无字段预留）、**Part5**（前端 gate / 购买入口 UI）、**Part6**（EntitlementProvider trait / keyring 直销授权 / catalog 与 worker_id）、**Part7**（CI 门控 / commercial-build 产线 / 平台签名；渠道 feature 已随 P16 退役）。
> 取证基线：所有论断以 `文件:行号` / Part 节号一手核验；旧 plan-docs 与记忆仅作线索，不予轻信。

---

## §1 目标与范围

### 1.1 一句话目标

Part0-7 已把「能跑、能门控、能签名」的产品**在 plan 层面**定稿（授权抽象 `EntitlementProvider` + keyring 直销实现**已落地**；enc_seed/AES 属未来研究，**无代码预留**；多渠道维度已整体退役，见 §2.1）；但**收款链路、license 签发端、官网获客全为零**。Part8 补这最后一层——让用户**能买到、能激活、能从官网了解并下单**。这是把技术资产**变现**的 Part，性质偏服务端/业务，代码接缝集中在签发端与现有验签链。

### 1.2 范围内（Part8 认领）

| 编号 | 主题 | 一句话 |
|---|---|---|
| C1 | **License 签发基础设施** | 离线/服务端**签发端**：持 Ed25519 私钥签 `LicensePayload`（嵌 `enc_seed`/`subject_hash`），host 只验签（Part6 已建验签侧） |
| C2 | **层1 加密打包侧** | 付费插件权重 AES-256-GCM 加密进 catalog；manifest `encrypted` 字段；与 Part6 解密侧同一 HKDF 派生（Part8 管加密+签发，Part6 管解密+消费） |
| C3 | **层2 激活微服务（可选）** | 一次性在线激活：收 HMAC 哈希（不收 PII）→ 签发设备绑定增强 token + CRL 退款吊销；**默认离线激活仍可用**（§8.2 红线） |
| C4 | **第三方商店 Provider（未实现，未来路线）** | 当前无渠道桩/feature/catalog 字段（P16 已删）；日后上架需重新设计 StoreContext IAP / Steam DLC ownership 查询 |
| C5 | **商店上架** | MS Store EXE 路径（主）+ MSIX（待 #14935）+ Steam depot；商品页 / 截图 / 动图 |
| C6 | **定价落地 + 支付渠道** | CNY 区域定价；国际 FastSpring/Paddle（MoR）+ 中国区微信/支付宝；第三方商店商品 ID 属未来（渠道字段已删） |
| C7 | **官网 / 落地页 / 动图** | 产品站 + 购买引导落地页（Store 引流官网支付 0% 抽成）+ 演示动图；诚实文案（§7.2） |
| C8 | **获客** | 内容/社群/SEO/对标定位（Eagle/digiKam）；开源 Star 漏斗（Apache 核心曝光→插件转化） |

### 1.3 范围外（已由其他 Part 建，Part8 消费；或显式后置）

- **客户端验签 / EntitlementProvider / 签名产线**：Part6 已落地（trait + `KeyringLicenseStore` 直销实现 + `FreeStubEntitlement` fail-closed 回退），Part7 已落签名与 CI 面。Part8 **不重做**，只对接签发端（RK1 已按现状收敛）。
- **渠道维度**：已整体退役（P16，2026-09-15）——`channel-msstore`/`channel-steam` feature、互斥门、渠道桩、`InstallSource` 多值枚举与 catalog 渠道字段均已删除；任何商店上架脚本须先重新设计渠道层。
- **改名执行本身**（repo/Org/域名/`identifier`/模块前缀替换）：Part0 §12 拥有；Part8 将其列为**上架前置硬门槛**（identifier 锁定后不可改，Part7 T18），但不执行替换动作。
- **支付/税务法律实体注册、隐私政策法律文本起草**：业务/法务事项，Part8 给清单与要求，不代行法务。

### 1.4 红线（贯穿 Part8）

1. 🔴 **防护诚实边界**（Part0 §8.1/§8.3，明示「写入 Part8」）：本地校验无绝对阻断；**云端校验 / kill-switch / 定期联网 / 机器指纹上报 已否决**。层2 激活**默认离线可用、仅单次联网、传不可逆 HMAC 哈希不传 PII**，须隐私政策声明。
2. 🔴 **文案不夸大**（Part0 §7.2）：AI/人脸权重**部分来自公开仓库**（`gficcg/clip_cn_vit-onnx`、`opencv_zoo`）；卖「集成 + 中文优化 + 自动更新 + 体验」，**非「秘密权重」**。落地页措辞据此，杜绝「独家模型」式宣传。
3. 🔴 **非商用模型绝不打包**（记忆/Part0 合规审计）：SCRFD(`det_10g.onnx`)+ArcFace(`w600k_r50.onnx`) InsightFace 研究专用——任何付费/免费包**绝不含**；catalog 彻底隔离 + CI 断言不打包。默认轨 YuNet(MIT)+SFace(Apache) 合规。
4. 🔴 **签发私钥 / enc_seed / AES 主密钥 / 激活服务密钥 永不入仓、永不入 public CI**（Part0 §10 + Part7 红线）：签发端在离线/HSM；激活微服务密钥在私有后端。
5. 🔴 **改名完成前不上架**（Part0 §12 / Part7 T18）：`identifier` `com.picasanext.app` 含 picasa 词根，首次上架即锁死；改名是上架前置。
6. 🔴 **中国区人脸合规**：PIPL +《**人脸识别技术应用安全管理办法**》（2025-06-01 施行，全名含「应用安全」，L4 已联网核实）——7 项义务随**产品形态**触发（本地自用 vs 上架/云端，Part4 §3.10 形态×义务矩阵，P1-11）；中国区发行前须法律评估。
7. 🔴 **渠道隔离（当前唯一渠道 = 直销）**：授权与签发只服务 direct，不存在跨渠道迁移；日后引入第三方渠道须为该渠道单独设计授权与定价对齐口径（Steam **key** 平价成文；官网平价非成文、待法务，L9/§3.6.2，P1-10）。

---

## §2 现状实测（file:line 一手取证）

> 4 路并行 recon + 人工复核（cargo 离线缓存）。Part8 性质特殊——多数是「从零建服务端/业务」，取证重心是**确认要对接的客户端契约**（Part6/Part7 设计）+ **划清缺口边界**。

### 2.1 授权抽象层：✅ 已落地（Part6 T9/T10/T13）；渠道维度已退役（P16）

授权 trait 与实现归一已落地（Part6 §3.8/§3.9）；**多渠道维度已整体删除**（2026-09-15 消融 P16），当前唯一渠道 = 直销：

| 设计项 | 设计来源 | 现状 | 谁先落地 |
|---|---|---|---|
| `EntitlementProvider` trait | Part6 §3.8/§8.3 | ✅ 已建（`scrollery-plugin-api`） | Part6 T9 |
| `ActivationInfo`（携带 `enc_seed`） | Part6 §8.3 | ❌ 已删（2026-09-15 消融 P10：`activate`/`deactivate` 返回 `Result<(), LicenseError>`，无 payload） | — |
| `MsStoreEntitlement`/`SteamEntitlement` stub | Part6 §3.8 / Part7 §3.6.3 | ❌ 已删（P16：渠道桩退役，无替代预留） | — |
| `DeliveryChannel`/`InstallSource` 枚举 | Part6 §8.4 | ❌ 已删（P16：枚举只剩单一取值，随参数一并移除） | — |
| `PluginDeliverySource` trait | Part6 §3.8/§8.4 | ❌ 已删（2026-09-15 消融 P09：交付来源包装与枚举均已删除，安装函数无来源参数） | — |
| `entitlement_source` DB 列 | Part6 §8.4 | 已随 P23 删除，唯一渠道无需持久化来源 | — |
| `store_product_id`/`steam_dlc_app_id` 字段 | Part0 §9.5 / Part6 §8.4 | ❌ 已删（P16：Catalog/Registry 两侧渠道字段移除） | — |
| `channel-*` features | Part7 §3.6.1 | ❌ 已删（P16：空渠道与 `channel-direct` 一并退役，updater 转普通依赖） | — |
| 授权实现归一 | Part6 §3.9 | ✅ `KeyringLicenseStore` 唯一实现 + `FreeStubEntitlement` fail-closed 回退（同住 `exotic/license.rs`） | Part6 T9/T10 |

**现有授权代码（Part8 要升级/对接的基线）**：
- `LicenseSource` trait（[license.rs:235](../../src-tauri/src/exotic/license.rs#L235)）：**仅** `evaluate(plugin_id, sku, now) -> LicenseStatus`，无 `activate`/`source_tag`/`enc_seed`。
- `KeyringLicenseStore`（license.rs:250-315）：`activate(plugin_id, sku, token, now) -> Result<LicensePayload>` 是 **inherent method**（非 trait，license.rs:282）；Part6 T9 升格为 `EntitlementProvider::activate`。
- `ExoticHost.licenses: Arc<dyn LicenseSource>`（mod.rs:184）→ Part6 T9 改 `Arc<dyn EntitlementProvider>`。
- 现有 `Availability` 枚举（mod.rs:45-64）：9 细分态（`AvailableUninstalled`/`InstalledUnlicensed`/`Authorized`/`LicenseExpired`/…）；§8.3 的 IPC 粗粒度 5 态（`NotInstalled|Installed|Authorized|UpdateAvailable|PlatformUnsupported`）是 Part5 徽章视图，**两者并存不冲突**。

> 🔴 **Part8 实施前提**：授权抽象（Part6 T9/T10）已落地；Part8 对接的是**已存在的验签链**，不重建抽象，也不按旧渠道骨架预留。plan 全程以 Part6 §8.3 权威签名为准。

### 2.2 License 验签链成熟，**签发端为零、缺 `enc_seed` 字段**

`LicensePayload`（[license.rs:39-52](../../src-tauri/src/exotic/license.rs#L39-L52)）现有字段：

| 字段 | 类型 | 说明 |
|---|---|---|
| `version` | u32 | 固定 1（`SUPPORTED_TOKEN_VERSION`） |
| `key_id` | String | 选 `purpose=license` 公钥 |
| `license_id` | String | 发行唯一标识 |
| `plugin_id` | String | 授权插件 |
| `sku` | String | 授权 SKU |
| `subject_hash` | Option<String> | 层2 设备绑定；`#[serde(default)]`；Debug 脱敏 `<redacted>` |
| `issued_at`/`not_before`/`expires_at` | i64/i64/Option<i64> | `expires_at=None`=永久（v3 主路径） |

- **🔴 `enc_seed` 字段当前不存在，且已无预留（2026-09-15 消融 P10）**——Part6 §3.7.2 曾设计 `LicensePayload` 加 `enc_seed: Option<String>`(base64)，该字段连同 `ActivationInfo` 已删；AES 未实现期间不保留生产契约，本节以下签发字段设计仅在层1 实装时重新落定。
- `verify_token`（license.rs:131-200）：8 步校验（长度≤8KiB→结构两段→base64url 解码→version 探针→反序列化→**验签** `verify_strict` 覆盖原始 payload_bytes→plugin_id/sku 比对→时间窗）。错误码折叠为 `BadSignature`（除 `UnknownKey`）防探测。
- token 格式：`base64url(payload_json).base64url(ed25519_sig_64B)`；存 OS keyring（`service="picasa-next"`/`account=plugin_id`）；DB 不存 token；日志/IPC 绝不输出 token/subject_hash。
- **host 仅验签不签发**：所有 `Ed25519KeyPair`/`from_seed_unchecked`/`sign()` 均在 `#[cfg(test)]`（crypto.rs:274-275 等）；release 二进制零私钥操作（crypto.rs:4 注释「私钥离线/HSM，永不入二进制」）。`BUILTIN_KEYSET_JSON=include_str!`(crypto.rs:23) 为占位公钥，发布前替换生产公钥。
- **AES 现状（Part6 §3.7 设计，未实现）**：crypto.rs 无任何 AES/encrypt/decrypt；`ring::aead` 已在依赖（随 rustls）从未调用。密文格式 `nonce(12B)||ct||tag(16B)`，salt 32B 存 `ModelBlob.enc_salt`，`HKDF-SHA256(ikm=enc_seed, salt, info=plugin_id||model_id)`。

> **Part8 签发端须建**（离线/服务端持私钥）：签全部 `LicensePayload` 字段 + **新增 `enc_seed`**（高熵 OsRng 32B→base64，**按 plugin_id 固定的插件主种子，非 per-license**——见 §3.2.2）；签名覆盖原始 `payload_json_bytes`；私钥用 `license-*` 用途密钥（与 `release` 用途严格分离，双信任根 Part0 §5.1）。

### 2.3 Catalog/定价字段缺口 + 插件 id/sku 现状 + Part0 定价锚点

**catalog 结构**（[catalog.rs](../../src-tauri/src/exotic/catalog.rs)）：`RawOffering`/`CatalogOffering` 含 plugin_id/name/media_kind/formats/capabilities/license_tier/platforms/min_host_version/`sku`(Option)/`store_url`(Option)/`distribution`/`worker_id`/`uses_gpu`；**无** `store_product_id`、**无** `steam_dlc_app_id`（P16 已删，渠道字段不预留）；`InstalledPluginRecord` 不再含 `entitlement_source`，DB/SQL/模型同步删除该空维度。

**插件 id/sku**：现仅 `exotic-image-psd`/`psd-engine-2026`（[coordinator.rs:30](../../src-tauri/src/exotic/coordinator.rs#L30) `PSD_PLUGIN_ID` + `exotic-catalog.json:6,12`）。规划 `ai-clip`/`ai-clip-pro`、`ai-face`/`ai-face-pro`、`exotic-formats`/`exotic-formats-pro` **仅存 plan 文字**，代码/catalog 无实体（待 Part4+Part6）。

**Part0 §7 定价（直销基准，精确）**：

| 项 | 定价 | 抽成（§9.2） |
|---|---|---|
| 核心（四媒体浏览） | 永久免费、无试用限制 | — |
| AI 分析（CLIP） | **$12.9 / ¥89**，买断含模型更新 1 年 | direct 0% / Store EXE 0% / MSIX IAP 15% / Steam 30% |
| 人脸聚类（YuNet+SFace） | **$9.9 / ¥69** 买断 | 同上 |
| 冷门格式（PSD/RAW/…） | **$9.9 / ¥69** 买断 | 同上 |
| 三件套（~20% 折） | **$24.9 / ¥168**（对标 Eagle $34.95） | 同上 |
| 大版本升级 | 50% 折；小版本永久免费 | — |
| 中国区 | **CNY 区域定价（USD 40-50%）** | — |

支付（§7.3）：国际 **FastSpring/Paddle**（MoR 代缴税）；中国区**必接微信/支付宝**（否则 CNY 形同虚设）。§9.6 时序：主直销→v1.0 后 3 月上 Store EXE 路径（0% 抽成，购买引导至官网）→ MSIX 待 #14935 → Steam 视受众。**定价对齐**（🔴 第 8 轮核验：成文 Steamworks 仅约束 **Steam key 平价**；「官网不得更便宜」**非成文公开规则**、系 Valve 施压 / Wolfire v. Valve 诉讼中未定论，详见 §3.6.2 / external_references L9——作商业策略待法务，**不写成平台政策事实**）。

### 2.4 前端商店 / 官网 / 支付全为零；商店 Provider 依赖离线差异

**前端购买链路全为零**（Part5 设计 T11/T12，未落地）：
- `ModelLibrary.vue` / `FaceModelLibrary.vue` 有模型下载/切换，但**无付费 gate / 授权 / 购买引导**。
- **exotic 插件商店前端完全为零**（`src/` 无任何 PluginStore/激活/购买 Vue/store；grep 验证）。
- 购买 gate 逻辑零（`App.vue` 里 `gate` 字样仅 GPU 分析占位锁注释，非授权 gate）。

**官网/落地页/marketing 完全不存在**（glob `official/landing/marketing/website/site` 零命中）——须从零建（C7）。

**🔴 商店 Provider 依赖的离线差异（人工核实 cargo 缓存）**：

| 依赖 | 用途 | 缓存 | 含义 |
|---|---|---|---|
| `windows` 0.58（已在依赖） | MsStore `Windows.Services.Store` StoreContext IAP | **已有** | MsStoreProvider **离线可编译**（只需加 `Services_Store` feature，元数据驱动）；运行需真实 Store 沙箱 |
| `steamworks` | Steam DLC ownership（`is_subscribed_app`） | **缺** | SteamProvider **离线无法编译**，须联网拉取（同 Part7 R6 性质） |
| `axum`/`hyper`/`tower`/`uuid`/`ring` | 签发端 / 激活微服务（若 Rust 实现） | **全有** | C1 签发端 + C3 激活微服务可**离线构建** |

> 结论：Part8 多数服务端代码离线可建；唯 **SteamProvider 须联网**。MsStore 离线编译 OK 但**运行**须 Store 关联应用（开发期模拟受限），属运行期约束。

### 2.5 合规现状：SCRFD 隔离半成品 + 改名未做 + PIPL 待评估

**非商用模型隔离（半成品，Part4 T1 待实施）**：
- `face_profile.rs:11-13/153/180-202`：默认轨 `yunet-sface` `commercial_ok=true`；可选轨 `scrfd-arcface-*` `commercial_ok=false`，`embed_file="w600k_r50.onnx"`。
- `face_commands.rs:364-426`：按 `downloadable` 过滤，SCRFD/ArcFace `downloadable=false`（一键下载报错、仅手动导入）。
- **🔴 隔离不彻底**：`commercial_ok=false`+`downloadable=false` 只挡「一键下载」；但 **SCRFD 函数体（face.rs:253-400）+ 路径常量（`w600k_r50.onnx`/`det_10g.onnx`）仍编进主二进制**。`#[cfg(feature="face-noncommercial")]` 隔离 + CI 断言不打包**未落地**（Part4 T1）——商业 build 前**必须**完成（红线 1.4.3）。

**改名未做**：仍用 `picasa-next` 工作代号、`identifier=com.picasanext.app`。Part0 §10.7：`PICASA`=Google 活跃商标（USPTO Reg. 2952412 Class 9）；**平台投诉即下架**，与「开源冲 Stars + 上架」策略直接对冲。候选名首选 **Mnemo**（次选 Foteca/Arclive/Tessera/Lumeo），均待清权（USPTO TESS + EUIPO + CNIPA 双类 + FTO 意见书 $1500-4000/名）。**公开/上架前必须完成改名 + 清权**。

**PIPL / 人脸识别办法**（Part0 §10.5）：中国大陆发行人脸功能须**商业发行前单独法律评估**。

**MSIX WACK #14935**（Part0 §9.4）：上游 Tauri bug 未修，先走 Store EXE 路径绕开（开放决策、不阻塞）。

---

## §3 设计方案

### 3.0 商业化端到端全景（一次直销购买如何流经全链路）

收官前先把分散在各 Part 的环节串成闭环。**直销渠道一次 AI 插件购买**的完整数据流（标注每环节归属 Part）：

```
① 用户在官网/应用内 gate 点「购买 AI 插件」
   └ 落地页(C7) → 支付(FastSpring/Paddle/微信/支付宝, C6) → 收款
② 支付成功 webhook → License 签发端(C1, 离线/服务端持私钥)
   └ 生成 LicensePayload{plugin_id=ai-clip, sku=ai-clip-pro, enc_seed=OsRng32B, expires_at=None}
   └ Ed25519 签(license-* 用途私钥) → token=base64url(payload).base64url(sig)
   └ 邮件/账户页投递 token 给用户
③ 用户在插件商店(Part5)粘贴 token → activate 命令
   └ KeyringLicenseStore.activate → verify_token(Part6 验签) → 写 keyring
   └ 从 payload 取 enc_seed → HKDF 派生 DerivedSecret → 写 ExoticHost 内存映射(Part6 §8.2 C2)
④ 下载加密插件包(.ppx) ← 私有 CDN
   └ 包内 ONNX 权重是 AES-256-GCM 密文(C2 打包侧加密, enc_salt 在 manifest)
   └ Registry index Ed25519 验签 + 逐文件 SHA-256(Part6 层0) → 安全解包安装(Part6)
⑤ 启动 worker 推理(Part4/Part6)
   └ DerivedSecret 门控: is_task_runnable 查 plugin_id→DerivedSecret(无则拒)
   └ 主进程 HKDF(enc_seed,enc_salt) → ring::aead 解密 ONNX → 匿名内存映射
   └ SessionInit 传 handle → worker mmap → commit_from_memory(Part6 §8.1 决策2) → VRAM
```

**唯一渠道 = 直销（direct）**：收款走官网支付；授权判定走 Ed25519 token + OS keyring；插件交付走私有 CDN 下载 `.ppx`（验签 Registry → HTTPS → size/sha256 校验）。权重加密/ `enc_seed` 属未来研究（**当前无字段、无原语、无 cfg 门控**）。第三方商店渠道（Store/Steam）**未实现且无代码预留**（P16 已删渠道 feature/桩/字段）；日后上架需重新设计该渠道层与其合规门控。

### 3.1 License 签发基础设施（C1，离线/服务端，红线：私钥永不入仓/public CI）

**定位**：host 只验签（Part6 已成），**签发是反向操作**——持 `license-*` 用途私钥签 token。这是 Part8 从零建的服务端。

**3.1.1 签发端形态**：
- **离线 CLI 工具**（最小可用）：`picasa-issuer sign --plugin ai-clip --sku ai-clip-pro --out token.txt`，私钥从 HSM/硬件令牌读，气隙环境运行。早期手工签发够用（订单量小）。
- **签发微服务**（规模化）：Rust `axum`（缓存已有）接支付 webhook → 自动签发 → 投递。私钥在隔离后端（KMS/HSM），与激活微服务（C3）可同进程或分离。

**3.1.2 签发字段全集**（对齐 `LicensePayload`，§2.2 + 新增 `enc_seed`）：

| 字段 | 签发取值 |
|---|---|
| `version` | 1 |
| `key_id` | 生产 `exotic-keyset-prod.json` 中 `purpose=license` 条目 id（如 `license-2026-01`） |
| `license_id` | `uuid` v4（缓存已有）或 ksuid，全局唯一防重放 |
| `plugin_id` | 精确匹配 catalog（`ai-clip`/`ai-face`/`exotic-formats`） |
| `sku` | 精确匹配 catalog（`ai-clip-pro` 等） |
| `subject_hash` | 直销离线 **None**；层2 激活时由客户端本地算 `HMAC(license_id, cpu+disk)` 回传（不在签发端，§3.3） |
| `issued_at`/`not_before` | 服务端时钟 Unix 秒 |
| `expires_at` | **None=永久**（买断主路径）；模型更新 1 年是**业务侧**记录（非 token 字段，AI 插件 §7.2） |
| **`enc_seed`** | **🔴 新增**：插件**主种子**（按 `plugin_id` 固定，**非 per-license**——见 §3.2.2）；首次为该插件 `SystemRandom` 生成 32B→base64，此后同插件所有 token 复用；AES 密钥链路起点 |

**3.1.3 签名操作**：`payload_bytes=serde_json::to_vec(payload)` → Ed25519 私钥签**原始 bytes**（不重序列化，与 verify_token 第6步对称）→ `base64url(payload_bytes).base64url(sig)`。

**3.1.4 密钥治理（红线）**：① 签发私钥（`license-*`）与发布签名私钥（`release-*`，Part7 updater/Authenticode）、exotic Registry 私钥**三套分离**（双信任根 + 用途分离，Part0 §5.1）；② 私钥气隙/HSM，永不入仓、永不入任何 CI；③ 发布前流水线把占位公钥集替换为生产公钥（`PICASA_EXOTIC_KEYSET_FILE` 编译期注入；本次归一保留既有 key ID、公钥、用途和有效期，**注入仍为整组替换、轮换时须保留需继续信任的历史键**；**验签公钥可公开**，Part6 §3.9）；④ `enc_seed`=**per-plugin 主种子，签发端必须可重复取得**（同插件后续 license 复用同一 enc_seed + 新模型版本加密都要它）→ **持久化于 HSM/secret manager，或由 HSM 主密钥按 plugin_id 确定性派生**（`enc_seed = KDF(master_key, plugin_id)`，主密钥不出 HSM）；**不入仓、不入 CI、不落普通 DB**。客户端侧 enc_seed 存 OS keyring。（修正：早稿「签完即弃」与 per-plugin 固定主种子矛盾。）

### 3.2 层1 加密打包侧（C2，与 Part6 解密侧对称；红线：AES 主密钥不入仓）

Part6 建**解密 + 消费**侧（客户端）；Part8 建**加密 + 打包**侧（服务端/打包流水线）。两侧靠 HKDF 对称。

**3.2.1 打包加密工具**（离线/私有 CI）：`picasa-pack --plugin ai-clip --model clip-vit-b16.onnx --enc-seed <issued> --out blob`：
- 生成 per-model 32B 随机 `salt`（`SystemRandom`）。
- `key = HKDF-SHA256(ikm=enc_seed, salt, info=plugin_id||model_id)`（`ring::hkdf`，与 Part6 解密侧**同一派生**）。
- `ring::aead` AES-256-GCM 加密 ONNX → 密文 `nonce(12B)||ciphertext||tag(16B)`，`nonce` 每次 `SystemRandom` 刷新。
- 写 `ModelBlob.enc_salt`(base64) + manifest `encrypted=true`（随 Registry Ed25519 签名保护，防篡改 salt）。

**3.2.2 关键约束**：
- **enc_seed 的两难**：每张 license 的 `enc_seed` 不同 → 同一 ONNX 须为每个购买者**单独加密一份**？❌ 不可行（CDN 存爆炸）。**正解**（对齐 Part6 §3.7 设计）：`enc_seed` 是**插件级共享密钥种子**，非 per-license——签发端为**同一 plugin_id 的所有 license 签入同一 `enc_seed`**（该插件的「主种子」），CDN 上每插件一份密文即可。per-license 唯一的是 `license_id`/`subject_hash`，**不是** `enc_seed`。
  > ⚠️ 这钉死全篇 `enc_seed` 粒度（§2.2/§3.1.2/RK2 均按此）——**enc_seed 按 plugin_id 固定**（一插件一主种子），非 per-license，否则 CDN 密文无法复用。签发端对 `ai-clip` 的所有 token 填同一 `enc_seed_ai-clip`。
- 🔴 **泄漏半径诚实标注（terminal review）**：per-plugin 共享 enc_seed + token `payload` **明文可读**（`base64url(payload)` 非加密）→ **任一付费用户泄漏其 token = 该插件模型密钥泄漏**（解析 token 即得 seed，**无需内存 dump**，比 Part0 §8.3 列的「内存 dump」门槛更低）。这是 per-plugin 换 CDN 密文复用的**固有代价**。缓解=主种子轮换（疑似泄漏即重加密 CDN + 签新 token）+ 可选层2 在线激活（§3.3）；根除须 per-device 个性化密文（成本高）或强制在线激活——默认离线买断**不采用**、接受此泄漏半径（与 §8.3 诚实评级一致）。见 §5 RK13。
- 主种子轮换：若某插件主种子疑似泄漏，签发新 `enc_seed` + 重加密 CDN 密文 + 新 token（旧 token 仍验签通过但解不开新密文）——配合层2 CRL（§3.3）。
- **AES 主流程不护公开权重的「秘密」**（§7.2 诚实边界）：加密挡的是「无 license 拿不到可用权重」，非「权重本身机密」（权重多来自公开仓库）。

### 3.3 层2 可选在线激活微服务（C3，红线：默认离线、单次、传哈希不传 PII）

**定位**（Part0 §8.2 层2）：**可选**增强，**默认离线激活仍可用**。挡退款滥用 + 无限设备激活。**绝不**做 kill-switch/定期联网/指纹上报（§8.1 否决）。

**3.3.1 流程**（🔴 **第 7 轮终审：补 PoP + nonce，修「无持有证明 + HMAC key 公开」的可冒名/可重放缺口**）：
```
直销 token 激活时(可选勾选「在线激活增强」)：
  → GET /activate/challenge { license_id } → 服务端返回一次性 nonce(短 TTL，防重放)
  客户端本地算 subject_hash = HMAC-SHA256(license_id, cpu_serial||disk_serial)  // 不可逆、不传原始硬件值
  → POST /activate { base_token, subject_hash, nonce }
        // base_token = 完整 license-* 签名 token（持有证明 PoP），非裸 license_id
  → 服务端:
       ① verify_strict(base_token) 验 license-* 签名 → 证明请求者真持有合法 token
          （非仅"知道 license_id"——license_id 是 token 明文字段、泄漏即公开）；从 token 取 license_id
       ② 校验 nonce 未用过且未过期（防重放）
       ③ 查 license_id 激活次数 ≤ N(如 3 台)；记录 subject_hash(非原值)
  → 签发设备绑定增强 token(activation-* 签，payload 填 subject_hash) → 客户端写 keyring
  此后永久离线; 客户端验签时比对本地重算 subject_hash == token.subject_hash
```
> 🔴 **为何必须 PoP**（第 7 轮终审）：原流程仅传 `{license_id, subject_hash}`，而 `license_id` 是 `LicensePayload` 明文字段（license.rs:42，token = `base64url(payload).base64url(sig)`，payload 可解）→ 以 `license_id` 为 HMAC key 时 **HMAC 退化为公开-key 哈希**（仅混淆、无认证）。无 PoP/nonce → 持泄漏 token 者可对**任意硬件**自算合法 `subject_hash` 并重放，冒名消耗正版激活槽、绕过设备绑定（Part0 §8.2 层1b「挡 token 共享/VM 直拷」否则被击穿）。补 `base_token`（PoP）+ `nonce`（防重放）后，服务端先证「请求者确持合法 token」再计数。**与 RK13（解密权重）是不同攻击面**（本条=冒名激活），见 §5 RK 表。

**3.3.2 CRL 退款吊销**：服务端把退款订单的 `subject_hash`/`license_id` 加黑名单；客户端**仅在下次在线激活时**拉 CRL 比对（离线模式层2 退化无效，§8.3 诚实评级——这是可接受上限）。

**3.3.3 技术栈**：Rust `axum`（缓存已有）+ 最小存储（license_id→激活计数 + subject_hash 黑名单）。**隐私政策须声明**「激活仅传不可逆哈希、不传硬件原值、不传 PII」（红线 1.4.1）。

🔴 **3.3.3a 增强 token 的签发密钥治理（第 5 轮复审补，原方案缺失）**：客户端 `verify_token`（license.rs:131）用 `purpose=license` 的 `VerifyingKeyset` 验签。C3 要签出能通过客户端验证的「增强 token」就**必须持私钥**——但 license-* 签发私钥按红线（C1 §3.1.4）是**气隙/HSM、绝不上线**。两者冲突的唯一干净解：
- **为 C3 引入独立的 `activation-*` purpose 密钥对**（与 license-* 物理分离）。`activation-*` **私钥在 C3 在线后端**（在线服务持在线密钥，不违气隙——气隙只约束 license-* 主签发私钥）；`activation-*` **公钥加入客户端 `BUILTIN_KEYSET`**。
- 客户端 `verify_token` **允许 `purpose ∈ {license, activation}`**：基础 token 由 license-* 签（气隙），增强 token 由 activation-* 签（在线）。两类 token 各按其 purpose 取对应公钥验签，互不越权。
- 备选（不推荐）：C1 作签名代理（C3 把待签 payload 转发 C1 气隙网关）——部署复杂、C1 须半在线，放弃「纯气隙」表述。
- **D4 验收锚点加**：增强 token 经 host `verify_token` 验签通过 + 其 purpose=activation + 公钥来源于 BUILTIN_KEYSET（密钥来源合规、未泄漏 license-* 私钥）；🔴 **且激活请求经 PoP（服务端 `verify_strict(base_token)` 通过、从 token 取 license_id）+ nonce 防重放校验**（第 7 轮终审 RK14，§3.3.1）。

**3.3.4 与直销主路径的关系**：层2 是 `subject_hash` 字段的**唯一填充者**（§2.2 该字段直销离线为 None）。不开层2 → 纯离线买断，token `subject_hash=None`，verify_token 不校验设备（§3.1.2）。

### 3.3a 🔴 生产服务硬化 + bundle 签发（P1-10，第 8 轮核验）

C1 签发端（§3.1）+ C3 激活服务（§3.3）此前仅设计「快乐路径」，缺生产必需项（多在仓外私有服务，但列入 D9/D4 验收）：

**资金/安全正确性（必须，否则可被白嫖/伪造）**：
- **webhook 验签**：FastSpring/Paddle 支付成功 webhook 必须验来源签名（各自 HMAC 头 + 共享密钥）——否则伪造 webhook 即免费铸 license。验签失败拒签发。
- **幂等键**：webhook 带订单 id 作幂等键，签发端记 `processed_webhooks(order_id)`，重复投递只签发一次（支付商重投常态）。
- **订单状态机**：`order: created→paid→issued→delivered→(refunded)`，状态转换 CAS 原子、非法转换拒绝。
- **退款 vs 签发竞态**：退款 webhook 与签发并发 → 订单行加锁/版本号；退款置 `refunded` 后拒新签发，并把已签 `license_id`/`subject_hash` 入 CRL（§3.3.2）。

**可靠性（生产运维）**：
- **投递失败重试**：签发 token 投递（邮件/账户页）失败 → 重试队列（指数退避）+ 死信告警；用户「已购买激活」入口可主动拉取兜底。
- **审计日志**：签发/激活/吊销/退款全留痕（order_id/license_id/时间/动作/结果），append-only 不可变。
- **速率限制**：`/activate/challenge`、`/activate` 端点限流（按 IP + license_id），防爆破/重放扫描。
- **密钥轮换**：`enc_seed` 主种子轮换已成文（§3.2.2）；补 **license-\* / activation-\* 签名私钥轮换**流程（双密钥重叠期 + 客户端 BUILTIN_KEYSET 多公钥并存验签 + 旧 token 宽限期）。
- **灾难恢复**：签发端/激活服务 DB（订单/激活计数/CRL）定期备份 + 异地 + 季度恢复演练；签发私钥气隙多副本分片保管。

**bundle 购买 → 多 token 签发（补 `LicensePayload.plugin_id` 单值缺口）**：
- 现 `LicensePayload{plugin_id, sku}` 单值 = 一 token 一插件（license.rs:43-44）；bundle（三件套 $24.9）须映射 token。
- **定稿：一订单签 N 个独立 token**——bundle SKU 解析为成员插件列表（`ai-clip`/`face`/`exotic`），签发端对每个成员各签一个 `LicensePayload{plugin_id=成员, sku=成员-pro}` token 一并投递；客户端各 token 独立验签/激活（复用现有单 token 链，**无需新增 bundle entitlement 字段**）。
- bundle 退款 → 经 `订单→token 列表`关联表吊销该订单签发的**全部** N 个 token。

> 落地：并入 §4 D9（签发/支付）+ D4（激活）任务；§6 验收加「webhook 验签/幂等/退款竞态/bundle 多 token」生产硬化项。webhook 验签 + 幂等缺失 = 可被伪造请求白嫖 license，属资金安全须先于上架收口。

### 3.4 第三方商店 Provider（C4，未来路线）

**现状**：未实现，且**无任何代码预留**——`channel-msstore`/`channel-steam` feature、渠道桩、互斥门、catalog 渠道字段与 `InstallSource` 多值枚举已随 P16 删除。下方为**重新引入该渠道时的设计草案**，不是现存代码契约；实施须先重建渠道 feature + cfg 物理排除 + 商品 ID 字段。草案共同前提：复用已落地的 `EntitlementProvider` trait（§8.3 权威签名），且该渠道不碰 Ed25519/keyring/AES（平台信任，§3.0 表）。

**3.4.1 trait 对接契约**（Part6 §8.3 权威）：
```rust
pub trait EntitlementProvider: Send + Sync {
    fn evaluate(&self, plugin_id: &str, sku: &str, now: i64) -> LicenseStatus;
    fn source_tag(&self) -> &'static str;                  // "ms_store" | "steam"
    fn activate(&self, plugin_id: &str, sku: &str, credential: &str, now: i64) -> Result<(), LicenseError>;
    fn deactivate(&self, plugin_id: &str) -> Result<()>;
}
```

**3.4.2 MsStoreProvider（设计草案）**：
```rust
#[cfg(feature = "channel-msstore")] // 该 feature 当前不存在，重新引入时才新建
impl EntitlementProvider for MsStoreProvider {
    fn evaluate(&self, plugin_id, sku, now) -> LicenseStatus {
        // StoreContext::GetAppLicenseAsync() → AddOnLicenses[store_product_id].IsActive
        // 商品 ID 须重新引入 catalog 字段（P16 已删 store_product_id）
        // IsActive → Authorized; else Unlicensed
    }
    fn source_tag(&self) -> &'static str { "ms_store" }
    fn activate(&self, plugin_id, sku, credential, now) -> Result<(), LicenseError> {
        // StoreContext::RequestPurchaseAsync(store_product_id) → 确认
        // 无 payload（ActivationInfo/enc_seed 已随 P10 删除）；渠道无 AES 派生密钥（worker 内置）
    }
    fn deactivate(&self, _) -> Result<(), LicenseError> { Ok(()) /* 清内存缓存 */ }
}
```
- 须加 `windows` crate `Services_Store` feature（仅该渠道 build，cfg 门控避免 direct/oss 引入）。
- **无派生密钥的含义**：该渠道 worker 内置、权重不加密 → 无需派生密钥；`enc_seed` 与 AES 解密原语全仓不存在（P10 已删、无未来字段预留）。

**3.4.3 SteamProvider（设计草案，🔴 离线无法编译——`steamworks` 缺，§2.4，待联网）**：
```rust
#[cfg(feature = "channel-steam")]
impl EntitlementProvider for SteamProvider {
    fn evaluate(&self, plugin_id, sku, now) -> LicenseStatus {
        // SteamApps::is_subscribed_app(steam_dlc_app_id) → Authorized/Unlicensed
        // DLC app id 须重新引入 catalog/registry 字段（P16 已删 steam_dlc_app_id）
    }
    fn source_tag(&self) -> &'static str { "steam" }
    fn activate(&self, ...) -> Result<(), LicenseError> {
        // Steam 无显式激活（购 DLC 后 is_subscribed_app 自动 true）
        // 无 payload（ActivationInfo/enc_seed 已随 P10 删除）
    }
    fn deactivate(&self, _) -> Result<(), LicenseError> { Ok(()) }
}
// main.rs（channel-steam）：启动时 SteamAPI_RestartAppIfNecessary(APP_ID) 确保从 Steam 启动
```
- `steamworks` crate（或 raw FFI 到 `steam_api64.dll`）须联网拉取；CI steam job 在联网环境构建。

**3.4.4 交付侧**：现 `installer.rs::install_staged_zip` **无来源参数**（P16 已删 `InstallSource`），恒走「验签 Registry → HTTPS 下载 → zip 内 manifest/hash 逐文件复核」。日后引入平台信任交付时须重新设计来源维度：平台渠道跳 Registry 验签，但**保留 hash/manifest 完整性复核**（防分发损坏）。

### 3.5 商店上架 + 改名前置（C5；红线：改名+清权完成前不上架）

**3.5.1 改名是上架的绝对前置**（红线 1.4.5，Part0 §10.7）：
- `identifier` `com.picasanext.app` 含 picasa 词根，首次上架即锁死（Part7 T18）→ 上架前**必须**完成改名。
- 改名清单（Part0 §10.7，一次性替换）：GitHub repo/Org、域名、`Cargo.toml` package name、`tauri.conf.json` identifier、内部 `picasa_next` 模块前缀、所有文档/营销物料。
- 清权（公开/上架前完成）：候选名首选 **Mnemo**；USPTO TESS + EUIPO eSearch + CNIPA（Class 9+42 双类、英中音译）+ 域名/Store 重名/GitHub Org 核 + FTO 意见书 US/EU/CN（$1500-4000/名）。
- **执行归属**：改名动作本身 Part0 拥有（§10.7）；Part8 将其列为**上架门禁**——CI/发布检查清单断言「无 picasa 词根（🔴 P1-11：限定代码标识符/产品名/Cargo·package/identifier/模块前缀/物料 + allowlist 豁免 docs/历史/SEO，**非全仓裸 grep**，详见 §7 阶段C）+ identifier 已锁定」方可提交商店。

**3.5.2 渠道上架时序**（Part0 §9.6，按抽成/风险排序）：

| 阶段 | 渠道 | 打包 | 收款 | 抽成 | 触发条件 |
|---|---|---|---|---|---|
| 1（主） | 官网直销 | Win32 EXE/MSI（Part7 commercial-release direct） | 官网（FastSpring/Paddle/微信/支付宝） | **0%** | v1.0 |
| 2 | 微软 Store **EXE 路径** | Win32 EXE 经 Store 提交，**插件购买引导官网** | 官网 | **0%** | v1.0 后 3 月 |
| 3 | 微软 Store **MSIX** | 未实现（需先重建渠道层；worker 内置 +30-80MB）| StoreContext IAP | 15% | 待 Tauri #14935 修复后评估 |
| 4 | Steam | 未实现（需先重建渠道层 + `steam_api64.dll` depot） | Steam | 30% | 视受众（曝光为主） |

**3.5.3 上架脚本（未实现；无渠道矩阵占位）**：
- **MSIX**：`winapp pack` / Tauri MSIX 产物 → Partner Center 提交。规避 WACK #14935 前不强推（先 EXE 路径）。
- **Steam**：SteamPipe `steamcmd +app_build` 上传 depot；DLC 配置（每插件一个 DLC app id，字段需重新引入）；`steam_api64.dll` 随 depot。
- **商品页素材**（每渠道）：截图、**演示动图**（语义搜索/人脸聚类/PSD 打开实拍）、描述（诚实文案 §3.7）、定价（含抽成对齐）、隐私政策链接（层2 激活声明）。

**3.5.4 商店合规自检（未来路线，当前无渠道可检）**：上架商店前须为每个平台渠道重建编译期物理排除与提交前 CI 断言（该渠道 build `cargo tree` 无 `tauri-plugin-updater`；二进制无可达 keyring DRM / 权重解密 / HTTPS-下载-执行路径；无 SCRFD/ArcFace 符号——红线 1.4.3）。🔴**第7轮终审收窄不变**：**不**断言「无 Ed25519/ring 符号」——验签原语 `verify_token` 留开源(§8.7)、`ring` 因 SHA-256 必链（详见 Part7 RK3/§6）。当前仅直销渠道，无此类断言对象。

### 3.6 定价落地 + 支付渠道（C6）

**3.6.1 catalog 字段落值**（Part6 T13 加字段，Part8 填值）：
- 第三方商店商品 ID（MS Store Add-on Product ID / Steam DLC app id）**当前无字段**（P16 已删）；上架该渠道时须重新引入 catalog 与 Registry 侧字段。
- 三插件 sku/价：`ai-clip-pro`=$12.9/¥89、`ai-face-pro`=$9.9/¥69、`exotic-formats-pro`=$9.9/¥69、bundle=$24.9/¥168。

**3.6.2 区域定价**：CNY = USD 40-50%（Part0 §7.2）。各渠道**价格对齐**（含抽成）——🔴 **Steam 平价口径修正（P1-10，L9 已联网核实）**：成文 Steamworks 仅约束 **Steam key 平价**（key 不得在别处更便宜）；「官网不得更便宜」**非成文公开规则**、系 Valve 施压（Wolfire v. Valve 反垄断诉讼中、未定论）→ 按实际 Steam Distribution Agreement + 法务确认，**不作公开政策事实**（原「红线 1.4.7」降为「商业策略 + 待法务」）。Store 价须把 15% 抽成计入或维持引导官网 0%。

**3.6.3 支付接入**：
- 国际：**FastSpring / Paddle**（Merchant of Record，代缴全球税 VAT/GST，省自建税务）；webhook → 签发端（§3.1）。
- 中国区：**必接微信支付 / 支付宝**（红线——否则 CNY 定价形同虚设，Part0 §7.3）；须企业资质 + 对接服务商。

**3.6.4 升级/退款**：大版本 50% 折（新 sku 或升级券）、小版本免费（同 sku 验签通过）；退款经层2 CRL 吊销（§3.3.2，仅在线激活用户可吊销，离线买断退款是业务风险，定价已含此损耗）。

### 3.7 官网 / 落地页 / 演示动图（C7；红线：诚实文案不夸大）

**3.7.1 站点形态**（从零建）：静态站（产品名确定后）——首页 + 功能页 + 定价页 + 下载页 + 购买/激活引导 + 文档/FAQ + 隐私政策。技术栈轻量（静态生成器即可，与 app 解耦）；域名不含 picasa 词根（§3.5.1）。

**3.7.2 购买引导落地页**（Store 引流核心）：
- 应用内 gate（Part5 T12）「购买」按钮 → 跳官网定价页（Store EXE 路径下收款仍走官网 0% 抽成，Part0 §9.6）。
- 「已购买激活」入口 → 粘贴 token → activate（§3.0 步③）。
- 三插件 + bundle 对比表、区域价（自动识别中国区显 CNY）。

**3.7.3 诚实文案基调**（🔴 红线 1.4.2，Part0 §7.2/§8.3 明示「写入 Part8」）：
- AI/人脸权重**部分来自公开仓库**（`gficcg/clip_cn_vit-onnx`、`opencv_zoo`）；卖点是**「开箱即用集成 + 中文优化 + 自动更新 + 体验 + 离线隐私」**，**不写「独家/秘密模型」**。
- 防护强度对外**不夸大**（§8.3）：不宣称「绝对防破解」；可写「本地离线、无云校验、尊重隐私」作为正向卖点（把「不做 kill-switch」转化为隐私优势）。
- 演示动图实拍真实功能（语义搜「夕阳」/人脸聚类/秒开 PSD），不摆拍不可达效果。

**3.7.4 定位文案**：对标 Eagle（$34.95）/digiKam（免费但重）——「核心永久免费 + 按需买断、完全离线、Win+mac 原生、<10MB 核心」。

### 3.8 获客（C8）

**3.8.1 开源 Star 漏斗**（核心策略，Part0 §10）：AGPL-3.0-only 开源核心 → GitHub 曝光冲 Stars → 大用户基数 → AI 演示价值 → 插件转化。护城河在**付费插件载荷 + 官方插件 CDN + 私有签发凭证 + 商标**，非核心代码可见性（fork 自行构建不获得官方签名发行与签发凭证，Part6）。
**3.8.2 渠道**：① 技术社群（Reddit r/selfhosted、V2EX、少数派、即刻——隐私/离线/自托管受众）；② 内容（语义搜索/人脸聚类演示视频、对标评测）；③ SEO（「Picasa 替代」「离线照片管理」「本地 AI 照片搜索」长尾——注意落地页 SEO **不蹭 picasa 商标**做付费词，仅自然内容提及）；④ Store/Steam 自带流量（上架即曝光，§3.5.2）。
**3.8.3 转化漏斗埋点**：免费核心 → gate 展示次数 → 购买页访问 → 下单。**埋点须离线友好**（不违背隐私承诺——本地计数或可选匿名遥测，默认不传）。

### 3.9 设计小结（C1-C8 落点速查）

| C | 主题 | 关键落点 | 离线 |
|---|---|---|---|
| C1 | License 签发端 | 离线 CLI/axum 微服务；签全字段 + 新增 `enc_seed`（插件主种子）；`license-*` 私钥 HSM | ✅ 可建 |
| C2 | 层1 加密打包 | `picasa-pack` AES-256-GCM + HKDF（与 Part6 解密对称）；enc_seed 按 plugin 固定 | ✅ 可建 |
| C3 | 层2 激活微服务 | axum；HMAC 不传 PII + CRL；默认离线；填 `subject_hash` | ✅ 可建 |
| C4 | MsStore/Steam Provider | 填 trait；StoreContext IAP / Steam DLC；cfg 门控；不碰 Ed25519/AES | MsStore✅ / Steam⚠️待联网 |
| C5 | 商店上架 + 改名前置 | 改名+清权门禁；EXE 路径优先；MSIX/SteamPipe 脚本；合规自检 | — |
| C6 | 定价 + 支付 | catalog 字段落值；CNY 区域；FastSpring/Paddle + 微信/支付宝；不跨渠道 | — |
| C7 | 官网/落地页/动图 | 静态站 + 购买引导；诚实文案；演示动图 | — |
| C8 | 获客 | 开源 Star 漏斗；社群/内容/SEO；离线友好埋点 | — |

---

## §4 任务清单

> 依赖标注：`← Part6 Tx` / `← Part7 Tx` 指该客户端抽象/feature 必须先实施（Part8 多数任务建立在「抽象层已落地」之上，§2.1）。`待联网` 指本机离线无法编译。

| # | 任务 | 落点 | 依赖 | 验收锚 |
|---|---|---|---|---|
| **D1** 🔴 | License 签发端（离线 CLI 起步）：签全字段 + 生成插件主种子 `enc_seed` + Ed25519 签原始 payload bytes | 新建 `tools/issuer/`（仓外或私有） | ← Part6 验签链（已成） | 签出 token 经 host `verify_token` 通过 |
| **D2** 🔴 | **验证** `LicensePayload.enc_seed: Option<String>` 字段已由 **Part6 §3.7.2/T11 加**（字段定义归 Part6 验签链，非 Part8 重复定义）；Part8 仅负责签发端**填值**（插件主种子） | license.rs（Part6 加列、Part8 填值） | ← Part6 T11（字段定义先落） | 字段存在 + `#[serde(default)]` 反序列化兼容，162 测试绿 |
| **D3** 🔴 | 层1 加密打包工具 `picasa-pack`：AES-256-GCM + HKDF(enc_seed,salt,plugin\|\|model) + `enc_salt`/`encrypted` 写 manifest | 新建 `tools/packer/` | ← Part6 §3.7 解密侧 | 打出密文 → Part6 解密侧能解（端到端对拍） |
| **D4** | 层2 激活微服务（可选）：axum + HMAC 校验 + 激活计数 + CRL；隐私政策声明 | 新建 `services/activation/`（私有） | D1 | HMAC 不传 PII；默认离线激活仍可用 |
| **D5**（未来） | 重建渠道层后填 `MsStoreProvider`：StoreContext IAP 查询 + `Services_Store` feature（cfg 门控） | 新建文件（原渠道桩模块已删） | ← 先重建渠道 feature；Part6 T9 trait 已在 | 该渠道编译；Store 环境查 IAP |
| **D6**（未来） | 重建渠道层后填 `SteamProvider`：`steamworks` DLC ownership + `SteamAPI_RestartAppIfNecessary` | 新建文件 | ← 先重建渠道 feature、待联网（steamworks 缺） | 该渠道编译（联网）；查 DLC |
| **D7**（未来） | 交付来源维度重新设计（跳 Registry 验签、保留 hash 复核） | installer.rs | ← 与 D5/D6 同批 | 平台交付路径完整性复核生效 |
| **D8**（未来） | 商品 ID 字段重新引入 + 三插件 sku 价落值 | exotic-catalog.json / RegistryEntry | ← 与 D5/D6 同批 | Provider 读到正确 product/dlc id |
| **D9** | 支付接入：FastSpring/Paddle webhook→签发；中国区微信/支付宝 | services / 官网后端 | D1 | 支付成功自动签发投递 token；🔴 webhook 验签+幂等+退款竞态+bundle 多 token（§3.3a P1-10） |
| **D10** 🔴 | 改名 + 清权执行门禁：CI 断言无 picasa 词根（🔴 P1-11：限定代码标识符+allowlist 豁免历史/SEO，**非全仓裸 grep**，§7 阶段C）+ identifier 锁定 | ci.yml / 全仓替换（Part0 §10.7） | — | 上架前断言通过 |
| **D11** | MSIX 上架：`winapp pack` → Partner Center（EXE 路径优先，MSIX 待 #14935） | 私有 CI / Partner Center | D5/D10、← Part7 commercial-release | Store EXE 路径上架 |
| **D12** | Steam 上架：SteamPipe depot upload + DLC 配置 | 私有 CI / steamcmd | D6/D10、待联网 | depot 上传、DLC 可购 |
| **D13** | 官网/落地页/演示动图：购买引导 + 激活入口 + 诚实文案 + 隐私政策 | 新建站点（仓外） | D9、产品名定 | 落地页可下单 + 激活 |
| **D14** | 获客：开源 Star 漏斗 + 社群/内容/SEO + 离线友好转化埋点 | 运营 | D13 | 直销转化漏斗可回溯 |
| **D15** 🔴 | SCRFD/ArcFace `#[cfg(feature="face-noncommercial")]` 隔离 + CI 断言不打包（商业 build 前必做） | face.rs/face_profile.rs/ci | ← Part4 T1 | 商业 build 二进制无 scrfd/arcface/w600k 符号 |

**关键路径**：D2（字段）→ D1（签发）→ D3（加密打包）= 直销四层闭环最先打通；D5/D6/D7/D8（商店 Provider+交付+字段）= 渠道扩展；D10/D15（改名+合规）= 上架硬门禁，**先于** D11/D12 上架；D9/D13/D14 = 收款+官网+获客。

> **Part8 边界自检**：D1/D3/D4/D9/D13/D14 是**新建服务端/业务/运营**（仓外或私有）；D2/D5/D6/D7/D8/D10/D15 是**仓内代码**，但全部依赖 Part6/Part7 抽象先落地（§2.1 红线）。

---

## §5 风险与缓解

| # | 风险 | 等级 | 缓解 |
|---|---|---|---|
| RK1 | **抽象层依赖未落地**：Part8 全部仓内任务建立在 Part6 T9/T13 + Part7 T10-T12 之上，若未实施则 D5-D8 悬空 | 🔴 | §2.1 红线显式标注「谁先落地」；Part8 实施前确认 EntitlementProvider/channel feature/catalog 字段已存在 |
| RK2 | **enc_seed 粒度错配**：若误按 per-license 唯一加密，CDN 密文无法复用、存储爆炸 | 🔴 | §3.2.2 钉死「enc_seed 按 plugin_id 固定（主种子）」；D1/D3 对拍验证同插件多 token 解同一密文（修正 Part4/Part6 未明确处） |
| RK3 | **签发私钥泄漏**：`license-*` 私钥泄漏 = keygen 灾难，四层防护层0 崩 | 🔴 | 气隙/HSM；与 release/Registry 私钥三套分离；永不入仓/CI；**enc_seed 持久化于 HSM/secret manager 或主密钥确定性派生**（§3.1.4，**非签完即弃**——per-plugin 主种子须可重取） |
| RK4 | **改名前误上架**：identifier 锁死，改名后须重新上架丢评分；或 Google 商标投诉下架 | 🔴 | D10 上架门禁 CI 断言；改名+清权（FTO 三辖区）公开前完成（§3.5.1） |
| RK5 | **SCRFD/ArcFace 误打包**：非商用模型进发行包 = 法律风险 | 🔴 | D15 `#[cfg(face-noncommercial)]` 隔离 + 商业 build CI 断言无 scrfd/arcface/w600k 符号（不能只靠「不可达」，Part4 T1） |
| RK6 | **Steam 离线无法构建**：`steamworks` 缺缓存 | 🟡 | D6/D12 标注待联网；CI steam job 联网拉取；Steam 本是后置渠道（曝光非主收入，§3.5.2），不阻塞直销 |
| RK7 | **MS Store 合规违规**：平台渠道 build 残留 updater/AES/keyring DRM/下载-执行代码 → 拒审 | 🟡（渠道未实现，当前无此构建） | D5-D7 重建渠道层时同批做提交前 CI 断言：`cargo tree` 无 `tauri-plugin-updater` + 无可达 AES/keyring/下载-执行路径（Part7 §3.6.2/RK3）。🔴**第7轮终审**：不扫「无 Ed25519/ring 符号」——验签原语 `verify_token` 留开源(§8.7)、`ring` 因 SHA-256 必链 |
| RK8 | **中国区支付未接**：CNY 定价无微信/支付宝 = 形同虚设 | 🟡 | D9 中国区必接（红线）；须企业资质，提前规划法务/资质周期 |
| RK9 | **PIPL 人脸合规**：中国大陆人脸功能无法律评估即发行 | 🟡 | 发行前单独法律评估（Part0 §10.5）；必要时中国区人脸插件延后或限制 |
| RK10 | **诚实文案与营销冲动冲突**：夸大「独家模型/绝对防破解」损信誉且可能被打脸 | 🟡 | §3.7.3 文案红线；把「公开权重+离线隐私」转为正向卖点，不夸大 |
| RK11 | **退款滥用（离线买断）**：纯离线 token 退款后仍可用 | 🟢 | 定价已含此损耗；层2 在线激活用户可 CRL 吊销（§3.3.2）；离线退款是可接受业务风险（§8.3 诚实上限） |
| RK12 | **支付 MoR 税务/资质**：FastSpring/Paddle 代缴税但仍需主体合规 | 🟢 | 用 MoR 降税务负担；主体注册/隐私政策（层2）属业务法务，Part8 给清单不代行 |
| RK13 | 🔴 **付费 token 泄漏 = 插件级模型密钥泄漏**：per-plugin 共享 enc_seed + token payload 明文可读 → 一个买家泄漏 token 即暴露该插件 enc_seed（解析即得，无需内存 dump），可解 CDN 全部该插件密文 | 🔴 | per-plugin 换 CDN 复用的固有代价（§3.2.2）；缓解=主种子轮换 + 可选层2 在线激活；根除须 per-device 密文/强制在线（默认离线不采用）；**文案诚实标注、不宣称强防护**（§8.3） |
| RK14 | 🔴 **冒名设备激活（层2，第 7 轮终审新增，≠RK13）**：原激活仅传 `{license_id, subject_hash}`，`license_id` 是 token 明文 + 以其为 HMAC key（公开）→ 持泄漏 token 者可对**任意硬件**自算合法 `subject_hash`、无 nonce 可重放 → 冒名消耗正版激活槽、绕过设备绑定（攻击面=**冒名激活**，区别于 RK13 的**解密权重**） | 🔴 | §3.3.1 补 **PoP**（回传完整签名 `base_token`，服务端 `verify_strict` 证持有，非仅知 license_id）+ **nonce**（防重放）；或显式下调层2「挡无限设备激活/防 token 共享/VM 直拷」表述、对齐 §8.3 诚实评级（不宣称强于实际） |

---

## §6 验收标准

**直销四层闭环（C1-C3，核心变现链）**：
- [ ] D1 签发端签出的 token 经 host `verify_token` 通过（plugin_id/sku/时间窗/签名全过）。
- [ ] D3 `picasa-pack` 加密的 ONNX → Part6 解密侧（HKDF+ring::aead）能解出明文（端到端对拍）。
- [ ] 🔴 **enc_seed 粒度验证**：同一 `plugin_id` 的两张不同 license（不同 `license_id`）的 token，**解同一份 CDN 密文**成功（证 enc_seed 按 plugin 固定，RK2）。
- [ ] DerivedSecret 门控生效：无有效 license（无 enc_seed）→ 无法派生密钥 → 密文解不开（patch bool gate 拿不到密钥，Part6 §8.2 C2）。
- [ ] D4 层2 激活：请求含 `base_token`(完整签名 token=PoP)+`subject_hash`(不可逆 HMAC)+`nonce`，无 PII；服务端先 `verify_strict(base_token)` 证持有 + 校验 nonce 防重放，再计激活数（第 7 轮终审 RK14）；**不勾选在线激活时纯离线买断仍可用**。

**商店渠道（C4-C5，未来路线）**：
- [ ] 仅当决定上架时才立项：先重建渠道 feature + cfg 物理排除 + 商品 ID 字段，再填该渠道 Provider（D5/D6）。
- [ ] 该渠道 build：`cargo tree` 无 `tauri-plugin-updater`；二进制无可达的 keyring DRM / 权重解密 / HTTPS 下载-执行(.ppx) 路径（RK7）。🔴**第7轮终审收窄**：**不**断言「无 Ed25519/ring 符号」——验签原语 `verify_token` 留开源(§8.7)、`ring` 因 SHA-256 必链（详见 Part7 RK3/§6）。
- [ ] 该渠道交付路径跳 Registry 验签但保留 hash/manifest 完整性复核（D7）。

**合规与改名（红线门禁）**：
- [ ] 🔴 D15：商业 build（含 free/付费）二进制 + bundle 清单**无** `scrfd`/`arcface`/`w600k`/`det_10g`/`insightface` 符号（CI 断言，RK5）。
- [ ] 🔴 D10：上架前 CI 断言无 picasa 词根（🔴 P1-11：限定代码标识符/产品名/Cargo/identifier/模块前缀/物料 + allowlist 豁免 docs/历史/SEO，**非全仓裸 grep**，§7 阶段C）+ `identifier` 已锁定且不再变更（RK4）。
- [ ] 改名 + 三辖区清权（USPTO/EUIPO/CNIPA + FTO 意见书）完成记录在案，方可提交任何商店。
- [ ] 中国区人脸功能 PIPL 法律评估完成（或该区人脸插件明确延后）。

**定价/支付/官网（C6-C7）**：
- [ ] catalog 三插件 sku/价落值正确；CNY 区域价 = USD 40-50%（第三方商店商品 ID 属未来，当前无字段）。
- [ ] 各渠道定价对齐（🔴 第 8 轮核验：Steam key 平价=成文；官网平价=非成文施压 / 诉讼中未定论，§3.6.2 / L9，按商业策略+法务定，**非平台硬规则**）。
- [ ] 支付成功 → 自动签发投递 token（D9）；中国区微信/支付宝可下单。
- [ ] 官网落地页可下单 + 「已购买激活」入口可用；**诚实文案**（无「独家模型/绝对防破解」表述，RK10）；隐私政策含层2 激活声明。
- [ ] 🔴 生产硬化（P1-10，§3.3a）：webhook 验签（拒伪造 webhook 铸 license）+ 幂等键（重投只签一次）+ 退款 vs 签发竞态（CRL 吊销）+ bundle 购买签 N 个独立 token（按成员插件）均有测试/验收；订单状态机 + 投递重试 + 审计/限流到位。

**获客（C8）**：
- [ ] 转化漏斗埋点离线友好（默认不传，或可选匿名）。

---

## §7 实施提示词（按依赖顺序分阶段）

> 通用前置：Part8 仓内任务**强依赖** Part6 T9/T13 + Part7 T10-T12 抽象已落地（先确认 `EntitlementProvider`/channel feature/catalog 字段存在，否则停并提示）。私钥/enc_seed/AES 主密钥/激活密钥**永不入仓、永不入 public CI**。`steamworks` 不在离线缓存（Steam 任务须联网）。改动后中文 commit，不主动 push。

### 阶段 A — 直销四层闭环（D1-D3，核心变现，离线可做）

```
任务：打通直销 license 签发 + 层1 加密闭环。前提：Part6 验签链已成（license.rs verify_token）。
1. D2：**验证** Part6 已加 `LicensePayload.enc_seed: Option<String>`（🔴 字段定义归 Part6 验签链 §2.2/§8.4，Part8 **不重复加字段**）；只确认 `#[serde(default)]` base64 反序列化兼容、不破坏现有 162 测试。
2. D1：新建签发端（离线 CLI，仓外或私有 tools/issuer）。签全字段：version=1/key_id=license-2026-01/license_id=uuid/plugin_id/sku/issued_at/not_before/expires_at=None/enc_seed。
   🔴 enc_seed 按 plugin_id 固定（插件主种子，非 per-license）：同插件所有 token 复用同一 enc_seed。私钥从 HSM 读，气隙运行。签名覆盖原始 payload bytes（serde_json::to_vec），格式 base64url(payload).base64url(sig)。
3. D3：新建打包工具 picasa-pack。per-model 32B 随机 salt → key=HKDF-SHA256(enc_seed,salt,plugin_id||model_id) → ring::aead AES-256-GCM 加密 ONNX → nonce(12B)||ct||tag(16B)；写 ModelBlob.enc_salt + manifest encrypted=true。
验收：签出 token 经 host verify_token 通过；picasa-pack 密文经 Part6 解密侧解出明文；🔴 同插件两张不同 license 解同一密文成功（证 enc_seed 按 plugin 固定）。
中文 commit。
```

### 阶段 B — 第三方商店（尚未实现）

当前只交付直销版本，没有待填充的渠道 stub、feature 或商品 ID 字段。商店上架另行立项时，按届时真实交付与平台授权需求设计实现和验证，不沿用已删除的渠道骨架。

### 阶段 C — 合规门禁（D10/D15，上架硬前置，先于上架）

```
任务：改名门禁 + 非商用模型隔离（上架绝对前置）。
1. D15：SCRFD/ArcFace 加 #[cfg(feature="face-noncommercial")] 隔离（face.rs:253-400 函数体 + 路径常量 w600k_r50.onnx/det_10g.onnx）；商业 build pipeline CI 断言 cargo tree/二进制无 scrfd/arcface/w600k/insightface 符号 + bundle 清单无对应 onnx（不能只靠 commercial_ok/downloadable 标志）。
2. D10：CI 加上架门禁断言——🔴 **限定范围**（第 8 轮核验：全仓裸 `grep -i picasa` 实测命中 172 处/48 文件，必含 plan-docs 自身/`_deprecated` 历史/迁移说明/SEO 文案/lock 文件 = 门禁恒红、不可执行）：仅断言 **代码标识符 + 产品名 + Cargo/package name + tauri.conf.json identifier + `picasa_next` 模块前缀 + 品牌物料** 无 picasa 词根，配 **allowlist** 豁免（plan-docs / 历史 / 兼容 / SEO 文本保留）+ tauri.conf.json identifier 已锁定。改名执行归 Part0 §10.7（repo/Org/域名/Cargo name/identifier/picasa_next 模块前缀/物料一次性替换 + 三辖区清权）。
验收：商业 build 二进制无非商用模型符号；上架前断言通过。
中文 commit。
```

### 阶段 D — 支付/激活/上架/官网/获客（D4/D9/D11-D14，业务+服务端，多在仓外）

```
任务：收款链路 + 可选激活 + 上架 + 官网 + 获客。多为仓外/私有服务。
1. D9 支付：FastSpring/Paddle webhook → 签发端自动签发投递；中国区接微信/支付宝（企业资质）。
2. D4 层2 激活微服务（可选，axum）：🔴 **按 §3.3.1 正文流程**（第 8 轮核验：回填 RK14 的 PoP+nonce，删旧 `{license_id, subject_hash}` 口径——否则照此提示词照抄会重引可冒名激活/可重放洞）——先 `GET /activate/challenge{license_id}` 取一次性 nonce，再收 `{base_token, subject_hash=HMAC(license_id,cpu+disk), nonce}`；服务端 ① `verify_strict(base_token)` 验签（PoP，从 token 取 license_id，**非**裸 license_id）② 校验 nonce 未用且未过期（防重放）③ 激活计数≤N、记 subject_hash；不收 PII；CRL 退款吊销；默认离线仍可用；隐私政策声明。
3. D11 MSIX 上架：winapp pack → Partner Center（EXE 路径优先，MSIX 待 #14935）。D12 Steam：SteamPipe depot + DLC（联网）。
4. D13 官网/落地页/演示动图：购买引导（Store 引流官网 0%）+ 激活入口 + 🔴 诚实文案（不写独家模型/绝对防破解，把公开权重+离线隐私转正向卖点）+ 隐私政策。
5. D14 获客：开源 Star 漏斗 + 社群(r/selfhosted/V2EX/少数派)/内容/SEO(不蹭 picasa 商标付费词) + 离线友好转化埋点。
验收：支付自动签发；层2 不传 PII 且离线可用；落地页可下单+激活；文案无夸大；漏斗可回溯。
中文 commit。
```

---

## §8 与其他 Part 的接缝（一致性自检）+ 9-Part 收官说明

| 接缝 | 约定 | 风险点 |
|---|---|---|
| ← **Part6**（EntitlementProvider trait/ActivationInfo/AES 解密/catalog 字段/pro 下沉） | Part8 填 Ms/Steam 实现 + 签发/加密侧；以 §8.3 权威签名为准 | 抽象未落地则悬空（RK1） |
| ← **Part7**（CI 门控 / commercial-release 产线 / 平台签名） | Part8 接 winapp pack/SteamPipe 脚本 + 合规自检（渠道层须先重建） | 平台渠道残留 DRM 符号（RK7） |
| ← **Part4**（未来加密权重消费（当前无返回字段预留）/DerivedSecret/SCRFD 隔离 T1） | Part8 供 enc_seed 签发值 + D15 落实隔离 | enc_seed 粒度（RK2）；SCRFD 误打包（RK5） |
| ← **Part5**（插件商店/gate/购买引导 T11/T12） | Part8 官网落地页承接 gate「购买」跳转 + 激活入口 | 前端商店未落地则购买引导断裂 |
| ↔ **Part0**（§7 定价/§8 防护/§9 渠道/§10.5 PIPL/§10.7 改名） | 全部锚点对齐；诚实文案/防护强度不夸大 | 改名时序（RK4）；夸大文案（RK10） |

**🔴 反向回溯到既有 Part 的发现**（Part8 取证暴露，建议回补）：
1. **`enc_seed` 粒度**（✅ 已回补）：Part8 §3.2.2 钉死「按 plugin_id 固定（主种子）」——否则 CDN 密文无法复用。Part4 §3.7.2 / Part6 §3.7.2 **已同步回补 per-plugin 粒度**（终局 review）；本条留作历史记录。
2. **`LicensePayload.enc_seed` 字段定义归属**：消费侧（Part4/Part6）读它，签发侧（Part8）填它——字段**定义**（结构加列）应在客户端 Part（Part6 验签链所在），Part8 只填值。D2 任务已据此安排。

### 9-Part 收官说明

至此 **Part0-8 全部成稿**。9 份文档构成可执行级重构方案：Part0 总纲与产品定稿 → Part1 数据层 → Part2 扫描画廊 → Part3 缩略图 GPU → Part4 AI 人脸插件化 → Part5 前端体验 → Part6 插件平台 exotic 收尾 → Part7 发布工程 → **Part8 商业化分发**。

**实施总依赖序**（变现视角）：Part1（schema）→ Part6（插件平台抽象：EntitlementProvider/keyring 授权/catalog/worker_id）→ Part7（workspace/签名/CI）→ Part4（AI/face worker + SCRFD 隔离；加密权重属未来研究，当前无字段预留）→ **Part8（签发/收款/官网）**。直销闭环（Part8 D1 + D9/D13）是变现最短路径；商店渠道（D5-D8）是独立未来路线，须先重建渠道层。

<!-- Part8 全文定稿待执行（terminal review 已并入；9-Part 收官）。 -->
