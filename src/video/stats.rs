use crate::rans;

/// Video stream statistics.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
#[doc(hidden)]
pub struct Stats {
    /// Luma (Y).
    pub y: rans::Stats,
    /// Chroma (U).
    pub u: rans::Stats,
    /// Chroma (V).
    pub v: rans::Stats,
    /// Video encoder performance.
    #[cfg(all(feature = "stats", feature = "std"))]
    pub time: TimeStats,
    /// The number of accumulated samples.
    pub count: u32,
}

#[cfg(feature = "stats")]
impl core::ops::AddAssign<&Stats> for Stats {
    fn add_assign(&mut self, other: &Stats) {
        self.y += &other.y;
        self.u += &other.u;
        self.v += &other.v;
        #[cfg(all(feature = "stats", feature = "std"))]
        {
            self.time += &other.time;
        }
        self.count += other.count;
    }
}

/// Video encoder performance.
#[cfg(all(feature = "stats", feature = "std"))]
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
#[doc(hidden)]
pub struct TimeStats {
    pub transform: std::time::Duration,
    pub delta: std::time::Duration,
    pub signmag: std::time::Duration,
    /// The number of accumulated samples.
    pub count: u32,
}

#[cfg(all(feature = "stats", feature = "std"))]
impl core::ops::AddAssign<&TimeStats> for TimeStats {
    fn add_assign(&mut self, other: &TimeStats) {
        self.transform += other.transform;
        self.delta += other.delta;
        self.signmag += other.signmag;
        self.count += other.count;
    }
}
