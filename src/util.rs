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

macro_rules! errors {
    ($(($error: ident $description: literal))+) => {
        $(
            #[doc = $description]
            #[derive(Debug)]
            pub struct $error;

            #[cfg(feature = "std")]
            impl std::error::Error for $error {}

            #[cfg(feature = "std")]
            impl core::fmt::Display for $error {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.write_str($description)
                }
            }
        )+
    };
}

pub(crate) use errors;

#[cfg(all(feature = "stats", feature = "std"))]
pub struct Timer {
    pub start: std::time::Instant,
    pub duration: std::cell::Cell<std::time::Duration>,
}

#[cfg(all(feature = "stats", feature = "std"))]
impl Timer {
    pub fn start() -> Self {
        Self {
            start: std::time::Instant::now(),
            duration: std::cell::Cell::new(std::time::Duration::ZERO),
        }
    }

    pub fn finish(&self) {
        self.duration.set(self.start.elapsed());
    }

    pub fn into_duration(self) -> std::time::Duration {
        self.duration.into_inner()
    }
}

#[cfg(all(feature = "stats", feature = "std"))]
macro_rules! timer_start {
    () => {
        $crate::Timer::start()
    };
}

#[cfg(not(all(feature = "stats", feature = "std")))]
macro_rules! timer_start {
    () => {
        ()
    };
}

pub(crate) use timer_start;

macro_rules! timer_finish {
    ($name: ident) => {
        #[cfg(all(feature = "stats", feature = "std"))]
        $name.finish();
        #[cfg(not(all(feature = "stats", feature = "std")))]
        {
            let _ = $name;
        }
    };
}

pub(crate) use timer_finish;
