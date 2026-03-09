use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZero;

use super::rgb_to_yuv;
use super::yuv_to_rgb;
use super::NUM_RGBA_COMPONENTS;
use super::NUM_RGB_COMPONENTS;
use crate::ToUsize;

#[derive(Clone)]
pub struct Yuv420pFrame {
    data: Vec<i16>,
    y_len: u32,
    uv_len: u32,
    uv_width: NonZero<u16>,
    uv_height: NonZero<u16>,
}

impl Yuv420pFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>) -> Self {
        let (y_len, uv_len, uv_width, uv_height) = yuv420p_dimensions(width, height);
        let data = vec![0; y_len.to_usize() + 2 * uv_len.to_usize()];
        Self {
            data,
            y_len,
            uv_len,
            uv_width,
            uv_height,
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [i16], &mut [i16], &mut [i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.y_len` and
        // `self.uv_len`.
        unsafe {
            let (y, uv) = self.data.split_at_mut_unchecked(self.y_len.to_usize());
            let (u, v) = uv.split_at_mut_unchecked(self.uv_len.to_usize());
            (y, u, v)
        }
    }

    /// Returns _Y_, _U_, _V_ as slices.
    pub fn as_slices(&self) -> (&[i16], &[i16], &[i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.y_len` and
        // `self.uv_len`.
        unsafe {
            let (y, uv) = self.data.split_at_unchecked(self.y_len.to_usize());
            let (u, v) = uv.split_at_unchecked(self.uv_len.to_usize());
            (y, u, v)
        }
    }

    pub fn uv_width(&self) -> NonZero<u16> {
        self.uv_width
    }

    pub fn uv_height(&self) -> NonZero<u16> {
        self.uv_height
    }
}

fn div_by_power_of_two(value: i16, q: u8) -> i16 {
    let sign_bit = value >> 15;
    (value + (sign_bit & ((1_i16 << q) - 1))) >> q
}

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
            .zip(rgb_row0.chunks(2 * NUM_RGB_COMPONENTS))
        {
            let mut u_sum = 0;
            let mut v_sum = 0;
            // -1
            let mut shift = !0_u8;
            // First tile row.
            for (y, rgb) in y_row0_chunk
                .iter_mut()
                .zip(rgb_row0_chunk.chunks_exact(NUM_RGB_COMPONENTS))
            {
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
                for (y, rgb) in y_row1_chunk
                    .iter_mut()
                    .zip(rgb_row1_chunk.chunks_exact(NUM_RGB_COMPONENTS))
                {
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

/// Converts indexed RGB frame to planar YUV frame using [`rgb_to_yuv`] with
/// 4:2:0 chroma subsampling.
///
/// If slices' lengths aren't correct the output is undefined; the function will
/// only panic when debug assertions are enabled.
pub fn rgb888_indexed8_to_yuv420p(
    indexed_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    let width = width.get();
    let (palette, indices) = indexed_frame.split_at(NUM_RGB_COMPONENTS * 256);
    let y_width = usize::from(width);
    let uv_width = y_width.div_ceil(2);
    let y_tiles = y.chunks_mut(2 * y_width);
    let u_rows = u.chunks_exact_mut(uv_width);
    let v_rows = v.chunks_exact_mut(uv_width);
    let indices_width = y_width;
    let indices_tiles = indices.chunks(2 * indices_width);
    for (((u_row, v_row), y_tile), indices_tile) in
        u_rows.zip(v_rows).zip(y_tiles).zip(indices_tiles)
    {
        let (Some((y_row0, y_row1)), Some((indices_row0, indices_row1))) = (
            y_tile.split_at_mut_checked(y_width),
            indices_tile.split_at_checked(indices_width),
        ) else {
            // Can only happen if y/indices lengths are incorrect.
            break;
        };
        // N.B. y_row1 and indices_row1 might be empty, hence we iterate over them
        // separately.
        let mut y_row1_chunks = y_row1.chunks_mut(2);
        let mut indices_row1_chunks = indices_row1.chunks(2);
        for (((u, v), y_row0_chunk), indices_row0_chunk) in u_row
            .iter_mut()
            .zip(v_row.iter_mut())
            .zip(y_row0.chunks_mut(2))
            .zip(indices_row0.chunks(2))
        {
            let mut u_sum = 0;
            let mut v_sum = 0;
            // -1
            let mut shift = !0_u8;
            // First tile row.
            for (y, index) in y_row0_chunk
                .iter_mut()
                .zip(indices_row0_chunk.iter().copied())
            {
                let rgb = [
                    palette[NUM_RGB_COMPONENTS * usize::from(index)],
                    palette[NUM_RGB_COMPONENTS * usize::from(index) + 1],
                    palette[NUM_RGB_COMPONENTS * usize::from(index) + 2],
                ];
                let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
                *y = yuv[0];
                u_sum += yuv[1];
                v_sum += yuv[2];
                shift = shift.wrapping_add(1);
            }
            // Second tile row.
            if let (Some(y_row1_chunk), Some(indices_row1_chunk)) =
                (y_row1_chunks.next(), indices_row1_chunks.next())
            {
                for (y, index) in y_row1_chunk
                    .iter_mut()
                    .zip(indices_row1_chunk.iter().copied())
                {
                    let rgb = [
                        palette[NUM_RGB_COMPONENTS * usize::from(index)],
                        palette[NUM_RGB_COMPONENTS * usize::from(index) + 1],
                        palette[NUM_RGB_COMPONENTS * usize::from(index) + 2],
                    ];
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
            .chunks_exact_mut(NUM_RGB_COMPONENTS)
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

/// Converts YUV420p frame to RGBA.
///
/// If slices' lengths aren't correct the output is undefined and the function
/// may or may not panic.
pub fn yuv420p_to_rgba8888(
    y: &[i16],
    u: &[i16],
    v: &[i16],
    width: NonZero<u16>,
    rgb_frame: &mut [u8],
) {
    #[cfg(debug_assertions)]
    {
        debug_assert_eq!(y.len(), rgb_frame.len() / 4);
        debug_assert!(rgb_frame.len() / 4 <= u32::MAX as usize);
        let height = (rgb_frame.len() / 4 / usize::from(width.get())) as u16;
        let uv_len = usize::from(width.get().div_ceil(2)) * usize::from(height.div_ceil(2));
        debug_assert_eq!(u.len(), uv_len);
        debug_assert_eq!(v.len(), uv_len);
    }
    let y_width = usize::from(width.get());
    let rgba_width = NUM_RGBA_COMPONENTS * y_width;
    let uv_width = y_width.div_ceil(2);
    let rgba_rows = rgb_frame.chunks_exact_mut(rgba_width);
    let y_rows = y.chunks_exact(y_width);
    for (i, (rgb_row, y_row)) in rgba_rows.zip(y_rows).enumerate() {
        for (j, (rgba_out, y)) in rgb_row
            .chunks_exact_mut(NUM_RGBA_COMPONENTS)
            .zip(y_row.iter().copied())
            .enumerate()
        {
            // TODO u,v indexing can be optimized similar to rgb888_to_yuv420p to avoid
            // slicing panicking
            let k = (i / 2) * uv_width + (j / 2);
            let rgb = yuv_to_rgb([y, u[k], v[k]]);
            rgba_out[0] = rgb[0];
            rgba_out[1] = rgb[1];
            rgba_out[2] = rgb[2];
            rgba_out[3] = u8::MAX;
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
pub mod tests {
    use super::*;
    use crate::video::yuv::tests::rgb_frame;
    use crate::ToUsize;
    use alloc::vec;
    use proptest::prelude::*;

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
