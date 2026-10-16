# CoreVM codec

This is a collection of codecs that are used internally by CoreVM to encode and decode output streams produced by the guest.
Currently this includes only video codec.


## Video codec

This is fast and reasonably efficient video codec optimized for PolkaVM.
This codec uses YUV color model, Haar wavelet transform, scalar quantization, simple inter-frame delta encoding, zigzag encoding, and rANS encoder with two contexts.
To the best of author's knowledge this is the smallest number of stages of a video encoding pipeline that are required to get decent compression ratio without significant performance degradation.

There are many [royalty-free and open source video codecs](https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Video_codecs) but they tend to trade encoding speed for better compression ratio.
This is acceptable tradeoff for a codec that is made for the web, but unacceptable in CoreVM context where a guest has only 6 seconds to generate and encode all video frames for the next time slot (i.e. next 6 seconds).
Another problem is that a sizable portion of the source code in such codecs is written in x86/ARM assembly ([example](https://github.com/xiph/rav1e/tree/master/src/x86)), and fallback code doesn't perform well on RISCV.
Hence a custom codec.

### Benchmarks

Below are the results of a benchmark for AV1, CoreVM and QOI+rANS codecs.
The benchmark encodes 6 seconds worth of Quake frames.
The benchmark was run inside CoreVM. Original video size is 9108 KiB.
This is the size of the 144 frames in Quake's indexed format (a 768-byte palette and 64000 8-bit indices per frame); the benchmark encodes the same frames converted to RGB888.

| Codec | Compressed size  | Encoding time | Compression ratio | Crate |
|-------|-----------------:|--------------:|------------------:|-------|
| CoreVM | 1498 KiB | 171 ms | 84% | |
| QOI+rANS | 5026 KiB | 181 ms | 45% | [qoi](https://docs.rs/qoi/latest/qoi/) |
| AV1 | 268 KiB | 15235 ms | 97% | [rav1e](https://docs.rs/rav1e/latest/rav1e/) |

Same benchmark on `x86_64` without CoreVM.

| Codec | Encoding time |
|-------|--------------:|
| CoreVM | 130 ms |
| QOI+rANS | 80 ms |
| AV1 | 1700 ms |
