// crates/exotic-workers/video-worker/src/session.rs
//! VideoSessionInit 的校验与会话态(design.md §2.3/§3.4)。
//!
//! 校验清单(全过才就绪,任一不过 → `FfmpegUnavailable` terminal,host 标记工具待重下载):
//!   1. `ffmpeg_exe_path` 存在且 canonicalize 可达;
//!   2. 文件 sha256 与 `ffmpeg_sha256` 相符(防用户手工换包);
//!   3. 同目录 `ffprobe(.exe)` 存在;
//!   4. `ffmpeg -version` 可运行,且 configuration 行**不含** `--enable-gpl`(许可运行时保险丝)。
//!
//! `work_dir` canonicalize 后作输出白名单前缀(与 EnhanceSessionInit.work_dir 同型)。

use std::path::{Path, PathBuf};

use crate::error::VideoError;

/// 已就绪的视频会话态(严格串行:同一时刻至多一个)。
#[derive(Debug)]
pub struct VideoSessionState {
    pub session_id: u64,
    /// canonicalize 后的 ffmpeg.exe 路径。
    pub ffmpeg_path: PathBuf,
    /// 同目录推导的 ffprobe(.exe) 路径。
    pub ffprobe_path: PathBuf,
    /// canonicalize 后的输出白名单前缀。
    pub work_dir: PathBuf,
    /// `ffmpeg -version` 首行版本号(回执/诊断)。
    pub ffmpeg_version: String,
}

impl VideoSessionState {
    /// 本会话可服务的四能力(Ready 通告与此一致)。
    pub fn caps() -> Vec<String> {
        use exotic_protocol::capability::*;
        vec![
            VIDEO_PROBE.to_string(),
            VIDEO_REMUX.to_string(),
            VIDEO_TRANSCODE.to_string(),
            VIDEO_FRAMES.to_string(),
        ]
    }
}

/// 校验并建立视频会话。IO(sha256/运行 -version)在此发生;纯解析拆分为
/// [`parse_ffmpeg_version`] / [`configuration_has_gpl`](单测覆盖)。
pub fn validate_video_init(
    session_id: u64,
    ffmpeg_exe_path: &str,
    ffmpeg_sha256: &str,
    work_dir: &str,
) -> Result<VideoSessionState, VideoError> {
    let ffmpeg_path = std::fs::canonicalize(ffmpeg_exe_path)
        .map_err(|e| VideoError::FfmpegUnavailable(format!("ffmpeg 路径不可达:{}", e.kind())))?;
    let work_dir = std::fs::canonicalize(work_dir)
        .map_err(|e| VideoError::Malformed(format!("work_dir 不可达:{}", e.kind())))?;

    // 完整性:文件 sha256 == 声明(防换包,§3.4)。
    let actual = sha256_file(&ffmpeg_path).map_err(|e| {
        VideoError::FfmpegUnavailable(format!("ffmpeg sha256 计算失败:{}", e.kind()))
    })?;
    if actual != ffmpeg_sha256.to_lowercase() {
        return Err(VideoError::FfmpegUnavailable(
            "ffmpeg sha256 与声明不符(疑似换包)".into(),
        ));
    }

    let ffprobe_path = derive_ffprobe_path(&ffmpeg_path);
    if !ffprobe_path.is_file() {
        return Err(VideoError::FfmpegUnavailable("同目录 ffprobe 缺失".into()));
    }

    // 运行 `ffmpeg -version`:读版本 + GPL 保险丝。
    let (ok, stdout, _stderr) = crate::ffrun::run_output(&ffmpeg_path, &["-version".to_string()])?;
    if !ok {
        return Err(VideoError::FfmpegUnavailable(
            "ffmpeg -version 运行失败".into(),
        ));
    }
    let (version, configuration) = parse_ffmpeg_version(&stdout);
    // fail-closed:configuration 行缺失就无法核验 GPL 保险丝,视同不可信构建直接拒绝
    // (宁可误拒也不放过潜在 GPL 构建;不能反过来把「无法判断」当「判断为否」放行)。
    if configuration_missing(&configuration) {
        return Err(VideoError::FfmpegUnavailable(
            "ffmpeg -version 输出缺 configuration 行,GPL 保险丝无法核验,fail-closed 拒绝".into(),
        ));
    }
    if configuration_has_gpl(&configuration) {
        return Err(VideoError::FfmpegUnavailable(
            "检出 GPL 构建(configuration 含 --enable-gpl),许可红线拒绝".into(),
        ));
    }

    Ok(VideoSessionState {
        session_id,
        ffmpeg_path,
        ffprobe_path,
        work_dir,
        ffmpeg_version: version,
    })
}

/// 由 ffmpeg 路径推导同目录 ffprobe 路径(Windows 带 `.exe` 后缀)。
pub fn derive_ffprobe_path(ffmpeg: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    match ffmpeg.parent() {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

/// 解析 `ffmpeg -version` 输出 → (版本号, configuration 行)。
/// 首行形如 `ffmpeg version n7.1.5-... Copyright ...`,版本取第 3 个 token;
/// configuration 行形如 `configuration: --prefix=... --enable-...`。
pub fn parse_ffmpeg_version(stdout: &str) -> (String, String) {
    let mut version = String::new();
    let mut configuration = String::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if version.is_empty() && trimmed.starts_with("ffmpeg version") {
            version = trimmed
                .split_whitespace()
                .nth(2)
                .unwrap_or_default()
                .to_string();
        } else if let Some(rest) = trimmed.strip_prefix("configuration:") {
            configuration = rest.trim().to_string();
        }
    }
    (version, configuration)
}

/// `ffmpeg -version` 输出是否缺 configuration 行(fail-closed 判据:缺失即无法核验
/// GPL 保险丝,一律拒绝而非默认放行)。
pub fn configuration_missing(configuration: &str) -> bool {
    configuration.is_empty()
}

/// configuration 行是否含 `--enable-gpl`(§3.4 保险丝)。**token 精确相等**,不匹配
/// `--enable-gpl3`/`--enable-version3` 等相邻串,避免误判 LGPL 包。
pub fn configuration_has_gpl(configuration: &str) -> bool {
    configuration
        .split_whitespace()
        .any(|t| t == "--enable-gpl")
}

/// 输出白名单校验:`output_tmp_path` 父目录 canonicalize 后须在 `work_dir` 之下,
/// 否则 Malformed(canonicalize 解析 `..`,穿越在 starts_with 处被拦;enhance-worker 同型)。
pub fn resolve_output_path(output_tmp_path: &str, work_dir: &Path) -> Result<PathBuf, VideoError> {
    let p = Path::new(output_tmp_path);
    let parent = p
        .parent()
        .filter(|s| !s.as_os_str().is_empty())
        .ok_or_else(|| VideoError::Malformed("output_tmp_path 无父目录".into()))?;
    let canon_parent = std::fs::canonicalize(parent)
        .map_err(|e| VideoError::Malformed(format!("output 父目录不可达:{}", e.kind())))?;
    if !canon_parent.starts_with(work_dir) {
        return Err(VideoError::Malformed(
            "output_tmp_path 越界 work_dir 白名单".into(),
        ));
    }
    let name = p
        .file_name()
        .ok_or_else(|| VideoError::Malformed("output_tmp_path 无文件名".into()))?;
    Ok(canon_parent.join(name))
}

/// 流式 sha256(ffmpeg.exe 可达数十 MB,不整读进内存),输出 64 位小写 hex。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_config_parse() {
        let out = "ffmpeg version n7.1.5-10-g2aefd64d48 Copyright (c) 2000-2026\n\
                   built with gcc 15.2.0\n\
                   configuration: --prefix=/x --enable-shared --disable-libx264 --enable-version3\n\
                   libavutil 59. 39.100\n";
        let (v, c) = parse_ffmpeg_version(out);
        assert_eq!(v, "n7.1.5-10-g2aefd64d48");
        assert!(c.contains("--disable-libx264"));
        assert!(!configuration_has_gpl(&c), "LGPL 包不应判 GPL");
    }

    #[test]
    fn missing_configuration_line_is_fail_closed() {
        // `-version` 输出无 configuration 行(如异常/被裁剪的构建)→ fail-closed 判缺失。
        let out = "ffmpeg version n7.1.5-10-g2aefd64d48 Copyright (c) 2000-2026\n\
                   built with gcc 15.2.0\n\
                   libavutil 59. 39.100\n";
        let (_, c) = parse_ffmpeg_version(out);
        assert!(c.is_empty(), "本 fixture 故意不含 configuration 行");
        assert!(
            configuration_missing(&c),
            "缺 configuration 行应判定为缺失(fail-closed)"
        );
        // 正常有 configuration 行时不误判缺失。
        assert!(!configuration_missing("--prefix=/x --enable-shared"));
    }

    #[test]
    fn gpl_fuse_detects_enable_gpl() {
        assert!(configuration_has_gpl(
            "--prefix=/x --enable-gpl --enable-libx264"
        ));
        assert!(configuration_has_gpl("--enable-gpl"));
        // 相邻串不误判。
        assert!(!configuration_has_gpl(
            "--enable-version3 --enable-gpl-something"
        ));
        assert!(!configuration_has_gpl("--disable-gpl"));
        assert!(!configuration_has_gpl(""));
    }

    #[test]
    fn ffprobe_derivation_same_dir() {
        let p = derive_ffprobe_path(Path::new("/opt/ff/bin/ffmpeg"));
        assert!(p.ends_with(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        }));
        assert!(p.to_string_lossy().contains("bin"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("video-worker-sess-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn sha256_matches_known() {
        let d = temp_dir("sha");
        let f = d.join("blob.bin");
        std::fs::write(&f, b"hello").unwrap();
        // sha256("hello") 已知常量。
        assert_eq!(
            sha256_file(&f).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn output_whitelist_inside_ok_outside_rejected() {
        let work = temp_dir("wl");
        let inside = work.join("job.mp4.tmp");
        let r = resolve_output_path(inside.to_str().unwrap(), &work).unwrap();
        assert!(r.starts_with(&work));

        let sibling = work
            .parent()
            .unwrap()
            .join(format!("video-worker-sess-{}-evil", std::process::id()));
        let _ = std::fs::remove_dir_all(&sibling);
        std::fs::create_dir_all(&sibling).unwrap();
        let out = sibling.join("x.tmp");
        assert!(resolve_output_path(out.to_str().unwrap(), &work).is_err());
    }

    #[test]
    fn output_whitelist_dotdot_traversal_rejected() {
        let work = temp_dir("dd");
        let outside = work
            .parent()
            .unwrap()
            .join(format!("video-worker-sess-{}-out", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let traversal = work
            .join("..")
            .join(outside.file_name().unwrap())
            .join("x.tmp");
        assert!(resolve_output_path(traversal.to_str().unwrap(), &work).is_err());
    }

    #[test]
    fn validate_rejects_sha_mismatch() {
        // 造一个假 ffmpeg 文件 + 假 ffprobe;sha 不符应在运行 -version 前即拒。
        let d = temp_dir("valsha");
        let ff = d.join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        std::fs::write(&ff, b"not a real exe").unwrap();
        std::fs::write(
            d.join(if cfg!(windows) {
                "ffprobe.exe"
            } else {
                "ffprobe"
            }),
            b"x",
        )
        .unwrap();
        let e = validate_video_init(
            1,
            ff.to_str().unwrap(),
            &"0".repeat(64),
            d.to_str().unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            e.code(),
            exotic_protocol::WorkerErrorCode::FfmpegUnavailable
        );
    }

    #[test]
    fn validate_rejects_missing_ffprobe() {
        let d = temp_dir("noprobe");
        let ff = d.join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        let content = b"dummy";
        std::fs::write(&ff, content).unwrap();
        let sha = sha256_file(&ff).unwrap();
        // sha 相符但无 ffprobe → FfmpegUnavailable。
        let e =
            validate_video_init(1, ff.to_str().unwrap(), &sha, d.to_str().unwrap()).unwrap_err();
        assert_eq!(
            e.code(),
            exotic_protocol::WorkerErrorCode::FfmpegUnavailable
        );
    }
}
