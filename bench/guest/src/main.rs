#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::num::NonZero;

const HEAP_SIZE: usize = 128 * 1024 * 1024;

#[global_allocator]
static mut GLOBAL_ALLOCATOR: picoalloc::Mutex<
    picoalloc::Allocator<picoalloc::ArrayPointer<HEAP_SIZE>>,
> = {
    static mut HEAP: picoalloc::Array<HEAP_SIZE> = picoalloc::Array([0; HEAP_SIZE]);
    picoalloc::Mutex::new(picoalloc::Allocator::new(unsafe {
        picoalloc::ArrayPointer::new(&raw mut HEAP)
    }))
};

static mut FRAMES: Vec<u8> = Vec::new();
static mut ENCODED: Vec<u8> = Vec::new();
static mut DECODED_FRAMES: Vec<u8> = Vec::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        core::arch::asm!("unimp", options(noreturn));
    }
}

fn frames() -> &'static mut Vec<u8> {
    unsafe { &mut *(&raw mut FRAMES) }
}

fn encoded() -> &'static mut Vec<u8> {
    unsafe { &mut *(&raw mut ENCODED) }
}

fn decoded_frames() -> &'static mut Vec<u8> {
    unsafe { &mut *(&raw mut DECODED_FRAMES) }
}

fn dimensions(width: u32, height: u32) -> (NonZero<u16>, NonZero<u16>) {
    let width = NonZero::new(width as u16).expect("zero width");
    let height = NonZero::new(height as u16).expect("zero height");
    (width, height)
}

#[polkavm_derive::polkavm_export]
extern "C" fn allocate_frames(len: u32) -> u32 {
    *frames() = alloc::vec![0; len as usize];
    *decoded_frames() = alloc::vec![0; len as usize];
    frames().as_ptr() as u32
}

#[polkavm_derive::polkavm_export]
extern "C" fn decoded_frames_address() -> u32 {
    decoded_frames().as_ptr() as u32
}

#[polkavm_derive::polkavm_export]
extern "C" fn encode_corevm(width: u32, height: u32, quantization_level: u32) -> u32 {
    let (width, height) = dimensions(width, height);
    *encoded() =
        corevm_codec_bench::encode_corevm(frames(), width, height, quantization_level as u8);
    encoded().len() as u32
}

#[polkavm_derive::polkavm_export]
extern "C" fn decode_corevm() -> u32 {
    corevm_codec_bench::decode_corevm(encoded(), decoded_frames()).expect("invalid stream");
    decoded_frames().len() as u32
}

#[polkavm_derive::polkavm_export]
extern "C" fn encode_qoi_rans(width: u32, height: u32) -> u32 {
    let (width, height) = dimensions(width, height);
    corevm_codec_bench::encode_qoi_rans(frames(), width, height) as u32
}
