use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZero;

use super::yuv_to_rgb;
use crate::{
    ToUsize,
    video::{Sample, quantize},
};

#[derive(Clone)]
pub struct Yuv420pFrame {
    data: Vec<Sample>,
    y_len: u32,
    uv_len: u32,
    uv_width: NonZero<u16>,
    uv_height: NonZero<u16>,
}

impl Yuv420pFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>) -> Self {
        let (y_len, uv_len, uv_width, uv_height) = yuv420p_dimensions(width, height);
        let data = vec![Sample::default(); y_len.to_usize() + 2 * uv_len.to_usize()];
        Self {
            data,
            y_len,
            uv_len,
            uv_width,
            uv_height,
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [Sample], &mut [Sample], &mut [Sample]) {
        let (y, uv) = self.data.split_at_mut(self.y_len.to_usize());
        let (u, v) = uv.split_at_mut(self.uv_len.to_usize());
        (y, u, v)
    }

    /// Returns _Y_, _U_, _V_ as slices.
    pub fn as_slices(&self) -> (&[Sample], &[Sample], &[Sample]) {
        let (y, uv) = self.data.split_at(self.y_len.to_usize());
        let (u, v) = uv.split_at(self.uv_len.to_usize());
        (y, u, v)
    }

    pub fn uv_width(&self) -> NonZero<u16> {
        self.uv_width
    }

    pub fn uv_height(&self) -> NonZero<u16> {
        self.uv_height
    }
}

#[inline(always)]
fn pixel_row_to_yuv420p<P>(
    pixels: &[P],
    y: &mut [Sample],
    u: &mut [Sample],
    v: &mut [Sample],
    to_yuv: impl Fn(&P) -> [i16; 3],
    chroma_of_pair: impl Fn(i16, i16, i16) -> i16,
    chroma_of_last_pixel: impl Fn(i16, i16) -> i16,
) {
    let (pixel_pairs, last_pixel) = pixels.as_chunks::<2>();
    let (y_pairs, last_y) = y.as_chunks_mut::<2>();
    for (((pixels, y), u), v) in pixel_pairs
        .iter()
        .zip(y_pairs)
        .zip(u.iter_mut())
        .zip(v.iter_mut())
    {
        let [yuv0, yuv1] = pixels.each_ref().map(&to_yuv);
        y[0].current = yuv0[0];
        y[1].current = yuv1[0];
        u.current = chroma_of_pair(u.current, yuv0[1], yuv1[1]);
        v.current = chroma_of_pair(v.current, yuv0[2], yuv1[2]);
    }
    if let ([pixel], [y], Some(u), Some(v)) = (last_pixel, last_y, u.last_mut(), v.last_mut()) {
        let yuv = to_yuv(pixel);
        y.current = yuv[0];
        u.current = chroma_of_last_pixel(u.current, yuv[1]);
        v.current = chroma_of_last_pixel(v.current, yuv[2]);
    }
}

#[inline(never)]
pub fn pixels_to_yuv420p<P>(
    pixels: &[P],
    width: NonZero<u16>,
    y: &mut [Sample],
    u: &mut [Sample],
    v: &mut [Sample],
    to_yuv: impl Fn(&P) -> [i16; 3],
) {
    let width = usize::from(width.get());
    let uv_width = width.div_ceil(2);
    debug_assert_eq!(pixels.len(), y.len());
    debug_assert_eq!(u.len(), uv_width * (y.len() / width).div_ceil(2));
    debug_assert_eq!(u.len(), v.len());
    let mut pixel_row_pairs = pixels.chunks_exact(2 * width);
    let mut y_row_pairs = y.chunks_exact_mut(2 * width);
    let mut u_rows = u.chunks_exact_mut(uv_width);
    let mut v_rows = v.chunks_exact_mut(uv_width);
    for (((pixels, y), u), v) in (&mut pixel_row_pairs)
        .zip(&mut y_row_pairs)
        .zip(&mut u_rows)
        .zip(&mut v_rows)
    {
        let (pixels0, pixels1) = pixels.split_at(width);
        let (y0, y1) = y.split_at_mut(width);
        pixel_row_to_yuv420p(
            pixels0,
            y0,
            u,
            v,
            &to_yuv,
            |_, chroma0, chroma1| chroma0 + chroma1,
            |_, chroma| chroma,
        );
        pixel_row_to_yuv420p(
            pixels1,
            y1,
            u,
            v,
            &to_yuv,
            |sum, chroma0, chroma1| quantize(sum + chroma0 + chroma1, 2),
            |sum, chroma| quantize(sum + chroma, 1),
        );
    }
    if let (Some(u), Some(v)) = (u_rows.next(), v_rows.next()) {
        pixel_row_to_yuv420p(
            pixel_row_pairs.remainder(),
            y_row_pairs.into_remainder(),
            u,
            v,
            &to_yuv,
            |_, chroma0, chroma1| quantize(chroma0 + chroma1, 1),
            |_, chroma| chroma,
        );
    }
}

#[inline(always)]
pub fn yuv420p_to_pixels<P>(
    y: &[Sample],
    u: &[Sample],
    v: &[Sample],
    width: NonZero<u16>,
    pixels: &mut [P],
    from_rgb: impl Fn(&mut P, [u8; 3]),
) {
    let width = usize::from(width.get());
    let uv_width = width.div_ceil(2);
    debug_assert_eq!(pixels.len(), y.len());
    debug_assert_eq!(u.len(), uv_width * (y.len() / width).div_ceil(2));
    debug_assert_eq!(u.len(), v.len());
    let uv_rows = u.chunks_exact(uv_width).zip(v.chunks_exact(uv_width));
    for ((pixels, y), (u, v)) in pixels
        .chunks_exact_mut(width)
        .zip(y.chunks_exact(width))
        .zip(uv_rows.flat_map(|uv_row| [uv_row; 2]))
    {
        let (pixel_pairs, last_pixel) = pixels.as_chunks_mut::<2>();
        let (y_pairs, last_y) = y.as_chunks::<2>();
        for (((pixels, y), u), v) in pixel_pairs.iter_mut().zip(y_pairs).zip(u).zip(v) {
            from_rgb(
                &mut pixels[0],
                yuv_to_rgb([y[0].current, u.current, v.current]),
            );
            from_rgb(
                &mut pixels[1],
                yuv_to_rgb([y[1].current, u.current, v.current]),
            );
        }
        if let ([pixel], [y], Some(u), Some(v)) = (last_pixel, last_y, u.last(), v.last()) {
            from_rgb(pixel, yuv_to_rgb([y.current, u.current, v.current]));
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
    use crate::ToUsize;
    use crate::video::yuv::{
        rgb_to_yuv,
        tests::{currents, rgb_frame},
    };
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
        let mut y2 = [Sample::default(); 2];
        let mut u2 = [Sample::default(); 1];
        let mut v2 = [Sample::default(); 1];
        pixels_to_yuv420p(
            frame.as_chunks::<3>().0,
            width,
            &mut y2[..],
            &mut u2[..],
            &mut v2[..],
            |rgb| rgb_to_yuv(*rgb),
        );
        assert_eq!(y, y2.map(|sample| sample.current));
        assert_eq!(u, u2.map(|sample| sample.current));
        assert_eq!(v, v2.map(|sample| sample.current));
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
            let mut y2 = vec![Sample::default(); y_len.to_usize()];
            let mut u2 = vec![Sample::default(); uv_len.to_usize()];
            let mut v2 = vec![Sample::default(); uv_len.to_usize()];
            pixels_to_yuv420p(x.as_chunks::<3>().0, width, &mut y2[..], &mut u2[..], &mut v2[..], |rgb| rgb_to_yuv(*rgb));
            assert_eq!(y, currents(&y2));
            assert_eq!(u, currents(&u2));
            assert_eq!(v, currents(&v2));
            let mut actual = vec![0; x.len()];
            yuv420p_to_rgb888_old(&y, &u, &v, width.into(), &mut actual);
            {
                let (y, u, v) = (&y2, &u2, &v2);
                let mut actual2 = vec![0; x.len()];
                yuv420p_to_pixels(y, u, v, width, actual2.as_chunks_mut::<3>().0, |pixel, rgb| *pixel = rgb);
                assert_eq!(actual, actual2);
                let mut actual_rgba = vec![0; x.len() / 3 * 4];
                yuv420p_to_pixels(y, u, v, width, actual_rgba.as_chunks_mut::<4>().0, |pixel, [r, g, b]| {
                    *pixel = [r, g, b, u8::MAX]
                });
                let expected_rgba: Vec<u8> = actual.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, u8::MAX]).collect();
                assert_eq!(expected_rgba, actual_rgba);
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
            for (j, rgb) in row.as_chunks::<3>().0.iter().enumerate() {
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
            for (j, rgb_out) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
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
