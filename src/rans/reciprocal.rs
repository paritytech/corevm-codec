//! This module implements integer division using reciprocals.

#[derive(Debug)]
pub struct Reciprocals<const N: usize> {
    values: [u32; N],
    shifts: [u8; N],
}

impl<const N: usize> Reciprocals<N> {
    pub const fn new() -> Self {
        Self {
            values: [0; N],
            shifts: [0; N],
        }
    }

    pub fn from_frequencies(freqs: &[u16; N]) -> Self {
        let mut rs = Self::new();
        for (i, f) in freqs.iter().copied().enumerate() {
            if f == 0 {
                continue;
            }
            let Reciprocal { value, shift } = Reciprocal::new(f);
            rs.values[i] = value;
            rs.shifts[i] = shift;
        }
        rs
    }

    pub fn get(&self, i: usize) -> Reciprocal {
        Reciprocal {
            value: self.values[i],
            shift: self.shifts[i],
        }
    }
}

const SHIFT: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reciprocal {
    pub value: u32,
    pub shift: u8,
}

impl Reciprocal {
    /// Compute reciprocal for the specified divisor (denominator).
    ///
    /// This reciprocal can be used to divide any 32-bit number by the divisor
    /// without using "division" assembly instruction.
    pub fn new(denominator: u16) -> Self {
        debug_assert_ne!(0, denominator);
        if denominator == 1 {
            // No shifts here.
            return Reciprocal { value: 0, shift: 0 };
        }
        let shift = u32::from(denominator).next_power_of_two().ilog2();
        let reciprocal = (1_u64 << (SHIFT + shift)).div_ceil(u64::from(denominator));
        // We split shifting into two stages.
        // First we shift by either 0 or 1 bits (0 is only for division by 1).
        // This shift is stored as the 6th bit of `shift` field.
        let secondary_shift = 1_u8 << 6;
        // Then we shift by 0-15 bits. This shift is stored in bits 0-5 of `shift`
        // field.
        let primary_shift = shift as u8 - 1;
        Self {
            // Truncate to u32.
            value: reciprocal as u32,
            shift: primary_shift | secondary_shift,
        }
    }
}

pub trait ReciprocalDiv {
    fn reciprocal_div(self, denominator_reciprocal: Reciprocal) -> Self;
}

impl ReciprocalDiv for u32 {
    /// Integer division using a reciprocal, i.e. using a multiplication and a
    /// bit shift.
    ///
    /// See R. Alverson "Integer division using reciprocals" https://doi.org/10.1109/ARITH.1991.145558.
    fn reciprocal_div(self, denominator_reciprocal: Reciprocal) -> Self {
        // Schematically `x/y == (x * r) >> (32 + p)`, but the actual implementation is
        // more involved to account for division by 1 and for the fact that `r`
        // is a 33-bit number.
        let numerator = self;
        let r = denominator_reciprocal;
        let q = ((u64::from(numerator) * u64::from(r.value)) >> SHIFT) as u32;
        let secondary_shift = (r.shift >> 6) & 1;
        let t = ((numerator - q) >> secondary_shift) + q;
        let primary_shift = r.shift & ((1_u8 << 6) - 1);
        t >> primary_shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reciprocal_div_simple_works() {
        for a in [0, 1, u32::MAX] {
            for b in 1..=u16::MAX {
                let r = Reciprocal::new(b);
                let q = a.reciprocal_div(r);
                assert_eq!(a / u32::from(b), q, "a = {a}, b = {b}, r = {r:?}");
            }
        }
    }

    #[test]
    fn reciprocal_div_works() {
        proptest!(|(a: u32)| {
            for b in 1..=u16::MAX {
                let r = Reciprocal::new(b);
                let q = a.reciprocal_div(r);
                assert_eq!(a / u32::from(b), q, "a = {a}, b = {b}, r = {r:?}");
            }
        });
    }
}
