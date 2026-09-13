// src-tauri/src/scanner/volume_probe.rs
//! 卷在线判定（缺失检测最小闭环所需的最小实现）。
//!
//! 缺失检测的入口守门（§3.1.3）与 TOCTOU 复查（§3.2.2）只需回答一个问题：**这个 scan_root 的
//! 根路径现在是否在场（在线）？** 故本模块只提供「路径可访问性」级别的在线判定。
//!
//! ⚠️ **范围声明**：完整的跨平台**卷稳定 ID 枚举 + 插拔热感知**（Win `GetVolumeNameForVolumeMountPoint`
//! 取卷 GUID / mac DiskArbitration `DAVolumeUUID`，让同一物理卷换盘符仍能复认）是 Part2 **T1/T2** 的
//! 完整实现——缺失检测最小闭环**用不到**，故此处刻意只做路径兜底级在线判定，原生枚举后续单独落。
//!
//! 抽象成 `trait` 的目的：让缺失检测的入口守门 / 写删前复查**可注入 mock**，从而单测「扫描中途拔盘
//! → 不误删」这一最高危场景（否则真实拔盘无法在测试里复现）。

use std::path::Path;

use crate::db::models::VolumeKind;

/// 卷在线判定抽象。生产用 [`PathProber`]；测试可注入 mock 模拟「拔盘」。
///
/// 不变量（Part2 §5）：**识别失败/不确定时应视为「在线」**——宁可不删、不可误删。具体策略由实现决定。
pub trait VolumeOnlineCheck: Send + Sync {
    /// 给定 scan_root 根路径，判定其所在卷当前是否在线（可访问）。
    fn is_online(&self, root_path: &Path) -> bool;
}

/// 默认实现：根路径目录可访问（`metadata` 成功且为目录）即视为在线。跨平台、无原生依赖。
///
/// 取 `metadata`（而非 `Path::exists`）：`exists` 对权限/网络盘半断的判定较粗；`metadata` 失败
/// （权限/IO/网络盘断开）→ 不在线，与「不完整扫描 ≠ 删除」一致（读不到就别在它上面做差集）。
pub struct PathProber;

impl VolumeOnlineCheck for PathProber {
    fn is_online(&self, root_path: &Path) -> bool {
        std::fs::metadata(root_path)
            .map(|m| m.is_dir())
            .unwrap_or(false)
    }
}

// ── 卷身份解析（缺失检测守门1 的 volume_id 来源）─────────────────────────────────
//
// add_scan_root 据此为新根建卷并绑定 volume_id；缺了它新根的缺失检测会休眠（C5 Piece1 注）。

/// 解析出的卷身份。供 add_scan_root 建 `volumes` 行 + 绑定 `scan_roots.volume_id`。
pub struct ResolvedVolume {
    /// 稳定身份键（写入 `volumes.stable_id`，UNIQUE → 同卷多根复用同一行）。
    pub stable_id: String,
    pub kind: VolumeKind,
    /// 卷挂载根（写入 `last_mount_path`，展示 + 运行期路径重组提示）。
    pub mount_path: String,
}

/// 卷身份解析抽象。生产用 [`PathVolumeResolver`]（路径派生、无原生依赖、跨平台）。
///
/// ⚠️ **范围**：路径派生**不抗盘符重映射**——真正稳定的卷 ID 需原生 API（Win
/// `GetVolumeNameForVolumeMountPoint` 取卷 GUID / mac DiskArbitration `DAVolumeUUID`），
/// 属后续 T2 完整实现。届时仅换本 trait 的实现即可、**不动 add_scan_root**；且因数据模型
/// 按「卷根」并卷（非每根一卷），原生 GUID 只是升级既有卷行的 stable_id，无需合并迁移。
pub trait VolumeResolver: Send + Sync {
    fn resolve(&self, root_path: &str) -> ResolvedVolume;
}

/// 默认实现：以「卷挂载根」（盘符 / UNC share / POSIX `/`）派生稳定 ID，使同盘多根并卷。
/// 该实现无法识别卷类型；它只适合非物理清理场景的路径级卷登记，Windows 生产路径由
/// [`PlatformVolumeResolver`] 补充原生判定。
pub struct PathVolumeResolver;

impl VolumeResolver for PathVolumeResolver {
    fn resolve(&self, root_path: &str) -> ResolvedVolume {
        let root = derive_volume_root(root_path);
        ResolvedVolume {
            stable_id: format!("path:{root}"),
            // 独立的路径派生 resolver 没有卷类型能力；其返回值只供非物理清理路径使用。
            // Windows 生产入口会在原生探测失败时把该结果收紧为 Unknown。
            kind: VolumeKind::Local,
            mount_path: root,
        }
    }
}

/// 从路径派生「卷挂载根」，作为同盘多根并卷的依据。
/// - Windows 盘符：`C:\Photos\2024` / `C:/Photos` → `C:`
/// - UNC：`//server/share/sub` → `//server/share`
/// - POSIX：`/home/x/pics` → `/`（无 statfs，保守取文件系统根；同机多挂载会并为一卷，
///   但 `mark_missing` 按 root-subtree 收窄、跨根不会误标，故安全）
pub fn derive_volume_root(path: &str) -> String {
    let p = path.replace('\\', "/");
    // UNC: //server/share/...
    if let Some(rest) = p.strip_prefix("//") {
        let mut it = rest.splitn(3, '/');
        if let (Some(server), Some(share)) = (it.next(), it.next()) {
            if !server.is_empty() && !share.is_empty() {
                return format!("//{server}/{share}");
            }
        }
    }
    // Windows 盘符: X:/...
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && (b[0] as char).is_ascii_alphabetic() {
        return p[..2].to_uppercase();
    }
    // POSIX 文件系统根
    "/".to_string()
}

/// 生产首选 resolver（C5 Piece A）：Windows 用**原生卷 GUID**（抗盘符重映射——拔 E 盘
/// 插成 F 盘仍复认同一物理卷）；非 Windows 或原生解析失败时**回退** [`PathVolumeResolver`]。
///
/// 防御原则：原生 API 任一步失败 → 回退路径派生，**绝不**因解析失败中断「添加扫描根」。
/// 数据模型不变（仍按卷登记），故 win/path 两种 stable_id 可共存；新根用原生 GUID，
/// 既有 path 派生卷行由迁移收紧为 Unknown，重新添加并完成原生探测后才重新获得具体能力。
pub struct PlatformVolumeResolver;

impl VolumeResolver for PlatformVolumeResolver {
    fn resolve(&self, root_path: &str) -> ResolvedVolume {
        #[cfg(windows)]
        {
            if let Some(v) = win_resolve_volume(root_path) {
                return v;
            }
            // 原生 API 解析失败时仍保留路径派生的 stable_id，但能力必须标为 Unknown；
            // 不能把“无法确认是固定本地卷”误报成 Local，从而开放物理清理。
            let mut fallback = PathVolumeResolver.resolve(root_path);
            fallback.kind = VolumeKind::Unknown;
            fallback
        }
        #[cfg(not(windows))]
        {
            PathVolumeResolver.resolve(root_path)
        }
    }
}

/// NUL 终止的 UTF-16 缓冲转 String（截到第一个 NUL）。
#[cfg(windows)]
fn wstr_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Windows 原生卷解析：路径 → 卷挂载根 → 卷 GUID（`\\?\Volume{GUID}\`）+ 盘类型。
/// 任一 Win32 调用失败即返回 `None`（由调用方回退路径派生）。
#[cfg(windows)]
fn win_resolve_volume(root_path: &str) -> Option<ResolvedVolume> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };

    // GetDriveTypeW 返回值（稳定 Win32 常量，避免跨 windows-crate 版本的常量位置漂移）。
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_REMOTE: u32 = 4;

    // 入参转宽字符 + NUL 终止。
    let wide: Vec<u16> = root_path.encode_utf16().chain(std::iter::once(0)).collect();

    // 1) 路径 → 卷挂载根（如 `E:\`）。
    let mut mount = [0u16; 260]; // MAX_PATH
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut mount).ok()? };

    // 2) 挂载根 → 卷 GUID 路径 `\\?\Volume{GUID}\`（盘符无关，抗重映射）。
    let mut guid = [0u16; 64]; // GUID 卷名约 49 字符 + NUL，留余量
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount.as_ptr()), &mut guid).ok()? };
    let guid_str = wstr_to_string(&guid);
    if guid_str.is_empty() {
        return None;
    }

    // 3) 盘类型 → VolumeKind。
    let kind = match unsafe { GetDriveTypeW(PCWSTR(mount.as_ptr())) } {
        3 => VolumeKind::Local, // DRIVE_FIXED
        DRIVE_REMOVABLE => VolumeKind::Removable,
        DRIVE_REMOTE => VolumeKind::Network,
        _ => VolumeKind::Unknown,
    };

    Some(ResolvedVolume {
        // 去尾部反斜杠，规整为稳定身份键。
        stable_id: format!("win:{}", guid_str.trim_end_matches('\\')),
        kind,
        mount_path: wstr_to_string(&mount),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_prober_online_for_existing_dir() {
        let dir = std::env::temp_dir().join(format!("scrollery_probe_ok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let prober = PathProber;
        assert!(prober.is_online(&dir), "存在的目录应判为在线");

        let _ = std::fs::remove_dir_all(&dir);
        assert!(!prober.is_online(&dir), "删除后（不在场）应判为离线");
    }

    #[test]
    fn path_prober_offline_for_nonexistent() {
        let missing = std::env::temp_dir().join(format!(
            "scrollery_probe_missing_{}_nope",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);
        assert!(!PathProber.is_online(&missing), "不存在的路径应判为离线");
    }

    #[test]
    fn path_prober_offline_for_file_not_dir() {
        // 指向文件而非目录 → 不是合法 scan_root 卷根 → 离线（保守）。
        let f =
            std::env::temp_dir().join(format!("scrollery_probe_file_{}.tmp", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        assert!(!PathProber.is_online(&f), "文件（非目录）应判为离线");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn derive_volume_root_windows_drive() {
        assert_eq!(derive_volume_root("C:\\Photos\\2024"), "C:");
        assert_eq!(derive_volume_root("c:/photos"), "C:", "盘符应大写归一");
        assert_eq!(derive_volume_root("E:\\"), "E:");
    }

    #[test]
    fn derive_volume_root_unc() {
        assert_eq!(derive_volume_root("//server/share/sub/x"), "//server/share");
        assert_eq!(derive_volume_root("\\\\server\\share\\a"), "//server/share");
    }

    #[test]
    fn derive_volume_root_posix() {
        assert_eq!(derive_volume_root("/home/u/pics"), "/");
        assert_eq!(derive_volume_root("/"), "/");
    }

    #[test]
    fn same_drive_roots_share_stable_id() {
        // 同盘多根 → 同 stable_id（并卷），未来原生 GUID 升级时无需合并迁移。
        let r = PathVolumeResolver;
        let a = r.resolve("C:\\A\\photos");
        let b = r.resolve("C:\\B\\videos");
        assert_eq!(a.stable_id, b.stable_id, "同盘多根应并为同一卷");
        assert_eq!(a.stable_id, "path:C:");
    }

    #[test]
    fn unknown_volume_kind_is_not_coerced_to_local() {
        assert_eq!(VolumeKind::Unknown.as_str(), "unknown");
        assert!(matches!(
            VolumeKind::from_str_lossy("future-volume-kind"),
            VolumeKind::Unknown
        ));
    }
}
