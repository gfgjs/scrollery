//! 查看器渲染色域的 IPC 命令(B 线,方案 §0①④⑦)。四命令:派生 URL 获取 + 自定义 ICC
//! 导入/列举/删除。target **不由前端传参**——后端从 `ConfigManager` 读 `viewer_color_target`
//! 与 `viewer_color_custom_id` 两键(config 单源,防前后端口径分叉),派生路径含 target_id,
//! URL 天然随 target 变化。
//!
//! 平台门控(D-414):移动端(android/ios)锁 sRGB——`get_viewer_color_url` 返 `Ok(None)`
//! (直显原图),import/list/delete 返 `unsupported_platform`(先例 `ipc::reveal.rs:37-41`)。

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::viewer_color::{color_err, CODE_UNSUPPORTED_PLATFORM, ICC_MAX_BYTES};

/// 自定义 ICC profile 的展示信息(设置页「已导入」列表)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IccProfileInfo {
    /// 16 位小写 hex profile id(= ICC 原字节 xxh3)。
    pub id: String,
    /// 展示名(取 profile description tag,缺省 `ICC {id 前 8 位}`)。
    pub name: String,
    /// 文件字节数。
    pub file_size_bytes: u64,
}

/// 判定当前编译目标是否为移动端(D-414 平台门控)。
macro_rules! is_mobile {
    () => {
        cfg!(any(target_os = "android", target_os = "ios"))
    };
}

// ════════════════════════════════════════════════════════════════════════════
// ① 派生 URL 获取
// ════════════════════════════════════════════════════════════════════════════

/// 获取查看器大图应显示的派生文件绝对路径(正斜杠,调用方 `convertFileSrc` 包装)。
/// `None` = 直显原图(target=srgb / 移动端 / 非 image / 未选自定义 / 卷内不可用 / 格式不支持)。
#[tauri::command]
pub async fn get_viewer_color_url(
    item_id: i64,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>> {
    if is_mobile!() {
        // 移动端锁 sRGB:直显原图(D-414)。
        return Ok(None);
    }
    // config 单源读 target(前端不传参)。srgb / custom 未选(含 custom_id 空,主线行为钉)→ None。
    let target_str = state.config.get("viewer_color_target").unwrap_or_default();
    let custom_id = state
        .config
        .get("viewer_color_custom_id")
        .unwrap_or_default();
    let Some(target) =
        crate::viewer_color::target::ViewerColorTarget::from_config(&target_str, &custom_id)
    else {
        return Ok(None);
    };

    // 并发去重:同 (item_id, target_id) 的并发请求单渲(keyed lock,跨 spawn_blocking 持有)。
    let target_id = target.target_id();
    let key = (item_id, target_id);
    let lock = state.viewer_render_lock(key.clone());
    let guard = lock.lock().await;

    let state_arc = state.inner().clone();
    let render_target = target.clone();
    let join = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
        render_viewer_color(&state_arc, item_id, &render_target)
    })
    .await;

    // 先释放 keyed lock(drop guard 放行等待者),再回收 map 条目(strong_count==1 时)。
    drop(guard);
    drop(lock);
    state.release_viewer_render_lock(&key);

    join.map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 派生 URL 的阻塞主体(DB 读 + 渲染整段离开 tokio worker,镜像 `media_commands.rs:141-209`)。
fn render_viewer_color(
    state: &AppState,
    item_id: i64,
    target: &crate::viewer_color::target::ViewerColorTarget,
) -> Result<Option<String>> {
    use crate::db::queries as q;

    let pool = state.db_read_pool.get().map_err(AppError::from)?;

    // 卷离线守门(镜像 get_companion_video_url :155):所在卷离线 → VolumeOffline(前端走既有错误 UI)。
    if let Some(label) = q::get_item_volume_offline_label(&pool, item_id)? {
        return Err(AppError::VolumeOffline(label));
    }

    let detail = q::get_media_detail(&pool, item_id)?;
    // 非 image / 已删 / 不在线 / 格式不在派生白名单 → 优雅回退直显原图。
    if detail.item.media_type != "image" {
        return Ok(None);
    }
    if detail.item.is_deleted || detail.availability != "online" {
        return Ok(None);
    }
    let ext = detail.item.file_format.to_ascii_lowercase();
    if !crate::viewer_color::render::is_supported_input_ext(&ext) {
        return Ok(None);
    }

    let cache_key = detail.item.cache_key;
    let cache_dir = state
        .thumb_config
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cache_dir
        .clone();
    let app_data_dir = state.app_data_dir.clone();

    let path = crate::viewer_color::render::ensure_derivative(
        Path::new(&detail.abs_path),
        &ext,
        cache_key,
        &cache_dir,
        target,
        &app_data_dir,
    )?;
    Ok(Some(path.to_string_lossy().replace('\\', "/")))
}

// ════════════════════════════════════════════════════════════════════════════
// ④ 自定义 ICC 导入 / 列举 / 删除
// ════════════════════════════════════════════════════════════════════════════

/// 平台门控错误(import/list/delete 在移动端不可用)。
fn unsupported_platform() -> AppError {
    color_err(CODE_UNSUPPORTED_PLATFORM, "当前平台不支持自定义色域")
}

/// 导入自定义 ICC profile。校验链见 [`import_icc_blocking`]。
#[tauri::command]
pub async fn import_icc_profile(
    file_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<IccProfileInfo> {
    if is_mobile!() {
        return Err(unsupported_platform());
    }
    let app_data_dir = state.app_data_dir.clone();
    tokio::task::spawn_blocking(move || import_icc_blocking(&app_data_dir, &file_path))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 列举已导入的自定义 ICC profile。
#[tauri::command]
pub async fn list_icc_profiles(state: State<'_, Arc<AppState>>) -> Result<Vec<IccProfileInfo>> {
    if is_mobile!() {
        return Err(unsupported_platform());
    }
    let app_data_dir = state.app_data_dir.clone();
    tokio::task::spawn_blocking(move || Ok(list_icc_blocking(&app_data_dir)))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 删除自定义 ICC profile(连带派生子树;若为当前选中则复位 config 至 srgb)。
#[tauri::command]
pub async fn delete_icc_profile(profile_id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    if is_mobile!() {
        return Err(unsupported_platform());
    }
    let state_arc = state.inner().clone();
    tokio::task::spawn_blocking(move || delete_icc_blocking(&state_arc, &profile_id))
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

// ── 阻塞实现(spawn_blocking 内)────────────────────────────────────────────────

use crate::viewer_color::{
    target, CODE_ICC_IO, CODE_ICC_NOT_DISPLAY_CLASS, CODE_ICC_NOT_FOUND, CODE_ICC_NOT_RGB,
    CODE_ICC_PARSE_FAILED, CODE_ICC_TOO_LARGE, CODE_ICC_TRANSFORM_UNSUPPORTED,
};
use moxcms::{ColorProfile, DataColorSpace, Layout, ProfileClass, ProfileText};

/// 校验链(方案 §0④):canonicalize+is_file → 大小 ≤16MB → new_from_slice → color_space==Rgb
/// → profile_class==DisplayDevice → 变换探针(sRGB→candidate,拦截 LUT 型不可变换)→ 持久化。
/// 同字节重复导入幂等(id 相同 → 文件已存在则不重写)。
fn import_icc_blocking(app_data_dir: &Path, file_path: &str) -> Result<IccProfileInfo> {
    // 路径 canonicalize + 存在性(硬约束:用户路径 canonicalize)。
    let path = std::fs::canonicalize(file_path)
        .map_err(|_| color_err(CODE_ICC_IO, "ICC 文件不可访问 | icc file inaccessible"))?;
    if !path.is_file() {
        return Err(color_err(CODE_ICC_IO, "路径不是文件 | path is not a file"));
    }
    let size = std::fs::metadata(&path)
        .map_err(|_| color_err(CODE_ICC_IO, "读取文件属性失败 | stat failed"))?
        .len();
    if size > ICC_MAX_BYTES {
        return Err(color_err(
            CODE_ICC_TOO_LARGE,
            "ICC 超过 16MB | icc too large",
        ));
    }
    let bytes = std::fs::read(&path)
        .map_err(|_| color_err(CODE_ICC_IO, "读取 ICC 文件失败 | read failed"))?;

    let profile = ColorProfile::new_from_slice(&bytes).map_err(|_| {
        color_err(
            CODE_ICC_PARSE_FAILED,
            "ICC profile 解析失败 | icc parse failed",
        )
    })?;
    if profile.color_space != DataColorSpace::Rgb {
        return Err(color_err(
            CODE_ICC_NOT_RGB,
            "ICC 色彩空间非 RGB | icc not rgb",
        ));
    }
    if profile.profile_class != ProfileClass::DisplayDevice {
        return Err(color_err(
            CODE_ICC_NOT_DISPLAY_CLASS,
            "ICC 非显示类 profile | icc not display class",
        ));
    }
    // 变换探针:sRGB→candidate 8-bit RGBA 变换。LUT 型等不可变换的 profile 在此拦截。
    ColorProfile::new_srgb()
        .create_transform_8bit(
            Layout::Rgba,
            &profile,
            Layout::Rgba,
            crate::editing::color::options(),
        )
        .map_err(|_| {
            color_err(
                CODE_ICC_TRANSFORM_UNSUPPORTED,
                "ICC 无法用于色彩变换 | icc transform unsupported",
            )
        })?;

    let id = target::profile_id_of(&bytes);
    let name = profile_name(&profile, &id);

    // 持久化 {app_data}/config/icc/{id}.icc(tmp+rename;幂等:同 id 已存在即同字节,不重写)。
    let dir = target::icc_dir(app_data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|_| color_err(CODE_ICC_IO, "创建 ICC 目录失败 | mkdir icc dir failed"))?;
    let dest = target::icc_profile_path(app_data_dir, &id);
    if !dest.exists() {
        crate::thumbnail::generator::write_atomic(&dest, &bytes)
            .map_err(|_| color_err(CODE_ICC_IO, "ICC 落盘失败 | icc write failed"))?;
    }

    Ok(IccProfileInfo {
        id,
        name,
        file_size_bytes: bytes.len() as u64,
    })
}

/// 从 profile description tag 取展示名;缺省 `ICC {id 前 8 位}`。
fn profile_name(profile: &ColorProfile, id: &str) -> String {
    let from_desc = match &profile.description {
        Some(ProfileText::PlainString(s)) => Some(s.clone()),
        Some(ProfileText::Localizable(v)) => v.first().map(|l| l.value.clone()),
        Some(ProfileText::Description(d)) => Some(d.ascii_string.clone()),
        None => None,
    };
    from_desc
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("ICC {}", &id[..8]))
}

/// 列举 icc 目录下所有合法 profile(id 须 16-hex,可解析)。best-effort:坏文件跳过。
fn list_icc_blocking(app_data_dir: &Path) -> Vec<IccProfileInfo> {
    let dir = target::icc_dir(app_data_dir);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("icc") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !target::is_valid_profile_id(stem) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(profile) = ColorProfile::new_from_slice(&bytes) else {
            continue;
        };
        let name = profile_name(&profile, stem);
        out.push(IccProfileInfo {
            id: stem.to_string(),
            name,
            file_size_bytes: bytes.len() as u64,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    out
}

/// 删除 profile 的文件产物(`.icc` + 派生子树 `cache/viewer_color/icc-{id}/`)。不碰 config。
/// id 须 16-hex(防路径注入,先例 cache.rs delete 校验)。`.icc` 不存在按幂等处理(仍清子树)。
fn delete_profile_files(app_data_dir: &Path, cache_dir: &Path, profile_id: &str) -> Result<()> {
    if !target::is_valid_profile_id(profile_id) {
        return Err(color_err(
            CODE_ICC_NOT_FOUND,
            "无效的 profile id | invalid profile id",
        ));
    }
    let icc_path = target::icc_profile_path(app_data_dir, profile_id);
    match std::fs::remove_file(&icc_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(color_err(
                CODE_ICC_IO,
                "删除 ICC 文件失败 | delete icc failed",
            ))
        }
    }
    let sub = cache_dir
        .join("viewer_color")
        .join(format!("icc-{profile_id}"));
    if sub.exists() {
        let _ = std::fs::remove_dir_all(&sub);
    }
    Ok(())
}

fn delete_icc_blocking(state: &AppState, profile_id: &str) -> Result<()> {
    let cache_dir = state
        .thumb_config
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cache_dir
        .clone();
    delete_profile_files(&state.app_data_dir, &cache_dir, profile_id)?;

    // 若删的恰是当前选中 profile,后端复位 target=srgb、清 custom_id(best-effort:写失败仅告警,
    // 此时文件已删 → resolve_profile 会返 icc_not_found → 前端自然回退直显原图)。
    if state.config.get("viewer_color_custom_id").as_deref() == Some(profile_id) {
        if let Err(e) = state.config.set_and_persist("viewer_color_target", "srgb") {
            tracing::warn!("删除当前 ICC 后复位 viewer_color_target 失败: {e}");
        }
        if let Err(e) = state.config.reset_key("viewer_color_custom_id") {
            tracing::warn!("删除当前 ICC 后清 viewer_color_custom_id 失败: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcms2::{CIExyY, ToneCurve};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("scrollery_vc_cmd_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn point(x: f64, y: f64) -> CIExyY {
        CIExyY { x, y, Y: 1.0 }
    }

    /// ⑧-4:坏字节 → icc_parse_failed。
    #[test]
    fn import_malformed_bytes_maps_to_parse_failed() {
        let dir = tmp_dir("bad");
        let src = dir.join("broken.icc");
        std::fs::write(&src, b"not a real icc profile").unwrap();
        let err = import_icc_blocking(&dir, src.to_str().unwrap()).unwrap_err();
        assert!(matches!(
            &err,
            AppError::Color { code, .. } if *code == CODE_ICC_PARSE_FAILED
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑧-4:Gray profile → icc_not_rgb。
    #[test]
    fn import_gray_profile_maps_to_not_rgb() {
        let dir = tmp_dir("gray");
        let curve = ToneCurve::new(2.2);
        let profile = lcms2::Profile::new_gray(&point(0.3127, 0.3290), &curve).unwrap();
        let icc = profile.icc().unwrap();
        let src = dir.join("gray.icc");
        std::fs::write(&src, &icc).unwrap();
        let err = import_icc_blocking(&dir, src.to_str().unwrap()).unwrap_err();
        assert!(matches!(
            &err,
            AppError::Color { code, .. } if *code == CODE_ICC_NOT_RGB
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑧-4:错误码序列化快照(前端分流真正读到的 `code` 字段;先例 reveal.rs:59-64)。
    #[test]
    fn color_error_serializes_stable_code() {
        let v = serde_json::to_value(color_err(CODE_ICC_TRANSFORM_UNSUPPORTED, "x")).unwrap();
        assert_eq!(v["code"], "icc_transform_unsupported");
        // message 不得泄漏路径/底层串(此处仅占位文案)。
        assert_eq!(v["message"], "x");
    }

    /// ⑧-4:import→list→delete 闭环(用内置 Display P3 encode 字节作合法 RGB+DisplayDevice 样本)。
    #[test]
    fn import_list_delete_roundtrip() {
        let app_dir = tmp_dir("roundtrip_app");
        let cache_dir = tmp_dir("roundtrip_cache");

        // 合法样本:内置 Display P3 的 encode 字节(RGB + DisplayDevice + 可变换)。
        let icc = moxcms::ColorProfile::new_display_p3().encode().unwrap();
        let src = app_dir.join("p3.icc");
        std::fs::write(&src, &icc).unwrap();

        let info = import_icc_blocking(&app_dir, src.to_str().unwrap()).unwrap();
        assert_eq!(info.id, target::profile_id_of(&icc));
        assert!(target::is_valid_profile_id(&info.id));
        // 落盘到 {app}/config/icc/{id}.icc。
        assert!(target::icc_profile_path(&app_dir, &info.id).exists());

        // list 反映之。
        let listed = list_icc_blocking(&app_dir);
        assert!(listed.iter().any(|p| p.id == info.id));

        // 造一个派生子树,删除应连带清除。
        let sub = cache_dir
            .join("viewer_color")
            .join(format!("icc-{}", info.id));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("dummy.jpg"), b"x").unwrap();

        delete_profile_files(&app_dir, &cache_dir, &info.id).unwrap();
        assert!(!target::icc_profile_path(&app_dir, &info.id).exists());
        assert!(!sub.exists());
        assert!(list_icc_blocking(&app_dir).iter().all(|p| p.id != info.id));

        let _ = std::fs::remove_dir_all(&app_dir);
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    /// ⑧-4:非法 id(防路径注入)→ icc_not_found,不触碰文件系统。
    #[test]
    fn delete_rejects_invalid_id() {
        let app_dir = tmp_dir("delinv_app");
        let cache_dir = tmp_dir("delinv_cache");
        let err = delete_profile_files(&app_dir, &cache_dir, "../etc").unwrap_err();
        assert!(matches!(
            &err,
            AppError::Color { code, .. } if *code == CODE_ICC_NOT_FOUND
        ));
        let _ = std::fs::remove_dir_all(&app_dir);
        let _ = std::fs::remove_dir_all(&cache_dir);
    }
}
