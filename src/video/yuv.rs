use crate::video::Sample;
use alloc::vec::Vec;
use core::num::NonZero;

mod dummy;
mod yuv420p;
mod yuv444p;

pub use self::{dummy::*, yuv420p::*, yuv444p::*};

#[derive(Clone)]
pub enum YuvFrame {
    Yuv420p(Yuv420pFrame),
    Yuv444p(Yuv444pFrame),
    Dummy(DummyFrame),
}

impl YuvFrame {
    pub fn new(
        width: NonZero<u16>,
        height: NonZero<u16>,
        chroma_subsampling: bool,
        raw: bool,
    ) -> Self {
        if raw {
            return Self::Dummy(DummyFrame::new(chroma_subsampling));
        }
        if chroma_subsampling {
            Self::Yuv420p(Yuv420pFrame::new(width, height))
        } else {
            Self::Yuv444p(Yuv444pFrame::new(width, height))
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [Sample], &mut [Sample], &mut [Sample]) {
        match self {
            Self::Yuv420p(frame) => frame.as_mut_slices(),
            Self::Yuv444p(frame) => frame.as_mut_slices(),
            Self::Dummy(..) => unreachable!(),
        }
    }

    pub fn uv_width(&self) -> NonZero<u16> {
        match self {
            Self::Yuv420p(frame) => frame.uv_width(),
            Self::Yuv444p(frame) => frame.width(),
            Self::Dummy(..) => unreachable!(),
        }
    }

    pub fn uv_height(&self) -> NonZero<u16> {
        match self {
            Self::Yuv420p(frame) => frame.uv_height(),
            Self::Yuv444p(frame) => frame.height(),
            Self::Dummy(..) => unreachable!(),
        }
    }

    pub fn chroma_subsampling(&self) -> bool {
        match self {
            Self::Yuv420p(..) => true,
            Self::Yuv444p(..) => false,
            Self::Dummy(frame) => frame.chroma_subsampling(),
        }
    }

    pub fn raw(&self) -> bool {
        match self {
            Self::Yuv420p(..) => false,
            Self::Yuv444p(..) => false,
            Self::Dummy(..) => true,
        }
    }

    pub fn previous_values(&self) -> Vec<i16> {
        match self {
            Self::Yuv420p(frame) => previous_values(frame.as_slices()),
            Self::Yuv444p(frame) => previous_values(frame.as_slices()),
            Self::Dummy(..) => Vec::new(),
        }
    }

    pub fn set_previous_values(&mut self, values: &[i16]) {
        let (y, u, v) = match self {
            Self::Yuv420p(frame) => frame.as_mut_slices(),
            Self::Yuv444p(frame) => frame.as_mut_slices(),
            Self::Dummy(..) => return,
        };
        for (sample, value) in y.iter_mut().chain(u).chain(v).zip(values) {
            sample.previous = *value;
        }
    }

    #[inline(always)]
    pub fn load_pixels<P>(
        &mut self,
        pixels: &[P],
        width: NonZero<u16>,
        to_yuv: impl Fn(&P) -> [i16; 3],
    ) {
        match self {
            Self::Yuv420p(frame) => {
                let (y, u, v) = frame.as_mut_slices();
                pixels_to_yuv420p(pixels, width, y, u, v, to_yuv);
            }
            Self::Yuv444p(frame) => {
                let (y, u, v) = frame.as_mut_slices();
                pixels_to_yuv444p(pixels, y, u, v, to_yuv);
            }
            Self::Dummy(..) => unreachable!(),
        }
    }

    #[inline(always)]
    pub fn store_pixels<P>(
        &self,
        pixels: &mut [P],
        width: NonZero<u16>,
        from_rgb: impl Fn(&mut P, [u8; 3]),
    ) {
        match self {
            Self::Yuv420p(frame) => {
                let (y, u, v) = frame.as_slices();
                yuv420p_to_pixels(y, u, v, width, pixels, from_rgb);
            }
            Self::Yuv444p(frame) => {
                let (y, u, v) = frame.as_slices();
                yuv444p_to_pixels(y, u, v, pixels, from_rgb);
            }
            Self::Dummy(..) => unreachable!(),
        }
    }
}

fn previous_values((y, u, v): (&[Sample], &[Sample], &[Sample])) -> Vec<i16> {
    y.iter()
        .chain(u)
        .chain(v)
        .map(|sample| sample.previous)
        .collect()
}

pub struct YuvPalette([u64; 256]);

impl YuvPalette {
    pub fn new(rgb_palette: &[[u8; 3]; 256]) -> Self {
        Self(rgb_palette.map(|rgb| {
            let [y, u, v] = rgb_to_yuv(rgb).map(|component| u64::from(component as u16));
            y | (u << 16) | (v << 32)
        }))
    }

    #[inline(always)]
    pub fn get(&self, index: u8) -> [i16; 3] {
        let entry = self.0[usize::from(index)];
        [entry as i16, (entry >> 16) as i16, (entry >> 32) as i16]
    }
}

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
    let g = y.wrapping_sub(u.wrapping_add(v) >> 2).wrapping_add(128);
    let r = u.wrapping_add(g);
    let b = v.wrapping_add(g);
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use alloc::vec::Vec;
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
    fn yuv_palette_works() {
        proptest!(|(palette in collection::vec(any::<[u8; 3]>(), 256))| {
            let palette: [[u8; 3]; 256] = palette.try_into().unwrap();
            let yuv_palette = YuvPalette::new(&palette);
            for (index, rgb) in palette.iter().enumerate() {
                assert_eq!(rgb_to_yuv(*rgb), yuv_palette.get(index as u8));
            }
        });
    }

    pub fn currents(samples: &[Sample]) -> Vec<i16> {
        samples.iter().map(|sample| sample.current).collect()
    }

    pub fn rgb_frame(
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
}
