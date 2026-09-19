---
status: 快照
type: 工作记忆
line: 许可证迁移至 MPL-2.0
created: 2026-08-25
---

# 进度日志:许可证迁移至 MPL-2.0

## 会话:2026-08-25
- 做了:读取 planning 技能与 docs 治理规则；完成许可证文件、项目声明、依赖 NOTICE、贡献权属、CLA、Copybara、Tauri bundle 与当前规范文档的只读盘点；核对 Mozilla、Apache、SPDX 官方资料；形成六阶段实施方案。
- 验证:`git status --short --branch` 确认工作区已有无关改动；`rg` 分类项目级与第三方 `Apache-2.0` 引用；`git shortlog -sne --all` 与作者去重均只见一个 Git 作者；读取 `copy.bara.sky` 证明闭源 pro 和 docs 不进公开镜像；读取 `src-tauri/tauri.conf.json` 证明当前安装包未声明法律/源码告知资源；三件套存在性/frontmatter 结构检查和 `git diff --check` 通过。
- 遗留:等待用户确认规划与权属闸门后，从阶段 2 开始实施；实施时先保护 README 在途改写，再做许可证、元数据、分发告知和规范文档批次。

## 会话:2026-08-25 · 施工
- 做了:用户确认现有手写代码权属；根 `LICENSE` 替换为 Mozilla 官方原文；更新 npm 元数据、README 中英文、贡献/商标政策、NOTICE 生成器注释、当前正典与开发者手册；新增许可证决策 brief、`SOURCE.md` 和渠道状态；Tauri bundle 接线 `LICENSE`、`NOTICE.md`、`SOURCE.md`；扩展 bundle 配置自检覆盖三项法律资源。
- 验证:在线逐字比对根 `LICENSE` 与 Mozilla 发布文本，结果完全一致；所有修改使用 `apply_patch`；未触碰第三方 NOTICE 条目、Cargo package license 元数据或用户其他在途改动。
- 验证:通过 `npm install --package-lock-only --ignore-scripts --no-audit --no-fund`、NOTICE 新鲜度门、npm lint/typecheck/test/build、Cargo metadata/check、Tauri 配置校验和文档门禁；NSIS 安装包 `target/release/bundle/nsis/Scrollery_0.1.0_x64-setup.exe` 成功生成，bundle 载荷闭包自检通过。
- 验证:剩余项目级 `Apache-2.0` 命中已分类为第三方依赖、vendored 资源、Cargo/模型依赖说明、历史文档或本次迁移记录；未发现需要替换的当前第一方主许可证声明。
- 遗留:无产品变更遗留；NSIS 压缩包内部文件名未用本机 7z 再列举，但 Tauri `bundle.resources` 配置解析、release 打包和载荷闭包门禁均通过。

## 回顾(收口时填)
- 亮点:先锁定公开 Community Edition 范围，再分层处理第一方声明、第三方 notices 和历史记录；新增 `SOURCE.md` 并接入 Tauri 资源，使源码告知随安装包交付。
- 教训:许可证搜索必须保留上下文分类；lockfile、NOTICE、vendor 与历史决策中的 Apache-2.0 不能因为主许可证迁移而被批量替换。
- 意外:本机旧调试进程锁住 debug 可执行文件，导致 `--no-bundle --debug` 收尾失败；release NSIS 路径无需打断该进程即可完成同等构建验证。
