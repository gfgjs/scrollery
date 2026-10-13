//! 编辑保存的峰值内存预算。
//!
//! 用 `examples/edit_memory_probe.rs` 在 24/50/100MP + 40MP 长边样本上实测
//! `decode→rotate90→fliph→flipv→crop→encode(JPEG q92)` 组合的 Windows `PeakWorkingSetSize`
//! 增量。
//!
//! 测量源文件需在独立进程生成，避免合成源预提交页面被测量阶段复用而低估增量。
//! JPEG 完整处理链在三个独立取样点约为 **6.02 字节/像素**；主峰发生在 rotate90 的
//! 新旧像素缓冲共存时。
//! 探针 full 链的 crop 使 encode 阶段输入减半;真实「无 crop 保存」的 encode 阶段活缓冲为
//! flipv 输出(3 B/px)+ 压缩输出 Vec(<0.5 B/px),仍低于 rotate 时刻主峰,不改变 6 B/px 结论。
//!
//! 预算取实测值的约两倍，以覆盖内容差异和未测平台：
//! [`EDIT_PEAK_BYTES_PER_PIXEL_BUDGET`] = 12。
//! [`EDIT_PEAK_BYTES_CEILING`] 为工程预算 1.5 GB，并非由基准直接推导；两常量对应 125MP 准入上限。

/// 单像素峰值内存预算(字节)= 实测 ≈6.02 × 2 安全系数,详见模块文档。
pub const EDIT_PEAK_BYTES_PER_PIXEL_BUDGET: u64 = 12;

/// 单次编辑保存允许占用的峰值内存上限(字节)。约 1.4 GiB——工程判断,见模块文档。
pub const EDIT_PEAK_BYTES_CEILING: u64 = 1_500_000_000;

/// 超限的稳定错误码(方案 §6)。
pub const CODE_TOO_LARGE: &str = "edit_image_too_large";

/// 按 `width*height*预算系数` 估算峰值字节数;溢出返回 `None`(调用方应按超限处理,不能当作 0)。
pub fn predicted_peak_bytes(width: u32, height: u32) -> Option<u64> {
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(EDIT_PEAK_BYTES_PER_PIXEL_BUDGET)
}

/// 是否超出内存预算(含溢出情形)。在任何大块分配之前调用,超限直接拒绝(`CODE_TOO_LARGE`)。
pub fn exceeds_memory_budget(width: u32, height: u32) -> bool {
    match predicted_peak_bytes(width, height) {
        Some(bytes) => bytes > EDIT_PEAK_BYTES_CEILING,
        None => true,
    }
}

/// fine rotate 会先分配展开缓冲，准入必须按展开尺寸而非原图面积判断。
/// 尺寸计算溢出同样按超限处理。
pub fn exceeds_memory_budget_after_fine_rotate(
    width: u32,
    height: u32,
    angle_degrees: f64,
) -> bool {
    crate::editing::geometry::expanded_dimensions(width, height, angle_degrees)
        .map(|(expanded_width, expanded_height)| {
            exceeds_memory_budget(expanded_width, expanded_height)
        })
        .unwrap_or(true)
}
