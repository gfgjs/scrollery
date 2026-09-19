---
id: 2026-07-06-D1-签发机与key-ceremony
status: active
type: runbook
line: 生产信任根 D1
created: 2026-07-06
---

# D1 签发机与 Key Ceremony(生产信任根落地手册)

> 2026-07-06 交付。定位:生产签发基础设施的**我方全部准备**——工具链、流程、彩排证据。
> 剩余动作只有一个:你在离线机上按 §2 跑一次 ceremony(半小时),把 keyset 运回来。
> 红线(不变):**私钥永不入仓、不进 CI env、不出现在测试 fixture**。签发私钥与私密凭证不入任何源码仓(含公开镜像);**keyset 是验签公钥集,可随源码公开**——须保护的从来是私钥,不是公钥。注入为**整组替换**(build.rs 整体烧录):本次归一保留既有 key ID、公钥、用途和有效期;**轮换时若旧 token 需继续可验,须把历史键一并纳入新 keyset**。

## 0. 工具链一览(全部已就绪)

| 件 | 路径 | 职责 |
|---|---|---|
| ceremony 脚本 | `scripts/exotic-prod-ceremony.mjs` | **自包含单文件**(离线机只需它 + Node ≥18):`init` 生两对 Ed25519 钥 + keyset + 留痕;`verify` 私钥↔keyset 配对自查;`verify-token` 只用公钥验 token(可在开发机跑) |
| 签发脚本(已硬化) | `scripts/exotic-issue-license.mjs` | 新增 `--key <pem> --key-id <id>` 生产形态:缺文件 fail-fast **绝不自动生钥**;不带参数则维持内测形态零改动 |
| 注入通道 | `PICASA_EXOTIC_KEYSET_FILE`(exotic-trust build.rs) | 编译期把 keyset **整组**烧进二进制;fail-fast + `"keys"` 哨兵 + cargo:warning 留痕已就绪;**本次归一保留既有 key ID、公钥、用途和有效期;轮换时须保留需继续信任的历史键** |
| 公钥可见性 | keyset 为**验签公钥集,可随源码公开**(Part0 §10) | 只有私钥与私密签发凭证属秘密:气隙/HSM、不入仓、不入任何 CI。注入为**整组替换**,本次归一保留既有 key ID、公钥、用途和有效期;**轮换时须把需继续信任的历史键纳入新 keyset** |

## 1. 签发机准备(一次性,用户侧)

- 一台**不再联网**的机器(旧笔记本即可):重装或离线化后装 Node ≥18(离线安装包 U 盘带入),此后网卡禁用/物理断网。
- 磁盘全盘加密(BitLocker/VeraCrypt);机器物理保管等同贵重物品。
- 两只专用 U 盘:A 盘「摆渡」(只进出 ceremony 脚本与公钥产物),B 盘「冷备」(只存私钥备份,异地保管,永不插联网机)。

## 2. Ceremony 步骤(半小时)

1. A 盘拷入 `exotic-prod-ceremony.mjs` → 签发机上 `node exotic-prod-ceremony.mjs init --tag 2026-07`。
2. 核对输出:selftest ✓、两把 key_id(`release-prod-2026-07` / `license-prod-2026-07`)、指纹。
3. `node exotic-prod-ceremony.mjs verify` → 两行 ✓(公钥比对一致 + 签验往返通过)。
4. **抄纸**:ceremony-record.txt 的两枚公钥 sha256 指纹 + keyset sha256(纸与机器分开存放)。
5. 私钥冷备:`prod-signing/` 整目录拷 B 盘 → B 盘异地保管 → record 上手写勾选。
6. **出机只带两件**:`exotic-keyset-prod.json` + `ceremony-record.txt`(经 A 盘)。私钥 pem 永不出机(冷备 B 盘除外)。
7. 开发机上核对 keyset sha256 与纸面一致 → 替换 pro resources 占位 **或** 配置发布流水线 `PICASA_EXOTIC_KEYSET_FILE` 注入 → 构建日志确认 `cargo:warning=pro 生产信任根已由构建注入`。

## 3. 日常签发工作流(签发机上)

```
node exotic-issue-license.mjs --key prod-signing/prod-license.pem --key-id license-prod-2026-07 \
  --plugin <id> --sku <sku> [--days N]
```
token 文本经 A 盘/抄写/二维码出机交付用户。建议在签发机上维护一份手工 `issuance-log.txt`(日期/license_id/plugin/sku/订单号),对账靠它。开发机可随时 `verify-token <token> --keyset <keyset>` 复核真伪(不需私钥)。

## 4. 备份·轮换·吊销

- **备份**:私钥仅两份(签发机 + B 盘冷备);任何第三份都是新的攻击面。
- **轮换**:新 tag 走新 ceremony(`init --out prod-signing-2027 --tag 2027-01`),新 keyset = 新旧两代键并列(旧 `not_after` 设截止/新 `not_before` 设启用),随下个版本发布;脚本对已有 pem **拒绝覆盖**,防手滑原地换钥作废全部已售 license。
- **吊销**:泄露即把该键 `status` 置 `revoked` 发新版本(Rust 侧 `KeyStatus::Revoked` 拒验已就绪);license 键泄露须同时准备重发用户 token 的通道。

## 5. 彩排证据(2026-07-06,临时钥全链,scratchpad 隔离)

| 环节 | 结果 |
|---|---|
| `init --tag rehearsal` + `verify` | ✓ selftest(spki 往返+签验往返+条目形状)/ 两键公钥比对一致 + 签验往返通过 |
| 生产形态签发(`--key --key-id`) | ✓ token 产出,key_id=license-prod-rehearsal |
| `verify-token` 正样本 | ✓ 验签通过(keyset 公钥侧闭环:keyset↔私钥配对实证) |
| `verify-token` 负样本(篡改 1 位) | ✓ 验签失败 exit 1(拒伪造实证) |
| 注入编译 `PICASA_EXOTIC_KEYSET_FILE` → `cargo test -p pro -p exotic-trust` | **首跑红,抓到真雷**(见下)→ 修后 ✓ exit 0 |
| 未注入回归(默认占位集) | ✓ exit 0(占位 key_id 钉死断言保留) |

**彩排抓到的真雷**:`exotic-trust::crypto::tests::builtin_keyset_parses` 原来把占位 key_id `release-2026-01` 断言死——内测链因「超集 keyset」侥幸绕过,真钥日的纯生产 keyset 必在此编译红;而为过测把占位键并进生产信任根属倒因为果(占位键公开,等于给信任根留门)。修法:注入无关断言改为「两用途各 ≥1 把 active 键」,占位 id 钉死断言仅在未注入构建生效(`option_env!` 门控)。**此雷若留到真钥日才发现,会在最敏感的换钥操作里逼出「往生产 keyset 塞占位键」的错误捷径——彩排的价值即在此。**

## 6. 与在案事项的衔接

- 本手册完成后,todo.md B1 真钥行的阻塞面收窄为「用户跑 §2」;签发机硬件属 G3 基建清单。
- 改名(Scrollery)不影响本链:`PICASA_EXOTIC_KEYSET_FILE` 在改名施工 §2-E 缓改豁免面内;key_id 命名与产品名无关。
- 生产 CDN/支付侧的 D4 激活微服务(可选)与本离线链解耦,归 Part8 后续。
