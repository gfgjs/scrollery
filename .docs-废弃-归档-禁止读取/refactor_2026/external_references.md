---
id: 2026-06-27-external_references
status: snapshot
type: review
line: refactor_2026
created: 2026-06-27
---

# 外部参考来源包（法律 / 平台政策 / 上游 issue）

> terminal review 补（2026-06-27）。Part0/4/6/7/8 散落的法律/平台/上游结论汇总于此，含来源、访问日期、级别、复核状态。
> 🔴 **法律/平台政策会变**：所有「需法务/平台复核」项**在公开发行 / 上架前必须重新核验并更新本表**（注明新访问日期）。本表是「撰写期快照」，非长期有效结论。

| # | 结论 | 来源 | 访问日期 | 级别 | 复核状态 |
|---|---|---|---|---|---|
| L1 | `PICASA` = Google LLC 活跃注册商标（USPTO Reg. 2952412，Class 9，Registered and Renewed）；停服 ≠ 放弃；平台投诉即下架 | USPTO TESS | 撰写期（2026-06，**待重核**） | 🔴 阻断上架 | **须法务 FTO 意见书**（US/EU/CN，约 $1500-4000/名），改名 + 清权后方可上架（Part0 §10.7） |
| L2 | MS Store Policy **10.2.2**：**非「禁所有下载执行代码」，实为禁「以违反政策 / 不符已声明功能的方式动态包含代码改变产品行为」**（v7.19 原文，发布 2025-09-10）；如实声明 + 签名校验 + 不偏离已声明功能即不触。**10.8.1：非游戏 PC 应用可用安全第三方支付 API**（无需 MS 分成，须 company account + 10.8.2 合规） | Microsoft Store Policies（learn.microsoft.com/windows/apps/publish/store-policies） | **2026-06-27 已联网核实** | 🟡 口径收窄（P1-9） | worker 内置仍为保守低风险方案（**项目策略，非政策唯一允许**）；metadata 须诚实声明动态加载 + Ed25519 信任链；第三方支付路径成立（Part7 §3.6.2 / Part8 §3.5.4 / §7.3） |
| L3 | Tauri MSIX WACK bug **#14935**（未修复）→ 近期走 Store EXE 路径绕开，MSIX 待上游修复后评估 | GitHub tauri-apps/tauri #14935 | 撰写期（**待重核**） | 🟡 渠道时序 | 评估 MSIX 前查 issue 状态（Part0 §9.4 / Part8 §3.5.2） |
| L4 | 中国大陆人脸功能涉 **PIPL** +《**人脸识别技术应用安全管理办法**》（CAC+公安部，**2025-06-01 施行**；全名含「应用安全」，旧 plan 误作「人脸识别技术管理办法」）。7 项产品义务：显著告知 / 单独同意 / 便捷撤回 / 最短保存 / 事前 PIA（记录存 3 年）/ 未成年人监护人同意 / 存储≥10 万人脸 30 工作日内省级网信备案；且人脸非唯一验证方式 | 全国人大 / 国家网信办 cac.gov.cn | **2026-06-27 已联网核实** | 🔴 区域发行阻断 | 🔑 **义务随产品形态触发**（Part4 §3.10「形态×义务」矩阵，P1-11）：纯本地自用（数据不出端）触发面有限、须落「单独同意 + 撤回即清库 + 最短保存级联删 face 派生」；上架分发 / 云端服务才触发备案 / PIA。**仍须中国法律评估**（Part0 §10.5 / Part8 RK9） |
| L5 | SCRFD(`det_10g.onnx`)+ArcFace R50(`w600k_r50.onnx`) = InsightFace **非商用研究专用**。README 原文：「The code of InsightFace is released under the MIT License...no limitation for both academic and commercial usage」**但**「The training data...and the models trained with these data...for non-commercial research purposes only」→ 权重商用须邮件单独授权（**非自动可得**） | InsightFace README/LICENSE（github.com/deepinsight/insightface） | **2026-06-27 已联网核实** | 🔴 法律红线 | `#[cfg(feature="face-noncommercial")]` 隔离 + CI 断言不打包（Part4 T1 / Part8 D15，**未落地**）；pro/付费轨默认**不**启用 SCRFD 轨（P1-12） |
| L6 | `gficcg/clip_cn_vit-onnx`（HF）**模型卡无 license 字段**；上游 OFA-Sys/Chinese-CLIP 仓库 `MIT-LICENSE.txt` = 标准 MIT（含 use/copy/modify/**sell**）→ **代码与权重商用依据 = 仓库 MIT**（非 HF 卡） | HF / GitHub OFA-Sys/Chinese-CLIP `MIT-LICENSE.txt` | **2026-06-27 已联网核实** | 🟠 低-中风险 | 从官方 MIT 源 `export_clip_l14_336_onnx.py` 自导出 + 自托管；审计依据写「**仓库 MIT-LICENSE.txt**」非 HF 卡，避免「权重无许可」质疑（Part0 §10 / Part4 §3.10.3，P1-12） |
| L7 | 默认人脸轨 YuNet(MIT) + SFace(Apache) / opencv_zoo —— 商用合规 | opencv_zoo 仓库 LICENSE | 撰写期 | 🟢 合规 | 维持默认轨；许可须发行前亲验（Part0 §10） |
| L8 | 依赖树 **669 Rust**（Cargo.lock `[[package]]` 实测；cargo metadata 报告 653，口径差——排除自身/dev-only/未启用 target）+ 199 npm，98% 宽松许可，无 GPL/AGPL 编译进 Win/mac 目标；MPL-2.0×5 静态链接合法走 NOTICE | cargo-license / 第五轮审计 / 第 8 轮核验 | 撰写期（数字 2026-06-28 实测订正） | 🟢 合规 | 🔴 **NOTICE/SBOM 当前仓库不存在**（git ls-files 无 tracked NOTICE/SBOM），属 Part0 §10 配置待办、**非既成事实**；发行前须真跑生成；内部 crate 补 `license="MIT OR Apache-2.0"`（Part0 §10） |
| L9 | **Steam 定价平价**：成文 Steamworks 规则仅约束 **Steam key 平价**（不得在别处更便宜卖 Steam key）；**「官网/他店不得更便宜」非成文公开规则**——系 Valve 私下施压（Wolfire v. Valve 反垄断指控的 PMFN「平台最惠国」；2018 Giardino 邮件威胁下架「whether or not using Steam keys」；summary judgment 2026 初被驳、待陪审团审 2026–27） | Steamworks Pricing / Wolfire v. Valve | **2026-06-27 已联网核实** | 🟡 商业策略（P1-10） | plan 凡称「Steam 不许官网更便宜」须改注「key 平价=成文；更广平价=施压/诉讼中、未定论」，按实际 Steam Distribution Agreement + 法务确认，**不写成公开政策事实**；Steam 渠道后置不阻塞任何开工项（Part8 §3.6.2） |

## 来源 URL 与审计字段

> 🔴 以下 URL 为撰写期（2026-06）依知识填写。**L2 / L4 / L5 / L6 / L9 已于 2026-06-27 经本环境联网核实**（上表「结论」与「复核状态」已更新为精确官方口径，第 6 轮独立核验）；**L1 / L3 / L7 / L8 仍待实访**。法律/平台政策会变 → 即便已核项，发行前仍须重访、记录**精确访问日期 + 结论摘录 + 复核人 + 快照证据**。

| # | 官方来源 URL（待实访确认） | 复核人 | 精确访问日期 | 快照 | 状态 |
|---|---|---|---|---|---|
| L1 | `tmsearch.uspto.gov`（检索 Reg. No. 2952412） | — | 待 | 待 | ☐ 待实访 + 法务 FTO |
| L2 | `learn.microsoft.com/.../store-policies`（§10.2.2） | — | 待 | 待 | ☐ 待实访 |
| L3 | `github.com/tauri-apps/tauri/issues/14935` | — | 待 | 待 | ☐ 待实访核状态 |
| L4 | `npc.gov.cn`（PIPL）+ 国家网信办《人脸识别技术应用安全管理办法》（2025-06-01 施行） | — | 待 | 待 | ☐ 待法务评估 |
| L5 | `github.com/deepinsight/insightface`（LICENSE 非商用） | — | 待 | 待 | ☐ 发行前最终 license 复核 |
| L6 | `github.com/OFA-Sys/Chinese-CLIP`(MIT) / HF `gficcg/clip_cn_vit-onnx`(无 license) | — | 待 | 待 | ☐ 待自托管 + license 确认 |
| L7 | `github.com/opencv/opencv_zoo`（YuNet MIT / SFace Apache） | — | 待 | 待 | ☐ 待实访 |
| L8 | 仓库内 `cargo-license` 输出（自生成） | — | 待 | 待 | ☐ 待重跑 |

## 复核流程（公开/上架前必做）

1. 逐行重新访问来源 URL，更新「访问日期」+ 确认结论未变。
2. L1/L4 取法务书面意见（FTO / PIPL 合规评估）。
3. L2/L3 核平台最新政策版本与上游 issue 状态。
4. L5/L6 核模型许可现状，确认隔离/自托管已落地（CI 断言通过）。
5. 在各引用处（Part0/4/6/7/8）保持与本表一致；本表为单一来源（single source of truth）。
