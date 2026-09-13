// src-tauri/examples/edit_memory_probe.rs
//! 图片编辑内存基准探针(方案 C §5 P0 内存门槛用,2026-07-19 施工对拍,不进产品包)。
//!
//! 单次进程只测一个 (format, width, height, mode) 组合——Windows `PeakWorkingSetSize`
//! 是进程生命周期内单调不减的历史峰值,同进程连测多组会让小尺寸读到大尺寸的残留峰值,
//! 因此调用方需要每组各起一个新进程(见 findings.md 的调用脚本)。
//!
//! **进程内合成源的伪影(F-003 第二层)**:即便每组独立进程,setup 阶段在测量进程内合成
//! 源图(RgbImage ≈3 B/px + encoder 缓冲)也会把同量级页面提前计入 peak_before,measure
//! 阶段 allocator 复用这些已提交页,delta 被系统性压低约一个像素缓冲。诚实测法是两段进程:
//! 先 `--emit-source` 生成源文件,另起进程用 `--source-path` 测量(setup 只读文件字节)。
//!
//! 用法:
//!   生成源:cargo run --release --example edit_memory_probe -- \
//!       --format jpeg|png|webp --width W --height H --emit-source PATH
//!   测量:  cargo run --release --example edit_memory_probe -- \
//!       --format jpeg --width W --height H --mode decode|full [--source-path PATH]
//!   (省略 --source-path 时退回旧的进程内合成,仅用于对拍伪影本身。)
//!
//! mode=decode:只解码(不 apply_orientation,合成源没有真实 EXIF 方向)。
//! mode=full:decode → rotate90 → fliph → flipv → crop(居中裁一半)→ encode(JPEG q92),
//!   对应方案 §5「operation 组合最坏峰值」。
//! mode=full_v2(2026-07-20,阶段 7 D-008 复跑,v2 设计 §3 测法升级):直接调用
//!   `scrollery_lib::editing::{geometry,adjust}` 生产函数(非重实现)回放 D-107 全链最坏组合——
//!   decode → apply_geometry(90° 合并旋转 + 双向 flip + 45° fine rotate 展开+内接裁,
//!   crop:None 保留展开后最大像素量)→ adjust(三滑杆非零,ICC 传入 srgb_profile_bytes()
//!   强制走 moxcms 分条 CMS 有标签路径而非 untagged 快路径)→ encode(JPEG q92)。
//!
//! 输出一行 JSON 到 stdout,字段:format/width/height/mode/setup_ms/op_ms/
//! peak_ws_before_bytes/peak_ws_after_bytes/delta_bytes。

use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageReader, RgbImage};
use std::io::Cursor;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = arg_value(&args, "--format").unwrap_or_else(|| "jpeg".to_string());
    let width: u32 = arg_value(&args, "--width")
        .and_then(|v| v.parse().ok())
        .unwrap_or(6000);
    let height: u32 = arg_value(&args, "--height")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4000);
    let mode = arg_value(&args, "--mode").unwrap_or_else(|| "full".to_string());
    // full_v2 专用:fine rotate 角度可覆盖(默认 45°)。100MP 方图在 45° 会被生产
    // memory_budget 准入门在解码前拒绝(展开后 ≈200MP > 125MP 上限),需要能测「实际可
    // 达」的小角度场景,而不是只测「若跳过准入门会怎样」的假设值。
    let fine_angle: f64 = arg_value(&args, "--fine-angle")
        .and_then(|v| v.parse().ok())
        .unwrap_or(45.0);

    // 生成模式:只落盘源文件后退出,供另一个测量进程用 --source-path 读取。
    if let Some(path) = arg_value(&args, "--emit-source") {
        let bytes = encode_synthetic(&format, width, height);
        std::fs::write(&path, &bytes).expect("write source file");
        println!(
            "{{\"emitted\":\"{}\",\"bytes\":{}}}",
            path.replace('\\', "/"),
            bytes.len()
        );
        return;
    }

    // Phase 0:取得源字节。--source-path 只读文件(诚实模式,setup 峰值仅压缩字节量级);
    // 否则进程内合成(遗留模式,delta 被 setup 预提交页面压低,见模块文档)。
    let setup_start = Instant::now();
    let source_bytes = match arg_value(&args, "--source-path") {
        Some(path) => std::fs::read(&path).expect("read source file"),
        None => encode_synthetic(&format, width, height),
    };
    let setup_ms = setup_start.elapsed().as_millis();

    let peak_before = peak_working_set_bytes();
    let op_start = Instant::now();
    let _output_len = match mode.as_str() {
        "decode" => run_decode(&source_bytes).0,
        "full" => run_full_pipeline(&source_bytes),
        "full_v2" => run_full_pipeline_v2(&source_bytes, fine_angle),
        other => {
            eprintln!("unknown --mode {other}, expected decode|full|full_v2");
            std::process::exit(2);
        }
    };
    let op_ms = op_start.elapsed().as_millis();
    let peak_after = peak_working_set_bytes();

    println!(
        "{{\"format\":\"{format}\",\"width\":{width},\"height\":{height},\"mode\":\"{mode}\",\
         \"fine_angle\":{fine_angle},\"setup_ms\":{setup_ms},\"op_ms\":{op_ms},\
         \"peak_ws_before_bytes\":{peak_before},\"peak_ws_after_bytes\":{peak_after},\
         \"delta_bytes\":{delta}}}",
        delta = peak_after.saturating_sub(peak_before),
    );
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// 合成一张 `width x height` 图并编码为目标格式的字节流。像素值由坐标派生,足够触发
/// 真实的 decode/encode 缓冲区分配,不需要真实照片。
fn encode_synthetic(format: &str, width: u32, height: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
    }));
    let mut out = Vec::new();
    match format {
        "jpeg" => {
            let enc = JpegEncoder::new_with_quality(&mut out, 92);
            img.write_with_encoder(enc).expect("encode synthetic jpeg");
        }
        "png" => {
            let enc = PngEncoder::new(&mut out);
            img.write_with_encoder(enc).expect("encode synthetic png");
        }
        "webp" => {
            // 合成源用 image 自带无损 WebP encoder 即可(只是构造输入,不涉及本任务
            // 已否决的有损 webp crate 元数据问题)。
            let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            img.write_with_encoder(enc).expect("encode synthetic webp");
        }
        other => {
            eprintln!("unknown --format {other}, expected jpeg|png|webp");
            std::process::exit(2);
        }
    }
    out
}

fn run_decode(bytes: &[u8]) -> (usize, DynamicImage) {
    // 借用而非 to_vec:避免把一份压缩源字节的复制算进被测 delta。
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .expect("guess format");
    let img = reader.decode().expect("decode");
    let len = (img.width() * img.height()) as usize;
    (len, img)
}

/// decode → rotate90 → fliph → flipv → crop(居中裁一半宽高)→ encode(JPEG q92)。
/// 对应方案 §5「operation 组合最坏峰值」:解码缓冲、变换结果与编码缓冲同时段共存。
fn run_full_pipeline(bytes: &[u8]) -> usize {
    let (_, mut img) = run_decode(bytes);
    img = img.rotate90();
    img = img.fliph();
    img = img.flipv();
    let (w, h) = (img.width(), img.height());
    let (cw, ch) = (w / 2, h / 2);
    let img = img.crop_imm(w / 4, h / 4, cw.max(1), ch.max(1));

    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, 92);
    img.write_with_encoder(enc).expect("encode output jpeg");
    out.len()
}

/// v2 D-008 最坏组合:直接调用生产 `geometry::apply_geometry` + `adjust::apply_adjust`,
/// 而非重实现变换——测的是真实产品代码路径的峰值,不是探针自己对算法的近似。
fn run_full_pipeline_v2(bytes: &[u8], fine_angle: f64) -> usize {
    use scrollery_lib::editing::{adjust, geometry};

    let (_, img) = run_decode(bytes);
    let ops = geometry::EditOps {
        rotate: 90,
        flip_h: true,
        flip_v: true,
        crop: None, // 展开+内接裁后不再二次裁剪,保留链上最大像素量(最坏情形)
        rotate_fine: if fine_angle == 0.0 {
            None
        } else {
            Some(fine_angle)
        },
        adjust: None, // adjust 走命令层同款的独立第二阶段,不塞进 EditOps
    };
    let out_img =
        geometry::apply_geometry(img, image::metadata::Orientation::NoTransforms, 0, &ops)
            .expect("apply_geometry");

    let adjust_ops = adjust::AdjustOps {
        brightness: 20,
        contrast: 15,
        saturation: -10,
    };
    // 传入 sRGB profile 字节强制走 moxcms 分条 CMS 有标签路径(而非 untagged 快路径),
    // 与设计 §4.4「有 adjust 即经 CMS」的最坏成本一致(sRGB→sRGB 变换本身是恒等,只测内存形状)。
    let icc = adjust::srgb_profile_bytes().expect("srgb icc bytes");
    let adjusted = adjust::apply_adjust(out_img, Some(&icc), &adjust_ops).expect("apply_adjust");

    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, 92);
    adjusted
        .write_with_encoder(enc)
        .expect("encode output jpeg v2");
    out.len()
}

#[cfg(windows)]
fn peak_working_set_bytes() -> u64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    if ok.is_ok() {
        counters.PeakWorkingSetSize as u64
    } else {
        0
    }
}

#[cfg(not(windows))]
fn peak_working_set_bytes() -> u64 {
    // 本探针只在 Windows 开发机上跑(方案 C 施工现场就是 Windows);其他平台的内存基准
    // 待 mac 线补测,与既往「缩略图流水线深审」的平台分工一致。
    0
}
