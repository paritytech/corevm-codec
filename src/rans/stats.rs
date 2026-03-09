/// ANS stream statistics.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct Stats {
    /// The number of bytes written to the output excluding the footer.
    pub data_len: u64,
    /// Footer size in bytes.
    pub footer_len: u32,
    /// The number of non-zero frequencies.
    pub num_freqs: u32,
    /// The total number of accumulated samples.
    pub count: u32,
}

impl Stats {
    /// Returns the number of bytes written to the output including the footer.
    pub fn total_out(&self) -> u64 {
        self.data_len + u64::from(self.footer_len)
    }
}

impl core::ops::AddAssign<&Stats> for Stats {
    fn add_assign(&mut self, other: &Stats) {
        self.data_len += other.data_len;
        self.footer_len += other.footer_len;
        self.num_freqs += other.num_freqs;
        self.count += other.count;
    }
}
