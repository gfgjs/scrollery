//! 文件树「所有文件」两态的受限 IPC（S 线 §4）。
//!
//! 本模块的入参 `rel_path` **来自 WebView，是不可信输入**。所有命令必须先经
//! [`resolve_within_root`] 拿到校验过的绝对路径，再做任何文件系统动作。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::info;

use super::blocking::read_blocking;
use super::reveal::reveal_path;
use crate::db::queries::{
    find_directory_id, get_scan_root, map_child_directory_ids, map_media_entities,
};
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::tree::cache::{slice_page, SnapshotKey};
use crate::tree::{
    child_rel_path, list_fs_entries, node_key, FsEntry, TreeEntryKind, TreeMediaCategory,
};
use crate::utils::path::resolve_within_root;

/// 一页的条目上限（S 线 §4.2）。
const PAGE_SIZE: usize = 200;

/// 只列目录 / 只列文件 / 全列。
///
/// 存在的理由是**对齐两种模式的结构**：DB 模式本就是「子目录一次给全（`get_directory_children`
/// 不分页）+ 文件按页给（`list_directory_files`）」，前端模型据此建成（子目录注入拍平数组、
/// 文件挂在 `node.files` 上分页）。若 FS 模式返回目录与文件混在一起的分页流，前端要么循环拉完
/// 全部页（10 万项目录 = 500 次 IPC 往返，而快照本就全量物化在内存里，纯属白跑），要么维护
/// 第二套模型。加一个过滤参数即可让两种模式走同一个模型。
///
/// 过滤发生在**取快照之后**，而快照缓存键 `(root_id, rel_path, include_hidden)` **不含 kind**
/// —— 故「先拉目录再拉文件」复用同一份快照，不会重复枚举磁盘。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TreeKindFilter {
    Dirs,
    Files,
}

/// 文件树的显示模式（S 线 §3）。
///
/// `RegisteredOnly` **不走本模块** —— 那是现有 DB 目录树快路径（零回归）。它在此枚举里出现
/// 只为让前端用同一个类型表达三态；后端收到它即报错，属调用方 bug。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TreeDisplayMode {
    RegisteredOnly,
    AllFiles,
    AllFilesWithHidden,
}

/// 一条树节点（S 线 §4.1）。
///
/// **两种身份严格分开**（D-013）：
/// - `node_key`/`parent_key` = **路径身份**，必有，跨 DB/FS 模式稳定，供 DOM key、展开态、
///   键盘导航使用。
/// - `directory_id`/`media_id` = **实体身份**，只在 DB 确有对应行时才有。禁止伪造 ——
///   `mediaRoute.ts` 对非 doc/audio 兜底到 `/view/{id}`，伪造 id 不会报错，会把未知文件
///   静默送进图片查看器。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// 路径身份：`{root_id}:{rel_path}`。
    pub node_key: String,
    pub parent_key: String,
    pub root_id: i64,
    pub rel_path: String,
    pub name: String,
    /// `"dir"` | `"file"`。
    pub kind: String,
    pub hidden: bool,
    /// 格式是否已注册（内置表 ∪ exotic Catalog）。**与「是否入库」无关**：隐藏目录里的 PNG
    /// `registered = true` 但没有 `media_id`（扫描器整棵剪掉了它，从没见过）。
    pub registered: bool,
    pub is_symlink: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_id: Option<i64>,
    /// **父**目录(即被列目录本身)的库行 id;父是 FS-only 时缺席。仅目录条目下发。
    ///
    /// 前端拖拽/移动/复制链全走 DB id(源目录的 `parentId` 还是移动 IPC 的入参)——缺它则
    /// FS 模式连**库内**目录也被拖拽入口第一关(`parentId === null` 挡扫描根)静默禁拖,
    /// 比设计 §4.1「只禁 FS-only」更紧且无处声明(审查 R-07)。FS-only 目录自身仍被
    /// `hasEntityIdentity`(directoryId 缺席)拦在动作面外,本字段不为它们开门。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_directory_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

/// 一页树节点。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreePage {
    pub entries: Vec<TreeEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<usize>,
    /// 本目录直接子项总数。**不是**递归媒体数 —— 「所有文件」模式有意不显示媒体角标，
    /// 那会把媒体数冒充文件数（S 线 §4.2）。
    pub total: usize,
}

/// 列出扫描根内某目录的直接子项（「所有文件」两态；分页）。
///
/// ## 信任模型
///
/// `root_id` 查 DB 得可信根路径；`rel_path` **来自 WebView，不可信** → 必经
/// [`resolve_within_root`]（`..` 逐段拒 / canonicalize / `starts_with` 边界断言）。
///
/// ## 卷不可用
///
/// 移动盘拔出时 FS 枚举失败 → 返回结构化错误，**绝不回落 DB 模式**：那会让「所有文件」
/// 静默变成「已注册格式」，用户以为文件没了，比报错更坏（S 线 §4.2）。
///
/// ## 稳定分页
///
/// 首页对目录做一次快照存入 [`DirSnapshotCache`](crate::tree::cache::DirSnapshotCache)，
/// 后续页按下标切片。既避免 10 万项目录每页重跑 `read_dir + sort`（平方级），也避免翻页
/// 途中目录增删导致的重复/漏项。
#[tauri::command]
pub async fn list_tree_entries(
    root_id: i64,
    rel_path: String,
    mode: TreeDisplayMode,
    kind: Option<TreeKindFilter>,
    categories: Option<Vec<TreeMediaCategory>>,
    cursor: Option<usize>,
    state: State<'_, Arc<AppState>>,
) -> Result<TreePage> {
    // span 埋点(W1,D-312 debug 档:视口滚动热路径查询(树展开/滚动),默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:list_tree_entries");
    let include_hidden = match mode {
        TreeDisplayMode::AllFiles => false,
        TreeDisplayMode::AllFilesWithHidden => true,
        // 调用方 bug：该模式走现有 DB 目录树命令，不该来这里。
        TreeDisplayMode::RegisteredOnly => {
            return Err(AppError::PathResolution(
                "invalid_mode | 「已注册格式」模式请走 DB 目录树命令".into(),
            ))
        }
    };

    let root = read_blocking(&state, {
        let _ = &rel_path;
        move |c| get_scan_root(c, root_id)
    })
    .await?;

    // ── 1) FS 侧：解析边界 → 取快照（命中缓存则复用）→ 切页 ──────────────────
    let cache = Arc::clone(&state.tree_snapshots);
    // 分类只作用于文件视图，且必须在 `slice_page` 前完成；快照本身仍只由磁盘事实构成，
    // 不把 UI 筛选加入缓存键。Arc 同时供页前过滤与本页的 registered 标记使用，确保两处
    // 走同一份 common-first 内置/Catalog 分类快照。
    let catalog = state.exotic_catalog.snapshot();
    let (page, rel_for_db) = {
        let rel_path = rel_path.clone();
        let root_path = root.path.clone();
        let catalog = Arc::clone(&catalog);
        tokio::task::spawn_blocking(move || -> Result<_> {
            let dir = resolve_within_root(&root_path, &rel_path)?;
            let key = SnapshotKey {
                root_id,
                rel_path: rel_path.clone(),
                include_hidden,
            };
            let snap = match cache.get(&key) {
                Some(s) => s,
                None => {
                    let s = Arc::new(list_fs_entries(&dir, include_hidden)?);
                    cache.put(key, Arc::clone(&s));
                    s
                }
            };
            let kind_view = apply_kind_filter(&snap, kind);
            let view = apply_media_category_filter(&kind_view, categories.as_deref(), &catalog);
            Ok((slice_page(&view, cursor.unwrap_or(0), PAGE_SIZE), rel_path))
        })
        .await
        .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??
    };

    // ── 2) DB 侧：给本页条目关联实体身份（有则赋，无则 None，绝不伪造）────────
    let file_names: Vec<String> = page
        .entries
        .iter()
        .filter(|e| e.kind == TreeEntryKind::File)
        .map(|e| e.name.clone())
        .collect();

    let (listed_dir_id, child_dirs, media) = read_blocking(&state, move |c| {
        let dir_id = find_directory_id(c, root_id, &rel_for_db)?;
        match dir_id {
            // FS-only 目录（磁盘有、库里没有）：其子项自然也没有实体身份。
            None => Ok((None, Default::default(), Default::default())),
            Some(id) => Ok((
                Some(id),
                map_child_directory_ids(c, id)?,
                map_media_entities(c, id, &file_names)?,
            )),
        }
    })
    .await?;

    let parent_key = node_key(root_id, &rel_path);
    let entries = page
        .entries
        .into_iter()
        .map(|e| {
            let child_rel = child_rel_path(&rel_path, &e.name);
            let registered = is_registered(&e, &catalog);
            let (media_id, media_type) = match media.get(&e.name) {
                Some((id, mt)) => (Some(*id), Some(mt.clone())),
                None => (None, None),
            };
            TreeEntry {
                node_key: node_key(root_id, &child_rel),
                parent_key: parent_key.clone(),
                root_id,
                rel_path: child_rel,
                kind: match e.kind {
                    TreeEntryKind::Dir => "dir".into(),
                    TreeEntryKind::File => "file".into(),
                },
                registered,
                directory_id: match e.kind {
                    TreeEntryKind::Dir => child_dirs.get(&e.name).copied(),
                    TreeEntryKind::File => None,
                },
                // 本页所有子目录的**父**就是被列目录自身,其库行 id 已在上面查过一次
                // (find_directory_id)——零新增查询。文件条目不发:DirFile 没有 parentId 轴。
                parent_directory_id: match e.kind {
                    TreeEntryKind::Dir => listed_dir_id,
                    TreeEntryKind::File => None,
                },
                media_id,
                media_type,
                name: e.name,
                hidden: e.hidden,
                is_symlink: e.is_symlink,
            }
        })
        .collect();

    Ok(TreePage {
        entries,
        next_cursor: page.next_cursor,
        total: page.total,
    })
}

/// 丢弃文件树的目录快照缓存 —— 前端「刷新」动作调用（S 线 §4.2 的第三个失效触发点；
/// 另两个是切模式〔`include_hidden` 进键，天然分离〕与扫描完成〔`scan_commands` 内接线〕）。
///
/// `root_id = None` = 全清。
///
/// 为什么需要显式刷新：快照的真相源是**磁盘**，而磁盘可以被本应用之外的任何东西改动
/// （用户在资源管理器里拖了个文件进来）。没有文件系统监听时，用户主动刷新是唯一的收敛手段。
#[tauri::command]
pub async fn invalidate_tree_cache(
    root_id: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    match root_id {
        Some(id) => state.tree_snapshots.invalidate_root(id),
        None => state.tree_snapshots.clear(),
    }
    Ok(())
}

/// 按 kind 过滤快照视图。
///
/// 在**快照之后**过滤：缓存键 `(root_id, rel_path, include_hidden)` 不含 kind，故「先拉目录
/// 再拉文件」共用同一次磁盘枚举。
///
/// 🔴 过滤**必须保序**（`filter` 而非重排/重新 `sort`）：D-002 要的是「共有项相对序不变」，
/// 而快照序是唯一事实源（`cmp_entries`）。在这里重排一次，就等于给同一棵树造了第二种序。
///
/// 抽成自由函数是为了可测 —— 内联在命令体里就得造 `State<AppState>` 才能跑。
fn apply_kind_filter(snap: &[FsEntry], kind: Option<TreeKindFilter>) -> Vec<FsEntry> {
    let want = match kind {
        None => return snap.to_vec(),
        Some(TreeKindFilter::Dirs) => TreeEntryKind::Dir,
        Some(TreeKindFilter::Files) => TreeEntryKind::File,
    };
    snap.iter().filter(|e| e.kind == want).cloned().collect()
}

/// 按文件树五类筛选快照视图。
///
/// 目录始终保留，保证筛选后树骨架仍可导航；只有文件根据 common-first 内置/Catalog
/// 分类器判定。该函数必须在 [`slice_page`] 前调用，否则前端会看到空页、漏项及错误的
/// `total`/`next_cursor`。`None` 和五类全选都表示不限制，空切片则只保留目录。
fn apply_media_category_filter(
    snap: &[FsEntry],
    selected: Option<&[TreeMediaCategory]>,
    catalog: &crate::exotic::catalog::CatalogSnapshot,
) -> Vec<FsEntry> {
    let Some(selected) = selected else {
        return snap.to_vec();
    };
    if TreeMediaCategory::ALL
        .iter()
        .all(|category| selected.contains(category))
    {
        return snap.to_vec();
    }

    snap.iter()
        .filter(|entry| {
            if entry.kind == TreeEntryKind::Dir {
                return true;
            }
            selected.contains(&file_category(&entry.name, catalog))
        })
        .cloned()
        .collect()
}

/// 以扫描器相同的 common-first 规则把磁盘文件投影到文件树五类；无扩展名/未知扩展名
/// 归入 `other`。目录不会调用此函数。
fn file_category(
    name: &str,
    catalog: &crate::exotic::catalog::CatalogSnapshot,
) -> TreeMediaCategory {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_lowercase())
        .unwrap_or_default();
    crate::scanner::walker::classify_scanned_file(&ext, catalog)
        .map(TreeMediaCategory::from_media_type)
        .unwrap_or(TreeMediaCategory::Other)
}

/// 条目的格式是否「已注册」（内置表 ∪ exotic Catalog）。
///
/// 目录恒 `false`（「已注册」是格式属性，目录没有格式）。走 `classify_scanned_file` 而非
/// 只查内置表 —— 那是扫描期真正用的 common-first 分类器，UI 说「已注册」而扫描器不收，
/// 是最坏的分歧。
fn is_registered(e: &FsEntry, catalog: &crate::exotic::catalog::CatalogSnapshot) -> bool {
    if e.kind == TreeEntryKind::Dir {
        return false;
    }
    let ext = std::path::Path::new(&e.name)
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_lowercase())
        .unwrap_or_default();
    crate::scanner::walker::classify_scanned_file(&ext, catalog).is_some()
}

/// 在系统文件管理器中**显示**（reveal）扫描根内的一个条目。
///
/// ## 为什么是 reveal 而不是「用默认应用打开」（D-001，🔴 安全）
///
/// 「所有文件」模式让扫描根内的**任意**文件出现在树里，包括 `.exe` / `.lnk` / `.bat` /
/// `.ps1`。用系统默认应用打开它们 = 双击即执行。而 `canonicalize` + 根边界**挡不住**这个 ——
/// 那些文件本来就在根内，边界检查会放行；威胁不是「路径越界」而是「根内文件本身可执行」。
///
/// 扩展名黑名单不是答案：`PATHEXT` 可被用户改、`.lnk` 可指向任意目标、双扩展名
/// （`x.jpg.exe`）、NTFS ADS 都能绕。黑名单在安全上默认是输的一方，且维护成本只涨。
///
/// reveal 把最后一步交给系统文件管理器的信任链 —— 用户在那里点开是他自己的决定，不是我们
/// 代做的。功能损失只有一次点击，而且日后要放宽随时可以，反向收窄则是行为回归。
///
/// ## 为什么不授 `opener:allow-reveal-item-in-dir`
///
/// 那个 capability **无预配置 scope**，授给 WebView 等于让前端 reveal 任意路径，绕过本函数
/// 的 `rootId + rel_path` 校验 —— 我们自己定义的扫描根边界会被从旁路掉（F-009）。
/// `tauri_plugin_opener::reveal_item_in_dir` 是 crate 根导出的**自由函数**（不需 `AppHandle`），
/// 故插件**完全不注册**、capability 一条不授，JS 侧够不着，ACL 面为零。
///
/// ## 移动端
///
/// 该函数在 Android/iOS 返回 `UnsupportedPlatform`，[`reveal_path`] 将其映射为稳定 IPC code
/// `unsupported_platform`（`AppError::Reveal`，R-08）。**不得**降级为 `openPath`
/// —— 那正好把上面挡掉的执行面放回来。UI 侧应隐藏/禁用该动作，本错误只是后端兜底。
#[tauri::command]
pub async fn reveal_tree_entry(
    root_id: i64,
    rel_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    // 扫描根取自 DB（可信），rel_path 来自 WebView（不可信）—— 二者的信任级别不同，
    // 这正是必须走 resolve_within_root 而非 resolve_media_path 的原因。
    let root = read_blocking(&state, move |c| get_scan_root(c, root_id)).await?;

    let target = {
        let rel_path = rel_path.clone();
        tokio::task::spawn_blocking(move || resolve_within_root(&root.path, &rel_path))
            .await
            .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))??
    };
    info!(
        "reveal_tree_entry: root={} rel={} | 在文件管理器中显示",
        root_id, rel_path
    );
    reveal_path(target).await
}

/// 预览内容上限（字节）。弹层预览不是编辑器，1 MiB 纯文本已远超可读范围；上限同时挡住
/// 「伪装 .txt 的大文件」把整块内容读进内存与 IPC 载荷。
const TEXT_PREVIEW_MAX_BYTES: u64 = 1024 * 1024;

/// 纯文本预览白名单（问题②方案 B v1，D-002）。**后端是安全边界**——前端
/// `isTextPreviewable` 只是 UX 预筛。只收零解码器攻击面的纯文本；epub/图片等
/// 解码器面留给后续显式扩展，不得顺手放宽。
const TEXT_PREVIEW_EXTS: [&str; 3] = ["txt", "md", "markdown"];

/// 树内纯文本预览载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeTextPreview {
    pub content: String,
    /// 超过 [`TEXT_PREVIEW_MAX_BYTES`] 被截断。前端据此显示「仅展示前 1 MiB」提示。
    pub truncated: bool,
}

/// 应用内**只读**预览扫描根内的一个纯文本文件（问题②方案 B v1）。
///
/// ## 与 D-001（reveal 而非默认应用打开）的关系
///
/// D-001 禁的是「交给系统默认应用**执行**」；本命令只读字节，前端渲染进 `<pre>` 文本节点
/// —— 无执行面、无 HTML 注入面、markdown 不渲染。白名单外（含 `.exe` 等）一律
/// `preview_unsupported_type`；这是白名单而非黑名单，双扩展名/ADS 那套绕法不适用。
///
/// ## 输入信任
///
/// `rel_path` 来自 WebView（不可信），过 [`resolve_within_root`]（逐段拒 `..` +
/// canonicalize + 根边界）；白名单判定取 canonicalize 后**真实路径**的扩展名，不看前端
/// 给的任何文件名。错误 message **不携带绝对路径**（[`AppError::Preview`] 契约）。
#[tauri::command]
pub async fn get_tree_text_preview(
    root_id: i64,
    rel_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<TreeTextPreview> {
    // span 埋点(W1,D-312 debug 档:树浏览悬停预览查询,默认 info 档不刷屏)。
    let _span = crate::logging::SpanTimer::debug("ipc:get_tree_text_preview");
    let root = read_blocking(&state, move |c| get_scan_root(c, root_id)).await?;
    tokio::task::spawn_blocking(move || {
        let target = resolve_within_root(&root.path, &rel_path)?;
        read_text_preview(&target)
    })
    .await
    .map_err(|e| AppError::internal("内部任务失败 | internal task failed", e))?
}

/// 纯核：白名单 + 限量读 + lossy 转码。拆出便于单测（不需要 AppState/DB）。
fn read_text_preview(target: &std::path::Path) -> Result<TreeTextPreview> {
    let ext = target
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_lowercase())
        .unwrap_or_default();
    if !TEXT_PREVIEW_EXTS.contains(&ext.as_str()) {
        return Err(AppError::Preview {
            code: "preview_unsupported_type",
            message: format!("扩展名不在预览白名单: .{ext}"),
        });
    }
    if !target.is_file() {
        return Err(AppError::Preview {
            code: "preview_failed",
            message: "目标不是常规文件".into(),
        });
    }
    // 错误只透 io::ErrorKind（"entity not found" 之类），不透 io::Error 全文——后者可能被
    // 上游塞进路径。多读 1 字节探测截断，免去 metadata 的 TOCTOU 不一致。
    use std::io::Read;
    let file = std::fs::File::open(target).map_err(|e| AppError::Preview {
        code: "preview_failed",
        message: e.kind().to_string(),
    })?;
    let mut buf = Vec::new();
    let n = file
        .take(TEXT_PREVIEW_MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| AppError::Preview {
            code: "preview_failed",
            message: e.kind().to_string(),
        })?;
    let truncated = (n as u64) > TEXT_PREVIEW_MAX_BYTES;
    if truncated {
        // 截口可能落在多字节字符中间——from_utf8_lossy 把残码降级为替换字符，预览可接受。
        buf.truncate(TEXT_PREVIEW_MAX_BYTES as usize);
    }
    Ok(TreeTextPreview {
        content: String::from_utf8_lossy(&buf).into_owned(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::cmp_entries;

    fn e(name: &str, kind: TreeEntryKind) -> FsEntry {
        FsEntry {
            name: name.into(),
            kind,
            hidden: false,
            is_symlink: false,
        }
    }

    /// 一份**已按 `cmp_entries` 排好**的快照(即 `list_fs_entries` 的真实产出形态):
    /// 目录在前(BINARY:大写先)、文件在后(NOCASE:不分大小写)。
    ///
    /// 名字的选取是**有区分度的**,不是随手取的:文件用 `Zebra.jpg`/`apple.jpg` —— 这一对在
    /// BINARY 下是 `Zebra` 先(`Z`=0x5A < `a`=0x61)、在 NOCASE 下是 `apple` 先,**两种序相反**。
    /// 故任何「过滤后顺手按 name 重排一次」的实现都会在文件子集上翻序,被
    /// `filtering_preserves_snapshot_order` 抓住。
    ///
    /// ⚠ 反面教材(2026-07-16 首版即此):原用 `A.png`/`b.jpg`/`c.txt` 并在注释里断言「两种序
    /// 至少有一个会翻」—— 那句话是**假的**,大写字母本就排在前,两种折叠给出**相同**结果,
    /// 对重排变异零区分度。变异验证当场抓出。判据要靠「哪个输入会让两种实现分叉」倒推,
    /// 不能靠「样本看起来典型」。
    fn snapshot() -> Vec<FsEntry> {
        let mut v = vec![
            e("Zebra", TreeEntryKind::Dir),
            e("apple", TreeEntryKind::Dir),
            e("Zebra.jpg", TreeEntryKind::File),
            e("apple.jpg", TreeEntryKind::File),
            e("m.txt", TreeEntryKind::File),
        ];
        v.sort_by(cmp_entries);
        v
    }

    fn names(v: &[FsEntry]) -> Vec<String> {
        v.iter().map(|x| x.name.clone()).collect()
    }

    #[test]
    fn no_filter_returns_everything_unchanged() {
        let snap = snapshot();
        assert_eq!(names(&apply_kind_filter(&snap, None)), names(&snap));
    }

    #[test]
    fn dirs_filter_keeps_only_dirs() {
        let got = apply_kind_filter(&snapshot(), Some(TreeKindFilter::Dirs));
        assert_eq!(names(&got), ["Zebra", "apple"]);
    }

    #[test]
    fn files_filter_keeps_only_files() {
        let got = apply_kind_filter(&snapshot(), Some(TreeKindFilter::Files));
        // NOCASE:apple.jpg < m.txt < Zebra.jpg(BINARY 会把 Zebra.jpg 排头 —— 两种序相反)。
        assert_eq!(names(&got), ["apple.jpg", "m.txt", "Zebra.jpg"]);
    }

    /// 过滤**必须保序**(样本区分度见 `snapshot()`:文件子集 `Zebra.jpg`/`apple.jpg` 在 BINARY
    /// 与 NOCASE 下序相反,故任何「顺手重排一次」的实现都会在这里翻车)。
    ///
    /// 断言钉的是「过滤后的序 == 原快照中同类项的出现序」,而非手写期望 —— 手写期望是第二份
    /// 实现,会带第二份 bug(F-016 ①)。
    #[test]
    fn filtering_preserves_snapshot_order() {
        let snap = snapshot();
        for (kind, want_kind) in [
            (TreeKindFilter::Dirs, TreeEntryKind::Dir),
            (TreeKindFilter::Files, TreeEntryKind::File),
        ] {
            let got = names(&apply_kind_filter(&snap, Some(kind)));
            let expected: Vec<String> = snap
                .iter()
                .filter(|x| x.kind == want_kind)
                .map(|x| x.name.clone())
                .collect();
            assert_eq!(got, expected, "过滤重排了 {kind:?},D-002 相对序被破坏");
        }
    }

    /// 空快照/无匹配项不得 panic,返回空页而非错误(空目录是常态,不是异常)。
    #[test]
    fn empty_and_no_match_yield_empty_view() {
        assert!(apply_kind_filter(&[], Some(TreeKindFilter::Dirs)).is_empty());
        let only_files = vec![e("a.jpg", TreeEntryKind::File)];
        assert!(apply_kind_filter(&only_files, Some(TreeKindFilter::Dirs)).is_empty());
    }

    #[test]
    fn media_categories_use_builtin_catalog_and_other_without_extending_media_type() {
        let catalog = crate::exotic::catalog::CatalogSnapshot::builtin().unwrap();
        // jpg 来自内置表，psd 来自 Catalog；两者都应投影到 image。未知/无扩展名才是 other。
        assert_eq!(
            crate::scanner::walker::classify_scanned_file("jpg", &catalog),
            Some(crate::utils::format::MediaType::Image)
        );
        assert_eq!(
            crate::scanner::walker::classify_scanned_file("psd", &catalog),
            Some(crate::utils::format::MediaType::Image)
        );
        assert_eq!(
            file_category("photo.JPG", &catalog),
            TreeMediaCategory::Image
        );
        assert_eq!(file_category("raw.psd", &catalog), TreeMediaCategory::Image);
        assert_eq!(file_category("README", &catalog), TreeMediaCategory::Other);
        assert_eq!(
            file_category("unknown.xyz", &catalog),
            TreeMediaCategory::Other
        );

        let snap = vec![
            e("folder", TreeEntryKind::Dir),
            e("photo.JPG", TreeEntryKind::File),
            e("raw.psd", TreeEntryKind::File),
            e("unknown.xyz", TreeEntryKind::File),
            e("README", TreeEntryKind::File),
            e("clip.mp4", TreeEntryKind::File),
        ];
        let image = apply_media_category_filter(&snap, Some(&[TreeMediaCategory::Image]), &catalog);
        assert_eq!(names(&image), ["folder", "photo.JPG", "raw.psd"]);
        let other = apply_media_category_filter(&snap, Some(&[TreeMediaCategory::Other]), &catalog);
        assert_eq!(names(&other), ["folder", "unknown.xyz", "README"]);
        let audio_or_video = apply_media_category_filter(
            &snap,
            Some(&[TreeMediaCategory::Audio, TreeMediaCategory::Video]),
            &catalog,
        );
        assert_eq!(names(&audio_or_video), ["folder", "clip.mp4"]);
    }

    #[test]
    fn empty_categories_keep_directory_skeleton_and_all_categories_are_noop() {
        let catalog = crate::exotic::catalog::CatalogSnapshot::builtin().unwrap();
        let snap = vec![
            e("folder", TreeEntryKind::Dir),
            e("photo.jpg", TreeEntryKind::File),
            e("unknown.xyz", TreeEntryKind::File),
        ];
        let none = apply_media_category_filter(&snap, Some(&[]), &catalog);
        assert_eq!(names(&none), ["folder"]);
        let all = apply_media_category_filter(&snap, Some(&TreeMediaCategory::ALL), &catalog);
        assert_eq!(names(&all), names(&snap));
        let omitted = apply_media_category_filter(&snap, None, &catalog);
        assert_eq!(names(&omitted), names(&snap));
    }

    #[test]
    fn media_category_filter_precedes_pagination_and_reports_filtered_cursor_total() {
        let catalog = crate::exotic::catalog::CatalogSnapshot::builtin().unwrap();
        let snap = vec![
            e("a.jpg", TreeEntryKind::File),
            e("b.unknown", TreeEntryKind::File),
            e("c.mp4", TreeEntryKind::File),
            e("d.png", TreeEntryKind::File),
        ];
        let files = apply_kind_filter(&snap, Some(TreeKindFilter::Files));
        let filtered = apply_media_category_filter(
            &files,
            Some(&[TreeMediaCategory::Image, TreeMediaCategory::Other]),
            &catalog,
        );
        assert_eq!(names(&filtered), ["a.jpg", "b.unknown", "d.png"]);

        let first = crate::tree::cache::slice_page(&filtered, 0, 2);
        assert_eq!(names(&first.entries), ["a.jpg", "b.unknown"]);
        assert_eq!(first.total, 3);
        assert_eq!(first.next_cursor, Some(2));
        let second = crate::tree::cache::slice_page(&filtered, first.next_cursor.unwrap(), 2);
        assert_eq!(names(&second.entries), ["d.png"]);
        assert_eq!(second.total, 3);
        assert_eq!(second.next_cursor, None);
    }

    // ── read_text_preview 纯核(问题②方案 B v1)────────────────────────────────

    /// 白名单命中:txt 可读,内容原样、不截断。
    #[test]
    fn text_preview_reads_whitelisted_txt() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("note.txt");
        std::fs::write(&p, "hello 预览").unwrap();
        let got = read_text_preview(&p).unwrap();
        assert_eq!(got.content, "hello 预览");
        assert!(!got.truncated);
    }

    /// 扩展名大小写不敏感(真实文件名 `.MD` 也该命中白名单)。
    #[test]
    fn text_preview_ext_check_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("README.MD");
        std::fs::write(&p, "# t").unwrap();
        assert!(read_text_preview(&p).is_ok());
    }

    /// 白名单外(exe / 无扩展名)必须拒,且 IPC code 是稳定的
    /// `preview_unsupported_type` —— 这是 D-001 的边界:预览面**只**对纯文本开放。
    #[test]
    fn text_preview_rejects_non_whitelisted_with_stable_code() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["evil.exe", "noext"] {
            let p = dir.path().join(name);
            std::fs::write(&p, b"MZ").unwrap();
            let err = read_text_preview(&p).unwrap_err();
            let v = serde_json::to_value(&err).unwrap();
            assert_eq!(
                v["code"], "preview_unsupported_type",
                "{name} 应被白名单拒绝"
            );
        }
    }

    /// 超限截断:置 truncated 标,内容长度恰为上限(截口残码由 lossy 降级,不 panic)。
    #[test]
    fn text_preview_truncates_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.txt");
        std::fs::write(&p, vec![b'a'; (TEXT_PREVIEW_MAX_BYTES + 7) as usize]).unwrap();
        let got = read_text_preview(&p).unwrap();
        assert!(got.truncated);
        assert_eq!(got.content.len(), TEXT_PREVIEW_MAX_BYTES as usize);
    }

    /// 伪装 .txt 的二进制:lossy 转码安全降级为替换字符,不得报错或 panic。
    #[test]
    fn text_preview_binary_masquerading_as_txt_degrades_safely() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fake.txt");
        std::fs::write(&p, [0xff, 0xfe, 0x00, 0x41]).unwrap();
        let got = read_text_preview(&p).unwrap();
        assert!(got.content.contains('\u{FFFD}'), "非法字节应降级为替换字符");
    }

    /// 目录伪装白名单扩展名(`x.txt/`)须拒为 preview_failed,不得读出目录项。
    #[test]
    fn text_preview_rejects_directory_target() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.txt");
        std::fs::create_dir(&p).unwrap();
        let err = read_text_preview(&p).unwrap_err();
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "preview_failed");
    }
}
