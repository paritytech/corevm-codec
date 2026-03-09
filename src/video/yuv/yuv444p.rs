use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZero;

use super::rgb_to_yuv;
use super::yuv_to_rgb;
use super::NUM_RGBA_COMPONENTS;
use super::NUM_RGB_COMPONENTS;
use crate::ToUsize;

#[derive(Clone)]
pub struct Yuv444pFrame {
    data: Vec<i16>,
    len: u32,
    width: NonZero<u16>,
    height: NonZero<u16>,
}

impl Yuv444pFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>) -> Self {
        let len = u32::from(width.get()) * u32::from(height.get());
        let data = vec![0; 3 * len.to_usize()];
        Self {
            data,
            len,
            width,
            height,
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [i16], &mut [i16], &mut [i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.len`.
        unsafe {
            let (y, uv) = self.data.split_at_mut_unchecked(self.len.to_usize());
            let (u, v) = uv.split_at_mut_unchecked(self.len.to_usize());
            (y, u, v)
        }
    }

    /// Returns _Y_, _U_, _V_ as slices.
    pub fn as_slices(&self) -> (&[i16], &[i16], &[i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.len`.
        unsafe {
            let (y, uv) = self.data.split_at_unchecked(self.len.to_usize());
            let (u, v) = uv.split_at_unchecked(self.len.to_usize());
            (y, u, v)
        }
    }

    pub fn width(&self) -> NonZero<u16> {
        self.width
    }

    pub fn height(&self) -> NonZero<u16> {
        self.height
    }
}

/// Converts RGB frame to planar YUV frame without chroma subsampling using
/// [`rgb_to_yuv`].
///
/// If slices' lengths aren't correct the output is undefined; the function will
/// only panic when debug assertions are enabled.
pub fn rgb888_to_yuv444p(
    rgb_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    rgb888_or_rgba888_to_yuv444p::<NUM_RGB_COMPONENTS>(rgb_frame, width, y, u, v);
}

/// Converts RGBA frame to planar YUV frame without chroma subsampling using
/// [`rgb_to_yuv`].
///
/// If slices' lengths aren't correct the output is undefined; the function will
/// only panic when debug assertions are enabled.
#[allow(unused)]
pub fn rgba8888_to_yuv444p(
    rgba_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    rgb888_or_rgba888_to_yuv444p::<NUM_RGBA_COMPONENTS>(rgba_frame, width, y, u, v);
}

#[inline]
fn rgb888_or_rgba888_to_yuv444p<const N: usize>(
    rgb_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    let width = usize::from(width.get());
    let rgb_width = N * width;
    for (((rgb_row, y_row), u_row), v_row) in rgb_frame
        .chunks_exact(rgb_width)
        .zip(y.chunks_exact_mut(width))
        .zip(u.chunks_exact_mut(width))
        .zip(v.chunks_exact_mut(width))
    {
        for (((rgb, y), u), v) in rgb_row
            .chunks_exact(N)
            .zip(y_row.iter_mut())
            .zip(u_row.iter_mut())
            .zip(v_row.iter_mut())
        {
            let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
            *y = yuv[0];
            *u = yuv[1];
            *v = yuv[2];
        }
    }
}

/// Converts planar YUV frame without chroma subsampling back to RGB.
///
/// If slices' lengths aren't correct the output is undefined and the function
/// may or may not panic.
pub fn yuv444p_to_rgb888(
    y: &[i16],
    u: &[i16],
    v: &[i16],
    width: NonZero<u16>,
    rgb_frame: &mut [u8],
) {
    yuv444p_to_rgb888_or_rgba8888::<NUM_RGB_COMPONENTS>(y, u, v, width, rgb_frame);
}

/// Converts planar YUV frame without chroma subsampling back to RGBA.
///
/// If slices' lengths aren't correct the output is undefined and the function
/// may or may not panic.
pub fn yuv444p_to_rgba8888(
    y: &[i16],
    u: &[i16],
    v: &[i16],
    width: NonZero<u16>,
    rgba_frame: &mut [u8],
) {
    yuv444p_to_rgb888_or_rgba8888::<NUM_RGBA_COMPONENTS>(y, u, v, width, rgba_frame);
}

#[inline]
fn yuv444p_to_rgb888_or_rgba8888<const N: usize>(
    y: &[i16],
    u: &[i16],
    v: &[i16],
    width: NonZero<u16>,
    rgb_frame: &mut [u8],
) {
    let width = usize::from(width.get());
    let rgb_width = N * width;
    for (((rgb_row, y_row), u_row), v_row) in rgb_frame
        .chunks_exact_mut(rgb_width)
        .zip(y.chunks_exact(width))
        .zip(u.chunks_exact(width))
        .zip(v.chunks_exact(width))
    {
        for (((rgb, y), u), v) in rgb_row
            .chunks_exact_mut(N)
            .zip(y_row.iter().copied())
            .zip(u_row.iter().copied())
            .zip(v_row.iter().copied())
        {
            let rgb_in = yuv_to_rgb([y, u, v]);
            rgb[0] = rgb_in[0];
            rgb[1] = rgb_in[1];
            rgb[2] = rgb_in[2];
            if N == 4 {
                rgb[3] = u8::MAX;
            }
        }
    }
}

/// Converts indexed RGB frame to planar YUV frame without chroma subsampling
/// using [`rgb_to_yuv`].
///
/// If slices' lengths aren't correct the output is undefined; the function will
/// only panic when debug assertions are enabled.
pub fn rgb888_indexed8_to_yuv444p(
    indexed_frame: &[u8],
    width: NonZero<u16>,
    y: &mut [i16],
    u: &mut [i16],
    v: &mut [i16],
) {
    let (palette, indices) = indexed_frame.split_at(NUM_RGB_COMPONENTS * 256);
    let width = usize::from(width.get());
    for (((indices_row, y_row), u_row), v_row) in indices
        .chunks_exact(width)
        .zip(y.chunks_exact_mut(width))
        .zip(u.chunks_exact_mut(width))
        .zip(v.chunks_exact_mut(width))
    {
        for (((index, y), u), v) in indices_row
            .iter()
            .copied()
            .zip(y_row.iter_mut())
            .zip(u_row.iter_mut())
            .zip(v_row.iter_mut())
        {
            let rgb = [
                palette[NUM_RGB_COMPONENTS * usize::from(index)],
                palette[NUM_RGB_COMPONENTS * usize::from(index) + 1],
                palette[NUM_RGB_COMPONENTS * usize::from(index) + 2],
            ];
            let yuv = rgb_to_yuv([rgb[0], rgb[1], rgb[2]]);
            *y = yuv[0];
            *u = yuv[1];
            *v = yuv[2];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::yuv::tests::rgb_frame;
    use crate::ToUsize;
    use alloc::vec;
    use alloc::vec::Vec;
    use proptest::prelude::*;

    #[test]
    fn rgb888_to_yuv444p_works() {
        proptest!(|((width, height, rgb) in rgb_frame(1..=10, 1..=10))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let len = (u32::from(width.get()) * u32::from(height.get())).to_usize();
            let mut y = vec![0; len];
            let mut u = vec![0; len];
            let mut v = vec![0; len];
            rgb888_to_yuv444p(
                &rgb[..],
                width,
                &mut y[..],
                &mut u[..],
                &mut v[..]
            );
            let mut actual = vec![0; rgb.len()];
            yuv444p_to_rgb888(&y, &u, &v, width, &mut actual);
            assert_eq!(rgb, actual);
        });
    }

    #[test]
    fn rgba8888_to_yuv444p_works() {
        proptest!(|((width, height, rgb) in rgb_frame(1..=10, 1..=10))| {
            let rgba: Vec<_> = rgb
                .chunks_exact(3)
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], u8::MAX])
                .collect();
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let len = (u32::from(width.get()) * u32::from(height.get())).to_usize();
            let mut y = vec![0; len];
            let mut u = vec![0; len];
            let mut v = vec![0; len];
            rgba8888_to_yuv444p(
                &rgba[..],
                width,
                &mut y[..],
                &mut u[..],
                &mut v[..]
            );
            let mut actual = vec![0; rgba.len()];
            yuv444p_to_rgba8888(&y, &u, &v, width, &mut actual);
            assert_eq!(rgba, actual);
        });
    }
}
