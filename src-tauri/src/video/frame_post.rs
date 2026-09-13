// src-tauri/src/video/frame_post.rs
//! Media Foundation 解码帧的后处理（尾段抽出，全程不碰 `IMFSourceReader`，与
//! `Session`/`ReaderCallback` 等异步回调核心无引用关系）。
//!
//! Post-processing for decoded MF frames — extracted from `media_foundation.rs`; never touches
//! `IMFSourceReader`, no reference into the async-callback core.

use crate::engine::traits::DecodedImage;

/// 按 `rotation` 度（顺时针）把解码帧旋转正立，90/270 时交换宽高。
pub(super) fn apply_rotation(img: DecodedImage, rotation: i32) -> DecodedImage {
    if rotation == 0 {
        return img;
    }
    let Some(rgba) = image::RgbaImage::from_raw(img.width, img.height, img.pixels) else {
        // 缓冲尺寸不符（理论上不会）：原样返回，避免 panic。
        return DecodedImage {
            pixels: Vec::new(),
            width: 0,
            height: 0,
            icc: None,
        };
    };
    let dyn_img = image::DynamicImage::ImageRgba8(rgba);
    let rotated = match rotation {
        90 => dyn_img.rotate90(),
        180 => dyn_img.rotate180(),
        270 => dyn_img.rotate270(),
        _ => dyn_img,
    };
    let out = rotated.to_rgba8();
    let (w, h) = (out.width(), out.height());
    DecodedImage {
        pixels: out.into_raw(),
        width: w,
        height: h,
        icc: None, // 视频帧无 ICC 来源,按 sRGB 假定
    }
}

/// 将解码帧规整到 `tw × th`（统一雪碧格）。尺寸相等零拷贝直通（XVP 协商命中的快路径）。
pub(super) fn resize_rgba(img: DecodedImage, tw: u32, th: u32) -> DecodedImage {
    if img.width == tw && img.height == th {
        return img;
    }
    use fast_image_resize::images::{Image as FirImage, ImageRef};
    use fast_image_resize::pixels::PixelType;
    use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};

    // ImageRef 以不可变借用包源缓冲(旧实现为满足 from_slice_u8 的 &mut 克隆整帧,已免)。
    let Ok(src) = ImageRef::new(
        img.width.max(1),
        img.height.max(1),
        &img.pixels,
        PixelType::U8x4,
    ) else {
        return img;
    };
    let mut dst = FirImage::new(tw.max(1), th.max(1), PixelType::U8x4);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    let mut resizer = Resizer::new();
    if resizer.resize(&src, &mut dst, &opts).is_err() {
        return img;
    }
    DecodedImage {
        pixels: dst.into_vec(),
        width: tw,
        height: th,
        icc: None, // 视频帧无 ICC 来源,按 sRGB 假定
    }
}

/// 廉价的「该帧是否接近全黑」判断（封面跳过片头黑帧）。
pub(super) fn is_too_dark(img: &DecodedImage) -> bool {
    if img.pixels.len() < 4 {
        return true;
    }
    // 每隔 64 个像素采样其亮度；廉价且足够。
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let mut i = 0;
    while i + 3 < img.pixels.len() {
        let r = img.pixels[i] as u64;
        let g = img.pixels[i + 1] as u64;
        let b = img.pixels[i + 2] as u64;
        sum += (r * 299 + g * 587 + b * 114) / 1000;
        count += 1;
        i += 4 * 64;
    }
    if count == 0 {
        return true;
    }
    (sum / count) < 16 // 平均亮度 < 16（0-255）视为黑帧
}

/// 把 MF 输出的 RGB32（内存序 B,G,R,X，每像素 4B，可能 bottom-up + stride 对齐填充）
/// 转成 top-down 的紧凑 RGBA。抽成纯函数（输入 `src` 切片）以便对边界单测。
///
/// 🔴 边界守卫（Part3 Q16 / §3.8.1）：行内像素 `x` 的最大访问下标是 `x*4+2`（R 通道），
/// 故缓冲行尾余 **3 字节**（恰含 B,G,R）时该像素仍须拷贝、不得填黑;不足 3 字节的残像素
/// 与其后像素留零（黑），截断缓冲不得 panic。
/// （2026-07-17 性能线重写为行级 `as_chunks`（定长数组块）：主体无逐像素边界检查，可被编译器向量化;
/// 尾像素单独按上述守卫补拷。语义与旧逐像素实现逐位一致，由下方四个单测钉住。）
pub(super) fn copy_bgr32_to_rgba(
    src: &[u8],
    w: usize,
    h: usize,
    abs_stride: usize,
    bottom_up: bool,
) -> Vec<u8> {
    let cur = src.len();
    let mut out = vec![0u8; w * h * 4];
    for row in 0..h {
        // bottom-up 帧：图像第 0 行（顶部）是内存中的最后一行。
        let mem_row = if bottom_up { h - 1 - row } else { row };
        let src_off = mem_row * abs_stride;
        if src_off >= cur {
            continue; // 整行在缓冲之外（截断帧）：留零，处理下一图像行
        }
        let avail = cur - src_off;
        // 行内完整 4B 像素数（不超过行宽 w）。
        let full = (avail / 4).min(w);
        let dst_off = row * w * 4;
        let src_row = &src[src_off..src_off + full * 4];
        let dst_row = &mut out[dst_off..dst_off + full * 4];
        for (d, s) in dst_row
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src_row.as_chunks::<4>().0)
        {
            d[0] = s[2]; // R ← byte[2]
            d[1] = s[1]; // G ← byte[1]
            d[2] = s[0]; // B ← byte[0]
            d[3] = 255; // A (RGB32 has no alpha)
        }
        // 尾像素守卫：行未拷满且缓冲恰余 3 字节（B,G,R 齐）→ 按旧语义补拷该像素。
        if full < w && avail % 4 == 3 {
            let s = src_off + full * 4;
            let d = dst_off + full * 4;
            out[d] = src[s + 2];
            out[d + 1] = src[s + 1];
            out[d + 2] = src[s];
            out[d + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::copy_bgr32_to_rgba;

    /// 🔴 回归：紧凑对齐（stride==w*4）下，缓冲区恰好覆盖到最后一个像素的 R 通道
    /// （行尾余 3 字节）时，尾像素必须被拷贝、不得填黑。旧 `>=` 守卫会丢这一像素。
    #[test]
    fn tail_pixel_copied_when_buffer_ends_exactly() {
        // w=2,h=1：像素0 用字节[0..3]，像素1 用字节[4..6]（B,G,R）。
        // cur=7 → 像素1 只剩 3 字节（恰含 B,G,R）：守卫要求仍拷贝。
        let src = [10u8, 11, 12, 99, 40, 50, 60]; // 7 字节
        let out = copy_bgr32_to_rgba(&src, 2, 1, 8, false);
        // 像素0：R=src[2],G=src[1],B=src[0]
        assert_eq!(&out[0..4], &[12, 11, 10, 255]);
        // 像素1（尾像素）：R=src[6],G=src[5],B=src[4] —— 不得为黑
        assert_eq!(&out[4..8], &[60, 50, 40, 255]);
    }

    /// bottom-up 帧（stride<0 → 此处 bottom_up=true）：图像第 0 行取自内存最后一行。
    #[test]
    fn bottom_up_reverses_row_order() {
        // w=1,h=2,stride=4：内存行0=[1,2,3,0]，内存行1=[4,5,6,0]。
        let src = [1u8, 2, 3, 0, 4, 5, 6, 0];
        let out = copy_bgr32_to_rgba(&src, 1, 2, 4, true);
        // 图像行0 = 内存行1：R=6,G=5,B=4
        assert_eq!(&out[0..4], &[6, 5, 4, 255]);
        // 图像行1 = 内存行0：R=3,G=2,B=1
        assert_eq!(&out[4..8], &[3, 2, 1, 255]);
    }

    /// stride 含填充（abs_stride > w*4）：每行尾部 padding 字节被跳过，不污染输出。
    #[test]
    fn padded_stride_skips_alignment_bytes() {
        // w=1,h=2,abs_stride=8（4 像素数据 + 4 填充）。
        let src = [9u8, 8, 7, 0, 0xAA, 0xBB, 0xCC, 0xDD, 6, 5, 4, 0, 0, 0, 0, 0];
        let out = copy_bgr32_to_rgba(&src, 1, 2, 8, false);
        assert_eq!(&out[0..4], &[7, 8, 9, 255]); // 行0 像素：R=7,G=8,B=9
        assert_eq!(&out[4..8], &[4, 5, 6, 255]); // 行1 像素：R=4,G=5,B=6
    }

    /// 损坏/截断缓冲区（cur 远小于 w*h*4）：守卫保证不 panic，未覆盖区域留零（黑），
    /// 已覆盖的前缀像素仍正确拷贝。
    #[test]
    fn truncated_buffer_does_not_panic() {
        // 声称 2×2，但只给 6 字节（不足 1.5 像素）。
        let src = [10u8, 20, 30, 0, 40, 50];
        let out = copy_bgr32_to_rgba(&src, 2, 2, 8, false);
        assert_eq!(out.len(), 2 * 2 * 4);
        // 像素(0,0)：s=0，4 字节完整 → 拷贝 R=30,G=20,B=10
        assert_eq!(&out[0..4], &[30, 20, 10, 255]);
        // 像素(0,1)：仅余 2 字节(<3) → 留零
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
    }
}
