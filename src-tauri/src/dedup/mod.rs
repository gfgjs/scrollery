//! 精确内容去重的只读摘要核心。
//!
//! 该模块由 crate 根公开接线，调用方可直接使用这里 re-export 的 API。
//! `utils::hash::content_fingerprint` 仍然保留其扫描变更指纹语义，不在此模块中复用。

pub mod folder_cache;
pub mod hash;
pub mod task;

pub use hash::{
    exact_digest, exact_digest_with_snapshot, exact_digest_with_snapshot_cancelled, file_snapshot,
    live_photo_unit_digest, live_photo_unit_digest_with_components,
    live_photo_unit_digest_with_components_cancelled, physical_key, quick_digest,
    quick_digest_with_snapshot, quick_digest_with_snapshot_cancelled, single_unit_digest,
    unit_digest, ExactDigest, ExactFileDigest, FileSnapshot, HashError, IoOperation,
    LivePhotoDigestComponents, PhysicalIdentity, QuickDigest, QuickFileDigest, UnitDigest,
    DEDUP_HASH_VERSION, EXACT_DIGEST_VERSION, HASH_BUFFER_SIZE, QUICK_DIGEST_VERSION,
    QUICK_SAMPLE_SIZE, UNIT_DIGEST_VERSION,
};
