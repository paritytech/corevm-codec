//! Range Asymmetric Numeral System (rANS) encoder/decoder.
//!
//! The code is based on <https://github.com/rygorous/ryg_rans>.
//! The original paper is <https://arxiv.org/pdf/1311.2540>.
//!
//! This variant has a 32-bit state and 16-bit words. Each symbol writes or
//! reads at most one word.
//!
//! The hot loops avoid divisions and data-dependent branches, because both are
//! expensive under PolkaVM gas metering.

use crate::errors;
use alloc::{boxed::Box, vec, vec::Vec};
use jam_codec::{Compact, Decode, Encode};

mod bitset;
mod io;
#[cfg(feature = "stats")]
mod stats;

use self::bitset::FreqBitSet;

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

/// Maximum number of symbols in one stream. Symbol counts and the word count
/// of a stream are `u32`.
pub const MAX_INPUT_LEN: usize = u32::MAX as usize;

// Total number of frequencies.
pub(crate) const NUM_FREQS: usize = 256;
// Total number of cumulative frequencies.
const NUM_CFREQS: usize = NUM_FREQS + 1;
// Frequencies are scaled to a sum of `2^SCALE_BITS`. 12 bits is the largest
// scale for which a decoder table entry (8-bit symbol, 12-bit frequency - 1 and
// 12-bit offset) fits in a `u32`.
const SCALE_BITS: u32 = 12;
// The sum of all frequencies.
const SUM_FREQ: u32 = 1 << SCALE_BITS;
const FIELD_MASK: u64 = (1 << SCALE_BITS) - 1;
const WORD_BITS: u32 = Word::BITS;
// The decoder reads a new word whenever its state drops below
// `STATE_LOWER_BOUND`. For these reads to match the words that the encoder
// wrote, the encoder state must stay within `[STATE_LOWER_BOUND, 2^32)`. The
// upper bound also makes the state fit in a `u32` (see
// `EncoderSymbol::quotient`).
const STATE_LOWER_BOUND: u64 = 1 << 16;
// Encoding a symbol with frequency `f` multiplies the state by about
// `SUM_FREQ / f`. A state of `f << RENORMALIZATION_SHIFT` or more would reach
// `2^32`, so the encoder first writes the low word of the state.
const RENORMALIZATION_SHIFT: u32 = STATE_LOWER_BOUND.ilog2() - SCALE_BITS + WORD_BITS;

type RawFrequencies = [u32; NUM_FREQS];
type NormalizedFrequencies = [u16; NUM_FREQS];
type CumulativeFrequencies = [u16; NUM_CFREQS];

/// Calculates the number of occurences of each byte in the provided buffer.
fn symbol_frequencies(input: &[u8]) -> RawFrequencies {
    debug_assert!(input.len() <= MAX_INPUT_LEN);
    let mut freqs = [0; NUM_FREQS];
    for symbol in input {
        freqs[usize::from(*symbol)] += 1;
    }
    freqs
}

const MIN_FREQUENCY: u16 = 1;

/// Normalizes symbol frequencies.
///
/// Makes their sum equal to `SUM_FREQ` and every non-zero frequency at least
/// `MIN_FREQUENCY`. Returns normalized frequencies and a set of indices of
/// non-zero frequencies.
///
/// Scaling rounds each frequency and raises it to at least `MIN_FREQUENCY`, so
/// the sum can differ from `SUM_FREQ`. The function then adds the missing
/// amount to the largest frequency, or removes the excess from the largest
/// frequencies, because there the relative change is the smallest.
fn normalize_frequencies(counts: RawFrequencies) -> (NormalizedFrequencies, FreqBitSet) {
    let total: u64 = counts.iter().copied().map(u64::from).sum();
    let mut freqs = [0; NUM_FREQS];
    let mut non_zero_freqs = FreqBitSet::new();
    if total == 0 {
        return (freqs, non_zero_freqs);
    }
    let scale = (u64::from(SUM_FREQ) << 32) / total;
    let mut sum: u32 = 0;
    for (i, (count, freq)) in counts.iter().zip(freqs.iter_mut()).enumerate() {
        if *count == 0 {
            continue;
        }
        let scaled = (u64::from(*count) * scale + (1 << 31)) >> 32;
        *freq = (scaled as u16).max(MIN_FREQUENCY);
        sum += u32::from(*freq);
        non_zero_freqs.set(i);
    }
    while sum != SUM_FREQ {
        let largest = freqs
            .iter_mut()
            .max_by_key(|freq| **freq)
            .expect("non-empty frequencies");
        if sum < SUM_FREQ {
            *largest += (SUM_FREQ - sum) as u16;
            sum = SUM_FREQ;
        } else {
            let decrease = (sum - SUM_FREQ).min(u32::from(*largest - MIN_FREQUENCY));
            *largest -= decrease as u16;
            sum -= decrease;
        }
    }
    (freqs, non_zero_freqs)
}

/// Calculates cumulative frequencies from normalized ones.
///
/// Element `i` is the sum of the frequencies of the symbols before `i`, so the
/// first element is 0.
fn cumulative_frequencies(freqs: &NormalizedFrequencies) -> CumulativeFrequencies {
    let mut cfreqs = [0; NUM_CFREQS];
    let mut sum = 0;
    for (f, cf) in freqs.iter().zip(cfreqs.iter_mut().skip(1)) {
        sum += f;
        *cf = sum;
    }
    cfreqs
}

/// Symbol frequencies, normalized to a sum of `SUM_FREQ`, or all 0 for a
/// stream without symbols.
pub struct FrequencyTable {
    freqs: NormalizedFrequencies,
    non_zero_freqs: FreqBitSet,
}

impl FrequencyTable {
    pub fn new(counts: RawFrequencies) -> Self {
        let (freqs, non_zero_freqs) = normalize_frequencies(counts);
        Self {
            freqs,
            non_zero_freqs,
        }
    }

    pub fn frequency(&self, symbol: u8) -> u16 {
        self.freqs[usize::from(symbol)]
    }

    pub fn len(&self) -> usize {
        self.non_zero_freqs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Encode for FrequencyTable {
    fn encode_to<O: jam_codec::Output + ?Sized>(&self, output: &mut O) {
        self.non_zero_freqs.encode_to(output);
        for i in self.non_zero_freqs.iter() {
            Compact(self.freqs[i]).encode_to(output);
        }
    }
}

impl Decode for FrequencyTable {
    fn decode<I: jam_codec::Input>(input: &mut I) -> Result<Self, jam_codec::Error> {
        let non_zero_freqs = FreqBitSet::decode(input)?;
        let mut freqs = [0; NUM_FREQS];
        let mut sum_freqs: u32 = 0;
        for i in non_zero_freqs.iter() {
            let Compact(f) = Compact::<u16>::decode(input)?;
            if f == 0 {
                return Err("Zero frequency".into());
            }
            freqs[i] = f;
            sum_freqs += u32::from(f);
        }
        if sum_freqs != 0 && sum_freqs != SUM_FREQ {
            return Err("Invalid frequencies".into());
        }
        Ok(Self {
            freqs,
            non_zero_freqs,
        })
    }
}

/// Encoder parameters of one symbol packed into a `u64`, so that encoding a
/// symbol takes a single load.
#[derive(Clone, Copy, Default)]
struct EncoderSymbol(u64);

const FREQUENCY_OFFSET: u32 = u16::BITS;
const RECIPROCAL_OFFSET: u32 = u32::BITS;

impl EncoderSymbol {
    fn new(cumulative_frequency: u16, frequency: u16) -> Self {
        debug_assert!(frequency >= MIN_FREQUENCY);
        let shift = u16::BITS - (frequency - 1).leading_zeros();
        let reciprocal =
            (1_u64 << (u32::BITS + shift)).div_ceil(u64::from(frequency)) - (1 << u32::BITS);
        Self(
            u64::from(cumulative_frequency)
                | (u64::from(frequency - 1) << FREQUENCY_OFFSET)
                | (reciprocal << RECIPROCAL_OFFSET),
        )
    }

    #[inline(always)]
    fn bias(self) -> u64 {
        u64::from(self.0 as u16)
    }

    #[inline(always)]
    fn frequency_minus_one(self) -> u64 {
        u64::from(self.0 as u32 >> FREQUENCY_OFFSET)
    }

    /// Returns `true` if the encoder must write the low word of the state
    /// before it encodes this symbol.
    #[inline(always)]
    fn is_full(self, state: u64) -> bool {
        state >> RENORMALIZATION_SHIFT > self.frequency_minus_one()
    }

    /// Computes `state / frequency` by multiplying with a precomputed
    /// reciprocal instead of dividing (Granlund and Montgomery, _"Division by
    /// Invariant Integers using Multiplication"_, 1994).
    ///
    /// This only works for `state < 2^32`. For larger states the product can
    /// overflow a `u64` and the result can be wrong. That is why the state is
    /// 32-bit.
    #[inline(always)]
    fn quotient(self, state: u64) -> u64 {
        let reciprocal = self.0 >> RECIPROCAL_OFFSET;
        let shift = u64::BITS - self.frequency_minus_one().leading_zeros();
        (((state * reciprocal) >> u32::BITS) + state) >> shift
    }

    #[inline(always)]
    fn encode(self, state: u64) -> u64 {
        let quotient = self.quotient(state);
        let remainder = state - quotient * (self.frequency_minus_one() + 1);
        (quotient << SCALE_BITS) + remainder + self.bias()
    }
}

/// rANS encoder with `TABLES` frequency tables. The caller selects the table
/// (context) of each symbol.
pub struct Encoder<const TABLES: usize> {
    state: u64,
    symbols: [[EncoderSymbol; NUM_FREQS]; TABLES],
}

impl<const TABLES: usize> Default for Encoder<TABLES> {
    fn default() -> Self {
        Self {
            state: STATE_LOWER_BOUND,
            symbols: [[EncoderSymbol::default(); NUM_FREQS]; TABLES],
        }
    }
}

impl<const TABLES: usize> Encoder<TABLES> {
    /// Starts a new stream with `tables`.
    ///
    /// Only symbols that have a non-zero frequency in `tables` may be encoded
    /// afterwards. This function only updates the parameters of those symbols,
    /// so any other symbol would be encoded with stale parameters from an
    /// earlier stream.
    pub fn reset(&mut self, tables: &[FrequencyTable; TABLES]) {
        self.state = STATE_LOWER_BOUND;
        for (symbols, table) in self.symbols.iter_mut().zip(tables) {
            let cfreqs = cumulative_frequencies(&table.freqs);
            for i in table.non_zero_freqs.iter() {
                symbols[i] = EncoderSymbol::new(cfreqs[i], table.freqs[i]);
            }
        }
    }

    /// Encode individual symbol.
    #[inline(always)]
    fn push(&mut self, (table, symbol): (usize, u8), window: &mut WordWindow<'_>) {
        let symbol = self.symbols[table][usize::from(symbol)];
        debug_assert_ne!(0, symbol.0);
        let state = self.state;
        let is_full = symbol.is_full(state);
        window.push_if(state as Word, is_full);
        let state = if is_full { state >> WORD_BITS } else { state };
        self.state = symbol.encode(state);
    }

    /// Encodes one symbol per item. `symbol` returns the table and the symbol
    /// of an item.
    ///
    /// rANS operates as a stack, so the items are encoded from last to first
    /// for [`Decoder::pop_all`] to output them from first to last. Each symbol
    /// writes at most one word, so one `write_window` call has space for
    /// `WINDOW_LEN` symbols.
    #[inline(always)]
    pub fn push_all<T>(
        &mut self,
        items: &[T],
        words: &mut WordStack,
        symbol: impl Fn(&T) -> (usize, u8),
    ) {
        let (pairs, last) = items.as_chunks::<2>();
        words.write_window(|window| {
            if let [item] = last {
                self.push(symbol(item), window);
            }
        });
        for window_pairs in pairs.rchunks(WINDOW_LEN / 2) {
            words.write_window(|window| {
                for [item0, item1] in window_pairs.iter().rev() {
                    self.push(symbol(item1), window);
                    self.push(symbol(item0), window);
                }
            });
        }
    }

    /// Returns the final state, which is always less than `2^32`.
    pub fn state(&self) -> u32 {
        self.state as u32
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

// A decoder slot contains the symbol, the frequency - 1 and the slot index
// minus the cumulative frequency of the symbol.
const SLOT_FREQUENCY_OFFSET: u32 = u8::BITS;
const SLOT_OFFSET_OFFSET: u32 = SLOT_FREQUENCY_OFFSET + SCALE_BITS;

pub struct DecoderTables<const TABLES: usize> {
    /// One slot per value of `state % SUM_FREQ` for each table, so that
    /// decoding a symbol takes a single lookup.
    slots: Box<[[u32; SUM_FREQ as usize]; TABLES]>,
}

impl<const TABLES: usize> Default for DecoderTables<TABLES> {
    fn default() -> Self {
        Self {
            slots: vec![[0; SUM_FREQ as usize]; TABLES]
                .into_boxed_slice()
                .try_into()
                .expect("invalid table count"),
        }
    }
}

impl<const TABLES: usize> DecoderTables<TABLES> {
    pub fn fill(&mut self, tables: &[FrequencyTable; TABLES]) {
        for (slots, table) in self.slots.iter_mut().zip(tables) {
            let mut slots = &mut slots[..];
            for symbol in table.non_zero_freqs.iter() {
                let frequency = table.freqs[symbol];
                let (run, rest) = slots.split_at_mut(usize::from(frequency));
                let mut entry = symbol as u32 | (u32::from(frequency - 1) << SLOT_FREQUENCY_OFFSET);
                // Fill eight slots per iteration to reduce the number of branches.
                //
                // For a symbol with frequency `SUM_FREQ`, the last `wrapping_add`
                // overflows. The overflowed value is never stored in a slot.
                let (chunks, remainder) = run.as_chunks_mut::<8>();
                for chunk in chunks {
                    for slot in chunk {
                        *slot = entry;
                        entry = entry.wrapping_add(1 << SLOT_OFFSET_OFFSET);
                    }
                }
                for slot in remainder {
                    *slot = entry;
                    entry = entry.wrapping_add(1 << SLOT_OFFSET_OFFSET);
                }
                slots = rest;
            }
        }
    }
}

pub struct Decoder<'a, const TABLES: usize> {
    state: u64,
    tables: &'a DecoderTables<TABLES>,
}

impl<'a, const TABLES: usize> Decoder<'a, TABLES> {
    pub fn new(state: u32, tables: &'a DecoderTables<TABLES>) -> Result<Self, InvalidRansStream> {
        let state = u64::from(state);
        if state < STATE_LOWER_BOUND {
            return Err(InvalidRansStream);
        }
        Ok(Self { state, tables })
    }

    /// Decode individual symbol.
    ///
    /// Always reads the next word, but consumes it only if the state dropped
    /// below the lower bound. This avoids a branch.
    #[inline(always)]
    fn pop(&mut self, table: usize, window: &mut ReverseWordWindow<'_>) -> u8 {
        let word = window.peek();
        let slot = u64::from(self.tables.slots[table][(self.state & FIELD_MASK) as usize]);
        let frequency = ((slot >> SLOT_FREQUENCY_OFFSET) & FIELD_MASK) + 1;
        let offset = slot >> SLOT_OFFSET_OFFSET;
        let state = frequency * (self.state >> SCALE_BITS) + offset;
        let is_empty = state < STATE_LOWER_BOUND;
        self.state = if is_empty {
            (state << WORD_BITS) | u64::from(word)
        } else {
            state
        };
        window.skip(usize::from(is_empty));
        slot as u8
    }

    /// Decodes one symbol per item. `table` returns the table of an item and
    /// `set` stores the decoded symbol in the item.
    ///
    /// Each symbol reads at most one word, so one `read_window` call can
    /// decode `WINDOW_LEN` symbols.
    #[inline(always)]
    pub fn pop_all<T>(
        &mut self,
        items: &mut [T],
        stream: &mut StreamReader<'_>,
        table: impl Fn(&T) -> usize,
        mut set: impl FnMut(&mut T, u8),
    ) {
        let (chunks, remainder) = items.as_chunks_mut::<4>();
        for window_chunks in chunks.chunks_mut(WINDOW_LEN / 4) {
            stream.read_window(|window| {
                for chunk in window_chunks {
                    for item in chunk {
                        let symbol = self.pop(table(item), window);
                        set(item, symbol);
                    }
                }
            });
        }
        stream.read_window(|window| {
            for item in remainder {
                let symbol = self.pop(table(item), window);
                set(item, symbol);
            }
        });
    }

    /// Returns an error unless the final state is `STATE_LOWER_BOUND`. The
    /// encoder starts from this state, so decoding a valid stream must end
    /// there.
    pub fn finish(self) -> Result<(), InvalidRansStream> {
        if self.state != STATE_LOWER_BOUND {
            return Err(InvalidRansStream);
        }
        Ok(())
    }
}

pub fn encode(input: &[u8], words: &mut WordStack, header: &mut Vec<u8>) {
    let tables = [FrequencyTable::new(symbol_frequencies(input))];
    let mut encoder = Encoder::default();
    encoder.reset(&tables);
    let start = words.len();
    encoder.push_all(input, words, |symbol| (0, *symbol));
    encode_stream_header(encoder.state(), words.len() - start, &tables, header);
}

pub fn decode(
    header: &StreamHeader<1>,
    words: &mut WordReader<'_>,
    output: &mut [u8],
    tables: &mut DecoderTables<1>,
) -> Result<(), InvalidRansStream> {
    if header.tables[0].is_empty() && !output.is_empty() {
        return Err(InvalidRansStream);
    }
    tables.fill(&header.tables);
    let mut decoder = Decoder::new(header.state, tables)?;
    let mut stream = words.stream(header.word_count)?;
    decoder.pop_all(output, &mut stream, |_| 0, |byte, symbol| *byte = symbol);
    stream.finish()?;
    decoder.finish()
}

pub fn encode_stream_header<const TABLES: usize>(
    state: u32,
    word_count: usize,
    tables: &[FrequencyTable; TABLES],
    output: &mut impl jam_codec::Output,
) {
    state.encode_to(output);
    Compact(word_count as u32).encode_to(output);
    tables.encode_to(output);
}

pub struct StreamHeader<const TABLES: usize> {
    /// Final encoder state, which is the initial decoder state.
    pub state: u32,
    pub word_count: u32,
    /// One frequency table per context.
    pub tables: [FrequencyTable; TABLES],
}

impl<const TABLES: usize> Decode for StreamHeader<TABLES> {
    fn decode<I: jam_codec::Input>(input: &mut I) -> Result<Self, jam_codec::Error> {
        Ok(Self {
            state: Decode::decode(input)?,
            word_count: Compact::<u32>::decode(input)?.0,
            tables: Decode::decode(input)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use proptest::prelude::*;

    fn assert_round_trip(input: &[u8]) {
        let mut words = WordStack::default();
        let mut encoded_header = Vec::new();
        encode(input, &mut words, &mut encoded_header);
        let header = StreamHeader::decode(&mut &encoded_header[..]).unwrap();
        let mut buffer = WordBuffer::default();
        buffer
            .read_from(&mut words.as_bytes(), words.len())
            .unwrap();
        let mut decoded = vec![0; input.len()];
        decode(
            &header,
            &mut buffer.reader(),
            &mut decoded,
            &mut DecoderTables::default(),
        )
        .unwrap();
        assert_eq!(input, decoded.as_slice());
    }

    #[test]
    fn encoder_simple_works() {
        assert_round_trip("Hello world!".as_bytes());
    }

    #[test]
    fn single_symbol_round_trip() {
        assert_round_trip(&[42; 100]);
    }

    #[test]
    fn encoder_works() {
        proptest!(|(input: Vec<u8>)| {
            assert_round_trip(&input);
        });
    }

    #[test]
    fn encoder_skewed_input_works() {
        proptest!(|(input in proptest::collection::vec(prop_oneof![9 => Just(0_u8), 1 => any::<u8>()], 0..5000))| {
            assert_round_trip(&input);
        });
    }

    #[test]
    fn encoder_symbol_quotient_is_exact() {
        for frequency in MIN_FREQUENCY..=SUM_FREQ as u16 {
            let symbol = EncoderSymbol::new(0, frequency);
            let frequency = u64::from(frequency);
            let max_state = (frequency << RENORMALIZATION_SHIFT) - 1;
            let states = (STATE_LOWER_BOUND >> WORD_BITS..frequency.min(4096))
                .chain((max_state - frequency.min(4096))..=max_state)
                .chain((0..=max_state).step_by((max_state / 4096) as usize));
            for state in states {
                assert_eq!(state / frequency, symbol.quotient(state));
            }
        }
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
        proptest!(|(counts in proptest::collection::vec(prop_oneof![1 => Just(0_u32), 1 => 0..10_u32, 1 => any::<u32>().prop_map(|count| count >> 8)], NUM_FREQS))| {
            let counts: RawFrequencies = counts.try_into().unwrap();
            let (freqs, non_zero_freqs) = normalize_frequencies(counts);
            let has_counts = counts.iter().any(|count| *count != 0);
            assert_eq!(has_counts, freqs.iter().map(|freq| u32::from(*freq)).sum::<u32>() == SUM_FREQ);
            for (i, (count, freq)) in counts.iter().zip(freqs).enumerate() {
                assert_eq!(*count != 0, freq >= MIN_FREQUENCY);
                assert_eq!(*count == 0, freq == 0);
                assert_eq!(*count != 0, non_zero_freqs.iter().any(|j| i == j));
            }
        });
    }

    #[test]
    fn normalize_frequencies_edge_cases() {
        let mut counts = [1_u32; NUM_FREQS];
        counts[0] = u32::MAX - 255;
        let (freqs, _non_zero_freqs) = normalize_frequencies(counts);
        assert_eq!(
            SUM_FREQ,
            freqs.iter().map(|freq| u32::from(*freq)).sum::<u32>()
        );
        assert!(freqs.iter().all(|freq| *freq >= MIN_FREQUENCY));
    }
}
