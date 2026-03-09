use crate::{rans::NUM_FREQS, ToUsize};
use core::{iter::FusedIterator, ops::RangeInclusive};
use jam_codec::{Decode, Encode};

const _: () = assert!(NUM_FREQS.is_multiple_of(64));
const BITSET_LEN: usize = NUM_FREQS / 64;

#[derive(Debug, PartialEq, Eq)]
enum EncodedForm {
    /// Indices of set bits.
    Ones,
    /// Indices of unset bits.
    Zeros,
    /// All bits.
    Raw,
    /// Ranges of indices of set bits.
    OnesRanges,
    /// Ranges of indices of unset bits.
    ZerosRanges,
}

pub struct FreqBitSet {
    data: [u64; BITSET_LEN],
}

impl FreqBitSet {
    const RAW_ENCODED_LEN: usize = core::mem::size_of::<u64>() * BITSET_LEN;

    pub const fn new() -> Self {
        Self {
            data: [0; BITSET_LEN],
        }
    }

    pub fn set(&mut self, index: usize) {
        let i = index / 64;
        let j = index % 64;
        self.data[i] |= 1_u64 << j;
    }

    pub fn clear(&mut self, index: usize) {
        let i = index / 64;
        let j = index % 64;
        self.data[i] &= !(1_u64 << j);
    }

    /// Returns the number of set bits.
    pub fn len(&self) -> usize {
        self.data.iter().map(|x| x.count_ones().to_usize()).sum()
    }

    /// Returns an iterator over indices of set bits.
    pub fn iter(&self) -> impl Iterator<Item = usize> + use<'_> {
        Iter::new(self.data.iter().copied())
    }

    /// Returns an iterator over indices of unset bits.
    pub fn zeros(&self) -> impl Iterator<Item = usize> + use<'_> {
        Iter::new(self.data.iter().map(|mask| !*mask))
    }

    fn encoded_len_and_form(&self) -> (usize, EncodedForm) {
        let num_ones = self.len();
        let num_zeros = BITSET_LEN * 64 - num_ones;
        let num_ones_ranges = RangesIter::new(self.iter()).count();
        let num_zeros_ranges = RangesIter::new(self.zeros()).count();
        let (len, form) = [
            (Self::RAW_ENCODED_LEN, EncodedForm::Raw),
            (num_ones, EncodedForm::Ones),
            (num_zeros, EncodedForm::Zeros),
            (2 * num_ones_ranges, EncodedForm::OnesRanges),
            (2 * num_zeros_ranges, EncodedForm::ZerosRanges),
        ]
        .into_iter()
        .min_by_key(|(len, _form)| *len)
        .expect("The array is non-empty");
        (len, form)
    }
}

impl Encode for FreqBitSet {
    fn encode_to<O: jam_codec::Output + ?Sized>(&self, output: &mut O) {
        let (len, form) = self.encoded_len_and_form();
        let header_byte = match form {
            EncodedForm::Raw => 0,
            EncodedForm::Ones => ((len as u8) << 3) | 1,
            EncodedForm::Zeros => ((len as u8) << 3) | 2,
            EncodedForm::OnesRanges => ((len as u8) << 3) | 3,
            EncodedForm::ZerosRanges => ((len as u8) << 3) | 4,
        };
        output.push_byte(header_byte);
        match form {
            EncodedForm::Ones => {
                for i in self.iter() {
                    output.push_byte(i as u8);
                }
            }
            EncodedForm::Zeros => {
                for i in self.zeros() {
                    output.push_byte(i as u8);
                }
            }
            EncodedForm::OnesRanges => {
                for range in RangesIter::new(self.iter()) {
                    debug_assert!(*range.start() <= usize::from(u8::MAX));
                    debug_assert!(*range.end() <= usize::from(u8::MAX));
                    output.push_byte(*range.start() as u8);
                    output.push_byte(*range.end() as u8);
                }
            }
            EncodedForm::ZerosRanges => {
                for range in RangesIter::new(self.zeros()) {
                    debug_assert!(*range.start() <= usize::from(u8::MAX));
                    debug_assert!(*range.end() <= usize::from(u8::MAX));
                    output.push_byte(*range.start() as u8);
                    output.push_byte(*range.end() as u8);
                }
            }
            EncodedForm::Raw => {
                for elem in self.data {
                    output.write(&elem.to_le_bytes());
                }
            }
        }
    }

    fn encoded_size(&self) -> usize {
        1 + self.encoded_len_and_form().0
    }
}

impl Decode for FreqBitSet {
    fn decode<I: jam_codec::Input>(input: &mut I) -> Result<Self, jam_codec::Error> {
        let header_byte = input.read_byte()?;
        let form = match header_byte & 0b111 {
            0 => EncodedForm::Raw,
            1 => EncodedForm::Ones,
            2 => EncodedForm::Zeros,
            3 => EncodedForm::OnesRanges,
            4 => EncodedForm::ZerosRanges,
            _ => return Err("InvalidRansStream".into()),
        };
        let len = (header_byte >> 3) as usize;
        if len > Self::RAW_ENCODED_LEN {
            // Someone encoded bitset in a non-optimal way.
            return Err("InvalidRansStream".into());
        }
        match form {
            EncodedForm::Ones => {
                let mut bits = Self {
                    data: [0; BITSET_LEN],
                };
                for _ in 0..len {
                    let i = u8::decode(input)?;
                    bits.set(i as usize);
                }
                Ok(bits)
            }
            EncodedForm::Zeros => {
                let mut bits = Self {
                    data: [u64::MAX; BITSET_LEN],
                };
                for _ in 0..len {
                    let i = u8::decode(input)?;
                    bits.clear(i as usize);
                }
                Ok(bits)
            }
            EncodedForm::OnesRanges => {
                let len = len / 2;
                let mut bits = Self {
                    data: [0; BITSET_LEN],
                };
                for _ in 0..len {
                    let start = u8::decode(input)?;
                    let end = u8::decode(input)?;
                    for i in start..=end {
                        bits.set(i as usize);
                    }
                }
                Ok(bits)
            }
            EncodedForm::ZerosRanges => {
                let len = len / 2;
                let mut bits = Self {
                    data: [u64::MAX; BITSET_LEN],
                };
                for _ in 0..len {
                    let start = u8::decode(input)?;
                    let end = u8::decode(input)?;
                    for i in start..=end {
                        bits.clear(i as usize);
                    }
                }
                Ok(bits)
            }
            EncodedForm::Raw => {
                let mut data = [0; BITSET_LEN];
                for elem in data.iter_mut() {
                    *elem = u64::decode(input)?;
                }
                Ok(Self { data })
            }
        }
    }
}

/// An iterator over set bits.
pub struct Iter<I> {
    inner: I,
    base: usize,
    elem: Option<u64>,
}

impl<I: Iterator<Item = u64>> Iter<I> {
    fn new(mut inner: I) -> Self {
        let elem = inner.next();
        Self {
            inner,
            base: 0,
            elem,
        }
    }
}

impl<I: Iterator<Item = u64>> Iterator for Iter<I> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(elem) = self.elem.as_mut() {
            if *elem == 0 {
                self.elem = self.inner.next();
                self.base += 64;
                continue;
            }
            let j = elem.trailing_zeros();
            *elem ^= *elem & 0_u64.wrapping_sub(*elem);
            return Some(self.base + j as usize);
        }
        None
    }
}

impl<I: Iterator<Item = u64>> FusedIterator for Iter<I> {}

/// An iterator that transforms an iterator over `usize` into an iterator of
/// `RangeInclusive<usize>` (consecutive ranges).
///
/// Doesn't work for unsorted sequences.
struct RangesIter<I: Iterator> {
    inner: I,
    start: Option<I::Item>,
    end: usize,
}

impl<I: Iterator> RangesIter<I> {
    const fn new(inner: I) -> Self {
        Self {
            inner,
            start: None,
            end: 0,
        }
    }
}

impl<I: Iterator<Item = usize>> Iterator for RangesIter<I> {
    type Item = RangeInclusive<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        for i in self.inner.by_ref() {
            match self.start {
                Some(start) => {
                    if self.end.saturating_add(1) != i {
                        let end = self.end;
                        self.start = Some(i);
                        self.end = i;
                        return Some(start..=end);
                    }
                }
                None => {
                    self.start = Some(i);
                }
            }
            self.end = i;
        }
        if let Some(start) = self.start.take() {
            return Some(start..=self.end);
        }
        None
    }
}

impl<I: Iterator<Item = usize> + FusedIterator> FusedIterator for RangesIter<I> {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use proptest::{collection, prelude::*};

    #[test]
    fn bitset_iter_works() {
        assert_eq!(
            vec![0, 2, 4, 6],
            Iter::new([0b01010101_u64, 0, 0, 0].iter().copied()).collect::<Vec<_>>(),
        );
        assert_eq!(
            vec![1, 3, 5, 7, 64, 64 + 2, 64 + 4, 64 + 6],
            Iter::new([0b10101010_u64, 0b1010101_u64, 0, 0].iter().copied()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ranges_iter_works() {
        assert_eq!(
            RangesIter::new([0_usize, 1, 2, 3, 5, 7, 8, 9].into_iter()).collect::<Vec<_>>(),
            vec![0_usize..=3, 5..=5, 7..=9]
        );
        assert_eq!(RangesIter::new([].into_iter()).collect::<Vec<_>>(), vec![]);
        assert_eq!(
            RangesIter::new(0_usize..10).collect::<Vec<_>>(),
            vec![0_usize..=9]
        );
    }

    #[test]
    fn bitset_io_zeros_ranges() {
        let mut bitset = FreqBitSet::new();
        bitset.set(0);
        bitset.set(1);
        bitset.set(254);
        bitset.set(255);
        assert_eq!(EncodedForm::ZerosRanges, bitset.encoded_len_and_form().1);
        let mut bytes = Vec::new();
        bitset.encode_to(&mut bytes);
        assert_eq!(bitset.encoded_size(), bytes.len());
        let mut slice = &bytes[..];
        let actual = FreqBitSet::decode(&mut slice).unwrap();
        assert_eq!(
            bitset.data,
            actual.data,
            "form = {:?}",
            bitset.encoded_len_and_form().1
        );
    }

    #[test]
    fn bitset_io_simple_works() {
        for step in 1..=3 {
            for flip in [false, true] {
                for len in 0..256 {
                    let mut bitset = FreqBitSet::new();
                    for i in (0..len).step_by(step) {
                        bitset.set(i);
                    }
                    if flip {
                        for mask in bitset.data.iter_mut() {
                            *mask = !*mask;
                        }
                    }
                    std::panic::catch_unwind(|| {
                        let mut bytes = Vec::new();
                        bitset.encode_to(&mut bytes);
                        assert_eq!(bitset.encoded_size(), bytes.len());
                        let mut slice = &bytes[..];
                        let actual = FreqBitSet::decode(&mut slice).unwrap();
                        assert_eq!(
                            bitset.data,
                            actual.data,
                            "form = {:?}, step = {step}, flip = {flip:?}, len = {len}",
                            bitset.encoded_len_and_form().1
                        );
                    })
                    .inspect_err(|_| {
                        std::eprintln!("Panic context: len = {len}");
                    })
                    .unwrap();
                }
            }
        }
    }

    fn bitset_vec() -> impl Strategy<Value = Vec<bool>> {
        collection::vec(any::<bool>(), 256)
    }

    #[test]
    fn bitset_io_works() {
        proptest!(|(bitset_vec in bitset_vec())| {
            let mut bitset = FreqBitSet::new();
            for (i, b) in bitset_vec.into_iter().enumerate() {
                if b {
                    bitset.set(i);
                }
            }
            let mut bytes = Vec::new();
            bitset.encode_to(&mut bytes);
            assert_eq!(bitset.encoded_size(), bytes.len());
            let mut slice = &bytes[..];
            let actual = FreqBitSet::decode(&mut slice).unwrap();
            assert_eq!(bitset.data, actual.data);
        });
    }
}
