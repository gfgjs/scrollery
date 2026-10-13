// src-tauri/src/scanner/mod.rs
pub mod enricher;
pub mod fast_scan;
pub(crate) mod fs_watch;
pub(crate) mod hdd_io;
pub mod live_photo;
pub mod metadata;
pub(crate) mod reconcile;
pub mod volume_probe;
pub mod volume_watch;
pub mod walker;

pub use enricher::run_enrichment;
pub use fast_scan::run_fast_scan;
