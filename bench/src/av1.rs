use crate::QUAKE_FRAMES;
use rav1e::prelude::*;

mod bt709;

pub fn transcode(width: u16, height: u16) {
    let mut speed_settings = SpeedSettings::from_preset(u8::MAX);
    speed_settings.rdo_lookahead_frames = 1;
    speed_settings.cdef = false;
    speed_settings.transform.tx_domain_rate = true;
    speed_settings.segmentation = SegmentationLevel::Disabled;
    speed_settings.scene_detection_mode = SceneDetectionSpeed::None;
    let encoder_config = EncoderConfig {
        width: width as usize,
        height: height as usize,
        speed_settings,
        ..Default::default()
    };
    let config = Config::new()
        .with_threads(1)
        .with_encoder_config(encoder_config);
    let mut av1_context: Context<u8> = config.new_context().expect("new_context failed");
    for rgb in QUAKE_FRAMES.chunks_exact(width as usize * height as usize * 3) {
        let mut frame = av1_context.new_frame();
        let (y, uv) = frame.planes.split_at_mut(1);
        let (u, v) = uv.split_at_mut(1);
        let y_offset = y[0].cfg.xorigin;
        let y_stride = y[0].cfg.stride;
        let u_offset = u[0].cfg.xorigin;
        let u_stride = u[0].cfg.stride;
        let v_offset = v[0].cfg.xorigin;
        let v_stride = v[0].cfg.stride;
        bt709::rgb888_to_yuv420p(
            rgb,
            width,
            height,
            &mut y[0].data[..],
            y_offset,
            y_stride,
            &mut u[0].data[..],
            u_offset,
            u_stride,
            &mut v[0].data[..],
            v_offset,
            v_stride,
        );
        av1_context.send_frame(frame).expect("send_frame failed");
    }
    av1_context.flush();
    loop {
        match av1_context.receive_packet() {
            Ok(_packet) => {}
            Err(EncoderStatus::Encoded) => {
                // A frame was encoded without emitting a packet. This is
                // normal, just proceed as usual.
            }
            Err(EncoderStatus::LimitReached) => {
                // All frames have been encoded. Time to break out of the
                // loop.
                break;
            }
            Err(EncoderStatus::NeedMoreData) => {
                // The encoder has requested additional frames. Push the
                // next frame in, or flush the encoder if there are no
                // frames left (on None).
                unreachable!();
            }
            Err(EncoderStatus::EnoughData) => {
                // Since we aren't trying to push frames after flushing,
                // this should never happen in this example.
                unreachable!();
            }
            Err(EncoderStatus::NotReady) => {
                // We're not doing two-pass encoding, so this can never
                // occur.
                unreachable!();
            }
            Err(EncoderStatus::Failure) => {
                panic!("Failed");
            }
        }
    }
}
