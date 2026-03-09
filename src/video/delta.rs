/// Delta-encode `x` using previous values from `x_prev` while simultaneously
/// updating `x_prev` with the current `x` values.
pub fn encode(x: &mut [i16], x_prev: &mut [i16]) {
    for (cur, prev) in x.iter_mut().zip(x_prev.iter_mut()) {
        let delta = cur.wrapping_sub(*prev);
        *prev = *cur;
        *cur = delta;
    }
}

/// Delta-decode `x` using previous values from `x_prev` while simultaneously
/// updating `x_prev` with the resulting `x` values.
pub fn decode(x: &mut [i16], x_prev: &mut [i16]) {
    for (cur, prev) in x.iter_mut().zip(x_prev.iter_mut()) {
        let value = cur.wrapping_add(*prev);
        *cur = value;
        *prev = value;
    }
}
