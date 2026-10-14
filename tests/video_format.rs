use core::num::NonZero;
use corevm_codec::video;

struct VideoCase {
    width: u16,
    height: u16,
    frame_count: usize,
    noise_amplitude: u64,
    quantization_level: u8,
    chroma_subsampling: bool,
    raw: bool,
    encoded_hash: u64,
    decoded_hash: u64,
}

const VIDEO_CASES: [VideoCase; 6] = [
    VideoCase {
        width: 64,
        height: 48,
        frame_count: 4,
        noise_amplitude: 8,
        quantization_level: 4,
        chroma_subsampling: true,
        raw: false,
        encoded_hash: 0x6574372636a585a3,
        decoded_hash: 0x4812643521ae11f1,
    },
    VideoCase {
        width: 37,
        height: 23,
        frame_count: 3,
        noise_amplitude: 16,
        quantization_level: 3,
        chroma_subsampling: true,
        raw: false,
        encoded_hash: 0xc246f0ec1e97002c,
        decoded_hash: 0x782d62d655f755ef,
    },
    VideoCase {
        width: 33,
        height: 40,
        frame_count: 3,
        noise_amplitude: 256,
        quantization_level: 0,
        chroma_subsampling: false,
        raw: false,
        encoded_hash: 0x472d871aab5ed713,
        decoded_hash: 0xbcc7336e221e8c7b,
    },
    VideoCase {
        width: 50,
        height: 31,
        frame_count: 3,
        noise_amplitude: 4,
        quantization_level: 2,
        chroma_subsampling: false,
        raw: false,
        encoded_hash: 0x800c93b99e91cb67,
        decoded_hash: 0xb49955c54783a18c,
    },
    VideoCase {
        width: 40,
        height: 9,
        frame_count: 2,
        noise_amplitude: 64,
        quantization_level: 14,
        chroma_subsampling: true,
        raw: false,
        encoded_hash: 0x66879364f60a2f86,
        decoded_hash: 0xca6d8b4667112fe5,
    },
    VideoCase {
        width: 7,
        height: 5,
        frame_count: 2,
        noise_amplitude: 256,
        quantization_level: 4,
        chroma_subsampling: true,
        raw: true,
        encoded_hash: 0x0b805ac07b19e968,
        decoded_hash: 0xdfd09d92404987f0,
    },
];

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn synthetic_frames(case: &VideoCase) -> Vec<Vec<u8>> {
    let mut random_state = 0x9e3779b97f4a7c15_u64;
    let mut next_random = move || {
        random_state ^= random_state << 13;
        random_state ^= random_state >> 7;
        random_state ^= random_state << 17;
        random_state
    };
    (0..case.frame_count as u64)
        .map(|frame_index| {
            let mut frame = Vec::new();
            for y in 0..u64::from(case.height) {
                for x in 0..u64::from(case.width) {
                    for (channel, slope) in [(0, 3), (1, 5), (2, 7)] {
                        let gradient = x * slope + y * (slope + 2) + frame_index * 9 + channel * 40;
                        let noise = next_random() % case.noise_amplitude;
                        frame.push((gradient + noise) as u8);
                    }
                }
            }
            frame
        })
        .collect()
}

fn encode_and_decode(case: &VideoCase, frames: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let width = NonZero::new(case.width).expect("zero width");
    let height = NonZero::new(case.height).expect("zero height");
    let mut config = video::Config::default();
    config.quantization_level = case.quantization_level;
    config.chroma_subsampling = case.chroma_subsampling;
    config.raw = case.raw;
    let mut encoded = Vec::new();
    let mut encoder = video::Encoder::new(width, height, config);
    encoder.start(&mut encoded);
    for frame in frames {
        encoder.write_rgb888_frame(frame, &mut encoded);
    }
    encoder.finish(&mut encoded);
    let mut input = &encoded[..];
    let mut decoder = video::Decoder::new(&mut input).expect("invalid stream header");
    let mut decoded = vec![0; frames.concat().len()];
    for frame in decoded.chunks_exact_mut(frames[0].len()) {
        decoder
            .read_rgb888_frame(&mut input, frame)
            .expect("invalid frame");
    }
    assert!(input.is_empty());
    (encoded, decoded)
}

#[test]
fn video_stream_format() {
    let expected_hashes: Vec<(u64, u64)> = VIDEO_CASES
        .iter()
        .map(|case| (case.encoded_hash, case.decoded_hash))
        .collect();
    let actual_hashes: Vec<(u64, u64)> = VIDEO_CASES
        .iter()
        .map(|case| {
            let (encoded, decoded) = encode_and_decode(case, &synthetic_frames(case));
            (fnv1a_hash(&encoded), fnv1a_hash(&decoded))
        })
        .collect();
    assert_eq!(expected_hashes, actual_hashes);
}
