// src-tauri/src/ai/runtime_config.rs
//! AI 运行时配置：解析激活模型、人脸模型、模型目录及 worker 的 provider 回声。

use std::path::PathBuf;

use crate::ai::profile::{self, ModelProfile};
use crate::db::queries::set_config;
use crate::state::AppState;

/// 从应用数据获取模型目录。
pub(crate) fn models_dir(state: &AppState) -> PathBuf {
    // 从 app_data_dir 真值派生,不得用 log_dir.parent() 反推——log_dir 用户可配置,
    // 改日志目录后反推值漂移会令已下载模型「消失」(2026-07-10 审查 A1)。
    state.app_data_dir.join("models")
}

/// 将 worker 会话的 provider/gpu_name 回声写入配置；无有效回声时保留现值。
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

/// 从配置（`ai_active_model`）解析当前激活的模型 profile，缺省回退默认。该 id 同时是本模型
/// 向量在 `ai_embeddings.model_name` 的键。
pub(crate) fn active_profile(state: &AppState) -> ModelProfile {
    active_profile_with(&state.config)
}

/// 从指定配置快照解析当前模型 profile。
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

/// 从配置（`face_model_active`）解析当前激活的人脸模型 profile，缺省回退默认。人脸功能关闭
/// （`face_enabled=false`）时返回 `None`，worker 无需加载人脸 session。
/// 该 id 同时是人脸向量在 `faces.model_name` 的键。
pub(crate) fn active_face_profile(
    state: &AppState,
) -> Option<crate::ai::face_profile::FaceProfile> {
    active_face_profile_with(&state.config)
}

/// 从指定配置快照解析人脸模型；`face_enabled` 以 `"false"` 表示禁用。
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

/// 图片分析只依赖图像塔及其外部权重；文本查询另按文本角色加载。
pub(crate) fn image_variant_installed(models_dir: &std::path::Path, image_file: &str) -> bool {
    models_dir.join(image_file).exists()
        && models_dir.join(format!("{image_file}.extra_file")).exists()
}
