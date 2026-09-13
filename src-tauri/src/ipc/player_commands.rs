//! Tauri IPC commands for the video player (播放器线,2026-07-22):字幕加载 + 截帧保存。
//! 与 media_commands 分文件——播放器专属、非 media_items 域 DAO 消费。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Serialize;
use tauri::State;

use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::ipc::blocking::read_blocking;
use crate::reader::encoding::decode_bytes;
use crate::state::AppState;

/// 字幕文件读取上限(5MB;方案约束):字幕是纯文本,远超此值大概率误选文件,拒绝而非硬吞。
const SUBTITLE_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// 字幕扩展名白名单(大小写不敏感)。
const SUBTITLE_EXTENSIONS: &[&str] = &["vtt", "srt"];

const CODE_SUBTITLE_NOT_FOUND: &str = "player_subtitle_not_found";
const CODE_SUBTITLE_UNSUPPORTED: &str = "player_subtitle_unsupported_type";
const CODE_SUBTITLE_TOO_LARGE: &str = "player_subtitle_too_large";
const CODE_SUBTITLE_IO: &str = "player_subtitle_io";
const CODE_FRAME_TARGET_INVALID: &str = "player_frame_target_invalid";
const CODE_FRAME_DECODE_FAILED: &str = "player_frame_decode_failed";
const CODE_FRAME_IO: &str = "player_frame_io";
const CODE_FRAME_TOO_LARGE: &str = "player_frame_too_large";
const CODE_FRAME_UNSUPPORTED_FORMAT: &str = "player_frame_unsupported_format";

/// 截帧解码后目标上限(64MB;纵深防御,GA-fix #5)。
const FRAME_DECODED_MAX_BYTES: usize = 64 * 1024 * 1024;
/// base64 解码前预检上限:按 base64 4/3 膨胀系数估算(64MB → ~88MB),避免为超大
/// payload 白白分配解码缓冲区再拒绝。
const FRAME_BASE64_MAX_BYTES: usize = FRAME_DECODED_MAX_BYTES / 3 * 4 + 4;

/// PNG 文件签名(8 字节魔数),RFC 2083 §3.1。
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn player_err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::Player {
        code,
        message: message.into(),
    }
}

/// 解析后的字幕文件:文件名(展示用)+ 解码后的 UTF-8 文本内容。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleFile {
    pub file_name: String,
    pub content: String,
}

/// 加载视频字幕(播放器线)。`path=None` 时按 item 文件同目录同 basename 探测 `.vtt`/`.srt`
/// (大小写不敏感);`Some` 为用户经原生对话框选中的路径。canonicalize + 扩展名白名单 + 5MB
/// 上限 + chardetng 自动编码检测解码(复用阅读器同一 seam,见 `reader::encoding::decode_bytes`)。
/// message **不携带绝对路径**(泄漏面,同 Preview/Reveal 姿态)。
#[tauri::command]
pub async fn load_video_subtitle(
    item_id: i64,
    path: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<SubtitleFile> {
    // R1-3:先经读池取 item 绝对路径(rusqlite 不得在 async worker 上直接跑)。
    // GA-fix #6:仓内目前无更轻的「仅取 abs_path」查询——get_media_detail 附带 image_meta/
    // video_meta 等联查,略重但唯一现成;新增专用轻量查询超出本次修复范围,留作后续。
    let abs_path: String = read_blocking(&state, move |conn| {
        q::get_media_detail(conn, item_id).map(|d| d.abs_path)
    })
    .await?;

    tokio::task::spawn_blocking(move || -> Result<SubtitleFile> {
        let candidate = match path {
            // 授权模型(GA-fix #3):显式 path 仅来自前端原生文件对话框回传——用户在对话框中
            // 选中该文件即完成授权,此处不再做“属于某个已知媒体目录”之类的归属校验;
            // 下方的扩展名白名单 + 5MB 上限是纵深防御,不是授权判定本身。
            Some(p) => PathBuf::from(p),
            None => find_sidecar_subtitle(Path::new(&abs_path)).ok_or_else(|| {
                player_err(
                    CODE_SUBTITLE_NOT_FOUND,
                    "未找到同名字幕文件 | no sidecar subtitle found",
                )
            })?,
        };
        load_subtitle_file(&candidate)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 在 `media_path` 同目录下探测同 basename 的 `.vtt`/`.srt`(大小写不敏感)。
/// `.vtt`/`.srt` 并存时显式优先 `.vtt`(GA-fix #2)——先收集两候选再定序,不依赖
/// `read_dir` 的枚举序(该序在不同文件系统/平台上无保证)。
fn find_sidecar_subtitle(media_path: &Path) -> Option<PathBuf> {
    let dir = media_path.parent()?;
    let stem = media_path.file_stem()?.to_str()?;
    let entries = fs::read_dir(dir).ok()?;
    let mut vtt: Option<PathBuf> = None;
    let mut srt: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let candidate = entry.path();
        let cand_stem = candidate.file_stem().and_then(|s| s.to_str());
        let cand_ext = candidate
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        // GA-fix #1:stem 比较改 eq_ignore_ascii_case——仅做 ASCII 大小写折叠,非 ASCII
        // 字符的大小写语义弱于 OS 原生比较,但足以覆盖绝大多数命名场景,可接受。
        let stem_matches = cand_stem.is_some_and(|s| s.eq_ignore_ascii_case(stem));
        if !stem_matches {
            continue;
        }
        match cand_ext.as_deref() {
            Some("vtt") if vtt.is_none() => vtt = Some(candidate),
            Some("srt") if srt.is_none() => srt = Some(candidate),
            _ => {}
        }
    }
    vtt.or(srt)
}

/// canonicalize + 扩展名白名单 + 5MB 上限 + 自动编码检测解码,产出 [`SubtitleFile`]。
fn load_subtitle_file(candidate: &Path) -> Result<SubtitleFile> {
    let canon = candidate.canonicalize().map_err(|_| {
        player_err(
            CODE_SUBTITLE_NOT_FOUND,
            "字幕文件不存在 | subtitle file not found",
        )
    })?;

    let ext_ok = canon
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| SUBTITLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !ext_ok {
        return Err(player_err(
            CODE_SUBTITLE_UNSUPPORTED,
            "不支持的字幕格式,仅支持 .vtt/.srt | unsupported subtitle format",
        ));
    }

    // GA-fix #4:metadata().len() 校验 + 独立 fs::read() 两步之间存在 TOCTOU 增长窗口
    // (文件在校验后、读取前被追加)。改为单步:File::open 后用 take(MAX+1) 读,读出的
    // 字节数超过上限即拒——上限判定与实际读取绑在同一次 I/O 里,无法被增长绕过。
    let file = File::open(&canon)
        .map_err(|_| player_err(CODE_SUBTITLE_IO, "字幕文件读取失败 | subtitle read failed"))?;
    let mut bytes = Vec::new();
    file.take(SUBTITLE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| player_err(CODE_SUBTITLE_IO, "字幕文件读取失败 | subtitle read failed"))?;
    if bytes.len() as u64 > SUBTITLE_MAX_BYTES {
        return Err(player_err(
            CODE_SUBTITLE_TOO_LARGE,
            "字幕文件超过 5MB 上限 | subtitle exceeds 5MB limit",
        ));
    }
    let outcome = decode_bytes(&bytes, None);

    let file_name = canon
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("subtitle")
        .to_string();

    Ok(SubtitleFile {
        file_name,
        content: outcome.text,
    })
}

/// 保存播放器截帧 PNG(播放器线)。`data_base64` 为标准 base64 编码的 PNG 字节;
/// canonicalize 父目录(须已存在)+ 路径 bounds check(取 `target_path` 的文件名重新拼回
/// canonical 父目录,杜绝文件名段挟带 `..`/分隔符逃逸)后,写 `*.tmp` → `sync_all` →
/// 同卷 `rename`(项目硬约束:先 tmp 后 rename,不发布半成品)。
///
/// 授权模型(GA-fix #5):`target_path` 仅来自前端系统 save 对话框——用户在对话框中
/// 选定保存位置即完成授权,覆盖已存在文件的确认也由对话框本身承担,此处不再二次确认。
/// base64 长度预检(64MB 解码目标 → ~88MB base64)与解码后的 PNG 魔数校验属纵深防御,
/// 不是授权判定本身。
#[tauri::command]
pub async fn save_frame_png(target_path: String, data_base64: String) -> Result<()> {
    tokio::task::spawn_blocking(move || save_frame_png_sync(target_path, data_base64))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// `save_frame_png` 的同步实现体(抽出以便单测直接调用,不必绕 tokio runtime)。
fn save_frame_png_sync(target_path: String, data_base64: String) -> Result<()> {
    {
        let target = PathBuf::from(&target_path);
        let file_name = target.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            player_err(
                CODE_FRAME_TARGET_INVALID,
                "目标文件名无效 | invalid target file name",
            )
        })?;
        let parent = target.parent().ok_or_else(|| {
            player_err(
                CODE_FRAME_TARGET_INVALID,
                "目标路径缺少父目录 | target path missing parent directory",
            )
        })?;
        let canon_parent = parent.canonicalize().map_err(|_| {
            player_err(
                CODE_FRAME_TARGET_INVALID,
                "目标目录不存在 | target directory not found",
            )
        })?;
        // bounds check:文件名重新拼回 canonicalize 后的父目录——file_name() 天然不含分隔符/
        // `..` 组件,故拼接结果恒落在 canon_parent 内(杜绝路径穿越)。
        let final_path = canon_parent.join(file_name);

        // GA-fix #5①:base64 解码前预检长度上限,避免为超大 payload 白白分配解码缓冲区。
        if data_base64.len() > FRAME_BASE64_MAX_BYTES {
            return Err(player_err(
                CODE_FRAME_TOO_LARGE,
                "帧数据超过大小上限 | frame data exceeds size limit",
            ));
        }

        let bytes = BASE64_STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| {
                player_err(
                    CODE_FRAME_DECODE_FAILED,
                    "帧数据解码失败 | frame data base64 decode failed",
                )
            })?;

        // GA-fix #5②:解码后校验 PNG 魔数,拒绝非 PNG 字节流(该命令语义即"保存 PNG",
        // 写入非 PNG 内容会产出扩展名与内容不符的坏文件)。
        if !bytes.starts_with(&PNG_MAGIC) {
            return Err(player_err(
                CODE_FRAME_UNSUPPORTED_FORMAT,
                "帧数据不是有效 PNG | frame data is not a valid PNG",
            ));
        }

        let tmp_path = final_path.with_file_name(format!("{file_name}.tmp"));
        let _ = fs::remove_file(&tmp_path); // 清可能的异常退出残留

        let write_result: std::io::Result<()> = (|| {
            let mut f = File::create(&tmp_path)?;
            f.write_all(&bytes)?;
            f.sync_all()
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp_path);
            return Err(player_err(
                CODE_FRAME_IO,
                "截帧写入失败 | frame write failed",
            ));
        }

        fs::rename(&tmp_path, &final_path).map_err(|_| {
            let _ = fs::remove_file(&tmp_path);
            player_err(CODE_FRAME_IO, "截帧写入失败 | frame write failed")
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod sidecar_tests {
    //! `find_sidecar_subtitle` 回归钉:GA-fix #1(大小写不敏感 stem 比较)+ #2(.vtt 优先)。
    use super::*;

    #[test]
    fn matches_same_stem_srt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("movie.srt"),
            b"1\n00:00:00,000 --> 00:00:01,000\nhi\n",
        )
        .unwrap();
        let media_path = dir.path().join("movie.mp4");
        let found = find_sidecar_subtitle(&media_path).unwrap();
        assert_eq!(found, dir.path().join("movie.srt"));
    }

    #[test]
    fn matches_case_insensitive_stem() {
        // GA-fix #1 回归锚:Movie.mp4 + movie.srt(stem 大小写不同)须命中。
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("movie.srt"), b"content").unwrap();
        let media_path = dir.path().join("Movie.mp4");
        let found = find_sidecar_subtitle(&media_path).unwrap();
        assert_eq!(found, dir.path().join("movie.srt"));
    }

    #[test]
    fn prefers_vtt_over_srt_when_both_present() {
        // GA-fix #2 回归锚:.vtt/.srt 并存时须显式取 .vtt,不依赖枚举序。
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("movie.vtt"), b"WEBVTT\n").unwrap();
        fs::write(dir.path().join("movie.srt"), b"1\n").unwrap();
        let media_path = dir.path().join("movie.mp4");
        let found = find_sidecar_subtitle(&media_path).unwrap();
        assert_eq!(found, dir.path().join("movie.vtt"));
    }

    #[test]
    fn no_match_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let media_path = dir.path().join("movie.mp4");
        assert!(find_sidecar_subtitle(&media_path).is_none());
    }
}

#[cfg(test)]
mod load_subtitle_file_tests {
    //! `load_subtitle_file` 回归钉:显式 path 分支——扩展名白名单 / 5MB 上限(GA-fix #4
    //! TOCTOU 收窄) / chardetng 解码 / not-found 报错不携带绝对路径。
    use super::*;

    #[test]
    fn rejects_extension_outside_whitelist() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.txt");
        fs::write(&p, b"hello").unwrap();
        let err = load_subtitle_file(&p).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_SUBTITLE_UNSUPPORTED
        ));
    }

    #[test]
    fn rejects_over_5mb_plus_one() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.srt");
        let body = vec![b'a'; (SUBTITLE_MAX_BYTES + 1) as usize];
        fs::write(&p, &body).unwrap();
        let err = load_subtitle_file(&p).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_SUBTITLE_TOO_LARGE
        ));
    }

    #[test]
    fn accepts_exactly_5mb_via_take_path() {
        // GA-fix #4:File::open + take(MAX+1) 路径——恰好 5MB 须放行(边界值,非超限)。
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("exact.srt");
        let body = vec![b'a'; SUBTITLE_MAX_BYTES as usize];
        fs::write(&p, &body).unwrap();
        let out = load_subtitle_file(&p).unwrap();
        assert_eq!(out.content.len(), SUBTITLE_MAX_BYTES as usize);
    }

    #[test]
    fn decodes_gbk_content_to_correct_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cn.srt");
        let text = "第一章 风起\n洛阳城的清晨,薄雾还未散去。\n";
        let (bytes, _, _) = encoding_rs::GBK.encode(text);
        fs::write(&p, &bytes).unwrap();
        let out = load_subtitle_file(&p).unwrap();
        assert_eq!(out.content, text, "GBK 字幕须解码为正确 UTF-8");
    }

    #[test]
    fn nonexistent_path_errors_without_leaking_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.srt");
        let err = load_subtitle_file(&p).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_SUBTITLE_NOT_FOUND
        ));
        assert!(
            !msg.contains(dir.path().to_string_lossy().as_ref()),
            "message 不得携带绝对路径,实际: {msg}"
        );
    }
}

#[cfg(test)]
mod save_frame_png_tests {
    //! `save_frame_png_sync` 回归钉:GA-fix #5(64MB 预检 + PNG 魔数校验 + 授权模型)
    //! 与既有 bounds-check / tmp-then-rename 行为。
    use super::*;

    fn valid_png_bytes() -> Vec<u8> {
        // 只需满足魔数前缀——本函数不做完整 PNG 结构校验(方案范围外)。
        let mut v = PNG_MAGIC.to_vec();
        v.extend_from_slice(b"fake-chunk-data");
        v
    }

    #[test]
    fn valid_base64_and_png_magic_writes_final_no_tmp_residual() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("frame.png");
        let b64 = BASE64_STANDARD.encode(valid_png_bytes());
        save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap();
        assert!(target.exists());
        assert!(!target.with_file_name("frame.png.tmp").exists());
    }

    #[test]
    fn traversal_via_middle_dotdot_still_resolves_inside_canonical_dir() {
        // 父目录路径中间含 `..`(会被 canonicalize 消解为真实存在的目录),文件名段本身
        // 干净——bounds check(文件名重拼回 canonical 父目录)在此仍成立、非穿越。
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        let target = sub.join("..").join("sub").join("frame.png");
        let b64 = BASE64_STANDARD.encode(valid_png_bytes());
        save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap();
        assert!(sub.join("frame.png").exists());
    }

    #[test]
    fn rejects_when_final_component_is_dotdot() {
        // target_path 以 `..` 结尾 → file_name() 为 None → 目标文件名无效,拒绝
        // (穿越防御的另一面:不允许把 `..` 本身当文件名段)。
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub").join("..");
        let b64 = BASE64_STANDARD.encode(valid_png_bytes());
        let err = save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_FRAME_TARGET_INVALID
        ));
    }

    #[test]
    fn rejects_when_parent_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("does-not-exist").join("frame.png");
        let b64 = BASE64_STANDARD.encode(valid_png_bytes());
        let err = save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_FRAME_TARGET_INVALID
        ));
    }

    #[test]
    fn rejects_invalid_base64_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("frame.png");
        let err = save_frame_png_sync(target.to_string_lossy().into_owned(), "not-base64!!".into())
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_FRAME_DECODE_FAILED
        ));
        assert!(!target.with_file_name("frame.png.tmp").exists());
    }

    #[test]
    fn rejects_non_png_bytes() {
        // GA-fix #5②回归锚:base64 合法但解码字节不是 PNG(缺魔数)须拒。
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("frame.png");
        let b64 = BASE64_STANDARD.encode(b"not a png file at all");
        let err = save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_FRAME_UNSUPPORTED_FORMAT
        ));
        assert!(!target.exists());
    }

    #[test]
    fn rejects_base64_over_max_bytes() {
        // GA-fix #5①回归锚:data_base64.len() 超过 FRAME_BASE64_MAX_BYTES 须在解码前
        // 就被预检拒绝(避免为超大 payload 白白分配解码缓冲区)。
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("frame.png");
        let oversize_b64 = "A".repeat(FRAME_BASE64_MAX_BYTES + 1);
        let err =
            save_frame_png_sync(target.to_string_lossy().into_owned(), oversize_b64).unwrap_err();
        assert!(matches!(
            err,
            AppError::Player { code, .. } if code == CODE_FRAME_TOO_LARGE
        ));
        assert!(!target.exists());
        assert!(!target.with_file_name("frame.png.tmp").exists());
    }

    #[test]
    fn overwriting_existing_file_succeeds() {
        // 授权模型(GA-fix #5):target_path 来自系统 save 对话框,覆盖确认由对话框承担——
        // 此处覆盖已存在文件应当允许,非拒绝项。
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("frame.png");
        fs::write(&target, b"old content").unwrap();
        let b64 = BASE64_STANDARD.encode(valid_png_bytes());
        save_frame_png_sync(target.to_string_lossy().into_owned(), b64).unwrap();
        let written = fs::read(&target).unwrap();
        assert_eq!(written, valid_png_bytes());
    }
}
