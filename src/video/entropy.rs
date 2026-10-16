//! Entropy coding of the coefficient deltas of one plane.
//!
//! Each delta is coded as one rANS symbol. The symbol is the zigzag value of
//! the delta, or [`ESCAPE_SYMBOL`] if the zigzag value is 255 or more.
//! [`PlaneHeader::escapes`] contains the zigzag values of the escaped deltas.
//!
//! Each symbol uses one of two frequency tables (contexts). Context 0 is for
//! coefficients that were 0 in the previous frame, context 1 for all others.

use crate::{
    errors,
    rans::{self, DecoderTables, FrequencyTable, Stats, StreamHeader, WordReader, WordStack},
    video::{
        Sample,
        sample::{PackedSample, PackedSamplePair},
    },
};
use alloc::vec::Vec;
use jam_codec::{Decode, Encode};

errors! {
    (InvalidEntropyStream "Invalid entropy-coded stream")
}

#[doc(hidden)]
impl From<rans::InvalidRansStream> for InvalidEntropyStream {
    fn from(_: rans::InvalidRansStream) -> InvalidEntropyStream {
        InvalidEntropyStream
    }
}

const ESCAPE_SYMBOL: u8 = u8::MAX;
pub const CONTEXT_COUNT: usize = 2;

/// Maps 0, -1, 1, -2, 2, ... to 0, 1, 2, 3, 4, ..., so that values with a
/// small magnitude get small symbols.
#[inline(always)]
fn zigzag(value: i16) -> u16 {
    ((value << 1) ^ (value >> 15)) as u16
}

#[inline(always)]
fn unzigzag(value: u16) -> i16 {
    ((value >> 1) as i16) ^ -((value & 1) as i16)
}

/// Returns `min(zigzag(value), ESCAPE_SYMBOL)` without a branch.
#[inline(always)]
fn symbol(value: i16) -> u8 {
    let zigzag = zigzag(value);
    (zigzag | u16::from(zigzag >> u8::BITS != 0).wrapping_neg()) as u8
}

/// Returns the context and the symbol of a sample after
/// [`haar::forward`](super::haar::forward).
///
/// At this point `current` contains the delta and `previous` contains the new
/// coefficient. The coefficient of the previous frame is `previous - current`,
/// which is 0 if and only if `previous` equals `current`.
#[inline(always)]
fn context_and_symbol(sample: &PackedSample) -> (usize, u8) {
    let current = sample.current();
    (usize::from(sample.previous() != current), symbol(current))
}

pub type Encoder = rans::Encoder<CONTEXT_COUNT>;
pub type Tables = DecoderTables<CONTEXT_COUNT>;

#[derive(Decode)]
pub struct PlaneHeader {
    stream: StreamHeader<CONTEXT_COUNT>,
    /// Zigzag values of the escaped deltas, in sample order.
    escapes: Vec<u16>,
}

/// Counts the symbols of each context. The loop loads two samples with one
/// `u64` load to halve the number of loads.
#[inline(never)]
fn symbol_counts(input: &[PackedSample]) -> [[u32; rans::NUM_FREQS]; CONTEXT_COUNT] {
    let mut counts = [[0_u32; rans::NUM_FREQS]; CONTEXT_COUNT];
    let mut count = |sample: &PackedSample| {
        let (context, symbol) = context_and_symbol(sample);
        counts[context][usize::from(symbol)] += 1;
    };
    let (prefix, pairs, suffix) = PackedSamplePair::split(input);
    prefix.iter().chain(suffix).for_each(&mut count);
    let (chunks, remainder) = pairs.as_chunks::<4>();
    for chunk in chunks {
        for pair in chunk {
            pair.samples().iter().for_each(&mut count);
        }
    }
    for pair in remainder {
        pair.samples().iter().for_each(&mut count);
    }
    counts
}

#[inline(never)]
fn push_samples(encoder: &mut Encoder, input: &[PackedSample], words: &mut WordStack) {
    encoder.push_all(input, words, context_and_symbol);
}

/// Fast path for the common case of a plane without escapes. It is the loop of
/// [`decode`] without the escape handling.
#[inline(never)]
fn pop_samples(
    decoder: &mut rans::Decoder<'_, CONTEXT_COUNT>,
    stream: &mut rans::StreamReader<'_>,
    output: &mut [Sample],
) {
    decoder.pop_all(
        output,
        stream,
        |sample| usize::from(sample.previous != 0),
        |sample, symbol| sample.current = unzigzag(u16::from(symbol)),
    );
}

/// Encodes the deltas of one plane. Appends the plane header to `header` and
/// the rANS words to `words`.
pub fn encode(
    input: &[Sample],
    encoder: &mut Encoder,
    words: &mut WordStack,
    header: &mut Vec<u8>,
) -> Stats {
    assert!(input.len() <= rans::MAX_INPUT_LEN);
    let packed_input = PackedSample::slice(input);
    let counts = symbol_counts(packed_input);
    // Escapes are rare, so collect them in a second pass only if there are any.
    let escapes = if counts
        .iter()
        .any(|counts| counts[usize::from(ESCAPE_SYMBOL)] != 0)
    {
        input
            .iter()
            .map(|sample| zigzag(sample.current))
            .filter(|zigzag| *zigzag >= u16::from(ESCAPE_SYMBOL))
            .collect()
    } else {
        Vec::new()
    };
    let tables = counts.map(FrequencyTable::new);
    encoder.reset(&tables);
    let words_start = words.len();
    push_samples(encoder, packed_input, words);
    #[cfg(feature = "stats")]
    let num_freqs = tables.iter().map(FrequencyTable::len).sum::<usize>() as u32;
    let header_start = header.len();
    rans::encode_stream_header(encoder.state(), words.len() - words_start, &tables, header);
    escapes.encode_to(header);
    #[cfg(not(feature = "stats"))]
    {
        let _ = (words_start, header_start);
        Stats
    }
    #[cfg(feature = "stats")]
    Stats {
        data_len: (size_of::<rans::Word>() * (words.len() - words_start)) as u64,
        header_len: (header.len() - header_start) as u32,
        num_freqs,
        count: 1,
    }
}

impl PlaneHeader {
    pub fn word_count(&self) -> usize {
        self.stream.word_count as usize
    }
}

/// Decodes a plane header and checks its word count against `sample_count`.
///
/// The encoder writes at most one word per sample, so a larger word count
/// means a corrupted stream.
pub fn decode_header(
    input: &mut impl jam_codec::Input,
    sample_count: usize,
) -> Result<PlaneHeader, InvalidEntropyStream> {
    let header = PlaneHeader::decode(input).map_err(|_| InvalidEntropyStream)?;
    if header.word_count() > sample_count {
        return Err(InvalidEntropyStream);
    }
    Ok(header)
}

/// Decodes the deltas of one plane into `current`.
///
/// Before [`haar::backward`](super::haar::backward), `previous` contains the
/// coefficient of the previous frame, which selects the context.
pub fn decode(
    header: &PlaneHeader,
    words: &mut WordReader<'_>,
    output: &mut [Sample],
    tables: &mut Tables,
) -> Result<(), InvalidEntropyStream> {
    let has_escape_symbol = header
        .stream
        .tables
        .iter()
        .any(|table| table.frequency(ESCAPE_SYMBOL) != 0);
    // `pop_samples` decodes the escape symbol as a plain zigzag value, so the
    // tables of a plane without escapes must not contain the escape symbol.
    if has_escape_symbol == header.escapes.is_empty() {
        return Err(InvalidEntropyStream);
    }
    tables.fill(&header.stream.tables);
    let mut decoder = rans::Decoder::new(header.stream.state, tables)?;
    let mut stream = words.stream(header.stream.word_count)?;
    let sample_context = |sample: &Sample| usize::from(sample.previous != 0);
    if header.escapes.is_empty() {
        pop_samples(&mut decoder, &mut stream, output);
    } else {
        let mut escapes = header.escapes.iter();
        let mut has_missing_escapes = false;
        decoder.pop_all(output, &mut stream, sample_context, |sample, symbol| {
            let zigzag = if symbol == ESCAPE_SYMBOL {
                escapes.next().copied().unwrap_or_else(|| {
                    has_missing_escapes = true;
                    0
                })
            } else {
                u16::from(symbol)
            };
            sample.current = unzigzag(zigzag);
        });
        if has_missing_escapes || escapes.next().is_some() {
            return Err(InvalidEntropyStream);
        }
    }
    stream.finish()?;
    decoder.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rans::WordBuffer, video::yuv::tests::currents};
    use alloc::vec::Vec;
    use proptest::prelude::*;

    #[test]
    fn zigzag_works() {
        for value in i16::MIN..=i16::MAX {
            assert_eq!(value, unzigzag(zigzag(value)));
        }
        assert_eq!([0, 1, 2, 3, 4], [0, -1, 1, -2, 2].map(zigzag));
    }

    fn encoded_plane(deltas: &[i16], previous_coefficients: &[i16]) -> (PlaneHeader, WordBuffer) {
        // Encoder samples after `haar::forward`, see `context_and_symbol`.
        let encoder_samples: Vec<Sample> = deltas
            .iter()
            .zip(previous_coefficients)
            .map(|(&delta, &previous)| Sample {
                current: delta,
                previous: previous.wrapping_add(delta),
            })
            .collect();
        let mut words = WordStack::default();
        let mut header = Vec::new();
        encode(
            &encoder_samples,
            &mut Encoder::default(),
            &mut words,
            &mut header,
        );
        let header = decode_header(&mut &header[..], deltas.len()).unwrap();
        let mut buffer = WordBuffer::default();
        buffer
            .read_from(&mut words.as_bytes(), words.len())
            .unwrap();
        (header, buffer)
    }

    fn decoded_plane(
        header: &PlaneHeader,
        buffer: &WordBuffer,
        previous_coefficients: &[i16],
    ) -> Result<Vec<i16>, InvalidEntropyStream> {
        // Decoder samples before `haar::backward`, see `decode`.
        let mut decoder_samples: Vec<Sample> = previous_coefficients
            .iter()
            .map(|&previous| Sample {
                current: 0,
                previous,
            })
            .collect();
        decode(
            header,
            &mut buffer.reader(),
            &mut decoder_samples,
            &mut Tables::default(),
        )?;
        Ok(currents(&decoder_samples))
    }

    fn assert_round_trip(deltas: &[i16], previous_coefficients: &[i16]) {
        let (header, buffer) = encoded_plane(deltas, previous_coefficients);
        let decoded = decoded_plane(&header, &buffer, previous_coefficients).unwrap();
        assert_eq!(deltas, decoded);
    }

    /// A corrupted header can contain more escapes than the plane contains
    /// escape symbols, or more escapes than samples. The decoder must return
    /// an error and not a plane from a corrupted stream.
    #[test]
    fn extra_escape_rejection() {
        let deltas = [1000, 0, 0, 0];
        let previous_coefficients = [0; 4];
        let (mut header, buffer) = encoded_plane(&deltas, &previous_coefficients);
        header.escapes.push(1);
        assert!(decoded_plane(&header, &buffer, &previous_coefficients).is_err());
        header.escapes.extend([1; 4]);
        assert!(decoded_plane(&header, &buffer, &previous_coefficients).is_err());
    }

    #[test]
    fn encode_simple_works() {
        let deltas = [i16::MIN, 2, -1, 0, 1, 2, i16::MAX, 127, -128, 128];
        assert_round_trip(&deltas, &[0; 10]);
        assert_round_trip(
            &deltas,
            &[0, 1, -1, 2, -2, 3, 100, -100, i16::MIN, i16::MAX],
        );
    }

    #[test]
    fn encode_works() {
        proptest!(|(samples: Vec<(i16, i16)>)| {
            let (deltas, previous_coefficients): (Vec<i16>, Vec<i16>) = samples.into_iter().unzip();
            assert_round_trip(&deltas, &previous_coefficients);
        });
    }

    #[test]
    fn encode_small_values_works() {
        proptest!(|(samples in proptest::collection::vec((-130_i16..=130, -3_i16..=3), 0..3000))| {
            let (deltas, previous_coefficients): (Vec<i16>, Vec<i16>) = samples.into_iter().unzip();
            assert_round_trip(&deltas, &previous_coefficients);
        });
    }
}
