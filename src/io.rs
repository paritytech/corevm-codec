// Just use JAM codec i/o traits.
pub use jam_codec::{Input, Output};

pub(crate) struct SliceOutput<'a>(pub(crate) &'a mut [u8]);

impl jam_codec::Output for SliceOutput<'_> {
    fn write(&mut self, buf: &[u8]) {
        let (slice, rest) = core::mem::take(&mut self.0).split_at_mut(buf.len());
        slice.copy_from_slice(buf);
        self.0 = rest;
    }

    fn push_byte(&mut self, byte: u8) {
        let (slice, rest) = core::mem::take(&mut self.0).split_at_mut(1);
        slice[0] = byte;
        self.0 = rest;
    }
}
