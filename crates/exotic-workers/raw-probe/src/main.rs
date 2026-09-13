// crates/exotic-workers/raw-probe/src/main.rs
//! RAW 技术去险探针（阶段 B 前置 spike）。
//!
//! 目标：在裁定 RAW 支持范围之前，用**可执行的实测**回答：
//!   1. `rsraw =0.1.1`（vendored LibRaw C++，经 x86_64-pc-windows-gnu 工具链）能否真编出来；
//!   2. `RawImage::open` / `extract_thumbs` API 面是否可调用（证 API 形状可编）；
//!   3. 畸形输入（空 / 随机字节 / 截断 / 错 magic）是否 **不 panic、稳定失败**。
//!
//! 仓内无真实 RAW 样张（无法像 PSD 那样手写合成一张最小合法 RAW——RAW 格式无统一
//! 简单容器）。故本探针的「合法输入」部分只证 API 面能编译调用；健壮性验证落在
//! 畸形输入的 catch_unwind 上。真样张后续走 `RAW_PROBE_SAMPLES` 环境目录。
//!
//! `RawImage::open` 返回 `Result`（非裸 panic 型 API），但底层是 vendored C++，
//! 无法排除 FFI 边界内部真实 crash/panic，故仍需 catch_unwind 兜底。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use rsraw::RawImage;

fn main() {
    println!("=== RAW probe (rsraw =0.1.1, target x86_64-pc-windows-gnu) ===\n");

    let mut ok = true;

    // ── 1. API 面可编译调用性证据：open() 对畸形输入应 Err 而非 panic ──────────
    println!("--- malformed inputs (must NOT panic) ---");
    ok &= probe_malformed("empty", &[]);
    ok &= probe_malformed("garbage-16B", &[0xABu8; 16]);
    ok &= probe_malformed("garbage-4KiB", &vec![0x5Au8; 4096]);
    ok &= probe_malformed("wrong-magic-tiff-like", &wrong_magic_tiff_like());
    ok &= probe_malformed("truncated-tiff-header", &truncated_tiff_header());

    // ── 2. 真实授权样本（可选）：唯一能验证 extract_thumbs() 真实解码路径的来源 ──
    if let Ok(dir) = std::env::var("RAW_PROBE_SAMPLES") {
        println!("\n--- authorized samples from {dir} ---");
        probe_sample_dir(Path::new(&dir));
    } else {
        println!(
            "\n[note] 未设 RAW_PROBE_SAMPLES：extract_thumbs() 真实解码路径未实测（需真实授权 RAW 样本）。"
        );
    }

    println!("\n=== probe {} ===", if ok { "PASS" } else { "FAIL" });
    std::process::exit(if ok { 0 } else { 1 });
}

/// 畸形输入探针：open() 全程 catch_unwind。返回 true=未 panic（合格，无论 Ok/Err）。
fn probe_malformed(label: &str, bytes: &[u8]) -> bool {
    let r = catch_unwind(AssertUnwindSafe(|| match RawImage::open(bytes) {
        Ok(mut img) => {
            // open 成功也可能在 extract_thumbs 阶段炸：继续探（虽然对畸形输入几乎不可能 open 成功）。
            let _ = img.extract_thumbs();
            "opened"
        }
        Err(_) => "rejected",
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

/// 真实授权样本目录：逐文件实测 open + extract_thumbs，仅打印，不参与 PASS/FAIL。
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
        if !p.is_file() {
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
        // rsraw::Error 未从 crate 公开导出（私有 err 模块），故不能在探针里具名标注该类型，
        // 只能用 match 而非 `?` 串联，改用 Result<_, String> 把 Debug 信息转字符串携带出来。
        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut img = match RawImage::open(&bytes) {
                Ok(img) => img,
                Err(e) => return Err(format!("open Err {e:?}")),
            };
            let dims = format!(
                "{}x{} colors={} make={} model={}",
                img.width(),
                img.height(),
                img.colors(),
                img.make(),
                img.model()
            );
            match img.extract_thumbs() {
                Ok(thumbs) => Ok((dims, thumbs.len())),
                Err(e) => Err(format!("{dims}, extract_thumbs Err {e:?}")),
            }
        }));
        match r {
            Ok(Ok((info, thumb_count))) => {
                println!(
                    "[sample] {name} ({} B): {info}, thumbs={thumb_count} OK",
                    bytes.len()
                )
            }
            Ok(Err(msg)) => println!("[sample] {name} ({} B): {msg}", bytes.len()),
            Err(_) => println!("[sample] {name} ({} B): PANIC", bytes.len()),
        }
    }
}

/// 伪造一个「看起来像 TIFF 头」的畸形输入（很多 RAW 格式基于 TIFF 容器），
/// 但 IFD 偏移指向越界，测试是否在跟随偏移前做长度校验。
fn wrong_magic_tiff_like() -> Vec<u8> {
    let mut b = vec![0u8; 16];
    b[0..2].copy_from_slice(b"II"); // little-endian TIFF magic
    b[2..4].copy_from_slice(&42u16.to_le_bytes()); // TIFF magic number 42
    b[4..8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // 越界 IFD offset
    b
}

/// 合法 TIFF 头但截断——无 IFD 数据。
fn truncated_tiff_header() -> Vec<u8> {
    let mut b = vec![0u8; 8];
    b[0..2].copy_from_slice(b"II");
    b[2..4].copy_from_slice(&42u16.to_le_bytes());
    b[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD 紧跟在头后，但文件到此截断
    b
}
