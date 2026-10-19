#![no_std] // no rust standard library
#![no_main] // no main entry point
#![reexport_test_harness_main = "test_main"]
#![feature(custom_test_frameworks)]
#![test_runner(rias::test_runner)]

use core::panic::PanicInfo;
use rias::{print, println};

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

    // x86_64::instructions::interrupts::int3();    // invoke a breakpoint interruption
    // trigger a page fault
    /*
        let ptr = 0x206386 as *mut u8;
        unsafe {
            let x = *ptr;
        }
        println!("read worked");

        unsafe {
            *ptr = 42;
        }
        println!("write worked");
    */

    use x86_64::registers::control::Cr3;

    let (level_4_page_table, _) = Cr3::read();
    println!(
        "Level 4 page table at: {:?}",
        level_4_page_table.start_address()
    );

    #[cfg(test)]
    test_main();

    println!("it did not crash yay");
    rias::hlt_loop();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    rias::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    rias::test_panic_handler(info);
}
