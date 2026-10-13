# 开发指南

本文按 2026-10-04 的 Windows x64 构建配置整理。其他平台的组件打包尚未完整接入。

## 环境

- Node.js 22 和 npm，与当前发布工作流一致。
- Rust stable，主程序使用 `x86_64-pc-windows-msvc` 工具链。
- Microsoft C++ Build Tools，安装“使用 C++ 的桌面开发”工作负载和 Windows SDK。
- WebView2 Runtime。
- MinGW-w64，确保终端能找到 `g++` 和 `dlltool`；RAW 组件需要这套工具链。

Windows 基础环境的安装步骤见 [Tauri 前置依赖](https://tauri.app/start/prerequisites/#windows)。

## 首次启动

克隆仓库后，在仓库根目录运行：

```powershell
npm ci
rustup target add x86_64-pc-windows-gnu
npm run tauri dev
```

`tauri dev` 会准备第三方许可材料，编译 AI、视频、影像增强、RAW 和图片处理组件，再启动前端与桌面程序。AI 组件在每次开发启动时增量编译为 debug，并同步到 Tauri 的暂存目录，同时准备 ONNX Runtime 动态库。首次执行需要下载依赖并编译这些组件。

之后通常只需运行 `npm run tauri dev`。仅运行 `npm run dev` 会启动前端开发服务器，不会启动桌面后端。

开发进程运行期间修改 AI worker 或其共享推理、协议代码后，重新运行 `npm run tauri dev`，使宿主与 worker 一起更新。单独更新开发版 AI 组件可运行 `npm run build:ai-worker:dev`；该命令同时更新编译产物和暂存副本。

## 生成安装包

在上述环境准备完成后运行：

```powershell
npm run tauri build
```

构建流程会自动构建前端、编译所需组件并收集运行库和许可材料。安装包输出到 `target/release/bundle/`。

AI 模型和可下载插件仍需另行准备。影像增强模型的交付条件见[模型说明](enhance-model-delivery.md)。

## 检查改动

按改动范围选择检查。前端命令在仓库根目录运行，例如：

```powershell
npm exec -- eslint src/stores/searchStore.ts
npm run typecheck
npm test -- src/stores/directoryOperations.spec.ts
npm run check:tests
```

Rust 改动检查相关 crate 的格式、Clippy 和测试；前端格式修复使用 `npm run lint:fix -- <本次文件路径>`。项目约定见[贡献指南](#contributing)。

## 代码入口

- `src/`：Vue 界面、状态管理与前端交互。
- `src-tauri/src/`：桌面后端、扫描、数据库、图库布局与媒体处理。
- `crates/`：共享库及独立处理组件。
- `scripts/`：构建、检查与发布工具。

模块职责和主要流程见[架构说明](architecture/README.md)。

## Contributing

Thanks for your interest in contributing! This document sets expectations so your time is well spent.

### Scope: what is open, what is not

Every first-party source file in this repository is licensed under **AGPL-3.0-only** (GNU Affero General Public License, version 3 only): the desktop frontend, the Rust host, the bundled workers, and the licensing implementation (the direct-sale keyring entitlement store and Ed25519 token verification). **Pull requests against any of it are welcome** — new features, fixes, refactors, and documentation alike.

A few things live outside this repository and outside the source license: the release signing keys and private issuance credentials, plus the official signed installers, managed updates, and official support that the paid offerings are planned to provide. Future independent commercial components are planned to be licensed separately, with **no open-source commitment made for them in advance**. Building from source gives you the application as it stands in the tree; it does not grant purchase entitlements or the official services.

Commercial use is permitted by the source license and needs no separate agreement. The planned paid offerings are separate from it: a one-time purchase of the official stable release under an end-user commercial license, and a negotiated license for enterprise / OEM use where the AGPL's obligations do not fit. See [COMMERCIAL.md](legal/COMMERCIAL.md).

### Contributor License Agreement (CLA)

The project also grants the [limited Graphviz additional permission](legal/ADDITIONAL-PERMISSION.md). Maintainers must verify that contributions used in that combination can be distributed with this permission, through the contributor's grant or the effective CLA relicensing rights.

Before we can merge your first pull request, you must agree to the project CLA (see [CLA.md](legal/CLA.md)). Signing runs through the project's CLA tooling, and a maintainer confirms that you have agreed to the current version before merging. The CLA was revised on 2026-09-14, so a signature recorded against an earlier version, or a stale green badge, does not by itself satisfy that check. The CLA is project-specific, adapted from the Apache individual CLA: **you keep your copyright**, and you grant the project maintainer a copyright and patent license to your contribution, **including the right to relicense it** — which is what allows the project to offer commercial licenses covering code that includes your contribution. Recipients of the project's distributions receive their rights under the project's distribution license (currently AGPL-3.0-only with the limited Graphviz additional permission); the CLA does not change what they get.

### Trademarks

The project name and logo are trademarks and are **not** licensed under AGPL-3.0-only. See [TRADEMARK.md](legal/TRADEMARK.md). Forks must use a different name and logo.

### Practical notes

- Rust: `rustfmt` + `clippy -D warnings` must pass. Frontend: ESLint + Prettier + `vue-tsc` strict.
- All SQL goes through parameter binding; no string concatenation.
- Core logic changes need unit tests; CI runs the full suite on every PR.
- Comments in the codebase are predominantly in Chinese (project convention); either language is fine in PRs.
