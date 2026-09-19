// src-tauri/src/ai/clip.rs
//! 再导出薄壳(T16 收束):CLIP 推理面(预处理/编码/分词)已随进程内引擎退场,
//! 推理恒在 ai-worker 子进程。host 仅消费**嵌入字节序纯件**(聚类/
//! 搜索打分/faces 落库共用),来自 ai-core 的 `embedding` 模块(不在 `inference`
//! feature 门内,S4 关默认特性后仍可用);引用路径 `crate::ai::clip::*` 不变。

pub use scrollery_ai_core::embedding::{bytes_to_embedding, embedding_to_bytes, l2_normalize};
