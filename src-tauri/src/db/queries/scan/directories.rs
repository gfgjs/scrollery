//! 目录(directories)域:树/CRUD/祖先链/子树/移动辅助。
//! (D 线从 `scan.rs` 拆出,SQL/事务边界不变。)

use rusqlite::{params, Connection, OptionalExtension, Row, ToSql};

use crate::db::models::{DirFile, DirNode, Directory};
use crate::error::{AppError, Result};
use crate::tree::TreeMediaCategory;

fn map_dir_node(row: &Row<'_>) -> rusqlite::Result<DirNode> {
    // 路径身份就地派生(S 线 D-013):键的**唯一实现**在 crate::tree,前端不推导键——
    // 否则 DB 模式(前端拼)与 FS 模式(后端拼)就是两份实现,分歧后果是切模式时同一目录
    // 被当成两个节点,且不报任何错。
    let root_id: i64 = row.get(1)?;
    let rel_path: String = row.get(4)?;
    Ok(DirNode {
        node_key: crate::tree::node_key(root_id, &rel_path),
        parent_key: crate::tree::parent_key(root_id, &rel_path),
        id: row.get(0)?,
        root_id,
        parent_id: row.get(2)?,
        name: row.get(3)?,
        rel_path,
        depth: row.get(5)?,
        media_count: row.get(6)?,
        has_children: row.get::<_, i64>(7)? != 0,
    })
}

// ── 目录 ──────────────────────────────────────────────────────────────

/// 插入或更新目录。返回行 ID。
pub fn upsert_directory(
    conn: &Connection,
    root_id: i64,
    parent_id: Option<i64>,
    rel_path: &str,
    name: &str,
    depth: i64,
    mtime: Option<i64>,
) -> Result<i64> {
    // tree_sort_key(方案 B):Rust 侧算键绑 blob —— 自足(不依赖连接注册 TREE_SORT_KEY,裸连接
    // 测试可用),与 move_directory 现有「先在 Rust 算 new_rel」风格一致。生产目录创建全部经此
    // funnel(根目录 scan_commands / 扫描子目录 fast_scan 都调 upsert_directory)。
    // ON CONFLICT 分支**无需写键**:冲突键 = (root_id, rel_path),命中即 rel_path 与传入值相等
    // → 键稳定不变;仅 INSERT 首次写键。
    let tree_key = crate::utils::path::encode_tree_sort_key(rel_path);
    // 扫描热路径:每目录一次 upsert + id 回查;prepare_cached 复用已编译语句。
    conn.prepare_cached(
        "INSERT INTO directories (root_id, parent_id, rel_path, name, depth, mtime, tree_sort_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(root_id, rel_path) DO UPDATE SET
             name=excluded.name, mtime=excluded.mtime",
    )?
    .execute(params![
        root_id, parent_id, rel_path, name, depth, mtime, tree_key
    ])?;
    // 插入或更新后，获取 id（可能之前就已存在）
    let id: i64 = conn
        .prepare_cached("SELECT id FROM directories WHERE root_id=?1 AND rel_path=?2")?
        .query_row(params![root_id, rel_path], |row| row.get(0))?;
    Ok(id)
}

/// J1 修复（方案 A 快照）：一次性加载本根全部目录的 `rel_path → mtime` 快照，
/// 供快速扫描剪枝判定只读快照、不再读活 DB 行——祖先链 `ensure_dir_chain` 在扫描
/// 过程中会持续覆写 `directories.mtime`（每段目录写当前 FS mtime），若剪枝判定
/// 直接查活行，会读到「本次扫描已改写」的新值而非「上一轮扫描终态」，导致父目录
/// 新增直接文件被误判为「未变」而漏扫（且基线被错误治愈、后续轮次永续漏，见 J1）。
/// 快照在扫描启动时一次性取（此时 DB 尚未被本轮扫描改写），语义即「上一轮扫描终态」；
/// mtime 为 NULL 的目录不入快照（等同「无基线」，剪枝判定按保守不剪枝处理）。
/// J1 fix (Plan A snapshot): load the whole root's `rel_path → mtime` snapshot
/// once up front so pruning decisions only ever read last-run state, never the
/// live row that the current scan's ancestor-chain upserts keep overwriting.
pub fn load_directory_mtime_snapshot(
    conn: &Connection,
    root_id: i64,
) -> Result<std::collections::HashMap<String, i64>> {
    let mut stmt = conn.prepare(
        "SELECT rel_path, mtime FROM directories WHERE root_id=?1 AND mtime IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![root_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (rel_path, mtime) = row?;
        map.insert(rel_path, mtime);
    }
    Ok(map)
}

/// 分类筛选在目录查询中的四种有效形态。
///
/// `None` 和五类全选都是 `Unfiltered`；空集合是文件树的「仅目录」语义，保留完整目录
/// 骨架但不计媒体；只有 `Other`（或其它不含四类 DB 类型的非空选择）没有可匹配的 DB
/// 媒体，直接返回空结果。`Other` 不会被伪造成 `media_items.media_type`。
#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectoryCategoryFilter {
    Unfiltered,
    DirectoriesOnly,
    MediaTypes(Vec<&'static str>),
    NoMatch,
}

fn directory_category_filter(
    media_categories: Option<&[TreeMediaCategory]>,
) -> DirectoryCategoryFilter {
    let Some(selected) = media_categories else {
        return DirectoryCategoryFilter::Unfiltered;
    };
    if selected.is_empty() {
        return DirectoryCategoryFilter::DirectoriesOnly;
    }
    if TreeMediaCategory::ALL
        .iter()
        .all(|category| selected.contains(category))
    {
        return DirectoryCategoryFilter::Unfiltered;
    }

    // Use the stable enum order rather than caller order, and project only the four real DB
    // media types. This also deduplicates a malformed repeated category list.
    let types = TreeMediaCategory::ALL
        .iter()
        .filter(|category| selected.contains(category))
        .filter_map(|category| category.db_media_type())
        .collect::<Vec<_>>();
    if types.is_empty() {
        DirectoryCategoryFilter::NoMatch
    } else {
        DirectoryCategoryFilter::MediaTypes(types)
    }
}

/// 查询一个目录层级的直接子目录，并按需要按匹配媒体的后代裁剪。
///
/// 递归 `sub` 一次物化每个直接候选目录的子树，`agg` 同时提供媒体角标和「该候选是否
/// 有匹配后代」判据。筛选态下 `has_children` 只对仍可显示的子目录为真；不限/仅目录态
/// 则保持历史语义（任意子目录即为真）。所有分类值均作为绑定参数传入 SQL。
fn query_directory_level(
    conn: &Connection,
    category_filter: &DirectoryCategoryFilter,
    seed_clause: &str,
    outer_clause: &str,
    scope_id: i64,
) -> Result<Vec<DirNode>> {
    let (media_predicate, matching_ancestors, candidate_predicate, has_children_expr) =
        match category_filter {
            DirectoryCategoryFilter::Unfiltered => (
                String::new(),
                String::new(),
                String::new(),
                "EXISTS(SELECT 1 FROM directories c WHERE c.parent_id = d.id)".to_string(),
            ),
            DirectoryCategoryFilter::DirectoriesOnly => (
                " AND 0".to_string(),
                String::new(),
                String::new(),
                "EXISTS(SELECT 1 FROM directories c WHERE c.parent_id = d.id)".to_string(),
            ),
            DirectoryCategoryFilter::MediaTypes(types) => {
                let placeholders = (2..=types.len() + 1)
                    .map(|index| format!("?{index}"))
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    format!(" AND m.media_type IN ({placeholders})"),
                    ",
                matched(id) AS (
                    SELECT id FROM matching
                    UNION
                    SELECT d.parent_id
                    FROM directories d
                    JOIN matched parent_match ON parent_match.id = d.id
                    WHERE d.parent_id IS NOT NULL
                )"
                    .to_string(),
                    " AND EXISTS(SELECT 1 FROM agg matching WHERE matching.top = d.id)".to_string(),
                    "EXISTS(
                    SELECT 1
                    FROM directories c
                    JOIN matched matching_child ON matching_child.id = c.id
                    WHERE c.parent_id = d.id
                )"
                    .to_string(),
                )
            }
            // There is no SQL type for `Other`; the caller must not see DB rows for this selection.
            DirectoryCategoryFilter::NoMatch => return Ok(Vec::new()),
        };

    // `seed_clause` and `outer_clause` are fixed internal fragments selected by the two public
    // query functions; user data only enters through `?1` and the media-type placeholders.
    let sql = format!(
        "WITH RECURSIVE sub(id, top) AS (
             SELECT id, id FROM directories WHERE {seed_clause}
             UNION ALL
             SELECT c.id, s.top FROM directories c JOIN sub s ON c.parent_id = s.id
         ),
         matching(id, top) AS (
             SELECT s.id, s.top
             FROM sub s JOIN media_items m ON m.directory_id = s.id
             WHERE m.is_deleted = 0 AND m.companion_of IS NULL{media_predicate}
         ),
         agg(top, cnt) AS (
             SELECT matching.top, COUNT(*)
             FROM matching
             GROUP BY matching.top
         )
         {matching_ancestors}
         SELECT d.id, d.root_id, d.parent_id, d.name, d.rel_path, d.depth,
                COALESCE(a.cnt, 0) AS media_count,
                {has_children_expr} AS has_children
         FROM directories d
         LEFT JOIN agg a ON a.top = d.id
         WHERE {outer_clause}{candidate_predicate}
         ORDER BY d.name ASC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut binds: Vec<&dyn ToSql> = Vec::with_capacity(
        1 + match category_filter {
            DirectoryCategoryFilter::MediaTypes(types) => types.len(),
            _ => 0,
        },
    );
    binds.push(&scope_id);
    if let DirectoryCategoryFilter::MediaTypes(types) = category_filter {
        for media_type in types {
            binds.push(media_type);
        }
    }
    let rows = stmt.query_map(binds.as_slice(), map_dir_node)?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 获取扫描根目录的直接子目录；筛选态只保留含匹配未删除、非 companion 媒体的子树。
pub fn get_directory_tree(
    conn: &Connection,
    root_id: i64,
    media_categories: Option<&[TreeMediaCategory]>,
) -> Result<Vec<DirNode>> {
    let category_filter = directory_category_filter(media_categories);
    query_directory_level(
        conn,
        &category_filter,
        "root_id = ?1 AND parent_id IS NULL",
        "d.root_id = ?1 AND d.parent_id IS NULL",
        root_id,
    )
}

/// 获取目录的直接子目录；筛选态只保留含匹配未删除、非 companion 媒体的子树。
pub fn get_directory_children(
    conn: &Connection,
    parent_id: i64,
    media_categories: Option<&[TreeMediaCategory]>,
) -> Result<Vec<DirNode>> {
    let category_filter = directory_category_filter(media_categories);
    query_directory_level(
        conn,
        &category_filter,
        "parent_id = ?1",
        "d.parent_id = ?1",
        parent_id,
    )
}

/// 单个目录（非其子树）的直接媒体文件，供侧边栏树的可展开文件列表使用。子文件夹各自
/// 作为节点出现，故此处仅列出物理位于本目录的文件。排除软删除项与 Live Photo 伴随视频
///（与目录 `media_count` 同口径），使叶子数量与角标一致。按名称不区分大小写排序，呈现
/// 整洁的文件管理器式列表。
///
/// 分页(T2 百万级):`limit`/`offset` 为 `None` 时退化为「全量」(SQLite `LIMIT -1` = 无限制),
/// 保持旧调用语义不变;侧边栏树按页拉取(每页 PAGE + 1 条以探测是否还有更多),避免单个含数万散
/// 文件的目录一次性灌满 `node.files` 与拍平行数组。列序 4 列不变。
pub fn list_directory_files(
    conn: &Connection,
    directory_id: i64,
    limit: Option<i64>,
    offset: Option<i64>,
    media_categories: Option<&[TreeMediaCategory]>,
) -> Result<Vec<DirFile>> {
    let limit = limit.unwrap_or(-1); // SQLite: LIMIT -1 → 不限制(全量)
    let offset = offset.unwrap_or(0);

    // 先取所属目录的 (root_id, rel_path) 以派生文件的路径身份(S 线 D-013)。
    // 有意用独立一次 PK 查找而非 JOIN 进下面那条列表查询:后者带 COLLATE NOCASE 排序 +
    // LIMIT/OFFSET,是分页热路径,不值得为一个每页只需一次的常量去动它的执行计划。
    let (root_id, dir_rel): (i64, String) = conn.query_row(
        "SELECT root_id, rel_path FROM directories WHERE id = ?1",
        params![directory_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let parent_key = crate::tree::node_key(root_id, &dir_rel);

    // `None` = 不限；空切片 = 没有任何文件匹配。五类全选可安全视为不限，避免把
    // `other` 虚构成领域 `media_type`。已知类别值只作为绑定参数进入 SQL，绝不拼接用户输入。
    let selected_types: Option<Vec<&'static str>> = media_categories.map(|selected| {
        if TreeMediaCategory::ALL
            .iter()
            .all(|category| selected.contains(category))
        {
            Vec::new()
        } else {
            selected
                .iter()
                .filter_map(|category| category.db_media_type())
                .collect()
        }
    });

    if selected_types.as_ref().is_some_and(Vec::is_empty)
        && media_categories.is_some_and(|selected| !selected.is_empty())
    {
        // `Some([Other])` 及五类全选都落到空 Vec，前者应为空结果、后者应不限；两者需分开。
        let all_selected = media_categories.is_some_and(|selected| {
            TreeMediaCategory::ALL
                .iter()
                .all(|category| selected.contains(category))
        });
        if !all_selected {
            return Ok(Vec::new());
        }
    }

    let mut sql = String::from(
        "SELECT id, file_name, media_type, is_favorited
         FROM media_items
         WHERE directory_id = ?1 AND is_deleted = 0 AND companion_of IS NULL",
    );
    if let Some(types) = selected_types.as_ref() {
        if types.is_empty() {
            // `Some([])` 是显式「无文件匹配」；前面的 `Some([Other])` 也已在此返回。
            if media_categories.is_some_and(|selected| selected.is_empty()) {
                return Ok(Vec::new());
            }
        } else {
            let placeholders = (4..=types.len() + 3)
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            sql.push_str(" AND media_type IN (");
            sql.push_str(&placeholders);
            sql.push(')');
        }
    }
    sql.push_str(" ORDER BY file_name COLLATE NOCASE ASC LIMIT ?2 OFFSET ?3");

    let mut stmt = conn.prepare(&sql)?;
    let mut binds: Vec<&dyn ToSql> =
        Vec::with_capacity(3 + selected_types.as_ref().map_or(0, Vec::len));
    binds.push(&directory_id);
    binds.push(&limit);
    binds.push(&offset);
    if let Some(types) = selected_types.as_ref() {
        for media_type in types {
            binds.push(media_type);
        }
    }
    let rows = stmt.query_map(binds.as_slice(), |row| {
        let file_name: String = row.get(1)?;
        let rel_path = crate::tree::child_rel_path(&dir_rel, &file_name);
        Ok(DirFile {
            node_key: crate::tree::node_key(root_id, &rel_path),
            parent_key: parent_key.clone(),
            rel_path,
            id: row.get(0)?,
            file_name,
            media_type: row.get(2)?,
            is_favorited: row.get(3)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

// ── 文件树 FS 模式的实体关联（S 线 §4.1 / D-013）─────────────────────────────
//
// 「所有文件」模式的树节点身份是**路径**（nodeKey），实体身份（directoryId/mediaId）只在
// DB 确有对应行时才赋值。下面三个函数就是那个「确有吗」的查询面。

// 注：「按 (root_id, rel_path) 查 directory_id」复用既有的 [`find_directory_id`]（本文件下方，
// 原为复制撤销所建）—— 语义与本用途完全一致：`None` = FS-only 目录（磁盘有、库里没有）。
// 扫描根自身的行是 `rel_path = ""`、`parent_id IS NULL`（见 `scan_commands.rs` 加根处），
// 与 [`crate::utils::path::resolve_within_root`] 对空 `rel_path` 返回根自身的约定一致。

/// 某目录的直接子目录 `name → id` 映射（供 FS 枚举结果关联实体身份）。
///
/// 一个目录的子目录数量级远小于文件数，故一次全取而不分页。
pub fn map_child_directory_ids(
    conn: &Connection,
    parent_id: i64,
) -> Result<std::collections::HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT name, id FROM directories WHERE parent_id = ?1")?;
    let rows = stmt.query_map(params![parent_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = std::collections::HashMap::new();
    for r in rows {
        let (n, i) = r?;
        out.insert(n, i);
    }
    Ok(out)
}

/// 某目录内**指定文件名**的媒体实体：`file_name → (id, media_type)`。
///
/// 只查当前页的文件名（而非整目录）—— 一个十万项的目录，每页都全取媒体行会把分页的意义
/// 抵消掉。
///
/// ⚠️ **谓词与 [`list_directory_files`] 完全一致**（`is_deleted = 0 AND companion_of IS NULL`），
/// 这不是抄漏而是契约：软删项与 Live Photo 伴随视频在磁盘上**真实存在**，故「所有文件」模式
/// 会列出它们，但 DB 模式不显示它们。若给它们赋 `mediaId`，「共有项」的定义就糊了 ——
/// D-002 的对拍不变量要求共有项恰好是 **DB 模式会显示的那些**。它们照常 `registered = true`
/// （格式确实已注册），只是没有实体身份 → 行为等同未注册文件（选中 + reveal），不会被点进
/// 查看器，也进不了批处理。
pub fn map_media_entities(
    conn: &Connection,
    directory_id: i64,
    file_names: &[String],
) -> Result<std::collections::HashMap<String, (i64, String)>> {
    let mut out = std::collections::HashMap::new();
    if file_names.is_empty() {
        return Ok(out);
    }
    // 每个名字一个占位符（硬约束：绑定每一个参数，绝不拼接）。?1 是 directory_id，
    // 故文件名从 ?2 起。页长 200 → 201 个参数，远低于 SQLite 上限。
    let placeholders = (2..=file_names.len() + 1)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT file_name, id, media_type
         FROM media_items
         WHERE directory_id = ?1 AND is_deleted = 0 AND companion_of IS NULL
           AND file_name IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(file_names.len() + 1);
    binds.push(&directory_id);
    for n in file_names {
        binds.push(n);
    }
    let rows = stmt.query_map(binds.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for r in rows {
        let (name, id, mt) = r?;
        out.insert(name, (id, mt));
    }
    Ok(out)
}

/// 批量写回各目录的「直接（非递归）媒体计数」基线（T17a 增量剪枝判据之一）。
///
/// ⚠️ **列语义已复用**：`directories.media_count` 此前是 DEFAULT 0 的死列（仅 mock_data 写）。
/// T17a 起由 fast_scan 写入**直接子项**媒体数（仅本目录、不含子目录），与目录 `mtime` 一起
/// 构成 quick 剪枝判据：目录未变 → 跳过该目录**直接媒体项**的 per-file 工作(stat / cache_key /
/// upsert)；子目录仍各自下降、各自比对基线，故不做整子树跳过。**目录树 UI 的角标计数仍走
/// `get_directory_tree`/`get_directory_children` 的递归子查询别名（子树聚合），不读此列**——
/// 二者口径不同、互不影响。
///
/// **绝对覆盖**（非累加）：每个计数值就是本轮的最终直接子项数。
/// 事务边界由调用方持有：目录数随重扫规模增长，逐条 autocommit 会为每个目录各付一次提交
/// （调查 F-004），故此处只做语句缓存复用，由调用方的事务一次提交。
pub fn set_directory_media_counts(
    conn: &Connection,
    counts: &std::collections::HashMap<i64, i64>,
) -> Result<()> {
    let mut stmt = conn.prepare_cached("UPDATE directories SET media_count = ?1 WHERE id = ?2")?;
    for (dir_id, count) in counts {
        stmt.execute(params![count, dir_id])?;
    }
    Ok(())
}

pub fn get_directory_ancestors(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE ancestors(id, parent_id) AS (
            SELECT id, parent_id FROM directories WHERE id = ?1
            UNION ALL
            SELECT d.id, d.parent_id FROM directories d
            JOIN ancestors a ON a.parent_id = d.id
         )
         SELECT id FROM ancestors;",
    )?;

    let mut rows = stmt.query(params![id])?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        ids.push(row.get::<_, i64>(0)?);
    }

    ids.reverse();
    Ok(ids)
}

/// 解析目录的绝对文件系统路径（root.path 拼接 rel_path）。
pub fn get_directory_abs_path(conn: &Connection, dir_id: i64) -> Result<String> {
    conn.query_row(
        "SELECT r.path, d.rel_path
         FROM directories d JOIN scan_roots r ON r.id = d.root_id
         WHERE d.id = ?1",
        params![dir_id],
        |row| {
            let root: String = row.get(0)?;
            let rel: String = row.get(1)?;
            Ok(if rel.is_empty() {
                root
            } else {
                format!("{}/{}", root, rel)
            })
        },
    )
    .map_err(|_| AppError::Internal(format!("directory {} not found", dir_id)))
}

/// 目录所属 scan_root 的 `id` 与 `volume_id`(图片简单编辑单文件 ingest 用,方案 C §6:
/// `volume_id` 须随新 item 一并入库,才能参与缺失检测守门1,同 `fast_scan::run_fast_scan`
/// 的既有查法)。
pub fn get_root_and_volume_for_directory(
    conn: &Connection,
    directory_id: i64,
) -> Result<(i64, Option<i64>)> {
    conn.query_row(
        "SELECT r.id, r.volume_id FROM directories d JOIN scan_roots r ON r.id = d.root_id
         WHERE d.id = ?1",
        params![directory_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|_| AppError::Internal(format!("directory {} not found", directory_id)))
}

/// `dir_id` 的所有后代目录 id（含自身）。用于点击「无直接媒体」的文件夹时确定滚动目标——
/// 跳到其首个「有媒体」的后代子文件夹（问题1）。
pub fn get_directory_descendant_ids(conn: &Connection, dir_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM directories WHERE id = ?1
            UNION ALL
            SELECT d.id FROM directories d JOIN subtree s ON d.parent_id = s.id
         )
         SELECT id FROM subtree",
    )?;
    let rows = stmt.query_map(params![dir_id], |r| r.get::<_, i64>(0))?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 按 id 获取单个目录行。
pub fn get_directory(conn: &Connection, id: i64) -> Result<Directory> {
    conn.query_row(
        "SELECT id, root_id, parent_id, rel_path, name, depth, media_count, mtime, created_at
         FROM directories WHERE id = ?1",
        params![id],
        |row| {
            Ok(Directory {
                id: row.get(0)?,
                root_id: row.get(1)?,
                parent_id: row.get(2)?,
                rel_path: row.get(3)?,
                name: row.get(4)?,
                depth: row.get(5)?,
                media_count: row.get(6)?,
                mtime: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or(AppError::Internal(format!(
        "directory not found: id={id} | 未找到目录"
    )))
}

/// 给定父目录下是否已存在同名的直接子文件夹。
pub fn dir_has_child_named(conn: &Connection, parent_id: i64, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM directories WHERE parent_id = ?1 AND name = ?2",
        params![parent_id, name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 被移动子树中的一个目录（id + 当前 rel_path + depth）。
pub struct SubtreeDirRow {
    pub id: i64,
    pub rel_path: String,
    pub depth: i64,
}

/// 以 `root_dir_id` 为根的子树中的所有目录（含自身）。
pub fn get_directory_subtree(conn: &Connection, root_dir_id: i64) -> Result<Vec<SubtreeDirRow>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id, rel_path, depth) AS (
            SELECT id, rel_path, depth FROM directories WHERE id = ?1
            UNION ALL
            SELECT d.id, d.rel_path, d.depth FROM directories d
            JOIN subtree s ON d.parent_id = s.id
         )
         SELECT id, rel_path, depth FROM subtree",
    )?;
    let rows = stmt.query_map(params![root_dir_id], |row| {
        Ok(SubtreeDirRow {
            id: row.get(0)?,
            rel_path: row.get(1)?,
            depth: row.get(2)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 移动时重算 cache_key / 重命名缩略图所需的最小媒体项信息。
pub struct SubtreeMediaRow {
    pub id: i64,
    pub directory_id: i64,
    pub file_name: String,
    pub file_mtime: i64,
    pub file_mtime_ns: Option<i64>,
    pub cache_key: i64,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
}

/// 目录子树内的所有媒体项（含软删除项与伴随项）。
pub fn get_media_in_subtree(conn: &Connection, root_dir_id: i64) -> Result<Vec<SubtreeMediaRow>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM directories WHERE id = ?1
            UNION ALL
            SELECT d.id FROM directories d JOIN subtree s ON d.parent_id = s.id
         )
         SELECT m.id, m.directory_id, m.file_name, m.file_mtime, m.file_mtime_ns,
                m.cache_key, m.thumb_status, m.thumb_path
         FROM media_items m
         WHERE m.directory_id IN (SELECT id FROM subtree)",
    )?;
    let rows = stmt.query_map(params![root_dir_id], |row| {
        Ok(SubtreeMediaRow {
            id: row.get(0)?,
            directory_id: row.get(1)?,
            file_name: row.get(2)?,
            file_mtime: row.get(3)?,
            file_mtime_ns: row.get(4)?,
            cache_key: row.get(5)?,
            thumb_status: row.get(6)?,
            thumb_path: row.get(7)?,
        })
    })?;
    rows.map(|r| r.map_err(AppError::from)).collect()
}

/// 删除目录行（CASCADE 级联删除后代目录及其媒体）。返回直接匹配的目录行数（0 或 1）。
pub fn delete_directory_by_id(conn: &Connection, id: i64) -> Result<usize> {
    let n = conn.execute("DELETE FROM directories WHERE id = ?1", params![id])?;
    Ok(n)
}

/// 按 (root_id, rel_path) 查找目录 id。供复制撤销定位已登记的行。
pub fn find_directory_id(conn: &Connection, root_id: i64, rel_path: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM directories WHERE root_id = ?1 AND rel_path = ?2",
        params![root_id, rel_path],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}
