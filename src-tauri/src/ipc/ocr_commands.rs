//! OCR 文字提取（B′ 路线，2026-07-23）· IPC 命令层（T7）。
//!
//! 四命令：`ocr_status`（门控+安装态查询）/ `ocr_extract_image`（画廊图片识别）/
//! `ocr_extract_frame`（视频帧识别，前端 canvas 截帧回传 base64）/
//! `download_ocr_models`（照抄 `ai_commands::download_model` / `face_commands::download_face_model`
//! 姿态）。识别两命令共用 [`run_ocr_batch`]（`state.ai_worker.lock()` 全程在 `spawn_blocking`
//! 闭包内，不跨 `.await` 持锁）。
//!
//! **capabilities 结论（T7，不需改动）**：四命令都是经 `invoke_handler` 注册的 app 自有
//! command，`core:default` 权限即可覆盖，不引入新 Tauri 插件；`ocr_extract_frame` 消费的
//! canvas 截帧走前端 `<canvas>`/`toBlob`（非插件能力），复制走 `navigator.clipboard`
//! （`LogWindowView` 先例，非 `plugin-clipboard`）。`src-tauri/capabilities/` 无需新增条目。

use std::io::Write;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use exotic_protocol::{OcrItem, WorkerErrorCode};
use scrollery_ai_core::ocr_profile::{find_ocr_profile, ocr_profiles, DEFAULT_OCR_PROFILE_ID};
use serde::Serialize;
use tauri::State;

use crate::ai::ocr_registry::{ocr_assets, ocr_tier_installed};
use crate::ai::worker_client::{build_ocr_session_spec, OcrSessionSpec};
use crate::error::{AppError, Result};
use crate::exotic::validate::OcrItemOutcome;
use crate::exotic::Availability;
use crate::ipc::model_download::{download_assets, DownloadProgress};
use crate::state::AppState;

/// tokio `spawn_blocking` 的 JoinError 统一归为内部错误（同 `ai_commands::join_err`）。
fn join_err(e: tokio::task::JoinError) -> AppError {
    AppError::internal("后台任务异常 | blocking task failed", e)
}

/// 视频帧 base64 解码后目标上限（64MB，D-OCR-3/边界6）。
const OCR_FRAME_DECODED_MAX_BYTES: usize = 64 * 1024 * 1024;
/// base64 解码前预检上限（4/3 膨胀系数估算，同 `player_commands::FRAME_BASE64_MAX_BYTES` 姿态）。
const OCR_FRAME_BASE64_MAX_BYTES: usize = OCR_FRAME_DECODED_MAX_BYTES / 3 * 4 + 4;
/// 临时帧文件陈旧判定（边界13：崩溃残留最多滞留到下次调用清扫）。
const OCR_STALE_FRAME_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

/// 生成 `n_bytes` 随机字节的十六进制串（fingerprint nonce / 临时文件名，替代 uuid 依赖；
/// 仿 `logging::generate_session_id` 的 `ring::rand` 姿态，熵源故障时退化用纳秒时间戳）。
fn random_hex(n_bytes: usize) -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buf = vec![0u8; n_bytes];
    if SystemRandom::new().fill(&mut buf).is_err() {
        let nanos = chrono::Local::now().timestamp_subsec_nanos().to_be_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = nanos[i % nanos.len()];
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

// ── 门控 ─────────────────────────────────────────────────────────────────────

/// 门控 helper（D-OCR-6）：availability 非 Authorized 时映射稳定码。四态：
/// `AvailableUninstalled`（未激活）/ `InstalledUnlicensed`（catalog 误配安装态但未授权，
/// builtin offering 理论不落此态，防御性同归激活引导，2026-07-23 深审#2）→ `ocr_unlicensed`；
/// `LicenseExpired` → `ocr_license_expired`；其余（平台/版本/安装态不满足）→ `ocr_unavailable`。
/// message 不含绝对路径/内部串。
fn ensure_ocr_authorized(state: &AppState) -> Result<()> {
    let resolution = state.exotic_host().resolve_format("ocr");
    match resolution.availability {
        Availability::Authorized => Ok(()),
        Availability::LicenseExpired => Err(AppError::Ocr {
            code: "ocr_license_expired",
            message: "OCR 插件授权已过期，请续订".into(),
        }),
        Availability::AvailableUninstalled | Availability::InstalledUnlicensed => {
            Err(AppError::Ocr {
                code: "ocr_unlicensed",
                message: "OCR 插件尚未激活".into(),
            })
        }
        _ => Err(AppError::Ocr {
            code: "ocr_unavailable",
            message: "OCR 功能当前不可用".into(),
        }),
    }
}

/// 模型就位检查：档位四文件（det/cls/rec/dict）均存在且非空（sha 深校验留给 worker
/// `OcrSessionInit`）。不过即 `ocr_model_missing`（引导前往设置页下载）。
fn ensure_ocr_model_ready(spec: &OcrSessionSpec) -> Result<()> {
    if ocr_tier_installed(&spec.models_dir, &spec.profile) {
        Ok(())
    } else {
        Err(AppError::Ocr {
            code: "ocr_model_missing",
            message: "OCR 模型尚未下载，请前往设置页下载".into(),
        })
    }
}

// ── DTO ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrTierDto {
    pub id: String,
    pub display_name: String,
    pub size_mb: u32,
    pub installed: bool,
    /// 该档位下载清单是否就绪（`ocr_assets` 能否为该 profile 查到钉定的资产条目）；未就绪时
    /// 设置页禁用该档位的下载按钮（边界14）。2026-07-23 裁决 J14：由 `OcrStatusDto` 顶层单值
    /// 改为逐档字段——两档资产当前均钉定恒 true，但字段结构提前收敛到「哪档下线哪档禁用」，
    /// 不再靠前端复用 mobile 档的全局标志误判 server 档。
    pub manifest_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatusDto {
    pub availability: Availability,
    pub store_url: Option<String>,
    pub active_tier: String,
    pub tiers: Vec<OcrTierDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLineDto {
    pub text: String,
    pub quad: [[f32; 2]; 4],
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResultDto {
    pub lines: Vec<OcrLineDto>,
    pub width: u32,
    pub height: u32,
}

fn active_ocr_tier(state: &AppState) -> String {
    state
        .config
        .get("ocr_active_tier")
        .unwrap_or_else(|| DEFAULT_OCR_PROFILE_ID.to_string())
}

// ── worker 通路（识别两命令共用） ─────────────────────────────────────────────

/// 跑一次单项 OCR 批并把结果收敛为 [`OcrResultDto`]：
/// - `client.ocr_batch` 若已产出类型化 `AppError::Ocr`（如 `worker_client::ensure_ocr_session`
///   对 `ModelLoadFailed` 的类型化判别，2026-07-23 深审#1）→ 原样透传，不吞码。
/// - 其余硬止损错（进程/协议异常，MAX_ATTEMPTS 耗尽等笼统 `AppError`）→ `ocr_worker_failed`
///   （message 用户级文案，详情留 tracing，不透传 worker 内部串）。
/// - 单项 `Err{code}`：`IoError`/`MalformedInput` → `ocr_decode_failed`；`ResourceLimit`/
///   `InternalError` → `ocr_engine_failed`；其余（理论不可达的进程级码）同归 `ocr_worker_failed`。
fn run_ocr_batch(state: &AppState, spec: &OcrSessionSpec, item: OcrItem) -> Result<OcrResultDto> {
    let outcomes = {
        let mut client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
        client.ocr_batch(spec, std::slice::from_ref(&item), &|| false)
    }
    .map_err(|e| match e {
        // 已类型化的 OCR 错误(如 ocr_model_missing):原样透传,禁止在此层再做字符串匹配。
        AppError::Ocr { code, message } => AppError::Ocr { code, message },
        other => {
            tracing::warn!("OCR worker 通路失败:{other}");
            AppError::Ocr {
                code: "ocr_worker_failed",
                message: "OCR 处理失败，请重试".into(),
            }
        }
    })?;

    let outcome = outcomes.into_iter().next().ok_or_else(|| AppError::Ocr {
        code: "ocr_worker_failed",
        message: "OCR 批未返回结果".into(),
    })?;

    match outcome {
        OcrItemOutcome::Ok {
            lines,
            width,
            height,
        } => Ok(OcrResultDto {
            lines: lines
                .into_iter()
                .map(|l| OcrLineDto {
                    text: l.text,
                    quad: l.quad,
                    confidence: l.confidence,
                })
                .collect(),
            width,
            height,
        }),
        OcrItemOutcome::Err { code } => {
            let (app_code, msg): (&'static str, &str) = match code {
                WorkerErrorCode::IoError | WorkerErrorCode::MalformedInput => {
                    ("ocr_decode_failed", "图像读取或解码失败")
                }
                WorkerErrorCode::ResourceLimit | WorkerErrorCode::InternalError => {
                    ("ocr_engine_failed", "OCR 推理失败")
                }
                _ => ("ocr_worker_failed", "OCR 工作进程异常"),
            };
            Err(AppError::Ocr {
                code: app_code,
                message: msg.into(),
            })
        }
    }
}

// ── 命令 ─────────────────────────────────────────────────────────────────────

/// OCR 门控 + 安装态查询（设置页 OCR 分节 / 查看器入口按钮点击前判门共用）。
/// **不带 busy 字段**——交互忙态由前端 `useOcr` 单源持有（D-OCR-7 姿态：秒级一次性动作，
/// 不套 `derivationStore` 长任务轮询模式）。
#[tauri::command]
pub async fn ocr_status(state: State<'_, Arc<AppState>>) -> Result<OcrStatusDto> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<OcrStatusDto> {
        crate::official::entitlement(state.entitlement_provider().as_ref())?;
        let resolution = state.exotic_host().resolve_format("ocr");
        let models_dir = crate::ai::runtime_config::models_dir(&state);
        let tiers = ocr_profiles()
            .into_iter()
            .map(|p| {
                let installed = ocr_tier_installed(&models_dir, &p);
                // 逐档查询（J14）：不再复用 default_ocr_profile()（mobile 档）的单一结果代表全体。
                let manifest_ready = ocr_assets(&p).is_some();
                OcrTierDto {
                    id: p.id,
                    display_name: p.display_name,
                    size_mb: p.size_mb,
                    installed,
                    manifest_ready,
                }
            })
            .collect();
        Ok(OcrStatusDto {
            availability: resolution.availability,
            store_url: resolution.store_url,
            active_tier: active_ocr_tier(&state),
            tiers,
        })
    })
    .await
    .map_err(join_err)?
}

/// 画廊图片 OCR 提取：门控 → 模型就位 → 按 `item_id` 查源路径（复用 `get_item_path_info`，
/// 与 reveal/预览命令同款查询，rusqlite 参数已绑定）→ canonicalize + 存在性校验 → worker 通路。
#[tauri::command]
pub async fn ocr_extract_image(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<OcrResultDto> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<OcrResultDto> {
        ensure_ocr_authorized(&state)?;
        let spec = build_ocr_session_spec(&state)?;
        ensure_ocr_model_ready(&spec)?;

        let conn = state.db_read_pool.get()?;
        let (root, rel, name) = crate::db::queries::get_item_path_info(&conn, item_id)?;
        drop(conn);
        let abs_path = crate::utils::path::resolve_media_path(&root, &rel, &name);
        let canon = std::path::Path::new(&abs_path)
            .canonicalize()
            .map_err(|_| AppError::MediaNotFound(item_id))?;

        let item = OcrItem {
            item_id,
            cache_key: None,
            source_path: Some(canon.to_string_lossy().into_owned()),
            fingerprint: random_hex(8),
        };
        run_ocr_batch(&state, &spec, item)
    })
    .await
    .map_err(join_err)?
}

/// 视频帧 OCR 提取（D-OCR-3）：前端 canvas 截帧回传 base64 PNG，host 落临时文件
/// （`*.tmp` 写 + 同卷 rename）后走同一 worker 通路，用完即删（finally 语义，成败都删，
/// best-effort warn）。命令开头先清 `{cache_dir}/ocr_frames/` 下 mtime>1h 的陈旧文件
/// （边界13：崩溃残留最多滞留到下次调用）。
#[tauri::command]
pub async fn ocr_extract_frame(
    data_base64: String,
    state: State<'_, Arc<AppState>>,
) -> Result<OcrResultDto> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || -> Result<OcrResultDto> {
        ensure_ocr_authorized(&state)?;
        let spec = build_ocr_session_spec(&state)?;
        ensure_ocr_model_ready(&spec)?;

        if data_base64.len() > OCR_FRAME_BASE64_MAX_BYTES {
            return Err(AppError::Ocr {
                code: "ocr_invalid_input",
                message: "帧数据超过大小上限".into(),
            });
        }
        let bytes = BASE64_STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| AppError::Ocr {
                code: "ocr_invalid_input",
                message: "帧数据 base64 解码失败".into(),
            })?;
        if bytes.len() > OCR_FRAME_DECODED_MAX_BYTES {
            return Err(AppError::Ocr {
                code: "ocr_invalid_input",
                message: "帧数据超过大小上限".into(),
            });
        }

        let cache_dir = state
            .thumb_config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .cache_dir
            .clone();
        let frames_dir = cache_dir.join("ocr_frames");
        std::fs::create_dir_all(&frames_dir)?;
        purge_stale_ocr_frames(&frames_dir);

        let file_name = format!("{}.png", random_hex(16));
        let final_path = frames_dir.join(&file_name);
        let tmp_path = frames_dir.join(format!("{file_name}.tmp"));
        let write_result: std::io::Result<()> = (|| {
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(&bytes)?;
            f.sync_all()
        })();
        if write_result.is_err() || std::fs::rename(&tmp_path, &final_path).is_err() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(AppError::Ocr {
                code: "ocr_decode_failed",
                message: "临时帧文件落盘失败".into(),
            });
        }

        let item = OcrItem {
            item_id: 0,
            cache_key: None,
            source_path: Some(final_path.to_string_lossy().into_owned()),
            fingerprint: random_hex(8),
        };
        let result = run_ocr_batch(&state, &spec, item);
        // finally 语义：成败都删,best-effort（残留文件已在下次调用被 purge_stale_ocr_frames 兜底）。
        if let Err(e) = std::fs::remove_file(&final_path) {
            tracing::warn!("OCR 临时帧文件清理失败:{e}");
        }
        result
    })
    .await
    .map_err(join_err)?
}

/// 清理 `frames_dir` 下 mtime 超过 [`OCR_STALE_FRAME_AGE`] 的文件（best-effort，扫描/删除
/// 失败均静默跳过——非关键路径，不得阻塞正常识别请求）。
fn purge_stale_ocr_frames(frames_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(frames_dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let is_stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > OCR_STALE_FRAME_AGE);
        if is_stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 下载 OCR 档位模型（照抄 `ai_commands::download_model` / `face_commands::download_face_model`
/// 姿态）：`ocr_assets` 查不到该 profile 的钉定资产时恒 `None` → `ocr_manifest_unready`
/// （边界14，fail-closed；2026-07-23 起两档均已钉定，正常路径不会触发）。
///
/// **有意不过 `ensure_ocr_authorized` 门（J8 认领，2026-07-23 深审）**：本命令只落模型字节，
/// 不触发识别。允许「先下后购」——未授权用户可预下载模型，购买/激活后零等待即可用（识别态
/// 提示见 `OcrModelSection.vue` 顶部授权态提示）。`ensure_ocr_authorized` 保护的是识别能力
/// （`ocr_extract_image`/`ocr_extract_frame` 已过此门），不是模型字节本身——PP-OCRv5 是公开
/// ONNX 权重，加门也拦不住用户自行下载，门在这里防不住东西、只会拖慢正当的预下载体验。
/// 后续 review 请勿再对此报「授权命令遗漏门控」。
#[tauri::command]
pub async fn download_ocr_models(
    tier: String,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let profile = find_ocr_profile(&tier).ok_or_else(|| AppError::Ocr {
        code: "ocr_invalid_input",
        message: format!("未知 OCR 档位:{tier}"),
    })?;
    let assets = ocr_assets(&profile).ok_or_else(|| AppError::Ocr {
        code: "ocr_manifest_unready",
        message: "OCR 模型下载清单尚未就绪".into(),
    })?;

    let models = crate::ai::runtime_config::models_dir(&state);
    std::fs::create_dir_all(&models)?;

    // 首选下载源：mirror=国内镜像优先；其它=官方优先（复用 CLIP/人脸约定）。
    let mirror_first = state.config.get("ai_download_source").as_deref() == Some("mirror");
    let client = crate::download::secure_client(crate::download::TimeoutPolicy::LargeFile)
        .map_err(|e| AppError::internal("HTTP 客户端构建失败 | client build failed", e))?;

    download_assets(
        &client,
        &models,
        &assets,
        mirror_first,
        &on_progress,
        &profile.id,
    )
    .await
    .map_err(AppError::System)
}
