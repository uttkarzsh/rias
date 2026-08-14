#![no_std] // no rust standard library
#![no_main] // no main entry point
#![reexport_test_harness_main = "test_main"]
#![feature(custom_test_frameworks)]
#![test_runner(rias::test_runner)]

use core::panic::PanicInfo;
use rias::println;

#[unsafe(no_mangle)] // do not mangle the name of this fn
pub extern "C" fn _start() -> ! {
    println!("hello world");

    #[cfg(test)]
    test_main();

    loop {}
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    rias::test_panic_handler(info);
}
