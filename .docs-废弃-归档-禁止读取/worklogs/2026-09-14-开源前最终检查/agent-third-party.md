---
id: 2026-09-14-开源前最终检查-第三方专项
status: snapshot
type: review
line: 渠道与开源边界
created: 2026-09-14
---

# 第三方/模型与对外付费依赖核查（任务C，只读）

基准：2026-09-14 当前工作树（HEAD `a5ba0bb7`，未提交改动仅 docs/前端样式）。结论均以当前代码复核，未采信 9/12 报告转述。

## 1. 总判

所查源码未见运营方固定付费 API 依赖：外部依赖为开源组件、公开模型权重、按需下载的运行时，以及用户自配端点的远程校对（实际费用依所选端点，不由运营方固定承担）。本结论限于源码可见范围，不含官方服务、运营系统与未公开计划。**9/12 的 L-01..05 当前无一补齐**：五条全部仍成立，且均可由「法律材料清单 × 实际发货闭包」的覆盖缺口证明。

根因是结构性的：NOTICE 生成面按 lockfile 依赖逐包列举，而实际发货的四件原生 DLL 需从 onnxruntime-node 包内二次抽取、16 个模型资产按需下载不入包（仓内 `.onnx` 命中 0）；「包 → 抽出件/下载权重」的版本、来源与归属链在仓内没有记录。

## 2. 对外服务依赖（源码可见部分的付费性质）

| 项目 | 证据 | 结论 |
|---|---|---|
| ONNX Runtime 1.26.0 + DirectML/dxcompiler/dxil | `package.json:60`、`package-lock.json:5105`、`scripts/build-ai-worker.mjs:30`、`:40` | 随 onnxruntime-node 分发，无计费调用；许可条款待主会话对官方源复核 |
| 模型权重（CLIP/OCR/人脸） | `crates/scrollery-ai-core/src/profile.rs:108`、`src-tauri/src/ai/ocr_registry.rs:37`、`crates/scrollery-ai-core/src/face_profile.rs:121` | 公开权重，按需下载，不入二进制；无调用计费 |
| 远程校对 | `src-tauri/src/proofread/mod.rs:20`、`:33`、`src-tauri/src/ipc/proofread_commands.rs:18` | 用户自配 OpenAI 兼容端点 + 自带 key（存 keyring）；是否付费与实际费率依所选端点，不由运营方固定承担 |
| Steam / MS Store 渠道 | `src-tauri/Cargo.toml:287`–`:291`（`channel-msstore`/`channel-steam` 为空 feature，仅 stub） | 未接入任何平台 SDK，无平台商业条款暴露 |
| 前端依赖 | `package.json:31`–`50` | 无遥测/支付/分析 SDK |

源码公开阻断见第 4 节：L-02 内嵌的 Graphviz 编译产物随 `public/` 进入公开树，构成源码仓公开阻断；其余为二进制发行面缺口。

## 3. L-01..05 当前状态（逐条复核）

| 编号 | 9/12 结论 | 当前证据 | 状态 |
|---|---|---|---|
| L-01 | DirectML/dxcompiler/dxil 三件缺来源与归属映射 | `third-party/license-sources.json` 全文只含 onnxruntime(2)/ffmpeg(3)/spdx(8)=13 条，**无** directml/dxcompiler/dxil 条目；`third-party/licenses/` 下无对应目录（仅 onnxruntime/ffmpeg/spdx）；`generate-notice.mjs` 无相关命中；四件 DLL 由 `node_modules/onnxruntime-node/bin/napi-v6/win32/x64/` 抽取（ORT 本体已作为依赖注册，缺的是抽取件的版本/来源/归属链） | **未补齐** |
| L-02 | Graphviz 2.40.1 EPL-1.0 与 Lute 归属缺失 | `public/vditor/dist/js/graphviz/full.render.js:4` 仍为 Viz.js 2.1.2（Graphviz 2.40.1）；`third-party/licenses/` 无 EPL-1.0 文本（spdx 集合仅 Apache/BSD2/BSD3/BSL/MIT/MPL/Unlicense）；`license-sources.json` 与 `NOTICE.md` 对 graphviz/lute 零命中。该产物在 `public/` 下，公开源码仓即已分发，故**源码公开面同样阻断**，不止二进制面 | **未补齐** |
| L-03 | SCRFD/ArcFace 非商用权重不得入商业发行 | 双重隔离已生效：`commercial_ok=false`（`face_profile.rs:213`）+ `#[cfg(feature = "face-noncommercial")]` 物理不编译（`:34`、`:191`，含 id/file 字面量）；`assets: Vec::new()`（`:216`）无下载直链；默认轨 YuNet/SFace 为 MIT/Apache-2.0（`:160`、`:159`） | 隔离成立，**当前默认分发无此阻断**（该轨不在默认/商业构建内且无下载直链）；仅当启用并分发该轨时才需商业/再分发授权文书 |
| L-04 | 权重版本/hash/模型卡未固化 | CLIP 托管权重已钉（`profile.rs:285`–`:299`），但 `profile.rs:133` 的 vocab.txt（OFA-Sys 来源）= `sha256: None` + size 0；OCR 七个资产全钉 sha256+size（`ocr_registry.rs:37`–`:83`）；enhance 仍 `PENDING_REPO_BASE` 占位、零校验且被 `listable()` 挡在下载之外（`src-tauri/src/enhance/registry.rs:54`、`:38`）。模型 LICENSE/NOTICE 随包**绝对无落点**（NOTICE 内模型字样零命中，仅 `NOTICE.md:669` 提 onnxruntime-node） | **未补齐** |
| L-05 | 不改私仓，补源码可获得性核对 | **已有处理**：FFmpeg 版本/来源/校验链钉在源码内——`src-tauri/src/exotic/tools.rs:35`–`:46`（`BTBN_RELEASE_TAG`、固定 tag 的 ZIP URL、size、zip sha256、ffmpeg/ffprobe exe sha256）与 `third-party/license-sources.json:4`–`:5`（`ffmpegSourceVersion: 2aefd64d48`、`btbnBuildRevision: 8c736b2`）；应用内展示对应 release tag 链接（`src/views/PluginStoreView.vue:258`，`:255` 为 LGPL 声明文案）；LICENSE.md/COPYING.LGPLv2.1/BtbN-LICENSE 三份文本 + sha256 齐备，`scripts/prepare-legal-resources.mjs:68`、`:75`–`:80` 校验 | **未补齐项收窄**：已有处理，未验证的是实际发行包字节与 LGPL 对应源码闭包的逐项对照、LibRaw 源码获得说明。不断言不合规 |

## 4. 公开阻断 / 发行阻断

**源码公开阻断（1 项）：** L-02 的 Graphviz 2.40.1 编译产物位于 `public/vditor/dist/js/graphviz/`，推送公开源码仓即已发生分发，其 EPL-1.0 文本与对应源码获得说明的缺失在源码仓公开时即成立，不等到出安装包。无组件因商业许可必须屏蔽源码。

**二进制发行阻断（3 项）：**

1. L-01 四件 DLL 无逐件来源/license/notices 映射（`license-sources.json` 缺条 + legal 目录缺件）。
2. L-02 Graphviz EPL-1.0 文本与源码获得说明缺失（EPL 在分发目标码时要求提供源码获得方式），Lute 版本仍不可从产物确认（`lute.min.js` 头部无版本串）。
3. L-05 FFmpeg/LibRaw：源码内已钉版本/URL/size/hash，应用内已展示 release tag 链接；**未验证**的是发行包实际字节与 LGPL 对应源码可获得性的对照（属发行验收项，非来源缺失）。

**模型分发阻断（1 项）：** L-04 的权重适用声明。L-03 仅在启用非商用轨分发时成为阻断，当前默认轨不触发。

## 5. 当前来源/license/hash 覆盖与缺口

覆盖完整：ONNX Runtime（LICENSE + ThirdPartyNotices，hash 已固定）；FFmpeg 三份文本（hash 已固定）；SPDX 正文 8 份；LibRaw 文本由 `scripts/prepare-legal-resources.mjs:236` 起复制；托管 CLIP 权重与 OCR 七件（sha256 + size 本机实测）。

缺口：DirectML/dxcompiler/dxil（来源、license、hash 全缺）；Graphviz 2.40.1 EPL-1.0（文本 + 源码说明）；Lute（版本 + MulanPSL-2.0）；`profile.rs:133` vocab.txt（hash/size 空）；enhance 全部占位；全部模型权重的随包 LICENSE。

经核：ORT 自带 ThirdPartyNotices **已含** LLVM/NCSA 与 MIT/Expat 文本（`third-party/licenses/onnxruntime/ThirdPartyNotices.txt:1433`、`:4336`），故 DirectML 的 LLVM 派生面不可按「文本缺失」上报；缺的是 Microsoft 侧发布日期、版本与包级映射。该三件的适用许可本次无网络未复核，待主会话对官方发布包确认。

## 6. 验证命令与结果

- `CARGO_NET_OFFLINE=true node scripts/generate-notice.mjs --check` → exit 0，输出「✓ NOTICE.md 新鲜(rust 460 + npm 164 + vendored 1 + runtime 2;strong-copyleft 旗标 2)」。**该 check 只证明 NOTICE 与其采用面一致**；采用面原文自陈「MANIFEST.json 中的包级文件/SPDX 正文映射仍需按上游来源逐项复核」，故不覆盖上节任一缺口。
- `rg --files . -g '*.onnx'`（排除 node_modules/target）→ 0 命中，与「权重按需下载、不入包」一致。
- `git log --since=2026-09-12 -- third-party scripts/prepare-legal-resources.mjs scripts/generate-notice.mjs NOTICE.md SOURCE.md scripts/build-ai-worker.mjs` → 仅 `a5ba0bb7`（NOTICE 再生 + bench 进程名），**无任何针对 L-01..05 的补漏提交**。

## 7. 未验证项与需主会话决策（官方源许可复核归主会话）

- 未确认 `lute.min.js` 的具体版本（产物内无版本串）；未取 Vditor 3.11.3 发行物比对。
- 未验证实际发行包 FFmpeg/LibRaw 字节与 LGPL 对应源码闭包的逐项对照（`tools.rs` 常量与应用内链接已存在，属发行验收项）。
- DirectML/dxcompiler/dxil 的**实际 DLL 版本**未从二进制读取（未做二进制审计），本报告只证「源码内无来源/归属记录」。
- 本次**无网络，未做任何官方源许可复核**；报告内出现的许可名（ONNX Runtime、DirectML/DirectXShaderCompiler、Graphviz 2.40.1、Lute、FFmpeg/LibRaw 等）均来自 9/12 记录或产物字符串线索，不作为本轮结论。
- `node scripts/verify-channel-bundle.mjs --no-dist` 由主会话执行并通过，本报告未重复；安装包内容验证未执行。
- 未验证 FFmpeg 实际构建配置与所选 BtbN 变体的 LGPL/GPL 边界。
