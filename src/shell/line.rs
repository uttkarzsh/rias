/// longest line the editor accepts. with the 6-column prompt a line always
/// fits on one 80-column VGA row, so backspace never has to wrap upwards.
pub const MAX_LINE: usize = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Echo(char),
    Erase,
    Submit,
}

pub struct LineEditor {
    buf: [u8; MAX_LINE],
    len: usize,
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl LineEditor {
    pub const fn new() -> Self {
        LineEditor {
            buf: [0; MAX_LINE],
            len: 0,
        }
    }

    pub fn feed(&mut self, c: char) -> Action {
        match c {
            '\n' => Action::Submit,
            '\u{8}' if self.len > 0 => {
                self.len -= 1;
                Action::Erase
            }
            ' '..='~' if self.len < MAX_LINE => {
                self.buf[self.len] = c as u8;
                self.len += 1;
                Action::Echo(c)
            }
            _ => Action::Nothing,
        }
    }

    pub fn line(&self) -> &str {
        // only printable ascii is ever stored, so this can't fail
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

#[test_case]
fn typing_builds_the_line() {
    let mut e = LineEditor::new();
    assert_eq!(e.feed('h'), Action::Echo('h'));
    assert_eq!(e.feed('i'), Action::Echo('i'));
    assert_eq!(e.line(), "hi");
}

#[test_case]
fn backspace_removes_the_last_char() {
    let mut e = LineEditor::new();
    e.feed('a');
    e.feed('b');
    assert_eq!(e.feed('\u{8}'), Action::Erase);
    assert_eq!(e.line(), "a");
}

#[test_case]
fn backspace_on_empty_line_does_nothing() {
    let mut e = LineEditor::new();
    assert_eq!(e.feed('\u{8}'), Action::Nothing);
}

#[test_case]
fn line_stops_growing_at_max_length() {
    let mut e = LineEditor::new();
    for _ in 0..MAX_LINE {
        e.feed('x');
    }
    assert_eq!(e.feed('x'), Action::Nothing);
    assert_eq!(e.line().len(), MAX_LINE);
}

#[test_case]
fn control_chars_are_ignored() {
    let mut e = LineEditor::new();
    assert_eq!(e.feed('\t'), Action::Nothing);
    assert_eq!(e.line(), "");
}
