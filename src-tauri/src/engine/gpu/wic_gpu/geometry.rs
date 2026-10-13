//! 不触碰设备的平面、方向及回读布局计算。

pub(super) const MAX_TEXTURE: u32 = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PlaneLayout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bytes: u32,
}

impl PlaneLayout {
    pub fn new(width: u32, height: u32, channels: u32) -> Option<Self> {
        if width == 0 || height == 0 || width > MAX_TEXTURE || height > MAX_TEXTURE {
            return None;
        }
        let stride = width.checked_mul(channels)?;
        let bytes = stride.checked_mul(height)?;
        Some(Self {
            width,
            height,
            stride,
            bytes,
        })
    }

    pub fn reserved_bytes(self) -> u64 {
        // 驱动纹理行距不可预先查询；按 256 字节行、4 KiB 分配页估算并配合 DXGI 实际用量。
        let rows = (u64::from(self.stride) + 255) & !255;
        (rows * u64::from(self.height) + 4095) & !4095
    }
}

pub(super) fn supported_chroma(y: PlaneLayout, c: PlaneLayout) -> bool {
    (c.width == y.width || c.width == y.width.div_ceil(2))
        && (c.height == y.height || c.height == y.height.div_ceil(2))
}

pub(super) fn oriented_size(target: (u32, u32), orientation: u32) -> Option<(u32, u32)> {
    PlaneLayout::new(target.0, target.1, 4)?;
    match orientation {
        1..=4 => Some(target),
        5..=8 => Some((target.1, target.0)),
        _ => None,
    }
}

/// 行向量矩阵；输入图像边界精确映射至最终目标边界，平移使用像素边界而非像素索引。
pub(super) fn orientation_matrix(
    source: (u32, u32),
    target: (u32, u32),
    orientation: u32,
) -> Option<[f32; 6]> {
    oriented_size(target, orientation)?;
    if source.0 == 0 || source.1 == 0 {
        return None;
    }
    let (w, h) = (target.0 as f32, target.1 as f32);
    let (sx, sy) = (w / source.0 as f32, h / source.1 as f32);
    Some(match orientation {
        1 => [sx, 0., 0., sy, 0., 0.],
        2 => [-sx, 0., 0., sy, w, 0.],
        3 => [-sx, 0., 0., -sy, w, h],
        4 => [sx, 0., 0., -sy, 0., h],
        5 => [0., sx, sy, 0., 0., 0.],
        6 => [0., sx, -sy, 0., h, 0.],
        7 => [0., -sx, -sy, 0., h, w],
        8 => [0., -sx, sy, 0., 0., w],
        _ => return None,
    })
}

pub(super) fn row_extent(width: u32, height: u32, pitch: u32) -> Option<usize> {
    let row = width.checked_mul(4)?;
    if width == 0 || height == 0 || pitch < row {
        return None;
    }
    let extent = u64::from(height - 1) * u64::from(pitch) + u64::from(row);
    if extent > isize::MAX as u64 {
        return None;
    }
    usize::try_from(extent).ok()
}

pub(super) fn copy_rgba_rows(
    source: &[u8],
    width: u32,
    height: u32,
    pitch: u32,
) -> Option<Vec<u8>> {
    let extent = row_extent(width, height, pitch)?;
    let row = usize::try_from(width.checked_mul(4)?).ok()?;
    let bytes = row.checked_mul(height as usize)?;
    if source.len() < extent {
        return None;
    }
    let mut output = Vec::new();
    output.try_reserve_exact(bytes).ok()?;
    // GPU 已生成最终通道顺序；批量追加既避开逐像素循环，也不预先清零待覆盖的缓冲。
    if pitch as usize == row {
        output.extend_from_slice(&source[..bytes]);
    } else {
        for input in source[..extent].chunks(pitch as usize) {
            output.extend_from_slice(&input[..row]);
        }
    }
    Some(output)
}

pub(super) fn fits_budget(reserved: u64, additional: u64, cap: u64, available: u64) -> bool {
    reserved
        .checked_add(additional)
        .is_some_and(|total| total <= cap)
        && additional <= available
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corners(orientation: u32) -> [(f32, f32); 4] {
        let m = orientation_matrix((8, 4), (4, 2), orientation).unwrap();
        [(0., 0.), (8., 0.), (0., 4.), (8., 4.)]
            .map(|(x, y)| (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5]))
    }

    #[test]
    fn exif_6_clockwise_bounds() {
        assert_eq!(corners(6), [(2., 0.), (2., 4.), (0., 0.), (0., 4.)]);
    }
    #[test]
    fn planar_size_and_overflow_limits() {
        let plane = PlaneLayout::new(513, 257, 2).unwrap();
        assert_eq!((plane.stride, plane.bytes), (1026, 263682));
        assert!(PlaneLayout::new(16385, 1, 1).is_none());
        assert!(PlaneLayout::new(1, 0, 1).is_none());
        assert!(PlaneLayout::new(16384, 16384, 16).is_none());
    }

    #[test]
    fn readback_respects_pitch_and_rgba_channels() {
        let source = [1, 2, 3, 255, 99, 99, 99, 99, 4, 5, 6, 255];
        assert_eq!(
            copy_rgba_rows(&source, 1, 2, 8),
            Some(vec![1, 2, 3, 255, 4, 5, 6, 255])
        );
        assert_eq!(copy_rgba_rows(&source, 1, 2, 3), None);
        assert_eq!(copy_rgba_rows(&source[..11], 1, 2, 8), None);
    }
}
