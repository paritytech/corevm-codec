//! Word storage of the rANS encoder and decoder.
//!
//! The hot loops access the words through fixed-size windows of `WINDOW_LEN`
//! words, so the bounds check runs once per window instead of once per symbol.

use super::InvalidRansStream;
use alloc::vec::Vec;

pub const WINDOW_LEN: usize = 128;

/// Renormalization unit of the rANS state. Streams store words in
/// little-endian byte order.
pub type Word = u16;

/// Reusable word storage.
#[derive(Default)]
struct Words {
    /// Words in use, followed by free words. The buffer never shrinks, so later
    /// frames reuse its memory.
    buffer: Vec<Word>,
    /// Number of words in use.
    len: usize,
}

impl Words {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            len: 0,
        }
    }

    fn as_slice(&self) -> &[Word] {
        &self.buffer[..self.len]
    }

    fn grow(&mut self, min_len: usize) {
        if self.buffer.len() < min_len {
            self.buffer.resize(min_len, 0);
        }
    }
}

/// Encoder output.
#[derive(Default)]
pub struct WordStack(Words);

impl WordStack {
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Words::with_capacity(capacity))
    }

    pub fn len(&self) -> usize {
        self.0.len
    }

    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }

    pub fn clear(&mut self) {
        self.0.len = 0;
    }

    pub fn as_slice(&self) -> &[Word] {
        self.0.as_slice()
    }

    pub fn as_bytes(&self) -> &[u8] {
        words_as_bytes(self.as_slice())
    }

    pub(super) fn write_window<R>(&mut self, write: impl FnOnce(&mut WordWindow<'_>) -> R) -> R {
        let words = &mut self.0;
        words.grow(words.len + WINDOW_LEN);
        let mut window = WordWindow {
            words: (&mut words.buffer[words.len..][..WINDOW_LEN])
                .try_into()
                .expect("invalid window length"),
            len: 0,
        };
        let result = write(&mut window);
        debug_assert!(window.len <= WINDOW_LEN);
        words.len += window.len;
        result
    }
}

pub(super) struct WordWindow<'a> {
    words: &'a mut [Word; WINDOW_LEN],
    len: usize,
}

impl WordWindow<'_> {
    /// Pushes `word` if `condition` is `true`. The word is always stored to
    /// avoid a branch; if it is not pushed, the next store overwrites it. The
    /// `% WINDOW_LEN` never changes the index, but lets the compiler skip the
    /// bounds check.
    #[inline(always)]
    pub(super) fn push_if(&mut self, word: Word, condition: bool) {
        self.words[self.len % WINDOW_LEN] = word.to_le();
        self.len += usize::from(condition);
    }
}

/// Decoder input. Contains `WINDOW_LEN` words of padding, followed by the words
/// of one frame. The padding keeps read windows at the start of the frame in
/// bounds.
#[derive(Default)]
pub struct WordBuffer(Words);

impl WordBuffer {
    /// Reads `word_count` words from `input`.
    ///
    /// If the input length is known, rejects a too-short input before
    /// allocating the buffer, because a corrupted word count can be huge.
    pub fn read_from(
        &mut self,
        input: &mut impl jam_codec::Input,
        word_count: usize,
    ) -> Result<(), InvalidRansStream> {
        if let Some(remaining_len) = input.remaining_len()?
            && remaining_len / size_of::<Word>() < word_count
        {
            return Err(InvalidRansStream);
        }
        let words = &mut self.0;
        words.len = WINDOW_LEN + word_count;
        words.grow(words.len);
        input.read(words_as_bytes_mut(&mut words.buffer[WINDOW_LEN..words.len]))?;
        Ok(())
    }

    pub fn reader(&self) -> WordReader<'_> {
        WordReader {
            words: self.0.as_slice(),
            position: WINDOW_LEN,
        }
    }
}

/// Divides the words of a frame into consecutive streams.
pub struct WordReader<'a> {
    words: &'a [Word],
    position: usize,
}

impl<'a> WordReader<'a> {
    pub fn stream(&mut self, word_count: u32) -> Result<StreamReader<'a>, InvalidRansStream> {
        let start = self.position;
        let end = usize::try_from(word_count)
            .ok()
            .and_then(|word_count| start.checked_add(word_count))
            .filter(|end| *end <= self.words.len())
            .ok_or(InvalidRansStream)?;
        self.position = end;
        Ok(StreamReader {
            words: self.words,
            start,
            position: end,
            is_invalid: false,
        })
    }
}

/// Reads the words of one stream from last to first.
pub struct StreamReader<'a> {
    words: &'a [Word],
    start: usize,
    position: usize,
    is_invalid: bool,
}

impl StreamReader<'_> {
    /// Calls `read` with the `WINDOW_LEN` words before the read position.
    ///
    /// On a corrupted stream the decoder can read past the start of the
    /// stream. Instead of checking for that on every symbol, this function
    /// checks once per window and [`finish`](Self::finish) reports the error.
    /// The read position is clamped to `WINDOW_LEN`, so the next window stays
    /// in bounds.
    pub(super) fn read_window<R>(
        &mut self,
        read: impl FnOnce(&mut ReverseWordWindow<'_>) -> R,
    ) -> R {
        let mut window = ReverseWordWindow {
            words: (&self.words[self.position - WINDOW_LEN..self.position])
                .try_into()
                .expect("invalid window length"),
            len: 0,
        };
        let result = read(&mut window);
        let position = self.position - window.len;
        self.is_invalid |= position < self.start;
        self.position = position.max(WINDOW_LEN);
        result
    }

    /// Fails unless the decoder consumed exactly the words of this stream.
    pub fn finish(self) -> Result<(), InvalidRansStream> {
        if self.is_invalid || self.position != self.start {
            return Err(InvalidRansStream);
        }
        Ok(())
    }
}

/// Window that reads words from its end to its start.
pub(super) struct ReverseWordWindow<'a> {
    words: &'a [Word; WINDOW_LEN],
    len: usize,
}

impl ReverseWordWindow<'_> {
    #[inline(always)]
    pub(super) fn peek(&self) -> Word {
        Word::from_le(self.words[WINDOW_LEN - 1 - self.len % WINDOW_LEN])
    }

    #[inline(always)]
    pub(super) fn skip(&mut self, count: usize) {
        self.len += count;
    }
}

fn words_as_bytes(words: &[Word]) -> &[u8] {
    // SAFETY: `Word` has no padding bytes, and `u8` has alignment 1.
    unsafe { core::slice::from_raw_parts(words.as_ptr().cast(), size_of_val(words)) }
}

fn words_as_bytes_mut(words: &mut [Word]) -> &mut [u8] {
    // SAFETY: `Word` has no padding bytes, all bit patterns are valid `Word`
    // values, and `u8` has alignment 1.
    unsafe { core::slice::from_raw_parts_mut(words.as_mut_ptr().cast(), size_of_val(words)) }
}
