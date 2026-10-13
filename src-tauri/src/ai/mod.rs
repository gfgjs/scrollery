// src-tauri/src/ai/mod.rs
//! AI 功能的 host 侧入口：CLIP 语义搜索、人脸识别与 OCR。

pub mod clip;
pub mod engine_boot;
pub mod face;
pub mod face_cluster;
pub mod face_pipeline;
pub mod face_profile;
pub mod ocr_registry;
pub mod pipeline;
mod prepared;
pub mod profile;
pub mod provider;
pub mod remote_registry;
pub mod runtime_config;
pub mod search;
pub mod search_control;
pub mod worker_client;
pub mod worker_pipeline;
