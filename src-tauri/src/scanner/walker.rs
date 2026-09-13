// src-tauri/src/scanner/walker.rs
//! 使用 `walkdir` 的递归目录遍历器。
//! 生成按媒体类型分类的 `WalkedFile` 条目的扁平列表。

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio_util::sync::CancellationToken;
use walkdir::{DirEntry, WalkDir};

use crate::exotic::catalog::CatalogSnapshot;
use crate::utils::format::{classify_media_type, MediaType};

/// 扫描期分类（R13）：common-first，仅当常见格式表返回 `None` 时查 Catalog。
/// **只依赖扩展名**——exotic 识别不需 enrichment（宽高/时长），扫描事务内即可判定、零额外 IO。
/// Catalog 不可覆盖常见格式（common 优先）。同一轮 walk 共用同一 snapshot，保证分类一致。
/// `pub(crate)`：`formats::merged_formats` 的契约测试要拿它当**扫描期真值**对拍合并集
/// （S 线 §6.4 测试项③）—— UI 说「能筛 X」而扫描器不收 X 是最坏的漂移。
pub(crate) fn classify_scanned_file(ext: &str, catalog: &CatalogSnapshot) -> Option<MediaType> {
    classify_media_type(ext).or_else(|| catalog.media_kind(ext).map(Into::into))
}

/// 一条遍历错误（权限/IO/symlink loop/metadata 失败）。
/// 🔴 数据安全关键：这些是**未能进入 `seen` 集的真实文件**——缺失检测的差集若在
/// `errors` 非空时仍执行，会把它们误判为「已删除」。故 `WalkReport.complete` 据此守门。
#[derive(Debug, Clone)]
pub struct WalkError {
    /// 出错路径（取不到则空串）。
    pub path: String,
    /// 错误类别 + 简述（诊断用；分类已足够支撑「不完整不删除」判定）。
    pub reason: String,
}

/// 一轮**流式**遍历的收尾结论（T12）：遍历错误 + 是否「完整」+ 是否被取消。
/// 流式遍历不再全量持有 `files`——文件在 [`MediaWalker`] 迭代中被逐批消费；遍历结束后由
/// [`MediaWalker::finish`] 产出本结论。
///
/// `complete == errors.is_empty() && !cancelled`：唯有完整扫描（零遍历/metadata 错误、未取消）
/// 才允许下游差集删除（不变量「不完整扫描 ≠ 删除」，Part2 §3.2.2）。
#[derive(Debug)]
pub struct WalkOutcome {
    pub errors: Vec<WalkError>,
    pub complete: bool,
    /// 中途被 `CancellationToken` 取消 → 视为不完整（`complete=false`），且调用方应返回 `Cancelled`。
    pub cancelled: bool,
}

/// 快速扫描目录剪枝器(阶段3):遍历器在**进入目录时**询问是否可剪枝。
///
/// 返回 `true` = 该目录 mtime 与扫描启动快照一致,其**直接文件**将不再逐个 stat/产出
/// (子目录仍独立下降,语义同 T17b 逐目录、非递归)。实现方负责在判定为可剪枝时把该目录
/// 全部未删媒体 id 回填 `seen`,守住「跳过 ≠ 消失」的缺失检测红线。
pub(crate) trait DirPruner {
    fn should_prune_dir(&self, abs_dir: &Path, rel_path: &str) -> bool;
}

/// 单个发现的文件条目。
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// 文件的绝对路径。
    pub abs_path: PathBuf,
    /// 文件名 (basename)。
    pub file_name: String,
    /// 小写文件扩展名。
    pub extension: String,
    /// 分类的媒体类型。
    pub media_type: MediaType,
    /// 文件大小（以字节为单位）。
    pub file_size: i64,
    /// 最后修改时间作为 Unix 时间戳。
    pub file_mtime: i64,
    /// 最后修改时间作为 Unix 纳秒时间戳；无法取得或超出 SQLite INTEGER 范围时为 0。
    /// 去重分析用它和文件大小做读前/读后漂移复核，不能退回秒级时间戳。
    pub file_mtime_ns: i64,
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

/// **流式**媒体遍历器（T12）：按 `walkdir` 顺序逐个产出 [`WalkedFile`]，**不再全量收 `Vec`**，
/// 使 fast_scan 内存峰值从 O(N)（百万级约 600MB，含 `order_for_view` 的 clone 再翻倍）降到
/// O(batch)（一次只持有 500 项）。
///
/// 实现 [`Iterator`]：消费方（fast_scan）自行按批拉取（`while let Some(f) = w.next()` 攒满一批即
/// 入库）。遍历过程中：
/// - **隐藏目录剪枝**：`filter_entry(!is_hidden)` 整棵剪掉点前缀目录（如 `.git`），不递归进去。
/// - **遍历/metadata 错误不再静默丢弃**，而是累积进 `errors`（🔴 数据安全：这些是未进 seen 的真实
///   文件，差集若在有错误时仍跑会误删它们）。遍历结束调 [`finish`](MediaWalker::finish) 取
///   [`WalkOutcome`]，其 `complete` 即缺失检测「不完整不删」门闩（Part2 §3.2.2）。
/// - **取消**：每步检查 `CancellationToken`，触发即停止产出并置 `cancelled`（→ `complete=false`）。
///
/// 分类未命中（非媒体/非已知 exotic 扩展名）**不算错误**——正常跳过，不影响完整性。
pub struct MediaWalker<'a> {
    // Box<dyn> 抹掉 `filter_entry` 闭包的匿名类型；闭包零捕获、`WalkDir` 自持 root 路径 → 'static。
    inner: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    root: PathBuf,
    catalog: &'a CatalogSnapshot,
    cancel: &'a CancellationToken,
    /// quick 模式的目录级剪枝回调;None(默认全量)时行为与旧实现逐项一致。
    dir_pruner: Option<&'a dyn DirPruner>,
    /// 当前 DFS 路径上被剪枝的目录栈 `(depth, abs_dir)`,仅用于跳过其**直接文件**。
    pruned_dirs: Vec<(usize, PathBuf)>,
    errors: Vec<WalkError>,
    cancelled: bool,
}

impl<'a> MediaWalker<'a> {
    pub fn new(root: &Path, catalog: &'a CatalogSnapshot, cancel: &'a CancellationToken) -> Self {
        let inner = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_hidden(e));
        Self {
            inner: Box::new(inner),
            root: root.to_path_buf(),
            catalog,
            cancel,
            dir_pruner: None,
            pruned_dirs: Vec::new(),
            errors: Vec::new(),
            cancelled: false,
        }
    }

    /// 为 opt-in 快速扫描挂接目录级剪枝器。必须在遍历开始前调用。
    pub(crate) fn with_dir_pruner(mut self, pruner: Option<&'a dyn DirPruner>) -> Self {
        self.dir_pruner = pruner;
        self
    }

    /// 消费遍历器，产出本轮收尾结论（错误集 + 完整性 + 取消标志）。**遍历耗尽或取消后调用**。
    pub fn finish(self) -> WalkOutcome {
        let cancelled = self.cancelled;
        let complete = self.errors.is_empty() && !cancelled;
        if !complete && !cancelled {
            // 上报但不致命：调用方据 complete 决定「只展示、不差集删除」。
            tracing::warn!(
                "Walk incomplete: {} error(s) | 扫描不完整：{} 处遍历错误（缺失检测将跳过差集删除）",
                self.errors.len(),
                self.errors.len()
            );
        }
        WalkOutcome {
            errors: self.errors,
            complete,
            cancelled,
        }
    }
}

impl Iterator for MediaWalker<'_> {
    type Item = WalkedFile;

    fn next(&mut self) -> Option<WalkedFile> {
        if self.cancelled {
            return None;
        }
        loop {
            // 每步先查取消：及时停止深目录遍历（产出 None，置 cancelled → 不完整 → 不差集删除）。
            if self.cancel.is_cancelled() {
                self.cancelled = true;
                return None;
            }

            let entry = match self.inner.next()? {
                Ok(e) => e,
                Err(e) => {
                    // 遍历错误（权限/IO/symlink loop）：记下而非丢——seen 完整性依赖此。
                    self.errors.push(WalkError {
                        path: e
                            .path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                        reason: format!("traverse: {e}"),
                    });
                    continue;
                }
            };

            // 维护剪枝栈:当前条目深度 <= 栈顶深度 = 已离开那个目录,弹出。
            let depth = entry.depth();
            while self.pruned_dirs.last().is_some_and(|(d, _)| *d >= depth) {
                self.pruned_dirs.pop();
            }

            let path = entry.path();

            if entry.file_type().is_dir() {
                // 目录条目仍要下降:quick 剪枝只跳**直接文件**,不递归剪掉子目录。
                if let Some(pruner) = self.dir_pruner {
                    let rel_path = crate::utils::path::dir_rel_path(&self.root, path);
                    let rel_path_norm = crate::utils::path::normalize_db_path(&rel_path);
                    if pruner.should_prune_dir(path, &rel_path_norm) {
                        self.pruned_dirs.push((depth, path.to_path_buf()));
                    }
                }
                continue;
            }

            // quick 剪枝:若直接父目录在剪枝栈顶,该文件不 stat、不产出(per-file 成本最大头)。
            if let Some((pruned_depth, pruned_dir)) = self.pruned_dirs.last() {
                if *pruned_depth + 1 == depth
                    && path.parent().is_some_and(|p| p == pruned_dir.as_path())
                {
                    continue;
                }
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();

            let media_type = match classify_scanned_file(&ext, self.catalog) {
                Some(t) => t,
                None => continue, // 既非常见格式、也非 Catalog 已知 exotic 格式 — 正常跳过（非错误）
            };

            // 单次 symlink_metadata 取代「file_type() 预检 + metadata()」双调用(吸收对照线,
            // 2026-08-23):无 d_type 的文件系统上 walkdir 对 file_type() 回退 lstat,原文件分支
            // 是 lstat + stat 两次元数据调用。symlink_metadata **不跟随符号链接** → 符号链接跳过
            // 语义不变(is_symlink 先判),常规文件同一份元数据同时给出类型/size/mtime。
            // stat 失败只发生在已分类媒体文件上:计入 errors(否则差集会误删它),口径不变。
            let meta = match std::fs::symlink_metadata(path) {
                Ok(m) => m,
                Err(e) => {
                    self.errors.push(WalkError {
                        path: path.display().to_string(),
                        reason: format!("metadata: {e}"),
                    });
                    continue;
                }
            };
            let meta_ft = meta.file_type();
            if meta_ft.is_symlink() || !meta_ft.is_file() {
                continue;
            }

            let file_size = meta.len() as i64;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
            let file_mtime = modified
                .as_ref()
                .map(|d| d.as_secs().min(i64::MAX as u64) as i64)
                .unwrap_or(0);
            let file_mtime_ns = modified
                .as_ref()
                .and_then(|d| i64::try_from(d.as_nanos()).ok())
                .unwrap_or(0);

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();

            return Some(WalkedFile {
                abs_path: path.to_path_buf(),
                file_name,
                extension: ext,
                media_type,
                file_size,
                file_mtime,
                file_mtime_ns,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_common_first_then_catalog() {
        let catalog = CatalogSnapshot::builtin().unwrap();
        // 常见格式：走 common，不查 Catalog。
        assert_eq!(
            classify_scanned_file("jpg", &catalog),
            Some(MediaType::Image)
        );
        // psd 已移出 common → 由 Catalog 识别为 exotic image。
        assert_eq!(
            classify_scanned_file("psd", &catalog),
            Some(MediaType::Image)
        );
        // 既非 common 也非 Catalog 已知 → None（walker 跳过）。
        assert_eq!(classify_scanned_file("xyz", &catalog), None);
        // builtin offering(D-OCR-5,如 exotic-ocr)非文件格式 → None（不得判为媒体入库,
        // 防磁盘杂散同名扩展文件如 .ocr 被误收编）。
        assert_eq!(classify_scanned_file("ocr", &catalog), None);
    }

    #[test]
    fn empty_catalog_drops_psd() {
        // 无 Catalog（降级）时 psd 不再被任何表识别 → 不入库（而非走会失败的主解码器）。
        let empty = CatalogSnapshot::empty();
        assert_eq!(classify_scanned_file("psd", &empty), None);
        assert_eq!(classify_scanned_file("jpg", &empty), Some(MediaType::Image));
    }

    /// 流式拉取辅助：把 [`MediaWalker`] 迭代到底，返回（文件集, 收尾结论）。
    fn collect(
        root: &Path,
        catalog: &CatalogSnapshot,
        cancel: &CancellationToken,
    ) -> (Vec<WalkedFile>, WalkOutcome) {
        let mut w = MediaWalker::new(root, catalog, cancel);
        let mut files = Vec::new();
        for f in w.by_ref() {
            files.push(f);
        }
        (files, w.finish())
    }

    /// 可读目录：complete==true、无错误、仅识别媒体文件（未知扩展名正常跳过、不计错误）。
    #[test]
    fn walk_complete_on_readable_dir() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("scrollery_walk_ok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("a.jpg")).unwrap();
        f.write_all(b"x").unwrap();
        drop(f);
        std::fs::File::create(dir.join("note.xyz")).ok(); // 未知扩展名 → 正常跳过（非错误）

        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let (files, outcome) = collect(&dir, &catalog, &cancel);

        assert!(outcome.complete, "可读目录应完整（零遍历错误）");
        assert!(!outcome.cancelled);
        assert!(outcome.errors.is_empty(), "未知扩展名不应计入 errors");
        assert_eq!(files.len(), 1, "仅识别 a.jpg");
        assert_eq!(files[0].file_name, "a.jpg");
        let expected_mtime_ns = std::fs::metadata(dir.join("a.jpg"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
        assert_eq!(files[0].file_mtime_ns, expected_mtime_ns);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 不存在的根：WalkDir 首项即遍历错误 → complete==false、errors 非空、零文件。
    /// 锁住「不完整扫描 ≠ 删除」：下游差集据 complete 守门，绝不在此场景误删。
    #[test]
    fn walk_incomplete_on_unreadable_root() {
        let missing = std::env::temp_dir().join(format!(
            "scrollery_walk_missing_{}_nope",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);

        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let (files, outcome) = collect(&missing, &catalog, &cancel);

        assert!(!outcome.complete, "不存在的根应不完整（遍历错误）");
        assert!(!outcome.errors.is_empty(), "应记录遍历错误而非静默丢弃");
        assert!(files.is_empty());
    }

    /// quick 剪枝:剪掉目录的直接文件,但**不递归剪子目录**(T17b 逐目录语义)。
    #[test]
    fn dir_pruner_skips_direct_files_before_yield_but_still_descends() {
        use std::cell::RefCell;
        use std::io::Write;

        struct FakePruner<'a> {
            prune_dir_name: &'a str,
            calls: RefCell<Vec<std::path::PathBuf>>,
        }
        impl DirPruner for FakePruner<'_> {
            fn should_prune_dir(&self, abs_dir: &Path, _rel_path: &str) -> bool {
                self.calls.borrow_mut().push(abs_dir.to_path_buf());
                abs_dir.file_name().and_then(|n| n.to_str()) == Some(self.prune_dir_name)
            }
        }

        let dir = std::env::temp_dir().join(format!("scrollery_walk_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let skip = dir.join("skip");
        let skip_sub = skip.join("sub");
        let keep = dir.join("keep");
        std::fs::create_dir_all(&skip_sub).unwrap();
        std::fs::create_dir_all(&keep).unwrap();
        for f in [
            dir.join("root.jpg"),
            skip.join("a.jpg"),
            skip_sub.join("b.jpg"),
            keep.join("c.jpg"),
        ] {
            let mut h = std::fs::File::create(&f).unwrap();
            h.write_all(b"x").unwrap();
        }

        let pruner = FakePruner {
            prune_dir_name: "skip",
            calls: RefCell::new(Vec::new()),
        };
        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        let mut walker = MediaWalker::new(&dir, &catalog, &cancel).with_dir_pruner(Some(&pruner));
        let mut names = Vec::new();
        for f in walker.by_ref() {
            names.push(f.file_name);
        }
        let outcome = walker.finish();

        assert!(outcome.complete, "剪枝不得制造遍历错误");
        names.sort();
        assert_eq!(
            names,
            vec![
                "b.jpg".to_string(),
                "c.jpg".to_string(),
                "root.jpg".to_string()
            ],
            "skip 的直接文件 a.jpg 应在 stat 前跳过;skip/sub 与其它目录仍正常产出"
        );
        assert!(
            pruner
                .calls
                .borrow()
                .iter()
                .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("skip")),
            "剪枝器应被询问 skip 目录"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 取消令牌已触发 → 流式遍历立即停产、cancelled==true、complete==false（不完整不删）。
    #[test]
    fn walk_cancelled_is_incomplete() {
        use std::io::Write;
        let dir =
            std::env::temp_dir().join(format!("scrollery_walk_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("a.jpg")).unwrap();
        f.write_all(b"x").unwrap();
        drop(f);

        let catalog = CatalogSnapshot::builtin().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel(); // 开扫前即取消

        let (files, outcome) = collect(&dir, &catalog, &cancel);
        assert!(files.is_empty(), "取消后不产出文件");
        assert!(outcome.cancelled, "应标记 cancelled");
        assert!(!outcome.complete, "取消即不完整 → 下游不得差集删除");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
