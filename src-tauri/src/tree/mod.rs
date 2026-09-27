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
