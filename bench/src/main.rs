use core::num::NonZero;

#[cfg(feature = "qoi")]
mod qoi_video;

#[cfg(feature = "av1")]
mod av1;

pub(crate) const QUAKE_FRAMES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/quake-frames.rgb888"));

fn main() {
    let width = NonZero::new(320).expect("Constant is non-zero");
    let height = NonZero::new(200).expect("Constant is non-zero");
    #[cfg(feature = "corevm")]
    print_compressed_size(transcode_corevm(width, height));
    #[cfg(feature = "qoi")]
    print_compressed_size(qoi_video::transcode(width.get(), height.get()));
    #[cfg(feature = "av1")]
    print_compressed_size(av1::transcode(width.get(), height.get()));
}

fn print_compressed_size(compressed_len: usize) {
    println!("Compressed size: {} KiB", compressed_len / 1024);
}

#[cfg(feature = "corevm")]
fn transcode_corevm(width: NonZero<u16>, height: NonZero<u16>) -> usize {
    use corevm_codec::video;
    let mut config = video::Config::default();
    config.quantization_level = 4;
    let mut buf = Vec::new();
    let mut encoder = video::Encoder::new(width, height, config);
    encoder.start(&mut buf);
    for frame in QUAKE_FRAMES.chunks_exact(usize::from(width.get()) * usize::from(height.get()) * 3)
    {
        encoder.write_rgb888_frame(frame, &mut buf);
    }
    encoder.finish(&mut buf);
    buf.len()
}
