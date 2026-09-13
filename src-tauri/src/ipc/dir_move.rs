//! 目录移动引擎：物理搬运 → 身份重写 → 阶段日志收尾，以及失败/重启后的幂等恢复。
//!
//! 背景（2026-09-12 审查 §7.1-A/B）：move_directory 的物理动作先于数据库事务，旧实现有三处缺陷：
//!   1. 跨卷（拷贝+删源）后只改 directories.root_id/rel_path 与 media_items.cache_key/thumb_path，
//!      没有重写 media_items.volume_id / volume_relative_path。源卷一离线，bulk_set_availability
//!      仍按旧卷把已搬到在线目标卷的条目标成 offline；扫描 upsert 的 COALESCE 只补 NULL，修不了既有错误值。
//!   2. cache_key 相同即 continue：跨根同相对路径时 cache_key 不变，而 thumb_status=3（直接使用
//!      源文件）的 thumb_path 存的是**绝对源路径**，必须随移动更新，卷定位列也必须照写。
//!   3. 物理成功、DB 失败只沿 ? 抛一条通用错误：用户看不到文件真实位置，也没有可重试的落点。
//!
//! ## 执行形状：短数据库阶段 + 锁外文件 IO
//!
//! 引擎只经 [MoveDb::with_conn] 触碰数据库，每次调用都是**短阶段**（插入日志行、事务化重写身份、
//! 推进阶段、读行），返回即释放 db_writer；所有文件 IO——搬运、整树校验、源残留清理、缓存文件
//! 重定位——都发生在这些阶段之外，因此不存在「持着 DB 锁等磁盘」的形态。
//!
//! ## 跨卷搬运的相位（凭据先落盘，再发布）
//!
//!   1. 独占创建暂存目录 → 复制并逐文件校验 → 得到「我们写出树」的内容凭据；
//!   2. **把凭据写进阶段日志**（短 DB 阶段）；
//!   3. rename 发布（暂存 → 目标）；
//!   4. 删源；
//!   5. 推进阶段为 published。
//!
//! 顺序不能调换：若先发布再落凭据，崩在 3 与 5 之间就会留下「源与目标都在、日志无凭据」的
//! 状态，恢复只能保守判为外部冲突，永远收不了尾。同卷 rename 路径没有「我们写出的树」（rename
//! 原子且不产生中间树），故其发布由「源已不在」这一物理事实自证。
//!
//! ## 三条硬不变量
//!
//!   - **目标不得覆盖**：目标物理存在（无论是否已索引）即拒绝，绝不合并不覆盖。
//!   - **源原件不丢**：跨卷先复制到我们自己的独占暂存目录（create_dir 独占创建 + 标记文件）并逐
//!     文件校验，再 rename 发布，发布成功后才删源；删源前必须证明源是目标内容的子集。
//!   - **清理只碰自己的东西**：暂存目录清理前必须验标记文件；源残留清理前必须逐文件证明一致。
//!
//! ## 恢复的证明规则（不允许认领不确定的目标，也不丢恢复线索）
//!
//!   - 源与目标**同时存在**：只有日志里的 payload_digest 与目标现算摘要一致才认领；对不上或没有
//!     凭据即保持冲突，不改库、不删源。
//!   - 目标是 rename 产物（无凭据）：只有「源已不在」这一种物理状态才认定发布完成。
//!   - 删源只在「源是目标内容的子集」时进行（逐文件比内容，尺寸相同不算数）。
//!   - 扫描根**行确实不存在**（Ok(None)）才作废日志；**查询失败**一律保留线索。
//!   - 两侧路径都判为「不在」时，只有两侧扫描根本身都可访问才敢认定「什么都没发生」；
//!     否则（常见：卷离线让 NotFound 伪装成不存在）保留日志。
//!   - 启动收尾只做可证的短收敛（存在性检查 + 索引重放，不删源、不做整树校验）。

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::queries as q;
use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::thumbnail::generator::THUMB_TIERS;
use crate::utils::hash::{cache_key_to_hex, compute_cache_key_with_mtime_ns};
use crate::utils::path::{path_depth, resolve_media_path};

/// 稳定码：物理搬运已发布、索引未更新（可重试）。
pub const CODE_DB_PENDING: &str = "move_db_pending";

/// 已发布、索引未更新的用户可见说明。
const MSG_INDEX_PENDING: &str = "文件已移动到新位置，索引尚未更新 | files moved, index not updated";
/// 尚未发布、连凭据都没写进去的说明（源与暂存都还在，重试即可）。
const MSG_NOT_STARTED: &str =
    "移动尚未开始（数据库暂不可用），可重试 | move not started, retryable";

/// 暂存目录的**兄弟**标记文件后缀：`{暂存目录名}.scrollery-staging`。
///
/// 用兄弟文件而非目录内文件：目录内容必须只是用户数据（内容摘要与发布后的目标树逐字节可比），
/// 我们的记账文件放在紧邻位置。清理与复用只认这个标记 —— 目标卷上同名的既有目录是用户数据。
const STAGING_MARKER_SUFFIX: &str = ".scrollery-staging";

/// 复制/校验的读缓冲（也是摘要计算的读块）。
const CHUNK: usize = 64 * 1024;

/// 进程内暂存目录名去重序号（同进程两次移动生成不同路径）。
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

// ════════════════════════════════════════════════════════════════════════════
// 数据库触点：短阶段，不与文件 IO 交叠
// ════════════════════════════════════════════════════════════════════════════

/// 引擎与数据库之间的唯一接口。
///
/// 用具体枚举而非 trait：方法带泛型（返回任意 T）就不是 object-safe，枚举让同一份编排同时服务
/// 「IPC 持 AppState」与「db/boot 启动收尾持裸 writer」两种调用方，且没有动态分发的额外层级。
/// 两种变体的实现都只在**一次短调用**内取连接并释放——闭包返回后调用方立刻回到文件 IO，因此
/// 「持 DB 锁做文件 IO」在本模块的编排里不可能发生。
#[derive(Clone, Copy)]
pub(crate) enum MoveDb<'a> {
    State(&'a AppState),
    Writer(&'a Mutex<Connection>),
}

impl MoveDb<'_> {
    pub(crate) fn with_conn<T>(
        &self,
        f: &mut dyn FnMut(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        match self {
            MoveDb::State(state) => {
                let mut conn = state.db_writer.lock().unwrap_or_else(|e| e.into_inner());
                f(&mut conn)
            }
            MoveDb::Writer(writer) => {
                let mut conn = writer.lock().unwrap_or_else(|e| e.into_inner());
                f(&mut conn)
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 计划与校验
// ════════════════════════════════════════════════════════════════════════════

/// 一次目录移动的完整计划（创建后不再改变；恢复路径从阶段日志重建）。
#[derive(Debug, Clone)]
pub(crate) struct MovePlan {
    pub source_dir_id: i64,
    pub name: String,
    /// 源目录当前 rel_path（子树前缀）。
    pub old_rel: String,
    /// 目标 rel_path（= 目标目录 rel_path + 源目录名）。
    pub new_rel: String,
    pub source_root_id: i64,
    pub target_root_id: i64,
    /// 目标父目录行 id。**不允许**为 None：落到扫描根下时它是该根 rel_path='' 的目录行，
    /// 写成 NULL 会把普通目录变成树里的伪根节点（审查 §7.1-B 复核 7）。
    pub target_parent_id: i64,
    /// 源/目标扫描根路径，**原样**（DB 口径：写进 thumb_path / 卷定位的值必须与扫描器一致，
    /// 不能带 canonicalize 在 Windows 上加的 \\?\ 前缀）。
    pub source_root_path: String,
    pub target_root_path: String,
    /// 目标根的卷绑定（NULL = 本地固定路径/未识别卷）。
    pub target_volume_id: Option<i64>,
    pub target_volume_subpath: Option<String>,
    /// 源目录的规范化绝对路径（必须在源根内）。
    pub src_abs: PathBuf,
    /// 目标的绝对路径（父级必须是目标根内的真实位置）。
    pub dst_abs: PathBuf,
    /// 跨卷路径的独占暂存目录（与目标同卷，rename 发布用）。
    pub staging_abs: PathBuf,
}

/// 我们实际写出的一棵树的内容凭据。
#[derive(Debug, Clone)]
pub(crate) struct Payload {
    pub files: usize,
    pub digest: String,
}

/// 数据库重写结果。
#[derive(Debug, Default, Clone)]
pub(crate) struct DbOutcome {
    pub affected_dirs: usize,
    pub affected_media: usize,
    /// 需要重定位的缓存文件 (旧 cache_key, 新 cache_key)。
    pub cache_renames: Vec<(i64, i64)>,
}

/// 一次移动的执行结果。
#[derive(Debug, Clone)]
pub(crate) struct MoveDirOutcome {
    pub affected_dirs: usize,
    pub affected_media: usize,
    /// 源目录残留的绝对路径（跨卷删源失败）；索引已更新，残留待清理。
    pub source_leftover: Option<String>,
    /// 仍需收尾的阶段日志 id（源残留）。
    pub recovery_id: Option<i64>,
}

/// 启动/重试收尾的报告（一条未完成或刚收尾的移动）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRecoveryReport {
    pub recovery_id: i64,
    /// 收尾后的阶段：intent / published / source_leftover（已收尾的行不再出现）。
    pub stage: String,
    pub source_name: String,
    pub source_abs_path: String,
    pub target_abs_path: String,
    pub target_rel_path: String,
    pub target_root_id: i64,
    /// true = 仍需重试（物理未搬完 / 目标未能证明 / 源残留未清 / 盘不在）。
    pub needs_retry: bool,
    /// 稳定短标签，供日志与 UI 分流：completed / absent / target_root_missing / target_missing /
    /// target_unverified / target_conflict / target_unresolved / source_leftover /
    /// source_changed / staging_busy / payload_pending / published / unknown。
    pub detail: &'static str,
}

/// 计划过期（等待根闸门期间，另一次移动/删根/重链接改动了源或目标）。
///
/// 复用 InvalidMove 而非新增稳定码：这是「重试一次即可」的瞬时状态，前端已有的失败提示就是
/// 正确处置；新码只会多一条没有分流动作的分支。
fn stale_plan() -> AppError {
    AppError::InvalidMove(
        "目录已被其他操作改动，请重试 | the folder changed, please retry the move".into(),
    )
}

/// 把源目录移动到目标目录下：读元数据 + 全部校验 + 计算规范化路径。
///
/// 校验口径与改动前一致（不可移扫描根 / 不可入自身 / 不可入自身子树 / 目标下同名已索引目录），
/// 另加**路径安全**校验（审查复核 4）：扫描根 canonicalize 后必须仍是目录，源目录必须
/// canonicalize 到源根内（拦符号链接逃逸），目标路径的每一段都必须是合法单段名，且其最近的
/// 已存在祖先必须 canonicalize 到目标根内。
///
/// 注意：本函数在**取根闸门之前**执行，结果可能在排队期间过期，调用方必须在闸门内调用
/// [verify_plan_current] 复核后才动文件系统。
pub(crate) fn load_plan(
    conn: &Connection,
    source_dir_id: i64,
    target_dir_id: i64,
) -> Result<MovePlan> {
    let source = q::get_directory(conn, source_dir_id)?;
    let target = q::get_directory(conn, target_dir_id)?;

    if source_dir_id == target_dir_id {
        return Err(AppError::InvalidMove(
            "不能移动到自身 | cannot move into itself".into(),
        ));
    }
    if source.parent_id.is_none() {
        return Err(AppError::InvalidMove(
            "不能移动扫描根目录 | cannot move a scan root".into(),
        ));
    }
    if source.parent_id == Some(target_dir_id) {
        return Err(AppError::InvalidMove(
            "已位于目标目录中 | already inside the target".into(),
        ));
    }
    // 目标不能位于源子树内（会成环）。
    let ancestors = q::get_directory_ancestors(conn, target_dir_id)?;
    if ancestors.contains(&source_dir_id) {
        return Err(AppError::InvalidMove(
            "不能移动到自身的子目录 | cannot move into own descendant".into(),
        ));
    }
    if q::dir_has_child_named(conn, target_dir_id, &source.name)? {
        return Err(AppError::DirectoryExists(source.name.clone()));
    }

    let source_root = q::find_scan_root_ref(conn, source.root_id)?
        .ok_or(AppError::ScanRootNotFound(source.root_id))?;
    let target_root = q::find_scan_root_ref(conn, target.root_id)?
        .ok_or(AppError::ScanRootNotFound(target.root_id))?;
    let source_root_canon = canonical_dir(&source_root.path)?;
    let target_root_canon = canonical_dir(&target_root.path)?;

    let new_rel = join_rel(&target.rel_path, &source.name);
    let src_abs = resolve_existing_in_root(&source_root_canon, &source.rel_path)?;
    let dst_abs = resolve_creatable_in_root(&target_root_canon, &rel_segments(&new_rel)?)?;

    Ok(MovePlan {
        source_dir_id,
        name: source.name.clone(),
        old_rel: source.rel_path.clone(),
        new_rel,
        source_root_id: source.root_id,
        target_root_id: target.root_id,
        target_parent_id: target_dir_id,
        source_root_path: source_root.path,
        target_root_path: target_root.path,
        target_volume_id: target_root.volume_id,
        target_volume_subpath: target_root.volume_subpath,
        staging_abs: staging_path_for(&dst_abs, &source.name),
        src_abs,
        dst_abs,
    })
}

/// 取闸后的计划复核：源/目标身份与根必须仍然与计划一致。
///
/// 等待闸门期间另一次移动、删根或重链接都可能让计划过期——此处的每一条都与物理动作直接相关：
///   - 源目录必须仍是源根下的同一行（未被别人搬走/删除、不再是顶层根行）；
///   - 目标父目录必须还是目标根下同一行；
///   - 目标 rel_path 不能已被占用（索引层面）。
///
/// 不一致一律返回 [stale_plan]，让调用方重试，绝不用过期计划去动磁盘。
pub(crate) fn verify_plan_current(conn: &Connection, plan: &MovePlan) -> Result<()> {
    let source = q::get_directory(conn, plan.source_dir_id).map_err(|_| stale_plan())?;
    if source.root_id != plan.source_root_id || source.rel_path != plan.old_rel {
        return Err(stale_plan());
    }
    if source.parent_id.is_none() {
        return Err(stale_plan());
    }
    let target = q::get_directory(conn, plan.target_parent_id).map_err(|_| stale_plan())?;
    if target.root_id != plan.target_root_id {
        return Err(stale_plan());
    }
    let parent_rel = parent_rel_of(&plan.new_rel);
    if q::find_directory_id(conn, plan.target_root_id, &parent_rel)? != Some(plan.target_parent_id)
    {
        return Err(stale_plan());
    }
    if q::dir_has_child_named(conn, plan.target_parent_id, &plan.name)? {
        return Err(AppError::DirectoryExists(plan.name.clone()));
    }
    Ok(())
}

/// 目录名必须是单段名：非空、不含分隔符 / 冒号（Windows 备用数据流与盘符）、不是 . 或 ..。
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
    {
        return Err(AppError::InvalidMove(format!(
            "目录名不合法，拒绝移动 | invalid directory name: {name}"
        )));
    }
    Ok(())
}

/// 校验并返回目标 rel_path 的分段（每段都是合法单段名）。
fn rel_segments(new_rel: &str) -> Result<Vec<String>> {
    let mut segs = Vec::new();
    for seg in new_rel.split('/') {
        if seg.is_empty() {
            continue;
        }
        validate_name(seg)?;
        segs.push(seg.to_string());
    }
    if segs.is_empty() {
        return Err(AppError::InvalidMove(
            "目标相对路径为空 | empty target path".into(),
        ));
    }
    Ok(segs)
}

fn join_rel(parent_rel: &str, name: &str) -> String {
    if parent_rel.is_empty() {
        name.to_string()
    } else {
        format!("{parent_rel}/{name}")
    }
}

/// 目标 rel_path 的父段（空 = 扫描根下）。
fn parent_rel_of(target_rel: &str) -> String {
    match target_rel.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// rel_path 的 (末段名, 父段)。
fn split_rel(rel: &str) -> (String, String) {
    match rel.rsplit_once('/') {
        Some((parent, name)) => (name.to_string(), parent.to_string()),
        None => (rel.to_string(), String::new()),
    }
}

/// canonicalize 一个必须存在的目录（扫描根）。
fn canonical_dir(path: &str) -> Result<String> {
    let p = std::fs::canonicalize(path).map_err(|_| {
        AppError::PathResolution("扫描根不存在或不可访问 | scan root missing".into())
    })?;
    if !p.is_dir() {
        return Err(AppError::PathResolution(
            "扫描根不是目录 | scan root is not a directory".into(),
        ));
    }
    Ok(p.to_string_lossy().to_string())
}

/// 源目录：逐段校验 rel_path 后 canonicalize，并要求仍落在（已 canonicalize 的）源根内。
/// 跟随符号链接再判边界 —— 指向根外的链接一律拒绝，避免从根外搬走/删除目录。
fn resolve_existing_in_root(root_canon: &str, rel_path: &str) -> Result<PathBuf> {
    let root = Path::new(root_canon);
    let target = crate::utils::path::resolve_within_root(root_canon, rel_path)?;
    if !target.starts_with(root) {
        return Err(AppError::PathResolution(
            "源目录越出扫描根 | source escapes scan root".into(),
        ));
    }
    Ok(target)
}

/// 目标目录：允许尚不存在，但其**最近的已存在祖先**必须 canonicalize 到目标根内，且路径的每一段
/// 都是合法单段名。这样既拦住符号链接逃逸，也不误拒绝正常的「新建目录」。
fn resolve_creatable_in_root(root_canon: &str, segs: &[String]) -> Result<PathBuf> {
    let root = Path::new(root_canon);
    let mut existing = root.to_path_buf();
    let mut pending: Vec<&str> = Vec::new();
    for (idx, seg) in segs.iter().enumerate() {
        let candidate = existing.join(seg);
        match path_state(&candidate) {
            PathState::Exists => existing = candidate,
            _ => {
                pending.extend(segs[idx..].iter().map(|s| s.as_str()));
                break;
            }
        }
    }
    let canon_existing = std::fs::canonicalize(&existing).map_err(|_| {
        AppError::PathResolution("目标路径不可访问 | target path not accessible".into())
    })?;
    if !canon_existing.starts_with(root) {
        return Err(AppError::PathResolution(
            "目标路径越出扫描根 | target escapes scan root".into(),
        ));
    }
    let mut out = canon_existing;
    for seg in pending {
        out.push(seg);
    }
    Ok(out)
}

/// 目标同卷的独占暂存目录：{目标父目录}/.scrollery-move-{名}-{pid}-{序号}。
fn staging_path_for(dst: &Path, name: &str) -> PathBuf {
    let token = format!(
        "{}-{}",
        std::process::id(),
        STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let file_name = format!(".scrollery-move-{name}-{token}");
    match dst.parent() {
        Some(parent) => parent.join(file_name),
        None => PathBuf::from(file_name),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 物理搬运
// ════════════════════════════════════════════════════════════════════════════

/// 路径三态：区分「不在」与「判不了」（盘不在/无权限）。判不了时按存在处理——宁可不覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathState {
    Exists,
    Absent,
    Unknown,
}

fn path_state(path: &Path) -> PathState {
    match std::fs::symlink_metadata(path) {
        Ok(_) => PathState::Exists,
        Err(e) if e.kind() == ErrorKind::NotFound => PathState::Absent,
        Err(_) => PathState::Unknown,
    }
}

/// 物理搬运的第一段：能 rename 就 rename（原子、无中间树）；rename 走不通就只回报「需要暂存」。
///
/// 复制与发布都不在本函数内：跨卷路径必须先把内容凭据写进阶段日志（[stage_and_persist]）才能
/// 发布（[publish_staging]），否则崩在发布与凭据之间会让双端存在的状态永远收不了尾。
pub(crate) enum PhysicalPhase {
    /// 同卷 rename 已落位（源已不在，原子）。
    Renamed,
    /// rename 走不通（跨卷或其它）：调用方需复制到独占暂存并持久化凭据后再发布。
    NeedsStaging,
}

pub(crate) fn begin_physical_move(plan: &MovePlan) -> Result<PhysicalPhase> {
    if let Some(parent) = plan.dst_abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::MoveFile(e.to_string()))?;
    }
    match path_state(&plan.dst_abs) {
        // 目标物理存在（未索引也在此拦下）：不合并、不覆盖。
        PathState::Exists | PathState::Unknown => {
            return Err(AppError::DirectoryExists(plan.name.clone()));
        }
        PathState::Absent => {}
    }
    match std::fs::rename(&plan.src_abs, &plan.dst_abs) {
        Ok(()) => Ok(PhysicalPhase::Renamed),
        Err(_) => Ok(PhysicalPhase::NeedsStaging),
    }
}

/// 复制到独占暂存并逐文件校验（不发布）。失败只留下源完整 + 我们自己的暂存被清掉。
pub(crate) fn prepare_staging(plan: &MovePlan) -> Result<Payload> {
    match path_state(&plan.src_abs) {
        PathState::Absent => {
            return Err(AppError::MoveFile(format!(
                "源目录不存在: {}",
                plan.src_abs.display()
            )))
        }
        PathState::Unknown => {
            return Err(AppError::MoveFile(format!(
                "源目录无法访问（卷离线或权限不足）: {}",
                plan.src_abs.display()
            )))
        }
        PathState::Exists => {}
    }
    if path_state(&plan.dst_abs) != PathState::Absent {
        return Err(AppError::DirectoryExists(plan.name.clone()));
    }
    // 暂存的父目录（目标父目录）可能整条还不存在：先建出来（只影响目标的上级目录，不产生目标本身）。
    if let Some(parent) = plan.staging_abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::CopyFile(e.to_string()))?;
    }
    // 独占创建：create_dir 在已存在时失败（TOCTOU 夹缝里也不会复用/覆盖别人的东西）。
    if let Err(e) = std::fs::create_dir(&plan.staging_abs) {
        return Err(AppError::MoveFile(format!(
            "暂存目录创建失败（可能已被占用）: {e} | staging create failed"
        )));
    }
    if let Err(e) = mark_staging(&plan.staging_abs) {
        cleanup_own_staging(&plan.staging_abs);
        return Err(e);
    }
    match copy_tree_verified(&plan.src_abs, &plan.staging_abs) {
        Ok(payload) => Ok(payload),
        Err(e) => {
            cleanup_own_staging(&plan.staging_abs);
            Err(e)
        }
    }
}

/// 上一轮留下的暂存目录能不能直接复用：必须是我们自己的（标记文件在）且内容与已落盘凭据一致。
/// 复用能省掉一次整树拷贝；不一致就清掉重做。
pub(crate) fn staging_reusable(plan: &MovePlan, expected_digest: &str) -> bool {
    if path_state(&staging_marker_path(&plan.staging_abs)) != PathState::Exists {
        return false;
    }
    match tree_payload_digest(&plan.staging_abs) {
        Ok(actual) if actual.digest == expected_digest => true,
        _ => {
            cleanup_own_staging(&plan.staging_abs);
            false
        }
    }
}

/// 发布暂存目录：rename 落位 → 删源。返回源是否已删除（false = 残留，由重试清理）。
///
/// 调用前必须已把内容凭据写进阶段日志（本函数一旦 rename 成功，日志就是唯一的「目标属于我们」证明）。
pub(crate) fn publish_staging(plan: &MovePlan) -> Result<bool> {
    if let Err(e) = std::fs::rename(&plan.staging_abs, &plan.dst_abs) {
        cleanup_own_staging(&plan.staging_abs);
        return Err(AppError::MoveFile(format!(
            "发布目标目录失败: {e} | publish failed"
        )));
    }
    // 目标已落位：记账用的兄弟标记文件到这里失去意义（不删会留在目标父目录里）。
    drop_staging_marker(&plan.staging_abs);
    match std::fs::remove_dir_all(&plan.src_abs) {
        Ok(()) => Ok(true),
        Err(e) => {
            tracing::warn!(
                "dir_move: 删源失败，留待重试 source={} error={e}",
                plan.src_abs.display()
            );
            Ok(false)
        }
    }
}

/// 复制到我们自己的暂存目录，并**把内容凭据持久化**——这是发布的硬前置。
///
/// 凭据写不进数据库时绝不发布（否则崩在「发布后、删源前」就没有任何证据能证明目标属于本次移动，
/// 恢复只能保守判为外部冲突，永远收不了尾）。此时源完整、暂存目录仍在（我们自己的、带标记），
/// 阶段日志保留待重试：用户重试会重新校验并重新复制，不会认领错东西。
pub(crate) fn stage_and_persist(
    db: MoveDb<'_>,
    plan: &MovePlan,
    journal_id: i64,
) -> Result<Payload> {
    let payload = prepare_staging(plan)?;
    let persisted = db.with_conn(&mut |conn| {
        q::set_payload(
            conn,
            journal_id,
            Some(&payload.digest),
            Some(payload.files as i64),
        )
    });
    match persisted {
        Ok(()) => Ok(payload),
        Err(e) => {
            tracing::error!(
                "dir_move: 内容凭据持久化失败，拒绝发布 journal={journal_id} staging={} error={e}",
                plan.staging_abs.display()
            );
            Err(AppError::MoveRecovery {
                code: CODE_DB_PENDING,
                message: MSG_NOT_STARTED.into(),
                recovery_id: journal_id,
                target_abs_path: plan.dst_abs.to_string_lossy().to_string(),
            })
        }
    }
}

/// 复制一棵目录树到目标位置：独占暂存 → 复制校验 → rename 发布。返回落盘的普通文件数。
///
/// 目标路径已存在（无论是否已索引）一律拒绝，不合并；中途失败只清我们自己的暂存目录，目标位置
/// 不会出现半棵树的中间态。入库由调用方随后触发重扫（结果带 needs_rescan）。
pub(crate) fn copy_tree_published(src: &Path, dst: &Path, name: &str) -> Result<usize> {
    match path_state(src) {
        PathState::Exists => {}
        _ => {
            return Err(AppError::CopyFile(format!(
                "源目录不存在或无法访问: {}",
                src.display()
            )))
        }
    }
    if path_state(dst) != PathState::Absent {
        return Err(AppError::DirectoryExists(name.to_string()));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::CopyFile(e.to_string()))?;
    }
    let staging = staging_path_for(dst, name);
    if let Err(e) = std::fs::create_dir(&staging) {
        return Err(AppError::CopyFile(format!(
            "暂存目录创建失败（可能已被占用）: {e} | staging create failed"
        )));
    }
    if let Err(e) = mark_staging(&staging) {
        cleanup_own_staging(&staging);
        return Err(e);
    }
    let payload = match copy_tree_verified(src, &staging) {
        Ok(p) => p,
        Err(e) => {
            cleanup_own_staging(&staging);
            return Err(e);
        }
    };
    if let Err(e) = std::fs::rename(&staging, dst) {
        cleanup_own_staging(&staging);
        return Err(AppError::CopyFile(format!(
            "发布目标目录失败: {e} | publish failed"
        )));
    }
    drop_staging_marker(&staging);
    Ok(payload.files)
}

/// 暂存目录打标记（清理前置条件）。
fn mark_staging(staging: &Path) -> Result<()> {
    std::fs::write(staging_marker_path(staging), b"scrollery move staging")
        .map_err(|e| AppError::CopyFile(e.to_string()))
}

/// 暂存目录的兄弟标记文件路径。
pub(crate) fn staging_marker_path(staging: &Path) -> PathBuf {
    let mut name = staging
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push_str(STAGING_MARKER_SUFFIX);
    staging
        .parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// 丢弃我们自己的标记文件（发布成功、或暂存被别的路径用掉时）。
fn drop_staging_marker(staging: &Path) {
    let _ = std::fs::remove_file(staging_marker_path(staging));
}

/// 递归复制并逐文件校验，返回内容凭据（文件数 / 总字节 / 摘要）。
/// 一边读一边写一边算摘要：不额外多读一遍磁盘；写入字节数不等于源长度即视为不完整。
fn copy_tree_verified(src: &Path, dst: &Path) -> Result<Payload> {
    let mut acc = TreeAcc::default();
    copy_into(src, src, dst, &mut acc)?;
    Ok(Payload {
        files: acc.files,
        digest: acc.finish(),
    })
}

fn copy_into(root: &Path, dir: &Path, dst: &Path, acc: &mut TreeAcc) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| AppError::CopyFile(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::CopyFile(e.to_string()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .map_err(|e| AppError::CopyFile(e.to_string()))?;
        if ft.is_dir() {
            std::fs::create_dir_all(&to).map_err(|e| AppError::CopyFile(e.to_string()))?;
            copy_into(root, &from, &to, acc)?;
        } else {
            copy_file_verified(root, &from, &to, acc)?;
        }
    }
    Ok(())
}

fn copy_file_verified(root: &Path, from: &Path, to: &Path, acc: &mut TreeAcc) -> Result<()> {
    let rel = rel_key(root, from);
    let mut input = std::fs::File::open(from)
        .map_err(|e| AppError::CopyFile(format!("读取源文件失败({}): {e}", from.display())))?;
    let mut output = std::fs::File::create(to)
        .map_err(|e| AppError::CopyFile(format!("创建目标文件失败({}): {e}", to.display())))?;
    let mut hasher = Sha256::new();
    hasher.update(rel.as_bytes());
    let mut buf = vec![0u8; CHUNK];
    let mut written = 0u64;
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| AppError::CopyFile(e.to_string()))?;
        if n == 0 {
            break;
        }
        output
            .write_all(&buf[..n])
            .map_err(|e| AppError::CopyFile(e.to_string()))?;
        hasher.update(&buf[..n]);
        written += n as u64;
    }
    output
        .flush()
        .map_err(|e| AppError::CopyFile(e.to_string()))?;
    drop(output);
    let src_len = std::fs::metadata(from)
        .map(|m| m.len())
        .map_err(|e| AppError::CopyFile(e.to_string()))?;
    if written != src_len {
        return Err(AppError::CopyFile(format!(
            "复制不完整（{} != {}）: {}",
            written,
            src_len,
            from.display()
        )));
    }
    acc.add(&rel, written, hasher.finalize().into());
    Ok(())
}

/// 只删**我们**创建的暂存目录：必须是目录且带兄弟标记文件。不匹配一律不动。
pub(crate) fn cleanup_own_staging(staging: &Path) {
    match std::fs::symlink_metadata(staging) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => return, // 同名文件不是我们的产物
        Err(_) => return,
    }
    if path_state(&staging_marker_path(staging)) != PathState::Exists {
        tracing::warn!(
            "dir_move: 暂存目录缺标记，不清理 staging={}",
            staging.display()
        );
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(staging) {
        tracing::warn!(
            "dir_move: 暂存目录清理失败 staging={} error={e}",
            staging.display()
        );
        return;
    }
    drop_staging_marker(staging);
}

// ════════════════════════════════════════════════════════════════════════════
// 内容凭据与比对（整树读取，只在显式重试路径上使用）
// ════════════════════════════════════════════════════════════════════════════

/// 树摘要累加器：与遍历顺序无关（各文件 SHA-256 的组合 + 文件数 + 总字节）。
///
/// 顺序无关是必须的：复制时的遍历顺序与恢复时由文件系统给出的顺序可能不同。组合方式用逐字节异或
/// 累加，另把文件数与字节数揉进最终摘要，使「少一个文件」「换个同名同长文件」这类意外差异必然
/// 改变摘要。用途是发现**意外不一致**，不是对抗性构造的密码学保证。
#[derive(Default)]
struct TreeAcc {
    files: usize,
    bytes: u64,
    acc: [u8; 32],
}

impl TreeAcc {
    fn add(&mut self, _rel: &str, len: u64, digest: [u8; 32]) {
        self.files += 1;
        self.bytes += len;
        for (slot, byte) in self.acc.iter_mut().zip(digest.iter()) {
            *slot ^= *byte;
        }
    }

    fn finish(self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.files.to_le_bytes());
        hasher.update(self.bytes.to_le_bytes());
        hasher.update(self.acc);
        to_hex(&hasher.finalize().into())
    }
}

/// 相对路径键（正斜杠，含文件名）——并入每个文件的摘要输入，使改名可被检出。
fn rel_key(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn to_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 单文件内容摘要（含相对路径与长度）。
fn file_digest(root: &Path, file: &Path) -> Result<([u8; 32], u64)> {
    let mut input = std::fs::File::open(file).map_err(|e| AppError::MoveFile(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(rel_key(root, file).as_bytes());
    let mut buf = vec![0u8; CHUNK];
    let mut len = 0u64;
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| AppError::MoveFile(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        len += n as u64;
    }
    Ok((hasher.finalize().into(), len))
}

/// 整树内容凭据（读取全部内容）。与 [copy_tree_verified] 对同一棵树算出同一个值。
pub(crate) fn tree_payload_digest(root: &Path) -> Result<Payload> {
    let mut acc = TreeAcc::default();
    digest_into(root, root, &mut acc)?;
    Ok(Payload {
        files: acc.files,
        digest: acc.finish(),
    })
}

fn digest_into(root: &Path, dir: &Path, acc: &mut TreeAcc) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| AppError::MoveFile(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::MoveFile(e.to_string()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| AppError::MoveFile(e.to_string()))?;
        if ft.is_dir() {
            digest_into(root, &path, acc)?;
        } else {
            let (digest, len) = file_digest(root, &path)?;
            acc.add(&rel_key(root, &path), len, digest);
        }
    }
    Ok(())
}

/// 源是不是目标内容的**子集**：源里每个目录在目标里是目录，每个文件在目标里存在且内容一致。
///
/// 这是删源的唯一依据（审查复核 2）：尺寸一致还不够（同尺寸改写发现不了），必须比内容。源里多出的
/// 文件不参与判定——那些是用户后来放进去的东西，保留即可，只要源里**没有**目标缺失或不同的条目，
/// 删掉源就不会丢掉任何尚未存在于目标的内容。
fn source_subset_of_target(src: &Path, dst: &Path) -> Result<bool> {
    let mut subset = true;
    walk_subset(src, src, dst, &mut subset)?;
    Ok(subset)
}

fn walk_subset(root: &Path, dir: &Path, dst: &Path, subset: &mut bool) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| AppError::MoveFile(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::MoveFile(e.to_string()))?;
        let from = entry.path();
        let rel = rel_key(root, &from);
        let mut to = dst.to_path_buf();
        for seg in rel.split('/') {
            to.push(seg);
        }
        let ft = entry
            .file_type()
            .map_err(|e| AppError::MoveFile(e.to_string()))?;
        if ft.is_dir() {
            match path_state(&to) {
                PathState::Exists if to.is_dir() => walk_subset(root, &from, dst, subset)?,
                _ => *subset = false,
            }
        } else {
            match path_state(&to) {
                PathState::Exists => {
                    let (src_digest, src_len) = file_digest(root, &from)?;
                    let (dst_digest, dst_len) = file_digest(dst, &to)?;
                    if src_len != dst_len || src_digest != dst_digest {
                        *subset = false;
                    }
                }
                _ => *subset = false,
            }
        }
        if !*subset {
            return Ok(());
        }
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════════
// 数据库身份重写
// ════════════════════════════════════════════════════════════════════════════

/// 重写子树：目录 rel_path/root_id/depth/父指针 + 媒体 cache_key/thumb_path/卷定位。
///
/// 保留 item id / 收藏 / 评分 / AI 嵌入（均按 id 关联，本函数不碰）。幂等：重复执行写回同样的值
/// （这是「发布已落盘、DB 未更新」的重放依据），故不得再用「cache_key 未变就跳过」的短路——
/// 跨根同相对路径时 cache_key 不变，而绝对源路径缩略图与卷身份仍必须更新（审查 §7.1 缺陷 2）。
pub(crate) fn apply_move_db(conn: &mut Connection, plan: &MovePlan) -> Result<DbOutcome> {
    let tx = conn.transaction().map_err(AppError::from)?;

    let dirs = q::get_directory_subtree(&tx, plan.source_dir_id)?;
    let mut new_rel_by_dir: HashMap<i64, String> = HashMap::with_capacity(dirs.len());
    for d in &dirs {
        let new_rel = remap_rel(&d.rel_path, &plan.old_rel, &plan.new_rel);
        let new_depth = path_depth(&new_rel);
        // tree_sort_key(方案 B):唯一运行时改 rel_path 的入口,子树每目录 rel_path 变 → 键须随之
        // 重算并同事务写回,否则移动后 folder 目录序读到陈旧键(DEFAULT X'' 会静默错序)。
        let new_key = crate::utils::path::encode_tree_sort_key(&new_rel);
        // 首行且尚未应用时写父指针；重放时父指针已就位（再写会把已完成的移动改回去）。
        let first_apply = d.rel_path == plan.old_rel;
        if d.id == plan.source_dir_id && first_apply {
            tx.execute(
                "UPDATE directories SET rel_path=?1, depth=?2, root_id=?3, parent_id=?4, tree_sort_key=?5 WHERE id=?6",
                rusqlite::params![
                    new_rel,
                    new_depth,
                    plan.target_root_id,
                    plan.target_parent_id,
                    new_key,
                    d.id
                ],
            )
            .map_err(AppError::Db)?;
        } else {
            tx.execute(
                "UPDATE directories SET rel_path=?1, depth=?2, root_id=?3, tree_sort_key=?4 WHERE id=?5",
                rusqlite::params![new_rel, new_depth, plan.target_root_id, new_key, d.id],
            )
            .map_err(AppError::Db)?;
        }
        new_rel_by_dir.insert(d.id, new_rel);
    }

    let media = q::get_media_in_subtree(&tx, plan.source_dir_id)?;
    let mut outcome = DbOutcome {
        affected_dirs: dirs.len(),
        affected_media: media.len(),
        cache_renames: Vec::new(),
    };
    for m in &media {
        let dir_rel = new_rel_by_dir
            .get(&m.directory_id)
            .cloned()
            .unwrap_or_default();
        let new_key = compute_cache_key_with_mtime_ns(
            &dir_rel,
            &m.file_name,
            m.file_mtime,
            m.file_mtime_ns.unwrap_or(0),
        );
        // thumb_status == 3 (source-direct): thumb_path is the ABSOLUTE source path, which moved
        // with the folder → recompute it. Otherwise it's the relative cache path
        // "{size}/{prefix}/{hex}.webp" keyed by cache_key → remap the hex.
        // thumb_status == 3（直接使用源文件）：thumb_path 是绝对源路径，随文件夹一起移动 →
        // 重新计算。否则它是按 cache_key 命名的相对缓存路径 → 重映射 hex。
        let new_thumb = match (m.thumb_status, m.thumb_path.as_deref()) {
            (3, Some(_)) => Some(resolve_media_path(
                &plan.target_root_path,
                &dir_rel,
                &m.file_name,
            )),
            (_, Some(p)) if new_key != m.cache_key => Some(remap_thumb_path(p, new_key)),
            (_, Some(p)) => Some(p.to_string()),
            (_, None) => None,
        };
        // 卷定位（审查 §7.1-A）：跨根移动后必须重绑目标卷 + 目标卷内相对路径，否则源卷离线会按
        // 旧 volume_id 把已经在新卷上的条目误标 offline（COALESCE 修不了既有非空值）。
        let volume_rel = volume_relative_path(
            plan.target_volume_id,
            plan.target_volume_subpath.as_deref(),
            &dir_rel,
            &m.file_name,
        );
        tx.execute(
            "UPDATE media_items SET cache_key=?1, thumb_path=?2, volume_id=?3,
                    volume_relative_path=?4, availability='online',
                    updated_at=strftime('%s','now')
             WHERE id=?5",
            rusqlite::params![new_key, new_thumb, plan.target_volume_id, volume_rel, m.id],
        )
        .map_err(AppError::Db)?;
        if new_key != m.cache_key {
            outcome.cache_renames.push((m.cache_key, new_key));
        }
    }

    tx.commit().map_err(AppError::Db)?;
    Ok(outcome)
}

/// 卷内相对路径（正斜杠，卷根起）。目标根未绑定卷 → NULL（语义同扫描：无卷身份的项不参与
/// 「卷根重挂载重链接」定位）。
fn volume_relative_path(
    volume_id: Option<i64>,
    volume_subpath: Option<&str>,
    dir_rel: &str,
    file_name: &str,
) -> Option<String> {
    volume_id?;
    let mut parts: Vec<&str> = Vec::new();
    if let Some(sub) = volume_subpath {
        let sub = sub.trim_matches('/');
        if !sub.is_empty() {
            parts.push(sub);
        }
    }
    if !dir_rel.is_empty() {
        parts.push(dir_rel.trim_matches('/'));
    }
    parts.push(file_name);
    Some(parts.join("/"))
}

/// 将（后代）rel_path 从旧前缀重映射到新前缀。
pub(crate) fn remap_rel(rel: &str, old_prefix: &str, new_prefix: &str) -> String {
    if rel == old_prefix {
        new_prefix.to_string()
    } else if let Some(suffix) = rel.strip_prefix(&format!("{old_prefix}/")) {
        format!("{new_prefix}/{suffix}")
    } else {
        // 实际上不在子树内 — 保持不变（防御性）。
        rel.to_string()
    }
}

/// 为新的 cache_key 重建存储的 thumb_path（保留尺寸档位段）。
pub(crate) fn remap_thumb_path(old: &str, new_key: i64) -> String {
    let size = old.split('/').next().unwrap_or("");
    let hex = cache_key_to_hex(new_key);
    let prefix = &hex[..2];
    format!("{size}/{prefix}/{hex}.webp")
}

/// 尽力而为：按新的 cache_key 重定位已缓存的缩略图 + 动态视频文件。
/// 重命名失败不致命 —— 资产会按需重新生成。
pub(crate) fn relocate_cache_files(cache_dir: &Path, renames: &[(i64, i64)]) {
    let thumbs = cache_dir.join("thumbnails");
    let motion = cache_dir.join("motion_videos");
    for &(old_key, new_key) in renames {
        let old_hex = cache_key_to_hex(old_key);
        let new_hex = cache_key_to_hex(new_key);
        // 缩略图按尺寸分桶存储 — 逐档位尝试。
        for tier in THUMB_TIERS {
            let old_p = thumbs
                .join(tier.to_string())
                .join(&old_hex[..2])
                .join(format!("{old_hex}.webp"));
            if old_p.exists() {
                let new_p = thumbs
                    .join(tier.to_string())
                    .join(&new_hex[..2])
                    .join(format!("{new_hex}.webp"));
                if let Some(parent) = new_p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::rename(&old_p, &new_p) {
                    tracing::warn!(
                        "thumb relocate failed {:?} -> {:?}: {} | 缩略图重定位失败",
                        old_p,
                        new_p,
                        e
                    );
                }
            }
        }
        // 动态视频（实况照片）缓存。
        let old_mv = motion.join(&old_hex[..2]).join(format!("{old_hex}.mp4"));
        if old_mv.exists() {
            let new_mv = motion.join(&new_hex[..2]).join(format!("{new_hex}.mp4"));
            if let Some(parent) = new_mv.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::rename(&old_mv, &new_mv);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 执行：阶段日志 + 物理 + DB（每个 DB 触点都是一次短阶段）
// ════════════════════════════════════════════════════════════════════════════

/// 执行一次目录移动。调用方须已持有源/目标两个扫描根的独占闸门与文件任务门闩，复核过计划仍然有效
/// （[verify_plan_current]），并运行在 blocking 线程上。
///
/// 失败语义：
///   - 物理失败 → 删日志行、源保持完整，返回原错误（我们自己的暂存目录已清）。
///   - 物理成功、DB 失败 → 日志停在 published（附我们写出树的内容凭据），返回
///     [AppError::MoveRecovery]（稳定码 move_db_pending + 日志 id + 目标绝对路径）。
///   - 全部成功但源残留 → 返回 Ok + source_leftover（索引已正确，仅旧路径还有副本）。
pub(crate) fn execute_move(
    db: MoveDb<'_>,
    plan: &MovePlan,
    cache_dir: &Path,
) -> Result<MoveDirOutcome> {
    // 阶段 1（短 DB）：登记意图。
    let journal_id = db.with_conn(&mut |conn| {
        q::insert_intent(
            conn,
            plan.source_dir_id,
            plan.source_root_id,
            &plan.old_rel,
            &plan.src_abs.to_string_lossy(),
            plan.target_root_id,
            &plan.new_rel,
            &plan.dst_abs.to_string_lossy(),
            plan.target_parent_id,
            Some(&plan.staging_abs.to_string_lossy()),
        )
    })?;

    // 阶段 2（文件 IO，无 DB 锁）：rename 或「复制到独占暂存」。
    let source_removed = match begin_physical_move(plan) {
        // 同卷 rename 原子落位：源已不在，没有「我们写出的树」也就没有可落的凭据。
        Ok(PhysicalPhase::Renamed) => true,
        Ok(PhysicalPhase::NeedsStaging) => {
            // 阶段 3（文件 IO + 短 DB）：复制到独占暂存，并把内容凭据持久化——**这是发布的硬
            // 前置**。凭据写不进库就绝不发布：源与暂存都还在，阶段日志保留待重试，返回明确失败。
            stage_and_persist(db, plan, journal_id)?;
            // 阶段 4（文件 IO）：发布 + 删源。
            match publish_staging(plan) {
                Ok(removed) => removed,
                Err(e) => {
                    // 未发布：源完整、暂存已清 → 日志行可作废（凭据留着也无害，但没必要）。
                    let _ = db.with_conn(&mut |conn| q::delete(conn, journal_id));
                    return Err(e);
                }
            }
        }
        Err(e) => {
            // 未发布：源完整，清掉日志行（暂存清理在物理阶段内完成）。
            let _ = db.with_conn(&mut |conn| q::delete(conn, journal_id));
            return Err(e);
        }
    };

    // 阶段 5（短 DB）：标记发布完成（凭据已在前面落盘，这里只推进阶段）。
    let _ = db.with_conn(&mut |conn| q::set_stage(conn, journal_id, q::STAGE_PUBLISHED));

    // 阶段 6（短 DB）：身份重写。
    match db.with_conn(&mut |conn| apply_move_db(conn, plan)) {
        Ok(outcome) => {
            // 阶段 7（文件 IO，无 DB 锁）：缓存文件重定位。
            relocate_cache_files(cache_dir, &outcome.cache_renames);
            let _ = db.with_conn(&mut |conn| {
                q::set_counts(
                    conn,
                    journal_id,
                    outcome.affected_dirs as i64,
                    outcome.affected_media as i64,
                )
            });
            // 阶段 8（短 DB）：收尾。源删除已由物理阶段完成；这里只处理「源还在」的残留态。
            if source_removed {
                let _ = db.with_conn(&mut |conn| q::delete(conn, journal_id));
                Ok(MoveDirOutcome {
                    affected_dirs: outcome.affected_dirs,
                    affected_media: outcome.affected_media,
                    source_leftover: None,
                    recovery_id: None,
                })
            } else {
                let _ = db.with_conn(&mut |conn| {
                    q::set_stage(conn, journal_id, q::STAGE_SOURCE_LEFTOVER)
                });
                Ok(MoveDirOutcome {
                    affected_dirs: outcome.affected_dirs,
                    affected_media: outcome.affected_media,
                    source_leftover: Some(plan.src_abs.to_string_lossy().to_string()),
                    recovery_id: Some(journal_id),
                })
            }
        }
        Err(e) => {
            // 物理已发布、库未改：真实原因只进日志，IPC 只给稳定码 + 位置（不泄漏 SQL）。
            tracing::error!(
                "dir_move: DB 重写失败，等待重试 journal={journal_id} target={} error={e}",
                plan.dst_abs.display()
            );
            Err(AppError::MoveRecovery {
                code: CODE_DB_PENDING,
                message: MSG_INDEX_PENDING.into(),
                recovery_id: journal_id,
                target_abs_path: plan.dst_abs.to_string_lossy().to_string(),
            })
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 恢复 / 重试
// ════════════════════════════════════════════════════════════════════════════

/// 收尾一条未完成的移动（幂等）。allow_physical = 是否允许重做物理搬运与整树校验。
///
/// 启动期传 false：只做**可证的短收敛**——目标存在性检查 + 已发布行的索引重放；大拷贝、整树摘要
/// 校验、源残留清理一律留给用户显式重试（审查复核 6）。
///
/// 任何「无法证明目标属于本次移动」的情形都保留阶段日志与原索引状态：不改库、不删源、不丢线索。
pub(crate) fn finish_entry(
    db: MoveDb<'_>,
    entry: &q::MoveJournalEntry,
    cache_dir: &Path,
    allow_physical: bool,
) -> MoveRecoveryReport {
    let mut report = MoveRecoveryReport {
        recovery_id: entry.id,
        stage: entry.stage.clone(),
        source_name: source_name_of(entry),
        source_abs_path: entry.source_abs_path.clone(),
        target_abs_path: entry.target_abs_path.clone(),
        target_rel_path: entry.target_rel_path.clone(),
        target_root_id: entry.target_root_id,
        needs_retry: false,
        detail: "completed",
    };
    let keep = |report: &mut MoveRecoveryReport, detail: &'static str| {
        report.needs_retry = true;
        report.detail = detail;
    };
    let set_stage = |db: MoveDb<'_>, id: i64, stage: &str| {
        let _ = db.with_conn(&mut |conn| q::set_stage(conn, id, stage));
    };

    // 目标根：行确实不存在（Ok(None)）才是终态可作废；查询失败只是「现在读不到」，必须保留线索。
    let target_root =
        match db.with_conn(&mut |conn| q::find_scan_root_ref(conn, entry.target_root_id)) {
            Ok(Some(root)) => root,
            Ok(None) => {
                let _ = db.with_conn(&mut |conn| q::delete(conn, entry.id));
                report.detail = "target_root_missing";
                return report;
            }
            Err(e) => {
                tracing::warn!(
                    "dir_move: 读取目标根失败 id={} error={e}（保留阶段日志，不当作根已删）",
                    entry.id
                );
                keep(&mut report, "unknown");
                return report;
            }
        };

    // 计划：DB 读（短阶段）→ 路径规范化（文件 IO）。
    let plan = match plan_from_journal(db, entry, &target_root.path) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "dir_move: 恢复无法重建计划 id={} error={e}（保留阶段日志）",
                entry.id
            );
            keep(&mut report, "target_unresolved");
            return report;
        }
    };
    // 扫描根本身的可访问性：只有根本身在场，「子路径不存在」的判定才可信（卷离线常让子路径报
    // NotFound，直接当成「没发生过」会把可恢复记录误作废）。
    // 根是否可见用**原样路径**判定（用户挂载点口径），canonical 只用于边界校验。
    let src_root_state = path_state(Path::new(&plan.source_root_path));
    let dst_root_state = path_state(Path::new(&plan.target_root_path));
    let published_stage =
        entry.stage == q::STAGE_PUBLISHED || entry.stage == q::STAGE_SOURCE_LEFTOVER;
    let mut claimed_published = published_stage;

    if published_stage {
        // 已发布行：目标必须仍在，否则绝不假收尾 —— 保留日志与旧索引状态。
        match path_state(&plan.dst_abs) {
            PathState::Exists => {}
            _ => {
                keep(
                    &mut report,
                    if dst_root_state == PathState::Exists {
                        "target_missing"
                    } else {
                        // 根都看不见 → 更像是卷离线，不是目标被删。
                        "unknown"
                    },
                );
                return report;
            }
        }
        // 凭据是可选的（rename 路径没有）。启动档不做整树读取；显式重试时凡有凭据就必须对上。
        if allow_physical {
            if let Some(expected) = entry.payload_digest.as_deref() {
                match tree_payload_digest(&plan.dst_abs) {
                    Ok(actual) if actual.digest == expected => {}
                    Ok(_) => {
                        keep(&mut report, "target_conflict");
                        return report;
                    }
                    Err(e) => {
                        tracing::warn!("dir_move: 目标摘要校验失败 id={} error={e}", entry.id);
                        keep(&mut report, "target_missing");
                        return report;
                    }
                }
            }
        }
    } else {
        let src_state = path_state(&plan.src_abs);
        let dst_state = path_state(&plan.dst_abs);
        match (src_state, dst_state) {
            // 源与目标都在：必须能证明目标是我们写出的那棵树。
            (PathState::Exists, PathState::Exists) => {
                let Some(expected) = entry.payload_digest.clone() else {
                    // rename 路径不可能同时留下两边 —— 目标不是我们的产物，保持冲突。
                    keep(&mut report, "target_conflict");
                    return report;
                };
                if !allow_physical {
                    keep(&mut report, "target_unverified");
                    return report;
                }
                match tree_payload_digest(&plan.dst_abs) {
                    Ok(actual) if actual.digest == expected => claimed_published = true,
                    Ok(_) => {
                        // 摘要对不上：目标可能是别人写了一半的同名目录，或是我们复制后被改动。
                        // 两种都不能认领（改了索引就指向不可信内容），也不能删源。
                        keep(&mut report, "target_conflict");
                        return report;
                    }
                    Err(e) => {
                        tracing::warn!("dir_move: 目标摘要校验失败 id={} error={e}", entry.id);
                        keep(&mut report, "unknown");
                        return report;
                    }
                }
            }
            // 源已不在、目标在：有凭据就必须对得上；rename 路径（无凭据）由物理状态自证。
            (PathState::Absent, PathState::Exists) => {
                if let Some(expected) = entry.payload_digest.as_deref() {
                    if !allow_physical {
                        keep(&mut report, "target_unverified");
                        return report;
                    }
                    match tree_payload_digest(&plan.dst_abs) {
                        Ok(actual) if actual.digest == expected => claimed_published = true,
                        Ok(_) => {
                            keep(&mut report, "target_conflict");
                            return report;
                        }
                        Err(e) => {
                            tracing::warn!("dir_move: 目标摘要校验失败 id={} error={e}", entry.id);
                            keep(&mut report, "unknown");
                            return report;
                        }
                    }
                } else {
                    claimed_published = true;
                }
            }
            // 双方都不在：只有两侧扫描根本身都可见，才能认定「本次移动什么都没发生」并作废日志；
            // 否则（卷离线）保留线索（审查复核 6）。
            (PathState::Absent, PathState::Absent) => {
                if src_root_state == PathState::Exists && dst_root_state == PathState::Exists {
                    let _ = db.with_conn(&mut |conn| q::delete(conn, entry.id));
                    report.detail = "absent";
                } else {
                    keep(&mut report, "unknown");
                }
                return report;
            }
            // 判不了（卷离线/权限）：保留日志行等下次。
            (PathState::Unknown, _) | (_, PathState::Unknown) => {
                keep(&mut report, "unknown");
                return report;
            }
            // 源在、目标不在：物理搬运没完成。启动档不做大拷贝。
            (PathState::Exists, PathState::Absent) => {
                if !allow_physical {
                    keep(&mut report, "staging_busy");
                    return report;
                }
                // 上一轮可能已复制完但还没发布：凭据一致就直接复用，省掉一次整树拷贝。
                let reusable = entry
                    .payload_digest
                    .as_deref()
                    .is_some_and(|d| staging_reusable(&plan, d));
                if !reusable {
                    cleanup_own_staging(&plan.staging_abs);
                    // 与首轮同一硬前置：复制后必须先落凭据才允许发布（见 [stage_and_persist]）。
                    // 凭据写不进去就保持源完整、暂存留着，日志保留待下次重试。
                    if let Err(e) = stage_and_persist(db, &plan, entry.id) {
                        tracing::warn!("dir_move: 恢复复制/落凭据失败 id={} error={e}", entry.id);
                        keep(&mut report, "payload_pending");
                        return report;
                    }
                }
                match publish_staging(&plan) {
                    Ok(_) => {
                        let _ = db.with_conn(&mut |conn| {
                            q::set_stage(conn, entry.id, q::STAGE_PUBLISHED)
                        });
                        claimed_published = true;
                    }
                    Err(e) => {
                        tracing::warn!("dir_move: 恢复发布失败 id={} error={e}", entry.id);
                        keep(&mut report, "staging_busy");
                        return report;
                    }
                }
            }
        }
    }

    if !claimed_published {
        keep(&mut report, "unknown");
        return report;
    }

    // ── 索引重写（幂等重放）──
    match db.with_conn(&mut |conn| apply_move_db(conn, &plan)) {
        Ok(outcome) => {
            relocate_cache_files(cache_dir, &outcome.cache_renames);
            let _ = db.with_conn(&mut |conn| {
                q::set_counts(
                    conn,
                    entry.id,
                    outcome.affected_dirs as i64,
                    outcome.affected_media as i64,
                )
            });
        }
        Err(e) => {
            tracing::error!("dir_move: 恢复 DB 重写失败 id={} error={e}", entry.id);
            set_stage(db, entry.id, q::STAGE_PUBLISHED);
            report.stage = q::STAGE_PUBLISHED.to_string();
            keep(&mut report, "published");
            return report;
        }
    }

    // ── 源残留处置 ──
    // 启动档只更新阶段、不动源（整树比对可能很大）；显式重试才逐文件比对并清理。
    match path_state(&plan.src_abs) {
        PathState::Absent => {
            let _ = db.with_conn(&mut |conn| q::delete(conn, entry.id));
            report.detail = "completed";
        }
        PathState::Unknown => {
            set_stage(db, entry.id, q::STAGE_SOURCE_LEFTOVER);
            report.stage = q::STAGE_SOURCE_LEFTOVER.to_string();
            keep(&mut report, "source_leftover");
        }
        PathState::Exists => {
            if !allow_physical {
                set_stage(db, entry.id, q::STAGE_SOURCE_LEFTOVER);
                report.stage = q::STAGE_SOURCE_LEFTOVER.to_string();
                keep(&mut report, "source_leftover");
                return report;
            }
            match source_subset_of_target(&plan.src_abs, &plan.dst_abs) {
                Ok(true) => {
                    cleanup_own_staging(&plan.staging_abs);
                    match std::fs::remove_dir_all(&plan.src_abs) {
                        Ok(()) => {
                            let _ = db.with_conn(&mut |conn| q::delete(conn, entry.id));
                            report.detail = "completed";
                        }
                        Err(e) => {
                            tracing::warn!(
                                "dir_move: 源残留清理失败 source={} error={e}",
                                plan.src_abs.display()
                            );
                            set_stage(db, entry.id, q::STAGE_SOURCE_LEFTOVER);
                            report.stage = q::STAGE_SOURCE_LEFTOVER.to_string();
                            keep(&mut report, "source_leftover");
                        }
                    }
                }
                // 源里有目标没有的内容（含同尺寸改写）→ 绝不删，报冲突等用户处置。
                Ok(false) => {
                    set_stage(db, entry.id, q::STAGE_SOURCE_LEFTOVER);
                    report.stage = q::STAGE_SOURCE_LEFTOVER.to_string();
                    keep(&mut report, "source_changed");
                }
                Err(e) => {
                    tracing::warn!(
                        "dir_move: 源残留比对失败 source={} error={e}",
                        plan.src_abs.display()
                    );
                    set_stage(db, entry.id, q::STAGE_SOURCE_LEFTOVER);
                    report.stage = q::STAGE_SOURCE_LEFTOVER.to_string();
                    keep(&mut report, "source_leftover");
                }
            }
        }
    }
    report
}

/// 从日志行重建移动计划。目标根的路径与卷绑定由调用方传入（读根是上一段短 DB 阶段的事），
/// 父目录按 target_parent_id 现查：行不在了（被删根/清库连带清理）即失败并保留日志，
/// 绝不写 NULL（那会把普通目录变成树里的伪根；审查复核 7）。
fn plan_from_journal(
    db: MoveDb<'_>,
    entry: &q::MoveJournalEntry,
    target_root_path_raw: &str,
) -> Result<MovePlan> {
    let parent_id = db.with_conn(&mut |conn| -> Result<i64> {
        // 落盘时记下的父 id 优先；缺失（历史行/手工构造）才按 rel 反查。
        if let Ok(d) = q::get_directory(conn, entry.target_parent_id) {
            return Ok(d.id);
        }
        let parent_rel = parent_rel_of(&entry.target_rel_path);
        q::find_directory_id(conn, entry.target_root_id, &parent_rel)?.ok_or_else(|| {
            AppError::Internal(format!(
                "目录移动恢复：目标父目录不存在 root={} rel={parent_rel}",
                entry.target_root_id
            ))
        })
    })?;
    let (name, _) = split_rel(&entry.target_rel_path);
    let src_abs = resolve_existing_or_absent(&entry.source_abs_path)?;
    let dst_abs = resolve_creatable_in_root(
        &canonical_dir(target_root_path_raw)?,
        &rel_segments(&entry.target_rel_path)?,
    )?;
    let (volume_id, volume_subpath) = db
        .with_conn(&mut |conn| q::find_scan_root_ref(conn, entry.target_root_id))
        .ok()
        .flatten()
        .map(|r| (r.volume_id, r.volume_subpath))
        .unwrap_or((None, None));
    let source_root_path = db
        .with_conn(&mut |conn| q::find_scan_root_ref(conn, entry.source_root_id))
        .ok()
        .flatten()
        .map(|r| r.path)
        .unwrap_or_else(|| entry.source_abs_path.clone());
    Ok(MovePlan {
        source_dir_id: entry.source_dir_id,
        name: name.clone(),
        old_rel: entry.source_rel_path.clone(),
        new_rel: entry.target_rel_path.clone(),
        source_root_id: entry.source_root_id,
        target_root_id: entry.target_root_id,
        target_parent_id: parent_id,
        source_root_path,
        target_root_path: target_root_path_raw.to_string(),
        target_volume_id: volume_id,
        target_volume_subpath: volume_subpath,
        staging_abs: entry
            .staging_abs_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| staging_path_for(&dst_abs, &name)),
        src_abs,
        dst_abs,
    })
}

/// 源路径（恢复用）：存在则 canonicalize；不存在时按字符串路径保留（状态判定会处理）。
fn resolve_existing_or_absent(path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    match path_state(&p) {
        PathState::Exists => Ok(std::fs::canonicalize(&p).unwrap_or(p)),
        _ => Ok(p),
    }
}

/// 日志行里的源目录名（目标 rel_path 的末段）。
fn source_name_of(entry: &q::MoveJournalEntry) -> String {
    let (name, _) = split_rel(&entry.target_rel_path);
    if !name.is_empty() {
        return name;
    }
    entry
        .source_abs_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// 收尾全部未完成项。allow_physical=false = 启动档（不做大拷贝、不做整树校验、不删源）。
///
/// 每一条都先在一次短 DB 阶段里读出行，再在**锁外**做文件系统判定，最后再进短 DB 阶段写，
/// 因此收尾不会持着写锁等磁盘。
pub(crate) fn recover_pending(
    db: MoveDb<'_>,
    cache_dir: &Path,
    allow_physical: bool,
) -> Result<Vec<MoveRecoveryReport>> {
    let pending = db.with_conn(&mut |conn| q::list_pending(conn))?;
    let mut reports = Vec::with_capacity(pending.len());
    for entry in pending {
        let report = finish_entry(db, &entry, cache_dir, allow_physical);
        if report.needs_retry || report.detail != "completed" {
            tracing::info!(
                "dir_move: 收尾 id={} stage={} detail={} needs_retry={}",
                report.recovery_id,
                report.stage,
                report.detail,
                report.needs_retry
            );
        }
        reports.push(report);
    }
    Ok(reports)
}

/// 按 id 重试一条（用户触发，允许重做物理搬运与整树校验）。
pub(crate) fn retry_entry(
    db: MoveDb<'_>,
    cache_dir: &Path,
    recovery_id: i64,
) -> Result<Option<MoveRecoveryReport>> {
    let Some(entry) = db.with_conn(&mut |conn| q::get(conn, recovery_id))? else {
        return Ok(None);
    };
    Ok(Some(finish_entry(db, &entry, cache_dir, true)))
}

/// 只读的未完成状态（不做任何收尾、不碰磁盘）：UI 据此说明「文件在哪、要不要重试」。
pub(crate) fn pending_report(entry: &q::MoveJournalEntry) -> MoveRecoveryReport {
    MoveRecoveryReport {
        recovery_id: entry.id,
        stage: entry.stage.clone(),
        source_name: source_name_of(entry),
        source_abs_path: entry.source_abs_path.clone(),
        target_abs_path: entry.target_abs_path.clone(),
        target_rel_path: entry.target_rel_path.clone(),
        target_root_id: entry.target_root_id,
        needs_retry: true,
        detail: if entry.stage == q::STAGE_INTENT {
            "staging_busy"
        } else {
            "source_leftover"
        },
    }
}

/// 启动期收尾（在 db/boot 自愈之后、任何管线拉起之前调用）。
///
/// 只做可证的短收敛：目标是存在性检查 + 已发布行的索引重放；不删源、不做整树摘要校验、不做大拷贝。
/// 仍在 intent 阶段的行（可能含 GB 级拷贝或需要整树证明）留待用户显式重试——调用方在启动路径上
/// 受控阻塞（管线尚未拉起，无并发写者）。
pub(crate) fn reconcile_at_startup(
    db: MoveDb<'_>,
    cache_dir: &Path,
) -> Result<Vec<MoveRecoveryReport>> {
    recover_pending(db, cache_dir, false)
}

#[cfg(test)]
mod tests;
