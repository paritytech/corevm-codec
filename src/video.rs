//! Video codec.

use crate::{errors, rans, timer_finish, timer_start, Input, Output, ToUsize};
use alloc::{vec, vec::Vec};
use core::num::NonZero;
use jam_codec::{Compact, Decode, Encode};

mod delta;
mod haar;
mod quant;
mod signmag;
mod stats;
mod yuv;

pub use self::stats::*;
use self::{quant::*, yuv::*};

#[derive(Clone)]
struct Yuv420pFrame {
    data: Vec<i16>,
    y_len: u32,
    uv_len: u32,
    uv_width: NonZero<u16>,
    uv_height: NonZero<u16>,
}

impl Yuv420pFrame {
    fn new(width: NonZero<u16>, height: NonZero<u16>) -> Self {
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
    fn as_mut_slices(&mut self) -> (&mut [i16], &mut [i16], &mut [i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.y_len` and
        // `self.uv_len`.
        unsafe {
            let (y, uv) = self.data.split_at_mut_unchecked(self.y_len.to_usize());
            let (u, v) = uv.split_at_mut_unchecked(self.uv_len.to_usize());
            (y, u, v)
        }
    }

    /// Returns _Y_, _U_, _V_ as slices.
    fn as_slices(&mut self) -> (&[i16], &[i16], &[i16]) {
        // SAFETY: This is safe because `self.data` is constructed from `self.y_len` and
        // `self.uv_len`.
        unsafe {
            let (y, uv) = self.data.split_at_unchecked(self.y_len.to_usize());
            let (u, v) = uv.split_at_unchecked(self.uv_len.to_usize());
            (y, u, v)
        }
    }
}

/// Video encoder configuration.
#[derive(Debug)]
#[non_exhaustive]
pub struct Config {
    /// Quantization level.
    ///
    /// Typical value is 3-4, maximum is 14.
    /// Low values might increase the input size instead of decreasing.
    pub quantization_level: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quantization_level: 4,
        }
    }
}

/// Video encoder.
///
/// This encoder uses semi-standard pipeline with the simplest and fastest
/// implementation for each stage. Currently the following stages are
/// implemented.
///
/// | Stage | How it is implemented |
/// |-------|-----------------------|
/// | RGB to YUV conversion | PGF (no division). |
/// | Transform | Haar transform (no division). |
/// | Quantization | Scalar quantization without a deadzone. |
/// | Motion estimation | Simple delta encoding. |
/// | Bit encoder | Sign-magniude conversion and rANS encoder. |
///
/// # Example
///
/// ```
/// use core::num::NonZero;
/// use corevm_codec::video;
///
/// let rgb_frame = [
///     0,   0, 0,
///     100, 0, 0,
///     0, 100, 0,
///     0, 0, 100,
/// ];
/// let width = NonZero::new(2).unwrap();
/// let height = width;
/// let mut output = Vec::new();
/// let config = video::Config::default();
/// let mut encoder = video::Encoder::new(width, height, config);
/// encoder.start(&mut output);
/// encoder.write_rgb888_frame(&rgb_frame, &mut output);
/// encoder.finish(&mut output);
/// ```
pub struct Encoder {
    width: NonZero<u16>,
    height: NonZero<u16>,
    quant: u8,
    // Current frame.
    frame: Yuv420pFrame,
    // Previous frame.
    prev_frame: Yuv420pFrame,
    // Output buffer.
    buf: Vec<u8>,
}

impl Encoder {
    /// Creates new encoder with the provided width, height, and configuration.
    pub fn new(width: NonZero<u16>, height: NonZero<u16>, config: Config) -> Self {
        let quant = config.quantization_level.min(MAX_QUANTIZATION_LEVEL);
        let frame = Yuv420pFrame::new(width, height);
        let prev_frame = frame.clone();
        let buf = Vec::with_capacity(32 * 1024);
        Self {
            width,
            height,
            quant,
            frame,
            prev_frame,
            buf,
        }
    }

    /// Encode RGB888 frame and append the resulting bytes to the output.
    pub fn write_rgb888_frame(&mut self, frame: &[u8], output: &mut impl Output) -> Stats {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) * 3,
            frame.len()
        );
        let (y, u, v) = self.frame.as_mut_slices();
        rgb888_to_yuv420p(frame, self.width, y, u, v);
        self.write_frame(output)
    }

    fn write_frame(&mut self, output: &mut impl Output) -> Stats {
        let (uv_width, uv_height) = (self.frame.uv_width, self.frame.uv_height);
        let (y, u, v) = self.frame.as_mut_slices();
        let t_transform = timer_start!();
        haar::forward(y, self.width, self.height, self.quant);
        haar::forward(u, uv_width, uv_height, self.quant);
        haar::forward(v, uv_width, uv_height, self.quant);
        timer_finish!(t_transform);
        let t_delta = timer_start!();
        let (y_prev, u_prev, v_prev) = self.prev_frame.as_mut_slices();
        delta::encode(y, y_prev);
        delta::encode(u, u_prev);
        delta::encode(v, v_prev);
        timer_finish!(t_delta);
        let t_signmag = timer_start!();
        self.buf.clear();
        let y_stats = signmag::encode(y, &mut self.buf);
        let u_stats = signmag::encode(u, &mut self.buf);
        let v_stats = signmag::encode(v, &mut self.buf);
        // Reverse to be able to read frames in normal order.
        self.buf.reverse();
        timer_finish!(t_signmag);
        let stats = Stats {
            y: y_stats,
            u: u_stats,
            v: v_stats,
            #[cfg(all(feature = "stats", feature = "std"))]
            time: TimeStats {
                delta: t_delta.into_duration(),
                transform: t_transform.into_duration(),
                signmag: t_signmag.into_duration(),
                count: 1,
            },
            count: 1,
        };
        output.write(&self.buf[..]);
        stats
    }

    /// Write stream header.
    pub fn start(&mut self, output: &mut impl Output) {
        Compact(self.width.get()).encode_to(output);
        Compact(self.height.get()).encode_to(output);
        self.quant.encode_to(output);
    }

    /// Write stream footer.
    ///
    /// This is currently empty but might change in the future.
    pub fn finish(self, _output: &mut impl Output) {}
}

errors! {
    (InvalidVideoStream "Invalid video stream")
}

#[doc(hidden)]
impl From<rans::InvalidRansStream> for InvalidVideoStream {
    fn from(_: rans::InvalidRansStream) -> Self {
        InvalidVideoStream
    }
}

#[doc(hidden)]
impl From<signmag::InvalidSignMagStream> for InvalidVideoStream {
    fn from(_: signmag::InvalidSignMagStream) -> Self {
        InvalidVideoStream
    }
}

/// Video decoder.
pub struct Decoder {
    width: NonZero<u16>,
    height: NonZero<u16>,
    quant: u8,
    frame: Yuv420pFrame,
    prev_frame: Yuv420pFrame,
}

impl Decoder {
    /// Create new decoder from the provided input.
    pub fn new(input: &mut impl Input) -> Result<Self, InvalidVideoStream> {
        let Compact(width) = Compact::<u16>::decode(input).map_err(|_| InvalidVideoStream)?;
        let Compact(height) = Compact::<u16>::decode(input).map_err(|_| InvalidVideoStream)?;
        let quant = u8::decode(input).map_err(|_| InvalidVideoStream)?;
        if quant > MAX_QUANTIZATION_LEVEL {
            return Err(InvalidVideoStream);
        }
        let width = NonZero::new(width).ok_or(InvalidVideoStream)?;
        let height = NonZero::new(height).ok_or(InvalidVideoStream)?;
        let frame = Yuv420pFrame::new(width, height);
        let prev_frame = frame.clone();
        Ok(Self {
            width,
            height,
            quant,
            frame,
            prev_frame,
        })
    }

    /// Returns video frame width.
    pub fn width(&self) -> NonZero<u16> {
        self.width
    }

    /// Returns video frame height.
    pub fn height(&self) -> NonZero<u16> {
        self.height
    }

    /// Decode RGB888 frame.
    pub fn read_rgb888_frame(
        &mut self,
        input: &mut impl Input,
        frame: &mut [u8],
    ) -> Result<(), InvalidVideoStream> {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) * 3,
            frame.len()
        );
        self.read_yuv420p_frame(input)?;
        let (y, u, v) = self.frame.as_slices();
        yuv420p_to_rgb888(y, u, v, self.width, frame);
        Ok(())
    }

    fn read_yuv420p_frame(&mut self, input: &mut impl Input) -> Result<(), InvalidVideoStream> {
        let (uv_width, uv_height) = (self.frame.uv_width, self.frame.uv_height);
        let (y, u, v) = self.frame.as_mut_slices();
        let (y_prev, u_prev, v_prev) = self.prev_frame.as_mut_slices();
        // Read in reverse order: v, u, y.
        let mut ans_input = rans::JamInputAsAnsInput(input);
        // v
        signmag::decode(&mut ans_input, v)?;
        delta::decode(v, v_prev);
        haar::backward(v, uv_width, uv_height, self.quant);
        // u
        signmag::decode(&mut ans_input, u)?;
        delta::decode(u, u_prev);
        haar::backward(u, uv_width, uv_height, self.quant);
        // y
        signmag::decode(&mut ans_input, y)?;
        delta::decode(y, y_prev);
        haar::backward(y, self.width, self.height, self.quant);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToUsize;
    use alloc::vec;
    use core::ops::RangeInclusive;
    use proptest::{collection, prelude::*};

    impl Config {
        fn no_quantization() -> Self {
            Self {
                quantization_level: 0,
                ..Default::default()
            }
        }
    }

    impl Encoder {
        fn write_yuv420p_frame(
            &mut self,
            y: &[i16],
            u: &[i16],
            v: &[i16],
            output: &mut impl Output,
        ) -> Stats {
            let (y0, u0, v0) = self.frame.as_mut_slices();
            y0.copy_from_slice(y);
            u0.copy_from_slice(u);
            v0.copy_from_slice(v);
            self.write_frame(output)
        }
    }

    #[test]
    fn encoder_simple_works() {
        let frames = [
            [
                [1, 2, 3, 4].as_slice(), // y
                [5].as_slice(),          // u
                [6].as_slice(),          // v
            ],
            [
                [7, 8, 9, 10].as_slice(), // y
                [11].as_slice(),          // u
                [12].as_slice(),          // v
            ],
        ];
        let mut output = Vec::new();
        let mut encoder = Encoder::new(
            NonZero::new(2).unwrap(),
            NonZero::new(2).unwrap(),
            Config::no_quantization(),
        );
        encoder.start(&mut output);
        for [y, u, v] in frames {
            encoder.write_yuv420p_frame(y, u, v, &mut output);
        }
        encoder.finish(&mut output);
        let mut decoder_input = &output[..];
        let mut decoder = Decoder::new(&mut decoder_input).unwrap();
        for [y, u, v] in frames {
            decoder.read_yuv420p_frame(&mut decoder_input).unwrap();
            let (decoded_y, decoded_u, decoded_v) = decoder.frame.as_mut_slices();
            assert_eq!(y, decoded_y);
            assert_eq!(u, decoded_u);
            assert_eq!(v, decoded_v);
        }
    }

    fn frames(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
        num_frames: RangeInclusive<u32>,
    ) -> impl Strategy<Value = (u16, u16, Vec<Vec<u8>>)> {
        (width, height, num_frames).prop_flat_map(|(width, height, num_frames)| {
            let (y_len, uv_len, ..) =
                yuv420p_dimensions(NonZero::new(width).unwrap(), NonZero::new(height).unwrap());
            (
                width..=width,
                height..=height,
                vec![
                    collection::vec(any::<u8>(), y_len.to_usize() + 2 * uv_len.to_usize());
                    num_frames.to_usize()
                ],
            )
        })
    }

    #[test]
    fn encoder_works() {
        proptest!(|((width, height, frames) in frames(1..=10, 1..=10, 1..=3))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let (y_len, uv_len, ..) = yuv420p_dimensions(width, height);
            let mut output = Vec::new();
            let mut encoder = Encoder::new(width, height, Config::no_quantization());
            encoder.start(&mut output);
            for frame in frames.iter() {
                let frame: Vec<i16> = frame.iter().copied().map(i16::from).collect();
                let (y, uv) = frame.split_at(y_len.to_usize());
                let (u, v) = uv.split_at(uv_len.to_usize());
                encoder.write_yuv420p_frame(y, u, v, &mut output);
            }
            encoder.finish(&mut output);
            let mut decoder_input = &output[..];
            let mut decoder = Decoder::new(&mut decoder_input).unwrap();
            for frame in frames {
                let frame: Vec<i16> = frame.iter().copied().map(i16::from).collect();
                let (y, uv) = frame.split_at(y_len.to_usize());
                let (u, v) = uv.split_at(uv_len.to_usize());
                decoder.read_yuv420p_frame(&mut decoder_input).unwrap();
                let (decoded_y, decoded_u, decoded_v) = decoder.frame.as_mut_slices();
                assert_eq!(y, decoded_y);
                assert_eq!(u, decoded_u);
                assert_eq!(v, decoded_v);
            }
        });
    }
}
