//! 图片简单编辑 IPC 命令(方案 C §6)。前台交互式单发任务(非 job/进度事件模型)——
//! 命令直接跑完整链路(解码 → 几何 → 编码 → 落盘 → 单文件 ingest)后返回终态,不像
//! backup/export 那样先返回 job_id 再靠事件轮询(编码耗时至多秒级,方案 §5 内存基准已实测)。

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::db::queries as q;
use crate::editing::entitlement::{self, EDITING_PLUGIN_ID, EDITING_SKU};
use crate::editing::geometry::{self, EditOps};
use crate::editing::{adjust, ingest, io as edit_io, memory_budget, metadata, naming, preview};
use crate::error::{AppError, Result};
use crate::exotic::PluginEntitlement;
use crate::state::{AppState, FILE_JOB_EDIT};

/// 与 backup/export 共用的门闩占用码(state.rs `file_job_owner` 文档:「A/B/C 共用」)。
const CODE_JOB_BUSY: &str = "file_job_busy";
const CODE_SOURCE_UNAVAILABLE: &str = "edit_source_unavailable";
const CODE_FORMAT_UNSUPPORTED: &str = "edit_format_unsupported";
const CODE_DECODE_FAILED: &str = "edit_decode_failed";
/// 单文件 ingest 失败但文件已落盘时,success 负载里携带的稳定标记(方案 §6 步骤 5:
/// 「不得返回普通失败诱导用户重复保存副本」)。与 error.rs 文档里的同名稳定码同族,
/// 但这里走**成功**响应的字段,不走 `AppError`。
const CODE_SAVED_NEEDS_INDEX: &str = "edit_saved_needs_index";

/// v1 输入承诺跨平台 `image` 解码主链可稳定处理的静态格式(方案 §3.2)。GIF(动画帧契约
/// 未定义)、HEIC/AVIF(依赖 Windows WIC,非跨平台一致)等一律落 `edit_format_unsupported`。
const SUPPORTED_INPUT_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif"];

/// 已准入的输出格式(方案 C-6:WebP 输出因元数据 spike 未通过,v1 不在此列——
/// 枚举里干脆没有该变体,类型层面就不可能选中它)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditFormat {
    Jpeg,
    Png,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditOutput {
    pub format: EditFormat,
    /// 仅 `Jpeg` 采用,默认 92(方案 §3.2);必须落在 1..=100,否则 `edit_invalid_ops`。
    pub quality: Option<u8>,
    /// 自定义文件名(v1 前端未提供改名 UI,恒传 `None`——字段为契约完整性预留,
    /// 已有单测覆盖 `Some` 分支行为)。
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum EditSaveResult {
    Saved {
        new_item_id: i64,
    },
    /// 文件已落盘、单文件 ingest 失败(方案 §6 步骤 5):`path_hint` 为新文件名(同目录,
    /// 不携带绝对路径),`recovery_code` 固定为 [`CODE_SAVED_NEEDS_INDEX`]——前端据此提示
    /// 「文件已保存,索引失败」并提供「立即扫描」,不诱导用户重复保存副本。
    SavedNeedsIndex {
        path_hint: String,
        recovery_code: String,
        /// 所在 scan_root id,供前端「立即扫描」按钮直接调用既有 rescan 命令。
        root_id: i64,
    },
}

/// 查询内建图片编辑高级功能的授权态。同步 keyring / 平台收据读取必须离开 async worker。
#[tauri::command]
pub async fn get_editing_entitlement(state: State<'_, Arc<AppState>>) -> Result<PluginEntitlement> {
    let provider = state.entitlement_provider();
    tokio::task::spawn_blocking(move || entitlement::editing_entitlement(provider.as_ref()))
        .await
        .map_err(|_| AppError::System("图片编辑授权查询异常终止".into()))
}

/// 激活内建编辑 feature。plugin id 与 SKU 均为后端可信常量，前端只提交 token。
#[tauri::command]
pub async fn activate_editing_feature(
    token: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let provider = state.entitlement_provider();
    tokio::task::spawn_blocking(move || {
        provider.activate(
            EDITING_PLUGIN_ID,
            EDITING_SKU,
            token.trim(),
            unix_now_secs(),
        )
    })
    .await
    .map_err(|_| AppError::System("图片编辑激活任务异常终止".into()))?
    .map_err(|e| AppError::Exotic {
        code: e.code(),
        message: format!("图片编辑激活失败：{}", e.code()),
    })?;
    Ok(())
}

fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 返回 E0 raw preview packet；二进制主体不走 serde JSON，前端解析固定包头后创建 blob URL。
#[tauri::command]
pub async fn get_edit_preview(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<tauri::ipc::Response> {
    let state_arc = state.inner().clone();
    let packet = tokio::task::spawn_blocking(move || run_get_edit_preview(&state_arc, item_id))
        .await
        .map_err(|_| AppError::System("编辑预览任务异常终止".into()))??;
    Ok(tauri::ipc::Response::new(packet))
}

/// 门闩释放 RAII 守卫:命令内部提前返回(`?`)也保证释放,不必在每个分支手写 `release_file_job`。
struct FileJobReleaseGuard {
    state: Arc<AppState>,
    owner: &'static str,
}

impl Drop for FileJobReleaseGuard {
    fn drop(&mut self) {
        self.state.release_file_job(self.owner);
    }
}

#[tauri::command]
pub async fn save_edited_image(
    item_id: i64,
    ops: EditOps,
    output: EditOutput,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<EditSaveResult> {
    // span 埋点(W1,D-312 info 档:图片编辑保存类,解码+几何+编码+落盘+ingest,真实 CPU/IO 工作)。
    let _span = crate::logging::SpanTimer::info("ipc:save_edited_image");
    if !state.try_acquire_file_job(FILE_JOB_EDIT) {
        return Err(AppError::Edit {
            code: CODE_JOB_BUSY,
            message: "已有文件任务在运行 | file job busy".into(),
        });
    }
    let guard = FileJobReleaseGuard {
        state: state.inner().clone(),
        owner: FILE_JOB_EDIT,
    };

    let state_arc = state.inner().clone();
    let join_result = tokio::task::spawn_blocking(move || {
        run_save_edited_image(&state_arc, item_id, &ops, &output)
    })
    .await;
    drop(guard); // 主链路已跑完(成功/失败均已确定)——尽快放行,不占后续 enrichment 触发的等待

    let (result, root_id) = join_result.map_err(|_| AppError::Edit {
        code: edit_io::CODE_IO,
        message: "编辑保存任务异常终止 | edit save task aborted".into(),
    })??;

    // 成功且已 ingest 入库:数据版本失效 + 后台单根 enrichment 是 fire-and-forget——
    // 不阻塞本命令返回(方案 §5:编辑保存须保持前台交互响应速度)。
    if let (EditSaveResult::Saved { .. }, Some(root_id)) = (&result, root_id) {
        state.bump_data_version();
        let app_bg = app.clone();
        let state_bg = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let cancel = tokio_util::sync::CancellationToken::new();
                crate::scanner::enricher::run_enrichment(
                    &app_bg,
                    &state_bg.db_writer,
                    root_id,
                    "edit",
                    "date",
                    "datetime",
                    "desc",
                    &cancel,
                )
            })
            .await;
        });
    }

    Ok(result)
}

/// 主链路(阻塞线程内执行):校验源 → 解码 → 几何 → 编码落盘 → 单文件 ingest。
/// 返回值第二项是成功 ingest 时的 `root_id`(供调用方触发后台 enrichment);ingest 失败
/// (partial)或整体失败时为 `None`。
fn run_save_edited_image(
    state: &AppState,
    item_id: i64,
    ops: &EditOps,
    output: &EditOutput,
) -> Result<(EditSaveResult, Option<i64>)> {
    // 后端真门放在阻塞主链入口：绕过前端直接 invoke 也无法使用付费能力。
    let provider = state.entitlement_provider();
    entitlement::require_editing_entitlement(provider.as_ref())?;
    // 在打开/解码源图前先拒绝非法角度；apply_geometry 会在纯函数边界再次校验，防止旁路调用。
    let fine_rotation = geometry::validate_ops(ops)?;

    let (detail, rel_path, root_id, volume_id) = {
        let conn = state.db_read_pool.get().map_err(AppError::from)?;
        let detail = q::get_media_detail(&conn, item_id)?;
        let (_, rel_path, _) = q::get_item_path_info(&conn, item_id)?;
        let (root_id, volume_id) =
            q::get_root_and_volume_for_directory(&conn, detail.item.directory_id)?;
        (detail, rel_path, root_id, volume_id)
    };

    if detail.item.is_deleted || detail.availability != "online" {
        return Err(AppError::Edit {
            code: CODE_SOURCE_UNAVAILABLE,
            message: "源文件当前不可用 | source file unavailable".into(),
        });
    }
    let ext = detail.item.file_format.to_ascii_lowercase();
    if !SUPPORTED_INPUT_EXTS.contains(&ext.as_str()) {
        return Err(AppError::Edit {
            code: CODE_FORMAT_UNSUPPORTED,
            message: "该格式不支持编辑 | format not supported for editing".into(),
        });
    }

    // ── 解码 + 内存预算前置校验(方案 §5:大块分配前拒绝)──────────────────────
    let reader = image::ImageReader::open(&detail.abs_path)
        .map_err(|_| AppError::Edit {
            code: CODE_SOURCE_UNAVAILABLE,
            message: "源文件读取失败 | failed to open source file".into(),
        })?
        .with_guessed_format()
        .map_err(|_| AppError::Edit {
            code: CODE_DECODE_FAILED,
            message: "无法识别图像格式 | unrecognized image format".into(),
        })?;
    let mut decoder = reader.into_decoder().map_err(|_| AppError::Edit {
        code: CODE_DECODE_FAILED,
        message: "解码失败 | decode failed".into(),
    })?;
    {
        use image::ImageDecoder;
        let (w, h) = decoder.dimensions();
        // orientation 与 90° 合并旋转只会交换宽高，不改变 expand 面积，因此这里可直接用
        // decoder 尺寸预测 fine rotate 最坏缓冲；任何溢出都按超预算拒绝。
        if memory_budget::exceeds_memory_budget(w, h)
            || memory_budget::exceeds_memory_budget_after_fine_rotate(w, h, fine_rotation)
        {
            return Err(AppError::Edit {
                code: memory_budget::CODE_TOO_LARGE,
                message: "图像超出内存预算 | image exceeds memory budget".into(),
            });
        }
    }
    let src_meta = metadata::read_source_metadata(&mut decoder);
    let img = image::DynamicImage::from_decoder(decoder).map_err(|_| AppError::Edit {
        code: CODE_DECODE_FAILED,
        message: "解码失败 | decode failed".into(),
    })?;

    // ── 几何(orientation 只应用一次 → 合并旋转 → flip → fine rotate → crop)────────
    let orientation = metadata::effective_source_orientation(&ext, src_meta.orientation);
    let out_img = geometry::apply_geometry(img, orientation, detail.item.view_rotation, ops)?;

    // ── E3 调色(D-107 链序最末;D-106:缺省/全零完全跳过,ICC 直通不变)────────────
    // 有 adjust 时像素经 CMS 转入 sRGB,输出按 §4.4 嵌显式 sRGB profile,不回写源 ICC。
    let (out_img, output_icc) = match adjust::effective_adjust(ops.adjust) {
        Some(adjust_ops) => {
            let adjusted =
                adjust::apply_adjust(out_img, src_meta.icc_profile.as_deref(), &adjust_ops)?;
            (adjusted, Some(adjust::srgb_profile_bytes()?))
        }
        None => (out_img, src_meta.icc_profile.clone()),
    };

    // ── 编码格式/质量 ─────────────────────────────────────────────────────
    let (out_format, out_ext) = match output.format {
        EditFormat::Jpeg => (edit_io::OutputFormat::Jpeg, "jpg"),
        EditFormat::Png => (edit_io::OutputFormat::Png, "png"),
    };
    let quality = match output.format {
        EditFormat::Jpeg => {
            let q = output.quality.unwrap_or(92);
            if q == 0 || q > 100 {
                return Err(AppError::Edit {
                    code: geometry::CODE_INVALID_OPS,
                    message: "JPEG 质量须在 1..=100 | jpeg quality out of range".into(),
                });
            }
            q
        }
        EditFormat::Png => 0,
    };

    // ── 目标命名 + 原子认领 + 落盘(同目录,方案 §3.2)──────────────────────────
    let dir = Path::new(&detail.abs_path)
        .parent()
        .ok_or_else(|| AppError::Edit {
            code: edit_io::CODE_IO,
            message: "目标目录解析失败 | failed to resolve target directory".into(),
        })?;
    let stem = naming::target_stem(output.file_name.as_deref(), &detail.item.file_name);
    let claimed = naming::claim_target_path(dir, &stem, out_ext)?;

    edit_io::write_edited_image(
        &claimed,
        &out_img,
        out_format,
        quality,
        output_icc.as_deref(),
        src_meta.date_time_original.as_deref(),
    )?;

    // ── 单文件 ingest(方案 §6 步骤 4)──────────────────────────────────────
    let stat = std::fs::metadata(&claimed).map_err(|_| AppError::Edit {
        code: edit_io::CODE_IO,
        message: "读取新文件属性失败 | failed to stat saved file".into(),
    })?;
    let file_mtime = stat
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let file_mtime_ns = stat
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0);
    let file_name = claimed
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let ingest_input = ingest::IngestInput {
        directory_id: detail.item.directory_id,
        rel_path_norm: &rel_path,
        file_name: &file_name,
        file_size: stat.len() as i64,
        file_mtime,
        file_mtime_ns,
        file_format: out_ext,
        width: i64::from(out_img.width()),
        height: i64::from(out_img.height()),
        volume_id,
    };

    let ingest_outcome = {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        ingest::ingest_single_file(&conn, &ingest_input)
    };

    match ingest_outcome {
        Ok(new_item_id) => Ok((EditSaveResult::Saved { new_item_id }, Some(root_id))),
        Err(_) => Ok((
            EditSaveResult::SavedNeedsIndex {
                path_hint: file_name,
                recovery_code: CODE_SAVED_NEEDS_INDEX.to_string(),
                root_id,
            },
            None,
        )),
    }
}

fn run_get_edit_preview(state: &AppState, item_id: i64) -> Result<Vec<u8>> {
    let provider = state.entitlement_provider();
    entitlement::require_editing_entitlement(provider.as_ref())?;

    let detail = {
        let conn = state.db_read_pool.get().map_err(AppError::from)?;
        q::get_media_detail(&conn, item_id)?
    };
    if detail.item.is_deleted || detail.availability != "online" {
        return Err(AppError::Edit {
            code: CODE_SOURCE_UNAVAILABLE,
            message: "源文件当前不可用 | source file unavailable".into(),
        });
    }
    let ext = detail.item.file_format.to_ascii_lowercase();
    if !SUPPORTED_INPUT_EXTS.contains(&ext.as_str()) {
        return Err(AppError::Edit {
            code: CODE_FORMAT_UNSUPPORTED,
            message: "该格式不支持编辑 | format not supported for editing".into(),
        });
    }

    let reader = image::ImageReader::open(&detail.abs_path)
        .map_err(|_| AppError::Edit {
            code: CODE_SOURCE_UNAVAILABLE,
            message: "源文件读取失败 | failed to open source file".into(),
        })?
        .with_guessed_format()
        .map_err(|_| AppError::Edit {
            code: CODE_DECODE_FAILED,
            message: "无法识别图像格式 | unrecognized image format".into(),
        })?;
    let mut decoder = reader.into_decoder().map_err(|_| AppError::Edit {
        code: CODE_DECODE_FAILED,
        message: "解码失败 | decode failed".into(),
    })?;
    {
        use image::ImageDecoder;
        let (width, height) = decoder.dimensions();
        if memory_budget::exceeds_memory_budget(width, height) {
            return Err(AppError::Edit {
                code: memory_budget::CODE_TOO_LARGE,
                message: "图像超出内存预算 | image exceeds memory budget".into(),
            });
        }
    }
    let source_metadata = metadata::read_source_metadata(&mut decoder);
    let image = image::DynamicImage::from_decoder(decoder).map_err(|_| AppError::Edit {
        code: CODE_DECODE_FAILED,
        message: "解码失败 | decode failed".into(),
    })?;
    let orientation = metadata::effective_source_orientation(&ext, source_metadata.orientation);
    preview::render_edit_preview(image, orientation, source_metadata.icc_profile.as_deref())
        .map(preview::EditPreview::into_packet)
}
