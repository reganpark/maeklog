//! Read-only frozen views: no command, capture, network or participation changes.
use crate::branch_panel::RecordSection;
use crate::panel_style::{Line, Role, wrap};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};

pub(super) struct Reader {
    title: String,
    scope: String,
    captured: String,
    sections: Vec<RecordSection>,
    selected: usize,
    anchor: (usize, usize),
    selection: Option<crate::panel_selection::Selection>,
    copy_request: Option<String>,
    selection_cleared: bool,
    pub return_scroll: usize,
}

impl Reader {
    pub fn new(
        title: &str,
        scope: String,
        captured: String,
        mut sections: Vec<RecordSection>,
        return_scroll: usize,
    ) -> Self {
        if sections.is_empty() {
            sections.push(RecordSection {
                title: crate::tr!("저장된 내용 없음", "No saved content").into(),
                rows: vec![Line::default()],
            });
        }
        Self {
            title: title.into(),
            scope,
            captured,
            sections,
            selected: 0,
            anchor: (0, 0),
            selection: None,
            copy_request: None,
            selection_cleared: false,
            return_scroll,
        }
    }

    pub fn frame(&mut self, width: usize, height: usize) -> Vec<Line> {
        if let Some(selection) = &self.selection {
            return selection.lines.clone();
        }
        let width = width.saturating_sub(2).max(2);
        let mut rows = self.header(width);
        let available = height.saturating_sub(rows.len() + 1);
        let (body, starts) = self.layout(width);
        let scroll = self
            .scroll(&starts, body.len())
            .min(body.len().saturating_sub(available.max(1)));
        self.save_anchor(scroll, &starts);
        rows.extend(body.into_iter().skip(scroll).take(available));
        rows
    }

    /// Rebuild chrome from the same captured data; keep category, time and anchor.
    pub fn language_changed(&mut self, translated: Self) {
        self.title = translated.title;
        self.scope = translated.scope;
        self.sections = translated.sections;
        self.selected = self.selected.min(self.sections.len().saturating_sub(1));
        self.selection_cleared = self.selection.take().is_some();
        self.copy_request = None;
    }

    pub fn footer(&self, width: usize) -> &'static str {
        if self.selection_cleared {
            return crate::tr!("선택 해제 · Esc 뒤로", "Selection cleared · Esc back");
        }
        if let Some(selected) = &self.selection {
            return if selected.dragging {
                crate::tr!(
                    "드래그 선택 중 · 놓으면 복사 요청 · Esc 선택 해제",
                    "Selecting · Release to request copy · Esc clear selection"
                )
            } else {
                crate::tr!(
                    "선택 유지 · 복사 요청 · Esc 선택 해제",
                    "Selection held · Copy requested · Esc clear selection"
                )
            };
        }
        if width < 48 {
            crate::tr!("←→ 범주 · ↑↓ · Esc 뒤로", "←→ category · ↑↓ · Esc back")
        } else {
            crate::tr!(
                "←→ 또는 [ ] 범주 · ↑↓/PgUp/PgDn 읽기 · Esc 메인으로",
                "←→ or [ ] category · ↑↓/PgUp/PgDn read · Esc main"
            )
        }
    }

    fn header(&self, width: usize) -> Vec<Line> {
        let mut rows = Vec::new();
        wrap(&mut rows, &self.title, width, Role::Title);
        wrap(&mut rows, &self.scope, width, Role::Muted);
        wrap(
            &mut rows,
            &crate::tr!("열람 시점 {} · 고정된 내용", "Captured {} · Frozen content"; self.captured),
            width,
            Role::Muted,
        );
        wrap(
            &mut rows,
            &format!(
                "{}/{} · {}",
                self.selected + 1,
                self.sections.len(),
                self.sections[self.selected].title
            ),
            width,
            Role::Title,
        );
        rows.push(Line::default());
        rows
    }

    fn layout(&self, width: usize) -> (Vec<Line>, Vec<usize>) {
        let mut body = Vec::new();
        let mut starts = Vec::new();
        for line in &self.sections[self.selected].rows {
            starts.push(body.len());
            if line.text.is_empty() {
                body.push(Line::default());
            } else {
                wrap(&mut body, &line.text, width, line.role);
            }
        }
        if body.is_empty() {
            starts.push(0);
            body.push(Line::default());
        }
        (body, starts)
    }

    fn scroll(&self, starts: &[usize], body_len: usize) -> usize {
        let row = self.anchor.0.min(starts.len().saturating_sub(1));
        let start = starts[row];
        let end = starts.get(row + 1).copied().unwrap_or(body_len);
        start + self.anchor.1.min(end.saturating_sub(start + 1))
    }
    fn save_anchor(&mut self, scroll: usize, starts: &[usize]) {
        let row = starts
            .partition_point(|start| *start <= scroll)
            .saturating_sub(1);
        self.anchor = (row, scroll.saturating_sub(starts[row]));
    }

    // Every event belongs to this reader; caller must never route unhandled keys to main.
    pub fn handle(&mut self, event: &Event, width: usize, height: usize) -> bool {
        self.selection_cleared = false;
        if self.handle_selection(event, width, height) {
            return false;
        }
        let width = width.saturating_sub(2).max(2);
        let (body, starts) = self.layout(width);
        let available = height.saturating_sub(self.header(width).len() + 1).max(1);
        let current = self.scroll(&starts, body.len());
        let mut next = current;
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc | KeyCode::Char('h' | 'q') => return true,
                KeyCode::Right | KeyCode::Char(']') => {
                    self.selected = (self.selected + 1) % self.sections.len();
                    self.anchor = (0, 0);
                    return false;
                }
                KeyCode::Left | KeyCode::Char('[') => {
                    self.selected = (self.selected + self.sections.len() - 1) % self.sections.len();
                    self.anchor = (0, 0);
                    return false;
                }
                KeyCode::Down | KeyCode::Char('j') => next = current.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => next = current.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => next = current.saturating_add(available),
                KeyCode::PageUp => next = current.saturating_sub(available),
                KeyCode::Home => next = 0,
                KeyCode::End => next = body.len().saturating_sub(available),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => next = current.saturating_add(3),
                MouseEventKind::ScrollUp => next = current.saturating_sub(3),
                _ => {}
            },
            _ => {}
        }
        self.save_anchor(next.min(body.len().saturating_sub(available)), &starts);
        false
    }

    pub fn selection(&self) -> Option<&crate::panel_selection::Selection> {
        self.selection.as_ref()
    }
    pub fn take_copy(&mut self) -> Option<String> {
        self.copy_request.take()
    }

    fn handle_selection(&mut self, event: &Event, width: usize, height: usize) -> bool {
        use crossterm::event::MouseButton;
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Esc
                && self.selection.is_some()
            {
                self.selection = None;
                return true;
            }
            self.selection = None;
        }
        match event {
            Event::Resize(..) => self.selection = None,
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if usize::from(mouse.row) < height.saturating_sub(1) =>
                {
                    self.selection = None;
                    let frame = self.frame(width, height);
                    self.selection = Some(crate::panel_selection::Selection::new(
                        frame,
                        0,
                        u16::try_from(width).unwrap_or(u16::MAX),
                        u16::try_from(height).unwrap_or(u16::MAX),
                        mouse.column,
                        mouse.row,
                    ));
                    return true;
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(selected) = &mut self.selection {
                        selected.update(mouse.column, mouse.row);
                    }
                    return true;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(selected) = &mut self.selection
                        && selected.dragging
                    {
                        selected.update(mouse.column, mouse.row);
                        selected.dragging = false;
                        self.copy_request =
                            crate::panel_selection::clipboard_sequence(&selected.text());
                    }
                    return true;
                }
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => self.selection = None,
                _ => {}
            },
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};
    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn reader() -> Reader {
        Reader::new(
            "전체 기록",
            "이 탭의 저장 기록 · main".into(),
            "2026/10/12 09:47".into(),
            vec![
                RecordSection {
                    title: "오늘 완료 · 42건".into(),
                    rows: (0..42)
                        .map(|i| Line {
                            text: format!("기록{i} · 조건: 승인 후 적용"),
                            role: Role::Body,
                        })
                        .collect(),
                },
                RecordSection {
                    title: "이전 기록".into(),
                    rows: vec![Line {
                        text: "이전 원문 전체".into(),
                        role: Role::Body,
                    }],
                },
            ],
            19,
        )
    }
    #[test]
    fn frozen_reader_keeps_all_rows_categories_and_original_main_scroll() {
        let mut view = reader();
        assert!(view.frame(80, 20).join("\n").contains(crate::tr!(
            "열람 시점 2026/10/12 09:47",
            "Captured 2026/10/12 09:47"
        )));
        view.handle(&key(KeyCode::End), 80, 20);
        assert!(view.frame(80, 20).join("\n").contains("기록41"));
        view.handle(&key(KeyCode::Right), 80, 20);
        assert!(view.frame(30, 20).join("\n").contains("이전 원문 전체"));
        assert!(view.handle(&key(KeyCode::Esc), 30, 20));
        assert_eq!(view.return_scroll, 19);
    }
    #[test]
    fn mutation_shortcuts_are_consumed_and_resize_preserves_logical_anchor() {
        let mut view = reader();
        view.handle(&key(KeyCode::PageDown), 80, 20);
        let original = view.anchor.0;
        for code in [
            KeyCode::Char('s'),
            KeyCode::Char('r'),
            KeyCode::Char('R'),
            KeyCode::Char('v'),
            KeyCode::Char('e'),
            KeyCode::Char('m'),
            KeyCode::Enter,
            KeyCode::F(10),
        ] {
            assert!(!view.handle(&key(code), 80, 20));
            assert_eq!(view.anchor.0, original);
        }
        view.frame(30, 20);
        assert_eq!(view.anchor.0, original);
    }
    #[test]
    fn widening_inside_a_long_paragraph_does_not_skip_to_the_next_record() {
        let mut view = reader();
        view.sections[0].rows[0].text = "긴 문단 원문을 읽습니다 ".repeat(16);
        view.anchor = (0, 8);
        view.frame(30, 20);
        assert_eq!(view.anchor.0, 0);
        view.frame(100, 20);
        assert_eq!(view.anchor.0, 0);
        assert!(view.anchor.1 < 8);
    }
    #[test]
    fn relabeling_retains_frozen_capture_category_and_anchor_and_clears_copy_selection() {
        let mut view = reader();
        view.selected = 1;
        view.anchor = (0, 1);
        let captured = view.captured.clone();
        let original = view.sections[1].rows.clone();
        view.selection = Some(crate::panel_selection::Selection::new(
            view.frame(30, 16),
            0,
            30,
            16,
            0,
            0,
        ));
        let anchor = view.anchor;
        view.copy_request = Some("stale-copy-coordinate".into());
        let mut translated = reader();
        translated.title = "Translated reader chrome".into();
        translated.scope = "Translated captured scope".into();
        translated.sections[1].title = "Translated category".into();
        view.language_changed(translated);
        assert_eq!(view.captured, captured);
        assert_eq!(view.anchor, anchor);
        assert_eq!(view.selected, 1);
        assert_eq!(
            view.sections[1]
                .rows
                .iter()
                .map(|line| (&line.text, line.role))
                .collect::<Vec<_>>(),
            original
                .iter()
                .map(|line| (&line.text, line.role))
                .collect::<Vec<_>>()
        );
        assert!(view.selection.is_none() && view.copy_request.is_none());
        assert!(
            view.footer(30)
                .contains(crate::tr!("선택 해제", "Selection cleared"))
        );
    }

    #[test]
    fn reader_can_copy_visible_full_records_and_escape_only_clears_selection_first() {
        use crossterm::event::{MouseButton, MouseEvent};
        let mut view = reader();
        let mouse = |kind, row, column| {
            Event::Mouse(MouseEvent {
                kind,
                row,
                column,
                modifiers: KeyModifiers::NONE,
            })
        };
        view.handle(
            &mouse(MouseEventKind::Down(MouseButton::Left), 5, 0),
            80,
            20,
        );
        assert!(
            view.footer(80)
                .starts_with(crate::tr!("드래그 선택 중", "Selecting"))
        );
        view.handle(
            &mouse(MouseEventKind::Drag(MouseButton::Left), 5, 20),
            80,
            20,
        );
        view.handle(&mouse(MouseEventKind::Up(MouseButton::Left), 5, 20), 80, 20);
        assert_eq!(
            view.footer(80),
            crate::tr!(
                "선택 유지 · 복사 요청 · Esc 선택 해제",
                "Selection held · Copy requested · Esc clear selection"
            )
        );
        assert!(
            view.take_copy()
                .is_some_and(|request| request.starts_with("\u{1b}]52;"))
        );
        assert!(view.selection().is_some());
        assert!(!view.handle(&key(KeyCode::Esc), 80, 20));
        assert!(view.selection().is_none());
        assert!(view.handle(&key(KeyCode::Esc), 80, 20));
    }
}
