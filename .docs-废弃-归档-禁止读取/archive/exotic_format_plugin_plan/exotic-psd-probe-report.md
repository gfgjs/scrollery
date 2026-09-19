# PSD 技术探针报告（Part1 P0）

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 状态：首轮探针完成（合成样本 + 畸形输入）
> 日期：2026-06-25
> 探针程序：`crates/exotic-workers/psd-probe`（独立 crate，不属于主构建）
> 适用：v3 总纲 §10、Part1 §0.1、勘误卷 R12
> 结论效力：仅对下列**锁定版本**成立；升级任一依赖须重跑探针。

## 1. 锁定依赖（探针 Cargo.lock）

| crate | 版本 | 许可证 | 角色 |
|---|---|---|---|
| `psd` | `=0.3.5` | MIT/Apache-2.0 | 候选 PSD 解码器 |
| `image` | `0.25.10` | MIT/Apache-2.0 | 缩放 + WebP 编码（与主程序同系 0.25） |
| `image-webp` | `0.2.4` | — | WebP 编解码后端 |
| `moxcms` | `0.8.1` | — | image 色彩管理传递依赖 |
| `pxfm` | `0.1.29` | — | image 传递依赖 |
| `bytemuck` | `1.25.0` | — | image 传递依赖 |
| `byteorder-lite` | `0.1.0` | — | image 传递依赖 |
| `num-traits` | `0.2.19` | — | 传递依赖 |
| `thiserror` | `1.0.69` | — | psd 错误类型 |
| `quick-error` | `2.0.1` | — | psd 依赖 |

`psd` 仓库：https://github.com/chinedufn/psd 。许可证 MIT/Apache-2.0 满足商用前置（最终合规仍由 Part3 §5.5 SBOM/审核决定，本报告不作法律结论）。

## 2. 实测结果（本机执行）

执行环境：Windows 11 / x86_64-pc-windows-msvc / `cargo run --release`。

### 2.1 合成 RGB 8-bit raw composite（PASS）

| 样本 | 解码 | 后处理 | 校验项 |
|---|---|---|---|
| 256×192 RGB 8-bit raw | 0.09 ms | 0.16 ms（缩放+WebP） | 尺寸一致、RGBA 长度 = w·h·4、alpha=255、WebP RIFF/WEBP 魔数有效（386 B） |
| 1×1 RGB 8-bit raw | 0.00 ms | 0.00 ms | 退化尺寸不崩、WebP 有效（36 B） |

- merged image data（raw 压缩、3 通道平面排列）→ `Psd::rgba()` 输出 RGBA 正确，通道顺序未错位。
- 后处理链（`image::imageops::resize` Lanczos3 + `WebPEncoder::new_lossless`）与主程序 0.25 一致，产物 WebP 魔数合法。
- 注：256 长边 < 480 档位，未触发下采样（探针按长边吸附逻辑保持原尺寸）；下采样路径由更大真实样本在 §4 补测。

### 2.2 畸形输入健壮性（PASS — 全部「不 panic」）

| 输入 | 结果 |
|---|---|
| 空字节 | rejected，无 panic |
| 16 B 随机 | rejected，无 panic |
| 4 KiB 随机 | rejected，无 panic |
| 错 magic（`XXXX`） | rejected，无 panic |
| 截断 header（20 B） | rejected，无 panic |
| 截断 image data（尾部缺 50 B） | **decoded**，无 panic（见风险①） |
| 巨型尺寸字段（60000×60000，无像素） | **rejected**，无 panic（见亮点①） |
| version=2（PSB 标记） | **rejected**，无 panic（印证 R12） |

**亮点①**：声明 60000×60000 但无像素数据时被拒绝 → `psd 0.3.5` 在分配前校验，**不会**因 header 谎报尺寸 OOM。降低了「畸形尺寸 → 内存炸弹」风险（仍须 Host 侧像素上限兜底）。

**风险①**：截断 image data 时 `from_bytes` 仍「成功」、`rgba()` 不 panic——说明 psd **不严格校验像素段长度**，可能产出截断/填充的错误图像。→ 直接印证 v3 Part2 §3.7 的强制要求：Host 必须用独立 WebP parser 二次验证**解码后实际尺寸/像素数**，不能信任 Worker 声明。

## 3. 源码级能力核验（R12，交叉印证；非执行实测）

`psd 0.3.5` 源码（本机 `~/.cargo/registry/.../psd-0.3.5`）：

- **PSB 不支持**：`src/sections/file_header_section.rs` 只接受 version 1 → §2.2 version-2 被拒已实测印证。
- **16-bit**：`src/sections/image_data_section.rs` 声明 8/16-bit，但 16-bit raw 路径仅显式下转换首通道，其余通道行为须真实 16-bit 样本验证。
- **CMYK**：header 可识别 ColorMode::Cmyk，但 `src/psd_channel.rs` 按 RGBA 通道位置组装，**未见** CMYK→RGB 色彩转换 → 不能据「可解析 ColorMode」宣称颜色正确。
- **ZIP 压缩**：存在 unsupported/unimplemented 路径。
- **blend mode / 图层 flatten**：`src/lib.rs` 注明未完整处理 → 无 merged image 的 PSD 合成结果不可保证。

## 4. 未实测项（需真实授权样本）

本机无法合成的样本，**首发不得宣称支持**，留待真实授权样本实测：

- CMYK PSD（色彩正确性）
- 16-bit / 32-bit PSD（多通道）
- RLE / ZIP 压缩的 composite
- 无 merged image（纯图层）PSD 的合成
- PSB（大文档；源码已确认不支持，预期失败）
- 超大画布 / 大量图层的峰值内存与耗时

**扩测方式**：把授权样本放入某目录，设环境变量后重跑探针：

```bash
PSD_PROBE_SAMPLES=/path/to/authorized/psd cargo run --release \
  --manifest-path crates/exotic-workers/psd-probe/Cargo.toml
```

探针会逐样本打印 `dims / color_mode / depth / rgba 是否成功`，把通过项追加到本报告再扩展 Catalog/manifest。

## 5. 首发范围冻结决议

依据「实测通过才登记」（v3 §10 / Part1 §0.1）：

| 维度 | 首发登记 | 依据 |
|---|---|---|
| 格式 | `psd`（**不**含 `psb`） | version-2 实测被拒 + 源码确认 |
| 能力 | `thumbnail` | 仅缩略图切片（§3.2 非目标：不做编辑/图层/色管） |
| 像素类型 | RGB 8-bit + merged image（raw） | §2.1 实测通过 |
| CMYK / 16-bit / 无 merged / RLE / ZIP | **不宣称** | §4 未实测 |

→ 内置 Catalog 与 PSD manifest 的 `formats` 只写 `["psd"]`、`capabilities` 只写 `["thumbnail"]`。
→ Worker（Part2）解码后若遇 CMYK/16-bit/无 merged，应返回稳定错误码（`unsupported_variant`），不强行出图。

## 6. 复现

```bash
cd crates/exotic-workers/psd-probe
cargo run --release          # 合成 + 畸形；exit 0 = PASS
```
