// src-tauri/src/utils/path.rs
//! 路径规范化和解析实用工具。
//!
//! 来自实施计划的 Q12 / Q14 / 5.7：
//! - 数据库始终存储正斜杠路径。
//! - 运行时路径构建使用 `PathBuf` 以实现操作系统兼容性。

use std::path::{Path, PathBuf};

/// 规范化路径字符串以便在数据库中存储。
///
/// 将所有操作系统特定的分隔符转换为 `/`。
/// 去除前导/尾随空格和分隔符。
pub fn normalize_db_path(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

/// 规范化**扫描根**路径以便存库（§3.8 8A —— 网络盘当扫描根）。
///
/// Unlike `normalize_db_path`, this preserves absolute-root markers: a leading UNC prefix
/// (`\\server\share` / `//server/share`), a POSIX /, or a Windows drive root (C:/). This keeps
/// a network share, POSIX root, or drive root from becoming a relative path in the DB.
/// 与 `normalize_db_path` 不同，本函数保留前导 UNC 前缀（`\\server\share` / `//server/share`），
/// 并保留 POSIX / 和 Windows 盘符根 C:/，避免存库后绝对路径变成相对路径。
/// 其它盘符路径和相对路径仍只转换分隔符并去掉尾部斜杠。
pub fn normalize_root_path(path: &str) -> String {
    let trimmed = path.trim();
    let is_unc = trimmed.starts_with("\\\\") || trimmed.starts_with("//");
    let fwd = trimmed.replace('\\', "/");
    if is_unc {
        // 把前导连续斜杠折叠为恰好两个，保留其余，去掉尾部斜杠。
        let body = fwd.trim_start_matches('/').trim_end_matches('/');
        format!("//{body}")
    } else if fwd.starts_with('/') {
        // POSIX 绝对路径保留一个根斜杠；根目录本身不能被裁成空串。
        let body = fwd.trim_start_matches('/').trim_end_matches('/');
        if body.is_empty() {
            "/".to_string()
        } else {
            format!("/{body}")
        }
    } else if fwd.len() >= 3
        && fwd.as_bytes()[0].is_ascii_alphabetic()
        && fwd.as_bytes()[1] == b':'
        && fwd.as_bytes()[2..].iter().all(|byte| *byte == b'/')
    {
        // C: 是 Windows 的盘符相对路径；只有带根斜杠的 C:/ 才保留为盘根。
        format!("{}/", &fwd[..2])
    } else {
        fwd.trim_end_matches('/').to_string()
    }
}

/// 根据存储的三个部分构建绝对 `PathBuf`。
///
/// - `root_path`: 扫描根目录的绝对路径（来自数据库）
/// - `rel_path`:  根目录内的相对路径（如果处于根级别则为空字符串）
/// - `file_name`: 基本文件名
pub fn resolve_media_path(root_path: &str, rel_path: &str, file_name: &str) -> String {
    let mut pb = PathBuf::from(root_path);
    if !rel_path.is_empty() {
        pb.push(rel_path);
    }
    pb.push(file_name);
    // 返回正斜杠字符串（跨平台工作）
    pb.to_string_lossy().replace('\\', "/")
}

/// 提取文件相对于给定根目录的相对路径。
/// 返回正斜杠相对路径，如果文件直接位于根目录中则返回空字符串。
pub fn relative_to_root(root: &Path, file: &Path) -> String {
    let parent = file.parent().unwrap_or(Path::new(""));
    if let Ok(rel) = parent.strip_prefix(root) {
        normalize_db_path(&rel.to_string_lossy())
    } else {
        String::new()
    }
}

/// 给定扫描根目录，计算文件的相对目录路径。
pub fn dir_rel_path(root: &Path, file_path: &Path) -> String {
    let parent = file_path.parent().unwrap_or(file_path);
    if parent == root {
        // 文件直接位于根目录下
        String::new()
    } else {
        parent
            .strip_prefix(root)
            .map(|p| normalize_db_path(&p.to_string_lossy()))
            .unwrap_or_default()
    }
}

/// 提取相对路径的深度（`/` 分隔符的数量 + 1，对于根目录则为 0）。
pub fn path_depth(rel_path: &str) -> i64 {
    if rel_path.is_empty() {
        0
    } else {
        (rel_path.matches('/').count() + 1) as i64
    }
}

/// 把已规范化的 `/`-分隔 `rel_path` 编码为「前序 DFS 排序键」（BLOB）。
///
/// 每个 path segment 的 UTF-8 字节后追加一个 `0x00` 终止字节，再串接。由于 `0x00` 小于任何
/// 合法路径字节（NUL 是唯一在任何文件系统都不能出现在文件名里的字节），字节字典序恰好等于
/// 前序 DFS 序：父路径键成为子路径键的**严格前缀**（父恒早于其全部后代、后代紧邻），同前缀下
/// 较短段的终止符 `0x00` 早于较长段的后续字节。据此得 `A < A/Z < A-`，而非原始字符串序的
/// `A < A- < A/Z`（`-`=0x2D < `/`=0x2F 造成的割裂）。根目录空路径 → 空键 `[]`。
///
/// SQLite 侧经 `TREE_SORT_KEY` 标量函数消费**同一逻辑**：BLOB memcmp 与 Rust `Vec<u8>::cmp`
/// 同构，据此保证内存 `build_dir_rank` 与 SQL `push_order_by` 的 folder 目录序逐项一致（刚性
/// 等价契约，见 `db::queries::canonical_derive_order_matches_sql_order`）。
///
/// 前置：输入须为 [`normalize_db_path`] 后的路径（`/` 分隔、无首尾斜杠）。防御性跳过空段
/// （内部 `//` 未折叠等异常输入），避免空段污染键。
pub fn encode_tree_sort_key(rel_path: &str) -> Vec<u8> {
    // 预留 rel_path 字节数 + 少量终止符空间（多数目录深度个位数）。
    let mut key = Vec::with_capacity(rel_path.len() + 4);
    for segment in rel_path.split('/') {
        if segment.is_empty() {
            // 根路径（""）或防御内部 `//`：空段不产生键字节（等价于折叠多余斜杠）。
            continue;
        }
        key.extend_from_slice(segment.as_bytes());
        key.push(0x00);
    }
    key
}

/// 把「**可信**扫描根 + **不可信** `rel_path`」解析为校验过的绝对路径（S 线 §4.2）。
///
/// 与 [`resolve_media_path`] 的区别是**信任模型**，不是功能：那个是纯字符串拼接，服务于
/// DB 里已由扫描器写入的可信三段；本函数的 `rel_path` 来自 WebView（文件树 IPC 入参），
/// 必须当作敌意输入。
///
/// 三层防线（顺序有意）：
/// 1. **逐段过滤**：拒 `..`（父级穿越）；Windows 另拒含 `:` 的段（`C:` 盘符、`name:stream`
///    NTFS 备用数据流）。按分隔符切段还顺带消除 `PathBuf::push` 的经典陷阱 ——
///    `push("/etc")` 会**整体替换**路径，而切段后任何段都不含分隔符，故不可能是绝对路径。
/// 2. **canonicalize 两侧**：解析 symlink/`.`/大小写，得到真实位置。不存在即拒。
/// 3. **`starts_with` 边界断言**：`Path::starts_with` 按**组件**比较而非字符串前缀，故
///    `/a/bc` 不会被判为在 `/a/b` 内 —— 字符串前缀写法在此处是个真实漏洞。
///
/// symlink 语义：本函数**跟随**符号链接再判边界 —— 指向根内的链接放行（只是同一内容的另一
/// 视图），指向根外的一律拒。这与扫描器 `walker.rs` 的 `follow_links(false)`（不**递归进**
/// 链接目录，避免环与重复入库）是两回事，不冲突。
///
/// `rel_path` 为空 → 返回扫描根自身（合法：树的根节点）。
pub fn resolve_within_root(root_path: &str, rel_path: &str) -> crate::error::Result<PathBuf> {
    use crate::error::AppError;

    let canonical_root = std::fs::canonicalize(root_path).map_err(|_| {
        AppError::PathResolution("扫描根不存在或不可访问 | scan root missing".into())
    })?;

    let mut target = canonical_root.clone();
    for seg in rel_path.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(AppError::PathResolution(
                "rel_path 含父级穿越 | parent traversal rejected".into(),
            ));
        }
        #[cfg(windows)]
        if seg.contains(':') {
            return Err(AppError::PathResolution(
                "rel_path 段含 ':' | invalid path component".into(),
            ));
        }
        target.push(seg);
    }

    let canonical_target = std::fs::canonicalize(&target)
        .map_err(|_| AppError::PathResolution("路径不存在 | path does not exist".into()))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err(AppError::PathResolution(
            "路径越出扫描根 | path escapes scan root".into(),
        ));
    }
    Ok(canonical_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sort_key_empty_is_empty() {
        assert_eq!(encode_tree_sort_key(""), Vec::<u8>::new());
    }

    #[test]
    fn tree_sort_key_single_and_multi_segment() {
        assert_eq!(encode_tree_sort_key("A"), vec![0x41, 0x00]);
        assert_eq!(encode_tree_sort_key("A/Z"), vec![0x41, 0x00, 0x5A, 0x00]);
        assert_eq!(encode_tree_sort_key("A-"), vec![0x41, 0x2D, 0x00]);
    }

    #[test]
    fn tree_sort_key_orders_as_preorder_dfs() {
        // 核心反例：原始字符串序 A < A- < A/Z（因 '-'=0x2D < '/'=0x2F）；
        // 前序 DFS 序 A < A/Z < A-（子树 A/Z 紧随父 A，同级 A- 在其后）。NUL 终止键给出后者。
        let a = encode_tree_sort_key("A");
        let az = encode_tree_sort_key("A/Z");
        let a_dash = encode_tree_sort_key("A-");
        assert!(a < az, "父 A 早于后代 A/Z");
        assert!(az < a_dash, "完整子树 A/Z 早于后续同级 A-");
        assert!(a < a_dash, "传递性 A < A-");
    }

    #[test]
    fn tree_sort_key_parent_is_strict_prefix_of_child() {
        // 父键是子键严格前缀 → 父恒早于其全部后代，且后代紧邻。
        let parent = encode_tree_sort_key("Photos/2024");
        let child = encode_tree_sort_key("Photos/2024/Jan");
        assert!(child.starts_with(&parent), "子键以父键为前缀");
        assert!(parent < child);
    }

    #[test]
    fn tree_sort_key_case_sensitive_binary() {
        // 与 directories.name 的 BINARY collation 一致：大写 < 小写（Z=0x5A < a=0x61）。
        assert!(encode_tree_sort_key("Albums") < encode_tree_sort_key("albums"));
    }

    #[test]
    fn tree_sort_key_unicode_sorts_after_ascii() {
        // 多字节 UTF-8（相=0xE7…）排在 ASCII 之后。
        assert!(encode_tree_sort_key("albums") < encode_tree_sort_key("相册"));
    }

    #[test]
    fn tree_sort_key_defensive_skips_empty_segments() {
        // 内部 `//` 未折叠时：空段不污染键，等价于折叠后的单斜杠路径。
        assert_eq!(encode_tree_sort_key("A//B"), encode_tree_sort_key("A/B"));
    }

    #[test]
    fn tree_sort_key_sibling_order_reduces_to_basename() {
        // 同父下，键比较归约为 basename 字节比较（前序 DFS 同级序 = name BINARY 序）；
        // 短者终止符先：AB < ABC。
        assert!(encode_tree_sort_key("p/AB") < encode_tree_sort_key("p/ABC"));
    }

    #[test]
    fn normalise_backslashes() {
        assert_eq!(
            normalize_db_path(r"photos\2024\january"),
            "photos/2024/january"
        );
    }

    #[test]
    fn normalise_trims_slashes() {
        assert_eq!(normalize_db_path("/photos/2024/"), "photos/2024");
    }

    #[test]
    fn root_path_preserves_unc() {
        // UNC share: leading `//` must survive (8A network drive as scan root).
        assert_eq!(
            normalize_root_path(r"\\NAS\media\photos"),
            "//NAS/media/photos"
        );
        assert_eq!(normalize_root_path("//NAS/media/"), "//NAS/media");
        // Extra leading slashes collapse to exactly two.
        assert_eq!(normalize_root_path(r"\\\\NAS\share"), "//NAS/share");
    }

    #[test]
    fn root_path_drive_letter_unchanged() {
        // Mapped/local drive paths behave exactly like normalize_db_path.
        assert_eq!(normalize_root_path(r"Z:\music"), "Z:/music");
        assert_eq!(
            normalize_root_path("C:/Users/me/Pictures/"),
            "C:/Users/me/Pictures"
        );
        // 盘根的 / 不能丢，否则 C: 会变成当前目录语义。
        assert_eq!(normalize_root_path("C:/"), "C:/");
        assert_eq!(normalize_root_path("C:\\"), "C:/");
    }

    #[test]
    fn root_path_preserves_posix_absolute_marker() {
        assert_eq!(normalize_root_path("/"), "/");
        assert_eq!(normalize_root_path("/var/lib/"), "/var/lib");
    }

    #[test]
    fn resolve_path_at_root() {
        let result = resolve_media_path("/data/photos", "", "IMG_001.jpg");
        assert!(result.ends_with("IMG_001.jpg"));
        assert!(result.contains("/data/photos/"));
    }

    #[test]
    fn path_depth_empty() {
        assert_eq!(path_depth(""), 0);
    }

    #[test]
    fn path_depth_nested() {
        assert_eq!(path_depth("a/b/c"), 3);
    }

    // ── resolve_within_root：安全边界（S 线 §4.2 / D-001）────────────────────
    //
    // 这些用例走**真实文件系统**（tempfile）而非字符串推演 —— symlink 逃逸、大小写、
    // Windows verbatim 前缀这三类只有真 FS 才暴露得出来。

    mod within_root {
        use super::*;
        use std::fs;

        /// 建根：root/{sub/inner.txt, top.txt}，外加根**外**的 outside/secret.txt。
        fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("root");
            let outside = tmp.path().join("outside");
            fs::create_dir_all(root.join("sub")).unwrap();
            fs::create_dir_all(&outside).unwrap();
            fs::write(root.join("top.txt"), b"t").unwrap();
            fs::write(root.join("sub/inner.txt"), b"i").unwrap();
            fs::write(outside.join("secret.txt"), b"s").unwrap();
            (tmp, root, outside)
        }

        fn root_str(root: &Path) -> String {
            root.to_string_lossy().to_string()
        }

        #[test]
        fn empty_rel_path_resolves_to_root_itself() {
            let (_t, root, _o) = fixture();
            let got = resolve_within_root(&root_str(&root), "").unwrap();
            assert_eq!(got, fs::canonicalize(&root).unwrap());
        }

        #[test]
        fn resolves_nested_and_accepts_both_separators() {
            let (_t, root, _o) = fixture();
            let want = fs::canonicalize(root.join("sub/inner.txt")).unwrap();
            // DB 存正斜杠；Windows 前端也可能回传反斜杠 —— 两种都要认。
            assert_eq!(
                resolve_within_root(&root_str(&root), "sub/inner.txt").unwrap(),
                want
            );
            assert_eq!(
                resolve_within_root(&root_str(&root), "sub\\inner.txt").unwrap(),
                want
            );
            // 冗余分隔符与 `.` 段是良性的，不该误拒。
            assert_eq!(
                resolve_within_root(&root_str(&root), "./sub//inner.txt").unwrap(),
                want
            );
        }

        #[test]
        fn rejects_parent_traversal() {
            let (_t, root, _o) = fixture();
            for evil in [
                "..",
                "../outside/secret.txt",
                "sub/../../outside/secret.txt",
                "sub/..",
                "..\\outside\\secret.txt",
            ] {
                let r = resolve_within_root(&root_str(&root), evil);
                assert!(r.is_err(), "父级穿越未被拒: {evil:?} → {r:?}");
            }
        }

        #[test]
        fn absolute_rel_path_cannot_replace_the_root() {
            // PathBuf::push("/etc") 会**整体替换**路径 —— 切段后不可能触发，此测试钉住该保证。
            let (_t, root, _o) = fixture();
            let outside_abs = _o.join("secret.txt");
            let r = resolve_within_root(&root_str(&root), &outside_abs.to_string_lossy());
            assert!(r.is_err(), "绝对 rel_path 逃逸: {r:?}");

            #[cfg(unix)]
            {
                let r = resolve_within_root(&root_str(&root), "/etc/passwd");
                assert!(r.is_err(), "绝对 unix 路径逃逸: {r:?}");
            }
        }

        #[cfg(windows)]
        #[test]
        fn rejects_colon_components() {
            let (_t, root, _o) = fixture();
            // 盘符 + NTFS 备用数据流（top.txt:hidden 是真实可写的隐藏流）。
            for evil in ["C:", "C:\\Windows", "top.txt:secret"] {
                let r = resolve_within_root(&root_str(&root), evil);
                assert!(r.is_err(), "含 ':' 的段未被拒: {evil:?} → {r:?}");
            }
        }

        #[test]
        fn rejects_nonexistent_path() {
            let (_t, root, _o) = fixture();
            assert!(resolve_within_root(&root_str(&root), "nope.txt").is_err());
            assert!(resolve_within_root(&root_str(&root), "sub/nope/deep.txt").is_err());
        }

        #[test]
        fn rejects_missing_root() {
            let tmp = tempfile::tempdir().unwrap();
            let gone = tmp.path().join("never-existed");
            assert!(resolve_within_root(&gone.to_string_lossy(), "").is_err());
        }

        /// 建一个指向 `target` 的目录级重定向。
        ///
        /// Windows 优先 **junction**（`mklink /J`）：与 `symlink_dir` 不同，它**不需要**开发者
        /// 模式或管理员，因此是任何 Windows 机器（含 CI runner）上都能造出的逃逸载体，且
        /// `canonicalize` 会解析它。symlink_dir 只作兜底。
        fn make_dir_link(target: &Path, link: &Path) -> bool {
            #[cfg(windows)]
            {
                let ok = std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(link)
                    .arg(target)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                ok || std::os::windows::fs::symlink_dir(target, link).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, link).is_ok()
            }
        }

        /// 根内的链接指向根外 → 必须拒。纯字符串校验**抓不到**这一类（rel_path 里没有 `..`）。
        ///
        /// ⚠️ 本用例是 `starts_with` 边界断言的**唯一**守卫 —— 逐段过滤挡掉了 `..`、
        /// canonicalize 挡掉了绝对/不存在路径，能活着走到边界判断的只有链接。经变异验证：
        /// 删掉边界断言后**只有本用例**变红。故它**不允许跳过** —— 建不出链接即硬失败，
        /// 否则「测试全绿」会在无链接权限的机器上悄悄退化成「边界完全没测」。
        #[test]
        fn rejects_link_escaping_root() {
            let (_t, root, outside) = fixture();
            let link = root.join("escape");
            assert!(
                make_dir_link(&outside, &link),
                "无法创建目录链接（Windows junction / Unix symlink 均失败）：\
                 本用例是边界断言的唯一守卫，不能跳过"
            );
            let r = resolve_within_root(&root_str(&root), "escape/secret.txt");
            assert!(r.is_err(), "链接逃出扫描根未被拒: {r:?}");
        }

        /// 指向根**内**的链接放行 —— 它只是同一内容的另一视图，非越界。
        #[test]
        fn allows_link_staying_inside_root() {
            let (_t, root, _o) = fixture();
            let link = root.join("alias");
            assert!(
                make_dir_link(&root.join("sub"), &link),
                "无法创建目录链接：根内链接放行语义无从验证"
            );
            let got = resolve_within_root(&root_str(&root), "alias/inner.txt").unwrap();
            assert_eq!(got, fs::canonicalize(root.join("sub/inner.txt")).unwrap());
        }

        /// `starts_with` 必须按**组件**比较：兄弟目录 `root-evil` 的字符串前缀是 `root`，
        /// 若用字符串前缀判边界，它会被误判为根内 —— 这是个真实漏洞形态。
        #[test]
        fn sibling_dir_sharing_string_prefix_is_not_inside() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("root");
            let evil = tmp.path().join("root-evil");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&evil).unwrap();
            fs::write(evil.join("x.txt"), b"x").unwrap();

            let canonical_root = fs::canonicalize(&root).unwrap();
            let canonical_evil = fs::canonicalize(evil.join("x.txt")).unwrap();
            // 字符串前缀会误判为「在根内」；Path::starts_with 不会。
            assert!(canonical_evil
                .to_string_lossy()
                .starts_with(&*canonical_root.to_string_lossy()));
            assert!(!canonical_evil.starts_with(&canonical_root));
        }
    }
}
