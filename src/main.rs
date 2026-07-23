#![no_std] // no rust standard library
#![no_main] // no main entry point

mod vga_buffer;

use core::panic::PanicInfo;

use crate::vga_buffer::print_smth;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // call this on panic (original panic is in std lib)
    loop {}
}

static HELLO: &[u8] = b"Hello World!";

#[unsafe(no_mangle)] // do not mangle the name of this fn
pub extern "C" fn _start() -> ! {
    print_smth();

    loop {}
}
