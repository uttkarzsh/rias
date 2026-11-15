use crate::{print, println};

pub fn execute(line: &str) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        None => {}
        Some("help") => {
            println!("commands:\n help\n echo\n clear\n uptime\n reboot\n exit/shutdown\n")
        }
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
        Some("uptime") => {
            let ticks = crate::interrupts::ticks();
            let secs = ticks * 65536 / 1_193_182;
            println!("{}m {}s", secs / 60, secs % 60);
        }
        Some("exit" | "shutdown") => {
            crate::shutdown();
        }
        Some("reboot") => {
            crate::reboot();
        }
        Some(cmd) => {
            println!("command not found: {cmd}");
        }
    }
}
