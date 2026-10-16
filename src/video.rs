//! Video codec.

use crate::{
    Input, Output, errors, rans,
    rans::{WordBuffer, WordStack},
    timer_finish, timer_start,
};
use alloc::{vec, vec::Vec};
use core::num::NonZero;
use jam_codec::{Compact, Decode, Encode};

mod entropy;
mod haar;
mod quant;
mod sample;
mod stats;
mod yuv;

pub use self::stats::*;
use self::{quant::*, sample::Sample, yuv::*};

/// Video encoder configuration.
#[derive(Debug)]
#[non_exhaustive]
pub struct Config {
    /// Quantization level.
    ///
    /// Typical value is 3-4, maximum is 14.
    /// Low values might increase the input size instead of decreasing.
    pub quantization_level: u8,
    /// Enable 4:2:0 chroma subsampling.
    pub chroma_subsampling: bool,
    /// Enable raw mode.
    ///
    /// In this mode no encoding is done, input frames are simply copied into
    /// the output instead.
    ///
    /// This mode is useful running the code under a RISCV interpreter.
    ///
    /// Other configuration options have no effect when raw mode is enabled.
    pub raw: bool,
}

impl Config {
    /// Returns default lossless configuration.
    pub fn default_lossless() -> Self {
        Self {
            quantization_level: 0,
            chroma_subsampling: false,
            raw: false,
        }
    }

    /// Returns default raw mode configuration.
    pub fn default_raw() -> Self {
        Self {
            quantization_level: 0,
            chroma_subsampling: false,
            raw: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quantization_level: 4,
            chroma_subsampling: true,
            raw: false,
        }
    }
}

#[derive(Debug, Encode, Decode)]
enum RawVideoFrameFormat {
    Rgb888 = 0,
    Rgb888Indexed8 = 1,
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
/// | Bit encoder | Zigzag conversion and rANS encoder with two contexts. |
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
    frame: YuvFrame,
    words: WordStack,
    header: Vec<u8>,
    entropy_encoder: entropy::Encoder,
}

impl Encoder {
    /// Creates new encoder with the provided width, height, and configuration.
    pub fn new(width: NonZero<u16>, height: NonZero<u16>, config: Config) -> Self {
        let quant = config.quantization_level.min(MAX_QUANTIZATION_LEVEL);
        let frame = YuvFrame::new(width, height, config.chroma_subsampling, config.raw);
        let words = if config.raw {
            WordStack::default()
        } else {
            WordStack::with_capacity(8 * 1024)
        };
        Self {
            width,
            height,
            quant,
            frame,
            words,
            header: Vec::new(),
            entropy_encoder: entropy::Encoder::default(),
        }
    }

    /// Encode RGB888 frame and append the resulting bytes to the output.
    pub fn write_rgb888_frame(&mut self, rgb_frame: &[u8], output: &mut impl Output) -> Stats {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) * 3,
            rgb_frame.len()
        );
        if self.frame.raw() {
            RawVideoFrameFormat::Rgb888.encode_to(output);
            output.write(rgb_frame);
            return Stats::default();
        }
        self.frame
            .load_pixels(rgb_frame.as_chunks::<3>().0, self.width, |rgb| {
                rgb_to_yuv(*rgb)
            });
        self.write_frame(output)
    }

    /// Encode indexed RGB888 frame and append the resulting bytes to the
    /// output.
    ///
    /// The frame consists of a 256-color palette and an array of 8-bit indices
    /// into the palette. Each palette element is encoded as RGB888.
    pub fn write_rgb888_indexed8_frame(
        &mut self,
        indexed_rgb_frame: &[u8],
        output: &mut impl Output,
    ) -> Stats {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) + 3 * 256,
            indexed_rgb_frame.len()
        );
        if self.frame.raw() {
            RawVideoFrameFormat::Rgb888Indexed8.encode_to(output);
            output.write(indexed_rgb_frame);
            return Stats::default();
        }
        let (palette, indices) = indexed_rgb_frame.split_at(3 * 256);
        let palette = YuvPalette::new(
            palette
                .as_chunks::<3>()
                .0
                .try_into()
                .expect("invalid palette length"),
        );
        self.frame
            .load_pixels(indices, self.width, |index| palette.get(*index));
        self.write_frame(output)
    }

    /// Encodes the YUV samples of `self.frame` and writes the frame to
    /// `output`.
    ///
    /// A frame contains the Y, U and V plane headers, followed by the rANS
    /// words of the Y, U and V planes. Placing all words after the headers
    /// lets the decoder read them with a single `Input::read` call.
    fn write_frame(&mut self, output: &mut impl Output) -> Stats {
        let (uv_width, uv_height) = (self.frame.uv_width(), self.frame.uv_height());
        let (y, u, v) = self.frame.as_mut_slices();
        let t_transform = timer_start!();
        haar::forward(y, self.width, self.height, self.quant);
        haar::forward(u, uv_width, uv_height, self.quant);
        haar::forward(v, uv_width, uv_height, self.quant);
        timer_finish!(t_transform);
        let t_entropy = timer_start!();
        self.words.clear();
        self.header.clear();
        let y_stats = entropy::encode(
            y,
            &mut self.entropy_encoder,
            &mut self.words,
            &mut self.header,
        );
        let u_stats = entropy::encode(
            u,
            &mut self.entropy_encoder,
            &mut self.words,
            &mut self.header,
        );
        let v_stats = entropy::encode(
            v,
            &mut self.entropy_encoder,
            &mut self.words,
            &mut self.header,
        );
        timer_finish!(t_entropy);
        let stats = Stats {
            y: y_stats,
            u: u_stats,
            v: v_stats,
            #[cfg(all(feature = "stats", feature = "std"))]
            time: TimeStats {
                transform: t_transform.into_duration(),
                entropy: t_entropy.into_duration(),
                count: 1,
            },
            count: 1,
        };
        output.write(&self.header);
        output.write(self.words.as_bytes());
        stats
    }

    /// Write stream header.
    pub fn start(&mut self, output: &mut impl Output) {
        Compact(self.width.get()).encode_to(output);
        Compact(self.height.get()).encode_to(output);
        let chroma_subsampling = if self.frame.chroma_subsampling() {
            1_u8
        } else {
            0_u8
        };
        let raw = match self.frame.raw() {
            true => 1_u8,
            false => 0_u8,
        };
        let config = self.quant
            | (chroma_subsampling << QUANTIZATION_LEVEL_BITS)
            | (raw << (QUANTIZATION_LEVEL_BITS + 1));
        config.encode_to(output);
    }

    /// Write stream footer.
    ///
    /// This is currently empty but might change in the future.
    pub fn finish(self, _output: &mut impl Output) {}

    /// Save current video encoder state in a checkpoint.
    ///
    /// Later you can restore the state via [`restore_from`](Self::restore_from)
    /// and continue encoding video frames as if no frames were encoded
    /// after the checkpoint.
    ///
    /// Note that the output needs to be truncated manually when you restore the
    /// state from a checkpoint.
    pub fn checkpoint(&self) -> EncoderCheckpoint {
        EncoderCheckpoint(Checkpoint::new(
            self.width,
            self.height,
            self.quant,
            &self.frame,
        ))
    }

    /// Restore video encoder state from a checkpoint created by
    /// [`checkpoint`](Self::checkpoint).
    ///
    /// Panics if the checkpoint was created for an encoder with a different
    /// configuration.
    pub fn restore_from(&mut self, checkpoint: EncoderCheckpoint) {
        checkpoint
            .0
            .restore(self.width, self.height, self.quant, &mut self.frame);
    }
}

/// Video encoder checkpoint.
///
/// Holds internal encoder state.
pub struct EncoderCheckpoint(Checkpoint);

struct Checkpoint {
    width: NonZero<u16>,
    height: NonZero<u16>,
    quant: u8,
    raw: bool,
    chroma_subsampling: bool,
    previous_values: Vec<i16>,
}

impl Checkpoint {
    fn new(width: NonZero<u16>, height: NonZero<u16>, quant: u8, frame: &YuvFrame) -> Self {
        Self {
            width,
            height,
            quant,
            raw: frame.raw(),
            chroma_subsampling: frame.chroma_subsampling(),
            previous_values: frame.previous_values(),
        }
    }

    fn restore(self, width: NonZero<u16>, height: NonZero<u16>, quant: u8, frame: &mut YuvFrame) {
        assert!(
            self.raw == frame.raw()
                && self.quant == quant
                && self.chroma_subsampling == frame.chroma_subsampling()
                && self.width == width
                && self.height == height
        );
        frame.set_previous_values(&self.previous_values);
    }
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
impl From<entropy::InvalidEntropyStream> for InvalidVideoStream {
    fn from(_: entropy::InvalidEntropyStream) -> Self {
        InvalidVideoStream
    }
}

#[doc(hidden)]
impl From<jam_codec::Error> for InvalidVideoStream {
    fn from(_: jam_codec::Error) -> Self {
        InvalidVideoStream
    }
}

/// Video decoder.
pub struct Decoder {
    width: NonZero<u16>,
    height: NonZero<u16>,
    quant: u8,
    frame: YuvFrame,
    rans_tables: entropy::Tables,
    words: WordBuffer,
}

impl Decoder {
    /// Create new decoder from the provided input.
    pub fn new(input: &mut impl Input) -> Result<Self, InvalidVideoStream> {
        let Compact(width) = Compact::<u16>::decode(input).map_err(|_| InvalidVideoStream)?;
        let Compact(height) = Compact::<u16>::decode(input).map_err(|_| InvalidVideoStream)?;
        let config = u8::decode(input).map_err(|_| InvalidVideoStream)?;
        let quant = config & 0b1111;
        if quant > MAX_QUANTIZATION_LEVEL {
            return Err(InvalidVideoStream);
        }
        let chroma_subsampling = match (config >> QUANTIZATION_LEVEL_BITS) & 1 {
            0 => false,
            1 => true,
            _ => return Err(InvalidVideoStream),
        };
        let raw = match (config >> (QUANTIZATION_LEVEL_BITS + 1)) & 1 {
            0 => false,
            1 => true,
            _ => return Err(InvalidVideoStream),
        };
        let width = NonZero::new(width).ok_or(InvalidVideoStream)?;
        let height = NonZero::new(height).ok_or(InvalidVideoStream)?;
        let frame = YuvFrame::new(width, height, chroma_subsampling, raw);
        Ok(Self {
            width,
            height,
            quant,
            frame,
            rans_tables: entropy::Tables::default(),
            words: WordBuffer::default(),
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

    /// Decode next frame as RGB888.
    pub fn read_rgb888_frame(
        &mut self,
        input: &mut impl Input,
        rgb_frame: &mut [u8],
    ) -> Result<(), InvalidVideoStream> {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) * 3,
            rgb_frame.len()
        );
        if self.frame.raw() {
            return self.read_rgb888_frame_raw(input, rgb_frame);
        }
        self.read_yuv420p_frame(input)?;
        self.frame.store_pixels(
            rgb_frame.as_chunks_mut::<3>().0,
            self.width,
            |pixel, rgb| *pixel = rgb,
        );
        Ok(())
    }

    fn read_rgb888_frame_raw(
        &mut self,
        input: &mut impl Input,
        rgb_frame: &mut [u8],
    ) -> Result<(), InvalidVideoStream> {
        let format = RawVideoFrameFormat::decode(input)?;
        match format {
            RawVideoFrameFormat::Rgb888Indexed8 => {
                let palette_len = 256 * 3;
                let indices_len = usize::from(self.width.get()) * usize::from(self.height.get());
                let mut indexed_rgb = vec![0_u8; palette_len + indices_len];
                input.read(&mut indexed_rgb[..])?;
                let (palette, indices) = indexed_rgb.split_at(palette_len);
                for (i, rgb) in indices
                    .iter()
                    .copied()
                    .zip(rgb_frame.as_chunks_mut::<3>().0.iter_mut())
                {
                    let j = 3 * i as usize;
                    rgb.copy_from_slice(&palette[j..j + 3]);
                }
            }
            RawVideoFrameFormat::Rgb888 => input.read(rgb_frame)?,
        }
        Ok(())
    }

    /// Decode next frame as RGBA8888.
    ///
    /// All pixels are fully opaque.
    pub fn read_rgba8888_frame(
        &mut self,
        input: &mut impl Input,
        rgba_frame: &mut [u8],
    ) -> Result<(), InvalidVideoStream> {
        assert_eq!(
            usize::from(self.width.get()) * usize::from(self.height.get()) * 4,
            rgba_frame.len()
        );
        if self.frame.raw() {
            return self.read_rgba8888_frame_raw(input, rgba_frame);
        }
        self.read_yuv420p_frame(input)?;
        self.frame.store_pixels(
            rgba_frame.as_chunks_mut::<4>().0,
            self.width,
            |pixel, [r, g, b]| *pixel = [r, g, b, u8::MAX],
        );
        Ok(())
    }

    fn read_rgba8888_frame_raw(
        &mut self,
        input: &mut impl Input,
        rgba_frame: &mut [u8],
    ) -> Result<(), InvalidVideoStream> {
        let format = RawVideoFrameFormat::decode(input)?;
        match format {
            RawVideoFrameFormat::Rgb888Indexed8 => {
                let palette_len = 256 * 3;
                let indices_len = usize::from(self.width.get()) * usize::from(self.height.get());
                let mut indexed_rgb = vec![0_u8; palette_len + indices_len];
                input.read(&mut indexed_rgb[..])?;
                let (palette, indices) = indexed_rgb.split_at(palette_len);
                for (i, rgba) in indices
                    .iter()
                    .copied()
                    .zip(rgba_frame.as_chunks_mut::<4>().0.iter_mut())
                {
                    let j = 3 * i as usize;
                    rgba[..3].copy_from_slice(&palette[j..j + 3]);
                    rgba[3] = u8::MAX;
                }
            }
            RawVideoFrameFormat::Rgb888 => {
                let rgb_len = 3 * usize::from(self.width.get()) * usize::from(self.height.get());
                let mut rgb_frame = vec![0_u8; rgb_len];
                input.read(&mut rgb_frame[..])?;
                for (rgb, rgba) in rgb_frame
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .zip(rgba_frame.as_chunks_mut::<4>().0.iter_mut())
                {
                    rgba[..3].copy_from_slice(rgb);
                    rgba[3] = u8::MAX;
                }
            }
        }
        Ok(())
    }

    /// Decodes one frame into the YUV samples of `self.frame`. See
    /// [`Encoder::write_frame`] for the frame format.
    fn read_yuv420p_frame(&mut self, input: &mut impl Input) -> Result<(), InvalidVideoStream> {
        let (uv_width, uv_height) = (self.frame.uv_width(), self.frame.uv_height());
        let (y, u, v) = self.frame.as_mut_slices();
        let y_header = entropy::decode_header(input, y.len())?;
        let u_header = entropy::decode_header(input, u.len())?;
        let v_header = entropy::decode_header(input, v.len())?;
        let word_count = [&y_header, &u_header, &v_header]
            .into_iter()
            .map(entropy::PlaneHeader::word_count)
            .sum();
        self.words.read_from(input, word_count)?;
        let mut words = self.words.reader();
        entropy::decode(&y_header, &mut words, y, &mut self.rans_tables)?;
        haar::backward(y, self.width, self.height, self.quant);
        entropy::decode(&u_header, &mut words, u, &mut self.rans_tables)?;
        haar::backward(u, uv_width, uv_height, self.quant);
        entropy::decode(&v_header, &mut words, v, &mut self.rans_tables)?;
        haar::backward(v, uv_width, uv_height, self.quant);
        Ok(())
    }

    /// Save current video decoder state in a checkpoint.
    ///
    /// Later you can restore the state via [`restore_from`](Self::restore_from)
    /// and continue decoding video frames as if no frames were decoded
    /// after the checkpoint.
    ///
    /// Note that the input needs to be rewound manually when you restore the
    /// state from a checkpoint.
    pub fn checkpoint(&self) -> DecoderCheckpoint {
        DecoderCheckpoint(Checkpoint::new(
            self.width,
            self.height,
            self.quant,
            &self.frame,
        ))
    }

    /// Restore video decoder state from a checkpoint created by
    /// [`checkpoint`](Self::checkpoint).
    ///
    /// Panics if the checkpoint was created for a decoder with a different
    /// configuration.
    pub fn restore_from(&mut self, checkpoint: DecoderCheckpoint) {
        checkpoint
            .0
            .restore(self.width, self.height, self.quant, &mut self.frame);
    }
}

/// Video decoder checkpoint.
///
/// Holds internal decoder state.
pub struct DecoderCheckpoint(Checkpoint);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToUsize;
    use crate::video::yuv::tests::currents;
    use alloc::{vec, vec::Vec};
    use core::ops::RangeInclusive;
    use proptest::{collection, prelude::*, sample};

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
            for (samples, values) in [(y0, y), (u0, u), (v0, v)] {
                for (sample, value) in samples.iter_mut().zip(values) {
                    sample.current = *value;
                }
            }
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
            assert_eq!(y, currents(decoded_y));
            assert_eq!(u, currents(decoded_u));
            assert_eq!(v, currents(decoded_v));
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
                assert_eq!(y, currents(decoded_y));
                assert_eq!(u, currents(decoded_u));
                assert_eq!(v, currents(decoded_v));
            }
        });
    }

    fn byte_frames(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
        num_frames: RangeInclusive<u32>,
        bytes_per_pixel: usize,
    ) -> impl Strategy<Value = (u16, u16, Vec<Vec<u8>>)> {
        (width, height, num_frames).prop_flat_map(move |(width, height, num_frames)| {
            let frame_len = usize::from(width) * usize::from(height) * bytes_per_pixel;
            (
                width..=width,
                height..=height,
                vec![collection::vec(any::<u8>(), frame_len); num_frames.to_usize()],
            )
        })
    }

    fn rgb_frames(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
        num_frames: RangeInclusive<u32>,
    ) -> impl Strategy<Value = (u16, u16, Vec<Vec<u8>>)> {
        byte_frames(width, height, num_frames, 3)
    }

    #[test]
    fn corrupted_stream_decoding() {
        proptest!(|(
            (width, height, frames) in rgb_frames(1..=10, 1..=10, 1..=3),
            config in config(),
            corruptions in collection::vec((any::<sample::Index>(), any::<u8>()), 1..=8),
        )| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let mut output = Vec::new();
            let mut encoder = Encoder::new(width, height, config);
            encoder.start(&mut output);
            let header_len = output.len();
            for frame in frames.iter() {
                encoder.write_rgb888_frame(frame, &mut output);
            }
            for (index, byte) in corruptions {
                let index = header_len + index.index(output.len() - header_len);
                output[index] ^= byte;
            }
            let mut input = &output[..];
            let Ok(mut decoder) = Decoder::new(&mut input) else {
                return Ok(());
            };
            let mut rgb_frame = vec![0; usize::from(decoder.width().get()) * usize::from(decoder.height().get()) * 3];
            while !input.is_empty() {
                if decoder.read_rgb888_frame(&mut input, &mut rgb_frame).is_err() {
                    break;
                }
            }
        });
    }

    #[test]
    fn indexed_frame_encoding() {
        proptest!(|(
            (width, height, index_frames) in byte_frames(1..=10, 1..=10, 1..=3, 1),
            palette in collection::vec(any::<u8>(), 3 * 256),
            quantization_level in 0_u8..=MAX_QUANTIZATION_LEVEL,
            chroma_subsampling in any::<bool>(),
        )| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let config = || Config {
                quantization_level,
                chroma_subsampling,
                raw: false,
            };
            let mut indexed_encoder = Encoder::new(width, height, config());
            let mut rgb_encoder = Encoder::new(width, height, config());
            let mut indexed_output = Vec::new();
            let mut rgb_output = Vec::new();
            for indices in index_frames {
                let indexed_frame = [palette.as_slice(), indices.as_slice()].concat();
                let rgb_frame: Vec<u8> = indices
                    .iter()
                    .flat_map(|index| palette[3 * usize::from(*index)..][..3].iter().copied())
                    .collect();
                indexed_encoder.write_rgb888_indexed8_frame(&indexed_frame, &mut indexed_output);
                rgb_encoder.write_rgb888_frame(&rgb_frame, &mut rgb_output);
            }
            assert_eq!(rgb_output, indexed_output);
        });
    }

    #[test]
    fn encoder_lossless_works() {
        proptest!(|((width, height, frames) in rgb_frames(1..=10, 1..=10, 1..=3))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let mut output = Vec::new();
            let mut encoder = Encoder::new(width, height, Config::default_lossless());
            encoder.start(&mut output);
            for frame in frames.iter() {
                encoder.write_rgb888_frame(frame, &mut output);
            }
            encoder.finish(&mut output);
            let mut decoder_input = &output[..];
            let mut decoder = Decoder::new(&mut decoder_input).unwrap();
            for frame in frames {
                let mut decoded_frame = vec![0_u8; usize::from(width.get()) * usize::from(height.get()) * 3];
                decoder.read_rgb888_frame(&mut decoder_input, &mut decoded_frame).unwrap();
                assert_eq!(frame, decoded_frame);
            }
        });
    }

    #[test]
    fn encoder_raw_works() {
        proptest!(|((width, height, frames) in rgb_frames(1..=10, 1..=10, 1..=3))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let mut output = Vec::new();
            let mut encoder = Encoder::new(width, height, Config::default_raw());
            encoder.start(&mut output);
            for frame in frames.iter() {
                encoder.write_rgb888_frame(frame, &mut output);
            }
            encoder.finish(&mut output);
            let mut decoder_input = &output[..];
            let mut decoder = Decoder::new(&mut decoder_input).unwrap();
            for frame in frames {
                let mut decoded_frame = vec![0_u8; usize::from(width.get()) * usize::from(height.get()) * 3];
                decoder.read_rgb888_frame(&mut decoder_input, &mut decoded_frame).unwrap();
                assert_eq!(frame, decoded_frame);
            }
        });
    }

    fn config() -> impl Strategy<Value = Config> {
        (any::<bool>(), any::<bool>(), 0_u8..=MAX_QUANTIZATION_LEVEL).prop_map(
            |(raw, chroma_subsampling, quantization_level)| Config {
                raw,
                chroma_subsampling,
                quantization_level,
            },
        )
    }

    #[test]
    fn encoder_checkpoint_works() {
        proptest!(|(
            (width, height, frames) in rgb_frames(1..=10, 1..=10, 1..=3),
            checkpoint in any::<sample::Index>(),
            config in config()
        )| {
            let checkpoint_at = checkpoint.index(frames.len());
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let mut output = Vec::new();
            let mut checkpoint = None;
            let mut output_len = 0;
            let mut encoder = Encoder::new(width, height, config);
            encoder.start(&mut output);
            for (i, frame) in frames.iter().enumerate() {
                if i == checkpoint_at {
                    checkpoint = Some(encoder.checkpoint());
                    output_len = output.len();
                }
                encoder.write_rgb888_frame(frame, &mut output);
            }
            let original_output = output.clone();
            // Restart from the checkpoint.
            encoder.restore_from(checkpoint.unwrap());
            output.truncate(output_len);
            for frame in frames.iter().skip(checkpoint_at) {
                encoder.write_rgb888_frame(frame, &mut output);
            }
            assert_eq!(original_output, output);
        });
    }

    #[test]
    fn decoder_checkpoint_works() {
        proptest!(|(
            (width, height, frames) in rgb_frames(1..=10, 1..=10, 1..=3),
            checkpoint in any::<sample::Index>(),
            config in config()
        )| {
            let checkpoint_at = checkpoint.index(frames.len());
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let mut output = Vec::new();
            let mut encoder = Encoder::new(width, height, config);
            encoder.start(&mut output);
            for frame in frames.iter() {
                encoder.write_rgb888_frame(frame, &mut output);
            }
            encoder.finish(&mut output);
            let mut checkpoint = None;
            let mut decoder_input_checkpoint: &[u8] = &[];
            let mut decoder_input = &output[..];
            let mut decoder = Decoder::new(&mut decoder_input).unwrap();
            let mut decoded_frames = Vec::new();
            for i in 0..frames.len() {
                if i == checkpoint_at {
                    checkpoint = Some(decoder.checkpoint());
                    decoder_input_checkpoint = decoder_input;
                }
                let mut decoded_frame = vec![0_u8; usize::from(width.get()) * usize::from(height.get()) * 3];
                decoder.read_rgb888_frame(&mut decoder_input, &mut decoded_frame).unwrap();
                decoded_frames.push(decoded_frame);
            }
            let orig_decoded_frames = decoded_frames.clone();
            // Restart from the checkpoint.
            decoder.restore_from(checkpoint.unwrap());
            decoder_input = decoder_input_checkpoint;
            decoded_frames.truncate(checkpoint_at);
            for _ in checkpoint_at..frames.len() {
                let mut decoded_frame = vec![0_u8; usize::from(width.get()) * usize::from(height.get()) * 3];
                decoder.read_rgb888_frame(&mut decoder_input, &mut decoded_frame).unwrap();
                decoded_frames.push(decoded_frame);
            }
            assert_eq!(orig_decoded_frames, decoded_frames);
        });
    }
}
