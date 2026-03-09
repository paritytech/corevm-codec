use crate::QUAKE_FRAMES;
use corevm_codec::rans;

pub fn transcode(width: u16, height: u16) {
    let mut buf = vec![0_u8; qoi::encode_max_len(width.into(), height.into(), 3)];
    let mut rans_buf = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for frame in QUAKE_FRAMES.chunks_exact(usize::from(width) * usize::from(height) * 3) {
        let len = qoi::encode_to_buf(&mut buf[..], frame, width.into(), height.into())
            .expect("We provide good values");
        let buf = &buf[..len];
        rans_buf.clear();
        rans::encode(buf, &mut rans_buf);
    }
}
