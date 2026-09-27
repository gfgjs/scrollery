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
