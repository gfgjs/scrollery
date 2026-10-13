# Scrollery

[English](README.md) | 简体中文

Scrollery 是一款本地优先的桌面媒体资产管理器，用于整理、浏览和检索大型个人媒体库。

[发布页](https://github.com/gfgjs/scrollery/releases) · [开发指南](docs/development.md) · [架构说明](docs/architecture/README.md) · [问题反馈](https://github.com/gfgjs/scrollery/issues)

> 项目处于公开预览阶段，仍在开发中。功能和数据格式可能调整，请保留重要文件与媒体库的备份。

## 主要功能

- **建立媒体库**：添加本地文件夹，扫描文件信息并生成缩略图。
- **浏览与筛选**：在画廊中浏览文件，按时间或目录分组，按格式、评分和颜色标记筛选。
- **整理与归类**：为文件添加收藏、评分和颜色标记，用收藏夹归类。
- **搜索图片**：按文件名查找，或用文字描述画面，通过本地 AI 搜索相关图片。AI 搜索需要先下载模型并完成图片分析。

媒体文件和索引保存在本机，AI 搜图在本地运行。模型和插件下载需要联网；使用可选的远程文本校对时，待校对文字会发送到你配置的服务。

## 预览版

截至 2026-10-03，[GitHub Releases](https://github.com/gfgjs/scrollery/releases) 尚未发布安装包。目前可以从源码构建，开发与打包以 Windows x64 为主；macOS、iOS 和 Android 尚未提供可下载版本。

预览期间，官方预览版免费。官方稳定版计划一次性付费，价格、升级范围和过渡安排将在发布前公布。详细安排见[商业计划](COMMERCIAL.md)。

使用时还有这些限制：

- 图片高级编辑、OCR 和 PSD 引擎需在“功能与插件”中分别点击“免费激活”，并按提示准备插件或模型。激活不需要注册码或联网验证。
- 影像增强（降噪、去 JPEG 伪影、超分）所需模型尚未交付，暂不可用。交付要求见[模型说明](docs/enhance-model-delivery.md)。

## 开发

项目使用 Rust、Tauri 2、Vue 3、TypeScript 和 SQLite。

Windows x64 开发需要 Node.js 22、Rust stable（MSVC）、Microsoft C++ Build Tools、WebView2，以及 RAW 组件使用的 MinGW-w64 工具链。先按[开发指南](docs/development.md)准备环境和组件，再启动应用：

```powershell
npm run tauri dev
```

安装、构建和检查步骤见开发指南；模块职责与主要处理流程见[架构说明](docs/architecture/README.md)。

## 反馈与贡献

遇到问题或有功能建议，可以提交 [Issue](https://github.com/gfgjs/scrollery/issues)。报告问题时，请附上系统与应用版本、复现步骤和相关日志。

欢迎改进代码和文档。提交 Pull Request 前请阅读[贡献指南](CONTRIBUTING.md)；合入贡献前需要签署 [CLA](CLA.md)。

## 许可

第一方源码采用 [AGPL-3.0-only](LICENSE)，可按该许可使用、修改、分发和商用。源码的商业使用无需另购商业授权。Graphviz / Viz.js 组合适用[限定附加许可](ADDITIONAL-PERMISSION.md)，第三方组件许可见 [NOTICE](NOTICE.md)。

Scrollery 的名称、Logo 和图标受[商标规则](TRADEMARK.md)约束；非官方发行需使用自己的品牌。

Copyright 2026 The Scrollery Authors.
