//! 模型资产下载的 IPC 侧编排(U-P2-b,2026-07-16 从 ipc/ai_commands.rs 迁出)。
//!
//! CLIP `download_model` 与人脸 `download_face_model` 共享的 Channel 下载编排:
//! 进度 DTO、逐资产循环、落盘名安全白名单。此前寄居单一命令文件,兄弟命令文件
//! 跨文件借用;独立成模块后两命令文件对称引用。
//!
//! 层次边界(U-D-003):本模块持 `tauri::ipc::Channel`(IPC transport),**留在 ipc 层**;
//! 通用 `crate::download` 只管传输机制(HTTPS/Range/镜像/校验),不得反向引入 Tauri IPC。

use tracing::info;

use crate::ai::profile;

/// 下载期间经 `Channel` 流式推给前端的进度事件。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model_id: String,
    /// 当前正在下载的文件（最终 `done` 事件时为空）。
    pub current_file: String,
    /// 当前文件序号（从 1 开始）。
    pub file_index: usize,
    pub file_count: usize,
    /// 迄今所有资产累计已接收字节数。
    pub received: u64,
    /// 全部资产的总字节数。
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// 把一组固定资产下载到 `models`：逐文件断点续传（HTTP Range）、镜像回退、size+sha256 校验、
/// `.part`→原子改名、节流进度经 `on_progress`。CLIP 的 download_model 与人脸的 download_face_model
/// 共用——上游差异仅在如何构建资产清单（在线发现 vs profile 静态资产）。
pub(crate) async fn download_assets(
    client: &reqwest::Client,
    models: &std::path::Path,
    assets: &[profile::ModelAsset],
    mirror_first: bool,
    on_progress: &tauri::ipc::Channel<DownloadProgress>,
    download_id: &str,
) -> std::result::Result<(), String> {
    let total: u64 = assets.iter().map(|a| a.size_bytes).sum();
    let file_count = assets.len();

    let send = |current_file: &str,
                file_index: usize,
                received: u64,
                done: bool,
                error: Option<String>| {
        let _ = on_progress.send(DownloadProgress {
            model_id: download_id.to_string(),
            current_file: current_file.to_string(),
            file_index,
            file_count,
            received,
            total,
            done,
            error,
        });
    };

    let mut base_received: u64 = 0; // 已完成文件累计字节数

    for (i, asset) in assets.iter().enumerate() {
        let idx = i + 1;
        // 落盘名净化(2026-07-06 审查 P1-9):dest 取自**未签名**的 HF/镜像 tree JSON(用户可切
        // hf-mirror.com),必须校验为单路径分量,否则恶意/被劫持镜像可借 "../.." 获得 models
        // 目录外任意写。exotic 侧对**已签名**内容尚且逐一校验,此处对未签名第三方内容更须设防。
        if !is_safe_model_file_name(&asset.dest) {
            let msg = format!(
                "非法模型文件名，拒绝下载 | unsafe asset name: {}",
                asset.dest
            );
            send(&asset.dest, idx, base_received, false, Some(msg.clone()));
            return Err(msg);
        }
        let dest = models.join(&asset.dest);
        // 纵深防御:即便白名单有漏,join 后必须仍在 models 目录内。
        if !dest.starts_with(models) {
            let msg = format!("路径逃逸，拒绝 | path escapes models dir: {}", asset.dest);
            send(&asset.dest, idx, base_received, false, Some(msg.clone()));
            return Err(msg);
        }

        // 跳过已存在、大小正确、且（若已知）sha256 匹配的文件。
        // R1-3：sha256 校验要整读文件（模型可达 GB 级），下沉 blocking，别拖垮 tokio worker。
        let already_ok = {
            let dest = dest.clone();
            let expect_size = asset.size_bytes;
            let sha = asset.sha256.clone();
            tokio::task::spawn_blocking(move || {
                std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) == expect_size
                    && crate::download::sha256_matches(&dest, sha.as_deref())
            })
            .await
            .map_err(|e| format!("后台任务异常 | blocking task failed: {e}"))?
        };
        if already_ok {
            base_received += asset.size_bytes;
            send(&asset.dest, idx, base_received, false, None);
            continue;
        }

        // A15(P 线):.part 的 metadata/remove/rename 改 tokio::fs——async fn 里的同步
        // fs 会占住 tokio worker(HDD/网络卷上一次 stat 可达毫秒级,下载循环逐文件反复付)。
        let part = models.join(format!("{}.part", asset.dest));
        let mut resume_from = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        // 残留 .part 超过目标大小 → 重新开始。
        if resume_from > asset.size_bytes {
            let _ = tokio::fs::remove_file(&part).await;
            resume_from = 0;
        }

        // 按用户偏好排序候选源：首选源在前，另一源作为失败回退在后。
        let mut urls: Vec<&str> = Vec::with_capacity(2);
        let mirror = asset.mirror_url.as_deref();
        if mirror_first {
            if let Some(m) = mirror {
                urls.push(m);
            }
            urls.push(asset.url.as_str());
        } else {
            urls.push(asset.url.as_str());
            if let Some(m) = mirror {
                urls.push(m);
            }
        }

        // R10：单文件流式下载（Range 续传）+ 镜像回退下沉通用引擎；进度回调把「本文件已收字节」
        // 聚合到全局 received（base_received = 已完成文件累计）。
        let on_bytes = |file_received: u64| {
            send(&asset.dest, idx, base_received + file_received, false, None);
        };
        if let Err(e) = crate::download::download_with_fallback(
            client,
            &urls,
            &part,
            resume_from,
            asset.size_bytes,
            &on_bytes,
        )
        .await
        {
            let msg = format!("下载失败 {} | download failed: {}", asset.dest, e);
            send(&asset.dest, idx, base_received, false, Some(msg.clone()));
            return Err(msg);
        }

        // 校验大小，再校验 sha256（若已知）。
        let got = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if got != asset.size_bytes {
            let _ = tokio::fs::remove_file(&part).await;
            let msg = format!(
                "{} 大小校验失败：期望 {} 实得 {} | size mismatch",
                asset.dest, asset.size_bytes, got
            );
            send(&asset.dest, idx, base_received, false, Some(msg.clone()));
            return Err(msg);
        }
        // R1-3：同上——下载后整文件 sha256 下沉 blocking。
        let sha_ok = if asset.sha256.is_some() {
            let part_c = part.clone();
            let sha = asset.sha256.clone();
            tokio::task::spawn_blocking(move || {
                crate::download::sha256_matches(&part_c, sha.as_deref())
            })
            .await
            .map_err(|e| format!("后台任务异常 | blocking task failed: {e}"))?
        } else {
            true
        };
        if !sha_ok {
            let _ = tokio::fs::remove_file(&part).await;
            let msg = format!(
                "{} sha256 校验失败（文件损坏或被篡改）| checksum mismatch",
                asset.dest
            );
            send(&asset.dest, idx, base_received, false, Some(msg.clone()));
            return Err(msg);
        }

        // 原子式就位。
        let _ = tokio::fs::remove_file(&dest).await;
        tokio::fs::rename(&part, &dest)
            .await
            .map_err(|e| e.to_string())?;
        base_received += asset.size_bytes;
        send(&asset.dest, idx, base_received, false, None);
    }

    send("", file_count, total, true, None);
    info!(
        "Model downloaded: {} ({} files) | 模型下载完成: {}（{} 个文件）",
        download_id, file_count, download_id, file_count
    );
    Ok(())
}

/// 模型落盘文件名白名单(2026-07-06 审查 P1-9):单路径分量,`[A-Za-z0-9._-]`,1-128 字节,
/// 非 "."/".."（顺带排除分隔符/盘符/父引用/NUL）。名字来自未签名的第三方 tree JSON,从紧设防。
fn is_safe_model_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

// `download_file`（单文件流式下载 + Range 续传）与 `sha256_matches` 已下沉 `crate::download` 通用
// 引擎（R10，Part6 §3.1.2），与 exotic 共用；此处不再重复实现。

#[cfg(test)]
mod dest_safety_tests {
    use super::is_safe_model_file_name;

    #[test]
    fn accepts_real_model_filenames() {
        for ok in [
            "vision_model.onnx",
            "model.onnx.extra_file",
            "clip_cn_vit-l-14-336.fp16.onnx",
            "text_model.onnx",
        ] {
            assert!(is_safe_model_file_name(ok), "应接受: {ok}");
        }
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for bad in [
            "",
            ".",
            "..",
            "../evil",
            "..\\evil",
            "a/b",
            "a\\b",
            "C:evil",
            "with space.onnx",
            "nul\0byte",
        ] {
            assert!(!is_safe_model_file_name(bad), "应拒绝: {bad:?}");
        }
    }
}
