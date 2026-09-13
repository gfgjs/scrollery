// src-tauri/src/ai/runtime_config.rs
//! AI 运行时配置真相(U-P2-a,2026-07-16 从 IPC 层 AI 命令文件下沉;
//! 本文件注释避写旧路径名——U 设计 §4.2 依赖方向门按裸词扫描本目录)。
//!
//! 「当前激活模型/人脸模型/模型目录/provider 回声」这些跨层共享的解析函数此前寄居
//! IPC 命令文件,导致 `ai/worker_client`、`ai/face_pipeline`、`ai/worker_pipeline`、
//! `ai/pipeline` 四个核心模块反向 import IPC 层(层次倒挂)。下沉后依赖方向恢复:
//! ipc → ai,核心层内部横向引用。函数体逐行原样搬迁,签名与行为不变。

use std::path::PathBuf;

use tracing::info;

use crate::ai::profile::{self, ModelProfile};
use crate::db::queries::{get_config, set_config};
use crate::state::AppState;

/// 从应用数据获取模型目录。
pub(crate) fn models_dir(state: &AppState) -> PathBuf {
    // 从 app_data_dir 真值派生,不得用 log_dir.parent() 反推——log_dir 用户可配置,
    // 改日志目录后反推值漂移会令已下载模型「消失」(2026-07-10 审查 A1)。
    state.app_data_dir.join("models")
}

/// `ai_backend` 配置退役(T16):行为恒 worker,读到遗留非 worker 值仅提示日志
/// (保键忽略值,不做 schema 迁移;一个版本周期后随例行清理删键)。
pub(crate) fn warn_legacy_ai_backend(state: &AppState) {
    let legacy = state
        .db_read_pool
        .get()
        .ok()
        .and_then(|conn| get_config(&conn, "ai_backend").ok().flatten());
    if let Some(v) = legacy {
        if v != "worker" {
            info!("配置 ai_backend={v} 已退役:推理恒经 ai-worker 子进程(T16),该值被忽略");
        }
    }
}

/// worker 会话的 provider/gpu_name 回声落库(T16):EP 探测只发生在 worker 侧,host 写回
/// 既有 `ai_provider`/`ai_gpu_name` 键——status 命令读法零改动;无会话/旧帧保留旧值。
pub(crate) fn persist_provider_echo(state: &AppState) {
    let echo = {
        let client = state.ai_worker.lock().unwrap_or_else(|p| p.into_inner());
        client.session().and_then(|d| {
            d.provider
                .clone()
                .map(|p| (p, d.gpu_name.clone().unwrap_or_default()))
        })
    };
    if let Some((provider, gpu_name)) = echo {
        let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = set_config(&conn, "ai_provider", &provider);
        let _ = set_config(&conn, "ai_gpu_name", &gpu_name);
    }
}

/// Resolve the currently-active model profile from config (`ai_active_model`), falling back
/// to the default. The id is also the `ai_embeddings.model_name` key for this model's vectors.
/// 从配置（`ai_active_model`）解析当前激活的模型 profile，缺省回退默认。该 id 同时是本模型
/// 向量在 `ai_embeddings.model_name` 的键。
///
/// A2:`ai_active_model`/`ai_active_image_file` 是 schema 设置类键,唯一真源已切到
/// `ConfigManager`——不再需要读池连接,`active_profile_with` 的 `conn` 参数随之改为
/// `&ConfigManager`。
pub(crate) fn active_profile(state: &AppState) -> ModelProfile {
    active_profile_with(&state.config)
}

/// A2 前称「连接已在手的变体」——`ai_active_model`/`ai_active_image_file` 迁往 config.toml 后
/// 不再需要 DB 连接,改收 `&ConfigManager`。
pub(crate) fn active_profile_with(config: &crate::config::ConfigManager) -> ModelProfile {
    // 现在「激活模型」由两段配置组成：`ai_active_model`=架构 id（= 向量空间主键），
    // `ai_active_image_file`=选中的图像 onnx 变体文件名（决定加载哪份图像塔，不改向量身份）。
    // image_file 缺省时由 resolve_profile 取该架构的 dyn/fp16 缺省变体；schema 默认值为空
    // 字符串（=未覆盖），须过滤成 None 才能落到「取该架构缺省变体」分支。
    let arch_id = config.get("ai_active_model");
    let image_file = config.get("ai_active_image_file").filter(|s| !s.is_empty());
    arch_id
        .as_deref()
        .and_then(|a| profile::resolve_profile(a, image_file.as_deref()))
        .unwrap_or_else(profile::default_profile)
}

/// Resolve the active face model profile from config (`face_model_active`), default fallback.
/// Returns `None` when face feature is disabled (`face_enabled=0`) → engine skips loading face
/// sessions entirely (saves load time/VRAM). The id is also the `faces.model_name` vector-space key.
/// 从配置（`face_model_active`）解析当前激活的人脸模型 profile，缺省回退默认。人脸功能关闭
/// （`face_enabled=0`）时返回 `None`，引擎完全跳过加载人脸 session（省加载时间/显存）。
/// 该 id 同时是人脸向量在 `faces.model_name` 的键。
///
/// A2:`face_enabled`/`face_model_active` 是 schema 设置类键,唯一真源已切到 `ConfigManager`。
pub(crate) fn active_face_profile(
    state: &AppState,
) -> Option<crate::ai::face_profile::FaceProfile> {
    active_face_profile_with(&state.config)
}

/// A2 前称「连接已在手的变体」——两键均已迁往 config.toml,改收 `&ConfigManager`。
///
/// **布尔编码变更**:config.toml 的 `Bool` 类型恒渲染为 `"true"`/`"false"`(见
/// `config::schema::SettingKind::Bool`),不再是历史 DB 的 `"0"`/`"1"`(`migrate.rs` 一次性
/// 迁移已把旧值归一化)——判定条件相应改为 `== Some("false")`。
pub(crate) fn active_face_profile_with(
    config: &crate::config::ConfigManager,
) -> Option<crate::ai::face_profile::FaceProfile> {
    use crate::ai::face_profile;
    if config.get("face_enabled").as_deref() == Some("false") {
        return None;
    }
    let id = config
        .get("face_model_active")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| face_profile::DEFAULT_FACE_PROFILE_ID.to_string());
    Some(face_profile::find_face_profile(&id).unwrap_or_else(face_profile::default_face_profile))
}

/// Whether a specific image-encoder variant is fully usable on disk: image onnx header + its
/// external-data weights, the shared text encoder + its weights, and the vocab. Existence-only — a
/// present-but-wrong-size file still counts (download re-fetches/repairs via size + sha256 checks).
/// 某个图像编码器变体是否已就位可用：图像 onnx 头 + 其外部权重、共享文本塔 + 其权重、词表。
/// 仅按存在判定 —— 存在但大小不符仍算已装（下载命令会按 大小+sha256 校验并按需重拉/修复）。
pub(crate) fn variant_installed(
    models_dir: &std::path::Path,
    image_file: &str,
    text_file: &str,
) -> bool {
    let needed = [
        image_file.to_string(),
        format!("{image_file}.extra_file"),
        text_file.to_string(),
        format!("{text_file}.extra_file"),
        "vocab.txt".to_string(),
    ];
    needed.iter().all(|f| models_dir.join(f).exists())
}
