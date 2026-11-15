pub mod commands;
mod line;

use crate::{
    print, println,
    shell::line::Action,
    task::keyboard::KeyReader,
    vga_buffer::{with_color, Color},
};
use line::LineEditor;
use pc_keyboard::DecodedKey;

pub async fn run() {
    let mut keys = KeyReader::new();
    let mut line = LineEditor::new();
    print!("riash > ");
    loop {
        let DecodedKey::Unicode(c) = keys.next_key().await else {
            continue;
        };
        match line.feed(c) {
            Action::Echo(c) => with_color(Color::Cyan, || print!("{c}")),
            Action::Erase => print!("\x08"),
            Action::Submit => {
                println!();
                with_color(Color::LightRed, || commands::execute(line.line()));
                line.clear();
                print!("riash > ");
            }
            Action::Nothing => {}
        }
    }
}
