#![no_std] // no rust standard library
#![no_main] // no main entry point

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // call this on panic (original panic is in std lib)
    loop {}
}

#[unsafe(no_mangle)] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    // actual entry point as main does not exist here
    loop {}
}
