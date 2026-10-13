// src-tauri/src/derive/mod.rs
//! 视频、文档、音频和图像派生共用的可续传、可取消流水线。
//! `pipeline` 负责调度、写入、孤儿恢复及任务优先级；各 kind 负责生成产物。

pub mod audio;
pub mod doc;
pub mod image;
pub mod kind;
pub mod pipeline;
pub mod video;

pub use kind::{DerivationContext, DerivationKind, DerivationOutput};
pub use pipeline::{derivation_counts, start_derivation_pipeline};
