#![no_std] // no rust standard library
#![no_main] // no main entry point

mod vga_buffer;

use core::panic::PanicInfo;

use crate::vga_buffer::print_smth;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // call this on panic (original panic is in std lib)
    println!("{}", info);
    loop {}
}

#[unsafe(no_mangle)] // do not mangle the name of this fn
pub extern "C" fn _start() -> ! {
    println!("hello world");
    panic!("wowowowoow");
    loop {}
}
