# CoreVM codec

This is a collection of codecs that are used internally by CoreVM to encode and decode output streams produced by the guest.
Currently this includes only video codec.


## Video codec

This is fast and reasonably efficient video codec optimized for PVM.
This codec uses YUV color model, Haar wavelet transform, scalar quantization, rANS encoder, and simple inter-frame delta encoding.
To the best of author's knowledge this is the smallest number of stages of a video encoding pipeline that are required to get decent compression ratio without significant performance degradation.

There are many [royalty-free and open source video codecs](https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Video_codecs) but they tend to trade encoding speed for better compression ratio.
This is acceptable tradeoff for a codec that is made for the web, but unacceptable in CoreVM context where a guest has only 6 seconds to generate and encode all video frames for the next time slot (i.e. next 6 seconds).
Another problem is that a sizable portion of the source code in such codecs is written in x86/ARM assembly ([example](https://github.com/xiph/rav1e/tree/master/src/x86)), and fallback code doesn't perform well on RISCV.
Hence a custom codec.

### Benchmarks

The benchmarks encode 6 seconds (144 frames, 320x200) of Quake gameplay with AV1, CoreVM and QOI+rANS codecs.
The raw RGB888 video size is 27000 KiB; the compression ratio is one minus the compressed size divided by the raw size.
CoreVM codec uses quantization level 4.
The benchmarks were run on AMD Ryzen Threadripper 3970X.

The following results were obtained under PolkaVM's recompiler (using `./scripts/bench.sh polkavm`),
with gas metering enabled and the full gas cost model with L2 cache hit memory access cost.
AV1 encoder depends on the Rust standard library and runs only natively.

| Codec | Compressed size | Compression ratio | Encoding time | Encoding gas per frame | Decoding time | Decoding gas per frame | Crate |
|-------|----------------:|------------------:|--------------:|-----------------------:|--------------:|-----------------------:|-------|
| corevm-codec 0.3 | 1498 KiB | 94.5% | 138 ms | 14.5 M | 146 ms | 11.7 M | |
| QOI+rANS | 5026 KiB | 81.4% | 105 ms | 19.8 M | | | [qoi](https://docs.rs/qoi/latest/qoi/) |

The following results were obtained natively (using `./scripts/bench.sh native`):

| Codec | Compressed size | Compression ratio | Encoding time | Crate |
|-------|----------------:|------------------:|--------------:|-------|
| corevm-codec 0.3 | 1498 KiB | 94.5% | 127 ms | |
| QOI+rANS | 5026 KiB | 81.4% | 82 ms | [qoi](https://docs.rs/qoi/latest/qoi/) |
| AV1 | 268 KiB | 99.0% | 1617 ms | [rav1e](https://docs.rs/rav1e/latest/rav1e/) |
