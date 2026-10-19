#![no_std] // no rust standard library
#![no_main] // no main entry point
#![reexport_test_harness_main = "test_main"]
#![feature(custom_test_frameworks)]
#![test_runner(rias::test_runner)]

use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use rias::{print, println};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use rias::memory::translate_addr;
    use x86_64::VirtAddr;

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

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    let addresses = [
        // identity mapped vga buffer page
        0xb8000,
        0x201008,                         // some code page
        0x0100_0020_1a10,                 // some stack page
        boot_info.physical_memory_offset, //virt address mapped to phys address 0
    ];

    for &address in &addresses {
        let virt = VirtAddr::new(address);
        let phys = unsafe { translate_addr(virt, phys_mem_offset) };
        println!("{:?} -> {:?}", virt, phys);
    }

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
