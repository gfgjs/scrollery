// src-tauri/src/layout/mod.rs
pub mod cache;
// justified / grid 共用布局类型、分组、出口拼装和宽高比辅助。
pub mod geometry;
// 均匀宫格布局算法。
pub mod grid_pack;
// 横向画廊布局算法和独立缓存，与 justified/cache 平行。
pub mod hcache;
pub mod horizontal;
// 视图取数缓存：滑块/窗宽/轴切换免重复 SQL。
pub mod items_cache;
pub mod publication;
pub mod justified;
// 重复镜头 groups 模式：组切片、组头打包与逐项投影。
pub mod lens;
// 重复镜头 folders 模式：关联文件夹簇组装与布局打包。
pub mod lens_folder;

pub use cache::{LayoutCache, LayoutCacheData};
pub use hcache::HLayoutCache;
pub use justified::compute_justified_layout;
