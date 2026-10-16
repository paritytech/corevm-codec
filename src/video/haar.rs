//! In-place 2D Haar transform, fused with quantization and delta coding.
//!
//! Level `k` transforms 2x2 blocks of the samples at positions that are
//! multiples of `2^k`. The LL coefficient replaces the top-left sample of the
//! block, and the next level transforms it again. The other coefficients, and
//! the LL coefficient of the last level, are final. They are quantized and
//! delta-coded against `previous`, the coefficient of the previous frame.
//! Intermediate LL values are stored only in `current`.
//!
//! Each level transforms, quantizes and delta-codes in a single pass, so a
//! level loads and stores each of its samples once.

use super::{
    MAX_QUANTIZATION_LEVEL, Sample, dequantize, max_transform_level, quantize, sample::PackedSample,
};
use core::num::NonZero;

#[inline]
fn forward_even(even: i16, odd: i16) -> i16 {
    (even + odd + 1) >> 1
}

#[inline]
fn forward_odd(even: i16, odd: i16) -> i16 {
    even - odd
}

#[inline]
fn backward_even(even: i16, odd: i16) -> i16 {
    (even << 1).wrapping_add(odd) >> 1
}

#[inline]
fn backward_odd(even: i16, odd: i16) -> i16 {
    (even << 1).wrapping_sub(odd) >> 1
}

/// Quantization shifts of one transform level. Higher frequencies get larger
/// shifts.
trait Quantization: Copy {
    /// At the last level, LL is a final coefficient. At other levels, LL is
    /// the input of the next level.
    const IS_LAST_LEVEL: bool;

    /// Shift of HH, at the odd row and odd column of a 2x2 block.
    fn odd_odd(self) -> u8;

    /// Shift of LH and HL, at the even row and odd column or the odd row and
    /// even column of a 2x2 block.
    #[inline(always)]
    fn even_odd(self) -> u8 {
        self.odd_odd().saturating_sub(1)
    }

    /// Shift of LL at the last level, at the even row and even column of a 2x2
    /// block.
    #[inline(always)]
    fn last_level_even_even(self) -> u8 {
        self.even_odd().saturating_sub(1)
    }
}

/// Shifts of all levels except the last as compile-time constants,
/// mainly done to reduce runtime register pressure.
#[derive(Clone, Copy)]
struct LevelQuantization<const ODD_ODD: u8>;

impl<const ODD_ODD: u8> Quantization for LevelQuantization<ODD_ODD> {
    const IS_LAST_LEVEL: bool = false;

    #[inline(always)]
    fn odd_odd(self) -> u8 {
        ODD_ODD
    }
}

/// Shifts of the last level at runtime. The last level contains few blocks, so
/// constant shifts would only make the program larger without much benefit.
#[derive(Clone, Copy)]
struct LastLevelQuantization {
    odd_odd: u8,
}

impl Quantization for LastLevelQuantization {
    const IS_LAST_LEVEL: bool = true;

    #[inline(always)]
    fn odd_odd(self) -> u8 {
        self.odd_odd
    }
}

// `with_level_quantization!` has one arm per shift up to
// `MAX_QUANTIZATION_LEVEL`.
const _: () = assert!(MAX_QUANTIZATION_LEVEL == 14);

/// Calls `$level_function` with the `LevelQuantization` of the runtime HH
/// shift `$odd_odd`.
macro_rules! with_level_quantization {
    ($odd_odd:expr, $level_function:ident($($argument:expr),*)) => {
        match $odd_odd {
            0 => $level_function($($argument,)* LevelQuantization::<0>),
            1 => $level_function($($argument,)* LevelQuantization::<1>),
            2 => $level_function($($argument,)* LevelQuantization::<2>),
            3 => $level_function($($argument,)* LevelQuantization::<3>),
            4 => $level_function($($argument,)* LevelQuantization::<4>),
            5 => $level_function($($argument,)* LevelQuantization::<5>),
            6 => $level_function($($argument,)* LevelQuantization::<6>),
            7 => $level_function($($argument,)* LevelQuantization::<7>),
            8 => $level_function($($argument,)* LevelQuantization::<8>),
            9 => $level_function($($argument,)* LevelQuantization::<9>),
            10 => $level_function($($argument,)* LevelQuantization::<10>),
            11 => $level_function($($argument,)* LevelQuantization::<11>),
            12 => $level_function($($argument,)* LevelQuantization::<12>),
            13 => $level_function($($argument,)* LevelQuantization::<13>),
            _ => $level_function($($argument,)* LevelQuantization::<14>),
        }
    };
}

/// Transforms and quantizes one 2x2 block `[a, b, c, d]`, with `a` and `b` in
/// the even row and `c` and `d` in the odd row. The result contains the LL,
/// LH, HL and HH coefficients in the same positions.
#[inline(always)]
fn forward_block<Q: Quantization>([a, b, c, d]: [i16; 4], quantization: Q) -> [i16; 4] {
    let (a, c) = (forward_even(a, c), forward_odd(a, c));
    let (b, d) = (forward_even(b, d), forward_odd(b, d));
    let ll = forward_even(a, b);
    [
        if Q::IS_LAST_LEVEL {
            quantize(ll, quantization.last_level_even_even())
        } else {
            ll
        },
        quantize(forward_odd(a, b), quantization.even_odd()),
        quantize(forward_even(c, d), quantization.even_odd()),
        quantize(forward_odd(c, d), quantization.odd_odd()),
    ]
}

#[inline(always)]
fn forward_output(sample: PackedSample, coefficient: i16, is_final: bool) -> PackedSample {
    if is_final {
        sample.delta_encoded(coefficient)
    } else {
        PackedSample::new(coefficient, sample.previous())
    }
}

/// Transforms the 2x2 blocks of an even row and the odd row `stride` rows
/// below it.
///
/// `STRIDE` is the stride as a compile-time constant, or 0 for the runtime
/// `stride`. If the last block lacks the right column, the transform uses a
/// copy of the left column. The coefficients of the right column are then 0
/// and are not stored.
#[inline(always)]
fn forward_row_pair<Q: Quantization, const STRIDE: usize>(
    even_row: &mut [PackedSample],
    odd_row: &mut [PackedSample],
    stride: usize,
    quantization: Q,
) {
    let stride = if STRIDE == 0 { stride } else { STRIDE };
    let mut even_chunks = even_row.chunks_exact_mut(2 * stride);
    let mut odd_chunks = odd_row.chunks_exact_mut(2 * stride);
    for (even, odd) in (&mut even_chunks).zip(&mut odd_chunks) {
        let [a, b, c, d] = [even[0], even[stride], odd[0], odd[stride]];
        let [ll, lh, hl, hh] = forward_block(
            [a.current(), b.current(), c.current(), d.current()],
            quantization,
        );
        even[0] = forward_output(a, ll, Q::IS_LAST_LEVEL);
        even[stride] = forward_output(b, lh, true);
        odd[0] = forward_output(c, hl, true);
        odd[stride] = forward_output(d, hh, true);
    }
    let (even, odd) = (even_chunks.into_remainder(), odd_chunks.into_remainder());
    if let (Some(&a), Some(&c)) = (even.first(), odd.first()) {
        let b = even.get(stride).copied().unwrap_or(a);
        let d = odd.get(stride).copied().unwrap_or(c);
        let [ll, lh, hl, hh] = forward_block(
            [a.current(), b.current(), c.current(), d.current()],
            quantization,
        );
        even[0] = forward_output(a, ll, Q::IS_LAST_LEVEL);
        odd[0] = forward_output(c, hl, true);
        if let (Some(even), Some(odd)) = (even.get_mut(stride), odd.get_mut(stride)) {
            *even = forward_output(b, lh, true);
            *odd = forward_output(d, hh, true);
        }
    }
}

/// Transforms the last row of a level with an odd number of rows. The
/// transform uses a copy of the even row as the odd row, so the HL and HH
/// coefficients are 0 and are not stored.
#[inline(always)]
fn forward_row<Q: Quantization>(row: &mut [PackedSample], stride: usize, quantization: Q) {
    for row in row.chunks_mut(2 * stride) {
        let a = row[0];
        let b = row.get(stride).copied().unwrap_or(a);
        let [ll, lh, ..] = forward_block(
            [a.current(), b.current(), a.current(), b.current()],
            quantization,
        );
        row[0] = forward_output(a, ll, Q::IS_LAST_LEVEL);
        if let Some(row) = row.get_mut(stride) {
            *row = forward_output(b, lh, true);
        }
    }
}

fn forward_level<Q: Quantization>(
    frame: &mut [PackedSample],
    width: usize,
    stride: usize,
    quantization: Q,
) {
    let odd_row_offset = stride * width;
    for rows in frame.chunks_mut(2 * odd_row_offset) {
        if rows.len() > odd_row_offset {
            let (even_row, odd_row) = rows.split_at_mut(odd_row_offset);
            let (even_row, odd_row) = (&mut even_row[..width], &mut odd_row[..width]);
            // Level 0 contains most blocks, so it uses a constant stride.
            if stride == 1 {
                forward_row_pair::<_, 1>(even_row, odd_row, stride, quantization);
            } else {
                forward_row_pair::<_, 0>(even_row, odd_row, stride, quantization);
            }
        } else {
            forward_row(&mut rows[..width], stride, quantization);
        }
    }
}

/// Performs in-place forward Haar transform, scalar quantization and delta
/// encoding.
///
/// `quant` is the [`quantize`] shift of the level 0 HH coefficients. The HH
/// shift of each next level is 1 smaller, down to 0.
/// The resulting coefficients are delta-encoded using the previous
/// coefficients of each sample, which are updated with the current ones.
pub fn forward(frame: &mut [Sample], width: NonZero<u16>, height: NonZero<u16>, quant: u8) {
    let frame = PackedSample::slice_mut(frame);
    let quant = quant.min(MAX_QUANTIZATION_LEVEL);
    let max_level = max_transform_level(width, height);
    let width = usize::from(width.get());
    for level in 0..max_level {
        with_level_quantization!(
            quant.saturating_sub(level),
            forward_level(frame, width, 1 << level)
        );
    }
    let quantization = LastLevelQuantization {
        odd_odd: quant.saturating_sub(max_level),
    };
    forward_level(frame, width, 1 << max_level, quantization);
}

#[inline(always)]
fn backward_pair(even: i16, odd: i16) -> (i16, i16) {
    (backward_even(even, odd), backward_odd(even, odd))
}

#[inline(always)]
fn backward_block<Q: Quantization>([ll, lh, hl, hh]: [i16; 4], quantization: Q) -> [i16; 4] {
    let ll = if Q::IS_LAST_LEVEL {
        dequantize(ll, quantization.last_level_even_even())
    } else {
        ll
    };
    let (a, b) = backward_pair(ll, dequantize(lh, quantization.even_odd()));
    let (c, d) = backward_pair(
        dequantize(hl, quantization.even_odd()),
        dequantize(hh, quantization.odd_odd()),
    );
    let (a, c) = backward_pair(a, c);
    let (b, d) = backward_pair(b, d);
    [a, b, c, d]
}

#[inline(always)]
fn backward_input(sample: PackedSample, is_final: bool) -> i16 {
    if is_final {
        sample.delta_decoded()
    } else {
        sample.current()
    }
}

#[inline(always)]
fn backward_output(
    sample: PackedSample,
    coefficient: i16,
    value: i16,
    is_final: bool,
) -> PackedSample {
    PackedSample::new(
        value,
        if is_final {
            coefficient
        } else {
            sample.previous()
        },
    )
}

/// Inverse of [`forward_row_pair`]. The coefficients that `forward_row_pair`
/// does not store are 0.
#[inline(always)]
fn backward_row_pair<Q: Quantization, const STRIDE: usize>(
    even_row: &mut [PackedSample],
    odd_row: &mut [PackedSample],
    stride: usize,
    quantization: Q,
) {
    let stride = if STRIDE == 0 { stride } else { STRIDE };
    let mut even_chunks = even_row.chunks_exact_mut(2 * stride);
    let mut odd_chunks = odd_row.chunks_exact_mut(2 * stride);
    for (even, odd) in (&mut even_chunks).zip(&mut odd_chunks) {
        let [a, b, c, d] = [even[0], even[stride], odd[0], odd[stride]];
        let coefficients = [
            backward_input(a, Q::IS_LAST_LEVEL),
            backward_input(b, true),
            backward_input(c, true),
            backward_input(d, true),
        ];
        let [ll, lh, hl, hh] = coefficients;
        let [a_value, b_value, c_value, d_value] = backward_block(coefficients, quantization);
        even[0] = backward_output(a, ll, a_value, Q::IS_LAST_LEVEL);
        even[stride] = backward_output(b, lh, b_value, true);
        odd[0] = backward_output(c, hl, c_value, true);
        odd[stride] = backward_output(d, hh, d_value, true);
    }
    let (even, odd) = (even_chunks.into_remainder(), odd_chunks.into_remainder());
    if let (Some(&a), Some(&c)) = (even.first(), odd.first()) {
        let ll = backward_input(a, Q::IS_LAST_LEVEL);
        let hl = backward_input(c, true);
        let (lh, hh) = match (even.get(stride), odd.get(stride)) {
            (Some(&b), Some(&d)) => (backward_input(b, true), backward_input(d, true)),
            _ => (0, 0),
        };
        let [a_value, b_value, c_value, d_value] = backward_block([ll, lh, hl, hh], quantization);
        even[0] = backward_output(a, ll, a_value, Q::IS_LAST_LEVEL);
        odd[0] = backward_output(c, hl, c_value, true);
        if let (Some(b), Some(d)) = (even.get_mut(stride), odd.get_mut(stride)) {
            *b = backward_output(*b, lh, b_value, true);
            *d = backward_output(*d, hh, d_value, true);
        }
    }
}

/// Inverse of [`forward_row`].
#[inline(always)]
fn backward_row<Q: Quantization>(row: &mut [PackedSample], stride: usize, quantization: Q) {
    for row in row.chunks_mut(2 * stride) {
        let a = row[0];
        let ll = backward_input(a, Q::IS_LAST_LEVEL);
        let lh = row.get(stride).map_or(0, |&b| backward_input(b, true));
        let [a_value, b_value, ..] = backward_block([ll, lh, 0, 0], quantization);
        row[0] = backward_output(a, ll, a_value, Q::IS_LAST_LEVEL);
        if let Some(b) = row.get_mut(stride) {
            *b = backward_output(*b, lh, b_value, true);
        }
    }
}

fn backward_level<Q: Quantization>(
    frame: &mut [PackedSample],
    width: usize,
    stride: usize,
    quantization: Q,
) {
    let odd_row_offset = stride * width;
    for rows in frame.chunks_mut(2 * odd_row_offset) {
        if rows.len() > odd_row_offset {
            let (even_row, odd_row) = rows.split_at_mut(odd_row_offset);
            let (even_row, odd_row) = (&mut even_row[..width], &mut odd_row[..width]);
            // Levels 0 and 1 contain most blocks, so they use constant strides.
            match stride {
                1 => backward_row_pair::<_, 1>(even_row, odd_row, stride, quantization),
                2 => backward_row_pair::<_, 2>(even_row, odd_row, stride, quantization),
                _ => backward_row_pair::<_, 0>(even_row, odd_row, stride, quantization),
            }
        } else {
            backward_row(&mut rows[..width], stride, quantization);
        }
    }
}

/// Performs in-place delta decoding, scalar dequantization and backward Haar
/// transform.
///
/// `quant` is the [`dequantize`] shift of the level 0 HH coefficients. The HH
/// shift of each next level is 1 smaller, down to 0.
/// The coefficients are delta-decoded using the previous coefficients of each
/// sample, which are updated with the decoded ones.
pub fn backward(frame: &mut [Sample], width: NonZero<u16>, height: NonZero<u16>, quant: u8) {
    let frame = PackedSample::slice_mut(frame);
    debug_assert!(quant <= MAX_QUANTIZATION_LEVEL);
    let max_level = max_transform_level(width, height);
    let width = usize::from(width.get());
    let quantization = LastLevelQuantization {
        odd_odd: quant.saturating_sub(max_level),
    };
    backward_level(frame, width, 1 << max_level, quantization);
    for level in (0..max_level).rev() {
        with_level_quantization!(
            quant.saturating_sub(level),
            backward_level(frame, width, 1 << level)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::{UV_MAX, UV_MIN, yuv::tests::currents};
    use alloc::{vec, vec::Vec};
    use core::ops::RangeInclusive;
    use proptest::{collection, prelude::*};

    fn frames(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
        frame_count: usize,
    ) -> impl Strategy<Value = (u16, u16, Vec<Vec<i16>>)> {
        (width, height).prop_flat_map(move |(width, height)| {
            let frame = collection::vec(UV_MIN..=UV_MAX, usize::from(width) * usize::from(height));
            (width..=width, height..=height, vec![frame; frame_count])
        })
    }

    fn samples(values: &[i16]) -> Vec<Sample> {
        values
            .iter()
            .map(|&current| Sample {
                current,
                previous: 0,
            })
            .collect()
    }

    fn assert_lossless_round_trip(width: u16, height: u16, frames: &[Vec<i16>]) {
        let width = NonZero::new(width).unwrap();
        let height = NonZero::new(height).unwrap();
        let len = usize::from(width.get()) * usize::from(height.get());
        let mut encoder_frame = vec![Sample::default(); len];
        let mut decoder_frame = vec![Sample::default(); len];
        for expected in frames {
            for (sample, value) in encoder_frame.iter_mut().zip(expected) {
                sample.current = *value;
            }
            forward(&mut encoder_frame, width, height, 0);
            for (decoder_sample, encoder_sample) in decoder_frame.iter_mut().zip(&encoder_frame) {
                decoder_sample.current = encoder_sample.current;
            }
            backward(&mut decoder_frame, width, height, 0);
            assert_eq!(expected, &currents(&decoder_frame));
        }
    }

    #[test]
    fn forward_works() {
        proptest!(|((width, height, frames) in frames(1..=300, 1..=40, 3))| {
            assert_lossless_round_trip(width, height, &frames);
        });
    }

    /// The last block of a row (width not a multiple of `2 * stride`) must
    /// decode the same values as a full block with the same coefficients.
    fn assert_backward_remainder(values: &[i16], quantization: impl Quantization) {
        let stride = 2;
        let (even_values, odd_values) = values.split_at(8);
        let (mut full_even, mut full_odd) = (samples(even_values), samples(odd_values));
        backward_row_pair::<_, 0>(
            PackedSample::slice_mut(&mut full_even),
            PackedSample::slice_mut(&mut full_odd),
            stride,
            quantization,
        );
        let (mut remainder_even, mut remainder_odd) =
            (samples(&even_values[..7]), samples(&odd_values[..7]));
        backward_row_pair::<_, 0>(
            PackedSample::slice_mut(&mut remainder_even),
            PackedSample::slice_mut(&mut remainder_odd),
            stride,
            quantization,
        );
        assert_eq!(&full_even[..7], &remainder_even[..]);
        assert_eq!(&full_odd[..7], &remainder_odd[..]);
        let mut full_row = samples(even_values);
        backward_row(PackedSample::slice_mut(&mut full_row), stride, quantization);
        let mut remainder_row = samples(&even_values[..7]);
        backward_row(
            PackedSample::slice_mut(&mut remainder_row),
            stride,
            quantization,
        );
        assert_eq!(&full_row[..7], &remainder_row[..]);
    }

    #[test]
    fn backward_remainder_pair() {
        proptest!(|(values in collection::vec(UV_MIN..=UV_MAX, 16), odd_odd in 0_u8..=4)| {
            assert_backward_remainder(&values, LastLevelQuantization { odd_odd });
            with_level_quantization!(odd_odd, assert_backward_remainder(&values));
        });
    }

    #[test]
    fn forward_works_for_max_transform_level() {
        for (width, height) in [
            (1, 1100),
            (1100, 1),
            (1029, 1030),
            (2047, 1024),
            (1024, 1500),
        ] {
            let frames = [0_u64, 1].map(|seed| {
                (0..u64::from(width) * u64::from(height))
                    .map(|i| ((i * 7919 + seed * 104729) % 511) as i16 - 255)
                    .collect::<Vec<_>>()
            });
            assert_lossless_round_trip(width, height, &frames);
        }
    }
}
