/// One sample of a plane and the transform coefficient of the previous frame
/// at the same position.
///
/// The values of the fields depend on the stage.
///
/// | Stage                               | `current`         | `previous`                        |
/// |-------------------------------------|-------------------|-----------------------------------|
/// | Encoder, after the color conversion | YUV value         | coefficient of the previous frame |
/// | Encoder, after `haar::forward`      | coefficient delta | coefficient                       |
/// | Decoder, after `entropy::decode`    | coefficient delta | coefficient of the previous frame |
/// | Decoder, after `haar::backward`     | YUV value         | coefficient                       |
///
/// The fields are interleaved, so that the Haar transform loads and stores
/// both values with one 32-bit memory access (see [`PackedSample`]).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C, align(4))]
pub struct Sample {
    pub current: i16,
    pub previous: i16,
}

// `PackedSample` reads the memory of `Sample` as a `u32`, so the bit offsets of
// the fields depend on the byte order.
const CURRENT_SHIFT: u32 = if cfg!(target_endian = "little") {
    0
} else {
    16
};
const PREVIOUS_SHIFT: u32 = 16 - CURRENT_SHIFT;

/// [`Sample`] as one `u32`, for one memory access per sample instead of two.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PackedSample(u32);

const _: () = {
    assert!(size_of::<Sample>() == size_of::<PackedSample>());
    assert!(align_of::<Sample>() == align_of::<PackedSample>());
};

impl PackedSample {
    pub fn slice(samples: &[Sample]) -> &[PackedSample] {
        // SAFETY: `Sample` and `PackedSample` have the same size and alignment,
        // and all bit patterns are valid for both types.
        unsafe { core::slice::from_raw_parts(samples.as_ptr().cast(), samples.len()) }
    }

    pub fn slice_mut(samples: &mut [Sample]) -> &mut [PackedSample] {
        // SAFETY: `Sample` and `PackedSample` have the same size and alignment,
        // and all bit patterns are valid for both types.
        unsafe { core::slice::from_raw_parts_mut(samples.as_mut_ptr().cast(), samples.len()) }
    }

    #[inline(always)]
    pub fn new(current: i16, previous: i16) -> Self {
        Self(
            (u32::from(current as u16) << CURRENT_SHIFT)
                | (u32::from(previous as u16) << PREVIOUS_SHIFT),
        )
    }

    #[inline(always)]
    pub fn current(self) -> i16 {
        (self.0 >> CURRENT_SHIFT) as i16
    }

    #[inline(always)]
    pub fn previous(self) -> i16 {
        (self.0 >> PREVIOUS_SHIFT) as i16
    }

    #[inline(always)]
    pub fn delta_encoded(self, coefficient: i16) -> Self {
        Self::new(coefficient.wrapping_sub(self.previous()), coefficient)
    }

    #[inline(always)]
    pub fn delta_decoded(self) -> i16 {
        self.current().wrapping_add(self.previous())
    }
}

/// Two adjacent samples as one `u64`, for one load per two samples.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PackedSamplePair(u64);

impl PackedSamplePair {
    pub fn split(samples: &[PackedSample]) -> (&[PackedSample], &[Self], &[PackedSample]) {
        // SAFETY: all bit patterns are valid for `PackedSample` and
        // `PackedSamplePair`.
        unsafe { samples.align_to() }
    }

    /// Returns the samples in memory order.
    #[inline(always)]
    pub fn samples(self) -> [PackedSample; 2] {
        let low = PackedSample(self.0 as u32);
        let high = PackedSample((self.0 >> u32::BITS) as u32);
        if cfg!(target_endian = "little") {
            [low, high]
        } else {
            [high, low]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use proptest::prelude::*;

    #[test]
    fn packed_sample_layout() {
        proptest!(|(current: i16, previous: i16)| {
            let mut samples = [Sample { current, previous }];
            let packed = PackedSample::slice_mut(&mut samples)[0];
            assert_eq!((current, previous), (packed.current(), packed.previous()));
            PackedSample::slice_mut(&mut samples)[0] = PackedSample::new(previous, current);
            assert_eq!(Sample { current: previous, previous: current }, samples[0]);
        });
    }

    #[test]
    fn packed_sample_pair_order() {
        proptest!(|(currents: Vec<i16>, offset in 0_usize..2)| {
            let samples: Vec<Sample> = currents
                .iter()
                .map(|&current| Sample { current, previous: current.wrapping_add(1) })
                .collect();
            let packed = &PackedSample::slice(&samples)[offset.min(samples.len())..];
            let (prefix, pairs, suffix) = PackedSamplePair::split(packed);
            let unpacked: Vec<(i16, i16)> = prefix
                .iter()
                .copied()
                .chain(pairs.iter().flat_map(|pair| pair.samples()))
                .chain(suffix.iter().copied())
                .map(|sample| (sample.current(), sample.previous()))
                .collect();
            let expected: Vec<(i16, i16)> = packed
                .iter()
                .map(|sample| (sample.current(), sample.previous()))
                .collect();
            assert_eq!(expected, unpacked);
        });
    }
}
