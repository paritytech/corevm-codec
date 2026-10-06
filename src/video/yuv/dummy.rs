use core::num::NonZero;

/// A dummy YUV frame that is used in place of real frames to avoid costly
/// memory allocations.
#[derive(Clone)]
pub struct DummyFrame {
    width: NonZero<u16>,
    height: NonZero<u16>,
    chroma_subsampling: bool,
}

impl DummyFrame {
    pub fn new(width: NonZero<u16>, height: NonZero<u16>, chroma_subsampling: bool) -> Self {
        Self {
            width,
            height,
            chroma_subsampling,
        }
    }

    pub fn width(&self) -> NonZero<u16> {
        self.width
    }

    pub fn height(&self) -> NonZero<u16> {
        self.height
    }

    pub fn chroma_subsampling(&self) -> bool {
        self.chroma_subsampling
    }
}
