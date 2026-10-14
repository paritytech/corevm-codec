use core::num::NonZero;

#[cfg(feature = "av1")]
mod av1;

const QUAKE_FRAMES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/quake-frames.rgb888"));
const WIDTH: NonZero<u16> = NonZero::new(320).expect("zero width");
const HEIGHT: NonZero<u16> = NonZero::new(200).expect("zero height");

fn print_size(encoded_len: usize) {
    println!(
        "Compressed size: {} KiB, compression ratio {:.1}%",
        encoded_len / 1024,
        100.0 * (1.0 - encoded_len as f64 / QUAKE_FRAMES.len() as f64)
    );
}

fn main() {
    #[cfg(feature = "corevm")]
    print_size(corevm_codec_bench::encode_corevm(QUAKE_FRAMES, WIDTH, HEIGHT, 4).len());
    #[cfg(feature = "qoi")]
    print_size(corevm_codec_bench::encode_qoi_rans(
        QUAKE_FRAMES,
        WIDTH,
        HEIGHT,
    ));
    #[cfg(feature = "av1")]
    print_size(av1::encode(QUAKE_FRAMES, WIDTH.get(), HEIGHT.get()));
}
