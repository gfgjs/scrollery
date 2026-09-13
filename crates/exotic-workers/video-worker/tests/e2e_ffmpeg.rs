// crates/exotic-workers/video-worker/tests/e2e_ffmpeg.rs
//! 真 ffmpeg 端到端(视频格式扩展子系统 design.md §8 V3 验收)。
//!
//! **门控**:环境变量 `SCROLLERY_TEST_FFMPEG` 指向 ffmpeg.exe(同目录须有 ffprobe)。
//! 未设即 skip(打印原因)。fixture 用 lavfi(testsrc2/sine)运行时合成到 temp,不入库大二进制。
//!
//! 本文件含两类覆盖,**层级不同,如实分开说明**(V3 深审修复批 #11)。
//!
//! `e2e_probe_remux_transcode_frames` 是**参数级链路**:以黑盒方式手写复现 worker 拼装的
//! ffmpeg 命令(参数与各 `build_*_args` 一致)+ ffprobe 复核断言,验证真实 ffmpeg 确实吃
//! 这些参数、产物确实符合预期;**不**经过 video-worker 进程本身(bin crate 的集成测试
//! 不能直接 `use` 其内部模块,worker 内部纯逻辑——参数拼装/JSON 解析/阶梯迭代——由 bin
//! 内单测覆盖)。
//!
//! `e2e_worker_process_hello_ready_probe` 是**worker 进程级**:真 spawn 编译产物
//! (`env!("CARGO_BIN_EXE_video-worker")`),走真实 Hello/Ready 握手 + VideoSessionInit +
//! VideoProbe 一条完整协议帧链路,断言 Ready 四能力与 probe 关键字段——补齐前者未覆盖的
//! 「进程边界 + 协议帧编解码」这一层。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use exotic_protocol::{
    read_frame, write_frame, Frame, FrameType, HelloBody, ReadyBody, RequestBody, SuccessBody,
    MAX_BLOB_LEN, PROTOCOL_VERSION,
};

/// 取 ffmpeg 路径(env 门控);None = skip。
fn ffmpeg_path() -> Option<PathBuf> {
    let p = std::env::var("SCROLLERY_TEST_FFMPEG").ok()?;
    let p = PathBuf::from(p);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn ffprobe_path(ffmpeg: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    ffmpeg
        .parent()
        .map(|d| d.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("video-worker-e2e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(&d).unwrap()
}

/// 合成一个 2s 小样:testsrc2 视频 + sine 音频,指定容器/编码。
/// video_codec 用 libopenh264(若环境含;缺则探测降级,H.264 软编)以便无 GPU 环境也能造 h264 源。
fn synth(
    ffmpeg: &Path,
    out: &Path,
    container_args: &[&str],
    vcodec: &[&str],
    acodec: &[&str],
) -> bool {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=15:duration=2",
    ])
    .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
    .args(vcodec)
    .args(acodec)
    .args(container_args)
    .arg(out);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 探测 ffmpeg 是否内含 libopenh264 编码器(`-encoders` 输出核实,不假设固定存在——
/// 不同渠道的 LGPL-shared 构建可能裁剪掉软编 H.264,#10 深审修复)。
fn has_libopenh264(ffmpeg: &Path) -> bool {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("libopenh264"),
        Err(_) => false,
    }
}

/// 用 ffprobe 读容器/编码/时长(复核 worker 会看到的同一事实)。
fn probe_json(ffprobe: &Path, src: &Path) -> Option<serde_json::Value> {
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(src)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

#[test]
fn e2e_probe_remux_transcode_frames() {
    let Some(ffmpeg) = ffmpeg_path() else {
        eprintln!(
            "SKIP e2e_probe_remux_transcode_frames:未设 SCROLLERY_TEST_FFMPEG(指向 ffmpeg.exe)"
        );
        return;
    };
    let ffprobe = ffprobe_path(&ffmpeg);
    assert!(
        ffprobe.is_file(),
        "同目录应有 ffprobe:{}",
        ffprobe.display()
    );
    let work = temp_dir("work");

    // #10 深审修复:不假设 libopenh264 固定存在——缺失时 fixture1/2 换 mpeg4 源
    // (这两处后续断言只查容器/音轨,不查视频编码,换源不影响其断言有效性)。
    let h264_ok = has_libopenh264(&ffmpeg);
    let fixture_vcodec: &[&str] = if h264_ok {
        &["-c:v", "libopenh264", "-b:v", "300k"]
    } else {
        eprintln!("SKIP-DOWNGRADE:环境无 libopenh264,fixture1/2 改用 mpeg4 源合成");
        &["-c:v", "mpeg4", "-q:v", "5"]
    };

    // ── fixture 1:mkv + h264(或降级 mpeg4)+ aac ────────────────────────────────
    let mkv_h264_aac = work.join("h264_aac.mkv");
    assert!(
        synth(
            &ffmpeg,
            &mkv_h264_aac,
            &["-f", "matroska"],
            fixture_vcodec,
            &["-c:a", "aac"],
        ),
        "合成 fixture1(mkv+aac)失败"
    );
    let pj = probe_json(&ffprobe, &mkv_h264_aac).expect("probe fixture1");
    let fmt = pj["format"]["format_name"].as_str().unwrap_or("");
    assert!(fmt.contains("matroska"), "容器应为 matroska,实得 {fmt}");

    // ── remux:-c copy → mp4 faststart ──────────────────────────────────────────
    let remux_out = work.join("remux.mp4");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-y", "-i"])
        .arg(&mkv_h264_aac)
        .args([
            "-map",
            "0:V:0",
            "-map",
            "0:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "copy",
            "-sn",
            "-movflags",
            "+faststart",
        ])
        .arg(&remux_out)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "remux 失败");
    let rpj = probe_json(&ffprobe, &remux_out).expect("probe remux");
    assert!(rpj["format"]["format_name"]
        .as_str()
        .unwrap_or("")
        .contains("mp4"));
    // 时长保留(±0.3s)。
    let src_dur: f64 = pj["format"]["duration"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let out_dur: f64 = rpj["format"]["duration"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    assert!(
        (src_dur - out_dur).abs() < 0.3,
        "remux 时长应保留:{src_dur} vs {out_dur}"
    );

    // ── fixture 2:mkv + h264(或降级 mpeg4)+ ac3(半转码档:视频 copy、音轨转 aac)
    let mkv_h264_ac3 = work.join("h264_ac3.mkv");
    assert!(
        synth(
            &ffmpeg,
            &mkv_h264_ac3,
            &["-f", "matroska"],
            fixture_vcodec,
            &["-c:a", "ac3"],
        ),
        "合成 fixture2(mkv+ac3)失败"
    );
    let audio_remux = work.join("audio_remux.mp4");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-y", "-i"])
        .arg(&mkv_h264_ac3)
        .args([
            "-map", "0:V:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-sn",
        ])
        .arg(&audio_remux)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "半转码(音轨转 aac)失败");
    let apj = probe_json(&ffprobe, &audio_remux).expect("probe audio remux");
    let acodec = apj["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .and_then(|s| s["codec_name"].as_str())
        .unwrap_or("");
    assert_eq!(acodec, "aac", "音轨应转为 aac");

    // ── fixture 3:avi + mpeg4 ─────────────────────────────────────────────────
    let avi_mpeg4 = work.join("mpeg4.avi");
    assert!(
        synth(
            &ffmpeg,
            &avi_mpeg4,
            &["-f", "avi"],
            &["-c:v", "mpeg4", "-q:v", "5"],
            &["-c:a", "mp2"],
        ),
        "合成 avi-mpeg4 失败"
    );

    // ── 半转码:mpeg4 → h264(software libopenh264,验证转码链路真出产物)────────
    // 这一步需要真正把画面编码为 h264,libopenh264 缺失时无可替代编码器(mpeg4 目标
    // 编码器等价于不转码),故此块整体降级为 SKIP,不拖累其余断言判红(#10)。
    if h264_ok {
        let transcoded = work.join("transcoded.mp4");
        let ok = Command::new(&ffmpeg)
            .args(["-hide_banner", "-y", "-i"])
            .arg(&avi_mpeg4)
            .args([
                "-map",
                "0:V:0",
                "-map",
                "0:a:0?",
                "-c:v",
                "libopenh264",
                "-b:v",
                "400k",
                "-vf",
                "scale=-2:180",
                "-c:a",
                "aac",
                "-sn",
                "-movflags",
                "+faststart",
            ])
            .arg(&transcoded)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "转码 mpeg4→h264 失败");
        let tpj = probe_json(&ffprobe, &transcoded).expect("probe transcoded");
        let vcodec = tpj["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["codec_type"] == "video")
            .and_then(|s| s["codec_name"].as_str())
            .unwrap_or("");
        assert_eq!(vcodec, "h264", "转码后视频应为 h264");
    } else {
        eprintln!("SKIP-DOWNGRADE:环境无 libopenh264,跳过 mpeg4→h264 转码断言");
    }

    // ── frames/cover:抽单帧 PNG,断言非空且可解码 ─────────────────────────────
    let cover_bytes = Command::new(&ffmpeg)
        .args(["-hide_banner", "-nostdin", "-ss", "1.0", "-i"])
        .arg(&mkv_h264_aac)
        .args([
            "-frames:v", "1", "-vf", "scale=w=min(iw\\,240):h=min(ih\\,240):force_original_aspect_ratio=decrease:force_divisible_by=2",
            "-f", "image2pipe", "-c:v", "png", "pipe:1",
        ])
        .output()
        .expect("cover 抽帧");
    assert!(cover_bytes.status.success(), "cover 抽帧退出非零");
    assert!(!cover_bytes.stdout.is_empty(), "cover PNG 不应为空");
    assert_eq!(&cover_bytes.stdout[1..4], b"PNG", "应为 PNG 输出");

    eprintln!("e2e PASS:probe/remux/半转码/transcode/frames 全链路真跑通过");
    let _ = std::fs::remove_dir_all(&work);
}

/// **worker 进程级** e2e(#11 深审修复):真 spawn 编译出的 video-worker.exe,走真实
/// Hello/Ready 握手 + VideoSessionInit(真 ffmpeg 路径 + sha256)+ VideoProbe 一条完整
/// 协议帧链路。协议帧读写样板抄 `exotic-protocol` 本 crate 的帧测试样式(`frame.rs`
/// `#[cfg(test)]` roundtrip 用法)与 host 侧 `worker.rs` 的 spawn 握手流程。
#[test]
fn e2e_worker_process_hello_ready_probe() {
    let Some(ffmpeg) = ffmpeg_path() else {
        eprintln!(
            "SKIP e2e_worker_process_hello_ready_probe:未设 SCROLLERY_TEST_FFMPEG(指向 ffmpeg.exe)"
        );
        return;
    };
    let work = temp_dir("proc");

    // 探针 fixture:mpeg4 是 LGPL-shared ffmpeg 构建里几乎恒在的原生编码器,不依赖
    // libopenh264 是否被裁剪(与主 e2e 的降级判据一致,#10)。
    let fixture = work.join("tiny.mp4");
    assert!(
        synth(
            &ffmpeg,
            &fixture,
            &["-f", "mp4"],
            &["-c:v", "mpeg4", "-q:v", "5"],
            &["-c:a", "aac"]
        ),
        "合成探针 fixture 失败"
    );

    let ffmpeg_sha256 = {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(&ffmpeg).expect("读取 ffmpeg 二进制失败");
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_video-worker"));
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn video-worker 失败");
    let mut stdin = child.stdin.take().expect("worker stdin");
    let mut stdout = child.stdout.take().expect("worker stdout");

    // ── Hello → Ready:断言四能力齐全 ────────────────────────────────────────────
    let hello = HelloBody {
        host_version: "e2e-test".to_string(),
        protocol_version: PROTOCOL_VERSION,
        max_blob_len: MAX_BLOB_LEN,
    };
    write_frame(
        &mut stdin,
        &Frame::control(FrameType::Hello, 0, &hello).unwrap(),
    )
    .unwrap();
    stdin.flush().unwrap();
    let ready_frame = read_frame(&mut stdout).expect("读 Ready 帧失败");
    assert_eq!(ready_frame.frame_type, FrameType::Ready, "握手应回 Ready");
    let ready: ReadyBody = ready_frame.parse_json().expect("Ready JSON 解析失败");
    assert_eq!(ready.worker_id, "video-worker");
    for cap in [
        "video_probe",
        "video_remux",
        "video_transcode",
        "video_frames",
    ] {
        assert!(
            ready.capabilities.iter().any(|c| c == cap),
            "Ready.capabilities 应含 {cap},实得 {:?}",
            ready.capabilities
        );
    }

    // ── VideoSessionInit:真路径 + 真 sha256 ─────────────────────────────────────
    let init = RequestBody::VideoSessionInit {
        session_id: 1,
        ffmpeg_exe_path: ffmpeg.to_string_lossy().into_owned(),
        ffmpeg_sha256,
        work_dir: work.to_string_lossy().into_owned(),
    };
    write_frame(
        &mut stdin,
        &Frame::control(FrameType::Request, 100, &init).unwrap(),
    )
    .unwrap();
    stdin.flush().unwrap();
    let init_resp = read_frame(&mut stdout).expect("读 VideoSessionInit 应答失败");
    assert_eq!(
        init_resp.frame_type,
        FrameType::Success,
        "VideoSessionInit 应成功"
    );
    let init_body: SuccessBody = init_resp.parse_json().expect("SuccessBody 解析失败");
    let vs = init_body.video_session.expect("应带 video_session");
    assert_eq!(vs.caps.len(), 4, "四能力齐全");

    // ── VideoProbe:断言关键流事实 ────────────────────────────────────────────────
    let probe_req = RequestBody::VideoProbe {
        session_id: 1,
        source_path: fixture.to_string_lossy().into_owned(),
        input_fingerprint: "fp".to_string(),
    };
    write_frame(
        &mut stdin,
        &Frame::control(FrameType::Request, 101, &probe_req).unwrap(),
    )
    .unwrap();
    stdin.flush().unwrap();
    let probe_resp = read_frame(&mut stdout).expect("读 VideoProbe 应答失败");
    assert_eq!(
        probe_resp.frame_type,
        FrameType::Success,
        "VideoProbe 应成功"
    );
    let probe_body: SuccessBody = probe_resp.parse_json().expect("SuccessBody 解析失败");
    let info = probe_body.video_probe.expect("应带 video_probe");
    assert_eq!(info.video_codec, "mpeg4");
    assert!(info.duration_ms.unwrap_or(0) > 0, "应探得时长");

    // ── Shutdown:优雅退出 ───────────────────────────────────────────────────────
    write_frame(
        &mut stdin,
        &Frame::control(FrameType::Shutdown, 0, &()).unwrap(),
    )
    .unwrap();
    stdin.flush().unwrap();
    let _ = child.wait();

    eprintln!("e2e PASS:worker 进程级 Hello/Ready/VideoSessionInit/VideoProbe 全链路真跑通过");
    let _ = std::fs::remove_dir_all(&work);
}
