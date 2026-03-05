use corevm_codec::video::InvalidVideoStream;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Error(#[allow(unused)] JsValue);

impl From<InvalidVideoStream> for Error {
    fn from(_: InvalidVideoStream) -> Self {
        Self(JsValue::from_str("Invalid video stream"))
    }
}

#[wasm_bindgen]
pub struct VideoDecoder {
    decoder: corevm_codec::video::Decoder,
    input: VecInput,
}

#[wasm_bindgen]
impl VideoDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(input: Vec<u8>) -> Result<Self, Error> {
        let mut input = VecInput {
            buf: input,
            offset: 0,
        };
        let decoder = corevm_codec::video::Decoder::new(&mut input)?;
        Ok(Self { decoder, input })
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u16 {
        self.decoder.width().get()
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u16 {
        self.decoder.height().get()
    }

    #[wasm_bindgen(js_name = "readFrame")]
    pub fn read_frame(&mut self) -> Result<Uint8Array, Error> {
        let frame_len =
            u32::from(self.decoder.width().get()) * u32::from(self.decoder.height().get()) * 3;
        let mut frame = vec![0_u8; frame_len as usize];
        self.decoder
            .read_rgb888_frame(&mut self.input, &mut frame[..])?;
        Ok(Uint8Array::new_from_slice(&frame[..]))
    }
}

struct VecInput {
    buf: Vec<u8>,
    offset: usize,
}

impl jam_codec::Input for VecInput {
    fn remaining_len(&mut self) -> Result<Option<usize>, jam_codec::Error> {
        Ok(Some(self.buf.len() - self.offset))
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), jam_codec::Error> {
        let len = buf.len();
        let slice = &self.buf.get(..len).ok_or("Buffer overflow")?;
        buf.copy_from_slice(slice);
        self.offset += len;
        Ok(())
    }
}
