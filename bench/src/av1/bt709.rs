/// Converts RGB to YUV using BT.709 standard.
#[inline]
pub fn rgb888_to_yuv888(rgb: [u8; 3]) -> [u8; 3] {
    const fn floor(x: f64) -> f64 {
        x as i64 as f64
    }

    const fn round(x: f64) -> f64 {
        floor(x + 0.5)
    }

    const Y_MAX: u32 = 2_u32.pow(16);
    const UV_MAX: i32 = 2_i32.pow(15);

    const fn y_coefs(x0: f64, x1: f64, _x2: f64) -> [u32; 3] {
        let c0 = round(x0 * Y_MAX as f64) as u32;
        let c1 = round(x1 * Y_MAX as f64) as u32;
        // N.B. We don't use the last coefficient to ensure that the sum of all three is
        // 1.
        let c2 = Y_MAX - c0 - c1;
        [c0, c1, c2]
    }

    const fn uv_coefs(x0: f64, x1: f64, denominator: f64) -> [i32; 2] {
        let c0 = round((x0 / denominator) * (224.0 / 219.0) * UV_MAX as f64) as i32;
        let c1 = round((x1 / denominator) * (224.0 / 219.0) * UV_MAX as f64) as i32;
        [c0, c1]
    }

    const fn u_coefs(x0: f64, x1: f64, _x2: f64, denominator: f64) -> [i32; 3] {
        let [c0, c1] = uv_coefs(x0, x1, denominator);
        // N.B. We don't use the last coefficient to ensure that the sum of all three is
        // 0.
        let c2 = -(c0 + c1);
        [c0, c1, c2]
    }

    const fn v_coefs(_x0: f64, x1: f64, x2: f64, denominator: f64) -> [i32; 3] {
        let [c1, c2] = uv_coefs(x1, x2, denominator);
        // N.B. We don't use the first coefficient to ensure that the sum of all three
        // is 0.
        let c0 = -(c1 + c2);
        [c0, c1, c2]
    }

    fn clamp_to_u8(x: i32) -> u8 {
        x.clamp(0, u8::MAX as i32) as u8
    }

    // Calculate discretized coefficients from analogue coefficients.
    const Y: [u32; 3] = y_coefs(0.2126, 0.7152, 0.0722);
    const U: [i32; 3] = u_coefs(-0.2126, -0.7152, 0.9278, 1.8556);
    const V: [i32; 3] = v_coefs(0.7874, -0.7152, -0.0722, 1.5748);

    // Sanity checks.
    const _: () = assert!(Y[0] != 0 && Y[1] != 0 && Y[2] != 0 && Y_MAX == Y[0] + Y[1] + Y[2]);
    const _: () = assert!(U[0] != 0 && U[1] != 0 && U[2] != 0 && 0 == U[0] + U[1] + U[2]);
    const _: () = assert!(V[0] != 0 && V[1] != 0 && V[2] != 0 && 0 == V[0] + V[1] + V[2]);

    let y = (Y[0] * rgb[0] as u32 + Y[1] * rgb[1] as u32 + Y[2] * rgb[2] as u32) / Y_MAX;
    let u = ((U[0] * rgb[0] as i32 + U[1] * rgb[1] as i32 + U[2] * rgb[2] as i32) / UV_MAX) + 128;
    let v = ((V[0] * rgb[0] as i32 + V[1] * rgb[1] as i32 + V[2] * rgb[2] as i32) / UV_MAX) + 128;
    debug_assert!(
        y <= u8::MAX as u32,
        "rgb [{rgb:?}], yuv = [{y}, {u}, {v}], U = {U:?}"
    );
    [y as u8, clamp_to_u8(u), clamp_to_u8(v)]
}

#[derive(Clone, Copy)]
struct SumUv {
    u: u16,
    v: u16,
    count: u8,
}

#[allow(clippy::too_many_arguments)]
pub fn rgb888_to_yuv420p(
    frame: &[u8],
    width: u16,
    height: u16,
    y: &mut [u8],
    y_offset: usize,
    y_stride: usize,
    u: &mut [u8],
    u_offset: usize,
    u_stride: usize,
    v: &mut [u8],
    v_offset: usize,
    v_stride: usize,
) {
    debug_assert_eq!(frame.len(), usize::from(width) * usize::from(height) * 3);
    //debug_assert_eq!(y.len(), (width * height).to_usize());
    //debug_assert_eq!(u.len(), (width.div_ceil(2) *
    // height.div_ceil(2)).to_usize()); debug_assert_eq!(v.len(),
    // (width.div_ceil(2) * height.div_ceil(2)).to_usize());
    let uv_len = u.len();
    let frame_row_len = usize::from(width) * 3;
    let uv_width = usize::from(width).div_ceil(2);
    let uv_height = usize::from(height).div_ceil(2);
    let mut sum_uv = vec![
        SumUv {
            u: 0,
            v: 0,
            count: 0
        };
        uv_len
    ];
    for (i, row) in frame.chunks_exact(frame_row_len).enumerate() {
        for (j, rgb) in row.chunks_exact(3).enumerate() {
            let yuv = rgb888_to_yuv888([rgb[0], rgb[1], rgb[2]]);
            y[y_offset + i * y_stride + j] = yuv[0];
            let sum = &mut sum_uv[(i / 2) * uv_width + (j / 2)];
            sum.u += u16::from(yuv[1]);
            sum.v += u16::from(yuv[2]);
            sum.count += 1;
        }
    }
    for i in 0..uv_height {
        for j in 0..uv_width {
            let sum = &sum_uv[i * uv_width + j];
            u[u_offset + i * u_stride + j] = (sum.u / u16::from(sum.count)) as u8;
            v[v_offset + i * v_stride + j] = (sum.v / u16::from(sum.count)) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Check that the conversion doesn't panic.
    #[test]
    fn no_panic() {
        for r in 0..=255 {
            for g in 0..=255 {
                for b in 0..=255 {
                    let _yuv = rgb888_to_yuv888([r, g, b]);
                }
            }
        }
    }

    #[test]
    fn check_max_residual() {
        // Reference formula from BT.709 standard.
        fn rgb888_to_yuv_bt709_analogue(rgb: [f64; 3]) -> [f64; 3] {
            let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
            let u = -(0.2126 / 1.8556) * rgb[0] - (0.7152 / 1.8556) * rgb[1]
                + (0.9278 / 1.8556) * rgb[2];
            let v = (0.7874 / 1.5748) * rgb[0]
                - (0.7152 / 1.5748) * rgb[1]
                - (0.0722 / 1.5748) * rgb[2];
            [y, u, v]
        }

        fn rgb_u8_to_f64(rgb: [u8; 3]) -> [f64; 3] {
            [
                (rgb[0].saturating_sub(16)) as f64 / 219.0,
                (rgb[1].saturating_sub(16)) as f64 / 219.0,
                (rgb[2].saturating_sub(16)) as f64 / 219.0,
            ]
        }

        fn yuv_u8_to_f64(yuv: [u8; 3]) -> [f64; 3] {
            [
                (yuv[0] as f64 - 16.0).max(0.0) / 219.0,
                (yuv[1] as f64 - 128.0) / 224.0,
                (yuv[2] as f64 - 128.0) / 224.0,
            ]
        }

        // Check that the discretized formula produces the same result as analogue one.
        let mut max_residual = [0.0; 3];
        for r in 16..=235 {
            for g in 16..=235 {
                for b in 16..=235 {
                    let yuv = rgb888_to_yuv888([r, g, b]);
                    let yuv_discretized = yuv_u8_to_f64(yuv);
                    let yuv_analogue = rgb888_to_yuv_bt709_analogue(rgb_u8_to_f64([r, g, b]));
                    let residual = [
                        (yuv_discretized[0] - yuv_analogue[0]).abs(),
                        (yuv_discretized[1] - yuv_analogue[1]).abs(),
                        (yuv_discretized[2] - yuv_analogue[2]).abs(),
                    ];
                    for i in 0..3 {
                        if residual[i] > max_residual[i] {
                            max_residual[i] = residual[i];
                        }
                    }
                }
            }
        }
        assert!(
            max_residual.iter().all(|r| *r < 0.005),
            "max_residual = {max_residual:?}"
        )
    }
}
