//! Range Assymertic Numeral System (rANS) encoder/decoder.

use crate::{errors, SliceOutput};
use jam_codec::{Compact, Decode, Encode};

mod bitset;
mod io;
mod reciprocal;
#[cfg(feature = "stats")]
mod stats;

use self::{bitset::FreqBitSet, reciprocal::*};

pub use self::io::*;
#[cfg(feature = "stats")]
pub use self::stats::*;

#[cfg(not(feature = "stats"))]
#[derive(Debug, Default, Clone)]
pub struct Stats;

#[cfg(not(feature = "stats"))]
impl core::ops::AddAssign<&Stats> for Stats {
    fn add_assign(&mut self, _: &Stats) {}
}

/// Maximum input size in bytes.
pub const MAX_INPUT_LEN: usize = u32::MAX as usize;

// Total number of frequencies.
pub(crate) const NUM_FREQS: usize = 256;
// Total number of cumulative frequencies.
const NUM_CFREQS: usize = NUM_FREQS + 1;
// The sum of all frequencies.
const SUM_FREQ: u32 = 2_u32.pow(14);
const INITIAL_STATE: u32 = 2_u32.pow(23);
// This number is artificial since it's difficult to estimate the actual maximum
// footer length.
const FOOTER_BUF_LEN: usize = 512;

type RawFrequencies = [u32; NUM_FREQS];
type NormalizedFrequencies = [u16; NUM_FREQS];
type CumulativeFrequencies = [u16; NUM_CFREQS];

// Sanity checks.
const _: () = assert!(INITIAL_STATE.is_multiple_of(SUM_FREQ));

/// Calculates the number of occurences of each byte in the provided buffer.
fn symbol_frequencies(input: &[u8]) -> RawFrequencies {
    debug_assert!(input.len() <= MAX_INPUT_LEN);
    let mut freqs = [0; NUM_FREQS];
    for symbol in input {
        freqs[usize::from(*symbol)] += 1;
    }
    freqs
}

/// Normalizes symbol frequencies.
///
/// Makes their sum equal to `SUM_FREQ`. Returns normalized frequencies and a
/// set of indices of non-zero frequencies.
fn normalize_frequencies(mut freqs: RawFrequencies) -> (NormalizedFrequencies, FreqBitSet) {
    let sum: u32 = freqs.iter().copied().sum();
    let mut non_zero_freqs = FreqBitSet::new();
    if sum == 0 {
        return ([0; NUM_FREQS], non_zero_freqs);
    }
    let mut sum_remaining = SUM_FREQ;
    let mut sum_extra: u32 = 0;
    let mut non_zero_freq_len: u16 = 0;
    for (i, f) in freqs.iter_mut().enumerate() {
        if *f == 0 {
            continue;
        }
        let mut f_new = (u64::from(*f) * u64::from(SUM_FREQ) / u64::from(sum)) as u32;
        if sum_remaining < f_new {
            f_new = sum_remaining;
        }
        if f_new == 0 {
            // Can only happen if `sum > SUM_FREQ`.
            *f = 1;
            if sum_remaining == 0 {
                sum_extra += 1;
            } else {
                sum_remaining -= 1;
            }
        } else {
            *f = f_new;
            sum_remaining -= f_new;
        }
        non_zero_freqs.set(i);
        non_zero_freq_len += 1;
    }
    // Spread evenly.
    if sum_remaining != 0 {
        let n = u32::from(non_zero_freq_len).min(sum_remaining);
        let df = sum_remaining.div_ceil(n);
        for i in non_zero_freqs.iter() {
            let f = &mut freqs[i];
            if df <= sum_remaining {
                *f += sum_remaining;
                break;
            }
            *f += df;
            sum_remaining -= df;
        }
    }
    // Spread evenly.
    'outer: while sum_extra != 0 {
        for i in non_zero_freqs.iter() {
            let f = &mut freqs[i];
            if *f == 1 {
                continue;
            }
            *f -= 1;
            sum_extra -= 1;
            if sum_extra == 0 {
                break 'outer;
            }
        }
    }
    let mut freqs_u16 = [0; NUM_FREQS];
    for (f0, f1) in freqs_u16.iter_mut().zip(freqs.iter()) {
        debug_assert!(*f1 <= u32::from(u16::MAX));
        *f0 = *f1 as u16;
    }
    (freqs_u16, non_zero_freqs)
}

/// Calculates cumulative frequencies from normalized ones.
///
/// The first frequency is always 0 to optimize RANS encoder/decoder.
fn cumulative_frequencies(freqs: &NormalizedFrequencies) -> CumulativeFrequencies {
    let mut cfreqs = [0; NUM_CFREQS];
    let mut sum = 0;
    for (f, cf) in freqs.iter().zip(cfreqs.iter_mut().skip(1)) {
        sum += f;
        *cf = sum;
    }
    cfreqs
}

pub fn encode(input: &[u8], output: &mut impl Output) -> Stats {
    let mut encoder = Encoder::new(input);
    encoder.write_rev(input, output);
    encoder.finish(output)
}

pub fn decode(input: &mut impl Input, output: &mut [u8]) -> Result<(), InvalidRansStream> {
    let mut decoder = Decoder::new(input)?;
    decoder.read_exact(input, output)
}

/// This is a range variant of Assymertic Numeral System (RANS) encoder that
/// uses _byte_ as a symbol.
///
/// The encoder operates as a stack of symbols, i.e. the symbol that was encoded
/// last is decoded first. In other words, the input should be encoded in
/// reverse order if you want to decode it in normal order.
///
/// The code is based on <https://github.com/rygorous/ryg_rans> with some improvements.
/// The original paper is here: <https://arxiv.org/pdf/1311.2540>.
///
/// # Interleaved output
///
/// Several encoders can use the same output to interleave different data
/// streams. This is useful when you want to encode different portions of a data
/// frame in separate streams to improve compression ratio, but at the same time
/// you want to decode the data that is related to the same frame without
/// decoding the whole streams.
///
/// For example, if you want to encode `x` and `y` as separate streams but
/// decode `x` and `y` related to the same frame together you could do the
/// following.
///
/// ```ignore
/// // Encoder side: frames in _reverse_ order, encoders in normal order.
/// for frame in frames.iter().rev() {
///     x_encoder.write_rev(&frame.x, same_output)?;
///     y_encoder.write_rev(&frame.y, same_output)?;
/// }
/// x_encoder.finish(same_output)?;
/// y_encoder.finish(same_output)?;
///
/// // Decoder side: frames in normal order, decoders in _reverse_ order.
/// for frame in frames.iter_mut() {
///     y_decoder.read_exact(same_input, &mut frame.y[..])?;
///     x_decoder.read_exact(same_input, &mut frame.x[..])?;
/// }
/// ```
pub struct Encoder {
    state: u32,
    cfreqs: CumulativeFrequencies,
    reciprocals: Reciprocals<NUM_FREQS>,
    non_zero_freqs: FreqBitSet,
    #[cfg(feature = "stats")]
    total_out: u32,
}

impl Encoder {
    /// Creates new encoder from the provided input.
    ///
    /// `input` _must_ contain all input that you plan to write to the encoder
    /// via [`write_rev`](Self::write_rev) in order to correctly determine
    /// symbol frequencies.
    ///
    /// Panics if the input size exceeds [`MAX_INPUT_LEN`].
    pub fn new(input: &[u8]) -> Self {
        let input_len = input.len();
        if input_len > MAX_INPUT_LEN {
            panic!("Too large input");
        }
        let freqs = symbol_frequencies(input);
        Self::from_frequencies_unchecked(freqs)
    }

    pub fn from_frequencies(freqs: RawFrequencies) -> Self {
        assert!(freqs.iter().copied().map(u64::from).sum::<u64>() <= u64::from(u32::MAX));
        Self::from_frequencies_unchecked(freqs)
    }

    fn from_frequencies_unchecked(freqs: RawFrequencies) -> Self {
        let (freqs, non_zero_freqs) = normalize_frequencies(freqs);
        let cfreqs = cumulative_frequencies(&freqs);
        let reciprocals = Reciprocals::from_frequencies(&freqs);
        Self {
            state: INITIAL_STATE,
            cfreqs,
            reciprocals,
            non_zero_freqs,
            #[cfg(feature = "stats")]
            total_out: 0,
        }
    }

    /// Encode next input chunk.
    ///
    /// - Chunks _must_ be fed into the encoder in reverse order if you want to
    ///   decode them in normal order.
    /// - Chunks _must_ be part of the original input provided via
    ///   [`Self::new`].
    pub fn write_rev(&mut self, input: &[u8], output: &mut impl Output) {
        for symbol in input.iter().rev() {
            self.push(*symbol, output);
        }
    }

    /// Encode individual symbol.
    pub fn push(&mut self, symbol: u8, output: &mut impl Output) {
        let i = usize::from(symbol);
        let freq = u32::from(self.freq(i));
        debug_assert_ne!(0, freq);
        let max = INITIAL_STATE / SUM_FREQ * 256 * freq;
        while self.state >= max {
            output.push((self.state & 0xff) as u8);
            self.state >>= 8;
            #[cfg(feature = "stats")]
            {
                self.total_out += 1;
            }
        }
        let cfreq = u32::from(self.cfreqs[i]);
        let freq_reciprocal = self.reciprocals.get(i);
        // Calculate self.state / freq.
        let quot = self.state.reciprocal_div(freq_reciprocal);
        // Calculate self.state % freq.
        let rem = self.state - quot * freq;
        self.state = quot * SUM_FREQ + rem + cfreq;
    }

    /// Finish writing to the stream.
    ///
    /// This method must be called after all data has been written to the
    /// stream.
    ///
    /// Panics when less number of bytes were written via
    /// [`write_rev`](Self::write_rev) than were supplied to [`Self::new`].
    ///
    /// Returns stream statistics when `stats` feature is enabled.
    pub fn finish(self, output: &mut impl Output) -> Stats {
        let mut buf = [0_u8; FOOTER_BUF_LEN];
        let mut footer = SliceOutput(&mut buf[..]);
        Compact(self.state).encode_to(&mut footer);
        self.non_zero_freqs.encode_to(&mut footer);
        // TODO we don't need to write the last/first frequency because we know their
        // sum
        for i in self.non_zero_freqs.iter() {
            let f = self.freq(i);
            Compact(f).encode_to(&mut footer);
        }
        let footer_len = FOOTER_BUF_LEN - footer.0.len();
        let footer = &mut buf[..footer_len];
        // We write the footer in reverse to not encode its length: we read it in
        // reverse too.
        footer.reverse();
        output.push_slice_rev(footer);
        #[cfg(not(feature = "stats"))]
        {
            Stats
        }
        #[cfg(feature = "stats")]
        Stats {
            data_len: self.total_out.into(),
            footer_len: footer_len as u32,
            num_freqs: self.non_zero_freqs.len() as u32,
            count: 1,
        }
    }

    fn freq(&self, i: usize) -> u16 {
        self.cfreqs[i + 1] - self.cfreqs[i]
    }
}

errors! {
    (InvalidRansStream "Invalid rANS stream")
}

#[doc(hidden)]
impl From<jam_codec::Error> for InvalidRansStream {
    fn from(_: jam_codec::Error) -> InvalidRansStream {
        InvalidRansStream
    }
}

/// This is a range variant of Assymertic Numeral System (RANS) decoder.
///
/// See [`Encoder`] for more information.
pub struct Decoder {
    state: u32,
    cfreqs: CumulativeFrequencies,
}

impl Decoder {
    /// Creates new decoder for the provided input.
    pub fn new(input: &mut impl Input) -> Result<Self, InvalidRansStream> {
        let footer = &mut AnsInputAsJamInput(input);
        let Compact(state) = Compact::<u32>::decode(footer)?;
        // TODO check state
        let non_zero_freqs = FreqBitSet::decode(footer)?;
        let mut freqs = [0; NUM_FREQS];
        let mut sum_freqs: u32 = 0;
        let mut has_freqs = false;
        for i in non_zero_freqs.iter() {
            let Compact(f) = Compact::<u16>::decode(footer)?;
            freqs[i] = f;
            sum_freqs += u32::from(f);
            has_freqs = true;
        }
        if has_freqs && sum_freqs != SUM_FREQ {
            // Invalid frequencies.
            return Err(InvalidRansStream);
        }
        let cfreqs = cumulative_frequencies(&freqs);
        Ok(Self { state, cfreqs })
    }

    /// Decodes `output.len()` bytes from the `input` and copies them to
    /// `output`.
    pub fn read_exact(
        &mut self,
        input: &mut impl Input,
        mut output: &mut [u8],
    ) -> Result<(), InvalidRansStream> {
        while !output.is_empty() {
            let symbol = self.pop(input)?;
            output[0] = symbol;
            output = &mut output[1..];
        }
        Ok(())
    }

    /// Decode individual symbol.
    pub fn pop(&mut self, input: &mut impl Input) -> Result<u8, InvalidRansStream> {
        const MASK: u32 = SUM_FREQ - 1;
        let cf = (self.state & MASK) as u16;
        let symbol = self.cfreq_to_symbol(cf).ok_or(InvalidRansStream)?;
        let i = usize::from(symbol);
        let freq = u32::from(self.freq(i));
        debug_assert_ne!(0, freq);
        let cfreq = u32::from(self.cfreqs[i]);
        self.state = (self.state / SUM_FREQ) * freq + (self.state & MASK) - cfreq;
        while self.state < INITIAL_STATE {
            let byte = input.pop().map_err(|_| InvalidRansStream)?;
            self.state = (self.state << 8) | u32::from(byte);
        }
        Ok(symbol)
    }

    fn cfreq_to_symbol(&self, cf: u16) -> Option<u8> {
        self.cfreqs
            .iter()
            .skip(1)
            .position(|cfreq| cf < *cfreq)
            .map(|i| i as u8)
    }

    fn freq(&self, i: usize) -> u16 {
        self.cfreqs[i + 1] - self.cfreqs[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use proptest::prelude::*;

    #[test]
    fn encoder_simple_works() {
        let input = "Hello world!".as_bytes();
        let mut encoder = Encoder::new(input);
        let mut output = Vec::new();
        encoder.write_rev(input, &mut output);
        let encoder_state = encoder.state;
        let encoder_cfreqs = encoder.cfreqs;
        encoder.finish(&mut output);
        let mut decoder = Decoder::new(&mut output).unwrap();
        assert_eq!(encoder_state, decoder.state);
        assert_eq!(encoder_cfreqs, decoder.cfreqs);
        let mut decoded = vec![0; input.len()];
        decoder.read_exact(&mut output, &mut decoded[..]).unwrap();
        assert_eq!(input, decoded.as_slice());
    }

    #[test]
    fn encoder_works() {
        proptest!(|(input: Vec<u8>)| {
            let mut encoder = Encoder::new(&input);
            let mut output = Vec::new();
            encoder.write_rev(&input, &mut output);
            let encoder_state = encoder.state;
            let encoder_cfreqs = encoder.cfreqs;
            encoder.finish(&mut output);
            let mut decoder = Decoder::new(&mut output).unwrap();
            assert_eq!(encoder_state, decoder.state);
            assert_eq!(encoder_cfreqs, decoder.cfreqs);
            let mut decoded = vec![0; input.len()];
            decoder.read_exact(&mut output, &mut decoded[..]).unwrap();
            assert_eq!(input, decoded.as_slice());
        });
    }

    #[test]
    fn symbol_frequencies_works() {
        proptest!(|(input: Vec<u8>)| {
            let freqs = symbol_frequencies(&input);
            assert_eq!(input.len() as u32, freqs.iter().copied().sum::<u32>());
        });
    }

    #[test]
    fn normalize_frequencies_works() {
        proptest!(|(input: Vec<u8>)| {
            let freqs = symbol_frequencies(&input);
            let (freqs, _non_zero_freqs) = normalize_frequencies(freqs);
            let sum = freqs.iter().copied().sum::<u16>();
            assert!(input.is_empty() || SUM_FREQ == u32::from(sum), "sum = {sum}, freqs = {freqs:?}");
        });
    }

    #[test]
    fn normalize_frequencies_edge_cases() {
        let mut freqs = [1_u32; NUM_FREQS];
        freqs[0] = SUM_FREQ;
        let (freqs, _non_zero_freqs) = normalize_frequencies(freqs);
        let sum = freqs.iter().copied().sum::<u16>();
        assert!(
            SUM_FREQ == u32::from(sum),
            "sum = {sum}, SUM_FREQ = {SUM_FREQ}, freqs = {freqs:?}"
        );
    }

    #[test]
    fn max_footer_len_is_correct() {
        // Try to produce the longest possible footer.
        for step in 1..=3 {
            let input: Vec<u8> = (0..u8::MAX).step_by(step).collect();
            let mut encoder = Encoder::new(&input);
            let mut output = Vec::new();
            encoder.state = u32::MAX;
            encoder.finish(&mut output);
            assert!(
                output.len() <= FOOTER_BUF_LEN,
                "output length = {}",
                output.len()
            );
        }
    }
}
