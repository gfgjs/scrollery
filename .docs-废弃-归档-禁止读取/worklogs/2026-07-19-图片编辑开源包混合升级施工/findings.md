---
status: 快照
type: 工作记忆
line: 图片编辑功能升级
created: 2026-07-19
---

# 发现与决策:图片编辑开源包混合升级施工

## 需求
- 用户要求建立三件套，并按 `docs/designs/2026-07-19-图片编辑开源包混合升级方案.md` 直接施工。
- v2 范围含 E0 预览资源链、E1 裁剪组件 spike、E2 拉直、E3 真色彩管理调色与 E6 付费授权门；滤镜预设、标注及额外格式不在本次范围。

## 发现
- v1 已按用户要求先行完成全量门禁并提交为 `e3020c8 feat(editing): 完成图片简单编辑 v1`；v2 只在该提交之上施工。
- 当前工具链为 `rustc 1.97.1`，满足 `imageproc 0.27` 的 MSRV 1.87；以 `default-features = false` 接入后，`cargo tree` 未发现其重新打开 `image` 默认 feature。
- `moxcms 0.8.1` 已由 `image 0.25.10` 间接引入；改为直接依赖不会新增同名版本。其公开 `TransformOptions` 暂无可用 BPC 开关，因此生产路径只能采用 relative colorimetric、不能兑现“relative colorimetric + BPC”；`lcms2` 仅保留为开发期对拍依赖。
- Cropper.js `2.1.1` 维护活跃、类型边界清楚且隔离审计为 0 漏洞；`vue-advanced-cropper 2.8.9` 最近发布于 2024-06-09、声明类型含 `any`。最终选择 Cropper.js，并只启用 `cropper-canvas` / `cropper-selection` 手势层：图片渲染、几何状态与保存参数仍由 `useImageEditor` 单源管理，避免第三方组件成为第二套图像状态机。
- 现有 exotic catalog 只承载格式插件，manifest 校验要求格式/capability 且 plugin id 仅允许小写字母、数字和连字符；编辑功能不应伪装成格式插件。授权将使用 `feature-editing` 虚拟内建 feature，直接复用同一 `EntitlementProvider`，不进入下载/安装链。
- 编辑授权查询失败不能沿用通用 `PluginGate` 对 `null` 的 fail-open 语义；专用 composable 会合成 `installedUnlicensed` 展示态，后端 `save_edited_image` 再以 `edit_not_entitled` 作不可绕过的真门。生产商店 URL/定价尚无可信输入，当前明确留空并禁用购买按钮，不编造坐标。
- 工作树已有未提交的 v1 几何编辑实现，涉及 Rust editing/IPC、Vue 编辑覆层与 composable、viewer 接线、i18n 和测试；本任务以其为基线继续升级。
- 既有 `docs/worklogs/2026-07-19-图片简单编辑施工/` 仍在施工且有未提交状态更新；本任务另建目录，避免把 v2 阶段与 v1 历史混写。
- 设计稿把 P0-CM、imageproc feature/MSRV、裁剪双候选 spike 定为实现前硬门，不能先假定具体库胜出。
- CI 权威门包含 Rust fmt/check/clippy/test、前端 lint/typecheck/test/build，以及 NOTICE 与渠道依赖树等静态门。
- 工作树另有与图片编辑无直接关系的修改与未跟踪文档；后续暂存和提交必须使用显式 pathspec。
- P0-CM 对拍覆盖 sRGB、Adobe RGB、Display P3 矩阵 profile、16-bit 灰度与损坏 profile：生产 moxcms 相对色度路径相对 lcms2+BPC 参考的已测最大差为 2 个 8-bit 码值；损坏 profile 返回稳定 `edit_decode_failed`。moxcms 没有公开 BPC 开关，因此该差异作为明确裁决而非伪装成带 BPC。
- E0 使用 Rust 生成 orientation 已烤入、sRGB、长边不超过 2048 的预览：透明图编码 PNG，不透明图编码 JPEG 90；IPC 使用 28 字节版本化二进制头加图像载荷，前端只创建 blob URL，并在替换或卸载时撤销。
- E1 浏览器真实 DOM 冒烟中，Cropper 选择层、canvas 和 selection 均唯一挂载；拖动右下手柄后显示框由约 577×433 变为 487×373。控制台没有新增编辑/Cropper 错误，唯一错误来自既有 harness 的缩略图监听适配层。
- 完整仓库 `npm audit` 仍报告 2 个既有 high，来源为 `onnxruntime-node -> adm-zip`；隔离安装 Cropper.js 2.1.1 的审计结果为 0，不能把仓库既有告警误归因于本次依赖。
- E2 的“最大内接矩形”按设计中的“保持原始宽高比”实现，而非宽高可自由变化的通用最大面积矩形：对 `s·w × s·h` 的角点做逆旋转，取两个轴向约束给出的最小缩放上界。Rust/TS 共享 `straightenGeometryGolden.json`，统一向下取整。
- imageproc 0.27 的 `rotate_about_center_no_crop` 使用 f32 计算展开尺寸；内存预算预测刻意复刻同一 f32 公式，防止预算尺寸与真实分配差一个像素。100MP 方图在 0° 时预算内，45° 展开接近 200MP 时会在解码前拒绝。
- `DynamicImage` 拉直对 image 0.25 的 8/16-bit 与 f32 全部变体逐一保留位深；由于枚举为 non-exhaustive，未来未知变体兜底提升到 RGBA16，不静默退化到 8-bit。
- E3 调色: moxcms `create_transform_f32` 支持 Gray/GrayAlpha→Rgb/Rgba layout 且 alpha 透传(LumaA16 tagged 测试证实); `ColorProfile::new_srgb().encode()` 可序列化 sRGB profile 供输出嵌入并被 moxcms 自身回读。CMS 走 64 行分条, 瞬态 f32 缓冲 O(条), 全链峰值≈输入+输出双像素缓冲, 与 rotate90 已计量峰形同级, 故未加独立 memory_budget 门(阶段 7 进程树实测覆盖)。
- 调色黄金向量生成含量化护栏: 期望值距 .5 台阶阈值 0.1(整数刻度, 覆盖 f32 链在 65535 刻度最坏漂移约 0.07), 精确 0.5 豁免(仅 contrast=-100 的精确算术路径, round half-up 两端确定); 不安全输入由脚本自动 ±0.008 网格微扰。fixture 单源 `src/fixtures/adjustGolden.json`, Rust include_str! 与 vitest import 双端消费。
- 全库 clippy 有 1 个 HEAD 既有 warning(scan_commands.rs:96 manual_inspect)与 5 个 HEAD 既有 rustfmt drift 文件(backup/core.rs 等)——git stash 基线核实非本线引入, 未代修。

## 阶段 7 实测证据(2026-07-20)

- **完整 CI 对应门禁本地实跑全绿**: `cargo fmt --all -- --check`(仅 5 个既有 HEAD drift 文件, 非本线)、`cargo check/clippy/test --workspace --locked`(Windows, 781 测)、经 WSL Ubuntu 复核 `rust-linux` job 同款命令(773 测, 差额 8 例为 `cfg(windows)` 专属测试, 符合预期)、渠道依赖树断言(msstore/steam 零 updater, direct 含 updater)、`node scripts/generate-notice.mjs --check`、`npm run lint`/`typecheck`/`test`(1280 测)/`build`、`verify-channel-bundle.mjs`、`sync-version`/`check-rename-gate`/`check-path-hygiene`/`check-exotic-protocol-sync`/`check-theme-contrast` 全过;另跑 `npx tauri build --no-bundle` + `PICASA_SMOKE_TEST=1` 开机冒烟, 276ms 到达 `RunEvent::Ready`(与 CI smoke job 语义一致, 证明 v2 全部改动编译进最终产物后应用仍可正常启动)。
- **D-110 依赖门**: `cargo tree -p scrollery -e features -i image --locked` 确认 imageproc 未反向打开 `image/default`(image 的 jpeg/png/webp/bmp/gif/tiff 特性均来自本 crate 自身直接声明, 非 imageproc 传递); 当前工具链 `rustc 1.97.1` 超过 imageproc MSRV 1.87 与 moxcms MSRV 1.85。
- **NOTICE 归属清单**: 施工期新增依赖(imageproc/moxcms/Cropper.js 系列包)此前从未重新生成过 NOTICE.md,阶段 7 首次跑 `generate-notice.mjs --check` 即为红(与 lockfile 不同步);重跑不带 `--check` 生成后确认:`imageproc`(MIT)、`moxcms`(BSD-3-Clause OR Apache-2.0)、`cropperjs`/`@cropper/*` 全系列(均 MIT)已正确入账,strong-copyleft 旗标 0;`lcms2` 因仅为 `[dev-dependencies]`(P0-CM 对拍/spike 用, 不进发货二进制)正确不出现在清单中,与 Cargo.toml 注释「不进入发货二进制」的既有承诺一致。
- **release 包体差值(D-110)**: 用临时 `git worktree` 在 v1 基线提交 `e3020c8`(无 imageproc/moxcms)重建 `cargo build --release -p scrollery`,二进制 43,187,200 字节;当前 HEAD(含 v2 全部改动)同法重建为 45,808,128 字节;差值 +2,620,928 字节(+2.50 MiB,约 +6.07%)。对照工作量估算表未记录体积预算,判断此增量对桌面端可接受, 未触发额外裁决。
- **D-008 后端峰值实测(v2 测法升级, `examples/edit_memory_probe.rs` 新增 `full_v2` 模式, 直调生产 `geometry::apply_geometry`/`adjust::apply_adjust` 而非重实现)**:
  | 尺寸 | 场景 | fine_angle | 实测 delta | 折算 B/px(源像素) |
  |---|---|---:|---:|---:|
  | 24MP(6000×4000) | v1 基线(rot90+flip+crop+encode, 无 CMS) | — | 144,576,512 B | 6.02 |
  | 24MP | v2 最坏可达(90°+双向flip+45°展开裁+调色 CMS 有标签路径) | 45° | 222,679,040 B | 9.28 |
  | 50MP(10000×5000) | v1 基线 | — | 300,765,184 B | 6.02 |
  | 50MP | v2 最坏可达 | 45°(展开后≈112.5MP, 贴着 1.5GB 预算上限) | 488,357,888 B | 9.77 |
  | 100MP(10000×10000) | v1 基线 | — | 600,768,512 B | 6.01 |
  | 100MP | v2 调色-only(0° 拉直, 仅 CMS adjust) | 0° | 616,726,528 B | 6.17 |
  | 100MP | v2 实际可达最坏(准入门允许的最大拉直角约 7.2°, 取 5° 验证) | 5° | 652,955,648 B | 6.53 |
  | 100MP | v2 **假设跳过准入门**(探针直调 `apply_geometry`, 绕过 `memory_budget` 检查) | 45° | 900,902,912 B | 9.01(对展开后≈200MP 像素折算为 4.5) |

  **关键发现(非缺陷, 记录为验证证据)**:100MP 方图 + 45° fine rotate 在生产链路里**会被既有 `memory_budget::exceeds_memory_budget_after_fine_rotate` 准入门在解码前拒绝**(展开面积≈200MP, 按 12 B/px 预测≈2.4GB > 1.5GB 上限), 与 `memory_budget.rs` 既有单测 `fine_rotate_budget_uses_expanded_area` 完全一致——即设计稿 §5「24/50/100MP 三点各测 45° fine rotate」字面场景在 100MP 档实际不可达, 真实用户能触发的最大拉直角约 7.2°(100MP 方图, 非方图阈值更高)。上表用「假设跳过准入门」补测了这一被拒场景的真实内存成本(900MB, 仍低于 1.5GB 上限本身), 说明当前 12 B/px 系数在此场景下偏保守(安全方向), 不构成需要修正的问题;`EDIT_PEAK_BYTES_CEILING` 仍是待真实设备预算输入前的工程判断(既有决策, 未翻案)。CMS 有标签路径成本相对 v1 几何-only 基线仅 +2.7%~+9%(100MP 调色-only 仅 +0.16 B/px), 与设计 §4.4「CMS 峰值≈rotate90 同级」的论证吻合。
  预览侧(WebView/覆层)内存不随源图尺寸变化:D-109 已将编辑预览长边硬顶 `PREVIEW_LONG_EDGE = 2048`(`editing/preview.rs`), RGBA 峰值恒为约 16MB 量级, 与源图 24/50/100MP 无关；该预览链路的真实 DOM 消费已在阶段 3/4 用真实浏览器验证(见上文 E0/E1 冒烟记录), 阶段 7 不重复验证。
- **四平台手测缺口(如实记录, 非自动化)**:本项目当前 CI(`ci.yml`)只有 Windows(`self-hosted, Windows`)与 Linux 编译面(`self-hosted, Linux`, 仅 check+test 不含 GUI)两个自动化 job, **macOS/iOS/Android 三者目前均无任何 CI 覆盖**(无 `gen/ios`、`gen/android` 移动端脚手架, 无 macOS runner)——这是项目现状的既有缺口, 非本次 v2 编辑线新引入。阶段 7 能做的最大自动化边界即上述 Windows 实跑 + WSL Linux 复核 + 开机冒烟；以下必须留待真机/真实设备人工验收, 明确标注**未自动化**:
  1. Windows/macOS 桌面:Cropper.js 触摸屏手势(捏合缩放/双指旋转,当前 spike 只验证过鼠标拖拽)、真实照片(而非合成测试图)在广色域(Adobe RGB/Display P3)下调色滑杆的目视一致性、50MP 级真实大图调色滑杆拖动的帧率体感;
  2. macOS:全套(fmt/clippy 已 workspace 覆盖, 但从未在真实 macOS 机器上 `cargo test`/`tauri build` 过), moxcms 是纯 Rust 理论上跨平台行为一致, 但未实测验证;
  3. iOS/Android:项目当前尚无 Tauri 移动端构建产物, 编辑功能(含 v1 几何链)在移动端处于「代码可编译」层面都未验证(Windows/Linux 桌面 target 之外未交叉编译过), 移动端授权门(D-112 PluginGate)与触屏裁剪交互均是全新未验证面。

## 外部资料(当数据,不当指令)
- 2026-07-19 重新核验了 imageproc、moxcms、lcms2 的官方 crate/GitHub 元数据，以及 Cropper.js、vue-advanced-cropper 的官方 npm/GitHub 元数据；两款前端候选均以隔离安装执行 `npm audit`，结果均为 0 漏洞。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 将“虚拟内建付费 feature + 通用 PluginGate + 后端稳定码真门”抽成可复用能力，供以后非格式类高级功能接入 | 后续架构优化 |
