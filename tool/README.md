# CoreVM codec tool

This tool converts video between various raw formats and CoreVM format.
To actually play the video convert it to raw format and then use `ffplay`.
Mostly useful for development.

## Typical workflow

```bash
# Transcode Quake frames in the original format (indexed RGB888) to CoreVM format.
cargo run -p corevm-codec-tool -- \
    -f quake -F corevm --width 320 --height 200 -q 4 \
    <quake-frames.bin >quake-frames.corevm

# Transcode from CoreVM to RGB888.
cargo run -p corevm-codec-tool -- \
    -f corevm -F rgb888 --width 320 --height 200 \
    <quake-frames.corevm >quake-frames.corevm.rgb888

# Play the resulting video.
ffplay -loglevel warning \
    -f rawvideo -pixel_format rgb24 -video_size 320x200 -framerate 25 \
    -i quake-frames.corevm.rgb888
```

## Other useful commands

```
# Convert raw Quake frames in RGB888 format to "lossless" mp4.
ffmpeg -f rawvideo -pix_fmt rgb24 -video_size 320x200 -i quake-frames.rgb888 \
    -vcodec h264 -preset veryslow -qscale 0 quake-frames.mp4
```
