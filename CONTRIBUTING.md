# Contributing

Thanks for your interest in contributing! This document sets expectations so your time is well spent.

## Scope: what is open, what is not

Every first-party source file in this repository is licensed under **AGPL-3.0-only** (GNU Affero General Public License, version 3 only): the desktop frontend, the Rust host, the bundled workers, and the licensing implementation (the direct-sale keyring entitlement store and Ed25519 token verification). **Pull requests against any of it are welcome** — new features, fixes, refactors, and documentation alike.

A few things live outside this repository and outside the source license: the release signing keys and private issuance credentials, plus the official signed installers, managed updates, and official support that the paid offerings are planned to provide. Future independent commercial components are planned to be licensed separately, with **no open-source commitment made for them in advance**. Building from source gives you the application as it stands in the tree; it does not grant purchase entitlements or the official services.

Commercial use is permitted by the source license and needs no separate agreement. The planned paid offerings are separate from it: a one-time purchase of the official stable release under an end-user commercial license, and a negotiated license for enterprise / OEM use where the AGPL's obligations do not fit. See [COMMERCIAL.md](COMMERCIAL.md).

## Contributor License Agreement (CLA)

The project also grants the [limited Graphviz additional permission](ADDITIONAL-PERMISSION.md). Maintainers must verify that contributions used in that combination can be distributed with this permission, through the contributor's grant or the effective CLA relicensing rights.

Before we can merge your first pull request, you must agree to the project CLA (see [CLA.md](CLA.md)). Signing runs through the project's CLA tooling, and a maintainer confirms that you have agreed to the current version before merging. The CLA was revised on 2026-09-14, so a signature recorded against an earlier version, or a stale green badge, does not by itself satisfy that check. The CLA is project-specific, adapted from the Apache individual CLA: **you keep your copyright**, and you grant the project maintainer a copyright and patent license to your contribution, **including the right to relicense it** — which is what allows the project to offer commercial licenses covering code that includes your contribution. Recipients of the project's distributions receive their rights under the project's distribution license (currently AGPL-3.0-only with the limited Graphviz additional permission); the CLA does not change what they get.

## Trademarks

The project name and logo are trademarks and are **not** licensed under AGPL-3.0-only. See [TRADEMARK.md](TRADEMARK.md). Forks must use a different name and logo.

## Practical notes

- Rust: `rustfmt` + `clippy -D warnings` must pass. Frontend: ESLint + Prettier + `vue-tsc` strict.
- All SQL goes through parameter binding; no string concatenation.
- Core logic changes need unit tests; CI runs the full suite on every PR.
- Comments in the codebase are predominantly in Chinese (project convention); either language is fine in PRs.
