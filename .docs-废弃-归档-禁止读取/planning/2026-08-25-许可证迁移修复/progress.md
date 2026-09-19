---
status: 施工中
type: 工作记忆
line: 许可证迁移至 MPL-2.0
created: 2026-08-25
---

# 进度日志：许可证迁移修复

## 会话：2026-08-25

- 已读取当前 `CLA.md`、`CONTRIBUTING.md` 和工作树状态。
- 已确认 P0 的关键事实仍不是仓库内代码可自行证明的内容：CLA 有未来贡献的重许可条款，但没有历史贡献者签署/权利链记录。
- 用户已确认当前第一方代码全部由其本人创作，没有其它参与者；P0 关闭，不需要补造权利材料。
- 现有决策 brief 已记录该权属边界；下一步进入 P1，修复实际发货闭包、完整法律文本和发布门禁。
- P1 发货闭包已完成：NOTICE 扫描 457 Rust + 128 npm + 1 vendored + 2 runtime 组件；`prepare:legal` 生成 ORT/FFmpeg/LibRaw/foliate-js、7 份 SPDX 正文和 888 份包级 license 文件；Tauri MSI/NSIS 实际包分别 926/930 条目，载荷与完整法律资源 verifier 全绿。
- P1 法律材料已完成：一次抓取并纳入 `third-party/licenses/` 的 ORT 1.26.0、FFmpeg 实际构建 commit `2aefd64d48`/BtbN revision `8c736b2` 和 SPDX v3.27.0 正文；构建只校验本地 hash、不联网；26 个没有包根文件的条目均通过 `legal/MANIFEST.json` 映射到 7 份固定 SPDX 正文，当前 unresolved 为 0。
- P1 当前暂停点：NOTICE 仍明确标记 LibRaw/FFmpeg 的 LGPL 相关强 copyleft 旗标，需人工确认分发渠道政策与源码/替换性要求。

## 会话：2026-08-25 · 会话恢复复核

- 做了：按当前工作树重新核对 P1 文件、法律材料、Tauri 配置、CI/release 门禁和已生成安装包；维护者重新确认 LibRaw CDDL-1.0 与 FFmpeg 独立 LGPL-shared 分发策略。
- 验证：`npm run prepare:legal` 通过（888 个包级文件、7 个固定 SPDX、unresolved 0）；`node scripts/generate-notice.mjs --check` 通过；`node scripts/verify-bundle-content.mjs` 通过（MSI 926 / NSIS 930）；`npm run vendor:licenses` 通过（fetched 0 / reused 12）；当前 `npm test` 通过（140 files / 1579 tests），typecheck、lint、JSON/JS 语法检查和 `git diff --check` 通过。
- 结论：未发现 P1 代码或法律材料丢失/损坏；已将接受的策略补回决策 brief 与工作记忆。P1 完成，下一阶段为 P2；P1 相关文件仍未提交，不能把当前工作树视为已持久化交付。

## 回顾（收口时填）

- 亮点：待任务完成后填写。
- 教训：待任务完成后填写。
- 意外：待任务完成后填写。
