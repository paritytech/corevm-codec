use crate::errors;
use alloc::vec::Vec;

errors! {
    (InputTooSmall "Input too small")
}

/// Writes bytes at the end of the stream.
pub trait Output {
    /// Append byte.
    fn push(&mut self, byte: u8);

    /// Append _reversed_ byte slice.
    fn push_slice_rev(&mut self, slice: &[u8]);
}

/// Reads bytes from the _end_ of the stream (in reverse order!).
pub trait Input {
    /// Read byte from the _end_ of the stream.
    fn pop(&mut self) -> Result<u8, InputTooSmall>;
}

impl Output for Vec<u8> {
    fn push(&mut self, byte: u8) {
        self.push(byte);
    }

    fn push_slice_rev(&mut self, slice: &[u8]) {
        self.extend_from_slice(slice);
    }
}

impl Input for Vec<u8> {
    fn pop(&mut self) -> Result<u8, InputTooSmall> {
        self.pop().ok_or(InputTooSmall)
    }
}

/// Convert ANS input to JAM input.
///
/// The bytes are read from the end of the stream in reverse.
pub(crate) struct AnsInputAsJamInput<'a, I: Input>(pub(crate) &'a mut I);

impl<I: Input> jam_codec::Input for AnsInputAsJamInput<'_, I> {
    fn remaining_len(&mut self) -> Result<Option<usize>, jam_codec::Error> {
        Ok(None)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), jam_codec::Error> {
        for byte in buf.iter_mut() {
            *byte = self.0.pop().map_err(|_| "Input too small")?;
        }
        Ok(())
    }
}

/// Convert JAM input to ANS input.
///
/// The bytes are read in normal order, which is reversed order from ANS
/// decoder's perspective. You would need to reverse the input stream first to
/// be able to decode it.
pub(crate) struct JamInputAsAnsInput<'a, I: jam_codec::Input>(pub(crate) &'a mut I);

impl<I: jam_codec::Input> Input for JamInputAsAnsInput<'_, I> {
    fn pop(&mut self) -> Result<u8, InputTooSmall> {
        self.0.read_byte().map_err(|_| InputTooSmall)
    }
}
