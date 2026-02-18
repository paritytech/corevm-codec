use crate::{
    errors, rans,
    rans::{Input, Output, Stats},
};

errors! {
    (InvalidSignMagStream "Invalid sign-magnitude stream")
}

#[doc(hidden)]
impl From<rans::InvalidRansStream> for InvalidSignMagStream {
    fn from(_: rans::InvalidRansStream) -> InvalidSignMagStream {
        InvalidSignMagStream
    }
}

const MAG_SIZE_ONE_BYTE: u8 = 0;
const MAG_SIZE_TWO_BYTES: u8 = 1;
const MAG_SIZE_ZERO: u8 = 2;

/// Convert signed integer to sign-magnitude representation.
///
/// This potentially decreases the number of symbols required to encode the
/// resulting byte stream with rANS, since positive and negative numbers with
/// equal absolute values are now encoded using the same symbol.
#[inline]
fn to_sign_magnitude(x: i16) -> (u8, u16) {
    let sign = if x < 0 { 1 } else { 0 };
    (sign, x.unsigned_abs())
}

/// Convert sign-magnitude representation back into signed integer.
#[inline]
fn from_sign_magnitude(sign: u8, magnitude: u16) -> Result<i16, InvalidSignMagStream> {
    let t = magnitude as i16;
    match sign {
        1 if t != i16::MIN => Ok(-t),
        0 | 1 => Ok(t),
        _ => Err(InvalidSignMagStream),
    }
}

/// Estimate symbol frequencies of a slice of signed integers using their
/// sign-magnitude representation.
fn estimate_frequencies_with_sign_magnitude(input: &[i16]) -> ([u32; rans::NUM_FREQS], u8) {
    assert!(input.len() <= rans::MAX_INPUT_LEN / 3);
    let mut freqs = [0_u32; rans::NUM_FREQS];
    let mut magnitude_size = MAG_SIZE_ONE_BYTE;
    for (i, elem) in input.iter().copied().enumerate() {
        let (sign, magnitude) = to_sign_magnitude(elem);
        freqs[usize::from(sign)] += 1;
        let [b0, b1] = magnitude.to_le_bytes();
        freqs[usize::from(b0)] += 1;
        if magnitude_size == MAG_SIZE_TWO_BYTES {
            freqs[usize::from(b1)] += 1;
        } else if b1 != 0 {
            magnitude_size = MAG_SIZE_TWO_BYTES;
            freqs[0] += i as u32;
            freqs[usize::from(b1)] += 1;
        }
    }
    if input.is_empty() {
        magnitude_size = MAG_SIZE_ZERO;
    }
    freqs[usize::from(magnitude_size)] += 1;
    (freqs, magnitude_size)
}

/// Encode a slice of signed integers using their sign-magnitude representation
/// and rANS encoder.
pub fn encode(input: &[i16], output: &mut impl Output) -> Stats {
    let (freqs, magnitude_size) = estimate_frequencies_with_sign_magnitude(input);
    let mut encoder = rans::Encoder::from_frequencies(freqs);
    for elem in input.iter().copied().rev() {
        let (sign, magnitude) = to_sign_magnitude(elem);
        encoder.push(sign, output);
        if magnitude_size == MAG_SIZE_ONE_BYTE {
            debug_assert!(magnitude <= u16::from(u8::MAX), "magnitude = {magnitude}");
            encoder.push(magnitude as u8, output);
        } else {
            let [b0, b1] = magnitude.to_le_bytes();
            encoder.push(b1, output);
            encoder.push(b0, output);
        }
    }
    encoder.push(magnitude_size, output);
    encoder.finish(output)
}

/// Decode a slice of integers from their rANS-encoded sign-magnitude
/// representation.
pub fn decode(input: &mut impl Input, output: &mut [i16]) -> Result<(), InvalidSignMagStream> {
    let mut decoder = rans::Decoder::new(input)?;
    let magnitude_size = match decoder.pop(input)? {
        MAG_SIZE_ONE_BYTE => MAG_SIZE_ONE_BYTE,
        MAG_SIZE_TWO_BYTES => MAG_SIZE_TWO_BYTES,
        MAG_SIZE_ZERO if output.is_empty() => return Ok(()),
        _ => return Err(InvalidSignMagStream),
    };
    for elem in output.iter_mut() {
        let b0 = decoder.pop(input)?;
        let magnitude = if magnitude_size == MAG_SIZE_ONE_BYTE {
            u16::from(b0)
        } else {
            let b1 = decoder.pop(input)?;
            u16::from_le_bytes([b0, b1])
        };
        let sign = decoder.pop(input)?;
        *elem = from_sign_magnitude(sign, magnitude)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use proptest::prelude::*;

    #[test]
    fn sign_magnitude_works() {
        for x in i16::MIN..=i16::MAX {
            let (sign, magnitude) = to_sign_magnitude(x);
            assert_eq!(x, from_sign_magnitude(sign, magnitude).unwrap());
        }
    }

    #[test]
    fn encode_simple_works() {
        let input = [i16::MIN, 2, -1, 0, 1, 2, i16::MAX];
        let mut output = Vec::new();
        encode(&input, &mut output);
        let mut actual = vec![33; input.len()];
        decode(&mut output, &mut actual[..]).unwrap();
        assert_eq!(input.as_slice(), actual.as_slice());
    }

    #[test]
    fn encode_works() {
        proptest!(|(input: Vec<i16>)| {
            let mut output = Vec::new();
            encode(&input, &mut output);
            let mut actual = vec![33; input.len()];
            decode(&mut output, &mut actual[..]).unwrap();
            assert_eq!(input.as_slice(), actual.as_slice());
        });
    }
}
