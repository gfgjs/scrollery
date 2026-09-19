---
status: 施工中
type: 工作记忆
line: 许可证迁移至 MPL-2.0
created: 2026-08-25
---

# 发现与决策：许可证迁移修复

## P0 复核结论

- 项目维护者已直接确认：当前第一方代码全部由其本人创作，没有其他参与者；因此不存在需要逐一取得历史贡献者重许可的对象。
- `CLA.md` 继续只约束未来外部贡献；第三方依赖、vendored 源码和闭源 `scrollery-pro` 不因该确认而获得 MPL 授权。
- P0 已关闭；不新增虚构的历史签署材料，保留现有决策 brief 中的权属依据说明。

## P1 施工进度

- 已把根 workspace、独立 raw-worker、实际发货的 `onnxruntime-node`、LibRaw、vendored foliate-js 和按需 FFmpeg 纳入 NOTICE/SBOM 的同一闭包；strong-copyleft 现在会明确标记 2 项（LibRaw dual CDDL/LGPL、FFmpeg LGPL-shared），不再报假绿。
- 已新增 `prepare-legal-resources.mjs`：Tauri 构建生成 `target/legal/`，包含 ORT/FFmpeg 官方法律材料、LibRaw 三份文本、foliate-js LICENSE、7 份固定 SPDX 正文和 888 份实际闭包包文件；MSI/NSIS verifier 均检查完整法律资源树的名称/字节尺寸。
- 26 个实际闭包包的包根目录不含 LICENSE/COPYING/NOTICE 文件；其 SPDX、版本、registry/resolved 来源已写入 `target/legal/MANIFEST.json`，并已映射到仓库内固定 hash 的 canonical SPDX 文本，构建期 unresolved 为 0。
- 维护者在会话恢复后重新确认 P1 分发策略：LibRaw 选择 CDDL-1.0；FFmpeg 接受精确钉定的 BtbN LGPL-shared 独立进程运行时。NOTICE 仍保留上游表达式和 strong-copyleft 旗标，作为每次发行前复核门；法律材料抓取已从构建路径移除。
- 当前未发现 P1 实现文件丢失或损坏：法律材料 manifest/hash、prepare、NOTICE、Tauri resources、release verifier 和现有 MSI/NSIS 闭包均可由当前工作树重建/验证。P2 与 Private-Pro 法律边界仍未处理。

## 耐久提升候选

| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| P0-001 | 建立历史第一方代码权利来源与重许可授权台账，并绑定发布范围。 | 法务/发布流程或仓库内可审计权利材料 |
| P0-002 | 依据权利证据选择统一 MPL、保留原许可、双许可或暂停发布的边界策略。 | `LICENSE`、源码范围说明和发布门禁 |
