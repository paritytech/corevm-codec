/// A dummy YUV frame that is used in place of real frames to avoid costly
/// memory allocations.
#[derive(Clone)]
pub struct DummyFrame {
    chroma_subsampling: bool,
}

impl DummyFrame {
    pub fn new(chroma_subsampling: bool) -> Self {
        Self { chroma_subsampling }
    }

    pub fn chroma_subsampling(&self) -> bool {
        self.chroma_subsampling
    }
}
