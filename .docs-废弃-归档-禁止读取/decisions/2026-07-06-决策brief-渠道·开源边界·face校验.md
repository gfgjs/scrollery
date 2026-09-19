---
id: 2026-07-06-决策brief-渠道·开源边界·face校验
status: active
type: decision
line: 渠道与开源边界
created: 2026-07-06
---

# 决策 brief:渠道 §9.6 · 开源边界 §10.6 · Part4-T5 face 校验

> 2026-07-06 交付(FTO 窗口期准备包第三件)。目的:把 todo.md G3「上架/开源前决策簇」中
> 可拍板的拍掉、被阻塞的写死重评触发条件,解锁 Part8 D5-D8 与 CONTRIBUTING/CLA 配置。
> 结论先行:**真正需要你现在拍板的只有 3 件**(face 校验修法 / 开源追认 / CLA 装配时机),
> 渠道四项全部有据 defer,但其中一项的成本数字被订正(Steam $400 → 实为 $100)。

## 0. 摘要

| # | 决策 | 状态 | 推荐 |
|---|---|---|---|
| 1 | Steam 是否注册($100) | 🕐 defer 有据 | 触发条件=直销 30 天转化数据;成本比 Part0 原文低 4 倍且可返还,窗口期无损失 |
| 2 | MSIX worker 内置 vs 运行时下载 | 🕐 defer 有据 | 触发条件=Tauri WACK bug #14935 修复(外部) |
| 3 | 直销 vs Store IAP(15% vs 自缴 VAT) | 🕐 defer 有据 | 同上,与 #2 同触发 |
| 4 | Steam DRM 叠服务端 CheckAppOwnership | 🕐 defer 有据 | 归 Part8 D4 激活微服务立项时一并定 |
| 5 | psd-worker / AI 管线开源 | 🔴 **已被事实裁决** | 追认开源 + 补 CONTRIBUTING/Brand Terms 条款(见 §2) |
| 6 | 开源(宣传)时机 | ✋ 待拍板 | 改名施工完成后以 Scrollery 名义首发宣传 |
| 7 | CLA 装配 | ✋ 待拍板 | 现在装 CLA-assistant(个人 CLA);Entity CLA 待首个公司贡献者 |
| 8 | face 变体 sha256 校验取舍 | ✋ 待拍板 | 选项 B:size 快检 + 显式修复动作才跑全量 sha256 |

## 1. 渠道四项(Part0 §9.6 开放决策 ①-④)

**事实订正(2026-07-06 取证)**:Part0 §9.6-① 原文「Steam 是否值得 **$400** 注册」数字有误——
[Steamworks 官方文档](https://partner.steamgames.com/doc/gettingstarted/appfee)实证 **Steam Direct = $100/产品**,
且产品累计 Adjusted Gross Revenue 达 $1,000 后**返还**。Part0 正文已同步订正。
决策影响:注册决策的沉没成本从「值不值 $400」降为「$100 且大概率可返还」——它不再是钱的问题,
而是**上架维护成本**(商店页/评审/更新流)与**受众匹配度**的问题。

- **① Steam 注册**:维持 defer,但把重评触发条件写死=**直销上线 30 天转化数据**。加分项:若 H-Lab
  卷轴交互终局面向创作者社群,Steam 的第二曝光价值上调。无窗口期风险($100 随时可交,不涨价不排队)。
- **② MSIX worker 内置 vs 运行时下载**:被 Tauri WACK bug #14935 阻塞,修复前 MSIX 路径整体不动;
  修复后按「模型是否 >500MB」二选一(Part0 §9.4-1 既有框架)。无需现在拍板。
- **③ 直销 vs Store IAP**:同被 #2 阻塞(EXE 路径收款本就走官网 0%);WACK 修复后重评时的
  核心算式=「Store 代缴 VAT + 退款客服成本」vs「15% 抽成」。
- **④ Steam DRM 叠加**:`RestartAppIfNecessary` 可内存绕过属已知;是否给高价插件叠服务端
  `CheckAppOwnership` 依赖 D4 激活微服务的存在——归 **Part8 D4 立项时一并定**,单独拍无意义。

## 2. 开源边界(Part0 §10.6 开放决策 ①-④)

**🔴 先摆事实:①② 已被 ③c 投影事实裁决。** copy.bara.sky 的 origin_files 只排除
`crates/picasa-next-pro/**`、`plan-docs/**` 与同步配置——**psd-worker、psd-probe、ai-worker 与
src-tauri/src/ai/* 全部已随公开 main(`e663e22`)公开**,且已公开的代码事实上收不回(git 历史 + 任意 clone)。
这与 Part0 §10.6 原推荐方向一致(推荐开源,Whisper.cpp 模式:价值在加密权重 + License 链 + 集成,
不在管线代码),因此不是事故而是既定方针的自然落地,但程序上应**正式追认**并补齐配套条款:

- **追认动作(拍板后我做)**:CONTRIBUTING.md 明确「AI/人脸/exotic 付费插件为闭源商业产品,
  不接受相关 PR」;LICENSE 附 Additional Brand Terms(fork 须改名——改名施工后写 Scrollery);
  psd-worker 不再需要「仅限官方授权使用」特别条款(既已按 Apache-2.0 公开,附加使用限制反而制造
  协议自相矛盾,放弃 Part0 原 ① 的条款设想)。
- **③ 开源时机 → 实为「宣传时机」**:pro 分离(③b)与公开镜像(③c)均已完成,公开仓已存在、
  未宣传。推荐:**改名施工完成后以 Scrollery 名义首发宣传**——以工作代号积累 Stars 再改名,
  等于把最难迁移的社区注意力资产建在要拆的地基上;且首发前人工核对公开树(todo.md B2 尾巴)本就在案。
- **④ CLA**:推荐**现在**装 CLA-assistant(GitHub App,个人 CLA 用 Apache ICLA 模板,零运维);
  贡献者=0 的此刻装配零摩擦,等有 PR 再装会流失首批贡献者。Entity CLA 待首个公司贡献者出现再补,
  与 Part0「视社区规模」一致。

## 3. Part4-T5:face 变体 sha256 校验取舍(需拍板)

**背景**:`face_variant_installed`(face_commands.rs:38)当前**只按文件存在判定**,有据——官方下载
路径(`download_face_model`,同 ai_commands.rs:84 纪律)本就「size+sha256 校验通过才落盘」,
存在≈已校验。风险面(Part4 F5):用户**手动**把 LFS pointer 文本/截断文件放进模型目录 →
`installed=true` 但推理崩溃,错误信息离根因很远。

| 选项 | 内容 | 代价 | 判语 |
|---|---|---|---|
| A 维持现状 | 存在=已装 | 自伤场景排查难 | 有据但留 F5 敞口 |
| **B size 快检(推荐)** | 判定时比对清单 size(已有字段);全量 sha256 仅在显式「校验/修复」动作跑 | ≈0(一次 stat) | LFS pointer(数百字节 vs 数十 MB)与截断文件当场识破,挡住绝大部分自伤面,零启动税 |
| C sha256 全对等 | 每次判定全量 hash | 数十~数百 MB 模型的启动性能税 | 「防篡改”在本威胁模型无意义(本地用户即所有者),为 1% 场景付 100% 税 |

**推荐 B**,工作量约半天含测试(size 不匹配 → `installed=false` + tracing 一条含期望/实际 size 的
warn,用户可读懂「文件坏了,重下」)。

## 4. 拍板记录(2026-07-06 当日全部拍板并执行)

| # | 决策 | 拍板 | 日期 |
|---|---|---|---|
| 5 | 开源追认 + 条款补齐 | ✅ 追认;CONTRIBUTING.md(付费插件不接 PR)+ TRADEMARK.md(品牌名占位,改名后填)+ CLA.md 三件落仓,随 ③c 投影公开 | 2026-07-06 |
| 6 | 宣传时机 = 改名后 | ✅ 按推荐:改名施工完成后以新名首发宣传 | 2026-07-06 |
| 7 | CLA-assistant 现在装 | ✅ CLA.md 已备;GitHub App 安装+指向 CLA.md 属用户侧动作(公开仓 Settings → Integrations → cla-assistant) | 2026-07-06 |
| 8 | face 校验选项 | ✅ 选 B:size 快检已实施(face_commands.rs `face_variant_installed` 改收 &FaceProfile,清单 size 比对,4 单测) | 2026-07-06 |
