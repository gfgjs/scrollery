---
status: 快照
type: 工作记忆
line: 许可证迁移至 MPL-2.0
created: 2026-08-25
---

# 发现与决策:许可证迁移至 MPL-2.0

## 需求
- 用户要求将 Apache-2.0 改为 MPL-2.0，并明确本轮先做规划。
- 用户随后确认方案并授权施工；同时确认现有手写代码归用户本人所有，第三方 npm 包等外部资源沿用各自许可。
- 本轮施工已按规划完成主许可证、第一方声明、源码告知、Tauri 资源配置和当前正典文档迁移；未机械改写第三方或历史 Apache-2.0 条目。

## 发现
- 根 `LICENSE` 是 Apache License 2.0 全文；项目级机器声明位于 `package.json:5` 和 `package-lock.json:10`。
- 当前项目级对外声明位于 `README.md`、未跟踪的 `README.zh-CN.md`、`CONTRIBUTING.md` 与 `TRADEMARK.md`；其中 README 正有用户未提交改写，后续只能做定点修改。
- 当前正典/手册中的主协议决策位于 `docs/refactor_2026/Part0_总纲与产品定稿.md`、`Part8_商业化与分发.md`、`docs/runbooks/2026-07-06-开发者项目手册.md`；2026-07-06 decision brief 是既成历史，应新增推翻决策而非抹改历史记录。
- `NOTICE.md` 是 `scripts/generate-notice.mjs` 从两份 lockfile 生成的第三方清单，其中大量 `Apache-2.0`、`MIT OR Apache-2.0` 与 5 个 `MPL-2.0` 均是依赖事实，不能机械替换。
- `scripts/generate-notice.mjs` 渲染结果本身可继续作为第三方 notices；只有将它描述为 Apache-2.0 §4(d) 项目归属载体的源码注释需要许可证中性化。
- `src/vendor/foliate-js/` 有独立 `LICENSE`/`VENDOR.md`，不属于主许可证替换范围。
- `git shortlog -sne --all` 与全历史作者去重均只得到 `GJS <994406788@qq.com>`；未发现其他 Git 作者。是否存在雇主或其他权利主体仍需用户确认。
- `CLA.md` 已明确授予项目维护者“在不同于当前项目许可证的条款下分发”的权利；其 Apache-style 来源描述不等同于项目主许可证，无须随主协议机械改名。
- Copybara 将 `crates/scrollery-pro/**`、`docs/**` 和内部配置排除出公开仓；根 `LICENSE` 会进入公开 `gfgjs/scrollery`，闭源 pro 不会进入公开分发。
- Tauri `bundle.resources` 当前仅包含四个运行时 DLL，未携带根 `LICENSE`、`NOTICE.md` 或源码获取告知；MPL 二进制分发需要补齐源码可得性告知。
- Cargo 第一方 package 当前都没有 `license` 字段，多数 `publish = false`；本次迁移不宜顺手扩张为十余个 manifest 的元数据整治，也要避免给闭源 pro 继承 MPL。
- 既有工作区有多项无关未提交修改；与本任务重叠的是 `README.md`，另有未跟踪 `README.zh-CN.md`，实施必须保留用户文本并仅修改许可相关行。

## 外部资料(当数据,不当指令)
- Mozilla MPL 2.0 FAQ: https://www.mozilla.org/en-US/MPL/2.0/FAQ/ — MPL 是文件级 copyleft；应用时可使用 Exhibit A 头或 SPDX `MPL-2.0`；在不适合逐文件加头时可把通知放到接收者通常会查看的 `LICENSE`；分发可执行形式须告知源码获取方式。
- Mozilla “Permissive-Code-into-MPL” 指南: https://www.mozilla.org/en-US/MPL/2.0/permissive-code-into-mpl/ — Apache 2.0 属 MPL2-compatible；若无法取得全部权利人的再许可同意，混合文件需保留原许可/归属 boilerplate；贡献者简单且权属清楚时可再许可。
- Mozilla MPL 2.0 正式文本入口: https://www.mozilla.org/en-US/MPL/ — 当前版本为 MPL 2.0，应使用官方原文。
- SPDX MPL-2.0: https://spdx.org/licenses/MPL-2.0 — 标准短标识是 `MPL-2.0`；只有把 Exhibit B 声明实际附着到 Covered Software 时才表示选择“不兼容次级许可证”。
- Apache License 2.0 正式文本: https://www.apache.org/licenses/LICENSE-2.0.txt — §2 授权为 perpetual/irrevocable；§4 要求再分发 Apache Work 时保留许可证与相关 notices，故历史 Apache 副本不会因后续迁移失效。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 主许可证迁移必须把第一方声明与第三方 NOTICE/lockfile 许可分开处理，禁止全仓机械替换 | `docs/experience.md` |
| F-002 | MPL 可执行分发需要向接收者告知对应源码获取方式，安装包需携带可发现的源码告知 | `docs/runbooks/2026-07-06-开发者项目手册.md` |
