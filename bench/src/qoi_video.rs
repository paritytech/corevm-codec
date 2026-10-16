use crate::QUAKE_FRAMES;
use corevm_codec::rans;

pub fn transcode(width: u16, height: u16) -> usize {
    let mut buf = vec![0_u8; qoi::encode_max_len(width.into(), height.into(), 3)];
    let mut rans_words = rans::WordStack::with_capacity(usize::from(width) * usize::from(height));
    let mut rans_header = Vec::new();
    let mut compressed_len = 0;
    for frame in QUAKE_FRAMES.chunks_exact(usize::from(width) * usize::from(height) * 3) {
        let len = qoi::encode_to_buf(&mut buf[..], frame, width.into(), height.into())
            .expect("We provide good values");
        let buf = &buf[..len];
        rans_words.clear();
        rans_header.clear();
        rans::encode(buf, &mut rans_words, &mut rans_header);
        compressed_len += rans_header.len() + rans_words.as_bytes().len();
    }
    compressed_len
}
