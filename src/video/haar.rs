use super::{
    dequantize, max_transform_level, quantization_levels, quantize, MAX_QUANTIZATION_LEVEL,
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
fn forward_row(x: &mut [i16], stride: NonZero<u16>, q_even: u8, q_odd: u8) {
    let stride = usize::from(stride.get());
    let mut chunks = x.chunks_exact_mut(2 * stride);
    for chunk in &mut chunks {
        let even = chunk[0];
        let odd = chunk[stride];
        chunk[0] = quantize(forward_even(even, odd), q_even);
        chunk[stride] = quantize(forward_odd(even, odd), q_odd);
    }
    let last_chunk = chunks.into_remainder();
    if !last_chunk.is_empty() {
        let even = last_chunk[0];
        // Odd element doesn't exist when stride is 1.
        let odd = last_chunk.get(stride).copied().unwrap_or(even);
        last_chunk[0] = quantize(forward_even(even, odd), q_even);
        if let Some(odd_out) = last_chunk.get_mut(stride) {
            *odd_out = quantize(forward_odd(even, odd), q_odd);
        }
    }
}

#[inline]
fn backward_even(even: i16, odd: i16) -> i16 {
    ((even << 1) + odd) >> 1
}

#[inline]
fn backward_odd(even: i16, odd: i16) -> i16 {
    ((even << 1) - odd) >> 1
}

#[inline]
fn backward_row(x: &mut [i16], stride: NonZero<u16>, q_even: u8, q_odd: u8) {
    let stride = usize::from(stride.get());
    let mut chunks = x.chunks_exact_mut(2 * stride);
    for chunk in &mut chunks {
        let even = dequantize(chunk[0], q_even);
        let odd = dequantize(chunk[stride], q_odd);
        chunk[0] = backward_even(even, odd);
        chunk[stride] = backward_odd(even, odd);
    }
    let last_chunk = chunks.into_remainder();
    if !last_chunk.is_empty() {
        let even = dequantize(last_chunk[0], q_even);
        // Default value is 0 because forward_odd(even, even) == 0.
        let odd = last_chunk
            .get(stride)
            .copied()
            .map(|odd| dequantize(odd, q_odd))
            .unwrap_or(0);
        last_chunk[0] = backward_even(even, odd);
        if let Some(odd_out) = last_chunk.get_mut(stride) {
            *odd_out = dequantize(backward_odd(even, odd), q_odd);
        }
    }
}

#[inline]
fn forward_col(even_row: &mut [i16], odd_row: &mut [i16], stride: NonZero<u16>) {
    let stride = usize::from(stride.get());
    let even_row_iter = even_row.iter_mut().step_by(stride);
    let odd_row_iter = odd_row.iter_mut().step_by(stride);
    for (even, odd) in even_row_iter.zip(odd_row_iter) {
        let e = *even;
        let o = *odd;
        *even = forward_even(e, o);
        *odd = forward_odd(e, o);
    }
}

#[inline]
fn backward_col(even_row: &mut [i16], odd_row: &mut [i16], stride: NonZero<u16>) {
    let stride = usize::from(stride.get());
    let even_row_iter = even_row.iter_mut().step_by(stride);
    let odd_row_iter = odd_row.iter_mut().step_by(stride);
    for (even, odd) in even_row_iter.zip(odd_row_iter) {
        let e = *even;
        let o = *odd;
        *even = backward_even(e, o);
        *odd = backward_odd(e, o);
    }
}

/// Performs in-place forward Haar transform and scalar quantization.
///
/// `quant` is the quantization level that is passed to [`quantize`].
pub fn forward(frame: &mut [i16], width: NonZero<u16>, height: NonZero<u16>, mut quant: u8) {
    if quant > MAX_QUANTIZATION_LEVEL {
        quant = MAX_QUANTIZATION_LEVEL;
    }
    let max_level = max_transform_level(width, height);
    let width = usize::from(width.get());
    for level in 0..=max_level {
        let stride = NonZero::new(1_u16 << level).expect("Level is capped");
        let (q0, q1, q2) = quantization_levels(quant, level, max_level);
        let v_stride = usize::from(stride.get()) * width;
        let v_stride_minus_1 = usize::from(stride.get() - 1) * width;
        let mut chunks = frame.chunks_exact_mut(2 * v_stride);
        for chunk in &mut chunks {
            let (even_row, rest) = chunk.split_at_mut(width);
            let odd_row = &mut rest[v_stride_minus_1..v_stride];
            forward_col(even_row, odd_row, stride);
            forward_row(even_row, stride, q0, q1);
            forward_row(odd_row, stride, q1, q2);
        }
        let last_chunk = chunks.into_remainder();
        if !last_chunk.is_empty() {
            let (even_row, rest) = last_chunk.split_at_mut(width);
            let mut odd_row = rest.get_mut(v_stride_minus_1..v_stride);
            if let Some(ref mut odd_row) = odd_row {
                forward_col(even_row, odd_row, stride);
            }
            forward_row(even_row, stride, q0, q1);
            if let Some(odd_row) = odd_row {
                forward_row(odd_row, stride, q1, q2);
            }
        }
    }
}

/// Performs in-place scalar dequantization and backward Haar transform.
///
/// `quant` is the quantization level that is passed to [`dequantize`].
pub fn backward(frame: &mut [i16], width: NonZero<u16>, height: NonZero<u16>, quant: u8) {
    debug_assert!(quant <= MAX_QUANTIZATION_LEVEL);
    let max_level = max_transform_level(width, height);
    let width = usize::from(width.get());
    for level in (0..=max_level).rev() {
        let stride = NonZero::new(1_u16 << level).expect("Level is capped");
        let (q0, q1, q2) = quantization_levels(quant, level, max_level);
        let v_stride = usize::from(stride.get()) * width;
        let v_stride_minus_1 = usize::from(stride.get() - 1) * width;
        let mut chunks = frame.chunks_exact_mut(2 * v_stride);
        for chunk in &mut chunks {
            let (even_row, rest) = chunk.split_at_mut(width);
            let odd_row = &mut rest[v_stride_minus_1..v_stride];
            backward_row(even_row, stride, q0, q1);
            backward_row(odd_row, stride, q1, q2);
            backward_col(even_row, odd_row, stride);
        }
        let last_chunk = chunks.into_remainder();
        if !last_chunk.is_empty() {
            let (even_row, rest) = last_chunk.split_at_mut(width);
            let mut odd_row = rest.get_mut(v_stride_minus_1..v_stride);
            backward_row(even_row, stride, q0, q1);
            if let Some(ref mut odd_row) = odd_row {
                backward_row(odd_row, stride, q1, q2);
            }
            if let Some(ref mut odd_row) = odd_row {
                backward_col(even_row, odd_row, stride);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::{MAX_TRANSFORM_LEVEL, UV_MAX, UV_MIN};
    use alloc::vec::Vec;
    use core::ops::RangeInclusive;
    use proptest::{collection, prelude::*};

    fn frame_row(width: RangeInclusive<u16>) -> impl Strategy<Value = (u16, Vec<i16>)> {
        width.prop_flat_map(|width| {
            (
                width..=width,
                collection::vec(UV_MIN..=UV_MAX, usize::from(width)),
            )
        })
    }

    #[test]
    fn forward_row_works() {
        proptest!(|((_width, mut x) in frame_row(1..=u16::MAX), level in 0..=MAX_TRANSFORM_LEVEL)| {
            let stride = 1_u16 << level;
            let expected = x.clone();
            forward_row(&mut x[..], NonZero::new(stride).unwrap(), 0, 0);
            backward_row(&mut x[..], NonZero::new(stride).unwrap(), 0, 0);
            assert_eq!(expected, x);
        });
    }

    fn frame(
        width: RangeInclusive<u16>,
        height: RangeInclusive<u16>,
    ) -> impl Strategy<Value = (u16, u16, Vec<i16>)> {
        (width, height).prop_flat_map(|(width, height)| {
            (
                width..=width,
                height..=height,
                collection::vec(UV_MIN..=UV_MAX, usize::from(width) * usize::from(height)),
            )
        })
    }

    #[test]
    fn forward_works() {
        proptest!(|((width, height, mut x) in frame(1..=10, 1..=10))| {
            let width = NonZero::new(width).unwrap();
            let height = NonZero::new(height).unwrap();
            let expected = x.clone();
            forward(&mut x[..], width, height, 0);
            backward(&mut x[..], width, height, 0);
            assert_eq!(expected, x);
        });
    }
}
