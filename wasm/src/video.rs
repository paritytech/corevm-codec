use crate::ToUsize;
use corevm_codec::VecInput;
use js_sys::Error;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

fn invalid_video_stream() -> Error {
    Error::new("Invalid video stream")
}

/// Video decoder.
#[wasm_bindgen]
pub struct VideoDecoder {
    decoder: corevm_codec::video::Decoder,
    input: VecInput,
}

#[wasm_bindgen]
impl VideoDecoder {
    /// Creates new video decoder using the provided byte array as the input.
    #[wasm_bindgen(constructor)]
    pub fn new(input: Uint8Array) -> Result<Self, Error> {
        let buf = input.to_vec();
        let mut input = VecInput::new(buf);
        let decoder =
            corevm_codec::video::Decoder::new(&mut input).map_err(|_| invalid_video_stream())?;
        Ok(Self { decoder, input })
    }

    /// Returns video frame width.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u16 {
        self.decoder.width().get()
    }

    /// Returns video frame height.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u16 {
        self.decoder.height().get()
    }

    /// Returns `true` if the input contains more frames.
    #[wasm_bindgen(js_name = "hasMoreFrames")]
    pub fn has_more_frames(&self) -> bool {
        !self.input.is_empty()
    }

    /// Decode next frame as RGBA8888.
    ///
    /// Returns decoded frame as `Uint8Array`. Throws an error if there are no
    /// more frames in the input.
    ///
    /// All pixels are fully opaque.
    #[wasm_bindgen(js_name = "readFrame")]
    pub fn read_frame(&mut self) -> Result<Uint8Array, Error> {
        let len = u32::from(self.decoder.width().get()) * u32::from(self.decoder.height().get());
        let frame_len = 4_u32.checked_mul(len).ok_or_else(invalid_video_stream)?;
        let mut frame = vec![0_u8; frame_len.to_usize()];
        self.decoder
            .read_rgba8888_frame(&mut self.input, &mut frame[..])
            .map_err(|_| invalid_video_stream())?;
        Ok(Uint8Array::new_from_slice(&frame[..]))
    }
}
