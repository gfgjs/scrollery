---
status: active
type: working-memory
line: RAW等格式照片支持
id: research-R2
created: 2026-07-24
---

# R2 调研:Rust LibRaw 绑定 Windows 可编性(2026-07-24)

> 范围:仅查"怎么编"。库选型(LibRaw CDDL 分支静态链接)已由 D-429 裁定,不翻案。
> 方法:crates.io API(`/api/v1/crates/<name>`)+ GitHub API(repo/issues/PR/raw 文件)交叉核对,截至 2026-07-24 查证。

## 1. 候选清单(crates.io 现状)

| crate | 最新版本 | 最后发布(crates.io) | GitHub 仓库最后活动 | 维护状态 |
|---|---|---|---|---|
| `libraw-sys` (dcuddeback) | 0.1.1 | 2015-12-16 | — | 已废弃(10 年无更新) |
| `libraw-rs` / `libraw-rs-sys` (paolobarbolini) | 0.0.4 / 0.0.4+libraw-0.20.1 | 2021-02-02(crates.io 停发) | pushed 2025-08-21,repo updated 2026-07-21(GitHub 仍有零星活动,但 5 年未重发布到 crates.io) | "Passively Maintained" 徽章,半死不活 |
| `rsraw` / `rsraw-sys` (Hexilee) | 0.1.1 | 2026-03-21(最新) | pushed 2026-03-21 | 活跃,但仅 22 commits/19 star,新项目 |
| `libraw_rs_vendor` (mgolub2) | 1.0.0 | 2023-08-08 | 7 commits,0 star/0 fork | 个人用途小工具,几乎无采用 |
| `rawkit` (GraphiteEditor) | 0.1.0 | 2024-11-03 | Graphite 编辑器子库 | 非 LibRaw 绑定(纯 Rust 重实现,仅支持 Sony `.arw`,无预览提取),不满足"LibRaw 绑定"前提,仅作背景记录 |
| `Kilerd/libraw-sys`(dcuddeback 的 fork) | 未见独立发布 | fork pushed 2023-05,updated 2023-07 | 停滞的 fork | 非独立候选,跳过 |

来源:
- https://crates.io/api/v1/crates/{libraw-sys,libraw-rs,libraw-rs-sys,rsraw,rsraw-sys,libraw_rs_vendor,rawkit}
- https://api.github.com/repos/{dcuddeback/libraw-sys,paolobarbolini/libraw-rs,hexilee/rsraw,mgolub2/libraw_rs_vendor,Kilerd/libraw-sys}
- https://github.com/dcuddeback/libraw-sys, https://github.com/paolobarbolini/libraw-rs, https://github.com/hexilee/rsraw

## 2. Vendored 源码 vs 系统 pkg-config

- **`libraw-sys` (dcuddeback)**:纯 pkg-config 路径。README/crates.io 描述明确要求"must have the libraw_r library installed where it can be found by pkg-config"(Debian `libraw-dev` / macOS Homebrew / FreeBSD 包)。**无 vendored 源码**,Windows 上无一键路径,需自行摆平 pkg-config(与 findings.md 已记的 Android NDK pkg-config 阻塞是同一类痛点)。
  来源:https://lib.rs/crates/libraw-sys ,https://github.com/dcuddeback/libraw-sys
- **`libraw-rs` / `libraw-rs-sys` (paolobarbolini)**:**vendored**。子 crate 目录内直接带 LibRaw 源码树(`libraw-sys/LibRaw/src/**`),`build.rs` 用 `cc` crate 编译约 80 个 `.cpp` 文件为静态库 `raw`,再用可选 `bindgen` feature 生成绑定;不依赖 pkg-config。
  来源:https://github.com/paolobarbolini/libraw-rs/blob/master/libraw-sys/build.rs ,https://github.com/paolobarbolini/libraw-rs/blob/master/libraw-sys/Cargo.toml
- **`rsraw` / `rsraw-sys` (Hexilee)**:**vendored**,同构:`rsraw-sys/LibRaw/` 内含完整 LibRaw 源码树(README.md/LICENSE.LGPL/LICENSE.CDDL/Makefile.msvc/Makefile.mingw 等标准官方发行物全在),`build.rs` 同样用 `cc` 编译 decoders/demosaic/metadata/postprocessing/... 各目录源文件为静态库,再 `bindgen` 生成绑定。不依赖 pkg-config。
  来源:https://github.com/hexilee/rsraw (repo tree) ,build.rs 内容摘要(见下节)
- **`libraw_rs_vendor`**:描述自称"Vendored libraw @version 0.21.1",但仓库信号极弱(0 star/0 fork/7 commit),未见任何 Windows 验证痕迹,风险未知。
  来源:https://github.com/mgolub2/libraw_rs_vendor

结论:两个"vendored"候选(`libraw-rs-sys` / `rsraw-sys`)才有"一键 cargo build 无需系统预装"的现实可能;dcuddeback 的 pkg-config 路线在 Windows 上基本走不通(手动 vcpkg/MSYS2 摆位置,非"cargo build 直出")。

## 3. Windows(MSVC)实际 build 报告

- **`rsraw-sys` — 硬阻断**:`build.rs` 显式检测编译器并 **panic**:
  ```rust
  if compiler.is_like_msvc() {
      panic!("MSVC is not supported");
  }
  ```
  即在默认 Rust Windows 工具链(`x86_64-pc-windows-msvc`,Tauri v2 Windows 官方要求的工具链)下,`cargo build` 会在 build script 阶段直接 panic 失败,不存在绕过空间(除非切到 `x86_64-pc-windows-gnu` 工具链,这对已用 MSVC 工具链的 Scrollery 项目是牵一发动全身的改动,不现实)。CI 矩阵也印证:`windows-latest` job 跑的是 `x86_64-pc-windows-gnu` target(装 MinGW),**没有 MSVC job**。
  来源:https://raw.githubusercontent.com/hexilee/rsraw/main/rsraw-sys/build.rs ,https://raw.githubusercontent.com/hexilee/rsraw/main/.github/workflows/core.yml

- **`libraw-rs-sys` (paolobarbolini) — 已知失败,有未合并的部分修复**:
  - Open Issue #1「Windows support」(2020-09-30 至今未关闭):作者明确"Currently compiling on Windows (at least on CI) fails",且自陈没有 Windows 环境调试,CI 里 Windows job 被注释掉(`# TODO: fix`)。
  - Draft/未合并 PR #15「build(windows): add some flags for windows build」:贡献者报告在本地 Windows(路径含 `D:\Dev`,推断为 MSVC 环境)加了额外编译 flag 后 **build 本身成功**,但测试套件在一处 `debug_assert!(!(*processor.inner).rawdata.raw_alloc.is_null())` 断言上失败;维护者确认这是在防真实的空指针解引用(非可随意注释掉的伪断言),PR 因此未合并。
  - 净结论:MSVC 下"能编出来"在个案上有报告(需要 PR #15 分支的额外 patch,且未发布到 crates.io,当前 0.0.4 发布版早于此 patch),但**没有干净、可复现、已合并/已发布的成功路径**;直接 `cargo add libraw-rs` 拉 crates.io 发布版大概率仍在 Windows 上失败或需要自行套用未合并 patch。
  来源:https://github.com/paolobarbolini/libraw-rs/issues/1 ,https://github.com/paolobarbolini/libraw-rs/pull/15 ,https://raw.githubusercontent.com/paolobarbolini/libraw-rs/master/.github/workflows/ci.yml

- **`libraw-sys` (dcuddeback,pkg-config 路线)**:无 vendoring,10 年无更新,未见任何 Windows build 报告——判定为死路,不再深挖。

- **`libraw_rs_vendor`**:未查到任何 Windows build 报告(issue/PR/CI 均空白信号),风险不可判定,视为未经验证的候选。

## 4. 提取嵌入预览的 API 面

- **`rsraw`(唯一有成型高层 API 证据的候选)**:
  ```rust
  // 最小调用序列
  let bytes = std::fs::read("IMG_0001.CR3")?;
  let raw_image = RawImage::open(&bytes)?;               // 对应 LibRaw open_buffer/open_file + unpack 内部封装
  let thumbs = raw_image.extract_thumbs()?;              // Vec<ThumbnailImage>,对应 unpack_thumb + dcraw_thumb_writer/内存导出
  for thumb in thumbs {
      // thumb.width / thumb.height / thumb.format(JPEG/Bitmap/Bitmap16/Layer/Rollei/H265) / thumb.data: Vec<u8>
      std::fs::write("thumb.jpg", &thumb.data)?;
  }
  ```
  格式识别覆盖 JPEG/Bitmap/Bitmap16/Layer/Rollei/H265,按尺寸自动排序,是目前唯一"开箱即用"的预览提取封装。
  来源:https://github.com/hexilee/rsraw (README 摘要,含以上示例)

- **`libraw-rs`(paolobarbolini)高层 crate**:docs.rs 显示 **0/18 items documented**,README 自称"early days, feel free to open a PR if a feature is missing"——**未见成型的 `unpack_thumb`/thumbnail 高层封装**。底层 `libraw-rs-sys` 是对 `libraw.h`(LibRaw 的 **C++ 类头**,非扁平 C API `libraw_c_api.h`)跑 `bindgen`,意味着调用方需要直接摸 C++ 方法名(`open_file`/`unpack_thumb`/`dcraw_thumb_writer`)做 unsafe FFI——可用但不构成开箱即用的 API,且 bindgen 对 C++ 成员函数的绑定天然比扁平 C API 更脆(依赖 name mangling/ABI,跨编译器版本更易碎)。
  来源:https://docs.rs/crate/libraw-rs/0.0.4 ,build.rs 摘要(bindgen 目标为 `LibRaw/libraw/libraw.h`)

- **`libraw-sys`(dcuddeback)**:纯裸 FFI,无任何预览封装,调用方需自行拼 C API 调用序列;且其绑定年代(2015)针对的是 `libraw_r`(reentrant 版)旧接口,与当前 LibRaw C API 是否严丝合缝未经核实。

## 5. CDDL 分支 vs 绑定 crate 默认拉取的许可

**未发现矛盾**——两个 vendored 候选内嵌的均是 LibRaw 官方标准双授权发行物,而非阉割掉 CDDL 选项的 LGPL-only 分叉:

- `rsraw-sys` 的 `LibRaw/` 目录内同时含 `LICENSE.LGPL` **和** `LICENSE.CDDL`,以及标准官方发行物才有的 `Makefile.msvc`/`Makefile.mingw`/`Changelog.txt` 等文件,判定为原样 vendoring 官方 LibRaw 源码树(双授权:LGPL2.1 **或** CDDL1.0,二选一)。
  来源:https://github.com/hexilee/rsraw (仓库树浏览,`rsraw-sys/LibRaw/` 目录列表)
- `libraw-rs-sys` 同理,`build.rs` 直接从 `libraw/src/**` 编译标准 LibRaw 源码目录结构(decoders/demosaic/metadata/...),未见任何许可裁剪迹象。
  来源:https://github.com/paolobarbolini/libraw-rs/blob/master/libraw-sys/build.rs

**推断**(未逐文件 diff 核实,仅基于 LibRaw 上游发行惯例):LibRaw 官方 GitHub 主仓(两 crate 均从此拉取)本身默认不含非自由的 demosaic 附加包(如 DCB/VCD/AHD 等以 `LibRaw-demosaic-pack-GPL2/3` 独立发布,不在核心仓库),所以只要两 crate vendoring 的是核心仓库源码(现有证据支持这一点),"CDDL 静态链接"在这两个候选上都能落地——**这是许可证选择(打包时你选 CDDL 条款履约),不是要切换到另一份源码/分支**。若要 100% 排除风险,仍建议在实际拉取的 commit 上做一次 `LICENSE.CDDL` 文件存在性 + `grep -r demosaic-pack` 的一次性确认(未在本次调研范围内做逐文件比对)。

## 结论(供 R2 施工定案参考)

本机 Windows(MSVC 工具链)下**没有一个候选能保证"直接 `cargo build` 一次过"**:`rsraw` 因 build.rs 显式拒绝 MSVC 而**确定失败**;`libraw-rs` 官方发布版**已知在 Windows CI 失败**(issue #1 未关),仅有未合并的 PR #15 分支报告"编译成功但测试断言失败";`libraw-sys`(dcuddeback)非 vendored 且已废弃,直接排除;`libraw_rs_vendor` 无可验证的 Windows 证据。

**相对最可行**的路径是 `libraw-rs` / `libraw-rs-sys`(paolobarbolini):它是唯一有"本地 MSVC 环境 build 成功"实证(PR #15)的候选,阻断点(测试断言,非编译本身)相对可控且已被维护者定位;代价是需要 fork/patch(基于 PR #15 分支)而非直接吃 crates.io 发布版,且高层 thumb 提取 API 需自行在 `libraw-rs-sys` 裸 FFI 上补一层(该 crate 的高层封装本身是空的)。`rsraw` 虽然预览提取 API 现成好用、且是许可证/vendoring 卫生最干净的一个,但 MSVC 硬 panic 是当前版本的绝对阻断,除非先给它提交上游 patch 摘除 `panic!("MSVC is not supported")` 并验证实际编译结果——这本身就是一项不确定的施工前置工作。

## 低置信度结论

- "PR #15 在 MSVC 下 build 成功"仅来自 GitHub 网页摘要转述,未直接读取该 PR 的 CI 日志原始输出逐行核实,存在转述偏差风险。
- "两个 vendored 候选源码未阉割 CDDL 选项"基于目录内 LICENSE 文件存在性推断,未逐文件 diff 官方 LibRaw 仓库确认无夹带非自由 demosaic 附加包。
- `libraw_rs_vendor` 因社区信号（0 star/0 fork）过弱,其"是否真的能在 Windows 一键编译"缺乏任何独立佐证,评级仅为"未经验证"而非"已知失败",不能排除它其实可行。
- `rsraw`/`rsraw-sys` 项目本身仅 3 个多月历史（首个版本晚于 2026-03）,长期可持续性未知,本次仅评估其"当前可编性"技术面,未评估其维护寿命风险。
