#![no_std]
#![doc = include_str!("../README.md")]

#[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
core::compile_error!("Only 32-bit and 64-bit architectures are supported.");

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

mod io;
mod util;

use self::util::*;

#[doc(hidden)]
pub mod rans;
pub mod video;

pub use self::io::*;
