#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::num::NonZero;
use corevm_codec::{rans, video};

fn frame_len(width: NonZero<u16>, height: NonZero<u16>) -> usize {
    usize::from(width.get()) * usize::from(height.get()) * 3
}

/// Encodes RGB888 frames with CoreVM video codec and returns the encoded
/// stream.
pub fn encode_corevm(
    frames: &[u8],
    width: NonZero<u16>,
    height: NonZero<u16>,
    quantization_level: u8,
) -> Vec<u8> {
    let mut config = video::Config::default();
    config.quantization_level = quantization_level;
    let mut encoded = Vec::new();
    let mut encoder = video::Encoder::new(width, height, config);
    encoder.start(&mut encoded);
    for frame in frames.chunks_exact(frame_len(width, height)) {
        encoder.write_rgb888_frame(frame, &mut encoded);
    }
    encoder.finish(&mut encoded);
    encoded
}

/// Decodes CoreVM video stream into RGB888 frames.
pub fn decode_corevm(
    mut encoded: &[u8],
    frames: &mut [u8],
) -> Result<(), video::InvalidVideoStream> {
    let mut decoder = video::Decoder::new(&mut encoded)?;
    for frame in frames.chunks_exact_mut(frame_len(decoder.width(), decoder.height())) {
        decoder.read_rgb888_frame(&mut encoded, frame)?;
    }
    Ok(())
}

/// Encodes RGB888 frames with QOI followed by rANS and returns the total
/// encoded size.
pub fn encode_qoi_rans(frames: &[u8], width: NonZero<u16>, height: NonZero<u16>) -> usize {
    let (width, height) = (u32::from(width.get()), u32::from(height.get()));
    let mut qoi_frame = alloc::vec![0_u8; qoi::encode_max_len(width, height, 3)];
    let mut rans_words = rans::WordStack::with_capacity((width * height) as usize);
    let mut rans_header = Vec::new();
    let mut encoded_len = 0;
    for frame in frames.chunks_exact((width * height * 3) as usize) {
        let qoi_len = qoi::encode_to_buf(&mut qoi_frame[..], frame, width, height)
            .expect("invalid QOI frame size");
        rans_words.clear();
        rans_header.clear();
        rans::encode(&qoi_frame[..qoi_len], &mut rans_words, &mut rans_header);
        encoded_len += rans_header.len() + rans_words.as_bytes().len();
    }
    encoded_len
}
