//! Explicit local language preference; browsing never changes the UI or queues work.
use crate::i18n::Language;
use crate::language_settings::{self, Preference};
use crate::panel_style::{self, Line, Role};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use unicode_width::UnicodeWidthChar;

const CHOICES: [Preference; 3] = [Preference::Auto, Preference::En, Preference::Ko];

#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    pub saved: bool,
}

pub struct Picker {
    current: Option<Preference>,
    selected: usize,
    load_error: Option<String>,
    save_error: Option<String>,
    row_targets: Vec<(usize, usize)>,
    frame_width: usize,
    pressed: Option<usize>,
}

pub const fn name(preference: Preference) -> &'static str {
    match preference {
        Preference::Auto => "자동 / Auto",
        Preference::En => "English",
        Preference::Ko => "한국어",
    }
}

const fn language_name(language: Language) -> &'static str {
    match language {
        Language::En => "English",
        Language::Ko => "한국어",
    }
}

impl Picker {
    pub fn open() -> Self {
        let (current, load_error) = match language_settings::load() {
            Ok(preference) => (Some(preference), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            current,
            selected: current
                .and_then(|choice| CHOICES.iter().position(|entry| *entry == choice))
                .unwrap_or(0),
            load_error,
            save_error: None,
            row_targets: Vec::new(),
            frame_width: 0,
            pressed: None,
        }
    }

    /// Refresh the saved marker without replacing an in-progress cursor choice.
    pub fn poll_saved(&mut self) {
        match language_settings::load() {
            Ok(preference) => {
                self.current = Some(preference);
                self.load_error = None;
            }
            Err(error) => self.load_error = Some(error.to_string()),
        }
    }

    pub fn handle(&mut self, event: &Event) -> Outcome {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
            && key.modifiers.is_empty()
        {
            match key.code {
                KeyCode::Esc => {
                    return Outcome {
                        close: true,
                        saved: false,
                    };
                }
                KeyCode::Up => self.selected = self.selected.saturating_sub(1),
                KeyCode::Down => self.selected = (self.selected + 1).min(CHOICES.len() - 1),
                KeyCode::Home => self.selected = 0,
                KeyCode::End => self.selected = CHOICES.len() - 1,
                KeyCode::Enter => return self.apply(),
                _ => {}
            }
        }
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.pressed = self.row_at(usize::from(mouse.row), usize::from(mouse.column));
                    if let Some(index) = self.pressed {
                        self.selected = index;
                    } else {
                        return Outcome {
                            close: true,
                            saved: false,
                        };
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let hit = self.row_at(usize::from(mouse.row), usize::from(mouse.column));
                    if let Some(pressed) = self.pressed.take()
                        && Some(pressed) == hit
                    {
                        return self.apply();
                    }
                }
                MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    self.selected = (self.selected + 1).min(CHOICES.len() - 1);
                }
                _ => {}
            }
        }
        if matches!(event, Event::Resize(..)) {
            self.pressed = None;
        }
        Outcome::default()
    }

    fn row_at(&self, row: usize, column: usize) -> Option<usize> {
        if column >= self.frame_width {
            return None;
        }
        self.row_targets
            .iter()
            .find(|(target, _)| *target == row)
            .map(|(_, index)| *index)
    }

    fn apply(&mut self) -> Outcome {
        if self.frame_width < 15
            || !self
                .row_targets
                .iter()
                .any(|(_, index)| *index == self.selected)
        {
            return Outcome::default();
        }
        match language_settings::save(CHOICES[self.selected]) {
            Ok(()) => Outcome {
                close: true,
                saved: true,
            },
            Err(error) => {
                self.save_error = Some(error.to_string());
                Outcome::default()
            }
        }
    }

    pub fn frame(&mut self, width: usize, height: usize) -> (Vec<Line>, String) {
        self.frame_width = width;
        self.row_targets.clear();
        let available = height.saturating_sub(1);
        let mut lines = vec![single("언어 / Language", width, Role::Title)];
        lines.push(single(
            &crate::tr!("저장됨: {}", "Saved: {}"; self.current.map_or("?", name)),
            width,
            Role::Body,
        ));
        let effective = language_name(crate::i18n::current());
        let effective_label = if self.current == Some(Preference::Auto) {
            crate::tr!("자동 적용: {effective}", "Auto resolves to: {effective}";)
        } else {
            crate::tr!("현재 화면: {effective}", "Current UI: {effective}";)
        };
        lines.push(single(&effective_label, width, Role::Muted));
        let count = available.saturating_sub(lines.len()).min(CHOICES.len());
        let start = self.selected.saturating_sub(count.saturating_sub(1));
        for (index, choice) in CHOICES.iter().enumerate().skip(start).take(count) {
            self.row_targets.push((lines.len(), index));
            lines.push(single(
                &format!(
                    "{} {}{}",
                    if index == self.selected { ">" } else { " " },
                    name(*choice),
                    if self.current == Some(*choice) {
                        crate::tr!(" · 저장됨", " · saved")
                    } else {
                        ""
                    }
                ),
                width,
                if index == self.selected {
                    Role::Selected
                } else {
                    Role::Body
                },
            ));
        }
        if count == CHOICES.len() {
            lines.push(Line::default());
            for text in [
                crate::tr!(
                    "자동 / Auto는 시스템 로캘을 따릅니다",
                    "자동 / Auto follows the system locale"
                ),
                crate::tr!(
                    "모든 Maeklog 탭에 즉시 적용",
                    "All Maeklog tabs · Applies immediately"
                ),
                crate::tr!(
                    "새 요약부터 이 언어 사용 · 기존 기록과 원문은 유지",
                    "New summaries use this language · Existing records and source text stay unchanged"
                ),
            ] {
                panel_style::wrap(
                    &mut lines,
                    text,
                    width.saturating_sub(2).max(2),
                    Role::Muted,
                );
            }
        }
        if let Some(error) = self.save_error.as_ref().or(self.load_error.as_ref()) {
            panel_style::wrap(
                &mut lines,
                &crate::tr!("설정 확인/저장 실패 · {error}", "Could not read/save settings · {error}";),
                width.saturating_sub(2).max(2),
                Role::Warning,
            );
        }
        lines.truncate(available);
        self.row_targets.retain(|(row, _)| *row < lines.len());
        let can_save = width >= 15 && !self.row_targets.is_empty();
        let footer = footer(width, can_save);
        (lines, footer)
    }
}

fn single(text: &str, width: usize, role: Role) -> Line {
    let mut cells = 0;
    let text = text
        .chars()
        .take_while(|ch| {
            cells += ch.width().unwrap_or(0);
            cells <= width
        })
        .collect();
    Line { text, role }
}

fn footer(width: usize, can_save: bool) -> String {
    let candidates = if can_save {
        [
            crate::tr!(
                "↑↓ 선택 · Enter 적용 · Esc 취소",
                "↑↓ choose · Enter apply · Esc cancel"
            ),
            crate::tr!("↑↓ · Enter 적용 · Esc", "↑↓ · Enter apply · Esc"),
            "Enter · Esc",
            "Esc",
        ]
    } else {
        [
            crate::tr!(
                "창을 넓혀주세요 · Esc 취소",
                "Enlarge the window · Esc cancel"
            ),
            "Esc",
            "Esc",
            "Esc",
        ]
    };
    candidates
        .into_iter()
        .find(|text| {
            text.chars()
                .map(|ch| ch.width().unwrap_or(0))
                .sum::<usize>()
                <= width
        })
        .map_or_else(|| "Esc".chars().take(width).collect(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};
    fn picker() -> Picker {
        Picker {
            current: Some(Preference::Auto),
            selected: 0,
            load_error: None,
            save_error: None,
            row_targets: Vec::new(),
            frame_width: 0,
            pressed: None,
        }
    }
    #[test]
    fn names_navigation_cancel_and_small_windows_do_not_persist() {
        let mut picker = picker();
        assert_eq!(name(Preference::Auto), "자동 / Auto");
        assert_eq!(name(Preference::En), "English");
        assert_eq!(name(Preference::Ko), "한국어");
        for code in [
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Char('s'),
            KeyCode::Char('r'),
            KeyCode::Char('q'),
            KeyCode::F(10),
        ] {
            assert!(
                !picker
                    .handle(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
                    .saved
            );
        }
        assert_eq!(picker.current, Some(Preference::Auto));
        assert_eq!(picker.selected, 2);
        for (width, height) in [(0, 0), (2, 4), (12, 8), (30, 8), (30, 28), (80, 40)] {
            let (lines, footer) = picker.frame(width, height);
            assert!(lines.len() <= height.saturating_sub(1));
            assert!(lines.iter().all(|line| {
                line.chars()
                    .map(|ch| ch.width().unwrap_or(0))
                    .sum::<usize>()
                    <= width
            }));
            assert!(
                footer
                    .chars()
                    .map(|ch| ch.width().unwrap_or(0))
                    .sum::<usize>()
                    <= width
            );
            if width < 15 {
                assert!(
                    !picker
                        .handle(&Event::Key(KeyEvent::new(
                            KeyCode::Enter,
                            KeyModifiers::NONE
                        )))
                        .saved
                );
            }
        }
        let outcome = picker.handle(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(outcome.close && !outcome.saved);
    }
}
