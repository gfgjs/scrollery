// src-tauri/examples/video_bench.rs
//! 视频封面/关键帧提取微基准(2026-07-17 性能审查线施工对拍用,不进产品包)。
//!
//! 用法:
//!   cargo run --release --example video_bench -- <video>... [--rotation-check <video>]
//!
//! 测量口径(与 derive/video.rs 的真实调用等价):
//!   - probe:元数据探测一次(单独计时);
//!   - cover:probe(选封面时间戳)+ cover 解码,即 run_cover 的后端部分;
//!   - kf(10):keyframes(KEYFRAME_COUNT) 解码+缩格,即 run_keyframes 的后端部分。
//!
//! 不含 WebP 编码/雪碧图拼接/落盘 —— 那些环节本轮不改,排除以聚焦解码路径。
//! 每文件 3 轮:首轮报 cold(含文件缓存冷启),再报 best-of-3(暖缓存,供改前后对拍)。

use std::path::{Path, PathBuf};
use std::time::Instant;

use scrollery_lib::video::backend_for;

/// 与 derive/video.rs::KEYFRAME_COUNT 保持一致(基准独立可跑,不引 derive 模块)。
const KEYFRAME_COUNT: usize = 10;

/// 与 derive/video.rs::DEFAULT_SPRITE_CELL_H 保持一致(批次C:keyframes 签名新增格高参数)。
const SPRITE_CELL_H: u32 = 200;

/// 封面长边请求值:对应 THUMB_TIERS 的 512 档(真实调用为 snap_to_tier(用户设置))。
const COVER_TIER: u32 = 512;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut rotation_check: Option<PathBuf> = None;
    if let Some(pos) = args.iter().position(|a| a == "--rotation-check") {
        args.remove(pos);
        if pos < args.len() {
            rotation_check = Some(PathBuf::from(args.remove(pos)));
        }
    }
    if args.is_empty() && rotation_check.is_none() {
        eprintln!("usage: video_bench <video>... [--rotation-check <video>]");
        std::process::exit(2);
    }

    #[cfg(windows)]
    println!(
        "hw decode available: {}",
        scrollery_lib::video::d3d::hw_available()
    );

    for f in &args {
        bench_file(Path::new(f));
    }
    if let Some(p) = rotation_check {
        if !rotation_assert(&p) {
            std::process::exit(1);
        }
    }
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn bench_file(path: &Path) {
    let ext = ext_of(path);
    let Some(backend) = backend_for(&ext) else {
        eprintln!("SKIP {}: no backend | 无后端", path.display());
        return;
    };
    println!("== {} ==", path.display());

    let t0 = Instant::now();
    match backend.probe(path) {
        Ok(info) => println!(
            "  probe : {:.1}ms {}x{} dur={}ms rot={} fps={:.1} codec={:?}",
            t0.elapsed().as_secs_f64() * 1000.0,
            info.width,
            info.height,
            info.duration_ms,
            info.rotation,
            info.fps,
            info.codec
        ),
        Err(e) => {
            println!("  probe FAILED: {e}");
            return;
        }
    }

    let mut cover_ms: Vec<f64> = Vec::new();
    let mut kf_ms: Vec<f64> = Vec::new();
    for round in 0..3 {
        // cover 口径 = run_cover 等价。T1c 后时间戳选择内聚在 cover 的单一会话里,
        // 不再有独立 probe;长边按 512 tier 请求(基线版此处为 probe+cover(t) 两步,
        // 口径均为「产出一张封面的后端全部工作」,可对拍)。
        let t0 = Instant::now();
        match backend.cover(path, COVER_TIER) {
            Ok(img) => {
                let dt = t0.elapsed().as_secs_f64() * 1000.0;
                if round == 0 {
                    println!(
                        "  cover : {:.1}ms (cold) out={}x{}",
                        dt, img.width, img.height
                    );
                }
                cover_ms.push(dt);
            }
            Err(e) => println!("  cover FAILED: {e}"),
        }

        let t1 = Instant::now();
        match backend.keyframes(path, KEYFRAME_COUNT, SPRITE_CELL_H) {
            Ok(frames) => {
                let dt = t1.elapsed().as_secs_f64() * 1000.0;
                if round == 0 {
                    let (w, h) = frames
                        .first()
                        .map(|f| (f.width, f.height))
                        .unwrap_or((0, 0));
                    println!(
                        "  kf x{:<2}: {:.1}ms (cold) cell={}x{}",
                        frames.len(),
                        dt,
                        w,
                        h
                    );
                }
                kf_ms.push(dt);
            }
            Err(e) => println!("  keyframes FAILED: {e}"),
        }
    }

    let best = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    if !cover_ms.is_empty() {
        println!(
            "  cover  best-of-{}: {:.1}ms",
            cover_ms.len(),
            best(&cover_ms)
        );
    }
    if !kf_ms.is_empty() {
        println!("  kf(10) best-of-{}: {:.1}ms", kf_ms.len(), best(&kf_ms));
    }
}

/// rot90 样本契约:源为左红右蓝的横幅,容器带 rotation=90 元数据。
/// 正立后必须是竖幅(h>w)且上下两半颜色不同、各自饱和(红/蓝一上一下)。
/// 双旋转(XVP 已旋 + 我们再旋)会回到横幅 → dims 断言直接抓住。
/// 打印上/下半均值 RGB,供改前后人工比对上下颜色是否翻转(180° 型回归)。
fn rotation_assert(path: &Path) -> bool {
    let ext = ext_of(path);
    let Some(backend) = backend_for(&ext) else {
        eprintln!("ROTATION SKIP: no backend");
        return false;
    };
    let info = match backend.probe(path) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("ROTATION probe FAILED: {e}");
            return false;
        }
    };
    println!(
        "rotation-check probe: {}x{} rot={}",
        info.width, info.height, info.rotation
    );
    if info.rotation != 90 && info.rotation != 270 {
        eprintln!(
            "ROTATION FAIL: metadata rotation={} (expect 90/270)",
            info.rotation
        );
        return false;
    }
    let img = match backend.cover(path, COVER_TIER) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("ROTATION cover FAILED: {e}");
            return false;
        }
    };
    let avg = |y0: u32, y1: u32| -> (u64, u64, u64) {
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for y in y0..y1 {
            for x in 0..img.width {
                let i = ((y * img.width + x) * 4) as usize;
                r += img.pixels[i] as u64;
                g += img.pixels[i + 1] as u64;
                b += img.pixels[i + 2] as u64;
                n += 1;
            }
        }
        (r / n.max(1), g / n.max(1), b / n.max(1))
    };
    let top = avg(0, img.height / 3);
    let bottom = avg(img.height * 2 / 3, img.height);
    println!(
        "rotation-check upright {}x{} top_rgb={:?} bottom_rgb={:?}",
        img.width, img.height, top, bottom
    );
    if img.height <= img.width {
        eprintln!(
            "ROTATION FAIL: not portrait after upright ({}x{}) — 疑似双旋转/未旋转",
            img.width, img.height
        );
        return false;
    }
    // 一半红占优、另一半蓝占优(不锁定哪半在上,方向约定以基线为准人工比对)。
    let is_red = |c: (u64, u64, u64)| c.0 > 128 && c.2 < 100;
    let is_blue = |c: (u64, u64, u64)| c.2 > 128 && c.0 < 100;
    let ok = (is_red(top) && is_blue(bottom)) || (is_blue(top) && is_red(bottom));
    if ok {
        println!("ROTATION PASS");
    } else {
        eprintln!("ROTATION FAIL: halves not red/blue split — 旋转内容错误");
    }
    ok
}
