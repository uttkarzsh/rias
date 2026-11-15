use crate::{print, println};

const COMMANDS: &[(&str, &str)] = &[
    ("help", "print this list."),
    ("echo", "print arguments."),
    ("clear", "clear the screen"),
    ("uptime", "show uptime"),
    ("reboot", "reboot"),
    ("exit | shutdown", "shutdown and exit rias"),
];
pub fn execute(line: &str) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        None => {}
        Some("help") => {
            for command in COMMANDS {
                println!("{}: {}", command.0, command.1);
            }
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
