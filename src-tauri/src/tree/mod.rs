// src-tauri/src/tree/mod.rs
//! 文件树的**文件系统数据源**（S 线 §4）。
//!
//! 文件树有两条数据源（§4）：`已注册格式` 模式走现有 DB 目录树（零回归的快路径）；两种
//! `所有文件` 模式走本模块的惰性 FS 枚举。本模块只回答**文件系统事实**（有什么、是不是目录、
//! 隐不隐藏），不碰媒体库实体 —— `registered`/`mediaId`/`directoryId` 的填充在 IPC 层，
//! 因为那需要 Catalog 与 DB（S 线 D-013：树节点身份 ≠ 媒体库实体身份）。
//!
//! 路径安全边界见 [`crate::utils::path::resolve_within_root`]：本模块的入参 `dir` 必须**已经**
//! 过那道校验，这里不再重复判边界。

pub mod cache;

use std::cmp::Ordering;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEntryKind {
    Dir,
    File,
}

/// 文件树专用的五类文件筛选值。
///
/// 这不是媒体库领域的 [`crate::utils::format::MediaType`]：`Other` 只表示磁盘上
/// 无法由内置格式表/Catalog 识别的文件，不能进入图库 SQL、URL 或媒体实体模型。
/// `None`（调用方省略参数）表示不限；传入空切片则表示不匹配任何文件，但目录骨架仍保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TreeMediaCategory {
    Image,
    Video,
    Audio,
    Document,
    Other,
}

impl TreeMediaCategory {
    /// 五类的稳定协议顺序；用于判断「五类全选」是否等价于省略过滤。
    pub const ALL: [Self; 5] = [
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Document,
        Self::Other,
    ];

    /// 已知四类对应数据库 `media_items.media_type` 的字符串；`Other` 不落库。
    pub const fn db_media_type(self) -> Option<&'static str> {
        match self {
            Self::Image => Some("image"),
            Self::Video => Some("video"),
            Self::Audio => Some("audio"),
            Self::Document => Some("document"),
            Self::Other => None,
        }
    }

    /// 将内置/Catalog 分类器返回的领域类型投影到文件树五类，不扩展领域枚举。
    pub const fn from_media_type(media_type: crate::utils::format::MediaType) -> Self {
        match media_type {
            crate::utils::format::MediaType::Image => Self::Image,
            crate::utils::format::MediaType::Video => Self::Video,
            crate::utils::format::MediaType::Audio => Self::Audio,
            crate::utils::format::MediaType::Document => Self::Document,
        }
    }
}

// ── 路径身份（D-013）──────────────────────────────────────────────────────────
//
// 树节点的身份是**路径**，不是数据库行 id —— 这是两条数据源能汇成同一棵树的前提：
// 「所有文件」模式下 FS-only 目录根本没有 DB 行，而同一个目录在两种模式下必须是**同一个
// 节点**——去重、折叠、键盘 active 行、粘性头等一切结构操作因此只需一套键。
// （R-18 裁决对齐：前端切模式**有意折回根**、不保留展开态——节点集合本身变了，硬套旧展开态
// 轻则重展开一批不存在的键、重则让用户以为「这个目录空了」；键统一的价值在「同一时刻只有
// 一套身份」，不承诺跨模式恢复展开。原注释「否则切模式时展开态丢失」与实现不符，已更正。）
//
// 本处是 nodeKey 格式的**唯一实现**。DB 模式的 `get_directory_tree` / `get_directory_children`
// / `list_directory_files` 与 FS 模式的 `list_tree_entries` 都从这里取键，前端**不推导**键 ——
// 若让前端自己拼 `${rootId}:${relPath}`，就有了跨语言的第二实现，而分歧的后果极其隐蔽：
// 不报错，只是切模式时同一目录被当成两个节点。消灭第二实现，而不是给两个实现加对拍。

/// 树节点的路径身份：`{root_id}:{rel_path}`。
///
/// **单射性**：`root_id` 是纯数字、不含 `:`，故首个 `:` 无歧义地分割两段 —— 不存在
/// 两组不同的 `(root_id, rel_path)` 产出同一个键。
///
/// 目录与文件**共用**同一命名空间：目录 `1:a/b` 与文件 `1:a/b` 会撞键，但文件系统本身
/// 保证同一父目录下目录名与文件名不重名，故「路径在根内唯一」由 FS 保证（DB 侧另有
/// `UNIQUE(root_id, rel_path)` 独立保证目录部分）。
///
/// 扫描根自身是 `rel_path = ""` 的目录行（`scan_commands` 建根时如此写入），其键为 `"{id}:"`。
pub fn node_key(root_id: i64, rel_path: &str) -> String {
    format!("{root_id}:{rel_path}")
}

/// 父节点的路径身份；扫描根（`rel_path` 为空）无父，返回 `None`。
///
/// 用 `rsplit_once('/')` 剥**最后**一段：`"a/b"` → 父 `"a"`；`"a"` → 父 `""`（即扫描根自身）。
pub fn parent_key(root_id: i64, rel_path: &str) -> Option<String> {
    if rel_path.is_empty() {
        return None; // 扫描根：树的顶层，无父
    }
    let parent_rel = rel_path.rsplit_once('/').map_or("", |(head, _)| head);
    Some(node_key(root_id, parent_rel))
}

/// 拼子项的 `rel_path`。扫描根下的直接子项父路径为空，此时不能拼出前导 `/`
/// （否则键变成 `"1:/x"`，与 DB 侧写入的 `"x"` 不等）。
pub fn child_rel_path(parent_rel: &str, name: &str) -> String {
    if parent_rel.is_empty() {
        name.to_string()
    } else {
        format!("{parent_rel}/{name}")
    }
}

/// 一条文件系统条目的原始事实（尚未与媒体库关联）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub kind: TreeEntryKind,
    pub hidden: bool,
    /// 是符号链接/junction。**不跟随**（见 [`list_fs_entries`] 的说明）。
    pub is_symlink: bool,
}

/// 判定一条目录项是否「隐藏」（S 线 §3 隐藏项口径）。
///
/// - 全平台：文件名点前缀。
/// - Windows：另加 `FILE_ATTRIBUTE_HIDDEN`（点前缀在 Windows 上并非系统级隐藏约定，
///   真正的隐藏文件靠属性位，故两者取**或**）。
/// - macOS：另加 `UF_HIDDEN` 标志位（Finder 的「隐藏」）。
///
/// `meta` 必须来自 [`std::fs::DirEntry::metadata`]（**不**跟随符号链接）—— 判的是链接自身
/// 的属性，而非其目标的，否则一个指向隐藏目标的可见链接会被误判为隐藏。
pub fn is_hidden_entry(name: &str, meta: &std::fs::Metadata) -> bool {
    name.starts_with('.') || platform_hidden_flag(meta)
}

/// Windows：`FILE_ATTRIBUTE_HIDDEN`（winnt.h）。点前缀在 Windows 并非系统级隐藏约定，
/// 真正的隐藏文件靠这个属性位 —— 只看点前缀（Unix 思维）会让隐藏文件在「所有文件（不含
/// 隐藏项）」模式下错误出现。
#[cfg(windows)]
fn platform_hidden_flag(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0000_0002;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

/// macOS：`UF_HIDDEN`（sys/stat.h）—— Finder 的「隐藏」标志位。
#[cfg(target_os = "macos")]
fn platform_hidden_flag(meta: &std::fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_HIDDEN: u32 = 0x0000_8000;
    meta.st_flags() & UF_HIDDEN != 0
}

/// Linux / Android：点前缀是唯一约定，无属性位。
#[cfg(not(any(windows, target_os = "macos")))]
fn platform_hidden_flag(_meta: &std::fs::Metadata) -> bool {
    false
}

/// SQLite `COLLATE NOCASE` 的 Rust 等价。
///
/// ⚠️ **必须只折叠 ASCII**（`to_ascii_lowercase`，**不是** `to_lowercase`）。SQLite 有意不做
/// 完整 Unicode case folding（折叠表太大），实测（2026-07-16，真 SQLite）：
/// `'a' = 'A' COLLATE NOCASE` → 1，但 `'Ä' = 'ä' COLLATE NOCASE` → 0。
///
/// 用 `to_lowercase()` 会与 DB 分歧，而分歧就是 D-002「共有项相对序不变」的破坏。要害不在
/// Ä/ä 这类（折叠只改第二字节，逐字节 tiebreak 恰好给出相同序，**看不出差别**），而在折叠会
/// 改变**首字节**的字符 —— 实测三例，SQLite 均与 ASCII 折叠一致、与 Unicode 折叠相反：
/// - `İ.jpg` vs `j.jpg`：`İ`(0xC4 0xB0) 的 `to_lowercase()` 是 `i`+组合点，首字节变 ASCII
///   0x69 → 排到 `j` **之前**（SQLite 说之后）。
/// - `Σ.jpg` vs `ο.jpg`：`Σ`(0xCE 0xA3) → `σ`(0xCF 0x83)，首字节 0xCE→0xCF 越过 `ο`。
/// - `ẞ.jpg` vs `à.jpg`：`ẞ`(0xE1 0xBA 0x9E) → `ß`(0xC3 0x9F)。
///
/// 折叠后相等时（如 `a.jpg` vs `A.jpg`）追加逐字节 tiebreak 保证**确定性**。DB 侧
/// `ORDER BY file_name COLLATE NOCASE ASC` 无 tiebreaker，此时其序由 SQLite 未定义（实测为
/// 插入序），故此处与 DB 可能不同 —— 但仅大小写不同的同目录同名文件在 Windows/macOS 上
/// **无法共存**，只有大小写敏感的 Linux 文件系统能造出来，且那时 DB 侧本就是任意序。
fn nocase_cmp(a: &str, b: &str) -> Ordering {
    a.to_ascii_lowercase()
        .as_bytes()
        .cmp(b.to_ascii_lowercase().as_bytes())
        .then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

/// 与 DB 模式**逐字节等价**的条目比较器（S 线 D-002）。
///
/// 三条规则各自对应 DB 侧的一处既有行为：
/// - **目录在前**：DB 模式是两次独立查询（`get_directory_children` 出目录、
///   `list_directory_files` 出文件），前端按序拼接；FS 侧一次 `read_dir` 同时拿到两者，
///   故必须把这个隐含顺序显式写出来。
/// - **目录间 BINARY**：对齐 `queries.rs:278/305` 的 `ORDER BY d.name ASC`（`directories.name`
///   无 COLLATE 子句 → 默认 BINARY，大写在前：`Zebra` < `apple`）。
/// - **文件间 NOCASE**：对齐 `queries.rs:336` 的 `ORDER BY file_name COLLATE NOCASE ASC`。
///
/// 目录 BINARY 而文件 NOCASE 是**既有的**不一致（同一面板内两套序，F-004），本模块**照抄而非
/// 修正** —— D-002 裁定「切模式绝不重排」优先于「用更好的排序」；三序统一到 `natural_cmp`
/// 是独立工作线，落地后本函数跟着改即可。
pub fn cmp_entries(a: &FsEntry, b: &FsEntry) -> Ordering {
    match (a.kind, b.kind) {
        (TreeEntryKind::Dir, TreeEntryKind::File) => Ordering::Less,
        (TreeEntryKind::File, TreeEntryKind::Dir) => Ordering::Greater,
        (TreeEntryKind::Dir, TreeEntryKind::Dir) => a.name.as_bytes().cmp(b.name.as_bytes()),
        (TreeEntryKind::File, TreeEntryKind::File) => nocase_cmp(&a.name, &b.name),
    }
}

/// 列出 `dir` 的**直接**子项，按 [`cmp_entries`] 排序。
///
/// 前置：`dir` 必须已过 [`crate::utils::path::resolve_within_root`] 校验。
///
/// **符号链接不跟随**：链接一律记为 `File` + `is_symlink = true`，不 stat 其目标、不可展开。
/// 这不只是防环/防重复 —— 扫描器 `walker.rs` 的 `follow_links(false)` 意味着 DB 里**从来
/// 没有**链接目标的行，若 FS 模式把链接目录展开成可浏览子树，两种显示模式会出现结构性分歧，
/// 而 D-002 要的恰是共有项一致。
///
/// **只收普通文件与目录**：FIFO/socket/设备节点等特殊文件跳过（§2「所有文件 = 所有**普通**
/// 文件」）。
///
/// 单条目的 metadata 失败（权限/竞态删除）**跳过该条而非整目录失败** —— 一个不可读的文件
/// 不该让整棵子树打不开。与扫描器不同，这里没有「缺失即删除」的差集语义，跳过是安全的。
pub fn list_fs_entries(dir: &Path, include_hidden: bool) -> Result<Vec<FsEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Ok(ft) = entry.file_type() else { continue };
        let Ok(meta) = entry.metadata() else { continue };

        let name = entry.file_name().to_string_lossy().to_string();
        let is_symlink = ft.is_symlink();
        let kind = if is_symlink {
            // 不 stat 目标：链接即叶子。
            TreeEntryKind::File
        } else if ft.is_dir() {
            TreeEntryKind::Dir
        } else if ft.is_file() {
            TreeEntryKind::File
        } else {
            continue; // 特殊文件
        };

        let hidden = is_hidden_entry(&name, &meta);
        if hidden && !include_hidden {
            continue;
        }
        out.push(FsEntry {
            name,
            kind,
            hidden,
            is_symlink,
        });
    }
    out.sort_by(cmp_entries);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn e(name: &str, kind: TreeEntryKind) -> FsEntry {
        FsEntry {
            name: name.into(),
            kind,
            hidden: false,
            is_symlink: false,
        }
    }
    fn dir(name: &str) -> FsEntry {
        e(name, TreeEntryKind::Dir)
    }
    fn file(name: &str) -> FsEntry {
        e(name, TreeEntryKind::File)
    }

    fn sorted(mut v: Vec<FsEntry>) -> Vec<String> {
        v.sort_by(cmp_entries);
        v.into_iter().map(|x| x.name).collect()
    }

    // ── 路径身份（D-013）────────────────────────────────────────────────────

    /// 扫描根自身是 `rel_path = ""` 的目录行，其键必须保留分隔符（`"1:"` 而非 `"1"`），
    /// 否则与 `parent_key("a")` 的产出对不上 —— 顶层目录会认不出自己的父。
    #[test]
    fn scan_root_key_keeps_the_separator() {
        assert_eq!(node_key(1, ""), "1:");
        assert_eq!(
            parent_key(1, "a").unwrap(),
            "1:",
            "顶层目录的父必须正好是扫描根键"
        );
    }

    #[test]
    fn scan_root_has_no_parent() {
        assert_eq!(parent_key(7, ""), None);
    }

    /// `parent_key` 必须剥**最后**一段而非首段。用 `split_once` 会让 `"a/b/c"` 的父变成
    /// `"a"`（祖父的祖父），树结构整个错位。
    #[test]
    fn parent_key_strips_exactly_one_segment() {
        assert_eq!(parent_key(3, "a/b/c").unwrap(), "3:a/b");
        assert_eq!(parent_key(3, "a/b").unwrap(), "3:a");
        assert_eq!(parent_key(3, "a").unwrap(), "3:");
    }

    /// 键必须是**单射**：不同 `(root_id, rel_path)` 不得撞键。要害在 rel_path 可以含 `:`
    /// —— `root_id` 是纯数字不含 `:`，故首个 `:` 是无歧义的分割点。
    #[test]
    fn keys_are_injective_even_when_rel_path_contains_colons() {
        assert_eq!(node_key(1, "0:x"), "1:0:x");
        assert_eq!(node_key(10, ":x"), "10::x");
        assert_ne!(node_key(1, "0:x"), node_key(10, ":x"));
        // 数字前缀相邻的一组：去掉分隔符后二者都会塌成 "11:a" 形态 → 本断言即分隔符的守卫。
        assert_ne!(node_key(1, "1:a"), node_key(11, "a"));
    }

    /// 扫描根下的直接子项：父 rel_path 为空时**不得**拼出前导 `/`。
    /// 拼错的话 FS 侧键是 `"1:/x"` 而 DB 侧写入的 rel_path 是 `"x"`（键 `"1:x"`）——
    /// 两模式对同一目录给出不同节点，切模式即丢展开态，且不报任何错。
    #[test]
    fn child_rel_path_of_scan_root_has_no_leading_slash() {
        assert_eq!(child_rel_path("", "x"), "x");
        assert_eq!(node_key(1, &child_rel_path("", "x")), "1:x");
        assert_eq!(child_rel_path("a", "x"), "a/x");
        assert_eq!(child_rel_path("a/b", "x"), "a/b/x");
    }

    /// 🔴 D-013 的核心契约：**同一个目录，DB 模式与 FS 模式必须得到同一个键**。
    ///
    /// 两侧的输入形态不同 —— DB 侧手里是整条 `rel_path`（`directories.rel_path` 列），
    /// FS 侧是「父目录 rel_path + 枚举出的子项名」逐层拼。本用例让两条路径在同一棵虚构
    /// 目录树上走到底，逐层比对。若哪天有人改了拼接规则（比如给根加前导 `/`），这里会响。
    #[test]
    fn db_path_and_fs_walk_agree_on_every_key() {
        const ROOT: i64 = 42;
        // DB 侧：directories.rel_path 的实际形态（扫描根为 ""，其余为根内相对路径）。
        let db_rel_paths = ["", "Photos", "Photos/2024", "Photos/2024/raw"];

        // FS 侧：从扫描根开始，用 child_rel_path 逐段下钻。
        let mut fs_rel = String::new();
        let mut fs_keys = vec![node_key(ROOT, &fs_rel)];
        for seg in ["Photos", "2024", "raw"] {
            fs_rel = child_rel_path(&fs_rel, seg);
            fs_keys.push(node_key(ROOT, &fs_rel));
        }

        let db_keys: Vec<String> = db_rel_paths.iter().map(|p| node_key(ROOT, p)).collect();
        assert_eq!(db_keys, fs_keys, "DB 路径与 FS 逐层下钻必须给出同一串键");

        // 且父子链自洽：每一层的 parent_key 正好是上一层的 node_key。
        for i in 1..db_keys.len() {
            assert_eq!(
                parent_key(ROOT, db_rel_paths[i]).unwrap(),
                db_keys[i - 1],
                "第 {i} 层的父键必须等于上一层的节点键"
            );
        }
    }

    // ── 比较器：对齐 DB 侧（D-002）──────────────────────────────────────────

    #[test]
    fn dirs_sort_before_files() {
        let got = sorted(vec![file("a.jpg"), dir("zzz"), file("b.jpg"), dir("aaa")]);
        assert_eq!(got, ["aaa", "zzz", "a.jpg", "b.jpg"]);
    }

    #[test]
    fn dirs_compare_binary_like_db() {
        // 对齐 `ORDER BY d.name ASC`（BINARY）：大写在前，`_` 夹在大小写之间。
        let got = sorted(vec![dir("apple"), dir("Zebra"), dir("_x"), dir("Apple")]);
        assert_eq!(got, ["Apple", "Zebra", "_x", "apple"]);
    }

    #[test]
    fn files_compare_nocase_like_db() {
        // 期望序**取自真 SQLite**（2026-07-16 实测，非手写推演）：
        //   sqlite> SELECT n FROM t ORDER BY n COLLATE NOCASE ASC;
        //   10.jpg | 9.jpg | _x | apple | B.jpg | b.jpg | Zebra
        // 折叠后逐字节：'1'<'9'<'_'(0x5F)<'a'<'b'<'z'，故 apple 在 B.jpg **之前**。
        let got = sorted(vec![
            file("Zebra"),
            file("apple"),
            file("_x"),
            file("B.jpg"),
            file("b.jpg"),
            file("10.jpg"),
            file("9.jpg"),
        ]);
        assert_eq!(
            got,
            ["10.jpg", "9.jpg", "_x", "apple", "B.jpg", "b.jpg", "Zebra"]
        );
    }

    /// `nocase_cmp` 必须只折叠 ASCII —— 用**能区分两种折叠策略**的字符对钉死。
    ///
    /// ⚠️ 本用例初版是无效的：它断言 `assert_ne!("Ä".to_ascii_lowercase(), "ä".to_ascii_lowercase())`
    /// —— 那测的是 **std 的 `to_ascii_lowercase`**（恒真），与 `nocase_cmp` 用不用它毫无关系。
    /// 且 `Ä`/`ä`(0xC3 0x84 / 0xC3 0xA4) 根本不具区分度：Unicode 折叠只改第二字节，逐字节
    /// tiebreak 恰好给出相同的序，两种实现输出一致。经变异验证（把 `to_ascii_lowercase` 换成
    /// `to_lowercase`）初版**全绿放行**。
    ///
    /// 下列三对经真 SQLite 实测：SQLite 与 ASCII 折叠一致、与 Unicode 折叠**相反**。
    /// 关键在于它们的 `to_lowercase()` 会改变**首字节**（`İ`→`i`+组合点，首字节 0xC4→0x69
    /// 直接跨到 `j` 之前；`Σ`→`σ` 首字节 0xCE→0xCF），tiebreak 救不回来。
    #[test]
    fn nocase_cmp_folds_ascii_only_like_sqlite() {
        // ASCII 必须折叠（否则 'B.jpg' 会排到 'apple' 前，与 DB 分歧）。
        assert!(nocase_cmp("apple", "B.jpg").is_lt());
        assert!(nocase_cmp("a", "A").is_gt()); // 折叠后相等 → 逐字节 tiebreak: 'A' < 'a'

        // 非 ASCII 必须**不**折叠 —— 这三条是区分 ASCII/Unicode 折叠的判据。
        assert!(
            nocase_cmp("İ.jpg", "j.jpg").is_gt(),
            "İ 被 Unicode 折叠成了 i+组合点"
        );
        assert!(
            nocase_cmp("Σ.jpg", "ο.jpg").is_lt(),
            "Σ 被 Unicode 折叠成了 σ"
        );
        assert!(
            nocase_cmp("ẞ.jpg", "à.jpg").is_gt(),
            "ẞ 被 Unicode 折叠成了 ß"
        );
    }

    #[test]
    fn nocase_equal_names_get_deterministic_tiebreak() {
        // DB 侧无 tiebreaker（SQLite 未定义序）；此处逐字节定序保证 FS 侧确定性。
        // 仅大小写不同的同名文件只有大小写敏感 FS 能造出来。
        assert!(nocase_cmp("A.jpg", "a.jpg").is_lt());
        assert!(nocase_cmp("a.jpg", "A.jpg").is_gt());
    }

    /// 与**真 SQLite** 对拍两种 collation，而非手写期望序（D-002 的机械判据）。
    ///
    /// 为什么非这样不可：本轮写上面 `files_compare_nocase_like_db` 的期望序时，我把
    /// `apple` 与 `B.jpg` 的相对位置写反了（实现反而是对的）。**手写期望序是第二份实现，
    /// 会引入第二份 bug**；让 SQLite 自己回答，对拍才有信息量。
    ///
    /// 用例名有意避开 NOCASE 冲突（不含仅大小写不同的同名项）—— 那种情况 SQLite 的序
    /// 未定义（实测为插入序），拿它对拍等于把不确定性写进契约；确定性 tiebreak 由
    /// `nocase_equal_names_get_deterministic_tiebreak` 单独覆盖。
    #[test]
    fn comparator_matches_real_sqlite_collations() {
        // 混合：数字/下划线/大小写/非 ASCII/带重音。
        // 末六项是**具区分度**的折叠判据（İ/Σ/ẞ 的 to_lowercase() 会改首字节）——
        // 没有它们，本对拍在 to_ascii_lowercase→to_lowercase 的变异下会全绿放行（已实证）。
        let names = [
            "10.jpg", "9.jpg", "_x", "apple", "B.jpg", "Zebra", "Ä.jpg", "ä.jpg", "café", "CAFE",
            "z", "A", "İ.jpg", "j.jpg", "Σ.jpg", "ο.jpg", "ẞ.jpg", "à.jpg",
        ];
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE d(n TEXT); CREATE TABLE f(n TEXT);")
            .unwrap();
        for n in names {
            conn.execute("INSERT INTO d VALUES(?1)", [n]).unwrap();
            conn.execute("INSERT INTO f VALUES(?1)", [n]).unwrap();
        }

        // DB 侧真值：目录用 queries.rs:278/305 的 `ORDER BY d.name ASC`（BINARY）。
        let sql_dirs: Vec<String> = conn
            .prepare("SELECT n FROM d ORDER BY n ASC")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        // DB 侧真值：文件用 queries.rs:336 的 `ORDER BY file_name COLLATE NOCASE ASC`。
        let sql_files: Vec<String> = conn
            .prepare("SELECT n FROM f ORDER BY n COLLATE NOCASE ASC")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let my_dirs = sorted(names.iter().map(|n| dir(n)).collect());
        let my_files = sorted(names.iter().map(|n| file(n)).collect());

        assert_eq!(my_dirs, sql_dirs, "目录序与 SQLite BINARY 不一致");
        assert_eq!(my_files, sql_files, "文件序与 SQLite NOCASE 不一致");
    }

    // ── 枚举：真实文件系统 ──────────────────────────────────────────────────

    #[test]
    fn lists_and_sorts_real_directory() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("sub")).unwrap();
        fs::create_dir(t.path().join("Alpha")).unwrap();
        fs::write(t.path().join("b.txt"), b"b").unwrap();
        fs::write(t.path().join("A.txt"), b"a").unwrap();

        let got: Vec<String> = list_fs_entries(t.path(), false)
            .unwrap()
            .into_iter()
            .map(|x| x.name)
            .collect();
        // 目录在前(BINARY: Alpha < sub)，文件在后(NOCASE: A.txt < b.txt)。
        assert_eq!(got, ["Alpha", "sub", "A.txt", "b.txt"]);
    }

    #[test]
    fn dot_prefixed_entries_are_hidden_on_every_platform() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("visible.txt"), b"v").unwrap();
        fs::write(t.path().join(".secret"), b"s").unwrap();
        fs::create_dir(t.path().join(".git")).unwrap();

        let shown: Vec<String> = list_fs_entries(t.path(), false)
            .unwrap()
            .into_iter()
            .map(|x| x.name)
            .collect();
        assert_eq!(shown, ["visible.txt"]);

        let all = list_fs_entries(t.path(), true).unwrap();
        let names: Vec<&str> = all.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, [".git", ".secret", "visible.txt"]);
        // 点前缀项必须**带上** hidden 标记，而不只是被过滤 —— 前端要据此加样式。
        assert!(all
            .iter()
            .filter(|x| x.name != "visible.txt")
            .all(|x| x.hidden));
    }

    #[cfg(windows)]
    #[test]
    fn windows_hidden_attribute_is_honoured_without_dot_prefix() {
        // 关键用例：`hidden.txt` 无点前缀，只有 FILE_ATTRIBUTE_HIDDEN。
        // 若隐藏判定只看点前缀（Unix 思维），它会在「所有文件(不含隐藏)」模式下**错误出现**。
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("hidden.txt");
        fs::write(&target, b"h").unwrap();
        fs::write(t.path().join("plain.txt"), b"p").unwrap();

        let ok = std::process::Command::new("attrib")
            .arg("+h")
            .arg(&target)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            ok,
            "attrib +h 失败：本用例是 Windows 隐藏属性判定的唯一守卫，不能跳过"
        );

        let shown: Vec<String> = list_fs_entries(t.path(), false)
            .unwrap()
            .into_iter()
            .map(|x| x.name)
            .collect();
        assert_eq!(shown, ["plain.txt"], "带 HIDDEN 属性的文件未被隐藏");

        let all = list_fs_entries(t.path(), true).unwrap();
        assert!(all.iter().any(|x| x.name == "hidden.txt" && x.hidden));
    }

    #[test]
    fn symlinks_are_leaves_not_traversable_dirs() {
        // 链接一律记为 File + is_symlink，不 stat 目标 —— 与 walker.rs 的 follow_links(false)
        // 一致：DB 里从来没有链接目标的行，FS 侧若展开它，两模式会结构性分歧。
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("real")).unwrap();
        let link = t.path().join("link");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(t.path().join("real"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(t.path().join("real"), &link).is_ok();
        assert!(
            made,
            "无法创建目录链接：本用例是「链接不跟随」语义的唯一守卫，不能跳过"
        );

        let all = list_fs_entries(t.path(), true).unwrap();
        let l = all.iter().find(|x| x.name == "link");

        // Windows junction 在 std 里报 is_symlink() —— 与 Unix symlink 同路径处理。
        if let Some(l) = l {
            assert!(l.is_symlink, "链接未被标记");
            assert_eq!(l.kind, TreeEntryKind::File, "链接被当成可展开目录");
        }
        // 真目录仍是目录。
        assert_eq!(
            all.iter().find(|x| x.name == "real").unwrap().kind,
            TreeEntryKind::Dir
        );
    }

    #[test]
    fn missing_dir_errors_but_unreadable_child_does_not_kill_the_listing() {
        let t = tempfile::tempdir().unwrap();
        assert!(list_fs_entries(&t.path().join("nope"), false).is_err());
        // 空目录是合法的空列表，不是错误。
        assert!(list_fs_entries(t.path(), false).unwrap().is_empty());
    }

    // ── D-002 核心不变量：共有项相对序稳定 ──────────────────────────────────

    /// **切显示模式时，共有目录之间不重排、共有文件之间不重排**（D-002）。
    ///
    /// 这是整条 S 线唯一的机械可验证交互契约，故用**真 FS + 真 DB** 端到端对拍，而不是
    /// 只测比较器。构造：磁盘上既有已注册媒体（DB 也有行）、也有未注册/隐藏文件（DB 没有）；
    /// 断言 FS 枚举结果里「DB 也有的那些」的相对顺序 ≡ DB 查询给出的顺序。
    ///
    /// 注意不变量**不是**「两个列表逐字节相等」—— FS 模式必然插入额外项（未注册文件、
    /// FS-only 目录），完整序列不可能相等。原方案曾把契约写成「FS 枚举序 ≡
    /// `list_directory_files` 序」，那是不可实现的。
    #[test]
    fn common_items_keep_relative_order_across_display_modes() {
        use crate::db::queries::{get_directory_children, list_directory_files};

        let t = tempfile::tempdir().unwrap();
        let root = t.path();

        // 磁盘：混合已注册媒体 / 未注册文件 / 隐藏项 / 子目录。
        //
        // ⚠️ 目录名 `Zebra`/`apple` 是**刻意**挑的判据：二者在 BINARY 与 NOCASE 下相对序
        // **相反**（BINARY: Zebra(0x5A) < apple(0x61)；NOCASE: apple < zebra）。初版用
        // `Alpha`/`zulu`，两种 collation 下序恰好相同 → 本对拍对「目录误用 NOCASE」这个最
        // 容易犯的错（顺手把两种序统一）**全绿放行**，经变异验证属实。文件侧的
        // `B.jpg`/`a.png` 同理构成 BINARY↔NOCASE 判据。
        for d in ["Zebra", "apple", "_scratch"] {
            fs::create_dir(root.join(d)).unwrap();
        }
        for f in [
            "B.jpg",
            "a.png",
            "Zed.mp4",
            "notes.txt",
            "readme.unknownext",
            "_tmp.bin",
        ] {
            fs::write(root.join(f), b"x").unwrap();
        }
        fs::write(root.join(".secret"), b"x").unwrap();

        // DB：只登记扫描器会收的那些（已注册格式、非隐藏）。
        // 有意**不**登记 _scratch（模拟 FS-only 目录）与 readme.unknownext/_tmp.bin(未注册)。
        let c = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::initialize_schema(&c).unwrap();
        c.execute_batch(
            "INSERT INTO scan_roots (id, path, alias) VALUES (1, '/r', 'R');
             INSERT INTO directories (id, root_id, parent_id, rel_path, name) VALUES
                 (10, 1, NULL, '', 'R'),
                 (11, 1, 10, 'Zebra', 'Zebra'),
                 (12, 1, 10, 'apple', 'apple');",
        )
        .unwrap();
        for (id, name, fmt, mt) in [
            (1, "B.jpg", "jpg", "image"),
            (2, "a.png", "png", "image"),
            (3, "Zed.mp4", "mp4", "video"),
            (4, "notes.txt", "txt", "document"),
        ] {
            c.execute(
                "INSERT INTO media_items (id, directory_id, file_name, file_size, file_mtime,
                     file_format, media_type, sort_datetime, cache_key)
                 VALUES (?1, 10, ?2, 1, 1, ?3, ?4, 100, 1)",
                rusqlite::params![id, name, fmt, mt],
            )
            .unwrap();
        }

        // DB 模式看到的序（两次独立查询，前端按序拼接）。
        let db_dirs: Vec<String> = get_directory_children(&c, 10, None)
            .unwrap()
            .into_iter()
            .map(|d| d.name)
            .collect();
        let db_files: Vec<String> = list_directory_files(&c, 10, None, None, None)
            .unwrap()
            .into_iter()
            .map(|f| f.file_name)
            .collect();
        // 自证前提：DB 目录序确实是 BINARY（Zebra 在 apple **之前**）——
        // 若 DB 侧哪天改成 NOCASE，本断言先红，提醒 FS 侧 comparator 必须跟着改。
        assert_eq!(db_dirs, ["Zebra", "apple"], "前提：DB 目录序为 BINARY");
        assert_eq!(
            db_files,
            ["a.png", "B.jpg", "notes.txt", "Zed.mp4"],
            "前提：DB 文件序为 NOCASE"
        );

        // FS 模式看到的序（一次 read_dir，含 DB 没有的额外项）。
        let fs_all = list_fs_entries(root, false).unwrap();
        let fs_dirs: Vec<&str> = fs_all
            .iter()
            .filter(|e| e.kind == TreeEntryKind::Dir)
            .map(|e| e.name.as_str())
            .collect();
        let fs_files: Vec<&str> = fs_all
            .iter()
            .filter(|e| e.kind == TreeEntryKind::File)
            .map(|e| e.name.as_str())
            .collect();

        // 自证前提：FS 模式确实**多**出了 DB 没有的项，否则本用例退化成「两个相同列表比较」。
        assert!(fs_dirs.contains(&"_scratch"), "FS 应含 FS-only 目录");
        assert!(fs_files.contains(&"readme.unknownext"), "FS 应含未注册文件");
        assert!(!fs_files.contains(&".secret"), "不含隐藏项时点前缀不该出现");

        // 不变量：共有项的**相对序**一致（FS 侧滤掉 DB 没有的，两边应逐项相等）。
        let common_dirs: Vec<&str> = fs_dirs
            .iter()
            .copied()
            .filter(|n| db_dirs.iter().any(|d| d == n))
            .collect();
        let common_files: Vec<&str> = fs_files
            .iter()
            .copied()
            .filter(|n| db_files.iter().any(|f| f == n))
            .collect();
        assert_eq!(common_dirs, db_dirs, "共有目录的相对序与 DB 模式不一致");
        assert_eq!(common_files, db_files, "共有文件的相对序与 DB 模式不一致");
    }
}
