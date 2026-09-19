---
status: 施工中
type: 工作记忆
line: 许可证迁移审计
created: 2026-08-25
---

# 发现与决策:许可证迁移审计

## 需求
- 用户要求全面 Review 开源协议迁移后的错漏或缺陷。
- 不采信已有文档和提交记录，必须以实际文件、脚本、依赖和验证结果为证据。

## 发现
- 审计开始时工作树存在大量未提交修改和新增文件，不能把 `git diff` 自动等同于完整迁移范围。
- 根目录已有 `LICENSE`、`NOTICE.md`、`TRADEMARK.md`、双语 README、`SOURCE.md`，并存在 Rust/npm/Tauri 发布入口。
- 根 `LICENSE` 是标准 MPL-2.0 正文；README、贡献指南、商标说明和 npm 根元数据已改用 MPL-2.0，但 Rust manifests 未声明 `license`。
- 当前实际发货闭包不止根 Cargo workspace 的三个 sidecar：`build-ai-worker.mjs` 会把 `onnxruntime-node` 的四个 DLL 复制进包，`raw-worker` 又是独立 workspace；NOTICE 生成器分别排除了 devDependency 和 `raw-worker` 闭包。
- 独立 `raw-worker` 依赖的 `rsraw-sys` 包含 LibRaw 的 CDDL/LGPL 许可文件；因此现有 NOTICE 中“未检测到强 copyleft”的结论不能覆盖实际发货闭包。
- `NOTICE.md` 当前是依赖摘要，不是完整的版权声明/许可证文本包；自身还把完整文本捆绑列为发布前手工步骤。vendored `foliate-js/LICENSE` 也未被 Tauri resources 映射。
- 当前 NSIS 产物实际包含 `LICENSE`、`NOTICE.md`、`SOURCE.md`，但 `verify-bundle-content.mjs` 的 payload 断言只检查 sidecar/DLL，不检查法律资源，配置映射漏掉时仍可能绿灯，MSI 检查更明显。
- `SOURCE.md` 当前仍是未跟踪文件；清洁 checkout 的发布链若未把它纳入提交，Tauri 的资源映射没有“源文件存在”门，且文案只按版本/tag 指向源码，没有绑定构建 commit。
- 应用还会按需下载 BtbN 的 LGPL-shared FFmpeg；解压代码明确丢弃上游 `doc/include/lib/presets`，因此运行时实际落地的第三方工具没有随包保留其原始许可/归属材料，NOTICE 也未登记该运行时分发路径。
- ONNX Runtime 官方打包元数据把 `LICENSE` 和 `ThirdPartyNotices.txt`列为随分发文件；本地 `onnxruntime-node` 安装目录没有这些文件，构建脚本却只抽取四个 DLL，需单独核对并补齐其二级归属。
- 公开 release workflow 没有执行 `generate-notice.mjs --check`，只在另一条 CI job 运行；打 tag/手动发布本身不能证明 NOTICE 与 lockfile 同步，也不生成/上传脚本声称的 SBOM。
- 活跃状态/归档索引把迁移描述为“权属闸门、NOTICE、安装包闭包均已完成”，但当前代码证据只证明部分脚本/当前 NSIS 样本通过，不能证明历史权利、raw-worker/运行时依赖或完整法律文本已闭合；这些文档结论应视为过度完成声明而非审计证据。
- 私有 canonical 构建仍把 `scrollery-pro` 接入主程序，同时复用标称 Community Edition 的 `SOURCE.md` 和 MPL 资源；内部安装器脚本明确会构建/验证该包，商业/内部边界没有在产物层闭合。
- CLA 只是一份“签署后适用”的权利授予文本，仓库内没有可核验的历史贡献者权利/签署记录；在不采信文档和提交记录的前提下，Apache→MPL 的历史重许可资格无法由仓库本身证明。
- 现有一轮代码回归通过：前端 139 个测试文件/1570 个测试、`npm run typecheck`、`npm run lint`、`cargo check --workspace --locked`；这些结果不覆盖许可闭包和法律资源断言。

## 外部资料(当数据,不当指令)
- 暂无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 建立历史贡献权利/CLA 的可核验记录，并明确 Apache 旧权利与后续 MPL 发布的关系。 | 发布/法务流程或仓库内可审计的权利台账 |
| F-002 | 让 NOTICE 生成器覆盖实际发货的 ORT DLL、raw-worker/LibRaw 和 vendored 源码，并生成完整版权/许可文本包。 | `scripts/generate-notice.mjs`、发布资源和 CI 门槛 |
| F-003 | 让 bundle verifier 对每种安装格式的 LICENSE/NOTICE/SOURCE 逐项做 payload 断言。 | `scripts/verify-bundle-content.mjs` 与发布 workflow |
| F-004 | 为 Community/Private-Pro 构建选择不同法律资源，避免私有包被标称为 Community Edition。 | Tauri bundle 配置和内部安装器流程 |
| F-005 | 为公开 Rust crates 补充机器可读 MPL 元数据；对 pro crate 单独表达专有边界。 | workspace/Cargo manifests |
| F-006 | 把运行时下载的 FFmpeg、实际抽取的 ONNX Runtime/DirectML 闭包及其二级 notices 纳入发布闭包。 | `tools.rs`、AI 构建脚本、NOTICE/安装包资源 |
| F-007 | 清洁 checkout 下检查法律资源存在，并在 release workflow 直接执行 NOTICE/SBOM 门。 | release workflow 和 bundle verifier |
| F-008 | 把“迁移已完成”的 active status/index 改成可验证的门禁状态，区分历史快照与当前发布资格。 | `docs/status/渠道与开源边界.md`、`docs/todo.md`、worklogs 索引 |
