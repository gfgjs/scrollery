# Third-Party Notices

Scrollery
Copyright 2026 The Scrollery Authors

Scrollery incorporates third-party open-source software.
This file lists the packages distributed with the application (desktop app,
bundled workers, and the compiled frontend) together with their declared licenses.

本文件由 `scripts/generate-notice.mjs` 从 `Cargo.lock` / `package-lock.json` 生成,
**请勿手改**;依赖变更后重新生成(CI/发布流程以 `--check` 校验新鲜度)。
完整 license 文本由 `scripts/prepare-legal-resources.mjs` 生成到安装包 `legal/`;
包根未携带文本的条目使用仓库内固定 hash 的 SPDX 正文,映射记录在 `legal/MANIFEST.json`;
构建阶段不联网,只校验并复制 `third-party/licenses/` 中的已固定材料。

Application version at generation time: 0.1.0

## License summary

| License (as declared) | Packages |
| --- | ---: |
| MIT | 249 |
| MIT OR Apache-2.0 | 216 |
| Apache-2.0 OR MIT | 33 |
| MIT/Apache-2.0 | 27 |
| Unicode-3.0 | 18 |
| Apache-2.0 | 16 |
| ISC | 6 |
| Unlicense OR MIT | 6 |
| BSD-3-Clause | 5 |
| MPL-2.0 | 5 |
| Zlib OR Apache-2.0 OR MIT | 4 |
| BSD-2-Clause | 3 |
| BSD-3-Clause OR Apache-2.0 | 3 |
| BSL-1.0 | 3 |
| MIT OR Apache-2.0 OR Zlib | 3 |
| Unlicense/MIT | 3 |
| Apache-2.0 OR ISC OR MIT | 2 |
| Apache-2.0 OR MIT OR Zlib | 2 |
| BSD-2-Clause OR Apache-2.0 OR MIT | 2 |
| CC0-1.0 OR MIT-0 OR Apache-2.0 | 2 |
| (Apache-2.0 OR MIT) AND BSD-3-Clause | 1 |
| (MIT OR Apache-2.0) AND Unicode-3.0 | 1 |
| 0BSD OR MIT OR Apache-2.0 | 1 |
| Apache-2.0 / MIT | 1 |
| Apache-2.0 AND ISC | 1 |
| Apache-2.0 AND MIT | 1 |
| Apache-2.0 OR BSL-1.0 | 1 |
| Apache-2.0/MIT | 1 |
| BSD-3-Clause AND MIT | 1 |
| BSD-3-Clause/MIT | 1 |
| CC0-1.0 | 1 |
| CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 1 |
| CDDL-1.0 OR LGPL-2.1-only | 1 |
| CDLA-Permissive-2.0 | 1 |
| EPL-1.0 | 1 |
| LGPL-2.1-or-later | 1 |
| MIT OR Zlib OR Apache-2.0 | 1 |
| MulanPSL-2.0 | 1 |
| OFL-1.1 | 1 |
| Unlicense | 1 |
| Zlib | 1 |

## Rust crates (460) — desktop application and workers

Dependency closure (normal deps) of the shipped binaries `scrollery`,
`ai-worker` and `psd-worker`, resolved for `x86_64-pc-windows-msvc`.

| Crate | Version | License |
| --- | --- | --- |
| [adler2](https://crates.io/crates/adler2) | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [ahash](https://crates.io/crates/ahash) | 0.8.12 | MIT OR Apache-2.0 |
| [aho-corasick](https://crates.io/crates/aho-corasick) | 1.1.4 | Unlicense OR MIT |
| [alloc-no-stdlib](https://crates.io/crates/alloc-no-stdlib) | 2.0.4 | BSD-3-Clause |
| [alloc-stdlib](https://crates.io/crates/alloc-stdlib) | 0.2.4 | BSD-3-Clause |
| [anyhow](https://crates.io/crates/anyhow) | 1.0.103 | MIT OR Apache-2.0 |
| [approx](https://crates.io/crates/approx) | 0.5.1 | Apache-2.0 |
| [arboard](https://crates.io/crates/arboard) | 3.6.1 | MIT OR Apache-2.0 |
| [arrayvec](https://crates.io/crates/arrayvec) | 0.7.8 | MIT OR Apache-2.0 |
| [atomic-waker](https://crates.io/crates/atomic-waker) | 1.1.2 | Apache-2.0 OR MIT |
| [base64](https://crates.io/crates/base64) | 0.13.1 | MIT/Apache-2.0 |
| [base64](https://crates.io/crates/base64) | 0.22.1 | MIT OR Apache-2.0 |
| [bit-set](https://crates.io/crates/bit-set) | 0.8.0 | Apache-2.0 OR MIT |
| [bit-vec](https://crates.io/crates/bit-vec) | 0.8.0 | Apache-2.0 OR MIT |
| [bitflags](https://crates.io/crates/bitflags) | 1.3.2 | MIT/Apache-2.0 |
| [bitflags](https://crates.io/crates/bitflags) | 2.13.0 | MIT OR Apache-2.0 |
| [blake3](https://crates.io/crates/blake3) | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| [block-buffer](https://crates.io/crates/block-buffer) | 0.10.4 | MIT OR Apache-2.0 |
| [brotli](https://crates.io/crates/brotli) | 8.0.4 | BSD-3-Clause AND MIT |
| [brotli-decompressor](https://crates.io/crates/brotli-decompressor) | 5.0.3 | BSD-3-Clause/MIT |
| [bumpalo](https://crates.io/crates/bumpalo) | 3.20.3 | MIT OR Apache-2.0 |
| [bytecheck](https://crates.io/crates/bytecheck) | 0.8.2 | MIT |
| [bytecheck_derive](https://crates.io/crates/bytecheck_derive) | 0.8.2 | MIT |
| [bytemuck](https://crates.io/crates/bytemuck) | 1.25.0 | Zlib OR Apache-2.0 OR MIT |
| [bytemuck](https://crates.io/crates/bytemuck) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [byteorder](https://crates.io/crates/byteorder) | 1.5.0 | Unlicense OR MIT |
| [byteorder-lite](https://crates.io/crates/byteorder-lite) | 0.1.0 | Unlicense OR MIT |
| [bytes](https://crates.io/crates/bytes) | 1.12.0 | MIT |
| [camino](https://crates.io/crates/camino) | 1.2.4 | MIT OR Apache-2.0 |
| [cargo-platform](https://crates.io/crates/cargo-platform) | 0.1.9 | MIT OR Apache-2.0 |
| [cargo_metadata](https://crates.io/crates/cargo_metadata) | 0.19.2 | MIT |
| [castaway](https://crates.io/crates/castaway) | 0.2.4 | MIT |
| [cfb](https://crates.io/crates/cfb) | 0.7.3 | MIT |
| [cfg-if](https://crates.io/crates/cfg-if) | 1.0.4 | MIT OR Apache-2.0 |
| [chacha20](https://crates.io/crates/chacha20) | 0.10.1 | MIT OR Apache-2.0 |
| [chardetng](https://crates.io/crates/chardetng) | 0.1.17 | Apache-2.0 OR MIT |
| [chrono](https://crates.io/crates/chrono) | 0.4.45 | MIT OR Apache-2.0 |
| [clipboard-win](https://crates.io/crates/clipboard-win) | 5.4.1 | BSL-1.0 |
| [color_quant](https://crates.io/crates/color_quant) | 1.1.0 | MIT |
| [compact_str](https://crates.io/crates/compact_str) | 0.9.1 | MIT |
| [constant_time_eq](https://crates.io/crates/constant_time_eq) | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| [cookie](https://crates.io/crates/cookie) | 0.18.1 | MIT OR Apache-2.0 |
| [cookie_store](https://crates.io/crates/cookie_store) | 0.22.1 | MIT OR Apache-2.0 |
| [cpufeatures](https://crates.io/crates/cpufeatures) | 0.2.17 | MIT OR Apache-2.0 |
| [cpufeatures](https://crates.io/crates/cpufeatures) | 0.3.0 | MIT OR Apache-2.0 |
| [crc32fast](https://crates.io/crates/crc32fast) | 1.5.0 | MIT OR Apache-2.0 |
| [crossbeam-channel](https://crates.io/crates/crossbeam-channel) | 0.5.15 | MIT OR Apache-2.0 |
| [crossbeam-deque](https://crates.io/crates/crossbeam-deque) | 0.8.6 | MIT OR Apache-2.0 |
| [crossbeam-epoch](https://crates.io/crates/crossbeam-epoch) | 0.9.18 | MIT OR Apache-2.0 |
| [crossbeam-utils](https://crates.io/crates/crossbeam-utils) | 0.8.21 | MIT OR Apache-2.0 |
| [crypto-common](https://crates.io/crates/crypto-common) | 0.1.7 | MIT OR Apache-2.0 |
| [cssparser](https://crates.io/crates/cssparser) | 0.36.0 | MPL-2.0 |
| [cssparser-macros](https://crates.io/crates/cssparser-macros) | 0.6.1 | MPL-2.0 |
| [ctor](https://crates.io/crates/ctor) | 0.8.0 | Apache-2.0 OR MIT |
| [ctor-proc-macro](https://crates.io/crates/ctor-proc-macro) | 0.0.7 | Apache-2.0 OR MIT |
| [darling](https://crates.io/crates/darling) | 0.20.11 | MIT |
| [darling](https://crates.io/crates/darling) | 0.23.0 | MIT |
| [darling_core](https://crates.io/crates/darling_core) | 0.20.11 | MIT |
| [darling_core](https://crates.io/crates/darling_core) | 0.23.0 | MIT |
| [darling_macro](https://crates.io/crates/darling_macro) | 0.20.11 | MIT |
| [darling_macro](https://crates.io/crates/darling_macro) | 0.23.0 | MIT |
| [dary_heap](https://crates.io/crates/dary_heap) | 0.3.9 | MIT OR Apache-2.0 |
| [data-encoding](https://crates.io/crates/data-encoding) | 2.11.0 | MIT |
| [deranged](https://crates.io/crates/deranged) | 0.5.8 | MIT OR Apache-2.0 |
| [derive_builder](https://crates.io/crates/derive_builder) | 0.20.2 | MIT OR Apache-2.0 |
| [derive_builder_core](https://crates.io/crates/derive_builder_core) | 0.20.2 | MIT OR Apache-2.0 |
| [derive_builder_macro](https://crates.io/crates/derive_builder_macro) | 0.20.2 | MIT OR Apache-2.0 |
| [derive_more](https://crates.io/crates/derive_more) | 2.1.1 | MIT |
| [derive_more-impl](https://crates.io/crates/derive_more-impl) | 2.1.1 | MIT |
| [digest](https://crates.io/crates/digest) | 0.10.7 | MIT OR Apache-2.0 |
| [dirs](https://crates.io/crates/dirs) | 6.0.0 | MIT OR Apache-2.0 |
| [dirs-sys](https://crates.io/crates/dirs-sys) | 0.5.0 | MIT OR Apache-2.0 |
| [displaydoc](https://crates.io/crates/displaydoc) | 0.2.6 | MIT OR Apache-2.0 |
| [document-features](https://crates.io/crates/document-features) | 0.2.12 | MIT OR Apache-2.0 |
| [dom_query](https://crates.io/crates/dom_query) | 0.27.0 | MIT |
| [dpi](https://crates.io/crates/dpi) | 0.1.2 | Apache-2.0 AND MIT |
| [dtoa](https://crates.io/crates/dtoa) | 1.0.11 | MIT OR Apache-2.0 |
| [dtoa-short](https://crates.io/crates/dtoa-short) | 0.3.5 | MPL-2.0 |
| [dunce](https://crates.io/crates/dunce) | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| [dyn-clone](https://crates.io/crates/dyn-clone) | 1.0.20 | MIT OR Apache-2.0 |
| [either](https://crates.io/crates/either) | 1.16.0 | MIT OR Apache-2.0 |
| [encoding_rs](https://crates.io/crates/encoding_rs) | 0.8.35 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| [equivalent](https://crates.io/crates/equivalent) | 1.0.2 | Apache-2.0 OR MIT |
| [erased-serde](https://crates.io/crates/erased-serde) | 0.4.10 | MIT OR Apache-2.0 |
| [error-code](https://crates.io/crates/error-code) | 3.3.2 | BSL-1.0 |
| [esaxx-rs](https://crates.io/crates/esaxx-rs) | 0.1.10 | Apache-2.0 |
| [fallible-iterator](https://crates.io/crates/fallible-iterator) | 0.3.0 | MIT/Apache-2.0 |
| [fallible-streaming-iterator](https://crates.io/crates/fallible-streaming-iterator) | 0.1.9 | MIT/Apache-2.0 |
| [fancy-regex](https://crates.io/crates/fancy-regex) | 0.14.0 | MIT |
| [fast_image_resize](https://crates.io/crates/fast_image_resize) | 4.2.3 | MIT OR Apache-2.0 |
| [fastrand](https://crates.io/crates/fastrand) | 2.4.1 | Apache-2.0 OR MIT |
| [fax](https://crates.io/crates/fax) | 0.2.7 | MIT |
| [fdeflate](https://crates.io/crates/fdeflate) | 0.3.7 | MIT OR Apache-2.0 |
| [ferrous-opencc](https://crates.io/crates/ferrous-opencc) | 0.4.0 | Apache-2.0 |
| [ferrous-opencc-compiler](https://crates.io/crates/ferrous-opencc-compiler) | 0.4.0 | Apache-2.0 |
| [filetime](https://crates.io/crates/filetime) | 0.2.29 | MIT/Apache-2.0 |
| [flate2](https://crates.io/crates/flate2) | 1.1.9 | MIT OR Apache-2.0 |
| [fnv](https://crates.io/crates/fnv) | 1.0.7 | Apache-2.0 / MIT |
| [foldhash](https://crates.io/crates/foldhash) | 0.2.0 | Zlib |
| [form_urlencoded](https://crates.io/crates/form_urlencoded) | 1.2.2 | MIT OR Apache-2.0 |
| [fst](https://crates.io/crates/fst) | 0.4.7 | Unlicense/MIT |
| [futures-channel](https://crates.io/crates/futures-channel) | 0.3.32 | MIT OR Apache-2.0 |
| [futures-core](https://crates.io/crates/futures-core) | 0.3.32 | MIT OR Apache-2.0 |
| [futures-macro](https://crates.io/crates/futures-macro) | 0.3.32 | MIT OR Apache-2.0 |
| [futures-sink](https://crates.io/crates/futures-sink) | 0.3.32 | MIT OR Apache-2.0 |
| [futures-task](https://crates.io/crates/futures-task) | 0.3.32 | MIT OR Apache-2.0 |
| [futures-util](https://crates.io/crates/futures-util) | 0.3.32 | MIT OR Apache-2.0 |
| [generic-array](https://crates.io/crates/generic-array) | 0.14.7 | MIT |
| [gethostname](https://crates.io/crates/gethostname) | 1.1.0 | Apache-2.0 |
| [getrandom](https://crates.io/crates/getrandom) | 0.2.17 | MIT OR Apache-2.0 |
| [getrandom](https://crates.io/crates/getrandom) | 0.3.4 | MIT OR Apache-2.0 |
| [getrandom](https://crates.io/crates/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [gif](https://crates.io/crates/gif) | 0.14.2 | MIT OR Apache-2.0 |
| [glob](https://crates.io/crates/glob) | 0.3.3 | MIT OR Apache-2.0 |
| [half](https://crates.io/crates/half) | 2.7.1 | MIT OR Apache-2.0 |
| [hashbrown](https://crates.io/crates/hashbrown) | 0.12.3 | MIT OR Apache-2.0 |
| [hashbrown](https://crates.io/crates/hashbrown) | 0.14.5 | MIT OR Apache-2.0 |
| [hashbrown](https://crates.io/crates/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [hashlink](https://crates.io/crates/hashlink) | 0.9.1 | MIT OR Apache-2.0 |
| [heck](https://crates.io/crates/heck) | 0.5.0 | MIT OR Apache-2.0 |
| [html5ever](https://crates.io/crates/html5ever) | 0.38.0 | MIT OR Apache-2.0 |
| [http](https://crates.io/crates/http) | 1.4.2 | MIT OR Apache-2.0 |
| [http-body](https://crates.io/crates/http-body) | 1.0.1 | MIT |
| [http-body-util](https://crates.io/crates/http-body-util) | 0.1.3 | MIT |
| [http-range](https://crates.io/crates/http-range) | 0.1.5 | MIT |
| [httparse](https://crates.io/crates/httparse) | 1.10.1 | MIT OR Apache-2.0 |
| [hyper](https://crates.io/crates/hyper) | 1.10.1 | MIT |
| [hyper-rustls](https://crates.io/crates/hyper-rustls) | 0.27.9 | Apache-2.0 OR ISC OR MIT |
| [hyper-util](https://crates.io/crates/hyper-util) | 0.1.20 | MIT |
| [ico](https://crates.io/crates/ico) | 0.5.0 | MIT |
| [icu_collections](https://crates.io/crates/icu_collections) | 2.2.0 | Unicode-3.0 |
| [icu_locale_core](https://crates.io/crates/icu_locale_core) | 2.2.0 | Unicode-3.0 |
| [icu_normalizer](https://crates.io/crates/icu_normalizer) | 2.2.0 | Unicode-3.0 |
| [icu_normalizer_data](https://crates.io/crates/icu_normalizer_data) | 2.2.0 | Unicode-3.0 |
| [icu_properties](https://crates.io/crates/icu_properties) | 2.2.0 | Unicode-3.0 |
| [icu_properties_data](https://crates.io/crates/icu_properties_data) | 2.2.0 | Unicode-3.0 |
| [icu_provider](https://crates.io/crates/icu_provider) | 2.2.0 | Unicode-3.0 |
| [ident_case](https://crates.io/crates/ident_case) | 1.0.1 | MIT/Apache-2.0 |
| [idna](https://crates.io/crates/idna) | 1.1.0 | MIT OR Apache-2.0 |
| [idna_adapter](https://crates.io/crates/idna_adapter) | 1.2.2 | Apache-2.0 OR MIT |
| [image](https://crates.io/crates/image) | 0.25.10 | MIT OR Apache-2.0 |
| [image-webp](https://crates.io/crates/image-webp) | 0.2.4 | MIT OR Apache-2.0 |
| [imageproc](https://crates.io/crates/imageproc) | 0.27.0 | MIT |
| [indexmap](https://crates.io/crates/indexmap) | 1.9.3 | Apache-2.0 OR MIT |
| [indexmap](https://crates.io/crates/indexmap) | 2.14.0 | Apache-2.0 OR MIT |
| [infer](https://crates.io/crates/infer) | 0.19.0 | MIT |
| [ipnet](https://crates.io/crates/ipnet) | 2.12.0 | MIT OR Apache-2.0 |
| [itertools](https://crates.io/crates/itertools) | 0.14.0 | MIT OR Apache-2.0 |
| [itoa](https://crates.io/crates/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [json-patch](https://crates.io/crates/json-patch) | 3.0.1 | MIT/Apache-2.0 |
| [jsonptr](https://crates.io/crates/jsonptr) | 0.6.3 | MIT OR Apache-2.0 |
| [kamadak-exif](https://crates.io/crates/kamadak-exif) | 0.5.5 | BSD-2-Clause |
| [keyboard-types](https://crates.io/crates/keyboard-types) | 0.7.0 | MIT OR Apache-2.0 |
| [keyring](https://crates.io/crates/keyring) | 3.6.3 | MIT OR Apache-2.0 |
| [lazy_static](https://crates.io/crates/lazy_static) | 1.5.0 | MIT OR Apache-2.0 |
| [libc](https://crates.io/crates/libc) | 0.2.186 | MIT OR Apache-2.0 |
| [libc](https://crates.io/crates/libc) | 0.2.189 | MIT OR Apache-2.0 |
| [libloading](https://crates.io/crates/libloading) | 0.9.0 | ISC |
| [libm](https://crates.io/crates/libm) | 0.2.16 | MIT |
| [libsqlite3-sys](https://crates.io/crates/libsqlite3-sys) | 0.28.0 | MIT |
| [libwebp-sys](https://crates.io/crates/libwebp-sys) | 0.9.6 | MIT |
| [litemap](https://crates.io/crates/litemap) | 0.8.2 | Unicode-3.0 |
| [litrs](https://crates.io/crates/litrs) | 1.0.0 | MIT OR Apache-2.0 |
| [lock_api](https://crates.io/crates/lock_api) | 0.4.14 | MIT OR Apache-2.0 |
| [lofty](https://crates.io/crates/lofty) | 0.22.4 | MIT OR Apache-2.0 |
| [lofty_attr](https://crates.io/crates/lofty_attr) | 0.11.1 | MIT OR Apache-2.0 |
| [log](https://crates.io/crates/log) | 0.4.33 | MIT OR Apache-2.0 |
| [macro_rules_attribute](https://crates.io/crates/macro_rules_attribute) | 0.2.2 | Apache-2.0 OR MIT OR Zlib |
| [macro_rules_attribute-proc_macro](https://crates.io/crates/macro_rules_attribute-proc_macro) | 0.2.2 | Apache-2.0 OR MIT OR Zlib |
| [markup5ever](https://crates.io/crates/markup5ever) | 0.38.0 | MIT OR Apache-2.0 |
| [matchers](https://crates.io/crates/matchers) | 0.2.0 | MIT |
| [matrixmultiply](https://crates.io/crates/matrixmultiply) | 0.3.10 | MIT/Apache-2.0 |
| [memchr](https://crates.io/crates/memchr) | 2.8.2 | Unlicense OR MIT |
| [memchr](https://crates.io/crates/memchr) | 2.8.3 | Unlicense OR MIT |
| [mime](https://crates.io/crates/mime) | 0.3.17 | MIT OR Apache-2.0 |
| [minimal-lexical](https://crates.io/crates/minimal-lexical) | 0.2.1 | MIT/Apache-2.0 |
| [minisign-verify](https://crates.io/crates/minisign-verify) | 0.2.5 | MIT |
| [miniz_oxide](https://crates.io/crates/miniz_oxide) | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| [mio](https://crates.io/crates/mio) | 1.2.1 | MIT |
| [monostate](https://crates.io/crates/monostate) | 0.1.18 | MIT OR Apache-2.0 |
| [monostate-impl](https://crates.io/crates/monostate-impl) | 0.1.18 | MIT OR Apache-2.0 |
| [moxcms](https://crates.io/crates/moxcms) | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| [muda](https://crates.io/crates/muda) | 0.19.3 | Apache-2.0 OR MIT |
| [munge](https://crates.io/crates/munge) | 0.4.7 | MIT |
| [munge_macro](https://crates.io/crates/munge_macro) | 0.4.7 | MIT |
| [mutate_once](https://crates.io/crates/mutate_once) | 0.1.2 | BSD-2-Clause |
| [nalgebra](https://crates.io/crates/nalgebra) | 0.35.0 | Apache-2.0 |
| [ndarray](https://crates.io/crates/ndarray) | 0.16.1 | MIT OR Apache-2.0 |
| [ndarray](https://crates.io/crates/ndarray) | 0.17.2 | MIT OR Apache-2.0 |
| [new_debug_unreachable](https://crates.io/crates/new_debug_unreachable) | 1.0.6 | MIT |
| [nom](https://crates.io/crates/nom) | 7.1.3 | MIT |
| [notify](https://crates.io/crates/notify) | 8.2.0 | CC0-1.0 |
| [notify-types](https://crates.io/crates/notify-types) | 2.1.0 | MIT OR Apache-2.0 |
| [nu-ansi-term](https://crates.io/crates/nu-ansi-term) | 0.50.3 | MIT |
| [num](https://crates.io/crates/num) | 0.4.3 | MIT OR Apache-2.0 |
| [num-complex](https://crates.io/crates/num-complex) | 0.4.6 | MIT OR Apache-2.0 |
| [num-conv](https://crates.io/crates/num-conv) | 0.2.2 | MIT OR Apache-2.0 |
| [num-integer](https://crates.io/crates/num-integer) | 0.1.46 | MIT OR Apache-2.0 |
| [num-iter](https://crates.io/crates/num-iter) | 0.1.46 | MIT OR Apache-2.0 |
| [num-rational](https://crates.io/crates/num-rational) | 0.4.2 | MIT OR Apache-2.0 |
| [num-traits](https://crates.io/crates/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [ogg_pager](https://crates.io/crates/ogg_pager) | 0.7.2 | MIT OR Apache-2.0 |
| [once_cell](https://crates.io/crates/once_cell) | 1.21.4 | MIT OR Apache-2.0 |
| [open](https://crates.io/crates/open) | 5.3.6 | MIT |
| [option-ext](https://crates.io/crates/option-ext) | 0.2.0 | MPL-2.0 |
| [ort](https://crates.io/crates/ort) | 2.0.0-rc.12 | MIT OR Apache-2.0 |
| [ort-sys](https://crates.io/crates/ort-sys) | 2.0.0-rc.12 | MIT OR Apache-2.0 |
| [os_info](https://crates.io/crates/os_info) | 3.15.0 | MIT |
| [os_pipe](https://crates.io/crates/os_pipe) | 1.2.3 | MIT |
| [parking_lot](https://crates.io/crates/parking_lot) | 0.12.5 | MIT OR Apache-2.0 |
| [parking_lot_core](https://crates.io/crates/parking_lot_core) | 0.9.12 | MIT OR Apache-2.0 |
| [paste](https://crates.io/crates/paste) | 1.0.15 | MIT OR Apache-2.0 |
| [percent-encoding](https://crates.io/crates/percent-encoding) | 2.3.2 | MIT OR Apache-2.0 |
| [phf](https://crates.io/crates/phf) | 0.13.1 | MIT |
| [phf_generator](https://crates.io/crates/phf_generator) | 0.13.1 | MIT |
| [phf_macros](https://crates.io/crates/phf_macros) | 0.13.1 | MIT |
| [phf_shared](https://crates.io/crates/phf_shared) | 0.13.1 | MIT |
| [pin-project-lite](https://crates.io/crates/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [plist](https://crates.io/crates/plist) | 1.9.0 | MIT |
| [png](https://crates.io/crates/png) | 0.17.16 | MIT OR Apache-2.0 |
| [png](https://crates.io/crates/png) | 0.18.1 | MIT OR Apache-2.0 |
| [potential_utf](https://crates.io/crates/potential_utf) | 0.1.5 | Unicode-3.0 |
| [powerfmt](https://crates.io/crates/powerfmt) | 0.2.0 | MIT OR Apache-2.0 |
| [ppv-lite86](https://crates.io/crates/ppv-lite86) | 0.2.21 | MIT OR Apache-2.0 |
| [precomputed-hash](https://crates.io/crates/precomputed-hash) | 0.1.1 | MIT |
| [proc-macro2](https://crates.io/crates/proc-macro2) | 1.0.106 | MIT OR Apache-2.0 |
| [proc-macro2](https://crates.io/crates/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [psd](https://crates.io/crates/psd) | 0.3.5 | MIT/Apache-2.0 |
| [psl-types](https://crates.io/crates/psl-types) | 2.0.11 | MIT/Apache-2.0 |
| [ptr_meta](https://crates.io/crates/ptr_meta) | 0.3.1 | MIT |
| [ptr_meta_derive](https://crates.io/crates/ptr_meta_derive) | 0.3.1 | MIT |
| [publicsuffix](https://crates.io/crates/publicsuffix) | 2.3.0 | MIT/Apache-2.0 |
| [pxfm](https://crates.io/crates/pxfm) | 0.1.29 | BSD-3-Clause OR Apache-2.0 |
| [pxfm](https://crates.io/crates/pxfm) | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| [quick-error](https://crates.io/crates/quick-error) | 2.0.1 | MIT/Apache-2.0 |
| [quick-xml](https://crates.io/crates/quick-xml) | 0.36.2 | MIT |
| [quick-xml](https://crates.io/crates/quick-xml) | 0.39.4 | MIT |
| [quote](https://crates.io/crates/quote) | 1.0.46 | MIT OR Apache-2.0 |
| [quote](https://crates.io/crates/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [r2d2](https://crates.io/crates/r2d2) | 0.8.10 | MIT/Apache-2.0 |
| [r2d2_sqlite](https://crates.io/crates/r2d2_sqlite) | 0.24.0 | MIT |
| [rancor](https://crates.io/crates/rancor) | 0.1.2 | MIT |
| [rand](https://crates.io/crates/rand) | 0.10.1 | MIT OR Apache-2.0 |
| [rand](https://crates.io/crates/rand) | 0.9.4 | MIT OR Apache-2.0 |
| [rand_chacha](https://crates.io/crates/rand_chacha) | 0.9.0 | MIT OR Apache-2.0 |
| [rand_core](https://crates.io/crates/rand_core) | 0.10.1 | MIT OR Apache-2.0 |
| [rand_core](https://crates.io/crates/rand_core) | 0.9.5 | MIT OR Apache-2.0 |
| [rand_distr](https://crates.io/crates/rand_distr) | 0.6.0 | MIT OR Apache-2.0 |
| [raw-window-handle](https://crates.io/crates/raw-window-handle) | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| [rawpointer](https://crates.io/crates/rawpointer) | 0.2.1 | MIT/Apache-2.0 |
| [rayon](https://crates.io/crates/rayon) | 1.12.0 | MIT OR Apache-2.0 |
| [rayon-cond](https://crates.io/crates/rayon-cond) | 0.4.0 | Apache-2.0/MIT |
| [rayon-core](https://crates.io/crates/rayon-core) | 1.13.0 | MIT OR Apache-2.0 |
| [regex](https://crates.io/crates/regex) | 1.12.4 | MIT OR Apache-2.0 |
| [regex-automata](https://crates.io/crates/regex-automata) | 0.4.14 | MIT OR Apache-2.0 |
| [regex-syntax](https://crates.io/crates/regex-syntax) | 0.8.11 | MIT OR Apache-2.0 |
| [rend](https://crates.io/crates/rend) | 0.5.4 | MIT |
| [reqwest](https://crates.io/crates/reqwest) | 0.12.28 | MIT OR Apache-2.0 |
| [reqwest](https://crates.io/crates/reqwest) | 0.13.4 | MIT OR Apache-2.0 |
| [rfd](https://crates.io/crates/rfd) | 0.16.0 | MIT |
| [ring](https://crates.io/crates/ring) | 0.17.14 | Apache-2.0 AND ISC |
| [rkyv](https://crates.io/crates/rkyv) | 0.8.17 | MIT |
| [rkyv_derive](https://crates.io/crates/rkyv_derive) | 0.8.17 | MIT |
| [rsraw](https://crates.io/crates/rsraw) | 0.1.1 | MIT |
| [rsraw-sys](https://crates.io/crates/rsraw-sys) | 0.1.1 | MIT |
| [rusqlite](https://crates.io/crates/rusqlite) | 0.31.0 | MIT |
| [rustc-hash](https://crates.io/crates/rustc-hash) | 2.1.2 | Apache-2.0 OR MIT |
| [rustls](https://crates.io/crates/rustls) | 0.23.41 | Apache-2.0 OR ISC OR MIT |
| [rustls-pki-types](https://crates.io/crates/rustls-pki-types) | 1.15.0 | MIT OR Apache-2.0 |
| [rustls-platform-verifier](https://crates.io/crates/rustls-platform-verifier) | 0.7.0 | MIT OR Apache-2.0 |
| [rustls-webpki](https://crates.io/crates/rustls-webpki) | 0.103.13 | ISC |
| [rustversion](https://crates.io/crates/rustversion) | 1.0.22 | MIT OR Apache-2.0 |
| [ryu](https://crates.io/crates/ryu) | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| [safe_arch](https://crates.io/crates/safe_arch) | 1.0.0 | Zlib OR Apache-2.0 OR MIT |
| [same-file](https://crates.io/crates/same-file) | 1.0.6 | Unlicense/MIT |
| [scheduled-thread-pool](https://crates.io/crates/scheduled-thread-pool) | 0.2.7 | MIT/Apache-2.0 |
| [schemars](https://crates.io/crates/schemars) | 0.8.22 | MIT |
| [schemars_derive](https://crates.io/crates/schemars_derive) | 0.8.22 | MIT |
| [scopeguard](https://crates.io/crates/scopeguard) | 1.2.0 | MIT OR Apache-2.0 |
| [selectors](https://crates.io/crates/selectors) | 0.36.1 | MPL-2.0 |
| [semver](https://crates.io/crates/semver) | 1.0.28 | MIT OR Apache-2.0 |
| [serde](https://crates.io/crates/serde) | 1.0.228 | MIT OR Apache-2.0 |
| [serde](https://crates.io/crates/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde-untagged](https://crates.io/crates/serde-untagged) | 0.1.9 | MIT OR Apache-2.0 |
| [serde_core](https://crates.io/crates/serde_core) | 1.0.228 | MIT OR Apache-2.0 |
| [serde_core](https://crates.io/crates/serde_core) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://crates.io/crates/serde_derive) | 1.0.228 | MIT OR Apache-2.0 |
| [serde_derive](https://crates.io/crates/serde_derive) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive_internals](https://crates.io/crates/serde_derive_internals) | 0.29.1 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/crates/serde_json) | 1.0.150 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/crates/serde_json) | 1.0.151 | MIT OR Apache-2.0 |
| [serde_repr](https://crates.io/crates/serde_repr) | 0.1.20 | MIT OR Apache-2.0 |
| [serde_spanned](https://crates.io/crates/serde_spanned) | 1.1.1 | MIT OR Apache-2.0 |
| [serde_urlencoded](https://crates.io/crates/serde_urlencoded) | 0.7.1 | MIT/Apache-2.0 |
| [serde_with](https://crates.io/crates/serde_with) | 3.21.0 | MIT OR Apache-2.0 |
| [serde_with_macros](https://crates.io/crates/serde_with_macros) | 3.21.0 | MIT OR Apache-2.0 |
| [serialize-to-javascript](https://crates.io/crates/serialize-to-javascript) | 0.1.2 | MIT OR Apache-2.0 |
| [serialize-to-javascript-impl](https://crates.io/crates/serialize-to-javascript-impl) | 0.1.2 | MIT OR Apache-2.0 |
| [servo_arc](https://crates.io/crates/servo_arc) | 0.4.3 | MIT OR Apache-2.0 |
| [sha2](https://crates.io/crates/sha2) | 0.10.9 | MIT OR Apache-2.0 |
| [sharded-slab](https://crates.io/crates/sharded-slab) | 0.1.7 | MIT |
| [shared_child](https://crates.io/crates/shared_child) | 1.1.1 | MIT |
| [simba](https://crates.io/crates/simba) | 0.10.0 | Apache-2.0 |
| [simd-adler32](https://crates.io/crates/simd-adler32) | 0.3.9 | MIT |
| [simdutf8](https://crates.io/crates/simdutf8) | 0.1.5 | MIT OR Apache-2.0 |
| [similar](https://crates.io/crates/similar) | 2.7.0 | Apache-2.0 |
| [siphasher](https://crates.io/crates/siphasher) | 1.0.3 | MIT/Apache-2.0 |
| [slab](https://crates.io/crates/slab) | 0.4.12 | MIT |
| [smallvec](https://crates.io/crates/smallvec) | 1.15.2 | MIT OR Apache-2.0 |
| [socket2](https://crates.io/crates/socket2) | 0.6.4 | MIT OR Apache-2.0 |
| [softbuffer](https://crates.io/crates/softbuffer) | 0.4.8 | MIT OR Apache-2.0 |
| [spm_precompiled](https://crates.io/crates/spm_precompiled) | 0.1.4 | Apache-2.0 |
| [stable_deref_trait](https://crates.io/crates/stable_deref_trait) | 1.2.1 | MIT OR Apache-2.0 |
| [static_assertions](https://crates.io/crates/static_assertions) | 1.1.0 | MIT OR Apache-2.0 |
| [string_cache](https://crates.io/crates/string_cache) | 0.9.0 | MIT OR Apache-2.0 |
| [strsim](https://crates.io/crates/strsim) | 0.11.1 | MIT |
| [subtle](https://crates.io/crates/subtle) | 2.6.1 | BSD-3-Clause |
| [symlink](https://crates.io/crates/symlink) | 0.1.0 | MIT/Apache-2.0 |
| [syn](https://crates.io/crates/syn) | 2.0.118 | MIT OR Apache-2.0 |
| [syn](https://crates.io/crates/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [syn](https://crates.io/crates/syn) | 3.0.3 | MIT OR Apache-2.0 |
| [sync_wrapper](https://crates.io/crates/sync_wrapper) | 1.0.2 | Apache-2.0 |
| [synstructure](https://crates.io/crates/synstructure) | 0.13.2 | MIT |
| [sys-locale](https://crates.io/crates/sys-locale) | 0.3.2 | MIT OR Apache-2.0 |
| [tao](https://crates.io/crates/tao) | 0.35.3 | Apache-2.0 |
| [tauri](https://crates.io/crates/tauri) | 2.11.4 | Apache-2.0 OR MIT |
| [tauri-codegen](https://crates.io/crates/tauri-codegen) | 2.6.3 | Apache-2.0 OR MIT |
| [tauri-macros](https://crates.io/crates/tauri-macros) | 2.6.3 | Apache-2.0 OR MIT |
| [tauri-plugin-dialog](https://crates.io/crates/tauri-plugin-dialog) | 2.7.1 | Apache-2.0 OR MIT |
| [tauri-plugin-fs](https://crates.io/crates/tauri-plugin-fs) | 2.5.1 | Apache-2.0 OR MIT |
| [tauri-plugin-opener](https://crates.io/crates/tauri-plugin-opener) | 2.5.4 | Apache-2.0 OR MIT |
| [tauri-plugin-os](https://crates.io/crates/tauri-plugin-os) | 2.3.2 | Apache-2.0 OR MIT |
| [tauri-plugin-shell](https://crates.io/crates/tauri-plugin-shell) | 2.3.5 | Apache-2.0 OR MIT |
| [tauri-plugin-updater](https://crates.io/crates/tauri-plugin-updater) | 2.10.1 | Apache-2.0 OR MIT |
| [tauri-plugin-window-state](https://crates.io/crates/tauri-plugin-window-state) | 2.4.1 | Apache-2.0 OR MIT |
| [tauri-runtime](https://crates.io/crates/tauri-runtime) | 2.11.3 | Apache-2.0 OR MIT |
| [tauri-runtime-wry](https://crates.io/crates/tauri-runtime-wry) | 2.11.4 | Apache-2.0 OR MIT |
| [tauri-utils](https://crates.io/crates/tauri-utils) | 2.9.3 | Apache-2.0 OR MIT |
| [tempfile](https://crates.io/crates/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [tendril](https://crates.io/crates/tendril) | 0.5.0 | MIT OR Apache-2.0 |
| [thiserror](https://crates.io/crates/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror](https://crates.io/crates/thiserror) | 2.0.18 | MIT OR Apache-2.0 |
| [thiserror-impl](https://crates.io/crates/thiserror-impl) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror-impl](https://crates.io/crates/thiserror-impl) | 2.0.18 | MIT OR Apache-2.0 |
| [thread_local](https://crates.io/crates/thread_local) | 1.1.9 | MIT OR Apache-2.0 |
| [thumbhash](https://crates.io/crates/thumbhash) | 0.1.0 | MIT |
| [tiff](https://crates.io/crates/tiff) | 0.11.3 | MIT |
| [time](https://crates.io/crates/time) | 0.3.51 | MIT OR Apache-2.0 |
| [time-core](https://crates.io/crates/time-core) | 0.1.9 | MIT OR Apache-2.0 |
| [time-macros](https://crates.io/crates/time-macros) | 0.2.30 | MIT OR Apache-2.0 |
| [tinystr](https://crates.io/crates/tinystr) | 0.8.3 | Unicode-3.0 |
| [tokenizers](https://crates.io/crates/tokenizers) | 0.21.4 | Apache-2.0 |
| [tokio](https://crates.io/crates/tokio) | 1.52.3 | MIT |
| [tokio-macros](https://crates.io/crates/tokio-macros) | 2.7.0 | MIT |
| [tokio-rustls](https://crates.io/crates/tokio-rustls) | 0.26.4 | MIT OR Apache-2.0 |
| [tokio-util](https://crates.io/crates/tokio-util) | 0.7.18 | MIT |
| [toml](https://crates.io/crates/toml) | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_datetime](https://crates.io/crates/toml_datetime) | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_edit](https://crates.io/crates/toml_edit) | 0.25.12+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_parser](https://crates.io/crates/toml_parser) | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_writer](https://crates.io/crates/toml_writer) | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| [tower](https://crates.io/crates/tower) | 0.5.3 | MIT |
| [tower-http](https://crates.io/crates/tower-http) | 0.6.11 | MIT |
| [tower-layer](https://crates.io/crates/tower-layer) | 0.3.3 | MIT |
| [tower-service](https://crates.io/crates/tower-service) | 0.3.3 | MIT |
| [tracing](https://crates.io/crates/tracing) | 0.1.44 | MIT |
| [tracing-appender](https://crates.io/crates/tracing-appender) | 0.2.5 | MIT |
| [tracing-attributes](https://crates.io/crates/tracing-attributes) | 0.1.31 | MIT |
| [tracing-core](https://crates.io/crates/tracing-core) | 0.1.36 | MIT |
| [tracing-log](https://crates.io/crates/tracing-log) | 0.2.0 | MIT |
| [tracing-panic](https://crates.io/crates/tracing-panic) | 0.1.2 | MIT/Apache-2.0 |
| [tracing-serde](https://crates.io/crates/tracing-serde) | 0.2.0 | MIT |
| [tracing-subscriber](https://crates.io/crates/tracing-subscriber) | 0.3.23 | MIT |
| [trash](https://crates.io/crates/trash) | 5.2.6 | MIT |
| [tray-icon](https://crates.io/crates/tray-icon) | 0.24.1 | MIT OR Apache-2.0 |
| [try-lock](https://crates.io/crates/try-lock) | 0.2.5 | MIT |
| [typeid](https://crates.io/crates/typeid) | 1.0.3 | MIT OR Apache-2.0 |
| [typenum](https://crates.io/crates/typenum) | 1.20.1 | MIT OR Apache-2.0 |
| [unic-char-property](https://crates.io/crates/unic-char-property) | 0.9.0 | MIT/Apache-2.0 |
| [unic-char-range](https://crates.io/crates/unic-char-range) | 0.9.0 | MIT/Apache-2.0 |
| [unic-common](https://crates.io/crates/unic-common) | 0.9.0 | MIT/Apache-2.0 |
| [unic-ucd-ident](https://crates.io/crates/unic-ucd-ident) | 0.9.0 | MIT/Apache-2.0 |
| [unic-ucd-version](https://crates.io/crates/unic-ucd-version) | 0.9.0 | MIT/Apache-2.0 |
| [unicode-ident](https://crates.io/crates/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-normalization-alignments](https://crates.io/crates/unicode-normalization-alignments) | 0.1.12 | MIT/Apache-2.0 |
| [unicode-segmentation](https://crates.io/crates/unicode-segmentation) | 1.13.3 | MIT OR Apache-2.0 |
| [unicode_categories](https://crates.io/crates/unicode_categories) | 0.1.1 | MIT OR Apache-2.0 |
| [untrusted](https://crates.io/crates/untrusted) | 0.9.0 | ISC |
| [url](https://crates.io/crates/url) | 2.5.8 | MIT OR Apache-2.0 |
| [urlpattern](https://crates.io/crates/urlpattern) | 0.3.0 | MIT |
| [utf-8](https://crates.io/crates/utf-8) | 0.7.6 | MIT OR Apache-2.0 |
| [utf8_iter](https://crates.io/crates/utf8_iter) | 1.0.4 | Apache-2.0 OR MIT |
| [uuid](https://crates.io/crates/uuid) | 1.23.4 | Apache-2.0 OR MIT |
| [walkdir](https://crates.io/crates/walkdir) | 2.5.0 | Unlicense/MIT |
| [wallpaper](https://crates.io/crates/wallpaper) | 3.2.0 | Unlicense |
| [want](https://crates.io/crates/want) | 0.3.1 | MIT |
| [web_atoms](https://crates.io/crates/web_atoms) | 0.2.5 | MIT OR Apache-2.0 |
| [webp](https://crates.io/crates/webp) | 0.3.1 | MIT OR Apache-2.0 |
| [webpki-roots](https://crates.io/crates/webpki-roots) | 1.0.8 | CDLA-Permissive-2.0 |
| [webview2-com](https://crates.io/crates/webview2-com) | 0.38.2 | MIT |
| [webview2-com-macros](https://crates.io/crates/webview2-com-macros) | 0.8.1 | MIT |
| [webview2-com-sys](https://crates.io/crates/webview2-com-sys) | 0.38.2 | MIT |
| [weezl](https://crates.io/crates/weezl) | 0.1.12 | MIT OR Apache-2.0 |
| [wide](https://crates.io/crates/wide) | 1.5.0 | Zlib OR Apache-2.0 OR MIT |
| [winapi](https://crates.io/crates/winapi) | 0.3.9 | MIT/Apache-2.0 |
| [winapi-util](https://crates.io/crates/winapi-util) | 0.1.11 | Unlicense OR MIT |
| [window-vibrancy](https://crates.io/crates/window-vibrancy) | 0.6.0 | Apache-2.0 OR MIT |
| [windows](https://crates.io/crates/windows) | 0.56.0 | MIT OR Apache-2.0 |
| [windows](https://crates.io/crates/windows) | 0.58.0 | MIT OR Apache-2.0 |
| [windows](https://crates.io/crates/windows) | 0.61.3 | MIT OR Apache-2.0 |
| [windows-collections](https://crates.io/crates/windows-collections) | 0.2.0 | MIT OR Apache-2.0 |
| [windows-core](https://crates.io/crates/windows-core) | 0.56.0 | MIT OR Apache-2.0 |
| [windows-core](https://crates.io/crates/windows-core) | 0.58.0 | MIT OR Apache-2.0 |
| [windows-core](https://crates.io/crates/windows-core) | 0.61.2 | MIT OR Apache-2.0 |
| [windows-future](https://crates.io/crates/windows-future) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-implement](https://crates.io/crates/windows-implement) | 0.56.0 | MIT OR Apache-2.0 |
| [windows-implement](https://crates.io/crates/windows-implement) | 0.58.0 | MIT OR Apache-2.0 |
| [windows-implement](https://crates.io/crates/windows-implement) | 0.60.2 | MIT OR Apache-2.0 |
| [windows-interface](https://crates.io/crates/windows-interface) | 0.56.0 | MIT OR Apache-2.0 |
| [windows-interface](https://crates.io/crates/windows-interface) | 0.58.0 | MIT OR Apache-2.0 |
| [windows-interface](https://crates.io/crates/windows-interface) | 0.59.3 | MIT OR Apache-2.0 |
| [windows-link](https://crates.io/crates/windows-link) | 0.1.3 | MIT OR Apache-2.0 |
| [windows-link](https://crates.io/crates/windows-link) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-numerics](https://crates.io/crates/windows-numerics) | 0.2.0 | MIT OR Apache-2.0 |
| [windows-result](https://crates.io/crates/windows-result) | 0.1.2 | MIT OR Apache-2.0 |
| [windows-result](https://crates.io/crates/windows-result) | 0.2.0 | MIT OR Apache-2.0 |
| [windows-result](https://crates.io/crates/windows-result) | 0.3.4 | MIT OR Apache-2.0 |
| [windows-strings](https://crates.io/crates/windows-strings) | 0.1.0 | MIT OR Apache-2.0 |
| [windows-strings](https://crates.io/crates/windows-strings) | 0.4.2 | MIT OR Apache-2.0 |
| [windows-sys](https://crates.io/crates/windows-sys) | 0.59.0 | MIT OR Apache-2.0 |
| [windows-sys](https://crates.io/crates/windows-sys) | 0.60.2 | MIT OR Apache-2.0 |
| [windows-sys](https://crates.io/crates/windows-sys) | 0.61.2 | MIT OR Apache-2.0 |
| [windows-targets](https://crates.io/crates/windows-targets) | 0.52.6 | MIT OR Apache-2.0 |
| [windows-targets](https://crates.io/crates/windows-targets) | 0.53.5 | MIT OR Apache-2.0 |
| [windows-threading](https://crates.io/crates/windows-threading) | 0.1.0 | MIT OR Apache-2.0 |
| [windows-version](https://crates.io/crates/windows-version) | 0.1.7 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://crates.io/crates/windows_x86_64_msvc) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://crates.io/crates/windows_x86_64_msvc) | 0.53.1 | MIT OR Apache-2.0 |
| [winnow](https://crates.io/crates/winnow) | 1.0.3 | MIT |
| [winreg](https://crates.io/crates/winreg) | 0.9.0 | MIT |
| [writeable](https://crates.io/crates/writeable) | 0.6.3 | Unicode-3.0 |
| [wry](https://crates.io/crates/wry) | 0.55.1 | Apache-2.0 OR MIT |
| [xxhash-rust](https://crates.io/crates/xxhash-rust) | 0.8.15 | BSL-1.0 |
| [yoke](https://crates.io/crates/yoke) | 0.8.3 | Unicode-3.0 |
| [yoke-derive](https://crates.io/crates/yoke-derive) | 0.8.2 | Unicode-3.0 |
| [zerocopy](https://crates.io/crates/zerocopy) | 0.8.52 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerocopy-derive](https://crates.io/crates/zerocopy-derive) | 0.8.52 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerofrom](https://crates.io/crates/zerofrom) | 0.1.8 | Unicode-3.0 |
| [zerofrom-derive](https://crates.io/crates/zerofrom-derive) | 0.1.7 | Unicode-3.0 |
| [zeroize](https://crates.io/crates/zeroize) | 1.9.0 | Apache-2.0 OR MIT |
| [zerotrie](https://crates.io/crates/zerotrie) | 0.2.4 | Unicode-3.0 |
| [zerovec](https://crates.io/crates/zerovec) | 0.11.6 | Unicode-3.0 |
| [zerovec-derive](https://crates.io/crates/zerovec-derive) | 0.11.3 | Unicode-3.0 |
| [zip](https://crates.io/crates/zip) | 2.4.2 | MIT |
| [zip](https://crates.io/crates/zip) | 4.6.1 | MIT |
| [zmij](https://crates.io/crates/zmij) | 1.0.21 | MIT |
| [zmij](https://crates.io/crates/zmij) | 1.0.23 | MIT |
| [zopfli](https://crates.io/crates/zopfli) | 0.8.3 | Apache-2.0 |
| [zune-core](https://crates.io/crates/zune-core) | 0.5.1 | MIT OR Apache-2.0 OR Zlib |
| [zune-jpeg](https://crates.io/crates/zune-jpeg) | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

## npm packages (164) — frontend bundle

Production dependency closure from `package-lock.json`, plus explicitly shipped native runtime packages.

| Package | Version | License | Shipping note |
| --- | --- | --- | --- |
| [@babel/helper-string-parser](https://www.npmjs.com/package/@babel/helper-string-parser) | 7.29.7 | MIT |  |
| [@babel/helper-validator-identifier](https://www.npmjs.com/package/@babel/helper-validator-identifier) | 7.29.7 | MIT |  |
| [@babel/parser](https://www.npmjs.com/package/@babel/parser) | 7.29.7 | MIT |  |
| [@babel/types](https://www.npmjs.com/package/@babel/types) | 7.29.7 | MIT |  |
| [@cropper/element](https://www.npmjs.com/package/@cropper/element) | 2.1.1 | MIT |  |
| [@cropper/element-canvas](https://www.npmjs.com/package/@cropper/element-canvas) | 2.1.1 | MIT |  |
| [@cropper/element-crosshair](https://www.npmjs.com/package/@cropper/element-crosshair) | 2.1.1 | MIT |  |
| [@cropper/element-grid](https://www.npmjs.com/package/@cropper/element-grid) | 2.1.1 | MIT |  |
| [@cropper/element-handle](https://www.npmjs.com/package/@cropper/element-handle) | 2.1.1 | MIT |  |
| [@cropper/element-image](https://www.npmjs.com/package/@cropper/element-image) | 2.1.1 | MIT |  |
| [@cropper/element-selection](https://www.npmjs.com/package/@cropper/element-selection) | 2.1.1 | MIT |  |
| [@cropper/element-shade](https://www.npmjs.com/package/@cropper/element-shade) | 2.1.1 | MIT |  |
| [@cropper/element-viewer](https://www.npmjs.com/package/@cropper/element-viewer) | 2.1.1 | MIT |  |
| [@cropper/elements](https://www.npmjs.com/package/@cropper/elements) | 2.1.1 | MIT |  |
| [@cropper/utils](https://www.npmjs.com/package/@cropper/utils) | 2.1.1 | MIT |  |
| [@floating-ui/core](https://www.npmjs.com/package/@floating-ui/core) | 1.8.0 | MIT |  |
| [@floating-ui/dom](https://www.npmjs.com/package/@floating-ui/dom) | 1.8.0 | MIT |  |
| [@floating-ui/utils](https://www.npmjs.com/package/@floating-ui/utils) | 0.2.12 | MIT |  |
| [@floating-ui/vue](https://www.npmjs.com/package/@floating-ui/vue) | 2.0.1 | MIT |  |
| [@fontsource-variable/inter](https://www.npmjs.com/package/@fontsource-variable/inter) | 5.2.8 | OFL-1.1 |  |
| [@intlify/core-base](https://www.npmjs.com/package/@intlify/core-base) | 9.14.5 | MIT |  |
| [@intlify/message-compiler](https://www.npmjs.com/package/@intlify/message-compiler) | 9.14.5 | MIT |  |
| [@intlify/shared](https://www.npmjs.com/package/@intlify/shared) | 9.14.5 | MIT |  |
| [@jridgewell/sourcemap-codec](https://www.npmjs.com/package/@jridgewell/sourcemap-codec) | 1.5.5 | MIT |  |
| [@lucide/vue](https://www.npmjs.com/package/@lucide/vue) | 1.17.0 | ISC |  |
| [@napi-rs/canvas](https://www.npmjs.com/package/@napi-rs/canvas) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-android-arm64](https://www.npmjs.com/package/@napi-rs/canvas-android-arm64) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-darwin-arm64](https://www.npmjs.com/package/@napi-rs/canvas-darwin-arm64) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-darwin-x64](https://www.npmjs.com/package/@napi-rs/canvas-darwin-x64) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-arm-gnueabihf](https://www.npmjs.com/package/@napi-rs/canvas-linux-arm-gnueabihf) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-arm64-gnu](https://www.npmjs.com/package/@napi-rs/canvas-linux-arm64-gnu) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-arm64-musl](https://www.npmjs.com/package/@napi-rs/canvas-linux-arm64-musl) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-riscv64-gnu](https://www.npmjs.com/package/@napi-rs/canvas-linux-riscv64-gnu) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-x64-gnu](https://www.npmjs.com/package/@napi-rs/canvas-linux-x64-gnu) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-linux-x64-musl](https://www.npmjs.com/package/@napi-rs/canvas-linux-x64-musl) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-win32-arm64-msvc](https://www.npmjs.com/package/@napi-rs/canvas-win32-arm64-msvc) | 0.1.100 | MIT |  |
| [@napi-rs/canvas-win32-x64-msvc](https://www.npmjs.com/package/@napi-rs/canvas-win32-x64-msvc) | 0.1.100 | MIT |  |
| [@shikijs/core](https://www.npmjs.com/package/@shikijs/core) | 4.3.1 | MIT |  |
| [@shikijs/engine-javascript](https://www.npmjs.com/package/@shikijs/engine-javascript) | 4.3.1 | MIT |  |
| [@shikijs/engine-oniguruma](https://www.npmjs.com/package/@shikijs/engine-oniguruma) | 4.3.1 | MIT |  |
| [@shikijs/langs](https://www.npmjs.com/package/@shikijs/langs) | 4.3.1 | MIT |  |
| [@shikijs/primitive](https://www.npmjs.com/package/@shikijs/primitive) | 4.3.1 | MIT |  |
| [@shikijs/themes](https://www.npmjs.com/package/@shikijs/themes) | 4.3.1 | MIT |  |
| [@shikijs/types](https://www.npmjs.com/package/@shikijs/types) | 4.3.1 | MIT |  |
| [@shikijs/vscode-textmate](https://www.npmjs.com/package/@shikijs/vscode-textmate) | 10.0.2 | MIT |  |
| [@tanstack/virtual-core](https://www.npmjs.com/package/@tanstack/virtual-core) | 3.17.5 | MIT |  |
| [@tanstack/vue-virtual](https://www.npmjs.com/package/@tanstack/vue-virtual) | 3.13.33 | MIT |  |
| [@tauri-apps/api](https://www.npmjs.com/package/@tauri-apps/api) | 2.11.0 | Apache-2.0 OR MIT |  |
| [@tauri-apps/plugin-dialog](https://www.npmjs.com/package/@tauri-apps/plugin-dialog) | 2.7.1 | MIT OR Apache-2.0 |  |
| [@tauri-apps/plugin-os](https://www.npmjs.com/package/@tauri-apps/plugin-os) | 2.3.2 | MIT OR Apache-2.0 |  |
| [@tauri-apps/plugin-shell](https://www.npmjs.com/package/@tauri-apps/plugin-shell) | 2.3.5 | MIT OR Apache-2.0 |  |
| [@tauri-apps/plugin-window-state](https://www.npmjs.com/package/@tauri-apps/plugin-window-state) | 2.4.1 | MIT OR Apache-2.0 |  |
| [@types/debug](https://www.npmjs.com/package/@types/debug) | 4.1.13 | MIT |  |
| [@types/hast](https://www.npmjs.com/package/@types/hast) | 3.0.4 | MIT |  |
| [@types/katex](https://www.npmjs.com/package/@types/katex) | 0.16.8 | MIT |  |
| [@types/mdast](https://www.npmjs.com/package/@types/mdast) | 4.0.4 | MIT |  |
| [@types/ms](https://www.npmjs.com/package/@types/ms) | 2.1.0 | MIT |  |
| [@types/unist](https://www.npmjs.com/package/@types/unist) | 3.0.3 | MIT |  |
| [@ungap/structured-clone](https://www.npmjs.com/package/@ungap/structured-clone) | 1.3.2 | ISC |  |
| [@vscode/markdown-editor](https://www.npmjs.com/package/@vscode/markdown-editor) | 0.0.2-0 | MIT |  |
| [@vscode/observables](https://www.npmjs.com/package/@vscode/observables) | 0.1.1-0 | MIT |  |
| [@vue/compiler-core](https://www.npmjs.com/package/@vue/compiler-core) | 3.5.35 | MIT |  |
| [@vue/compiler-dom](https://www.npmjs.com/package/@vue/compiler-dom) | 3.5.35 | MIT |  |
| [@vue/compiler-sfc](https://www.npmjs.com/package/@vue/compiler-sfc) | 3.5.35 | MIT |  |
| [@vue/compiler-ssr](https://www.npmjs.com/package/@vue/compiler-ssr) | 3.5.35 | MIT |  |
| [@vue/devtools-api](https://www.npmjs.com/package/@vue/devtools-api) | 6.6.4 | MIT |  |
| [@vue/devtools-api](https://www.npmjs.com/package/@vue/devtools-api) | 6.6.4 | MIT |  |
| [@vue/devtools-api](https://www.npmjs.com/package/@vue/devtools-api) | 7.7.9 | MIT |  |
| [@vue/devtools-kit](https://www.npmjs.com/package/@vue/devtools-kit) | 7.7.9 | MIT |  |
| [@vue/devtools-shared](https://www.npmjs.com/package/@vue/devtools-shared) | 7.7.9 | MIT |  |
| [@vue/reactivity](https://www.npmjs.com/package/@vue/reactivity) | 3.5.35 | MIT |  |
| [@vue/runtime-core](https://www.npmjs.com/package/@vue/runtime-core) | 3.5.35 | MIT |  |
| [@vue/runtime-dom](https://www.npmjs.com/package/@vue/runtime-dom) | 3.5.35 | MIT |  |
| [@vue/server-renderer](https://www.npmjs.com/package/@vue/server-renderer) | 3.5.35 | MIT |  |
| [@vue/shared](https://www.npmjs.com/package/@vue/shared) | 3.5.35 | MIT |  |
| [birpc](https://www.npmjs.com/package/birpc) | 2.9.0 | MIT |  |
| [ccount](https://www.npmjs.com/package/ccount) | 2.0.1 | MIT |  |
| [character-entities](https://www.npmjs.com/package/character-entities) | 2.0.2 | MIT |  |
| [character-entities-html4](https://www.npmjs.com/package/character-entities-html4) | 2.1.0 | MIT |  |
| [character-entities-legacy](https://www.npmjs.com/package/character-entities-legacy) | 3.0.0 | MIT |  |
| [comma-separated-tokens](https://www.npmjs.com/package/comma-separated-tokens) | 2.0.3 | MIT |  |
| [commander](https://www.npmjs.com/package/commander) | 8.3.0 | MIT |  |
| [copy-anything](https://www.npmjs.com/package/copy-anything) | 4.0.5 | MIT |  |
| [cropperjs](https://www.npmjs.com/package/cropperjs) | 2.1.1 | MIT |  |
| [csstype](https://www.npmjs.com/package/csstype) | 3.2.3 | MIT |  |
| [debug](https://www.npmjs.com/package/debug) | 4.4.3 | MIT |  |
| [decode-named-character-reference](https://www.npmjs.com/package/decode-named-character-reference) | 1.3.0 | MIT |  |
| [dequal](https://www.npmjs.com/package/dequal) | 2.0.3 | MIT |  |
| [devlop](https://www.npmjs.com/package/devlop) | 1.1.0 | MIT |  |
| [diff-match-patch](https://www.npmjs.com/package/diff-match-patch) | 1.0.5 | Apache-2.0 |  |
| [entities](https://www.npmjs.com/package/entities) | 7.0.1 | BSD-2-Clause |  |
| [estree-walker](https://www.npmjs.com/package/estree-walker) | 2.0.2 | MIT |  |
| [hast-util-to-html](https://www.npmjs.com/package/hast-util-to-html) | 9.0.5 | MIT |  |
| [hast-util-whitespace](https://www.npmjs.com/package/hast-util-whitespace) | 3.0.0 | MIT |  |
| [hookable](https://www.npmjs.com/package/hookable) | 5.5.3 | MIT |  |
| [html-void-elements](https://www.npmjs.com/package/html-void-elements) | 3.0.0 | MIT |  |
| [is-what](https://www.npmjs.com/package/is-what) | 5.5.0 | MIT |  |
| [katex](https://www.npmjs.com/package/katex) | 0.16.47 | MIT |  |
| [magic-string](https://www.npmjs.com/package/magic-string) | 0.30.21 | MIT |  |
| [mdast-util-to-hast](https://www.npmjs.com/package/mdast-util-to-hast) | 13.2.1 | MIT |  |
| [micromark](https://www.npmjs.com/package/micromark) | 4.0.2 | MIT |  |
| [micromark-core-commonmark](https://www.npmjs.com/package/micromark-core-commonmark) | 2.0.3 | MIT |  |
| [micromark-extension-gfm](https://www.npmjs.com/package/micromark-extension-gfm) | 3.0.0 | MIT |  |
| [micromark-extension-gfm-autolink-literal](https://www.npmjs.com/package/micromark-extension-gfm-autolink-literal) | 2.1.0 | MIT |  |
| [micromark-extension-gfm-footnote](https://www.npmjs.com/package/micromark-extension-gfm-footnote) | 2.1.0 | MIT |  |
| [micromark-extension-gfm-strikethrough](https://www.npmjs.com/package/micromark-extension-gfm-strikethrough) | 2.1.0 | MIT |  |
| [micromark-extension-gfm-table](https://www.npmjs.com/package/micromark-extension-gfm-table) | 2.1.1 | MIT |  |
| [micromark-extension-gfm-tagfilter](https://www.npmjs.com/package/micromark-extension-gfm-tagfilter) | 2.0.0 | MIT |  |
| [micromark-extension-gfm-task-list-item](https://www.npmjs.com/package/micromark-extension-gfm-task-list-item) | 2.1.0 | MIT |  |
| [micromark-extension-math](https://www.npmjs.com/package/micromark-extension-math) | 3.1.0 | MIT |  |
| [micromark-factory-destination](https://www.npmjs.com/package/micromark-factory-destination) | 2.0.1 | MIT |  |
| [micromark-factory-label](https://www.npmjs.com/package/micromark-factory-label) | 2.0.1 | MIT |  |
| [micromark-factory-space](https://www.npmjs.com/package/micromark-factory-space) | 2.0.1 | MIT |  |
| [micromark-factory-title](https://www.npmjs.com/package/micromark-factory-title) | 2.0.1 | MIT |  |
| [micromark-factory-whitespace](https://www.npmjs.com/package/micromark-factory-whitespace) | 2.0.1 | MIT |  |
| [micromark-util-character](https://www.npmjs.com/package/micromark-util-character) | 2.1.1 | MIT |  |
| [micromark-util-chunked](https://www.npmjs.com/package/micromark-util-chunked) | 2.0.1 | MIT |  |
| [micromark-util-classify-character](https://www.npmjs.com/package/micromark-util-classify-character) | 2.0.1 | MIT |  |
| [micromark-util-combine-extensions](https://www.npmjs.com/package/micromark-util-combine-extensions) | 2.0.1 | MIT |  |
| [micromark-util-decode-numeric-character-reference](https://www.npmjs.com/package/micromark-util-decode-numeric-character-reference) | 2.0.2 | MIT |  |
| [micromark-util-encode](https://www.npmjs.com/package/micromark-util-encode) | 2.0.1 | MIT |  |
| [micromark-util-html-tag-name](https://www.npmjs.com/package/micromark-util-html-tag-name) | 2.0.1 | MIT |  |
| [micromark-util-normalize-identifier](https://www.npmjs.com/package/micromark-util-normalize-identifier) | 2.0.1 | MIT |  |
| [micromark-util-resolve-all](https://www.npmjs.com/package/micromark-util-resolve-all) | 2.0.1 | MIT |  |
| [micromark-util-sanitize-uri](https://www.npmjs.com/package/micromark-util-sanitize-uri) | 2.0.1 | MIT |  |
| [micromark-util-subtokenize](https://www.npmjs.com/package/micromark-util-subtokenize) | 2.1.0 | MIT |  |
| [micromark-util-symbol](https://www.npmjs.com/package/micromark-util-symbol) | 2.0.1 | MIT |  |
| [micromark-util-types](https://www.npmjs.com/package/micromark-util-types) | 2.0.2 | MIT |  |
| [mitt](https://www.npmjs.com/package/mitt) | 3.0.1 | MIT |  |
| [ms](https://www.npmjs.com/package/ms) | 2.1.3 | MIT |  |
| [nanoid](https://www.npmjs.com/package/nanoid) | 3.3.16 | MIT |  |
| [oniguruma-parser](https://www.npmjs.com/package/oniguruma-parser) | 0.12.2 | MIT |  |
| [oniguruma-to-es](https://www.npmjs.com/package/oniguruma-to-es) | 4.3.6 | MIT |  |
| [onnxruntime-node](https://www.npmjs.com/package/onnxruntime-node) | 1.26.0 | MIT | devDependency; native DLLs are copied into the installer by build-ai-worker.mjs |
| [pdfjs-dist](https://www.npmjs.com/package/pdfjs-dist) | 4.10.38 | Apache-2.0 |  |
| [perfect-debounce](https://www.npmjs.com/package/perfect-debounce) | 1.0.0 | MIT |  |
| [picocolors](https://www.npmjs.com/package/picocolors) | 1.1.1 | ISC |  |
| [pinia](https://www.npmjs.com/package/pinia) | 3.0.4 | MIT |  |
| [postcss](https://www.npmjs.com/package/postcss) | 8.5.25 | MIT |  |
| [property-information](https://www.npmjs.com/package/property-information) | 7.2.0 | MIT |  |
| [regex](https://www.npmjs.com/package/regex) | 6.1.0 | MIT |  |
| [regex-recursion](https://www.npmjs.com/package/regex-recursion) | 6.0.2 | MIT |  |
| [regex-utilities](https://www.npmjs.com/package/regex-utilities) | 2.3.0 | MIT |  |
| [rfdc](https://www.npmjs.com/package/rfdc) | 1.4.1 | MIT |  |
| [shiki](https://www.npmjs.com/package/shiki) | 4.3.1 | MIT |  |
| [source-map-js](https://www.npmjs.com/package/source-map-js) | 1.2.1 | BSD-3-Clause |  |
| [space-separated-tokens](https://www.npmjs.com/package/space-separated-tokens) | 2.0.2 | MIT |  |
| [speakingurl](https://www.npmjs.com/package/speakingurl) | 14.0.1 | BSD-3-Clause |  |
| [stringify-entities](https://www.npmjs.com/package/stringify-entities) | 4.0.4 | MIT |  |
| [superjson](https://www.npmjs.com/package/superjson) | 2.2.6 | MIT |  |
| [trim-lines](https://www.npmjs.com/package/trim-lines) | 3.0.1 | MIT |  |
| [typescript](https://www.npmjs.com/package/typescript) | 5.6.3 | Apache-2.0 |  |
| [unist-util-is](https://www.npmjs.com/package/unist-util-is) | 6.0.1 | MIT |  |
| [unist-util-position](https://www.npmjs.com/package/unist-util-position) | 5.0.0 | MIT |  |
| [unist-util-stringify-position](https://www.npmjs.com/package/unist-util-stringify-position) | 4.0.0 | MIT |  |
| [unist-util-visit](https://www.npmjs.com/package/unist-util-visit) | 5.1.0 | MIT |  |
| [unist-util-visit-parents](https://www.npmjs.com/package/unist-util-visit-parents) | 6.0.2 | MIT |  |
| [vditor](https://www.npmjs.com/package/vditor) | 3.11.3 | MIT |  |
| [vfile](https://www.npmjs.com/package/vfile) | 6.0.3 | MIT |  |
| [vfile-message](https://www.npmjs.com/package/vfile-message) | 4.0.3 | MIT |  |
| [vue](https://www.npmjs.com/package/vue) | 3.5.35 | MIT |  |
| [vue-i18n](https://www.npmjs.com/package/vue-i18n) | 9.14.5 | MIT |  |
| [vue-router](https://www.npmjs.com/package/vue-router) | 4.6.4 | MIT |  |
| [zwitch](https://www.npmjs.com/package/zwitch) | 2.0.4 | MIT |  |

## Vendored source (3) — bundled into frontend

第三方源码以钉定版本 vendored 进本仓(不经包管理器),打进前端 bundle 随产品分发。
对应源目录的许可证文件会进入安装包 `legal/`(见 VENDOR.md / LICENSE)。

| Source | Version (pinned) | License | Notes |
| --- | --- | --- | --- |
| [foliate-js](https://github.com/johnfactotum/foliate-js) | 78914ae | MIT | vendored at pinned commit; PDF backend stripped |
| [Graphviz (embedded in Viz.js full.render.js)](https://gitlab.com/graphviz/graphviz) | 2.40.1 | EPL-1.0 | Emscripten object code built by Viz.js 2.1.2 (MIT); unmodified upstream, source availability entry recorded in legal/graphviz |
| [Lute (lute.min.js)](https://github.com/88250/lute) | 1.7.6 | MulanPSL-2.0 | Go object code shipped by Vditor 3.11.3; unmodified upstream, text in legal/lute |

## Additional shipped runtimes (2)

这些组件不由根 lockfile 的普通依赖闭包完整表达，但其代码或二进制会进入产品运行路径。
完整许可证/归属材料由 `node scripts/prepare-legal-resources.mjs` 生成到安装包 `legal/`。

| Component | Version | License | Notes |
| --- | --- | --- | --- |
| [FFmpeg (BtbN LGPL-shared runtime)](https://github.com/BtbN/FFmpeg-Builds) | autobuild-2026-07-24-13-32 | LGPL-2.1-or-later | downloaded on demand; fixed FFmpeg/BtbN license texts are emitted under legal/ffmpeg; source link is shown in the app |
| [LibRaw (embedded by rsraw-sys)](https://www.libraw.org) | rsraw-sys-0.1.1 | CDDL-1.0 OR LGPL-2.1-only | embedded in raw-worker; LGPL-2.1-only branch elected from CDDL-1.0 OR LGPL-2.1-only, upstream texts kept under legal/LibRaw; the OSS source release ships no RAW binary, and a future static-link distribution still owes LGPL-2.1 source/relink materials or the §3 GPL-3.0 route — the installer is not thereby declared compliant |

## Review notes

### ⚠ Strong-copyleft flagged (manual legal review REQUIRED before release)

- FFmpeg (BtbN LGPL-shared runtime)@autobuild-2026-07-24-13-32 (runtime-download): LGPL-2.1-or-later
- LibRaw (embedded by rsraw-sys)@rsraw-sys-0.1.1 (embedded-runtime): CDDL-1.0 OR LGPL-2.1-only

- Weak/file-level copyleft packages (used in unmodified form; source available upstream):
  - cssparser@0.36.0 (crates.io): MPL-2.0
  - cssparser-macros@0.6.1 (crates.io): MPL-2.0
  - dtoa-short@0.3.5 (crates.io): MPL-2.0
  - Graphviz (embedded in Viz.js full.render.js)@2.40.1 (vendored): EPL-1.0
  - option-ext@0.2.0 (crates.io): MPL-2.0
  - selectors@0.36.1 (crates.io): MPL-2.0

生成物为归属清单,非法律意见。上架前人工环节(G3):license 兼容性终审、
`legal/MANIFEST.json` 中的包级文件/SPDX 正文映射仍需按上游来源逐项复核、各分发渠道政策核验。
