//! 已登记扫描根监听：通知只收线索，后台局部枚举后再提交数据库差集。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::reconcile::{self, DirectorySnapshot, MoveCandidate};
use crate::db::queries as q;
use crate::error::Result;
use crate::state::AppState;
use crate::utils::path::normalize_db_path;

pub(crate) const EVENT_FILES_CHANGED: &str = "files:changed";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilesChanged {
    pub root_id: i64,
    pub directories: Vec<String>,
}

struct DirtyRoot {
    dirs: BTreeSet<String>,
    renames: Vec<(String, String)>,
    work: VecDeque<String>,
    recursive: bool,
    first: Instant,
    last: Instant,
}
impl DirtyRoot {
    fn new(recursive: bool) -> Self {
        let now = Instant::now();
        Self {
            dirs: BTreeSet::new(),
            renames: Vec::new(),
            work: VecDeque::new(),
            recursive,
            first: now,
            last: now,
        }
    }
    fn add(&mut self, rel: String) {
        self.last = Instant::now();
        if self.recursive && self.dirs.contains("") {
            return;
        }
        if self.dirs.len() < 512 {
            self.dirs.insert(rel);
        } else {
            self.dirs.clear();
            self.dirs.insert(String::new());
            self.recursive = true;
        }
    }
}

struct PreparedDirectory {
    snapshot: DirectorySnapshot,
    moved: Option<MoveCandidate>,
    children: Vec<Option<MoveCandidate>>,
    media: Vec<Option<MoveCandidate>>,
}

struct EnrichmentJob {
    cancel: tokio_util::sync::CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for EnrichmentJob {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn parent_rel(rel: &str) -> String {
    Path::new(rel)
        .parent()
        .map(|p| normalize_db_path(&p.to_string_lossy()))
        .unwrap_or_default()
}

/// 明确的同根 rename 可为尚未建立身份基线的旧记录提供迁移证据。
fn rename_candidate(
    conn: &rusqlite::Connection,
    root_id: i64,
    target: &str,
    renames: &[(String, String)],
    directory: bool,
) -> Result<Option<MoveCandidate>> {
    let Some((old, _)) = renames.iter().find(|(_, new)| new == target) else {
        return Ok(None);
    };
    if directory {
        Ok(conn
            .query_row(
                "SELECT id,rel_path FROM directories WHERE root_id=?1 AND rel_path=?2 AND physical_key IS NULL",
                params![root_id, old],
                |r| {
                    Ok(MoveCandidate {
                        id: r.get(0)?,
                        old_rel: r.get(1)?,
                        old_name: None,
                    })
                },
            )
            .optional()?)
    } else {
        let name = Path::new(old)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(conn.query_row("SELECT m.id FROM media_items m JOIN directories d ON d.id=m.directory_id WHERE d.root_id=?1 AND d.rel_path=?2 AND m.file_name=?3 AND m.is_deleted=0 AND m.physical_key IS NULL",params![root_id,parent_rel(old),name],|r|Ok(MoveCandidate{id:r.get(0)?,old_rel:parent_rel(old),old_name:Some(name.clone())})).optional()?)
    }
}

fn prepare(
    state: &AppState,
    root_id: i64,
    root: &Path,
    dirty: &mut DirtyRoot,
) -> Result<Vec<PreparedDirectory>> {
    let catalog = state.exotic_catalog.snapshot();
    let mut queue = std::mem::take(&mut dirty.work);
    queue.extend(std::mem::take(&mut dirty.dirs));
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    while out.len() < 32 {
        let Some(rel) = queue.pop_front() else { break };
        if rel.split('/').any(|component| component.starts_with('.')) {
            continue;
        }
        if !visited.insert(rel.clone()) {
            continue;
        }
        let snapshot = match reconcile::read_directory(root, &rel, &catalog) {
            Ok(v) => v,
            Err(e) => {
                if matches!(std::fs::symlink_metadata(root.join(&rel)), Err(error) if error.kind()==std::io::ErrorKind::NotFound)
                {
                    continue;
                }
                tracing::warn!(root_id,error=%e,"目录未完整读取，保留待核对范围");
                dirty.work = queue;
                dirty.dirs.insert(rel);
                dirty.dirs.extend(
                    out.iter()
                        .map(|p: &PreparedDirectory| p.snapshot.rel.clone()),
                );
                return Err(e);
            }
        };
        let planned = (|| {
            let conn = state.db_read_pool.get()?;
            let own = reconcile::candidate(&conn, root_id, snapshot.key.as_deref(), true)?.or(
                rename_candidate(&conn, root_id, &rel, &dirty.renames, true)?,
            );
            let mut children = Vec::new();
            for (child, key) in &snapshot.dirs {
                children.push(
                    reconcile::candidate(&conn, root_id, key.as_deref(), true)?.or(
                        rename_candidate(&conn, root_id, child, &dirty.renames, true)?,
                    ),
                );
            }
            let mut media = Vec::new();
            for (_, item, key) in &snapshot.files {
                let target = if rel.is_empty() {
                    item.file_name.clone()
                } else {
                    format!("{rel}/{}", item.file_name)
                };
                media.push(
                    reconcile::candidate(&conn, root_id, key.as_deref(), false)?.or(
                        rename_candidate(&conn, root_id, &target, &dirty.renames, false)?,
                    ),
                );
            }
            let mut descendants = Vec::new();
            for (child, key) in &snapshot.dirs {
                let known: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM directories WHERE root_id=?1 AND rel_path=?2 AND is_missing=0 AND physical_key IS ?3 AND physical_key IS NOT NULL)",params![root_id,child,key],|r|r.get(0))?;
                if dirty.recursive || !known {
                    descendants.push(child.clone());
                }
            }
            Ok::<_, crate::error::AppError>((own, children, media, descendants))
        })();
        let (moved, children, media, descendants) = match planned {
            Ok(planned) => planned,
            Err(error) => {
                // 数据库读取失败也不能丢掉已取出的目录队列，恢复整个未提交范围。
                dirty.work = queue;
                dirty.dirs.insert(rel);
                dirty
                    .dirs
                    .extend(out.iter().map(|p| p.snapshot.rel.clone()));
                return Err(error);
            }
        };
        let moved = reconcile::absent_candidate(root, moved);
        let children = children
            .into_iter()
            .map(|m| reconcile::absent_candidate(root, m))
            .collect::<Vec<_>>();
        let media = media
            .into_iter()
            .map(|m| reconcile::absent_candidate(root, m))
            .collect();
        queue.extend(descendants);
        out.push(PreparedDirectory {
            snapshot,
            moved,
            children,
            media,
        });
    }
    dirty.work = queue;
    Ok(out)
}

fn apply(
    conn: &rusqlite::Connection,
    root_id: i64,
    prepared: &[PreparedDirectory],
    catalog: &crate::exotic::catalog::CatalogSnapshot,
) -> Result<Vec<String>> {
    let volume: Option<i64> = conn.query_row(
        "SELECT volume_id FROM scan_roots WHERE id=?1",
        [root_id],
        |r| r.get(0),
    )?;
    let tx = conn.unchecked_transaction()?;
    let mut changed = BTreeSet::new();
    // 先处理所有迁移再做旧目录差集，避免源目录在同批中先被判 missing。
    for p in prepared {
        let s = &p.snapshot;
        reconcile::upsert_observed_directory(
            &tx,
            root_id,
            &s.rel,
            s.key.as_deref(),
            p.moved.as_ref(),
        )?;
        changed.insert(s.rel.clone());
        if let Some(m) = &p.moved {
            changed.insert(parent_rel(&m.old_rel));
        }
        for ((child, key), moved) in s.dirs.iter().zip(&p.children) {
            reconcile::upsert_observed_directory(
                &tx,
                root_id,
                child,
                key.as_deref(),
                moved.as_ref(),
            )?;
            if let Some(m) = moved {
                changed.insert(parent_rel(&m.old_rel));
            }
        }
        let dir_id = reconcile::ensure_directory(&tx, root_id, &s.rel)?;
        for ((_, raw, key), moved) in s.files.iter().zip(&p.media) {
            let mut item = raw.clone();
            item.directory_id = dir_id;
            item.cache_key = crate::utils::hash::compute_cache_key_with_mtime_ns(
                dir_id,
                &s.rel,
                &item.file_name,
                item.file_mtime,
                item.file_mtime_ns,
            );
            let outcome = reconcile::upsert_observed_item(
                &tx,
                &item,
                volume,
                key.as_deref(),
                moved.as_ref(),
            )?;
            if matches!(outcome, q::UpsertOutcome::SuspectChanged(_)) {
                // 实时内容变化直接失效源，不为每次写文件再读一遍内容摘要。
                q::resolve_suspect_change(&tx, outcome.id(), &item, volume, None)?;
            }
            if let Some(off) = catalog.resolve_format(&item.file_format) {
                if matches!(
                    outcome,
                    q::UpsertOutcome::SourceChanged(_) | q::UpsertOutcome::SuspectChanged(_)
                ) {
                    q::invalidate_exotic_tasks_for_item(&tx, outcome.id())?;
                }
                let caps = off
                    .capabilities
                    .iter()
                    .map(|c| c.as_str().to_string())
                    .collect::<Vec<_>>();
                q::seed_exotic_tasks_for_item(&tx, outcome.id(), &off.plugin_id, &caps)?;
            }
            if let Some(m) = moved {
                changed.insert(m.old_rel.clone());
            }
        }
    }
    for p in prepared {
        let s = &p.snapshot;
        let dir_id = reconcile::ensure_directory(&tx, root_id, &s.rel)?;
        let names: HashSet<&str> = s
            .files
            .iter()
            .map(|(_, f, _)| f.file_name.as_str())
            .collect();
        let missing = {
            let mut stmt=tx.prepare("SELECT id,file_name FROM media_items WHERE directory_id=?1 AND is_deleted=0 AND availability!='missing'")?;
            let rows = stmt.query_map([dir_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|(id, name)| (!names.contains(name.as_str())).then_some(id))
                .collect::<Vec<_>>()
        };
        reconcile::mark_missing_ids(&tx, &missing)?;
        let children: HashSet<&str> = s.dirs.iter().map(|(rel, _)| rel.as_str()).collect();
        let missing_dirs = {
            let mut stmt = tx.prepare(
                "SELECT id,rel_path FROM directories WHERE parent_id=?1 AND is_missing=0",
            )?;
            let rows = stmt.query_map([dir_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|(id, rel)| (!children.contains(rel.as_str())).then_some(id))
                .collect::<Vec<_>>()
        };
        for id in missing_dirs {
            let ids = {
                let mut stmt=tx.prepare("WITH RECURSIVE sub(id) AS(SELECT ?1 UNION ALL SELECT d.id FROM directories d JOIN sub ON d.parent_id=sub.id) SELECT m.id FROM media_items m JOIN sub ON m.directory_id=sub.id WHERE m.is_deleted=0 AND m.availability!='missing'")?;
                let rows = stmt.query_map([id], |r| r.get(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            reconcile::mark_missing_ids(&tx, &ids)?;
            tx.execute("WITH RECURSIVE sub(id) AS(SELECT ?1 UNION ALL SELECT d.id FROM directories d JOIN sub ON d.parent_id=sub.id) UPDATE directories SET is_missing=1 WHERE id IN(SELECT id FROM sub)",[id])?;
        }
        tx.execute("UPDATE directories SET media_count=(SELECT count(*) FROM media_items WHERE directory_id=?1 AND is_deleted=0 AND availability!='missing' AND companion_of IS NULL) WHERE id=?1",[dir_id])?;
    }
    tx.commit()?;
    Ok(changed.into_iter().collect())
}

fn reconcile_root(
    state: &AppState,
    root_id: i64,
    path: &Path,
    dirty: &mut DirtyRoot,
) -> Result<Option<Vec<String>>> {
    let Some(epoch) = state.current_database_epoch() else {
        return Ok(None);
    };
    let idle =
        state.with_scan_root_epoch_write(root_id, epoch, || !state.any_scan_running(&[root_id]));
    if idle != Some(true) {
        return Ok(None);
    }
    let filesystem_scopes = dirty.dirs.clone();
    let prepared = prepare(state, root_id, path, dirty)?;
    let catalog = state.exotic_catalog.snapshot();
    let result = state
        .with_scan_root_epoch_write(root_id, epoch, || {
            if state.any_scan_running(&[root_id]) {
                return Ok(None);
            }
            // 根不可访问时不作缺失差集，离线由卷服务处理。文件 IO 不占 writer。
            if !path.is_dir()
                || prepared
                    .iter()
                    .any(|p| !reconcile::snapshot_is_current(path, &p.snapshot))
            {
                return Ok(None);
            }
            let conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
            let actual: Option<String> = conn
                .query_row(
                    "SELECT path FROM scan_roots WHERE id=?1 AND is_active=1",
                    [root_id],
                    |r| r.get(0),
                )
                .optional()?;
            if actual.as_deref() != Some(path.to_string_lossy().as_ref()) {
                return Ok(None);
            }
            let mut directories = apply(&conn, root_id, &prepared, &catalog)?;
            directories.extend(filesystem_scopes.iter().cloned());
            // 分类裁剪与媒体数聚合依赖祖先；局部文件变化也须刷新已加载的祖先行。
            let ancestors = directories
                .iter()
                .flat_map(|rel| {
                    let mut paths = Vec::new();
                    let mut parent = rel.clone();
                    while !parent.is_empty() {
                        parent = parent_rel(&parent);
                        paths.push(parent.clone());
                    }
                    paths
                })
                .collect::<Vec<_>>();
            directories.extend(ancestors);
            directories.sort_unstable();
            directories.dedup();
            state.bump_data_version();
            state.tree_snapshots.invalidate_root(root_id);
            Ok(Some(directories))
        })
        .unwrap_or(Ok(None));
    if !matches!(result, Ok(Some(_))) {
        dirty.dirs.extend(filesystem_scopes);
        dirty
            .dirs
            .extend(prepared.iter().map(|p| p.snapshot.rel.clone()));
    }
    result
}

fn collect_event(
    event: &Event,
    roots: &HashMap<i64, String>,
    pending: &mut BTreeMap<i64, DirtyRoot>,
) {
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    for (id, root) in roots {
        let root = Path::new(root);
        let relatives = event
            .paths
            .iter()
            .filter_map(|path| {
                path.strip_prefix(root)
                    .ok()
                    .map(|p| normalize_db_path(&p.to_string_lossy()))
            })
            .collect::<Vec<_>>();
        if relatives.is_empty() && !event.need_rescan() {
            continue;
        }
        let dirty = pending.entry(*id).or_insert_with(|| DirtyRoot::new(false));
        if event.need_rescan() {
            dirty.recursive = true;
            dirty.add(String::new());
        }
        for rel in &relatives {
            dirty.add(parent_rel(rel));
        }
        if matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        ) && relatives.len() == 2
            && dirty.renames.len() < 512
        {
            dirty
                .renames
                .push((relatives[0].clone(), relatives[1].clone()));
        }
    }
}

/// 生命周期由后台句柄池持有；回调有界，溢出保留根级补偿信号。
pub(crate) fn spawn(app: AppHandle, state: Arc<AppState>) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Event>(256);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) => {
                    if sender.try_send(event).is_err() {
                        callback_overflow.store(true, Ordering::Release);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error,"扫描目录监听事件失败");
                    callback_overflow.store(true, Ordering::Release);
                }
            });
        let mut watcher: RecommendedWatcher = match watcher {
            Ok(w) => w,
            Err(error) => {
                tracing::warn!(%error,"扫描目录监听启动失败");
                return;
            }
        };
        let mut roots = HashMap::<i64, String>::new();
        let mut pending = BTreeMap::<i64, DirtyRoot>::new();
        let mut enrichment_needed = BTreeSet::new();
        let mut enrichments = HashMap::<i64, EnrichmentJob>::new();
        let mut root_check = Instant::now() - Duration::from_secs(3);
        let mut observed_epoch = state.current_database_epoch();
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tick.tick().await;
            if root_check.elapsed() >= Duration::from_secs(2) {
                root_check = Instant::now();
                let state_read = state.clone();
                let listed = tokio::task::spawn_blocking(move || {
                    let list = {
                        let conn = state_read.db_read_pool.get()?;
                        q::list_scan_roots(&conn)?
                    };
                    Ok::<_, crate::error::AppError>(
                        list.into_iter()
                            .filter(|r| r.is_active && Path::new(&r.path).is_dir())
                            .collect::<Vec<_>>(),
                    )
                })
                .await;
                if let Ok(Ok(list)) = listed {
                    let epoch = state.current_database_epoch();
                    if epoch != observed_epoch {
                        for path in roots.values() {
                            let _ = watcher.unwatch(Path::new(path));
                        }
                        roots.clear();
                        pending.clear();
                        enrichments.clear();
                        enrichment_needed.clear();
                        observed_epoch = epoch;
                    }
                    let wanted = list
                        .into_iter()
                        .filter(|r| r.is_active)
                        .map(|r| (r.id, r.path))
                        .collect::<HashMap<_, _>>();
                    let mut next = HashMap::new();
                    for (id, path) in &roots {
                        if wanted.get(id) != Some(path) {
                            let _ = watcher.unwatch(Path::new(path));
                            pending.remove(id);
                            enrichments.remove(id);
                            enrichment_needed.remove(id);
                        }
                    }
                    for (id, path) in &wanted {
                        if roots.get(id) != Some(path) {
                            match watcher.watch(Path::new(path), RecursiveMode::Recursive) {
                                Ok(()) => {
                                    let mut dirty = DirtyRoot::new(true);
                                    dirty.add(String::new());
                                    pending.insert(*id, dirty);
                                    next.insert(*id, path.clone());
                                }
                                Err(error) => tracing::warn!(root_id=id,%error,"扫描根监听失败"),
                            }
                        } else {
                            next.insert(*id, path.clone());
                        }
                    }
                    roots = next;
                }
            }
            while let Ok(event) = receiver.try_recv() {
                collect_event(&event, &roots, &mut pending);
            }
            enrichments.retain(|_, job| !job.handle.is_finished());
            let to_enrich = enrichment_needed
                .iter()
                .copied()
                .filter(|id| !enrichments.contains_key(id) && !state.any_scan_running(&[*id]))
                .collect::<Vec<_>>();
            for id in to_enrich {
                let Some(epoch) = state.current_database_epoch() else {
                    continue;
                };
                enrichment_needed.remove(&id);
                let cancel = tokio_util::sync::CancellationToken::new();
                let work_cancel = cancel.clone();
                let work_state = state.clone();
                let work_app = app.clone();
                let handle = tokio::task::spawn_blocking(move || {
                    if let Err(error) = super::enricher::run_watch_enrichment(
                        &work_app,
                        &work_state,
                        id,
                        epoch,
                        &work_cancel,
                    ) {
                        if !matches!(error, crate::error::AppError::Cancelled) {
                            tracing::warn!(root_id=id,%error,"文件变化元数据补全失败");
                        }
                    }
                });
                enrichments.insert(id, EnrichmentJob { cancel, handle });
            }
            if overflow.swap(false, Ordering::AcqRel) {
                for id in roots.keys() {
                    let dirty = pending.entry(*id).or_insert_with(|| DirtyRoot::new(true));
                    dirty.recursive = true;
                    dirty.add(String::new());
                }
            }
            let ready = pending
                .iter()
                .filter(|(_, d)| {
                    d.last.elapsed() >= Duration::from_millis(250)
                        || d.first.elapsed() >= Duration::from_secs(1)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in ready {
                let Some(path) = roots.get(&id).cloned() else {
                    continue;
                };
                let mut dirty = pending.remove(&id).unwrap();
                // 新溢出补偿覆盖已排队子范围；从根重新递归即可。
                if dirty.recursive && dirty.dirs.contains("") {
                    dirty.work.clear();
                }
                let state_work = state.clone();
                let app_work = app.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let result = reconcile_root(&state_work, id, Path::new(&path), &mut dirty);
                    if let Ok(Some(directories)) = &result {
                        state_work
                            .wake_exotic(crate::exotic::coordinator::WakeReason::ScanCommitted);
                        let _ = app_work.emit(
                            EVENT_FILES_CHANGED,
                            FilesChanged {
                                root_id: id,
                                directories: directories.clone(),
                            },
                        );
                    }
                    (result, dirty)
                })
                .await;
                match result {
                    Ok((Ok(Some(_)), mut dirty)) if !dirty.work.is_empty() => {
                        dirty.first = Instant::now();
                        dirty.last = dirty.first;
                        pending.insert(id, dirty);
                    }
                    Ok((Ok(Some(_)), _)) => {
                        enrichment_needed.insert(id);
                    }
                    Ok((Ok(None), mut dirty)) => {
                        dirty.last = Instant::now();
                        dirty.first = dirty.last;
                        pending.insert(id, dirty);
                    }
                    Ok((Err(error), mut dirty)) => {
                        tracing::warn!(root_id=id,%error,"文件变化对账失败，保留待核对范围");
                        dirty.last = Instant::now();
                        dirty.first = dirty.last;
                        pending.insert(id, dirty);
                    }
                    Err(error) => tracing::warn!(root_id=id,%error,"文件变化对账任务失败"),
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exotic::catalog::{CatalogSnapshot, CatalogStore};

    fn fixture() -> (tempfile::TempDir, AppState, std::path::PathBuf) {
        let tmp = tempfile::Builder::new()
            .prefix("scrollery-watch-")
            .tempdir()
            .unwrap();
        let root = tmp.path().join("assets");
        std::fs::create_dir_all(root.join("source/folder/nested")).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("source/loose.jpg"), b"loose").unwrap();
        std::fs::write(root.join("source/folder/nested/image.jpg"), b"nested").unwrap();
        let db_path = tmp.path().join("library.sqlite");
        let writer = crate::db::create_write_connection(&db_path).unwrap();
        {
            let conn = writer.lock().unwrap();
            crate::db::schema::initialize_schema(&conn).unwrap();
            conn.execute(
                "INSERT INTO scan_roots(id,path,alias) VALUES(1,?1,'assets')",
                [root.to_string_lossy().as_ref()],
            )
            .unwrap();
        }
        let pool = crate::db::create_read_pool(&db_path, 1).unwrap();
        let (config, _) =
            crate::config::ConfigManager::load_or_init(tmp.path().join("config.toml")).unwrap();
        let (logger, _guard) = tracing_appender::non_blocking(std::io::sink());
        let state = AppState::new(
            writer,
            pool,
            Arc::new(config),
            tmp.path().join("cache"),
            tmp.path().join("logs"),
            crate::logging::LogRingBuffer::new(Arc::from("file-sync-test"), 1),
            logger.error_counter(),
            tmp.path().to_path_buf(),
            tmp.path().join("exotic"),
            256,
            0,
            "balanced".into(),
            "cpu".into(),
            false,
            80,
            Arc::new(CatalogStore::with_snapshot(
                CatalogSnapshot::builtin().unwrap(),
            )),
        );
        let mut dirty = DirtyRoot::new(true);
        dirty.add(String::new());
        assert!(reconcile_root(&state, 1, &root, &mut dirty)
            .unwrap()
            .is_some());
        (tmp, state, root)
    }

    fn online_count(state: &AppState) -> i64 {
        state
            .db_writer
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM media_items WHERE availability='online'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn native_events_reconcile_mixed_moves_without_rescan_and_preserve_ids() {
        let (_tmp, state, root) = fixture();
        let original: Vec<i64> = state
            .db_writer
            .lock()
            .unwrap()
            .prepare("SELECT id FROM media_items ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        state
            .db_writer
            .lock()
            .unwrap()
            .execute("UPDATE media_items SET is_favorited=1,rating=5", [])
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let _ = sender.send(event);
        })
        .unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        std::fs::rename(root.join("source/folder"), root.join("target/folder")).unwrap();
        std::fs::rename(
            root.join("source/loose.jpg"),
            root.join("target/renamed.txt"),
        )
        .unwrap();
        let roots = HashMap::from([(1, root.to_string_lossy().into_owned())]);
        let mut pending = BTreeMap::new();
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(300)) {
                Ok(Ok(event)) => collect_event(&event, &roots, &mut pending),
                Ok(Err(error)) => panic!("native watcher failed: {error}"),
                Err(_) if !pending.is_empty() => break,
                Err(_) => {}
            }
        }
        assert!(pending.contains_key(&1), "真实系统移动必须产生可对账事件");
        let mut dirty = pending.remove(&1).unwrap();
        let changed = reconcile_root(&state, 1, &root, &mut dirty)
            .unwrap()
            .unwrap();
        assert!(changed.contains(&"source".to_string()) && changed.contains(&"target".to_string()));
        let conn = state.db_writer.lock().unwrap();
        let now: Vec<i64> = conn
            .prepare("SELECT id FROM media_items ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(now, original);
        assert_eq!(conn.query_row("SELECT count(*) FROM media_items WHERE is_favorited=1 AND rating=5 AND availability='online'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(conn.query_row("SELECT count(*) FROM media_items m JOIN directories d ON d.id=m.directory_id WHERE d.rel_path IN ('target','target/folder/nested')",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        let classification: (String, String) = conn
            .query_row(
                "SELECT file_format,media_type FROM media_items WHERE file_name='renamed.txt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(classification, ("txt".into(), "document".into()));
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM directories WHERE rel_path LIKE 'source/folder%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn queued_native_writes_after_prepare_converge_after_stale_commit() {
        let (_tmp, state, root) = fixture();
        let file = root.join("source/loose.jpg");
        let (id, revision): (i64, i64) = {
            let conn = state.db_writer.lock().unwrap();
            conn.execute(
                "UPDATE media_items SET is_favorited=1,rating=5,thumb_status=1,thumb_path='old-thumb' WHERE file_name='loose.jpg'",
                [],
            )
            .unwrap();
            conn.query_row(
                "SELECT id,source_revision FROM media_items WHERE file_name='loose.jpg'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Event>(256);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) => {
                    if sender.try_send(event).is_err() {
                        callback_overflow.store(true, Ordering::Release);
                    }
                }
                Err(_) => callback_overflow.store(true, Ordering::Release),
            })
            .unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        let mut dirty = DirtyRoot::new(false);
        dirty.add("source".into());
        let prepared = prepare(&state, 1, &root, &mut dirty).unwrap();
        assert!(dirty.dirs.is_empty() && dirty.work.is_empty());
        assert_eq!(prepared[0].snapshot.files[0].1.file_size, 5);

        // 固定枚举后、提交前的改写窗口；暂不消费通知，复现后台对账期间的排队。
        std::fs::write(&file, b"intermediate contents").unwrap();
        let latest = b"latest contents after two writes";
        std::fs::write(&file, latest).unwrap();
        assert!(reconcile::snapshot_is_current(&root, &prepared[0].snapshot));
        {
            let conn = state.db_writer.lock().unwrap();
            apply(&conn, 1, &prepared, &CatalogSnapshot::builtin().unwrap()).unwrap();
            assert_eq!(
                conn.query_row("SELECT file_size FROM media_items WHERE id=?1", [id], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
                5
            );
        }

        let roots = HashMap::from([(1, root.to_string_lossy().into_owned())]);
        let mut pending = BTreeMap::new();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !pending.contains_key(&1) && Instant::now() < deadline {
            while let Ok(event) = receiver.try_recv() {
                collect_event(&event, &roots, &mut pending);
            }
            if pending.is_empty() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert!(!overflow.load(Ordering::Acquire));
        let mut next = pending
            .remove(&1)
            .expect("改写通知必须重新标脏旧快照的父目录");
        assert!(next.dirs.contains("source"));
        let changed = reconcile_root(&state, 1, &root, &mut next)
            .unwrap()
            .unwrap();
        assert!(changed.contains(&"source".into()) && changed.contains(&String::new()));
        assert!(next.dirs.is_empty() && next.work.is_empty());
        let conn = state.db_writer.lock().unwrap();
        let actual: (i64, i64, i64, i64, Option<String>, i64, i64, String) = conn
            .query_row(
                "SELECT id,file_size,source_revision,thumb_status,thumb_path,is_favorited,rating,availability FROM media_items WHERE file_name='loose.jpg'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
            )
            .unwrap();
        assert_eq!(
            actual,
            (
                id,
                latest.len() as i64,
                revision + 1,
                0,
                None,
                1,
                5,
                "online".into()
            )
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM media_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn local_delete_marks_only_observed_scope_and_keeps_history() {
        let (_tmp, state, root) = fixture();
        std::fs::remove_file(root.join("source/loose.jpg")).unwrap();
        let mut dirty = DirtyRoot::new(false);
        dirty.add("source".into());
        reconcile_root(&state, 1, &root, &mut dirty)
            .unwrap()
            .unwrap();
        assert_eq!(online_count(&state), 1);
        let conn = state.db_writer.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM media_items WHERE availability='missing' AND is_deleted=0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM media_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        // 历史缺失项保留人工数据，但不能进入后台元数据队列生成错误的最小记录。
        assert_eq!(q::get_metadata_workload(&conn, 1).unwrap().files, 1);
    }

    #[test]
    fn incomplete_directory_read_never_marks_media_missing() {
        let (_tmp, state, root) = fixture();
        let mut dirty = DirtyRoot::new(false);
        dirty.add("source/loose.jpg".into());
        assert!(reconcile_root(&state, 1, &root, &mut dirty).is_err());
        assert_eq!(online_count(&state), 2);
        assert!(!dirty.dirs.is_empty());
    }

    #[test]
    fn offline_root_and_active_manual_scan_defer_reconciliation() {
        let (_tmp, state, root) = fixture();
        let original_id: i64 = {
            let conn = state.db_writer.lock().unwrap();
            conn.execute(
                "UPDATE media_items SET is_favorited=1,rating=4 WHERE file_name='image.jpg'",
                [],
            )
            .unwrap();
            conn.query_row(
                "SELECT id FROM media_items WHERE file_name='image.jpg'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let mut dirty = DirtyRoot::new(true);
        dirty.add(String::new());
        let (generation, _) = state.new_scan_run_token(1);
        assert!(reconcile_root(&state, 1, &root, &mut dirty)
            .unwrap()
            .is_none());
        assert!(state.finish_scan(1, generation));
        std::fs::rename(&root, root.with_file_name("unmounted")).unwrap();
        assert!(reconcile_root(&state, 1, &root, &mut dirty)
            .unwrap()
            .is_none());
        assert_eq!(online_count(&state), 2);

        let offline = root.with_file_name("unmounted");
        std::fs::write(
            offline.join("source/folder/nested/image.jpg"),
            b"offline edit",
        )
        .unwrap();
        std::fs::write(offline.join("source/folder/nested/new.jpg"), b"new").unwrap();
        std::fs::rename(offline, &root).unwrap();
        // 重挂监听会重新建立根级递归补偿，不能依赖离线期间的文件事件。
        let mut reconnected = DirtyRoot::new(true);
        reconnected.add(String::new());
        assert!(reconcile_root(&state, 1, &root, &mut reconnected)
            .unwrap()
            .is_some());
        assert_eq!(online_count(&state), 3);
        let conn = state.db_writer.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT id,file_size,is_favorited,rating FROM media_items WHERE file_name='image.jpg'", [], |r| Ok((r.get::<_, i64>(0)?,r.get::<_, i64>(1)?,r.get::<_, i64>(2)?,r.get::<_, i64>(3)?))).unwrap(),
            (original_id, 12, 1, 4)
        );
        assert!(reconnected.dirs.is_empty() && reconnected.work.is_empty());
    }

    #[test]
    fn obsolete_epoch_rejects_commit_and_changed_snapshot_requires_reread() {
        let (_tmp, state, root) = fixture();
        let epoch = state.current_database_epoch().unwrap();
        state.with_scan_lifecycle_exclusive(|| {});
        assert_eq!(state.with_scan_root_epoch_write(1, epoch, || true), None);
        let snapshot =
            reconcile::read_directory(&root, "source", &CatalogSnapshot::builtin().unwrap())
                .unwrap();
        std::fs::write(root.join("source/new.jpg"), b"new").unwrap();
        assert!(!reconcile::snapshot_is_current(&root, &snapshot));
        assert_eq!(online_count(&state), 2);
    }

    #[test]
    fn event_storm_and_pathless_rescan_keep_bounded_compensation() {
        let roots = HashMap::from([(1, String::from("C:/assets"))]);
        let mut pending = BTreeMap::new();
        for i in 0..1000 {
            let event = Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(std::path::PathBuf::from(format!("C:/assets/{i}/a.jpg")));
            collect_event(&event, &roots, &mut pending);
        }
        let dirty = &pending[&1];
        assert_eq!(dirty.dirs, BTreeSet::from([String::new()]));
        assert!(dirty.recursive);
        pending.clear();
        let mut event = Event::new(EventKind::Other);
        event.attrs.set_flag(notify::event::Flag::Rescan);
        collect_event(&event, &roots, &mut pending);
        assert!(pending[&1].recursive);
        assert!(pending[&1].dirs.contains(""));
    }
}
