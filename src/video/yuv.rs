use core::num::NonZero;

#[cfg(test)]
pub const UV_MIN: i16 = -255;
#[cfg(test)]
pub const UV_MAX: i16 = 255;

/// Convert RGB to YUV.
///
/// This function uses fast and reversible integer-based conversion from
/// _"PGF – A new progressive file format for lossy and lossless image
/// compression"_ (2002) by Christoph Stamm.
///
/// _Y_ is in the range _[-128; 127], _U_ and _V_ are in the range _[-255;
/// 255]_.
#[inline]
pub fn rgb_to_yuv([r, g, b]: [u8; 3]) -> [i16; 3] {
    let r = i16::from(r);
    let g = i16::from(g);
    let b = i16::from(b);
    let y = ((r + (g << 1) + b) >> 2) - 128;
    let u = r - g;
    let v = b - g;
    [y, u, v]
}

/// Convert YUV to RGB.
///
/// This function uses fast and reversible integer-based conversion.
/// Any resulting out-of-range RGB values are clamped.
/// If YUV values are out-of-range, the result is undefined.
#[inline]
pub fn yuv_to_rgb([y, u, v]: [i16; 3]) -> [u8; 3] {
    let g = y - ((u + v) >> 2) + 128;
    let r = u + g;
    let b = v + g;
    [clamp_u8(r), clamp_u8(g), clamp_u8(b)]
}

#[inline]
fn clamp_u8(value: i16) -> u8 {
    if value < 0 {
        return 0;
    }
    if value > i16::from(u8::MAX) {
        return u8::MAX;
    }
    value as u8
}

const NUM_RGB_COMPONENTS: usize = 3;

/// Converts RGB frame to planar YUV frame using [`rgb_to_yuv`] with 4:2:0
/// chroma subsampling.
///
/// If slices' lengths aren't correct the output is undefined; the function will
/// only panic when debug assertions are enabled.
pub fn rgb888_to_yuv420p(
    rgb_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    fn div_by_power_of_two(value: i16, q: u8) -> i16 {
        let sign_bit = value >> 15;
        (value + (sign_bit & ((1_i16 << q) - 1))) >> q
    }
    #[cfg(debug_assertions)]
    {
        debug_assert_eq!(y.len(), rgb_frame.len() / 3);
        debug_assert!(rgb_frame.len() / 3 <= u32::MAX as usize);
        let height = (rgb_frame.len() / 3 / usize::from(width.get())) as u16;
        let uv_len = usize::from(width.get().div_ceil(2)) * usize::from(height.div_ceil(2));
        debug_assert_eq!(u.len(), uv_len);
        debug_assert_eq!(v.len(), uv_len);
    }
    let width = width.get();
    let y_width = usize::from(width);
    let uv_width = y_width.div_ceil(2);
    let y_tiles = y.chunks_mut(2 * y_width);
    let u_rows = u.chunks_exact_mut(uv_width);
    let v_rows = v.chunks_exact_mut(uv_width);
    let rgb_width = NUM_RGB_COMPONENTS * y_width;
    let rgb_tiles = rgb_frame.chunks(2 * rgb_width);
    for (((u_row, v_row), y_tile), rgb_tile) in u_rows.zip(v_rows).zip(y_tiles).zip(rgb_tiles) {
        let (Some((y_row0, y_row1)), Some((rgb_row0, rgb_row1))) = (
            y_tile.split_at_mut_checked(y_width),
            rgb_tile.split_at_checked(rgb_width),
        ) else {
            // Can only happen if y/rgb_frame lengths are incorrect.
            break;
        };
        // N.B. y_row1 and rgb_row1 might be empty, hence we iterate over them
        // separately.
        let mut y_row1_chunks = y_row1.chunks_mut(2);
        let mut rgb_row1_chunks = rgb_row1.chunks(2 * NUM_RGB_COMPONENTS);
        for (((u, v), y_row0_chunk), rgb_row0_chunk) in u_row
            .iter_mut()
            .zip(v_row.iter_mut())
            .zip(y_row0.chunks_mut(2))
            .zip(rgb_row0.chunks(2 * 3))
        {
            let mut u_sum = 0;
            let mut v_sum = 0;
            let mut shift = !0_u8; // -1
                                   // First tile row.
            for (y, rgb) in y_row0_chunk.iter_mut().zip(rgb_row0_chunk.chunks_exact(3)) {
                let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
                *y = yuv[0];
                u_sum += yuv[1];
                v_sum += yuv[2];
                shift = shift.wrapping_add(1);
            }
            // Second tile row.
            if let (Some(y_row1_chunk), Some(rgb_row1_chunk)) =
                (y_row1_chunks.next(), rgb_row1_chunks.next())
            {
                for (y, rgb) in y_row1_chunk.iter_mut().zip(rgb_row1_chunk.chunks_exact(3)) {
                    let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
                    *y = yuv[0];
                    u_sum += yuv[1];
                    v_sum += yuv[2];
                }
                shift = shift.wrapping_add(1);
            }
            // Here we divide sum by count to compute average. This code works because tiles
            // are 2x2 and we always increase shift by 1 in the second row (and
            // when we process the first row we increase it either by 1 or 2);
            // count can only be 1, 2 or 4.
            *u = div_by_power_of_two(u_sum, shift);
            *v = div_by_power_of_two(v_sum, shift);
        }
    }
}

/// Converts YUV420p frame back to RGB.
///
/// If slices' lengths aren't correct the output is undefined and the function
/// may or may not panic.
pub fn yuv420p_to_rgb888(
    y: &[i16],
    u: &[i16],
    v: &[i16],
    width: NonZero<u16>,
    rgb_frame: &mut [u8],
) {
    #[cfg(debug_assertions)]
    {
        debug_assert_eq!(y.len(), rgb_frame.len() / 3);
        debug_assert!(rgb_frame.len() / 3 <= u32::MAX as usize);
        let height = (rgb_frame.len() / 3 / usize::from(width.get())) as u16;
        let uv_len = usize::from(width.get().div_ceil(2)) * usize::from(height.div_ceil(2));
        debug_assert_eq!(u.len(), uv_len);
        debug_assert_eq!(v.len(), uv_len);
    }
    let y_width = usize::from(width.get());
    let rgb_width = NUM_RGB_COMPONENTS * y_width;
    let uv_width = y_width.div_ceil(2);
    let rgb_rows = rgb_frame.chunks_exact_mut(rgb_width);
    let y_rows = y.chunks_exact(y_width);
    for (i, (rgb_row, y_row)) in rgb_rows.zip(y_rows).enumerate() {
        for (j, (rgb_out, y)) in rgb_row
            .chunks_exact_mut(3)
            .zip(y_row.iter().copied())
            .enumerate()
        {
            // TODO u,v indexing can be optimized similar to rgb888_to_yuv420p to avoid
            // slicing panicking
            let k = (i / 2) * uv_width + (j / 2);
            let rgb = yuv_to_rgb([y, u[k], v[k]]);
            rgb_out[0] = rgb[0];
            rgb_out[1] = rgb[1];
            rgb_out[2] = rgb[2];
        }
    }
}

/// Returns the lengths of planar _Y_ and _U/V_ slices as well as _U/V_ width
/// and height for the provided image width and height.
pub fn yuv420p_dimensions(
    width: NonZero<u16>,
    height: NonZero<u16>,
) -> (u32, u32, NonZero<u16>, NonZero<u16>) {
    let y_len = u32::from(width.get()) * u32::from(height.get());
    let uv_width = NonZero::new(width.get().div_ceil(2)).expect("This div_ceil never returns zero");
    let uv_height =
        NonZero::new(height.get().div_ceil(2)).expect("This div_ceil never returns zero");
    let uv_len = u32::from(uv_width.get()) * u32::from(uv_height.get());
    (y_len, uv_len, uv_width, uv_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToUsize;
    use alloc::{vec, vec::Vec};
    use core::ops::RangeInclusive;
    use proptest::{collection, prelude::*};

    #[test]
    fn symmetry() {
        for r in 0..=255 {
            for g in 0..=255 {
                for b in 0..=255 {
                    assert_eq!([r, g, b], yuv_to_rgb(rgb_to_yuv([r, g, b])));
                }
            }
        }
    }

    #[test]
    fn min_max() {
        let mut min = [i16::MAX; 3];
        let mut max = [i16::MIN; 3];
        for r in 0..=255 {
            for g in 0..=255 {
                for b in 0..=255 {
                    let yuv = rgb_to_yuv([r, g, b]);
                    for ((x, x_min), x_max) in
                        yuv.iter().copied().zip(min.iter_mut()).zip(max.iter_mut())
                    {
                        if x < *x_min {
                            *x_min = x;
                        }
                        if *x_max < x {
                            *x_max = x;
                        }
                    }
                }
            }
        }
        pub const Y_MIN: i16 = -128;
        pub const Y_MAX: i16 = 127;
        assert_eq!(Y_MIN, min[0]);
        assert_eq!(Y_MAX, max[0]);
        assert_eq!(UV_MIN, min[1]);
        assert_eq!(UV_MAX, max[1]);
        assert_eq!(UV_MIN, min[2]);
        assert_eq!(UV_MAX, max[2]);
    }

    #[test]
    fn rgb888_to_yuv420p_simple_works() {
        let frame = [0, 0, 0, 0, 0, 8];
        let mut y = [0; 2];
        let mut u = [0; 1];
        let mut v = [0; 1];
        let width = NonZero::<u16>::new(1).unwrap();
        let height = NonZero::<u16>::new(2).unwrap();
        rgb888_to_yuv420p_old(
            &frame[..],
            width.get().into(),
            height.get().into(),
            &mut y[..],
            &mut u[..],
            &mut v[..],
        );
        {
            let mut y2 = [0; 2];
            let mut u2 = [0; 1];
            let mut v2 = [0; 1];
            rgb888_to_yuv420p(&frame[..], width, &mut y2[..], &mut u2[..], &mut v2[..]);
            assert_eq!(y, y2);
            assert_eq!(u, u2);
            assert_eq!(v, v2);
        }
        let mut actual = frame;
        actual.fill(0);
        yuv420p_to_rgb888(&y, &u, &v, width, &mut actual);
    }

    fn rgb_frame(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
    ) -> impl Strategy<Value = (u16, u16, Vec<u8>)> {
        (width, height).prop_flat_map(|(width, height)| {
            (
                width..=width,
                height..=height,
                collection::vec(any::<u8>(), usize::from(width) * usize::from(height) * 3),
            )
        })
    }

    #[test]
    fn rgb888_to_yuv420p_works() {
        proptest!(|((width, height, x) in rgb_frame(1..=10, 1..=10))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let (y_len, uv_len, ..) = yuv420p_dimensions(width, height);
            let mut y = vec![0; y_len.to_usize()];
            let mut u = vec![0; uv_len.to_usize()];
            let mut v = vec![0; uv_len.to_usize()];
            rgb888_to_yuv420p_old(
                &x[..],
                width.get().into(),
                height.get().into(),
                &mut y[..],
                &mut u[..],
                &mut v[..]
            );
            {
                let mut y2 = vec![0; y_len.to_usize()];
                let mut u2 = vec![0; uv_len.to_usize()];
                let mut v2 = vec![0; uv_len.to_usize()];
                rgb888_to_yuv420p(&x[..], width, &mut y2[..], &mut u2[..], &mut v2[..]);
                assert_eq!(y, y2);
                assert_eq!(u, u2);
                assert_eq!(v, v2);
            }
            let mut actual = vec![0; x.len()];
            yuv420p_to_rgb888_old(&y, &u, &v, width.into(), &mut actual);
            {
                let mut actual2 = vec![0; x.len()];
                yuv420p_to_rgb888(&y, &u, &v, width, &mut actual2);
                assert_eq!(actual, actual2);
            }
        });
    }

    // This is old, unoptimized and hopefully more readable implementation.
    fn rgb888_to_yuv420p_old(
        frame: &[u8],
        width: u32,
        height: u32,
        y: &mut [i16],
        u: &mut [i16],
        v: &mut [i16],
    ) {
        #[derive(Clone, Copy)]
        struct SumUv {
            u: i16,
            v: i16,
            count: u8,
        }
        debug_assert_eq!(frame.len(), (width * height * 3).to_usize());
        debug_assert_eq!(y.len(), (width * height).to_usize());
        debug_assert_eq!(u.len(), (width.div_ceil(2) * height.div_ceil(2)).to_usize());
        debug_assert_eq!(v.len(), (width.div_ceil(2) * height.div_ceil(2)).to_usize());
        let uv_len = u.len();
        let frame_row_len = width.to_usize() * 3;
        let uv_width = width.to_usize().div_ceil(2);
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
                let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
                y[i * width.to_usize() + j] = yuv[0];
                let sum = &mut sum_uv[(i / 2) * uv_width + (j / 2)];
                sum.u += yuv[1];
                sum.v += yuv[2];
                sum.count += 1;
            }
        }
        for ((u, v), sum) in u.iter_mut().zip(v.iter_mut()).zip(sum_uv.iter()) {
            *u = sum.u / i16::from(sum.count);
            *v = sum.v / i16::from(sum.count);
        }
    }

    // This is old, unoptimized and hopefully more readable implementation.
    fn yuv420p_to_rgb888_old(
        y: &[i16],
        u: &[i16],
        v: &[i16],
        width: NonZero<u32>,
        rgb_frame: &mut [u8],
    ) {
        #[cfg(debug_assertions)]
        {
            debug_assert_eq!(y.len(), rgb_frame.len() / 3);
            let height = (rgb_frame.len() / 3 / width.get().to_usize()) as u32;
            debug_assert_eq!(
                u.len(),
                (width.get().div_ceil(2) * height.div_ceil(2)).to_usize()
            );
            debug_assert_eq!(
                v.len(),
                (width.get().div_ceil(2) * height.div_ceil(2)).to_usize()
            );
        }
        let width = width.get().to_usize();
        let frame_row_len = width * 3;
        let uv_width = width.div_ceil(2);
        for (i, row) in rgb_frame.chunks_exact_mut(frame_row_len).enumerate() {
            for (j, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                let k = (i / 2) * uv_width + (j / 2);
                let yuv = [y[i * width + j], u[k], v[k]];
                let rgb = yuv_to_rgb(yuv);
                rgb_out[0] = rgb[0];
                rgb_out[1] = rgb[1];
                rgb_out[2] = rgb[2];
            }
        }
    }
}
