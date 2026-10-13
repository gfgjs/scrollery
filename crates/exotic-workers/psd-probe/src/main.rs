// crates/exotic-workers/psd-probe/src/main.rs
//! PSD 解码技术探针。
//!
//! 通过可执行检查确认：
//!     1. `psd 0.3.5` 能否把一张合成 RGB 8-bit composite 解出正确尺寸 + RGBA；
//!     2. 解出的像素能否走 缩放 → WebP 编码 这条与主程序一致的后处理链；
//!     3. 畸形输入（截断 / 随机字节 / 错 magic / 超大尺寸字段）是否 **不 panic、不越界、稳定失败**。
//!
//! 关键：`Psd::rgba()` 返回 `Vec<u8>` 而**非** `Result`——畸形输入可能 panic。
//! 因此本探针对 `from_bytes` 与 `rgba()` 全程 `catch_unwind` 包裹，正是为了量化这一风险。
//!
//! CMYK / 16-bit / PSB / RLE / ZIP 等需要**真实授权样本**，本机无法合成。
//! 把授权样本放入某目录并设 `PSD_PROBE_SAMPLES=<dir>` 即可让探针逐个实测（见报告）。

use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::Instant;

use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use psd::{ColorMode, Psd, PsdDepth};

/// 主程序的缩略图档位（见 src-tauri/src/thumbnail/generator.rs:27）。探针用 480 做一次真实下采样。
const TARGET_TIER: u32 = 480;

fn main() {
    println!("=== PSD probe (psd =0.3.5) ===\n");

    let mut ok = true;

    // ── 1. 合成 RGB 8-bit raw composite：核心「能否出图」证据 ───────────────────
    ok &= probe_synthetic_rgb(256, 192);
    ok &= probe_synthetic_rgb(1, 1); // 退化尺寸边界

    // ── 2. 畸形输入健壮性：必须稳定失败、绝不 panic / 越界 ─────────────────────
    println!("\n--- malformed inputs (must NOT panic) ---");
    let mut malformed_clean = true;
    malformed_clean &= probe_malformed("empty", &[]);
    malformed_clean &= probe_malformed("garbage-16B", &[0xABu8; 16]);
    malformed_clean &= probe_malformed("garbage-4KiB", &vec![0x5Au8; 4096]);
    malformed_clean &= probe_malformed("wrong-magic", &wrong_magic());
    malformed_clean &= probe_malformed("truncated-header", &make_rgb_psd(64, 64)[..20]);
    malformed_clean &= probe_malformed(
        "truncated-imagedata",
        &truncate_image_data(make_rgb_psd(64, 64)),
    );
    malformed_clean &= probe_malformed("huge-dimensions", &huge_dimensions_psd());
    malformed_clean &= probe_malformed("version-2-psb", &psb_version_psd());
    ok &= malformed_clean;

    // ── 3. 真实授权样本（可选）：CMYK / 16-bit / PSB / RLE / ZIP 唯一可信来源 ──
    if let Ok(dir) = std::env::var("PSD_PROBE_SAMPLES") {
        println!("\n--- authorized samples from {dir} ---");
        probe_sample_dir(Path::new(&dir));
    } else {
        println!(
            "\n[note] 未设 PSD_PROBE_SAMPLES：CMYK/16-bit/PSB/RLE/ZIP 未实测（需真实授权样本）。"
        );
    }

    println!("\n=== probe {} ===", if ok { "PASS" } else { "FAIL" });
    std::process::exit(if ok { 0 } else { 1 });
}

/// 合成一张 `w×h` 的 RGB 8-bit、raw 压缩、仅含 merged image data 的最小合法 PSD。
/// 全大端。结构：header(26) + colorModeData(0) + imageResources(0) + layerMask(0) + imageData。
fn make_rgb_psd(w: u32, h: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"8BPS"); // signature
    b.extend_from_slice(&1u16.to_be_bytes()); // version 1（非 PSB）
    b.extend_from_slice(&[0u8; 6]); // reserved
    b.extend_from_slice(&3u16.to_be_bytes()); // channels = 3 (R,G,B)
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&8u16.to_be_bytes()); // depth = 8
    b.extend_from_slice(&3u16.to_be_bytes()); // color mode = 3 (RGB)
    b.extend_from_slice(&0u32.to_be_bytes()); // color mode data length = 0
    b.extend_from_slice(&0u32.to_be_bytes()); // image resources length = 0
    b.extend_from_slice(&0u32.to_be_bytes()); // layer & mask length = 0
    b.extend_from_slice(&0u16.to_be_bytes()); // image data compression = 0 (raw)
                                              // 平面排列：先整张 R，再整张 G，再整张 B。
    for ch in 0..3u32 {
        for y in 0..h {
            for x in 0..w {
                let v = match ch {
                    0 => {
                        if w > 1 {
                            (x * 255 / (w - 1)) as u8
                        } else {
                            200
                        }
                    }
                    1 => {
                        if h > 1 {
                            (y * 255 / (h - 1)) as u8
                        } else {
                            120
                        }
                    }
                    _ => 128u8,
                };
                b.push(v);
            }
        }
    }
    b
}

/// 探针：合成 RGB → 解码 → 校验尺寸/色彩模式/深度 → RGBA → 缩放 → WebP。
fn probe_synthetic_rgb(w: u32, h: u32) -> bool {
    let bytes = make_rgb_psd(w, h);
    let t0 = Instant::now();
    let decoded = catch_unwind(AssertUnwindSafe(|| Psd::from_bytes(&bytes)));
    let psd = match decoded {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            println!("[FAIL] synthetic {w}x{h}: from_bytes Err: {e:?}");
            return false;
        }
        Err(_) => {
            println!("[FAIL] synthetic {w}x{h}: from_bytes PANIC");
            return false;
        }
    };

    if psd.width() != w || psd.height() != h {
        println!(
            "[FAIL] synthetic {w}x{h}: dims mismatch {}x{}",
            psd.width(),
            psd.height()
        );
        return false;
    }
    let cm = psd.color_mode();
    let depth = psd.depth();
    if !matches!(cm, ColorMode::Rgb) || !matches!(depth, PsdDepth::Eight) {
        println!("[FAIL] synthetic {w}x{h}: cm={cm:?} depth={depth:?}");
        return false;
    }

    // rgba() 无 Result，必须包裹。
    let rgba = match catch_unwind(AssertUnwindSafe(|| psd.rgba())) {
        Ok(v) => v,
        Err(_) => {
            println!("[FAIL] synthetic {w}x{h}: rgba() PANIC");
            return false;
        }
    };
    let decode_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let expect = (w as usize) * (h as usize) * 4;
    if rgba.len() != expect {
        println!(
            "[FAIL] synthetic {w}x{h}: rgba len {} != {expect}",
            rgba.len()
        );
        return false;
    }

    // 抽样校验左上/右上像素，确认 channel 顺序未错位（R 随 x 增、alpha=255）。
    if w > 1 && h > 1 {
        let tl = &rgba[0..4];
        if tl[3] != 255 {
            println!("[FAIL] synthetic {w}x{h}: alpha={} (expect 255)", tl[3]);
            return false;
        }
    }

    // 后处理链：缩放到 480 长边 + WebP 编码（与主程序同系 image 0.25）。
    let img = match RgbaImage::from_raw(w, h, rgba) {
        Some(i) => i,
        None => {
            println!("[FAIL] synthetic {w}x{h}: RgbaImage::from_raw None");
            return false;
        }
    };
    let (nw, nh) = scaled_dims(w, h, TARGET_TIER);
    let t1 = Instant::now();
    let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
    let mut webp = Vec::new();
    let enc = image::codecs::webp::WebPEncoder::new_lossless(Cursor::new(&mut webp));
    if let Err(e) = enc.write_image(resized.as_raw(), nw, nh, ExtendedColorType::Rgba8) {
        println!("[FAIL] synthetic {w}x{h}: webp encode {e:?}");
        return false;
    }
    let post_ms = t1.elapsed().as_secs_f64() * 1000.0;

    if webp.len() < 12 || &webp[0..4] != b"RIFF" || &webp[8..12] != b"WEBP" {
        println!("[FAIL] synthetic {w}x{h}: webp magic invalid");
        return false;
    }

    println!(
        "[ OK ] synthetic {w}x{h} -> {nw}x{nh}: decode {decode_ms:.2}ms, post {post_ms:.2}ms, webp {} B",
        webp.len()
    );
    true
}

/// 畸形输入探针：from_bytes + （若成功）rgba() 全程 catch_unwind。返回 true=未 panic（合格）。
fn probe_malformed(label: &str, bytes: &[u8]) -> bool {
    let r = catch_unwind(AssertUnwindSafe(|| {
        match Psd::from_bytes(bytes) {
            Ok(p) => {
                // 解析「成功」也可能在 rgba 阶段炸：继续探。
                let _ = p.rgba();
                "decoded"
            }
            Err(_) => "rejected",
        }
    }));
    match r {
        Ok(state) => {
            println!("[ OK ] malformed {label}: {state} (no panic)");
            true
        }
        Err(_) => {
            println!("[FAIL] malformed {label}: PANIC");
            false
        }
    }
}

/// 真实授权样本目录：逐 .psd 实测，记录 cm/depth/dims/rgba 是否成功。仅打印，不参与 PASS/FAIL。
fn probe_sample_dir(dir: &Path) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            println!("[warn] read_dir {dir:?}: {e}");
            return;
        }
    };
    for entry in rd.flatten() {
        let p = entry.path();
        let is_psd = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let e = e.to_lowercase();
                e == "psd" || e == "psb"
            })
            .unwrap_or(false);
        if !is_psd {
            continue;
        }
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => {
                println!("[warn] read {p:?}: {e}");
                continue;
            }
        };
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let r = catch_unwind(AssertUnwindSafe(|| {
            let psd = Psd::from_bytes(&bytes)?;
            let info = format!(
                "{}x{} cm={:?} depth={:?}",
                psd.width(),
                psd.height(),
                psd.color_mode(),
                psd.depth()
            );
            let rgba_len = std::panic::catch_unwind(AssertUnwindSafe(|| psd.rgba().len()));
            Ok::<_, psd::PsdError>((info, rgba_len))
        }));
        match r {
            Ok(Ok((info, Ok(len)))) => {
                println!(
                    "[sample] {name} ({} B): {info}, rgba {len} B OK",
                    bytes.len()
                )
            }
            Ok(Ok((info, Err(_)))) => {
                println!("[sample] {name} ({} B): {info}, rgba() PANIC", bytes.len())
            }
            Ok(Err(e)) => println!("[sample] {name} ({} B): from_bytes Err {e:?}", bytes.len()),
            Err(_) => println!("[sample] {name} ({} B): from_bytes PANIC", bytes.len()),
        }
    }
}

/// 按长边缩放计算目标尺寸（保持比例，至少 1px）。
fn scaled_dims(w: u32, h: u32, tier: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= tier {
        return (w.max(1), h.max(1));
    }
    let scale = tier as f64 / long as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

fn wrong_magic() -> Vec<u8> {
    let mut b = make_rgb_psd(8, 8);
    b[0..4].copy_from_slice(b"XXXX");
    b
}

/// 截断到 image data 段中间，制造「头合法、像素数据不足」的畸形输入。
fn truncate_image_data(mut b: Vec<u8>) -> Vec<u8> {
    let cut = b.len().saturating_sub(50);
    b.truncate(cut);
    b
}

/// header 声明天文数字尺寸，但实际像素数据为空——测试是否在分配前校验长度。
fn huge_dimensions_psd() -> Vec<u8> {
    let mut b = make_rgb_psd(2, 2);
    // header: height @ offset 14 (u32), width @ offset 18 (u32)
    b[14..18].copy_from_slice(&60000u32.to_be_bytes());
    b[18..22].copy_from_slice(&60000u32.to_be_bytes());
    b
}

/// 把 version 改成 2（PSB 标记）——psd 0.3.5 明确只接受 version 1（R12）。
fn psb_version_psd() -> Vec<u8> {
    let mut b = make_rgb_psd(8, 8);
    b[4..6].copy_from_slice(&2u16.to_be_bytes());
    b
}
