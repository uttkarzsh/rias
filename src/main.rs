#![no_std] // no rust standard library
#![no_main] // no main entry point
#![reexport_test_harness_main = "test_main"]
#![feature(custom_test_frameworks)]
#![test_runner(rias::test_runner)]

use core::panic::PanicInfo;
use rias::println;

#[unsafe(no_mangle)] // do not mangle the name of this fn
pub extern "C" fn _start() -> ! {
    println!(
        r#"
       _            ___  ____
  _ __(_) __ _ ___ / _ \/ ___|
 | '__| |/ _` / __| | | \___ \
 | |  | | (_| \__ \ |_| |___) |
 |_|  |_|\__,_|___/\___/|____/

"#
    );

    rias::init();

<<<<<<< HEAD
    // x86_64::instructions::interrupts::int3();
=======
    // x86_64::instructions::interrupts::int3();    // invoke a breakpoint interruption

    // trigger a page fault
    unsafe {
        *(0xdeadbeef as *mut u8) = 42;
    };
>>>>>>> b8b8078 (add double fault handler)

    #[cfg(test)]
    test_main();

    println!("it did not crash yay");

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
