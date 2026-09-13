// crates/scrollery-ai-core/tests/ocr_golden.rs
//! OCR 模型级 characterization + 速度基准(均 `#[ignore]`,本机手跑非 CI)。
//!
//! 依赖已下载模型(env `OCR_MODELS_DIR`)与检入 fixture(`tests/fixtures/ocr/`)。
//! fixture 或模型缺失时以明确 `eprintln` 提前 return(**不 panic**),避免误伤 CI/他人机器。
//!
//! # golden(通道序/归一化定案仪式)
//! 这是 `swap_rb` 与归一化参数的**定案测试**:若断言不过,翻转 `ocr_profile.rs` 的
//! `swap_rb` 重跑,以过者为准回写默认值并在注释记录定案依据(见 construction-plan 边界1)。
//!
//! 运行:
//! ```text
//! set OCR_MODELS_DIR=<模型目录>
//! cargo test -p scrollery-ai-core --release --features inference --test ocr_golden -- --ignored --nocapture
//! ```
#![cfg(feature = "inference")]

use std::path::PathBuf;
use std::time::Instant;

use scrollery_ai_core::ocr::OcrEngine;
use scrollery_ai_core::ocr_profile::{find_ocr_profile, ocr_profiles, DEFAULT_OCR_PROFILE_ID};

/// 取模型目录 env;缺失返回 None(调用方提前 return)。
fn models_dir() -> Option<PathBuf> {
    std::env::var_os("OCR_MODELS_DIR").map(PathBuf::from)
}

/// fixture 目录(相对本 crate manifest)。
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ocr")
}

/// golden 断言表:(fixture 文件名, 识别文本需包含的子串集合)。
/// 子串约定见 `tests/fixtures/ocr/README.md`;fixture 落位后据实际图内容微调。
const GOLDEN: &[(&str, &[&str])] = &[
    // 中英混排印刷体一张:含中文词与英文/数字。
    ("mixed_zh_en.png", &["Scrollery", "文字"]),
    // 含 180° 倒置行一张:cls 应摆正后识别出倒置行文本。
    ("inverted_line.png", &["倒置"]),
];

#[test]
#[ignore = "需 OCR_MODELS_DIR + 检入 fixture,本机手跑"]
fn ocr_golden() {
    let Some(dir) = models_dir() else {
        eprintln!("[ocr_golden] SKIP: OCR_MODELS_DIR 未设置");
        return;
    };
    let profile = find_ocr_profile(DEFAULT_OCR_PROFILE_ID).expect("default profile");
    let engine = match OcrEngine::init(&dir, &profile) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[ocr_golden] SKIP: 引擎加载失败(模型缺失?): {e}");
            return;
        }
    };

    let fdir = fixtures_dir();
    let mut ran = 0usize;
    for (file, needles) in GOLDEN {
        let path = fdir.join(file);
        if !path.exists() {
            eprintln!("[ocr_golden] SKIP fixture 缺失: {path:?}");
            continue;
        }
        let img = image::open(&path).expect("fixture 应可解码");
        let out = engine.recognize(&img).expect("recognize");
        let joined: String = out
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        eprintln!("[ocr_golden] {file}: {} 行\n{joined}", out.lines.len());
        for needle in *needles {
            assert!(
                joined.contains(needle),
                "{file}: 识别文本应含子串 {needle:?}(若通道序错,翻转 swap_rb 重跑)\n实际:\n{joined}"
            );
        }
        ran += 1;
    }
    if ran == 0 {
        eprintln!("[ocr_golden] SKIP: 无可用 fixture(落位后再判定)");
    }
}

#[test]
#[ignore = "需 OCR_MODELS_DIR + OCR_BENCH_IMG,本机手跑"]
fn ocr_bench() {
    let Some(dir) = models_dir() else {
        eprintln!("[ocr_bench] SKIP: OCR_MODELS_DIR 未设置");
        return;
    };
    let Some(img_path) = std::env::var_os("OCR_BENCH_IMG").map(PathBuf::from) else {
        eprintln!("[ocr_bench] SKIP: OCR_BENCH_IMG 未设置(指向本机一张 1080p~4K 含文字图)");
        return;
    };
    if !img_path.exists() {
        eprintln!("[ocr_bench] SKIP: 图不存在 {img_path:?}");
        return;
    }
    let img = image::open(&img_path).expect("bench 图应可解码");

    for profile in ocr_profiles() {
        let engine = match OcrEngine::init(&dir, &profile) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[ocr_bench] SKIP {}: 加载失败: {e}", profile.id);
                continue;
            }
        };
        let mut ms: Vec<f64> = Vec::new();
        // 6 次弃首轮(冷启动)。
        for i in 0..6 {
            let t0 = Instant::now();
            let _ = engine.recognize(&img).expect("recognize");
            let dt = t0.elapsed().as_secs_f64() * 1000.0;
            if i > 0 {
                ms.push(dt);
            }
        }
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = ms[ms.len() / 2];
        eprintln!(
            "[ocr_bench] {} 端到端中位数 = {:.1} ms(5 计时轮:{:?})",
            profile.id, median, ms
        );
    }
}
