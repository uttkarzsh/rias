use crate::{print, println};

const COMMANDS: &[(&str, &str)] = &[
    ("help", "print this list"),
    ("echo", "print arguments"),
    ("clear", "clear the screen"),
    ("uptime", "show uptime"),
    ("meminfo", "show heap usage"),
    ("reboot", "reboot"),
    ("riasfetch", "show system stats"),
    ("exit | shutdown", "shutdown and exit rias"),
];

fn uptime() -> (u64, u64, u64) {
    let ticks = crate::interrupts::ticks();
    let secs = ticks * 65536 / 1_193_182;
    (secs / 3600, secs / 60, secs % 60)
}

fn heap_usage() -> (usize, usize, usize) {
    let (used, free) = crate::allocator::heap_stats();
    let total = crate::allocator::HEAP_SIZE;
    (used / 1024, free / 1024, total / 1024)
}
pub fn execute(line: &str) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        None => {}
        Some("help") => {
            println!("Available Commands:");
            for command in COMMANDS {
                println!("{}: {}", command.0, command.1);
            }
            println!("");
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
            let (hr, min, sec) = uptime();
            println!("{}h {}m {}s", hr, min, sec);
        }
        Some("meminfo") => {
            let (used, free, total) = heap_usage();
            println!(
                "heap: {} / {} KiB used ({}%)",
                used,
                total,
                used * 100 / total
            );
            println!("free: {} KiB\n", free);
        }
        Some("riasfetch") => {
            let (hr, min, sec) = uptime();
            let (used, total, _free) = heap_usage();
            println!("os: riasOS");
            println!("kernel: rias");
            println!("shell: riash");
            println!("uptime: {}h {}m {}s", hr, min, sec);
            println!(
                "memory usage: {} / {} KiB used ({}%)",
                used,
                total,
                used * 100 / total
            );
            println!("created by: uttkarzsh");
            println!("code: https://github.com/uttkarzsh/rias\n");
        }
        Some("reboot") => {
            crate::reboot();
        }
        Some("exit" | "shutdown") => {
            crate::shutdown();
        }
        Some(cmd) => {
            println!("command not found: {cmd}");
        }
    }
}
