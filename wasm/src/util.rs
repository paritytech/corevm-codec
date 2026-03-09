pub trait ToUsize {
    fn to_usize(self) -> usize;
}

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
impl ToUsize for u32 {
    fn to_usize(self) -> usize {
        self as usize
    }
}

#[cfg(target_pointer_width = "64")]
impl ToUsize for u64 {
    fn to_usize(self) -> usize {
        self as usize
    }
}
