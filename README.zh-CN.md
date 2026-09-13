# Scrollery

[English](README.md) | 简体中文

**一款面向大型个人媒体库的本地优先、高性能资产管理器。**

Scrollery 是一款桌面优先的媒体整理、浏览与检索应用。围绕流畅画廊浏览、后台处理和可预测的性能设计，目标是在媒体库从数千项增长到数十万项后仍保持良好体验。

> [!IMPORTANT]
> Scrollery 当前处于 **Public Preview（公开预览）**阶段，仍在活跃开发中。预览版主要用于测试，可能包含未完成功能、兼容性调整或破坏性数据迁移。请为重要媒体库保留备份。

## 公开预览与商业化计划

我们希望从一开始就把预览版到正式版的转变说明清楚：

- Public Preview 期间，官方预览构建免费提供。
- 官方稳定版计划采用**一次性付费购买**。具体价格、升级覆盖范围和过渡方案将在正式发布前公布。
- 应用源码（桌面前端、Rust 主机、随附 Worker 与授权实现）按照 [GNU Affero General Public License 第 3 版（仅限第 3 版）](LICENSE)（AGPL-3.0-only）提供，用户可以自行构建，也可以用于商业用途。
- 随附 Graphviz 2.40.1 / Viz.js 2.1.2 的组合适用[限定附加许可](ADDITIONAL-PERMISSION.md)；第三方许可与对应源码提供要求仍须遵守。
- 计划中的[商业产品](COMMERCIAL.md)与 AGPL 发布相互独立：官方稳定版以终端用户商业许可一次性买断提供，另有针对 AGPL 不适配的企业／OEM 用法的协商授权。使用 AGPL 源码从事普通商业活动无需两者中的任何一个。
- 未来独立商业组件计划另定许可，**不预先承诺开源**。
- 付费产品计划提供官方签名安装包、托管更新、部分专业组件和官方支持；自行构建得到的是仓库中的源码，不包含购买权益与上述官方服务。
- 如果未来推出在线或托管服务，可能采用独立的收费方式。计划中的 NAS 服务端与 Web 组件尚未开发交付；未来若某版本支持远程网络交互且适用 AGPL 第 13 条，其运营方须向远端交互的用户提供正在运行的实际修改版本的对应源码。

上述计划可能根据预览期反馈进行调整，但我们不会把正式版收费作为面向早期用户的突然变化。

## Scrollery 关注什么

- **本地优先的数据所有权**：媒体文件、文件夹组织结构和索引由用户掌控。
- **大型媒体库性能**：扫描、布局、元数据富化和派生资源以后台流水线运行，避免占用 UI 线程。
- **快速视觉浏览**：两端对齐画廊、按视口取数和虚拟化共同保证浏览响应速度。
- **搜索与发现**：通过元数据工作流和本地 AI 语义搜索，突破文件名与目录结构的限制。
- **可扩展的媒体处理**：重型或可选能力放在独立 Worker 和插件化格式管线中，避免拖累界面进程。
- **跨平台基础**：Rust、Tauri v2、Vue 3 与 TypeScript 组成共享应用核心，当前预览阶段优先完善桌面交付。

当前预览版已经包含图片媒体库核心流程、画廊浏览、元数据富化、缩略图生成和 AI 语义搜索。更广泛的媒体支持、正式打包、更新分发和专业组件仍在持续完善。

## 架构亮点

- **两阶段扫描**：快速首轮扫描先生成可浏览画廊，随后在后台补充元数据和关联关系。
- **后端两端对齐布局**：Rust 负责计算并缓存画廊几何信息，前端只请求可见行。
- **按视口补充数据**：大型媒体库不会常驻所有厚重元数据，仅在需要显示时加载。
- **分桶虚拟化**：将超大逻辑画廊映射到浏览器可承受的有限滚动区域。
- **隔离的 AI 推理**：语义搜索推理运行在独立 Worker 中，避免重型运行时依赖进入宿主进程。
- **本地 SQLite 存储**：桌面数据库使用 `rusqlite`、WAL 模式以及独立的读写协调机制。

## 获取预览版

支持平台有可用构建时，官方预览安装包会通过 [GitHub Releases](../../releases) 发布。

只有 Scrollery 项目发布的安装包属于官方构建。源码许可证允许 fork 和第三方构建，但这些版本不具备 Scrollery 官方签名、支持或背书。

## 从源码构建

### 前置环境

- Node.js
- Rust 工具链
- [Tauri v2 前置依赖](https://tauri.app/start/prerequisites/)
- Windows：Microsoft C++ Build Tools 和 WebView2 Runtime

克隆仓库后运行：

```bash
npm install
npm run tauri dev
```

生成本地发布包：

```bash
npm run build
npm run tauri build
```

常用检查：

```bash
npm run lint
npm run typecheck
npm test
cargo check --manifest-path src-tauri/Cargo.toml --tests
```

从本仓库自行生成的构建属于社区/自构建发行，不包含项目的发布签名密钥、代码签名证书、购买权益或官方更新服务。

## 仓库结构

```text
src/
  components/   Vue 界面与媒体视图
  composables/  画廊、选择、查看器与请求编排
  stores/       应用状态

src-tauri/src/
  db/           SQLite 表结构、迁移与查询
  scanner/      文件系统扫描与元数据富化
  layout/       两端对齐布局与常驻缓存
  thumbnail/    缩略图与派生资源流水线
  engine/       媒体解码引擎
  ai/           语义搜索编排与缓存
  ipc/          Tauri 命令边界
```

## 参与贡献

欢迎提交缺陷报告、聚焦的功能提案、文档改进和代码贡献。发起 Pull Request 前请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)；代码贡献需要签署项目 [CLA](CLA.md)。

安全敏感问题不应在公开 Issue 中披露；当仓库提供安全报告流程时，请按照对应说明提交。

## 许可证与商标

Copyright 2026 The Scrollery Authors.

本仓库中的第一方源码均采用 [GNU Affero General Public License 第 3 版（仅限第 3 版）](LICENSE)（AGPL-3.0-only）授权。商业使用在 AGPL 下即被允许，无需另购授权；计划中的[商业产品](COMMERCIAL.md)——付费的官方稳定版与协商的企业／OEM 授权——与 AGPL 发布相互独立。未来独立商业组件计划另定许可，不预先承诺开源。第三方组件保持自身许可，声明见 [NOTICE.md](NOTICE.md)；此前以 Mozilla Public License 2.0 发布过的版本仍按当时的许可条款提供。

源码许可证**不授予**在 fork 或衍生产品中使用 Scrollery 名称、Logo 或图标的权利。非官方发行必须使用自己的品牌，且不得暗示获得 Scrollery 项目背书或属于官方版本。详见 [TRADEMARK.md](TRADEMARK.md)。
