// src-tauri/src/ai/clip.rs
//! host 侧的嵌入向量转换工具，复用 ai-core 的 `embedding` 模块。
//! CLIP 推理由 ai-worker 子进程执行。

pub use scrollery_ai_core::embedding::{bytes_to_embedding, embedding_to_bytes, l2_normalize};
