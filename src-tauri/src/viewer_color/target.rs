//! 查看器渲染目标色域的解析与 profile 装配(方案 §0②③④)。
//!
//! `ViewerColorTarget` 从 config 的 `viewer_color_target` + `viewer_color_custom_id` 两键派生
//! (config 单源,前端不传 target,防口径分叉)。srgb / 未选自定义 → `None`(无派生,直显原图)。
//! `resolve_profile` 同时给出 CMS 变换用的 `ColorProfile` 对象与要嵌入输出的**同一份字节**
//! (D-412 构造性成立:内置走 `encode()`,自定义走导入原字节,绝不重编码)。

use std::path::{Path, PathBuf};

use moxcms::{ColorProfile, DataColorSpace};

use super::{color_err, CODE_ICC_IO, CODE_ICC_NOT_FOUND, CODE_ICC_PARSE_FAILED, CODE_RENDER_IO};
use crate::error::AppError;

/// 需派生的目标色域。sRGB / 未选自定义**不在本枚举内**——那两种情形由 [`from_config`] 返 `None`
/// 表达「零派生」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerColorTarget {
    /// Display P3(内置)。
    DisplayP3,
    /// DCI-P3(内置,主要用于影院素材核对)。
    DciP3,
    /// 自定义导入的 ICC,携带 16 位小写 hex profile id。
    Custom(String),
}

/// profile id 合法性:严格 16 位小写 hex(防路径注入,先例 `thumbnail::cache::parse_cache_key_stem`
/// / cache.rs delete 校验)。导入命令生成的 id 恒满足此式。
pub fn is_valid_profile_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// ICC 原字节 → 16 位小写 hex profile id(xxh3,与 cache_key 同哈希族,方案 §0②)。
pub fn profile_id_of(bytes: &[u8]) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(bytes))
}

impl ViewerColorTarget {
    /// 从 config 两键解析。返回 `None` 表示**无派生**(srgb / 未知值 / custom 但 custom_id 非法或空
    /// ——主线行为钉:target=custom 且 id 空 → 优雅回退直显原图,不落错误码)。
    pub fn from_config(target: &str, custom_id: &str) -> Option<ViewerColorTarget> {
        match target {
            "display-p3" => Some(ViewerColorTarget::DisplayP3),
            "dci-p3" => Some(ViewerColorTarget::DciP3),
            "custom" => {
                let id = custom_id.trim();
                is_valid_profile_id(id).then(|| ViewerColorTarget::Custom(id.to_string()))
            }
            // srgb 与任何未知值都视为零派生(直显原图)。
            _ => None,
        }
    }

    /// 派生缓存的 `target_id` 目录段(方案 §0②):`display-p3` / `dci-p3` / `icc-{profile_id}`。
    pub fn target_id(&self) -> String {
        match self {
            ViewerColorTarget::DisplayP3 => "display-p3".to_string(),
            ViewerColorTarget::DciP3 => "dci-p3".to_string(),
            ViewerColorTarget::Custom(id) => format!("icc-{id}"),
        }
    }

    /// 装配 (CMS 变换用 profile 对象, 要嵌入输出的字节)。二者同源 → D-412 构造性成立。
    /// - 内置:`ColorProfile::new_display_p3()`/`new_dci_p3()` + `.encode()`,变换与嵌入同一对象/字节。
    /// - 自定义:读 `{app_data_dir}/config/icc/{id}.icc` 原字节,`new_from_slice` 解析同一字节。
    pub fn resolve_profile(
        &self,
        app_data_dir: &Path,
    ) -> Result<(ColorProfile, Vec<u8>), AppError> {
        match self {
            ViewerColorTarget::DisplayP3 => {
                let profile = ColorProfile::new_display_p3();
                let bytes = profile
                    .encode()
                    .map_err(|_| color_err(CODE_RENDER_IO, "内置 Display P3 profile 编码失败"))?;
                Ok((profile, bytes))
            }
            ViewerColorTarget::DciP3 => {
                let profile = ColorProfile::new_dci_p3();
                let bytes = profile
                    .encode()
                    .map_err(|_| color_err(CODE_RENDER_IO, "内置 DCI-P3 profile 编码失败"))?;
                Ok((profile, bytes))
            }
            ViewerColorTarget::Custom(id) => {
                let path = icc_profile_path(app_data_dir, id);
                let bytes = std::fs::read(&path).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        color_err(CODE_ICC_NOT_FOUND, "自定义 ICC profile 不存在")
                    } else {
                        color_err(CODE_ICC_IO, "读取自定义 ICC profile 失败")
                    }
                })?;
                let profile = ColorProfile::new_from_slice(&bytes)
                    .map_err(|_| color_err(CODE_ICC_PARSE_FAILED, "自定义 ICC profile 解析失败"))?;
                // 防御:被外部改坏成非 RGB 空间的自定义 profile 不用于变换(导入时已探针,此处兜底)。
                if profile.color_space != DataColorSpace::Rgb {
                    return Err(color_err(
                        CODE_ICC_PARSE_FAILED,
                        "自定义 ICC profile 色彩空间非 RGB",
                    ));
                }
                Ok((profile, bytes))
            }
        }
    }
}

/// 自定义 ICC 持久化根目录:`{app_data_dir}/config/icc`(方案 §0④,state.rs app_data_dir 单源)。
pub fn icc_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("config").join("icc")
}

/// 单个自定义 ICC 文件路径:`{icc_dir}/{id}.icc`。调用方须先经 [`is_valid_profile_id`] 校验 `id`。
pub fn icc_profile_path(app_data_dir: &Path, id: &str) -> PathBuf {
    icc_dir(app_data_dir).join(format!("{id}.icc"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_maps_targets_and_none_cases() {
        assert_eq!(
            ViewerColorTarget::from_config("display-p3", ""),
            Some(ViewerColorTarget::DisplayP3)
        );
        assert_eq!(
            ViewerColorTarget::from_config("dci-p3", "ignored"),
            Some(ViewerColorTarget::DciP3)
        );
        // srgb / 未知值 → 无派生。
        assert_eq!(ViewerColorTarget::from_config("srgb", ""), None);
        assert_eq!(ViewerColorTarget::from_config("weird", ""), None);
        // custom 但 id 空 / 非 16-hex → 无派生(主线行为钉:优雅回退)。
        assert_eq!(ViewerColorTarget::from_config("custom", ""), None);
        assert_eq!(ViewerColorTarget::from_config("custom", "xyz"), None);
        assert_eq!(
            ViewerColorTarget::from_config("custom", "0123456789abcdef"),
            Some(ViewerColorTarget::Custom("0123456789abcdef".to_string()))
        );
    }

    #[test]
    fn target_id_layout() {
        assert_eq!(ViewerColorTarget::DisplayP3.target_id(), "display-p3");
        assert_eq!(ViewerColorTarget::DciP3.target_id(), "dci-p3");
        assert_eq!(
            ViewerColorTarget::Custom("00ff00ff00ff00ff".to_string()).target_id(),
            "icc-00ff00ff00ff00ff"
        );
    }

    #[test]
    fn is_valid_profile_id_strict_16_lower_hex() {
        assert!(is_valid_profile_id("0123456789abcdef"));
        assert!(!is_valid_profile_id("0123456789ABCDEF")); // 大写不收
        assert!(!is_valid_profile_id("0123456789abcde")); // 15 位
        assert!(!is_valid_profile_id("0123456789abcdefg")); // 非 hex
        assert!(!is_valid_profile_id("../../../etc/pas")); // 路径注入
    }

    /// ⑧-1:内置 profile `encode()` 字节可 `new_from_slice` 回读且 `color_space==Rgb`。
    #[test]
    fn builtin_profiles_encode_roundtrip_rgb() {
        let dummy = Path::new("C:/unused");
        for target in [ViewerColorTarget::DisplayP3, ViewerColorTarget::DciP3] {
            let (_profile, bytes) = target.resolve_profile(dummy).expect("resolve builtin");
            let reread = ColorProfile::new_from_slice(&bytes).expect("re-parse encoded bytes");
            assert_eq!(reread.color_space, DataColorSpace::Rgb);
        }
    }

    #[test]
    fn profile_id_is_stable_16_hex() {
        let a = profile_id_of(b"some-icc-bytes");
        let b = profile_id_of(b"some-icc-bytes");
        assert_eq!(a, b);
        assert!(is_valid_profile_id(&a));
        assert_ne!(a, profile_id_of(b"other-bytes"));
    }
}
