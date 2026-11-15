use crate::{print, println};

pub fn execute(line: &str) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        None => {}
        Some("help") => println!("commands: help echo clear exit/shutdown"),
        Some("echo") => {
            for w in parts {
                print!("{w} ");
            }
            println!();
        }
        Some("clear") => {
            x86_64::instructions::interrupts::without_interrupts(|| {
                let mut writer = crate::vga_buffer::WRITER.lock();
                writer.clear_screen();
                writer.update_cursor();
            });
        }
        Some("exit" | "shutdown") => {
            crate::shutdown();
        }
        Some(cmd) => {
            println!("command not found: {cmd}");
        }
    }
}
