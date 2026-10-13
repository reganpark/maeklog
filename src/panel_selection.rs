//! View-only selection. Captured lines stay stable until selection is dismissed.
use crate::panel_style::Line;
use unicode_width::UnicodeWidthChar;

pub struct Selection {
    pub lines: Vec<Line>,
    pub scroll: usize,
    start: (usize, usize),
    end: (usize, usize),
    width: usize,
    height: usize,
    pub dragging: bool,
    moved: bool,
}
impl Selection {
    pub fn new(lines: Vec<Line>, scroll: usize, width: u16, height: u16, x: u16, y: u16) -> Self {
        let start = (scroll + usize::from(y), usize::from(x));
        Self {
            lines,
            scroll,
            start,
            end: start,
            width: usize::from(width),
            height: usize::from(height).saturating_sub(1),
            dragging: true,
            moved: false,
        }
    }
    pub fn update(&mut self, x: u16, y: u16) {
        self.end = (
            self.scroll + usize::from(y).min(self.height.saturating_sub(1)),
            usize::from(x).min(self.width.saturating_sub(1)),
        );
        self.moved |= self.end != self.start;
    }
    pub fn selected(&self, row: usize, column: usize, cells: usize) -> bool {
        if !self.moved {
            return false;
        }
        let (start, end) = (self.start.min(self.end), self.start.max(self.end));
        (row, column + cells.saturating_sub(1)) >= start && (row, column) <= end
    }
    pub fn text(&self) -> String {
        if !self.moved {
            return String::new();
        }
        let (start, end) = (self.start.min(self.end), self.start.max(self.end));
        let mut rows = Vec::new();
        for row in start.0..=end.0 {
            let Some(line) = self.lines.get(row) else {
                break;
            };
            let mut text = String::new();
            let mut col = 0;
            let mut previous_selected = false;
            for ch in line.chars() {
                let cells = ch.width().unwrap_or(0);
                let selected = if cells == 0 {
                    previous_selected
                } else {
                    self.selected(row, col, cells)
                };
                if selected {
                    text.push(ch);
                }
                previous_selected = selected;
                col += cells;
            }
            rows.push(text.trim_end().to_string());
        }
        rows.join("\n")
    }
}

// OSC 52 requests the terminal/Herdr clipboard path. It does not read clipboard
// contents and provides no success acknowledgement; never claim verified copy.
pub fn clipboard_sequence(text: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if text.is_empty() || text.len() > 65_536 {
        return None;
    }
    let mut encoded = String::new();
    for bytes in text.as_bytes().chunks(3) {
        let a = bytes[0];
        let b = bytes.get(1).copied().unwrap_or(0);
        let c = bytes.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(a >> 2)]));
        encoded.push(char::from(ALPHABET[usize::from(((a & 3) << 4) | (b >> 4))]));
        encoded.push(if bytes.len() > 1 {
            char::from(ALPHABET[usize::from(((b & 15) << 2) | (c >> 6))])
        } else {
            '='
        });
        encoded.push(if bytes.len() > 2 {
            char::from(ALPHABET[usize::from(c & 63)])
        } else {
            '='
        });
    }
    Some(format!("\x1b]52;c;{encoded}\x07"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel_style::Role;
    #[test]
    fn selection_uses_cells_and_frozen_rows_in_both_directions() {
        let lines = ["ignored", "한글 ABC", "다음 줄"]
            .map(|text| Line {
                text: text.into(),
                role: Role::Body,
            })
            .to_vec();
        let mut forward = Selection::new(lines.clone(), 1, 30, 10, 1, 0);
        assert!(forward.text().is_empty());
        forward.update(1, 1);
        assert_eq!(forward.text(), "한글 ABC\n다");
        let mut backward = Selection::new(lines, 1, 30, 10, 1, 1);
        backward.update(1, 0);
        assert_eq!(backward.text(), forward.text());
        assert!(forward.selected(1, 0, 2));
        assert!(!forward.selected(0, 0, 1));
    }
    #[test]
    fn clipboard_is_bounded_encoded_and_explicit() {
        assert_eq!(
            clipboard_sequence("한글").unwrap(),
            "\x1b]52;c;7ZWc6riA\x07"
        );
        for (s, b64) in [("a", "YQ=="), ("ab", "YWI="), ("abc", "YWJj")] {
            assert_eq!(
                clipboard_sequence(s).unwrap(),
                format!("\x1b]52;c;{b64}\x07")
            );
        }
        assert!(clipboard_sequence("").is_none());
        assert!(clipboard_sequence(&"a".repeat(65_537)).is_none());
    }
}
