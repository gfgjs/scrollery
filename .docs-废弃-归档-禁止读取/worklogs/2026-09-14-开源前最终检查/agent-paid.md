---
id: 2026-09-14-开源前最终检查-收费专项
status: snapshot
type: review
line: 渠道与开源边界
created: 2026-09-14
---

# 收费项目代码盘点（任务B，只读）

基准：2026-09-14 工作树。以代码实际门控为准。不改产品、不改门控。

## 1. 功能级授权项：catalog 3 个 paid + 1 个独立虚拟 feature

Cold-format catalog（`src-tauri/resources/exotic-catalog.json`）内 `license_tier: "paid"` 的条目共 **3 个**：PSD、OCR、增强。图片编辑**不在 catalog**，它以固定虚拟 feature id / SKU 复用共享 provider（editing/entitlement.rs:3-4,10-12），因此写作 3+1，不把 catalog 记作 4 条。

| 授权项 | 归属 | 功能 | 未授权时的免费基线 | 付费触发 | 价格 | 接通状态 | 可售性 | 证据 |
|---|---|---|---|---|---|---|---|---|
| `psd-engine-2026` | catalog paid | PSD 图像引擎 | 该 PSD 引擎未授权，走 InstalledUnlicensed/AvailableUninstalled | token 验签 expected_sku | 无 | 仅 direct（包安装） | 占位：store_url=example.invalid | exotic-catalog.json:20-33；exotic/mod.rs:406-418 |
| `ocr-engine-2026` | catalog paid | OCR 文字提取 | 该 OCR 引擎未激活时 ocr_* 命令回 ocr_unlicensed | 同上 | 无 | builtin，无安装包 | 占位：store_url=example.invalid | exotic-catalog.json:35-47；ipc/ocr_commands.rs:60-75 |
| `enhance-engine-2026` | catalog paid | 影像增强（降噪/超分） | 该增强入口未授权时 enhance_start/preview 回 enhance_unlicensed | 同上 | 无 | builtin | 不可售：模型资产源为占位 | enhance_commands.rs:33-45；enhance/registry.rs:20,31-40 |
| `editing-tools-2026` | 独立虚拟 feature（非 catalog） | 图片编辑高级功能（旋转/翻转/裁剪/拉直/调色） | 该授权入口覆盖的编辑保存不通过 | 同上 | 无 | 编译在 Host 内 | 占位：store_url=None，购买按钮禁用 | editing/entitlement.rs:10-12,41,46-56；edit_commands.rs:220,391 |

免费内建（无授权门）：`exotic-image-raw`（RAW 解码，exotic-catalog.json:49-60）、`video-extended`（rmvb/vob 视频扩展，:63-74）。

门控顺序实现：exotic/mod.rs:360-419（平台→Host 版本→dev fixture→安装态→授权）。builtin free 显式判 `license_tier=="free"` 才放行（:380-384），paid 缺 SKU 一律 fail-closed（:385-387、:407-409）。dev fixture 分支仅 test / debug+feature 存在（:374）。

## 2. 发行级收费计划（非功能 SKU）

| 项目 | 内容 | 价格 | 接通状态 | 证据 |
|---|---|---|---|---|
| 官方稳定版 | 计划一次性付费购买；预览构建免费 | 未公布 | 仓内无对应门控；一次性销售可在下载/商店侧完成 | README.zh-CN.md:16-17,23 |
| 未来独立商业组件 | 另定许可，不预先承诺开源 | 未公布 | 仓内无对应目录/清单项 | README.zh-CN.md:19 |
| 未来在线/托管服务 | 可能独立收费 | 未公布 | 仓内无实现 | README.zh-CN.md:21 |

发行级与功能级是两条独立线：前者承诺官方安装包与更新等服务，后者是应用内的能力授权。官方安装包的一次性销售在下载或商店侧即可完成，本轮未发现必须再加一道应用启动权益门的证据。

## 3. 缺失与阻断项

- 仓内未定价：exotic-catalog.json 无 price/amount/currency 字段（匹配数 0），catalog.rs 结构体只有 store_url / store_product_id / steam_dlc_app_id（:134-140）。定价可在外部分发/商店侧维护，仓内无价格字段不构成本身缺陷；仓内能确认的只是「未在本仓定价」。
- 三个 catalog 条目的 store_url 全为 `https://example.invalid/...`（exotic-catalog.json:16,32,45）；PluginGate 在无 URL 时禁用按钮并提示 `exotic.gateNoStore`（src/components/exotic/PluginGate.vue:35-36,104）。这是直销接通的直接阻碍点。
- Steam/MS Store 未接通：渠道授权为桩（exotic/channel_stubs.rs:22,37,50），非 direct 安装路径 fail-closed（exotic/installer.rs:707-725），Steam 启动自检仍是占位日志（lib.rs:112-123）。
- 增强模型不可下载：`PENDING_REPO_BASE` 指向 `huggingface.co/PENDING_USER_REPO`，`manifest_is_ready` 因 PENDING_/零字节/空 sha256 恒 false（enhance/registry.rs:20,31-40,54-55）。即付费也无载荷可交付。
- OCR 资产已钉定（真实 sha256/字节数，ai/ocr_registry.rs:42-86），与增强形成对照。

## 4. 未列入收费的能力（本轮未发现授权门）

AI 语义检索、人脸检测/聚类、人员库：ai_commands.rs / face_commands.rs 中未出现 `Availability`/`require_*` 授权门控调用；face 侧 `license` 字段是模型许可元数据（face_commands.rs:591），非付费权益。结论面向当前代码，不推断未来规划。

关于时效：未发现订阅类 SKU，不等于 token 不能到期。验签返回 Expired 时前端与命令映射为「续订」文案（如 enhance_commands.rs:36-39），这只是过期提示语，不代表仓内已实现按周期计费的订阅流程。

## 5. 给主会话的取舍影响面（未实施）

1. 若按功能授权收费：直销侧需把 example.invalid 换成真实商店页（价格可由商店侧承载）。
2. 若按发行级一次性付费：现有 3+1 授权门可保持不变，销售与交付在下载/商店侧完成即可，未见必须新增应用启动权益门的证据；两者对外口径需先统一。
3. Steam/MS Store 收费：需实装 channel_stubs 与 installer 的 fail-closed 分支，属 Part8 未开工范围。
4. 增强收费：资产源定案（仓名+sha256+bytes）前不具备可售交付物，收费先于交付会形成空门。

## 6. 验证与未验证

验证方式：静态阅读 + 精确检索（`rg` 定位门控与字段、逐行读关键函数），未运行构建或测试。
未验证项：运行时授权行为、keyring 实际读写、真实注册表/签发流程、release.yml 与 README 宣传的一致性（该部分由主会话负责写入）。
