---
id: 2026-07-17-status-refactor_2026
status: active
type: rolling-status
line: refactor_2026
created: 2026-07-17
---

# refactor_2026 · 滚动状态

> 状态板 2026-08-24 自 docs/todo.md「Part6 红线收尾 / Part6 其他剩余 / Part5 剩余」整体迁入(D-016 分片);已完成项交付史与 ▸ 详注指针仍归 completed.md。

## Part6 红线收尾

### B1. ③b 闭源物理下沉(耦合变现;外向/难回滚)

 > 🔴 **本子节已被 2026-09-12 裁决取代**:主程序第一方源码统一开源,授权实现归一为 `KeyringLicenseStore`(唯一 direct 真实现)+ `channel_stubs` fail-closed 回退;`crates/scrollery-pro` 与 `crates/scrollery-free-stub` 已删除,Copybara 只做内部文件过滤(不再剥离源码/lock)。**2026-09-14 该范围许可改定为 AGPL-3.0-only(另设单独商业授权)**;现行规范见 [Part0 §10](../refactor_2026/Part0_总纲与产品定稿.md)。下表为 2026-07-05 当时的③b 形态留痕:

> 🟢 **③b 已落地(2026-07-05,`7a428e6`)**:组合根标记块 swap 闭源 DirectEntitlement + pro 入 workspace + copy.bara.sky 增 Cargo.lock 剥离(第四层防线)。**裁决(用户在环)**:KeyringLicenseStore 保留不删、开源默认不切 FreeStub(fork 可自建签发链);真管线实跑收官(私有 CI→main ff `23bdd32`→sync-oss 实推→公开 `e663e22` 三处抽查剥离逐字节精确→公开 main 一致);本地四门+剥离模拟 `--locked` 绿。→ **▸ 详注 B1-1**

| 状态 | 待办 | 阻塞源 / 备注 |
|---|---|---|
| 📦 | `KeyringLicenseStore` 迁入 pro 成 `DirectEntitlement` | **历史留痕**:当时裁决为 pro 侧对等实现 + swap 接线(`7a428e6`);2026-09-12 改为授权实现归一——`KeyringLicenseStore` 作唯一 direct 真实现,pro/free-stub 双 crate 已删 |
| ⬜ | 占位公钥集 → 替换为受控签发机(离线/HSM)**真实生产公钥** | **唯一外部前置**。注入通道就绪:`PICASA_EXOTIC_KEYSET_FILE` 编译期注入(exotic-trust build.rs);**2026-07-06 D1 工具链+彩排收官**:ceremony 脚本(`exotic-prod-ceremony.mjs`,自包含单文件,init/verify/verify-token)+ 签发脚本生产形态硬化(`--key/--key-id`,fail-fast 禁自动生钥)+ 临时钥全链彩排(签发→verify-token 正负样本→注入编译双态)全绿;**彩排抓真雷**:`builtin_keyset_parses` 钉死占位 key_id,真钥日纯生产 keyset 必编译红——已改注入无关断言(两用途各≥1 active 键,占位 id 仅未注入构建钉死)。剩余动作仅=你在离线机跑 ceremony([手册](../runbooks/2026-07-06-D1-签发机与key-ceremony.md) §2,半小时)。注:**公钥可公开、非秘密**(Part0 §10) |
| ✅ | 决策:开源默认是否由 KeyringLicenseStore 切 `FreeStub` | **已裁决:保留 KeyringLicenseStore**,不切 FreeStub、不删激活路径(2026-09-12 授权实现归一后,`FreeStub` 迁入 `channel_stubs`,独立 crate 删除) |
| ✅ | `exotic_commands` 激活 IPC(activate/get_token)随 KeyringLicenseStore 迁移一并处理 | 已消解:R1-1 早已收敛全部激活路径到 swap 点,swap 即全路径切换、IPC 层零改(swap 冒烟测试实证 direct) |
| 📦 | 私有仓给根 Cargo.toml / src-tauri Cargo.toml / mod.rs 加 `BEGIN/END-INTERNAL` 标记(§4) | **历史留痕**:标记块 2026-09-12 随授权实现归一移除(公开树与私有树同构,不再剥离源码) |

### B2. ③c Copybara 同步管线(已收官)

> 📦 本子节 2026-07-15 整节收官搬迁:末条「首发前人工核对公开树 vs 预期开源快照」由**用户 2026-07-15 完成核对**,全节 ✅ → 状态板与 ▸ 详注(B2-1…B2-5)均已迁 [completed.md](../completed.md) 同名子节。

### B3. ③c 开放问题(已收官,§10)

> 📦 本子节 2026-07-12 整节收官搬迁:四项开放问题全部拍板消解,状态板已迁 [completed.md](../completed.md) 同名子节。

## Part6 其他剩余

| 状态 | 待办 | 现状结论 |
|---|---|---|
| ⬜ | Part6-T11 AES 解密 / DerivedSecret | 计划明定**后置变现后**(首发 exotic-formats 无 ONNX 权重不需 AES) |
| ✅* | Part6 T3-T8 框架层 G1-G6(协议 / Coordinator 通用化 / 长驻 session / GpuLimiter / model blob / 低优先级) | T3-T7 已随 Part4 T10-T15 交付;T8 defer → **▸ 详注 C-1** |
| ⬜ | Part6 T12-T13 | T12 暂缓有据(分发面待 Part7/8);T13 多渠道字段预留 ✅ `5d1b1ff` → **▸ 详注 C-2** |
| ✅ | `tauri build` release 路径验证(target 迁根后) | 静态无 target 依赖 + `--no-bundle` 实证二进制落 workspace 根 → **▸ 详注 C-3** |
| ✅ | 完整安装器打包(WiX MSI / NSIS)验证 | 2026-07-05 随 T17 dry-run 实证 MSI+NSIS 双产出;⚠ 语义反转须核对 bundle 无 ONNX DLL → **▸ 详注 C-4** |
| ✅* | 公开树 Linux/macOS 构建适配 | Linux 半边 + gate 回迁 ✅(2026-07-05);macOS 半边 defer → **▸ 详注 C-5** |

> `✅*` = 部分完成 / 带条件,详情见对应 ▸ 详注。

> 📦 本节 ✅ 已完成项的 ▸ 详注(沿革 / 分叉裁决 / 病历 / 教训)已归档 → [completed.md](../completed.md)。

## Part5 剩余

> 📦 本节 2026-07-10 整节收官搬迁:状态板全文已迁 [completed.md](../completed.md) 同名节。
