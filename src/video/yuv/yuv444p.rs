use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZero;

use super::yuv_to_rgb;
use crate::{ToUsize, video::Sample};

#[derive(Clone)]
pub struct Yuv444pFrame {
    data: Vec<Sample>,
    len: u32,
    width: NonZero<u16>,
    height: NonZero<u16>,
}

impl Yuv444pFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>) -> Self {
        let len = u32::from(width.get()) * u32::from(height.get());
        let data = vec![Sample::default(); 3 * len.to_usize()];
        Self {
            data,
            len,
            width,
            height,
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [Sample], &mut [Sample], &mut [Sample]) {
        let (y, uv) = self.data.split_at_mut(self.len.to_usize());
        let (u, v) = uv.split_at_mut(self.len.to_usize());
        (y, u, v)
    }

    /// Returns _Y_, _U_, _V_ as slices.
    pub fn as_slices(&self) -> (&[Sample], &[Sample], &[Sample]) {
        let (y, uv) = self.data.split_at(self.len.to_usize());
        let (u, v) = uv.split_at(self.len.to_usize());
        (y, u, v)
    }

    pub fn width(&self) -> NonZero<u16> {
        self.width
    }

    pub fn height(&self) -> NonZero<u16> {
        self.height
    }
}

#[inline(never)]
pub fn pixels_to_yuv444p<P>(
    pixels: &[P],
    y: &mut [Sample],
    u: &mut [Sample],
    v: &mut [Sample],
    to_yuv: impl Fn(&P) -> [i16; 3],
) {
    debug_assert_eq!(pixels.len(), y.len());
    debug_assert_eq!(pixels.len(), u.len());
    debug_assert_eq!(pixels.len(), v.len());
    for (((pixel, y), u), v) in pixels.iter().zip(y).zip(u).zip(v) {
        [y.current, u.current, v.current] = to_yuv(pixel);
    }
}

#[inline(always)]
pub fn yuv444p_to_pixels<P>(
    y: &[Sample],
    u: &[Sample],
    v: &[Sample],
    pixels: &mut [P],
    from_rgb: impl Fn(&mut P, [u8; 3]),
) {
    debug_assert_eq!(pixels.len(), y.len());
    debug_assert_eq!(pixels.len(), u.len());
    debug_assert_eq!(pixels.len(), v.len());
    for (((pixel, y), u), v) in pixels.iter_mut().zip(y).zip(u).zip(v) {
        from_rgb(pixel, yuv_to_rgb([y.current, u.current, v.current]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToUsize;
    use crate::video::yuv::{rgb_to_yuv, tests::rgb_frame};
    use alloc::vec;
    use alloc::vec::Vec;
    use proptest::prelude::*;

    #[test]
    fn rgb888_to_yuv444p_works() {
        proptest!(|((width, height, rgb) in rgb_frame(1..=10, 1..=10))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let len = (u32::from(width.get()) * u32::from(height.get())).to_usize();
            let mut y = vec![Sample::default(); len];
            let mut u = vec![Sample::default(); len];
            let mut v = vec![Sample::default(); len];
            pixels_to_yuv444p(rgb.as_chunks::<3>().0, &mut y[..], &mut u[..], &mut v[..], |rgb| rgb_to_yuv(*rgb));
            let mut actual = vec![0; rgb.len()];
            yuv444p_to_pixels(&y, &u, &v, actual.as_chunks_mut::<3>().0, |pixel, rgb| *pixel = rgb);
            assert_eq!(rgb, actual);
        });
    }

    #[test]
    fn yuv444p_to_rgba8888_works() {
        proptest!(|((width, height, rgb) in rgb_frame(1..=10, 1..=10))| {
            let rgba: Vec<_> = rgb
                .as_chunks::<3>().0.iter()
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], u8::MAX])
                .collect();
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let len = (u32::from(width.get()) * u32::from(height.get())).to_usize();
            let mut y = vec![Sample::default(); len];
            let mut u = vec![Sample::default(); len];
            let mut v = vec![Sample::default(); len];
            pixels_to_yuv444p(rgb.as_chunks::<3>().0, &mut y[..], &mut u[..], &mut v[..], |rgb| rgb_to_yuv(*rgb));
            let mut actual = vec![0; rgba.len()];
            yuv444p_to_pixels(&y, &u, &v, actual.as_chunks_mut::<4>().0, |pixel, [r, g, b]| {
                *pixel = [r, g, b, u8::MAX]
            });
            assert_eq!(rgba, actual);
        });
    }
}
