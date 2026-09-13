//! 影像增强（降噪/超分子系统 design.md §G）· IPC 命令层（P0 批 4）。
//!
//! 七命令：`enhance_status`（门控+模型态查询）/ `download_enhance_model`（复用
//! `download_assets`）/ `delete_enhance_model` / `enhance_preview`（单 tile 前后对比）/
//! `enhance_start`（付费门控 + 入队 + 后台驱动）/ `enhance_cancel` / `get_enhance_queue`。
//!
//! **capabilities 结论（核验，无需改动）**：七命令均为经 `invoke_handler` 注册的 app 自有
//! command，`core:default` 权限即覆盖（同 OCR 先例 T7，plugin-store-map §6），不引入新
//! Tauri 插件；`src-tauri/capabilities/` 无需新增条目。
//!
//! 错误全走 `AppError::Enhance { code, message }`（serde::Serialize + 稳定 code），不泄内部串。

use std::sync::Arc;

use scrollery_ai_core::enhance_profile::enhance_profiles;
use serde::Serialize;
use tauri::State;

use crate::enhance::registry::{enhance_assets, enhance_manifest_ready, enhance_model_installed};
use crate::enhance::service::{validate_strengths, PreviewPaths};
use crate::enhance::{EnhanceParams, JobDto};
use crate::error::{AppError, Result};
use crate::exotic::Availability;
use crate::ipc::model_download::{download_assets, DownloadProgress};
use crate::state::AppState;

/// `spawn_blocking` 的 JoinError 统一归内部错误。
fn join_err(e: tokio::task::JoinError) -> AppError {
    AppError::internal("后台任务异常 | blocking task failed", e)
}

/// 门控 helper（付费门控，D-OCR-6 同型）：availability 非 Authorized → 稳定码。
fn ensure_enhance_authorized(state: &AppState) -> Result<()> {
    match state.exotic_host().resolve_format("enhance").availability {
        Availability::Authorized => Ok(()),
        Availability::LicenseExpired => Err(AppError::Enhance {
            code: "enhance_unlicensed",
            message: "影像增强授权已过期，请续订".into(),
        }),
        _ => Err(AppError::Enhance {
            code: "enhance_unlicensed",
            message: "影像增强插件尚未激活".into(),
        }),
    }
}

// ── DTO ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhanceModelDto {
    pub id: String,
    /// 任务种类字符串（denoise / dejpegArtifact / upscale）。
    pub task: String,
    pub scale: u32,
    pub installed: bool,
    /// 该档下载清单是否已钉定 URL、字节数与 sha256。
    pub manifest_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhanceStatusDto {
    pub availability: Availability,
    pub store_url: Option<String>,
    /// 当前推理执行提供器回声（worker 会话就绪后写回；P0 未起会话时为 None）。
    pub provider: Option<String>,
    pub models: Vec<EnhanceModelDto>,
}

// ── 命令 ─────────────────────────────────────────────────────────────────────

/// 门控 + 模型态查询（设置页「影像增强」分节 / 入口按钮判门共用）。
#[tauri::command]
pub async fn enhance_status(state: State<'_, Arc<AppState>>) -> Result<EnhanceStatusDto> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<EnhanceStatusDto> {
        let resolution = state.exotic_host().resolve_format("enhance");
        let models_dir = crate::ai::runtime_config::models_dir(&state);
        let models = enhance_profiles()
            .into_iter()
            .map(|p| EnhanceModelDto {
                installed: enhance_model_installed(&models_dir, &p.id),
                manifest_ready: enhance_manifest_ready(&p.id),
                task: serde_json::to_value(p.task)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                scale: p.scale,
                id: p.id,
            })
            .collect();
        let provider = state.config.get("ai_provider");
        Ok(EnhanceStatusDto {
            availability: resolution.availability,
            store_url: resolution.store_url,
            provider,
            models,
        })
    })
    .await
    .map_err(join_err)?
}

/// 下载某增强模型档位（照 `download_ocr_models` 姿态；复用 `download_assets`，P0 批 4 #11）。
/// **不过授权门**（先下后购，同 OCR download 命令裁决）：只落模型字节，不触发增强。
#[tauri::command]
pub async fn download_enhance_model(
    model_id: String,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let assets = enhance_assets(&model_id).ok_or_else(|| AppError::Enhance {
        code: "enhance_model_missing",
        message: format!("未知增强模型档位:{model_id}"),
    })?;
    if !enhance_manifest_ready(&model_id) {
        return Err(AppError::Enhance {
            code: "enhance_manifest_unready",
            message: "增强模型下载清单尚未就绪".into(),
        });
    }

    let models = crate::ai::runtime_config::models_dir(&state);
    std::fs::create_dir_all(&models).map_err(|_| AppError::Enhance {
        code: "enhance_io",
        message: "模型目录创建失败".into(),
    })?;

    let mirror_first = state.config.get("ai_download_source").as_deref() == Some("mirror");
    let client = crate::download::secure_client(crate::download::TimeoutPolicy::LargeFile)
        .map_err(|_| AppError::Enhance {
            code: "enhance_download_failed",
            message: "HTTP 客户端构建失败".into(),
        })?;

    download_assets(
        &client,
        &models,
        &assets,
        mirror_first,
        &on_progress,
        &model_id,
    )
    .await
    .map_err(|_| AppError::Enhance {
        code: "enhance_download_failed",
        message: "增强模型下载失败，请重试".into(),
    })
}

/// 删除某增强模型档位的两文件（fp32 + fp16）。缺文件不算错误（幂等）。
#[tauri::command]
pub async fn delete_enhance_model(model_id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let profile = scrollery_ai_core::enhance_profile::find_enhance_profile(&model_id)
            .ok_or_else(|| AppError::Enhance {
                code: "enhance_model_missing",
                message: format!("未知增强模型档位:{model_id}"),
            })?;
        let models_dir = crate::ai::runtime_config::models_dir(&state);
        for filename in [&profile.file_fp32, &profile.file_fp16] {
            let path = models_dir.join(filename);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|_| AppError::Enhance {
                    code: "enhance_io",
                    message: "删除模型文件失败".into(),
                })?;
            }
        }
        Ok(())
    })
    .await
    .map_err(join_err)?
}

/// 前后对比预览（design.md §F）：付费门控 → 参数校验 → 裁中心（或 `point` 归一化点位为
/// 中心）512² 单 tile 跑一次增强 → 返回 before/after 固定名文件路径（前端 convertFileSrc
/// 消费）。worker 正忙（job 执行中）→ `enhance_busy`；不入队列、不 claim、不 ingest。
///
/// 注：稳定码 `enhance_not_implemented` 已随本实现退役（不再产出），保留为可能的兼容占位。
#[tauri::command]
pub async fn enhance_preview(
    item_id: i64,
    point: Option<(f64, f64)>,
    params: EnhanceParams,
    state: State<'_, Arc<AppState>>,
) -> Result<PreviewPaths> {
    let state = Arc::clone(&state);
    // 付费门控（后端真门，同 enhance_start）。
    {
        let gate = Arc::clone(&state);
        tokio::task::spawn_blocking(move || ensure_enhance_authorized(&gate))
            .await
            .map_err(join_err)??;
    }
    validate_strengths(&params.steps)?;
    tokio::task::spawn_blocking(move || {
        state
            .enhance_service
            .preview(&state, item_id, point, &params)
    })
    .await
    .map_err(join_err)?
}

/// 发起增强（多选入队，顺序执行）：付费门控 → 参数校验 → 入队 → 后台驱动，返回 job_id。
#[tauri::command]
pub async fn enhance_start(
    app: tauri::AppHandle,
    item_ids: Vec<i64>,
    params: EnhanceParams,
    state: State<'_, Arc<AppState>>,
) -> Result<u64> {
    let state = Arc::clone(&state);
    // 付费门控（后端真门：绕过前端直接 invoke 亦拦）。
    {
        let state_gate = Arc::clone(&state);
        tokio::task::spawn_blocking(move || ensure_enhance_authorized(&state_gate))
            .await
            .map_err(join_err)??;
    }
    if item_ids.is_empty() {
        return Err(AppError::Enhance {
            code: "enhance_invalid_params",
            message: "未选择任何图片".into(),
        });
    }
    // strength 有限性校验（NaN/Inf 拒）。
    validate_strengths(&params.steps)?;

    // RAW 同步准入（深审 g，批 5 复核）：入队前拦 RAW，避免产生死 job，前端 submit 直接
    // 收到稳定码引导。RAW 集单源 catalog `exotic-image-raw`。超尺寸/解码类维持异步。
    {
        let s = Arc::clone(&state);
        let ids = item_ids.clone();
        let has_raw = tokio::task::spawn_blocking(move || {
            ids.iter()
                .any(|&id| crate::enhance::service::item_is_raw(&s, id))
        })
        .await
        .map_err(join_err)?;
        if has_raw {
            return Err(AppError::Enhance {
                code: "enhance_input_unsupported",
                message: "RAW 源暂不支持增强".into(),
            });
        }
    }

    // enqueue 内 tiles_total 估算读 DB（db_read_pool.get + 逐 item get_media_detail）——
    // 与本命令其余门/RAW 判一致，同步 rusqlite 必须离开 async 命令线程（仓级硬约束）。
    let job_id = {
        let enq_state = Arc::clone(&state);
        tokio::task::spawn_blocking(move || {
            enq_state
                .enhance_service
                .enqueue(&enq_state, item_ids, params)
        })
        .await
        .map_err(join_err)?
    };

    // 后台驱动（run_job_blocking 内持 worker 锁串行；GPU 令牌 D2 顺序在服务内保证）。
    let svc_state = Arc::clone(&state);
    let app_bg = app.clone();
    tokio::task::spawn_blocking(move || {
        svc_state
            .enhance_service
            .run_job_blocking(&app_bg, &svc_state, job_id);
    });

    Ok(job_id)
}

/// 取消某 job（置 token → 在途 run_request/limiter 醒来即中止 + supervisor kill）。
#[tauri::command]
pub async fn enhance_cancel(job_id: u64, state: State<'_, Arc<AppState>>) -> Result<()> {
    state.enhance_service.cancel(job_id);
    Ok(())
}

/// 队列全量快照（前端轮询/事件后拉取）。
#[tauri::command]
pub async fn get_enhance_queue(state: State<'_, Arc<AppState>>) -> Result<Vec<JobDto>> {
    Ok(state.enhance_service.queue_snapshot())
}
