use core::num::NonZero;

mod yuv420p;
mod yuv444p;

pub use self::{yuv420p::*, yuv444p::*};

#[derive(Clone)]
pub enum YuvFrame {
    Yuv420p(Yuv420pFrame),
    Yuv444p(Yuv444pFrame),
}

impl YuvFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>, chroma_subsampling: bool) -> Self {
        if chroma_subsampling {
            Self::Yuv420p(Yuv420pFrame::new(width, height))
        } else {
            Self::Yuv444p(Yuv444pFrame::new(width, height))
        }
    }

    /// Returns _Y_, _U_, _V_ as mutable slices.
    pub fn as_mut_slices(&mut self) -> (&mut [i16], &mut [i16], &mut [i16]) {
        match self {
            Self::Yuv420p(frame) => frame.as_mut_slices(),
            Self::Yuv444p(frame) => frame.as_mut_slices(),
        }
    }

    pub fn uv_width(&self) -> NonZero<u16> {
        match self {
            Self::Yuv420p(frame) => frame.uv_width(),
            Self::Yuv444p(frame) => frame.width(),
        }
    }

    pub fn uv_height(&self) -> NonZero<u16> {
        match self {
            Self::Yuv420p(frame) => frame.uv_height(),
            Self::Yuv444p(frame) => frame.height(),
        }
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
const NUM_RGBA_COMPONENTS: usize = 4;

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
