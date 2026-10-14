use alloc::vec::Vec;

// Just use JAM codec i/o traits.
pub use jam_codec::{Input, Output};

/// A wrapper that implements [`Input`] for `Vec<u8>`.
pub struct VecInput {
    buf: Vec<u8>,
    offset: usize,
}

impl VecInput {
    /// Creates new input from the provided vector.
    pub fn new(buf: Vec<u8>) -> Self {
        Self { buf, offset: 0 }
    }

    /// Returns `true` if there are no remaining bytes left.
    pub fn is_empty(&self) -> bool {
        self.buf.len() == self.offset
    }

    /// Returns underlying vector.
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
}

impl jam_codec::Input for VecInput {
    fn remaining_len(&mut self) -> Result<Option<usize>, jam_codec::Error> {
        Ok(Some(self.buf.len() - self.offset))
    }

    fn read_byte(&mut self) -> Result<u8, jam_codec::Error> {
        let byte = self
            .buf
            .get(self.offset)
            .copied()
            .ok_or("Buffer overflow")?;
        self.offset += 1;
        Ok(byte)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), jam_codec::Error> {
        let len = buf.len();
        let slice = &self
            .buf
            .get(self.offset..self.offset + len)
            .ok_or("Buffer overflow")?;
        buf.copy_from_slice(slice);
        self.offset += len;
        Ok(())
    }
}
