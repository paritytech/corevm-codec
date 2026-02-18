use core::num::NonZero;

/// How many nested transforms to make.
pub const MAX_TRANSFORM_LEVEL: u8 = 10;

/// How many least significant bits can be removed.
pub const MAX_QUANTIZATION_LEVEL: u8 = 14;

/// Quantize signed integer using the provided quantization level `q`.
///
/// The integer is divided by _2^q_.
pub fn quantize(value: i16, q: u8) -> i16 {
    //let threshold = 2_i16.pow(u32::from(q)) * 7 / 5;
    //if value.abs() <= threshold {
    //    return 0;
    //}
    let sign_bit = value >> 15;
    (value + (sign_bit & ((1_i16 << q) - 1))) >> q
}

/// Dequantize signed integer uainsg the provided quantization level `q`.
///
/// The integer is multiplied by _2^q_.
///
/// This function might return either 0 or negative number on overflow for
/// efficiency reasons. This can only occur if incorrect input value is used.
pub fn dequantize(value: i16, q: u8) -> i16 {
    value.unbounded_shl(u32::from(q))
}

/// Returns quantization levels _LL_, _LH_/_HL_, _HH_ that correspond to the
/// provided transform level.
///
/// _L_ means low frequency, _H_ means high frequency; the first letter refers
/// to horizontal transform, the second letter refers to vertical transform.
/// _LL_ is non-zero only for the maximum transform level.
pub fn quantization_levels(
    power: u8,
    transform_level: u8,
    max_transform_level: u8,
) -> (u8, u8, u8) {
    let q_odd_odd = power.saturating_sub(transform_level);
    let q_even_odd = q_odd_odd.saturating_sub(1);
    let q_even_even = if transform_level == max_transform_level {
        q_even_odd.saturating_sub(1)
    } else {
        0
    };
    (q_even_even, q_even_odd, q_odd_odd)
}

/// Returns maximum possible transform level for the given image/tile width and
/// height.
pub fn max_transform_level(width: NonZero<u16>, height: NonZero<u16>) -> u8 {
    width
        .min(height)
        .ilog2()
        .min(u32::from(MAX_TRANSFORM_LEVEL))
        .saturating_sub(1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_works() {
        fn quantize_naive(value: i16, q: u8) -> i16 {
            value / 2_i16.pow(u32::from(q))
        }
        for x in i16::MIN..=i16::MAX {
            for q in 0..=MAX_QUANTIZATION_LEVEL {
                assert_eq!(quantize_naive(x, q), quantize(x, q), "x = {x}, q = {q}");
            }
        }
    }

    #[test]
    fn dequantize_works() {
        fn dequantize_naive(value: i16, q: u8) -> i16 {
            value.saturating_mul(2_i16.pow(u32::from(q)))
        }
        for q in 0..=MAX_QUANTIZATION_LEVEL {
            let x_min = i32::from(i16::MIN).max(-2_i32.pow(15 - u32::from(q))) as i16;
            let x_max = i32::from(i16::MAX).min(2_i32.pow(14 - u32::from(q))) as i16;
            for x in x_min..=x_max {
                assert_eq!(dequantize_naive(x, q), dequantize(x, q), "x = {x}, q = {q}");
            }
        }
    }
}
