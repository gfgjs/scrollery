//! 编辑保存的峰值内存预算(方案 C §5,P0 spike)。
//!
//! 用 `examples/edit_memory_probe.rs` 在 24/50/100MP + 40MP 长边样本上实测
//! `decode→rotate90→fliph→flipv→crop→encode(JPEG q92)` 组合的 Windows `PeakWorkingSetSize`
//! 增量(详见 `docs/worklogs/2026-07-19-图片简单编辑施工/findings.md`)。
//!
//! **2026-07-19 复审修正**:首轮读数(JPEG ≈2.95 B/px)在测量进程内合成源图,setup 阶段已把
//! 一个像素缓冲量级的页面预提交进工作集、被 measure 阶段 allocator 复用,delta 被系统性腰斩
//! ——与首轮已识别的 PNG/WebP 伪影同源,只是当时未对 JPEG 应用同一怀疑。改为两段进程
//! (`--emit-source` 生成文件 + `--source-path` 只读文件测量)后,JPEG full 链在 24/50/100MP
//! 三个独立取样点均为 **≈6.02 字节/像素**(decode-only ≈3.17,即一个 RGB8 缓冲;full 链主峰
//! 出现在 rotate90 时刻新旧两个像素缓冲共存),线性一致,与分析吻合,采信为格式无关基线。
//! 探针 full 链的 crop 使 encode 阶段输入减半;真实「无 crop 保存」的 encode 阶段活缓冲为
//! flipv 输出(3 B/px)+ 压缩输出 Vec(<0.5 B/px),仍低于 rotate 时刻主峰,不改变 6 B/px 结论。
//!
//! 预算取「已验证的最坏值 × 2 安全系数」(覆盖内容相关方差与非 Windows 平台的未测行为):
//! [`EDIT_PEAK_BYTES_PER_PIXEL_BUDGET`] = 12。
//! [`EDIT_PEAK_BYTES_CEILING`] 是**尚未拿到目标设备预算输入前的工程判断**,不是从基准直接推导的
//! 硬数字——标 1.4 GiB 是「常见消费级设备≥8GB内存、单次前台编辑保存瞬时独占这个量级可接受」的
//! 保守估计,后续有明确设备预算后应重新核定(不是本 spike 能替用户拍板的事)。当前两常量合起来
//! 的准入上限 = 1.5e9/12 = 125MP,仍覆盖 100MP 基准样本。

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

/// D-008/D-107：fine rotate 会先分配展开缓冲，准入必须按展开尺寸而非原图面积判断。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_photo_sizes_are_within_budget() {
        assert!(!exceeds_memory_budget(6000, 4000)); // 24MP
        assert!(!exceeds_memory_budget(10000, 5000)); // 50MP
    }

    #[test]
    fn boundary_is_computed_not_guessed() {
        let max_pixels = EDIT_PEAK_BYTES_CEILING / EDIT_PEAK_BYTES_PER_PIXEL_BUDGET;
        let side = (max_pixels as f64).sqrt() as u32;
        assert!(!exceeds_memory_budget(side, side));
        // 每边各 +2%,面积超出比例远大于系数误差,必须稳定越界——防止阈值判断悄悄失效。
        let over = side + side / 50 + 1;
        assert!(exceeds_memory_budget(over, over));
    }

    #[test]
    fn extreme_dimensions_overflow_to_rejected_not_panic() {
        assert!(exceeds_memory_budget(u32::MAX, u32::MAX));
        assert!(predicted_peak_bytes(u32::MAX, u32::MAX).is_none());
    }

    #[test]
    fn zero_dimension_is_within_budget_but_not_meaningful() {
        // 零像素本身该在更早的「零面积裁剪」校验(方案 §6 crop_empty)拒绝,不归内存预算管;
        // 这里只确认不会因 0 触发 checked_mul 的意外路径。
        assert!(!exceeds_memory_budget(0, 0));
    }

    #[test]
    fn fine_rotate_budget_uses_expanded_area() {
        // 100MP 原图按 12 B/px 尚在 1.5GB 内；45° 展开接近 200MP，必须在解码前拒绝。
        assert!(!exceeds_memory_budget(10_000, 10_000));
        assert!(exceeds_memory_budget_after_fine_rotate(
            10_000, 10_000, 45.0
        ));
        assert!(!exceeds_memory_budget_after_fine_rotate(
            10_000, 10_000, 0.0
        ));
    }
}
