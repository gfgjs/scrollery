//! 流式精确摘要、候选快速摘要和逻辑单元摘要。
//!
//! 这里的 exact digest 是字节级内容身份，和 `utils::hash::content_fingerprint` 的
//! 扫描变更指纹严格分离。所有文件摘要都从打开的文件句柄读取，并在读取结束后同时
//! 复核句柄与路径的 size、纳秒 mtime 和物理身份；复核失败时不返回任何摘要。

use std::cmp::min;
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt as _;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle as _;
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO};

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// 所有顺序读摘要使用的固定 heap buffer 大小。
///
/// 2 MiB 位于要求的 1–4 MiB 范围内；它不会随文件大小增长。
pub const HASH_BUFFER_SIZE: usize = 2 * 1024 * 1024;

/// quick digest 每个头/中/尾采样窗口的大小。
pub const QUICK_SAMPLE_SIZE: u64 = 256 * 1024;

/// 摘要编码的版本。算法、域分隔和组合编码任何一项变化都必须递增它。
pub const DEDUP_HASH_VERSION: u32 = 1;
pub const EXACT_DIGEST_VERSION: u32 = DEDUP_HASH_VERSION;
pub const QUICK_DIGEST_VERSION: u32 = DEDUP_HASH_VERSION;
pub const UNIT_DIGEST_VERSION: u32 = DEDUP_HASH_VERSION;

const QUICK_DOMAIN: &[u8] = b"scrollery/dedup/quick\0";
const SINGLE_UNIT_DOMAIN: &[u8] = b"scrollery/dedup/unit/single\0";
const LIVE_PHOTO_UNIT_DOMAIN: &[u8] = b"scrollery/dedup/unit/live-photo\0";

/// 文件 I/O 阶段的稳定标识，不携带路径或操作系统原始错误文本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOperation {
    Open,
    StatBefore,
    StatAfter,
    Read,
    Seek,
}

impl fmt::Display for IoOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Open => "open",
            Self::StatBefore => "stat-before",
            Self::StatAfter => "stat-after",
            Self::Read => "read",
            Self::Seek => "seek",
        };
        formatter.write_str(label)
    }
}

/// 摘要失败的稳定错误契约。
///
/// 该错误故意不保存底层 `io::Error`，因此 `Display`/`Debug` 都不会把内部路径或 OS
/// 错误字符串泄漏给上层 IPC。调用方可用 [`HashError::code`] 做稳定分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HashError {
    #[error("dedup source is unavailable")]
    Unavailable,
    #[error("dedup source is not a regular file")]
    NotRegularFile,
    #[error("dedup source metadata is unavailable")]
    MetadataUnavailable,
    #[error("dedup source physical identity is unavailable")]
    IdentityUnavailable,
    #[error("dedup source is unstable")]
    Unstable,
    #[error("dedup source changed while hashing")]
    Stale,
    #[error("dedup hashing was cancelled")]
    Cancelled,
    #[error("dedup hash I/O failed during {operation}")]
    Io { operation: IoOperation },
}

impl HashError {
    /// 稳定的上层错误码；不包含路径和底层错误细节。
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "SOURCE_UNAVAILABLE",
            Self::NotRegularFile => "SOURCE_NOT_REGULAR",
            Self::MetadataUnavailable => "SOURCE_METADATA_UNAVAILABLE",
            Self::IdentityUnavailable => "SOURCE_IDENTITY_UNAVAILABLE",
            Self::Unstable => "SOURCE_UNSTABLE",
            Self::Stale => "SOURCE_STALE",
            Self::Cancelled => "DEDUP_CANCELLED",
            Self::Io { .. } => "HASH_IO",
        }
    }

    /// 是否表示摘要因源文件漂移而不可发布。
    pub const fn is_stale(self) -> bool {
        matches!(self, Self::Stale | Self::Unstable)
    }
}

/// 当前平台能取得的稳定物理文件身份。
///
/// Unix 使用 device/inode，Windows 使用 volume serial/128-bit file id。其他平台不伪造
/// 身份，而是让摘要操作返回 [`HashError::IdentityUnavailable`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial: u64,
        file_id: [u8; 16],
    },
    #[cfg(not(any(unix, windows)))]
    Unsupported,
}

/// 把当前平台的物理文件身份编码为稳定 BLOB，供 dedup sidecar 做硬链接去重和
/// 空间估算。它不是跨设备永久 ID；迁移/挂载变化时只能作为未知身份处理。
pub fn physical_key(identity: PhysicalIdentity) -> Vec<u8> {
    let mut key = b"scrollery/dedup/physical/v2\0".to_vec();
    match identity {
        #[cfg(unix)]
        PhysicalIdentity::Unix { device, inode } => {
            key.push(b'u');
            key.extend_from_slice(&device.to_le_bytes());
            key.extend_from_slice(&inode.to_le_bytes());
        }
        #[cfg(windows)]
        PhysicalIdentity::Windows {
            volume_serial,
            file_id,
        } => {
            key.push(b'w');
            key.extend_from_slice(&volume_serial.to_le_bytes());
            key.extend_from_slice(&file_id);
        }
        #[cfg(not(any(unix, windows)))]
        PhysicalIdentity::Unsupported => key.push(b'?'),
    }
    key
}

/// 读取前后用于比对的文件状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSnapshot {
    pub size: u64,
    pub mtime_ns: i128,
    pub physical_identity: PhysicalIdentity,
}

/// 经前后状态核对后发布的 BLAKE3-256 精确摘要。
///
/// 类型上独立于 [`QuickDigest`]，避免调用方把候选摘要误当成精确证据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExactDigest([u8; 32]);

/// 带状态快照的精确摘要，供 index/task 写回 source revision 和文件状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactFileDigest {
    pub digest: ExactDigest,
    pub snapshot: FileSnapshot,
}

/// 仅用于候选筛选的版本化 quick digest。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QuickDigest([u8; 32]);

/// 带状态快照的 quick digest。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickFileDigest {
    pub digest: QuickDigest,
    pub snapshot: FileSnapshot,
}

/// 版本化逻辑单元摘要：普通文件或 Live Photo 主项及其 companions 的组合身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitDigest {
    version: u32,
    bytes: [u8; 32],
}

/// Live Photo 组合摘要及各组件的已核验精确摘要。
///
/// 任务层用组合摘要写入主项，用组件摘要写入 companion 自己的 sidecar；这样 companion
/// 不会成为独立重复组成员，但其 exact digest 仍然可用于审查、漂移检测和后续恢复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePhotoDigestComponents {
    pub unit_digest: UnitDigest,
    pub main: ExactFileDigest,
    pub companions: Vec<(PathBuf, ExactFileDigest)>,
}

impl ExactDigest {
    pub const VERSION: u32 = EXACT_DIGEST_VERSION;

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn to_hex(self) -> String {
        hex_lower(&self.0)
    }
}

impl AsRef<[u8]> for ExactDigest {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl ExactFileDigest {
    pub const fn size(&self) -> u64 {
        self.snapshot.size
    }

    pub const fn mtime_ns(&self) -> i128 {
        self.snapshot.mtime_ns
    }
}

impl QuickDigest {
    pub const VERSION: u32 = QUICK_DIGEST_VERSION;

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn to_hex(self) -> String {
        hex_lower(&self.0)
    }
}

impl AsRef<[u8]> for QuickDigest {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl QuickFileDigest {
    pub const fn size(&self) -> u64 {
        self.snapshot.size
    }

    pub const fn mtime_ns(&self) -> i128 {
        self.snapshot.mtime_ns
    }
}

impl UnitDigest {
    pub const fn version(self) -> u32 {
        self.version
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.bytes
    }

    pub fn to_hex(self) -> String {
        hex_lower(&self.bytes)
    }
}

impl AsRef<[u8]> for UnitDigest {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// 读取并验证一个文件的 size、纳秒 mtime 和物理身份。
pub fn file_snapshot(path: &Path) -> Result<FileSnapshot, HashError> {
    open_verified_file(path).map(|(_, snapshot)| snapshot)
}

/// 流式计算文件的 BLAKE3-256 精确摘要。
///
/// 文件只按固定 2 MiB buffer 顺序读取；源文件或路径在读取期间发生 size、mtime 或
/// 物理身份漂移时返回 [`HashError::Stale`]，不会返回旧摘要。
pub fn exact_digest(path: &Path) -> Result<ExactDigest, HashError> {
    exact_digest_with_snapshot(path).map(|result| result.digest)
}

/// 计算精确摘要并返回已验证的最终文件快照。
pub fn exact_digest_with_snapshot(path: &Path) -> Result<ExactFileDigest, HashError> {
    exact_digest_with_snapshot_cancelled(path, &|| false)
}

/// 可取消的精确摘要；取消检查位于每个固定大小读块之后，因此取消延迟最多受一个
/// `HASH_BUFFER_SIZE` 读块限制。
pub fn exact_digest_with_snapshot_cancelled(
    path: &Path,
    should_cancel: &dyn Fn() -> bool,
) -> Result<ExactFileDigest, HashError> {
    let (mut file, before) = open_verified_file(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    let mut bytes_read = 0u64;

    loop {
        let read = file.read(&mut buffer).map_err(|_| HashError::Io {
            operation: IoOperation::Read,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes_read = bytes_read
            .checked_add(read as u64)
            .ok_or(HashError::Unstable)?;
        notify_test_read_hook(path, bytes_read);
        if should_cancel() {
            return Err(HashError::Cancelled);
        }
    }

    let after = finish_verified_file(path, &file, before)?;
    if bytes_read != before.size || bytes_read != after.size {
        return Err(HashError::Stale);
    }

    Ok(ExactFileDigest {
        digest: ExactDigest(finalize_hasher(hasher)),
        snapshot: after,
    })
}

/// 计算版本化 quick digest，仅可用于候选筛选，不能替代 [`exact_digest`]。
///
/// 大文件只读取头/中/尾各一个采样窗口，但同样执行前后状态校验。quick digest 的
/// 输入带固定域、版本、文件长度、窗口位置和窗口长度，和 exact digest 在类型及编码上
/// 都彼此隔离。
pub fn quick_digest(path: &Path) -> Result<QuickDigest, HashError> {
    quick_digest_with_snapshot(path).map(|result| result.digest)
}

/// 计算 quick digest 并返回已验证的最终文件快照。
pub fn quick_digest_with_snapshot(path: &Path) -> Result<QuickFileDigest, HashError> {
    quick_digest_with_snapshot_cancelled(path, &|| false)
}

/// 可取消的 quick 摘要版本；每个采样读块后检查取消令牌。
pub fn quick_digest_with_snapshot_cancelled(
    path: &Path,
    should_cancel: &dyn Fn() -> bool,
) -> Result<QuickFileDigest, HashError> {
    let (mut file, before) = open_verified_file(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(QUICK_DOMAIN);
    hasher.update(&QUICK_DIGEST_VERSION.to_le_bytes());
    hasher.update(&before.size.to_le_bytes());

    let offsets = sample_offsets(before.size);
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    for (index, offset) in offsets.into_iter().enumerate() {
        let wanted = min(QUICK_SAMPLE_SIZE, before.size.saturating_sub(offset));
        hasher.update(&(index as u32).to_le_bytes());
        hasher.update(&offset.to_le_bytes());
        hasher.update(&wanted.to_le_bytes());
        read_sample(
            &mut file,
            offset,
            wanted,
            &mut buffer,
            &mut hasher,
            path,
            should_cancel,
        )?;
    }

    let after = finish_verified_file(path, &file, before)?;
    Ok(QuickFileDigest {
        digest: QuickDigest(finalize_hasher(hasher)),
        snapshot: after,
    })
}

/// 普通文件的逻辑单元摘要。
pub fn unit_digest(path: &Path) -> Result<UnitDigest, HashError> {
    let exact = exact_digest_with_snapshot(path)?;
    Ok(single_unit_digest(exact.snapshot.size, &exact.digest))
}

/// 从已核验的普通文件 size/exact digest 构造逻辑单元摘要。
///
/// 该 helper 便于 index/task 在已经完成 exact 读取后避免再次打开文件。
pub fn single_unit_digest(size: u64, exact: &ExactDigest) -> UnitDigest {
    let mut hasher = unit_hasher(SINGLE_UNIT_DOMAIN);
    hasher.update(&size.to_le_bytes());
    hasher.update(exact.as_bytes());
    UnitDigest {
        version: UNIT_DIGEST_VERSION,
        bytes: finalize_hasher(hasher),
    }
}

/// Live Photo 主文件和全部 companion 的确定性组合摘要。
///
/// companions 会按平台原始路径字节（Windows 为 UTF-16 code unit 的小端编码）排序，
/// 因而调用方传入顺序不会影响结果。路径只用于排序，不进入摘要；摘要内容只包含
/// 主文件/companion 的 size 和 exact digest。每个组件在自己的 exact 读取后以及整个
/// 组合结束前都会再次核对快照。
pub fn live_photo_unit_digest<P, I, C>(main: P, companions: I) -> Result<UnitDigest, HashError>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = C>,
    C: AsRef<Path>,
{
    Ok(live_photo_unit_digest_with_components(main, companions)?.unit_digest)
}

/// 计算 Live Photo 组合摘要，同时返回每个组件的精确摘要。
///
/// 组件路径只用于确定性排序和把结果映射回数据库行，不进入摘要字节；主文件和全部
/// companion 在计算完成后都会再次复核，任何组件漂移都使整个组合失败。
pub fn live_photo_unit_digest_with_components<P, I, C>(
    main: P,
    companions: I,
) -> Result<LivePhotoDigestComponents, HashError>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = C>,
    C: AsRef<Path>,
{
    live_photo_unit_digest_with_components_cancelled(main, companions, &|| false)
}

/// 可取消的 Live Photo 组合摘要；主文件和每个 companion 均复用可取消精确读取。
pub fn live_photo_unit_digest_with_components_cancelled<P, I, C>(
    main: P,
    companions: I,
    should_cancel: &dyn Fn() -> bool,
) -> Result<LivePhotoDigestComponents, HashError>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = C>,
    C: AsRef<Path>,
{
    let main_path = main.as_ref().to_path_buf();
    let mut companion_paths: Vec<PathBuf> = companions
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .collect();
    companion_paths.sort_by_cached_key(|path| stable_path_key(path));

    let main_digest = exact_digest_with_snapshot_cancelled(&main_path, should_cancel)?;
    let mut companion_digests = Vec::with_capacity(companion_paths.len());
    for path in &companion_paths {
        companion_digests.push((
            path.clone(),
            exact_digest_with_snapshot_cancelled(path, should_cancel)?,
        ));
    }

    // 主文件可能在读取 companion 期间漂移；组件也可能在各自 exact 完成后漂移。
    verify_snapshot_unchanged(&main_path, main_digest.snapshot)?;
    for (path, digest) in &companion_digests {
        verify_snapshot_unchanged(path, digest.snapshot)?;
    }

    let mut hasher = unit_hasher(LIVE_PHOTO_UNIT_DOMAIN);
    hasher.update(&main_digest.snapshot.size.to_le_bytes());
    hasher.update(main_digest.digest.as_bytes());
    hasher.update(&(companion_digests.len() as u64).to_le_bytes());
    for (_, digest) in &companion_digests {
        hasher.update(&digest.snapshot.size.to_le_bytes());
        hasher.update(digest.digest.as_bytes());
    }

    Ok(LivePhotoDigestComponents {
        unit_digest: UnitDigest {
            version: UNIT_DIGEST_VERSION,
            bytes: finalize_hasher(hasher),
        },
        main: main_digest,
        companions: companion_digests,
    })
}

fn open_verified_file(path: &Path) -> Result<(File, FileSnapshot), HashError> {
    // `symlink_metadata` is deliberate: the scanner does not follow symlinks, so an analysis
    // request must not silently turn a path escape into a digest of its target.
    reject_link_components(path)?;
    let path_before = fs::symlink_metadata(path)
        .map_err(|error| map_initial_io(IoOperation::StatBefore, error))?;
    let path_before_fields = snapshot_fields_from_metadata(&path_before)?;
    let file = open_hash_file(path).map_err(map_open_after_stat_io)?;
    let handle_snapshot = snapshot_from_file(&file)?;

    // 重新打开路径取得第二个 handle，才能把“打开前看到的路径”与实际读到的 handle
    // 绑定起来；no-follow 打开和句柄 physical identity 比对共同关闭路径替换窗口。
    let path_file = open_hash_file(path).map_err(|_| HashError::Stale)?;
    let path_snapshot = snapshot_from_file(&path_file).map_err(map_final_snapshot_error)?;

    if path_before_fields != snapshot_fields(&handle_snapshot) || path_snapshot != handle_snapshot {
        return Err(HashError::Stale);
    }
    Ok((file, handle_snapshot))
}

fn finish_verified_file(
    path: &Path,
    file: &File,
    before: FileSnapshot,
) -> Result<FileSnapshot, HashError> {
    reject_link_components(path).map_err(|_| HashError::Stale)?;
    let handle_after = snapshot_from_file(file).map_err(map_final_snapshot_error)?;
    let path_metadata = fs::symlink_metadata(path).map_err(|_| HashError::Stale)?;
    let path_fields =
        snapshot_fields_from_metadata(&path_metadata).map_err(map_final_snapshot_error)?;
    let path_file = open_hash_file(path).map_err(|_| HashError::Stale)?;
    let path_after = snapshot_from_file(&path_file).map_err(map_final_snapshot_error)?;

    if path_fields != snapshot_fields(&path_after)
        || before != handle_after
        || before != path_after
        || handle_after != path_after
    {
        return Err(HashError::Stale);
    }
    Ok(handle_after)
}

fn verify_snapshot_unchanged(path: &Path, expected: FileSnapshot) -> Result<(), HashError> {
    reject_link_components(path).map_err(|_| HashError::Stale)?;
    let file = open_hash_file(path).map_err(|_| HashError::Stale)?;
    let actual = snapshot_from_file(&file).map_err(map_final_snapshot_error)?;
    if actual != expected {
        return Err(HashError::Stale);
    }
    Ok(())
}

fn snapshot_from_file(file: &File) -> Result<FileSnapshot, HashError> {
    let metadata = file.metadata().map_err(|_| HashError::Io {
        operation: IoOperation::StatBefore,
    })?;
    let (size, mtime_ns) = snapshot_fields_from_metadata(&metadata)?;
    let physical_identity = physical_identity_for_file(file, &metadata)?;
    Ok(FileSnapshot {
        size,
        mtime_ns,
        physical_identity,
    })
}

/// 以不跟随链接的方式打开摘要输入。
///
/// `symlink_metadata` 只能证明某一时刻的路径对象；若随后普通 `File::open` 跟随
/// reparse point/symlink，就会把路径竞态变成越界读取。平台原生 no-follow 标志与
/// 后续句柄快照配合，保证摘要只来自打开时固定的普通文件对象。
fn open_hash_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    #[cfg(windows)]
    options.custom_flags(0x0020_0000);
    options.open(path)
}

/// 拒绝目标及所有父级中的 symlink/reparse point。
///
/// `O_NOFOLLOW`/`FILE_FLAG_OPEN_REPARSE_POINT` 只约束最终目录项；若历史数据库路径的
/// 父目录后来被替换成链接，单独依赖这两个标志仍可能把摘要读到扫描根之外。这里在每个
/// 路径状态复核点检查整条祖先链；竞态替换仍会由句柄物理身份和末尾路径复核拦截。
fn reject_link_components(path: &Path) -> Result<(), HashError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| map_initial_io(IoOperation::StatBefore, error))?;
        #[cfg(windows)]
        let is_reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        #[cfg(not(windows))]
        let is_reparse = false;
        if metadata.file_type().is_symlink() || is_reparse {
            return Err(HashError::NotRegularFile);
        }
    }
    Ok(())
}

fn snapshot_fields_from_metadata(metadata: &Metadata) -> Result<(u64, i128), HashError> {
    if !metadata.is_file() {
        return Err(HashError::NotRegularFile);
    }
    let mtime_ns = system_time_to_ns(
        metadata
            .modified()
            .map_err(|_| HashError::MetadataUnavailable)?,
    )?;
    Ok((metadata.len(), mtime_ns))
}

const fn snapshot_fields(snapshot: &FileSnapshot) -> (u64, i128) {
    (snapshot.size, snapshot.mtime_ns)
}

fn system_time_to_ns(time: SystemTime) -> Result<i128, HashError> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).map_err(|_| HashError::Unstable),
        Err(error) => {
            let nanos =
                i128::try_from(error.duration().as_nanos()).map_err(|_| HashError::Unstable)?;
            nanos.checked_neg().ok_or(HashError::Unstable)
        }
    }
}

#[cfg(unix)]
fn physical_identity_for_file(
    _file: &File,
    metadata: &Metadata,
) -> Result<PhysicalIdentity, HashError> {
    let device = metadata.dev();
    let inode = metadata.ino();
    if device == 0 && inode == 0 {
        return Err(HashError::Unstable);
    }
    Ok(PhysicalIdentity::Unix { device, inode })
}

#[cfg(windows)]
fn physical_identity_for_file(
    file: &File,
    _metadata: &Metadata,
) -> Result<PhysicalIdentity, HashError> {
    let mut information = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle for the duration of this call and the Windows API
    // writes exactly one initialized FILE_ID_INFO value into the correctly sized buffer.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileIdInfo,
            (&mut information as *mut FILE_ID_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| HashError::IdentityUnavailable)?,
        )
    }
    .map_err(|_| HashError::IdentityUnavailable)?;

    let file_id = information.FileId.Identifier;
    if information.VolumeSerialNumber == 0 && file_id == [0; 16] {
        return Err(HashError::Unstable);
    }
    Ok(PhysicalIdentity::Windows {
        volume_serial: information.VolumeSerialNumber,
        file_id,
    })
}

#[cfg(not(any(unix, windows)))]
fn physical_identity_for_file(
    _file: &File,
    _metadata: &Metadata,
) -> Result<PhysicalIdentity, HashError> {
    Err(HashError::IdentityUnavailable)
}

fn map_initial_io(operation: IoOperation, error: std::io::Error) -> HashError {
    if error.kind() == ErrorKind::NotFound {
        HashError::Unavailable
    } else {
        HashError::Io { operation }
    }
}

fn map_open_after_stat_io(error: std::io::Error) -> HashError {
    if error.kind() == ErrorKind::NotFound {
        HashError::Stale
    } else {
        HashError::Io {
            operation: IoOperation::Open,
        }
    }
}

fn map_final_snapshot_error(error: HashError) -> HashError {
    match error {
        HashError::IdentityUnavailable | HashError::Unstable => error,
        HashError::NotRegularFile
        | HashError::MetadataUnavailable
        | HashError::Stale
        | HashError::Unavailable
        | HashError::Cancelled
        | HashError::Io { .. } => HashError::Stale,
    }
}

fn sample_offsets(size: u64) -> [u64; 3] {
    [
        0,
        (size / 2).saturating_sub(QUICK_SAMPLE_SIZE / 2),
        size.saturating_sub(QUICK_SAMPLE_SIZE),
    ]
}

fn read_sample(
    file: &mut File,
    offset: u64,
    wanted: u64,
    buffer: &mut [u8],
    hasher: &mut blake3::Hasher,
    path: &Path,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(), HashError> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| HashError::Io {
            operation: IoOperation::Seek,
        })?;
    let mut remaining = wanted;
    let mut total_read = 0u64;
    while remaining != 0 {
        let request = min(remaining, buffer.len() as u64) as usize;
        let read = file
            .read(&mut buffer[..request])
            .map_err(|_| HashError::Io {
                operation: IoOperation::Read,
            })?;
        if read == 0 {
            return Err(HashError::Stale);
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
        total_read += read as u64;
        notify_test_read_hook(path, total_read);
        if should_cancel() {
            return Err(HashError::Cancelled);
        }
    }
    Ok(())
}

fn unit_hasher(domain: &[u8]) -> blake3::Hasher {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(&UNIT_DIGEST_VERSION.to_le_bytes());
    hasher
}

fn finalize_hasher(hasher: blake3::Hasher) -> [u8; 32] {
    *hasher.finalize().as_bytes()
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn stable_path_key(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        path.as_os_str().as_bytes().to_vec()
    }

    #[cfg(windows)]
    {
        let mut key = Vec::new();
        for unit in path.as_os_str().encode_wide() {
            key.extend_from_slice(&unit.to_le_bytes());
        }
        key
    }

    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

#[cfg(test)]
use std::cell::RefCell;

#[cfg(test)]
type TestReadHook = Box<dyn FnMut(&Path, u64)>;

#[cfg(test)]
std::thread_local! {
    static TEST_READ_HOOK: RefCell<Option<TestReadHook>> = RefCell::new(None);
}

#[cfg(test)]
fn notify_test_read_hook(path: &Path, bytes_read: u64) {
    TEST_READ_HOOK.with(|hook| {
        if let Some(callback) = hook.borrow_mut().as_mut() {
            callback(path, bytes_read);
        }
    });
}

#[cfg(not(test))]
fn notify_test_read_hook(_path: &Path, _bytes_read: u64) {}

#[cfg(test)]
fn with_test_read_hook<H, F, R>(hook: H, operation: F) -> R
where
    H: FnMut(&Path, u64) + 'static,
    F: FnOnce() -> R,
{
    TEST_READ_HOOK.with(|slot| {
        assert!(slot.borrow().is_none(), "test read hook already installed");
        *slot.borrow_mut() = Some(Box::new(hook));
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    TEST_READ_HOOK.with(|slot| *slot.borrow_mut() = None);
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "scrollery-dedup-{label}-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create test temp directory");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_repeated(path: &Path, size: usize, byte: u8) {
        let mut file = File::create(path).expect("create test file");
        let buffer = vec![byte; 1024 * 1024];
        let mut remaining = size;
        while remaining != 0 {
            let count = min(remaining, buffer.len());
            file.write_all(&buffer[..count]).expect("write test file");
            remaining -= count;
        }
    }

    fn write_patterned(path: &Path, size: usize, middle_offset: usize, middle: u8) {
        let mut bytes = vec![0x11; size];
        let end = middle_offset + 1024;
        bytes[middle_offset..end].fill(middle);
        std::fs::write(path, bytes).expect("write patterned test file");
    }

    #[test]
    fn exact_digest_is_streaming_blake3_256() {
        let dir = TempDir::new("exact");
        let path = dir.file("a.bin");
        let contents = b"hello exact digest";
        std::fs::write(&path, contents).unwrap();

        let digest = exact_digest(&path).unwrap();
        assert_eq!(digest.as_bytes(), blake3::hash(contents).as_bytes());
        assert_eq!(ExactDigest::VERSION, 1);
        assert_eq!(digest.to_hex().len(), 64);
    }

    #[test]
    fn same_size_different_contents_have_different_exact_digest() {
        let dir = TempDir::new("same-size");
        let left = dir.file("left.bin");
        let right = dir.file("right.bin");
        std::fs::write(&left, [0x11u8; 32]).unwrap();
        std::fs::write(&right, [0x22u8; 32]).unwrap();

        assert_eq!(
            file_snapshot(&left).unwrap().size,
            file_snapshot(&right).unwrap().size
        );
        assert_ne!(exact_digest(&left).unwrap(), exact_digest(&right).unwrap());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_links_share_physical_identity_but_keep_distinct_paths() {
        let dir = TempDir::new("hard-link");
        let original = dir.file("original.bin");
        let alias = dir.file("alias.bin");
        std::fs::write(&original, b"one physical file").unwrap();
        if std::fs::hard_link(&original, &alias).is_err() {
            // 某些测试卷/权限策略不提供硬链接能力；不把环境缺口误报为业务失败。
            return;
        }

        let original_snapshot = file_snapshot(&original).unwrap();
        let alias_snapshot = file_snapshot(&alias).unwrap();
        assert_eq!(
            physical_key(original_snapshot.physical_identity),
            physical_key(alias_snapshot.physical_identity),
            "硬链接必须报告同一 physical key"
        );
        assert_ne!(
            stable_path_key(&original),
            stable_path_key(&alias),
            "硬链接的路径身份仍应保持独立"
        );
        assert_eq!(
            exact_digest(&original).unwrap(),
            exact_digest(&alias).unwrap()
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn exact_digest_rejects_symlink_without_following_target() {
        let dir = TempDir::new("symlink");
        let target = dir.file("target.bin");
        let link = dir.file("link.bin");
        std::fs::write(&target, b"target").unwrap();
        let symlink_result = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target, &link)
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&target, &link)
            }
        };
        if symlink_result.is_err() {
            // 普通用户 Windows 环境可能没有创建 symlink 的权限；cleanup 模块仍覆盖
            // 同一 no-follow 边界，具备权限的 CI 会执行这里的断言。
            return;
        }

        assert_eq!(exact_digest(&link), Err(HashError::NotRegularFile));
        assert_eq!(std::fs::read(&target).unwrap(), b"target");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn exact_digest_rejects_symlinked_parent_without_reading_escape_target() {
        let dir = TempDir::new("parent-symlink");
        let outside = TempDir::new("parent-symlink-target");
        let target = outside.file("target.bin");
        std::fs::write(&target, b"outside target").unwrap();
        let linked_parent = dir.file("linked-parent");
        let child = linked_parent.join("target.bin");
        let symlink_result = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(outside.path.clone(), &linked_parent)
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside.path, &linked_parent)
            }
        };
        if symlink_result.is_err() {
            return;
        }

        assert_eq!(exact_digest(&child), Err(HashError::NotRegularFile));
        assert_eq!(std::fs::read(&target).unwrap(), b"outside target");
    }

    #[test]
    fn quick_sampling_can_collide_but_exact_digest_cannot() {
        let dir = TempDir::new("sampling");
        let left = dir.file("left.bin");
        let right = dir.file("right.bin");
        let size = (QUICK_SAMPLE_SIZE as usize) * 12;
        // 1 MiB 的中段改动不落在头/中/尾 256 KiB 窗口中。
        write_patterned(&left, size, QUICK_SAMPLE_SIZE as usize * 2, 0x22);
        write_patterned(&right, size, QUICK_SAMPLE_SIZE as usize * 2, 0x33);

        assert_eq!(quick_digest(&left).unwrap(), quick_digest(&right).unwrap());
        assert_ne!(exact_digest(&left).unwrap(), exact_digest(&right).unwrap());
    }

    #[test]
    fn quick_digest_is_versioned_and_distinct_from_exact_type() {
        let dir = TempDir::new("quick-version");
        let path = dir.file("a.bin");
        std::fs::write(&path, b"candidate only").unwrap();

        let quick = quick_digest(&path).unwrap();
        let exact = exact_digest(&path).unwrap();
        assert_eq!(QuickDigest::VERSION, DEDUP_HASH_VERSION);
        assert_eq!(QUICK_DIGEST_VERSION, UNIT_DIGEST_VERSION);
        assert_ne!(quick.as_ref(), exact.as_ref());
    }

    #[test]
    fn cancellable_hash_stops_after_a_read_chunk() {
        let dir = TempDir::new("cancel");
        let path = dir.file("source.bin");
        write_repeated(&path, HASH_BUFFER_SIZE * 3, 0x44);
        let checks = std::cell::Cell::new(0u32);
        let result = exact_digest_with_snapshot_cancelled(&path, &|| {
            checks.set(checks.get() + 1);
            true
        });
        assert!(matches!(result, Err(HashError::Cancelled)));
        assert_eq!(checks.get(), 1);
    }

    #[test]
    fn replacement_during_hash_returns_stale_without_digest() {
        let dir = TempDir::new("replace");
        let path = dir.file("source.bin");
        write_repeated(&path, HASH_BUFFER_SIZE * 3, 0x10);
        let replacement = vec![0x77; HASH_BUFFER_SIZE * 3 + 1];
        let path_for_hook = path.clone();

        let result = with_test_read_hook(
            move |hook_path, bytes_read| {
                if bytes_read >= HASH_BUFFER_SIZE as u64 {
                    let replacement_path = hook_path.with_extension("replacement");
                    std::fs::write(&replacement_path, &replacement).unwrap();
                    std::fs::rename(&replacement_path, hook_path).unwrap();
                }
            },
            || exact_digest(&path_for_hook),
        );

        assert!(matches!(result, Err(HashError::Stale)));
    }

    #[test]
    fn truncation_during_hash_returns_stale_without_digest() {
        let dir = TempDir::new("truncate");
        let path = dir.file("source.bin");
        write_repeated(&path, HASH_BUFFER_SIZE * 3, 0x20);
        let path_for_hook = path.clone();

        let result = with_test_read_hook(
            move |hook_path, bytes_read| {
                if bytes_read >= HASH_BUFFER_SIZE as u64 {
                    let file = OpenOptions::new().write(true).open(hook_path).unwrap();
                    file.set_len(1).unwrap();
                }
            },
            || exact_digest(&path_for_hook),
        );

        assert!(matches!(result, Err(HashError::Stale)));
    }

    #[test]
    fn deletion_during_hash_returns_stale_without_digest() {
        let dir = TempDir::new("delete");
        let path = dir.file("source.bin");
        write_repeated(&path, HASH_BUFFER_SIZE * 3, 0x30);
        let path_for_hook = path.clone();
        let mut deleted = false;

        let result = with_test_read_hook(
            move |hook_path, bytes_read| {
                if !deleted && bytes_read >= HASH_BUFFER_SIZE as u64 {
                    std::fs::remove_file(hook_path).unwrap();
                    deleted = true;
                }
            },
            || exact_digest(&path_for_hook),
        );

        assert!(matches!(result, Err(HashError::Stale)));
    }

    #[test]
    fn live_photo_companion_changes_unit_digest_and_order_is_stable() {
        let dir = TempDir::new("live-photo");
        let main = dir.file("IMG_0001.jpg");
        let companion_a = dir.file("IMG_0001.mov");
        let companion_b = dir.file("IMG_0001.mp4");
        std::fs::write(&main, b"same main").unwrap();
        std::fs::write(&companion_a, b"motion A").unwrap();
        std::fs::write(&companion_b, b"motion B").unwrap();

        let ordered =
            live_photo_unit_digest(&main, vec![companion_a.clone(), companion_b.clone()]).unwrap();
        let reversed =
            live_photo_unit_digest(&main, vec![companion_b.clone(), companion_a.clone()]).unwrap();
        assert_eq!(ordered, reversed);
        assert_eq!(ordered.version(), UNIT_DIGEST_VERSION);

        std::fs::write(&companion_b, b"motion C").unwrap();
        let changed = live_photo_unit_digest(&main, vec![companion_a, companion_b]).unwrap();
        assert_ne!(ordered, changed);
    }

    #[test]
    fn ordinary_unit_digest_is_versioned_and_not_raw_exact_digest() {
        let dir = TempDir::new("unit");
        let path = dir.file("a.bin");
        std::fs::write(&path, b"ordinary unit").unwrap();
        let exact = exact_digest(&path).unwrap();
        let unit = unit_digest(&path).unwrap();

        assert_eq!(
            unit,
            single_unit_digest(b"ordinary unit".len() as u64, &exact)
        );
        assert_eq!(unit.version(), UNIT_DIGEST_VERSION);
        assert_ne!(unit.as_ref(), exact.as_ref());
    }
}
