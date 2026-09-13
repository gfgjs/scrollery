// crates/scrollery-ai-core/src/enhance/tiling.rs
//! 影像增强 tiling 几何(纯几何,零 ort,不门控 feature)。
//!
//! 把源图切成固定 `tile×tile`(默认 512²)的重叠瓦片:每瓦片喂模型跑一次,
//! 融合时**只取中心有效区**拼接,重叠 pad 区丢弃——消除边界接缝(design.md §E)。
//!
//! # 联动契约(D-439)
//! `tile` 与导出 ONNX 的**静态输入 shape**(`1×3×512×512`)是**联动契约**:
//! 改 `tile` 必须重导模型,否则 tiling 产出的瓦片尺寸与模型输入 shape 不符,推理直接失败。
//!
//! # 切分方案(clamp-short 非重叠分区)
//! 沿每轴独立切:步进 `step = tile − 2·pad`(=480),core(有效区)起点 `0, step, 2·step, …`,
//! 末瓦片 core 长度**截短**到贴源边(`min(step, len − start)`),使各轴 core 恰好**无缝无叠**
//! 分区 `[0, len)`。故 dst 矩形天然两两不相交且全覆盖 `[0, len·scale)`——**无需**重叠区归属
//! 仲裁(选此方案即因它最简且可证零缝零重写)。末瓦片 `src_rect` 会向源外越界(右/下),
//! 由采样层 reflect 补齐;core 右/下边恰在源边界,reflect 即正确的边缘处理。
//! 小图(某轴 `len ≤ step`)该轴单瓦片,`src_rect` 为 `[0, tile)`,越界部分 reflect 补齐。
//!
//! # 越界与坐标类型
//! `src_rect.x/y` 可为**负**或超出源尺寸(reflect 由 [`chain`](super::chain) 采样处理),故用 `i64`;
//! `core_rect`/`dst_rect` 起点非负,但下标运算须防 100MP 溢出——消费方一律以 `usize/i64` 展开
//! (见 chain 采样/写回),本模块只产出矩形不做像素索引。

/// 一个轴对齐矩形。`x/y` 用 `i64` 以表达 `src_rect` 的越界(负/超界)坐标;`w/h` 恒非负。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// 单个瓦片的三矩形契约。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileSpec {
    /// 采样窗口(源坐标系,恒 `tile×tile`;可越界,越界由采样层 reflect 补齐)。
    pub src_rect: Rect,
    /// 源内有效区(core;各瓦片的 core 无缝无叠分区整图)。
    pub core_rect: Rect,
    /// core 映射到输出画布的矩形(= `core × scale`;两两不相交、全覆盖输出)。
    pub dst_rect: Rect,
}

/// 沿一个轴切 core:返回各瓦片 `(core_start, core_len)`,恰好无缝无叠分区 `[0, len)`。
/// `len == 0` 返回空;`len ≤ step` 单瓦片(整轴一片)。
fn axis_tiles(len: u32, tile: u32, pad: u32) -> Vec<(u32, u32)> {
    if len == 0 {
        return Vec::new();
    }
    // 步进 = tile − 2·pad;非法参数(2·pad ≥ tile)饱和到 1 防死循环(生产用 512/16 恒 480)。
    let step = tile.saturating_sub(2 * pad).max(1);
    if len <= step {
        return vec![(0, len)];
    }
    let mut out = Vec::new();
    let mut cs = 0u32;
    while cs < len {
        let core_len = step.min(len - cs);
        out.push((cs, core_len));
        cs += step;
    }
    out
}

/// 规划整图瓦片。`tile`(512)/`pad`(16)/`scale`(1 或 4)见 design.md §E。
///
/// 各瓦片 `src_rect` 恒 `tile×tile`(静态 shape),起点 = `core 起点 − pad`(可越界);
/// `dst_rect = core × scale`。两轴独立切后取笛卡尔积。`src_w/src_h == 0` 返回空。
pub fn plan_tiles(src_w: u32, src_h: u32, tile: u32, pad: u32, scale: u32) -> Vec<TileSpec> {
    let xs = axis_tiles(src_w, tile, pad);
    let ys = axis_tiles(src_h, tile, pad);
    let mut out = Vec::with_capacity(xs.len() * ys.len());
    let pad_i = pad as i64;
    let scale_i = scale as i64;
    for &(cy, ch) in &ys {
        for &(cx, cw) in &xs {
            let src_rect = Rect {
                x: cx as i64 - pad_i,
                y: cy as i64 - pad_i,
                w: tile,
                h: tile,
            };
            let core_rect = Rect {
                x: cx as i64,
                y: cy as i64,
                w: cw,
                h: ch,
            };
            let dst_rect = Rect {
                // i64 下标运算:cx/cy 可达 100MP 单边,×scale 仍稳落 i64。
                x: cx as i64 * scale_i,
                y: cy as i64 * scale_i,
                w: cw * scale,
                h: ch * scale,
            };
            out.push(TileSpec {
                src_rect,
                core_rect,
                dst_rect,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生产几何:512/16 → step 480。
    const T: u32 = 512;
    const P: u32 = 16;

    #[test]
    fn axis_exact_multiple_of_step() {
        // len = 960 = 2·480 → 两片 [0,480) [480,480),无截短。
        let v = axis_tiles(960, T, P);
        assert_eq!(v, vec![(0, 480), (480, 480)]);
    }

    #[test]
    fn axis_remainder_clamps_last() {
        // len = 1000 → [0,480) [480,480) [960,40)。
        let v = axis_tiles(1000, T, P);
        assert_eq!(v, vec![(0, 480), (480, 480), (960, 40)]);
    }

    #[test]
    fn axis_small_single_tile() {
        assert_eq!(axis_tiles(480, T, P), vec![(0, 480)]); // 恰等 step 仍单片
        assert_eq!(axis_tiles(300, T, P), vec![(0, 300)]);
        assert_eq!(axis_tiles(1, T, P), vec![(0, 1)]); // 1×1 极端
    }

    #[test]
    fn axis_between_step_and_tile() {
        // 480 < len ≤ 512:仍两片(末片 mostly-reflect),接受(design 只在 len ≤ step 单片)。
        assert_eq!(axis_tiles(500, T, P), vec![(0, 480), (480, 20)]);
    }

    #[test]
    fn axis_empty_on_zero() {
        assert!(axis_tiles(0, T, P).is_empty());
    }

    #[test]
    fn plan_empty_on_zero_dim() {
        assert!(plan_tiles(0, 100, T, P, 1).is_empty());
        assert!(plan_tiles(100, 0, T, P, 1).is_empty());
    }

    #[test]
    fn src_rect_first_tile_offset_is_negative_pad() {
        // 首瓦片 src 左/上越界为 −pad(其余瓦片的 src/core 偏移不变量已并入
        // `assert_full_disjoint_coverage` 的参数化循环全数断言,见下)。
        let tiles = plan_tiles(1000, 1000, T, P, 4);
        assert_eq!(tiles[0].src_rect.x, -(P as i64));
        assert_eq!(tiles[0].src_rect.y, -(P as i64));
    }

    #[test]
    fn dst_maps_core_by_scale_1_and_4() {
        // scale 1:dst == core。
        let t1 = plan_tiles(600, 600, T, P, 1);
        for ts in &t1 {
            assert_eq!(ts.dst_rect.x, ts.core_rect.x);
            assert_eq!(ts.dst_rect.w, ts.core_rect.w);
        }
        // scale 4:dst = core·4。
        let t4 = plan_tiles(600, 600, T, P, 4);
        for ts in &t4 {
            assert_eq!(ts.dst_rect.x, ts.core_rect.x * 4);
            assert_eq!(ts.dst_rect.w, ts.core_rect.w * 4);
            assert_eq!(ts.dst_rect.h, ts.core_rect.h * 4);
        }
    }

    /// 参数化全覆盖检查:dst 矩形须**无缝无叠**铺满 `[0, w·scale)×[0, h·scale)`。
    /// 用布尔网格逐像素计数,断言每格恰被覆盖一次(零缝隙 ∧ 零重写)。
    fn assert_full_disjoint_coverage(w: u32, h: u32, tile: u32, pad: u32, scale: u32) {
        let tiles = plan_tiles(w, h, tile, pad, scale);
        let ow = (w * scale) as usize;
        let oh = (h * scale) as usize;
        let mut hit = vec![0u32; ow * oh];
        for ts in &tiles {
            // src 起点 = core 起点 − pad(采样层局部偏移恒 = pad·scale 依赖此不变量);
            // 全数断言(每个瓦片,含物理边缘越界的),而非仅抽样单个用例。
            assert_eq!(ts.src_rect.w, tile);
            assert_eq!(ts.src_rect.h, tile);
            assert_eq!(ts.src_rect.x, ts.core_rect.x - pad as i64);
            assert_eq!(ts.src_rect.y, ts.core_rect.y - pad as i64);

            let (dx, dy) = (ts.dst_rect.x as usize, ts.dst_rect.y as usize);
            for ry in 0..ts.dst_rect.h as usize {
                for rx in 0..ts.dst_rect.w as usize {
                    hit[(dy + ry) * ow + (dx + rx)] += 1;
                }
            }
        }
        assert!(
            hit.iter().all(|&c| c == 1),
            "覆盖异常 w={w} h={h} tile={tile} pad={pad} scale={scale}: 存在缝隙(0)或重写(>1)"
        );
    }

    #[test]
    fn coverage_generic_small_params() {
        // 用小数值(tile 8 pad 1 → step 6)验几何通性:整除/余数/贴边/小图/1×1。
        for &(w, h) in &[
            (1u32, 1u32),
            (6, 6),   // 恰等 step
            (7, 7),   // step+1
            (12, 12), // 2·step
            (13, 5),  // 余数 × 小图轴
            (20, 17),
            (1, 30),
            (30, 1),
        ] {
            for &scale in &[1u32, 4] {
                assert_full_disjoint_coverage(w, h, 8, 1, scale);
            }
        }
    }

    #[test]
    fn coverage_production_geometry() {
        // 生产 512/16 几何:整除/余数/小图/贴边多尺寸,scale 1 与 4。
        for &(w, h) in &[
            (100u32, 100u32),
            (512, 512),
            (960, 960),
            (1000, 700),
            (1920, 1080),
            (513, 481),
        ] {
            assert_full_disjoint_coverage(w, h, T, P, 1);
            assert_full_disjoint_coverage(w, h, T, P, 4);
        }
    }
}
