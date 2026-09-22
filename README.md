# Scrollery

English | [简体中文](README.zh-CN.md)

**A local-first, high-performance media asset manager built for large personal libraries.**

Scrollery is a desktop-first application for organizing, browsing, and finding media without sending the library to a remote service. Its architecture is designed around responsive gallery browsing, background processing, and predictable performance as a library grows from thousands to hundreds of thousands of items.

> [!IMPORTANT]
> Scrollery is currently in **Public Preview** and under active development. Preview builds are intended for testing and may contain incomplete features, compatibility changes, or breaking data migrations. Keep backups of important libraries.

## Public Preview and commercial plan

We want the transition from preview to a stable release to be clear from the beginning:

- Official preview builds are free during the Public Preview period.
- The official stable distribution is planned as a **paid, one-time purchase**. Pricing, upgrade coverage, and transition details will be announced before the stable release.
- The application source code — desktop frontend, Rust host, bundled workers, and the licensing implementation — is published under the [GNU Affero General Public License, version 3 only](LICENSE) (AGPL-3.0-only), including the right to build it independently and to use it commercially.
- A [limited additional permission](ADDITIONAL-PERMISSION.md) permits the bundled Graphviz 2.40.1 / Viz.js 2.1.2 combination. Third-party licenses and the source-provision requirements remain applicable.
- The planned [commercial offerings](COMMERCIAL.md) are separate from the AGPL release: a one-time purchase of the official stable release under an end-user commercial license, and a negotiated license for enterprise / OEM use that the AGPL does not fit. Ordinary commercial use of the AGPL-licensed source requires neither.
- Future independent commercial components are planned to be licensed separately; **no open-source commitment is made for them in advance**.
- Paid offerings are planned to provide the official signed installers, managed updates, selected professional components, and official support. Building from source gives you the code in the tree; it does not include purchase entitlements or the official services.
- Online or hosted services, if introduced in the future, may be offered separately. The planned NAS server and web components have not been built yet; if a future version supports remote network interaction and AGPL section 13 applies to it, its operator must offer those users the Corresponding Source of the modified version running.

The plan may be refined in response to preview feedback, but a paid stable distribution should not come as a surprise to early users.

## What Scrollery focuses on

- **Local-first ownership** — media and indexes remain under the user's control.
- **Large-library performance** — scanning, layout, metadata enrichment, and derived assets are designed as background pipelines rather than UI-thread work.
- **Fast visual browsing** — justified gallery layout, viewport-driven data loading, and virtualization keep navigation responsive.
- **Search and discovery** — metadata workflows and local AI semantic search help users find media beyond filenames and folders.
- **Extensible media handling** — isolated workers and a plugin-oriented format pipeline keep heavy or optional capabilities outside the UI process.
- **Cross-platform foundations** — Rust, Tauri v2, Vue 3, and TypeScript provide a shared application core, with the current preview focused on desktop delivery.

The current preview already includes the core image-library workflow, gallery browsing, metadata enrichment, thumbnail generation, and AI semantic search. Broader media support, release packaging, update delivery, and professional components are still evolving.

Image enhancement (denoising, JPEG artifact removal, and upscaling) is **not yet available in this preview**. Windows builds include `enhance-worker` and use the bundled ONNX Runtime libraries, but the five model profiles still lack approved ONNX assets, distribution notices, and pinned download metadata. Downloads and processing remain disabled until those prerequisites are supplied. Successful enhancement and before/after preview in an installed release have not been verified. See the [model delivery requirements](docs/enhance-model-delivery.md).

On 2026-09-22, an isolated Windows release build passed MSI/NSIS payload checks and installed-NSIS checks for worker startup, ONNX Runtime initialization, rejection of missing model files, and unavailable-component UI/IPC gates. Test source images remained byte-for-byte unchanged on these rejected paths; this does not verify successful processing.

## Architecture highlights

- **Two-phase scanning:** a fast initial pass populates the gallery, followed by background metadata and relationship enrichment.
- **Backend justified layout:** Rust computes and caches gallery geometry; the frontend requests only the visible rows.
- **Viewport hydration:** heavy metadata is loaded on demand instead of being retained for every item in a large library.
- **Bucket virtualization:** large logical galleries are mapped to bounded browser scroll regions.
- **Isolated AI inference:** semantic-search inference runs in a separate worker process to keep heavy runtime dependencies away from the host process.
- **Local SQLite storage:** the desktop database uses `rusqlite`, WAL mode, and separate read/write coordination.

## Getting the preview

Official preview packages are published through [GitHub Releases](../../releases) when a build is available for a supported platform.

Only packages published by the Scrollery project should be treated as official builds. Forks and third-party builds are permitted by the source license, but they are not signed, supported, or endorsed by the Scrollery project.

## Building from source

### Prerequisites

- Node.js
- Rust toolchain
- [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/)
- On Windows: Microsoft C++ Build Tools and the WebView2 runtime

After cloning the repository:

```bash
npm install
npm run tauri dev
```

Create a local release bundle:

```bash
npm run build
npm run tauri build
```

Focused checks:

```bash
npm run lint
npm run typecheck
npm test
cargo check --manifest-path src-tauri/Cargo.toml --tests
```

Builds produced from this repository are community/self-built distributions. They do not include the project's release signing keys, code-signing certificates, purchase entitlements, or the official update service.

## Official edition activation

Features & Plugins manages one perpetual official-edition license for advanced image editing, OCR, enhancement, and the PSD engine. RAW decoding and video format extensions remain free. Licensing, plugin installation, and model preparation are shown separately. Uninstalling a plugin preserves the license; removing the local license preserves files, models, and plugins.

Purchasing is not yet available. Production storefront, public keys, and release delivery remain pending. Enhancement also requires release model assets and is not currently usable just by activating a license. This change has focused tests and browser-harness UI checks; end-to-end usage in a new installer has not been verified.

## Repository layout

```text
src/
  components/   Vue interface and media views
  composables/  gallery, selection, viewer, and request orchestration
  stores/       application state

src-tauri/src/
  db/           SQLite schema, migrations, and queries
  scanner/      filesystem scanning and metadata enrichment
  layout/       justified layout and resident caches
  thumbnail/    thumbnail and derived-asset pipelines
  engine/       media decoding engines
  ai/           semantic-search orchestration and caches
  ipc/          Tauri command boundary
```

## Contributing

Bug reports, focused proposals, documentation improvements, and code contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Contributions require the project [CLA](CLA.md).

Security-sensitive issues should not be disclosed in a public issue; follow the repository's security-reporting instructions when available.

## License and trademarks

Copyright 2026 The Scrollery Authors.

All first-party source code in this repository is licensed under the [GNU Affero General Public License, version 3 only](LICENSE) (AGPL-3.0-only). Commercial use is permitted under the AGPL and needs no separate license; the planned [commercial offerings](COMMERCIAL.md) — the paid official stable release and negotiated enterprise / OEM licensing — are separate from the AGPL release. Future independent commercial components are planned to be licensed separately, without an open-source commitment in advance. Third-party components keep their own licenses, listed in [NOTICE.md](NOTICE.md), and releases previously published under the Mozilla Public License 2.0 remain available under the terms they were released with.

The source license does **not** grant permission to use the Scrollery name, logo, or icons for a fork or derived product. Unofficial distributions must use their own branding and must not imply endorsement or official status. See [TRADEMARK.md](TRADEMARK.md).
