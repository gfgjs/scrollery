// src-tauri/src/layout/mod.rs
pub mod cache;
// justified / grid 共享骨架(tierB-4 简案从 justified.rs 拆出):输出类型/LayoutParams/
// 并行组骨架/分组辅助/出口拼装/宽高比辅助。
pub mod geometry;
// 均匀宫格布局算法(T20),tierB-4 简案从 justified.rs 拆出。
pub mod grid_pack;
// H-Lab 横向画廊实验(docs/designs/2026-07-02-horizontal-gallery-lab.md):算法 + 独立缓存,
// 与生产 justified/cache 平行,互不可见。
pub mod hcache;
pub mod horizontal;
// S1 视图取数缓存(Part2 重排提速):把「取数」从「几何」拆出,滑块/窗宽/轴切换免 SQL。
pub mod items_cache;
pub mod publication;
pub mod justified;
// 重复镜头布局(2026-09-02 主画廊重复项浏览方案 §6/§11/§12):groups 模式的组切片
// 组装、duplicateGroup 组头打包与逐项投影;folders 模式是 P3。
pub mod lens;
// 重复镜头 folders 模式(方案 §3.3/§7.1-§7.5,P3 第二步):关联文件夹簇的纯函数组装
// (Union-Find 连通分量 + §7.4 确定性遍历 + §7.5 桶内序),布局打包由下一步任务接入。
pub mod lens_folder;

pub use cache::{LayoutCache, LayoutCacheData};
pub use hcache::HLayoutCache;
pub use justified::compute_justified_layout;
