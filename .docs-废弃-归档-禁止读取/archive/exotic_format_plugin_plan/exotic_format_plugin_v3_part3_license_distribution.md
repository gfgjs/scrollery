# 冷门格式插件子系统 v3.1 · Part 3 商业与分发（P5–P6）

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 范围：信任模型、License、签名 Catalog/Registry、包清单、安全安装、升级/回滚/卸载、合规材料。
> 前置：Part1 Catalog/task 与 Part2 Worker/Pipeline 全部完成。
> 版本：v3.1（已并入 `exotic_format_plugin_plan_v3.1_addendum.md` 的 R8/R10/R11）。
> 发布范围：只接受产品厂商签名 key；第三方原生 Worker 不开放。

## 本卷完成定义

1. Release 二进制中不存在 dev 授权绕过路径。
2. License 同时校验 token version、key_id、plugin_id、sku、有效时间与签名。
3. Registry index 被签名，具 sequence/expiry；回滚、冻结、篡改被拦截。
4. Package 在 staging 中验证；路径穿越、符号链接、额外 DLL、zip bomb、hash 不符均被拒绝。
5. 安装/升级原子切换，可回滚；Windows 运行中 exe 不会直接覆盖。
6. 激活、安装、升级完成后均唤醒 Coordinator。
7. Windows/macOS Worker 完成平台代码签名与实际启动验证。
8. PSD 包具备 SBOM、许可证、审核编号；Plan 不以布尔字段替代法律审核。

## 单调量 / 版本名词表（R11）

v3 多处「sequence / version」易混，统一定义（schema 字段命名以此为准）：

| 名词 | 含义 | 单调性 / 作用 |
|---|---|---|
| `schema_version` | 本地 DB 迁移版本（`app_config`） | 本地递增（落地为 V9） |
| `catalog_sequence` | 能力目录版本（内置/远程合并取大） | 防目录回滚 |
| `package_sequence` | 单个插件包安全单调序号 | 安装只允许更高，防包回滚 |
| `protocol_version` | IPC 帧协议版本 | 握手协商，非单调 |
| License token `version` | License payload 结构版本 | 验签先校 |
| `min_host_version` | 包要求的最低 Host 版本 | 兼容门控 |
| `worker_version` | Worker 二进制版本 | 入指纹，升级触发重算 |

「版本字符串」只用于展示；一切**安全单调性**判断只看对应 sequence。

## Phase 5 — 信任根、License 与合规

### 5.1 信任根分离

至少使用两组 Ed25519 key：

| Key | 用途 | 在线性 |
|---|---|---|
| Release metadata key | 签 Registry index 与 package manifest | 离线/HSM；发布流水线受控调用 |
| License key | 签用户 License token | 独立离线服务或受控签发机 |

公钥 keyset 编入 Host，包含 `key_id/status/not_before/not_after`。支持新旧 key 重叠轮换；删除旧 key 前必须保证已发行永久 License 仍可验证，或提供签名 keyset 更新机制。

私钥不入仓、不写普通 CI 环境变量、不出现在测试 fixture。测试使用独立测试 key，Host Release 不包含测试公钥。

### 5.2 License token

编码：

```text
base64url(payload_json_utf8) + "." + base64url(ed25519_signature)
```

签名覆盖收到的原始 payload bytes；Verifier 不重新序列化后验签。签发工具负责稳定字段顺序，但安全性不依赖 JSON 重新 canonicalize。

```json
{
  "version": 1,
  "key_id": "license-2026-01",
  "license_id": "lic_...",
  "plugin_id": "exotic-image-psd",
  "sku": "psd-engine-2026",
  "subject_hash": "sha256:...",
  "issued_at": 1782345600,
  "not_before": 1782345600,
  "expires_at": null
}
```

校验顺序：结构/大小 → version/key_id → `verify_strict` 或库等价严格验签 → plugin_id → manifest/Catalog sku → not_before/expires_at。错误码区分 missing/malformed/unknown_key/bad_signature/plugin_mismatch/sku_mismatch/not_yet_valid/expired/keyring_unavailable。

token 存 keyring：service 固定，account=`plugin_id`。DB 只缓存授权结果与检查时间，不保存 token。日志、遥测、panic、IPC 错误不得输出 token 或 subject_hash。

永久授权为 v3 首发主路径。若启用订阅：承认离线系统时钟可回拨、无实时撤销；必须另写 grace/revocation/可信时间产品策略，不能只增加 `expires_at` 就宣称完成订阅控制。

### 5.3 校验时机

- App 启动 Host refresh；
- 激活输入时；
- 每批 task 领取前；
- 每个 Worker 启动前；
- 长驻 Worker 派发下一任务前。

License 过期不把 task 标 error；FormatResolution 变 `LicenseExpired`，停止新派发，在途任务可完成或按产品策略取消。重新激活后 Coordinator wake。

### 5.4 Release 禁止 dev bypass

开发 fixture 只允许：

```rust
#[cfg(all(debug_assertions, feature = "exotic-dev-fixtures"))]
```

CI 增加 Release 二进制/源码配置测试：不能注册 `set_exotic_dev_mode` 命令、不能读取 `exotic_dev_mode` DB key、不能包含测试公钥。数据库手改不得产生 Authorized。

### 5.5 合规记录

删除 manifest 自声明 `commercial_ok` 作为运行依据。每个 `(plugin_id, version, target)` 发布前生成：

- SPDX/CycloneDX SBOM，含完整依赖版本、feature、静态/动态链接方式；
- 第三方 LICENSE/NOTICE；
- LGPL/GPL 等适用义务清单、源码或重链接材料；
- 专利/商标/格式许可审核记录；
- `compliance_review_id`、审核人、日期、适用地区；
- 构建产物 hash 与可复现构建信息。

Registry 只发布审核通过的包。`compliance_review_id` 进入签名 package manifest；客户端可展示，不自行判断法律结论。

“用户自行安装 GPL 程序”“LGPL 动态链接”“调用 OS 解码 API”均需要按具体版本、组合方式、分发地区审核。进程边界或 OS API 不自动产生法律豁免。

## Phase 6 — 签名 Registry 与安全安装

### 6.1 签名 Registry index

Registry 文件：`index.json` + `index.sig`。签名覆盖原始 index bytes。

```json
{
  "schema": 1,
  "key_id": "release-2026-01",
  "sequence": 42,
  "generated_at": 1782345600,
  "expires_at": 1784937600,
  "plugins": [
    {
      "plugin_id": "exotic-image-psd",
      "version": "1.0.0",
      "package_sequence": 3,
      "media_kind": "image",
      "formats": ["psd"],
      "capabilities": ["thumbnail"],
      "sku": "psd-engine-2026",
      "min_host_version": "0.1.0",
      "target": "x86_64-pc-windows-msvc",
      "package_url": "https://cdn.example.invalid/exotic-image-psd-1.0.0-win.zip",
      "package_size": 7340032,
      "package_sha256": "...",
      "store_url": "https://store.example.invalid/plugins/psd"
    }
  ]
}
```

规则：

- 先验签，再解析/使用 URL；前端只能传 plugin_id，不能传任意下载 URL；
- 只接受 HTTPS；重定向后仍须 HTTPS；设置连接、总下载、空闲超时；
- sequence 小于本地最高已接受值即拒绝，防回滚；
- index 过期时允许展示缓存与已安装插件，但不允许从过期元数据执行新安装；
- 远程 offering 不能覆盖内置常见格式或改变既有 plugin_id 的 media_kind；
- 本地缓存采用临时文件 + 原子替换，并保存最后有效签名与 sequence。

### 6.2 通用下载器重构

现有 `crate::ipc::ai_commands::download_assets`（`ai_commands.rs:958`）绑定 `crate::ai::profile::ModelAsset`（`ai/profile.rs:38`）与进度结构 `DownloadProgress`（`ai_commands.rs:846`），不能由 exotic 直接套用（R8 真实路径）。先抽：

```text
src-tauri/src/download/mod.rs
  DownloadAsset { id, urls, dest, size, sha256 }
  DownloadProgress { download_id, file, received, total, state }
  download_assets(...)
  sha256_file(...)
```

AI/face/exotic 适配到通用接口。保留 Range、镜像回退、`.part`、大小/hash、节流进度；增加总下载字节上限、响应 Content-Length 检查、取消清理与目标目录预创建。此次重构须有 AI/face 回归测试。

顺序与契约（R10）：**先抽通用接口、让 AI/face 行为与事件不变地适配，再供 exotic 复用**。前端 AI 下载 UI 依赖 `DownloadProgress` 的字段形状与事件名——通用化后字段契约**不得破坏**；exotic 若需额外字段，加可选字段、不改原义。`ModelAsset` 解耦为通用 `DownloadAsset` 时保留 AI 侧适配层。

### 6.3 Package 内容

```text
package.zip
├── package-manifest.json
├── package-manifest.sig
├── plugin-manifest.json
├── bin/<target>/psd-worker[.exe]
├── icon.png
├── SBOM.spdx.json
├── LICENSES/...
└── COMPLIANCE.json
```

`package-manifest.json` 的 `files` 列出全部 payload 文件；仅排除无法自哈希的 `package-manifest.json` 与对应 `.sig`。每项包含规范相对路径、size、sha256、kind、executable。动态库、配置、资源、许可证全部在清单内；不只 hash 主 Worker。

```json
{
  "schema": 1,
  "key_id": "release-2026-01",
  "plugin_id": "exotic-image-psd",
  "version": "1.0.0",
  "package_sequence": 3,
  "target": "x86_64-pc-windows-msvc",
  "min_host_version": "0.1.0",
  "protocol_version": 1,
  "compliance_review_id": "review-2026-psd-001",
  "files": []
}
```

### 6.4 安全安装算法

1. 从已验签 Registry 按 plugin_id/当前 target 选择条目；检查 Host 版本与 package sequence。
2. 下载 zip 到 app data 的 downloads staging；校验 signed index 中的 size/SHA-256。
3. 只从 archive 中有界读取 `package-manifest.json/.sig`；先验签再信任其他 entry。
4. 校验 manifest 的 plugin_id/version/target/protocol/min_host/catalog formats 与 Registry 一致。
5. 扫描 central directory：entry 集合必须等于签名 files 清单加两份签名元数据；拒绝额外文件、重复规范路径、绝对路径、盘符、UNC、`..`、NUL、保留设备名、符号链接、硬链接、reparse point。
6. 校验单文件大小、总展开大小、文件数、压缩比上限，防 zip bomb。
7. 解包到随机新建 staging 目录；每个文件使用安全相对路径和 `create_new`；写入时同步计算 hash/size。
8. 全文件复核；解析 plugin manifest，确认 formats/capabilities 是 Catalog 子集；检查 Worker 平台代码签名。
9. 请求 Coordinator quiesce 该 plugin，终止并 wait 全部 Worker，释放 Windows 文件句柄。
10. `current → backup`、`staging → current` 原子切换；DB 事务更新安装记录；Host refresh；健康握手。
11. 健康检查失败：恢复 backup 与 DB；成功后保留有限期 rollback 包并清理旧版本。

解包失败、取消、App 崩溃后的 staging/backup 恢复必须有启动维护流程。安装目录名只使用已验证 plugin_id，不直接拼接前端输入。

### 6.5 升级、降级与卸载

- 默认只允许更高 `package_sequence`；版本字符串只用于展示，安全单调性看 sequence。
- 用户显式回滚只能选本机已验证 backup，且记录原因；远程低 sequence 包不可安装。
- 升级后按 `worker_version/capability_api_version` 重新计算任务指纹；只失效受影响 capability。
- 卸载：先 pause plugin → kill/wait Worker → 原子移走目录 → 更新 DB/Host → Coordinator wake → 异步删除 tombstone。
- 卸载默认保留 License token，提供单独“移除授权”操作；UI 明示差异。
- 卸载不删除媒体记录；已生成缩略图是否保留由产品策略决定，默认保留并标记产物来源版本。

### 6.6 激活命令

```text
list_exotic_registry()
install_exotic_plugin(plugin_id)
activate_exotic_plugin(plugin_id, token)
repair_exotic_plugin(plugin_id)
rollback_exotic_plugin(plugin_id)
uninstall_exotic_plugin(plugin_id, remove_license=false)
```

命令参数永远不接受 URL、目标路径、hash 或可执行路径。安装成功与激活成功都调用 `coordinator.wake(...)`。激活失败不覆盖 keyring 中现有有效 token。

### 6.7 平台签名

Windows：Worker 与 DLL 使用 Authenticode；Host 验证签名链、预期发布者和文件 hash。实测 Defender/SmartScreen、无控制台窗口、升级时文件锁释放。

macOS：Worker 作为适当签名 bundle/可执行分发，使用 hardened runtime、Developer ID、notarization/stapling；在干净机器通过 Gatekeeper `spctl` 与实际 `Command::spawn`。仅 `chmod +x` 不算发布完成。

平台代码签名是 OS 信任与发布体验层；不能替代 package manifest hash/Ed25519 元数据签名。

## 测试

### License/密钥

- payload 篡改、签名篡改、未知/过期 key、错误 plugin/sku、not_before/expiry；
- keyring unavailable/locked；有效旧 token 不被失败激活覆盖；
- Release 无测试 key、feature bypass、DB dev-mode 读取路径；
- key 轮换：新旧 token 在窗口内验证，撤销 key 按策略失败。

### Registry

- index 篡改、sequence 回滚、expiry、缓存原子替换失败；
- 恶意 URL/HTTP/重定向、超大 size、下载取消、Range 服务异常；
- 远程 Catalog 试图覆盖常见格式或改变 media_kind。

### Package 攻击集

- `../`、绝对路径、Windows 盘符/UNC/设备名、大小写碰撞、Unicode 规范化碰撞；
- symlink/hardlink/reparse、重复 entry、额外 DLL、缺文件、hash/size 不符；
- zip bomb、超高压缩比、超多小文件、截断 central directory；
- 安装中断、切换中断、健康握手失败、backup 回滚；
- 运行中升级/卸载与 Windows 文件锁。

## 本卷产出清单

- [ ] 分离的 Release/License keyset 与轮换字段
- [ ] License 严格验签、plugin+sku+时间校验、keyring 错误处理
- [ ] Release 无 dev bypass 自动检查
- [ ] 签名 Registry sequence/expiry/cache
- [ ] 通用下载模块；AI/face 回归
- [ ] 全文件签名 package manifest
- [ ] staging 安全解包、原子安装、健康检查、回滚
- [ ] 升级/卸载 Worker quiesce 生命周期
- [ ] Windows Authenticode 与 macOS notarization 实机记录
- [ ] PSD SBOM/许可证/合规审核材料
- [ ] License、Registry、恶意 Package 测试通过

## 续作提示词

```text
实施 plan-docs/exotic_format_plugin_plan/exotic_format_plugin_v3_part3_license_distribution.md。
前置：Part1/Part2 全部完成。
先完成信任根与恶意包测试，再写安装算法；禁止先解包后验签。
Release 不得包含测试 key 或 dev 授权旁路；每阶段中文 commit。
完成后进入 exotic_format_plugin_v3_part4_frontend_release.md。
```
