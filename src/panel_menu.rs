//! A Maeklog-pane-only context menu. Opening/dismissing never changes participation.
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};

const ITEMS: [(&str, &str); 15] = [
    ("저장 기록 보기", "View saved records"),
    ("요약 모델…", "Summary model…"),
    ("언어 / Language…", "언어 / Language…"),
    ("진행 상황 보기", "View current progress"),
    ("이슈·PR 연결", "Manage issue/PR links"),
    ("근거 확인·기록 수정", "View evidence / edit records"),
    ("연결된 이슈·PR 열기", "Open linked issue/PR"),
    ("진행 상황 요약", "Summarize current progress"),
    ("날짜별 대화 요약…", "Summarize by date…"),
    ("오늘 미반영 대화 요약", "Summarize pending turns today"),
    ("연결 상태 새로고침", "Refresh link status"),
    (
        "──────── 패널 / 수집 ────────",
        "──────── Panel / collection ────────",
    ),
    ("패널 숨기기", "Hide panel"),
    ("이 탭 수집 끄기", "Stop recording in this tab"),
    ("닫기", "Close menu"),
];
const SHORTCUTS: [&str; 15] = [
    "h", "", "", "", "l", "e", "o", "s", "b", "r", "v", "", "q", "", "Esc",
];

pub fn shortcut(index: usize) -> &'static str {
    SHORTCUTS[index]
}

pub fn item(index: usize) -> &'static str {
    let (ko, en) = ITEMS[index];
    crate::tr!(ko, en)
}
pub fn hint() -> &'static str {
    crate::tr!("기능 선택", "Choose an action")
}
pub fn footer(width: usize) -> String {
    if width < 15 {
        return "Esc".chars().take(width).collect();
    }
    let candidates = [
        crate::tr!(
            "↑↓ 이동 · Enter 선택 · Esc 닫기",
            "↑↓ move · Enter choose · Esc close"
        ),
        crate::tr!("↑↓ · Enter 선택 · Esc 닫기", "↑↓ · Enter choose · Esc"),
        "Enter · Esc",
        "Esc",
    ];
    candidates
        .into_iter()
        .find(|text| {
            text.chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                .sum::<usize>()
                <= width
        })
        .map_or_else(|| "Esc".chars().take(width).collect(), str::to_owned)
}

const SEPARATOR: usize = 11;
const MIN_WIDTH: u16 = 15;
const ACTIONS: [Option<Action>; 15] = [
    Some(Action::Records),
    Some(Action::Model),
    Some(Action::Language),
    Some(Action::Progress),
    Some(Action::Links),
    Some(Action::Evidence),
    Some(Action::OpenLinks),
    Some(Action::CatchUp),
    Some(Action::History),
    Some(Action::Retry),
    Some(Action::RefreshLinks),
    None,
    Some(Action::Hide),
    Some(Action::Disable),
    None,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Records,
    Progress,
    Evidence,
    OpenLinks,
    Retry,
    Disable,
    Hide,
    History,
    CatchUp,
    Links,
    RefreshLinks,
    Model,
    Language,
}

#[derive(Debug)]
pub struct Menu {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub selected: usize,
    pressed: Option<usize>,
    first_visible: usize,
}

pub fn route(
    menu: &mut Option<Menu>,
    event: &Event,
    width: u16,
    height: u16,
) -> (bool, Option<Action>) {
    match event {
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Right) => {
            *menu = Some(Menu::new(mouse.column, mouse.row, width, height));
            return (true, None);
        }
        Event::Key(key)
            if key.kind == KeyEventKind::Press
                && (key.code == KeyCode::F(10)
                    || (key.code == KeyCode::Char('m') && key.modifiers.is_empty())) =>
        {
            *menu = Some(Menu::new(0, 1, width, height));
            return (true, None);
        }
        Event::Resize(w, h) => {
            if let Some(menu) = menu {
                menu.resize(*w, *h);
            }
        }
        _ => {}
    }
    if let Some(active) = menu {
        let (close, action) = active.handle(event);
        if close {
            *menu = None;
        }
        return (true, action);
    }
    (false, None)
}

impl Menu {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        let mut menu = Self {
            x,
            y,
            width: 0,
            height: 0,
            selected: 0,
            pressed: None,
            first_visible: 0,
        };
        menu.resize(width, height);
        menu
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width.min(38);
        self.height = height.min(u16::try_from(ITEMS.len() + 4).unwrap_or(u16::MAX));
        self.x = self.x.min(width.saturating_sub(self.width));
        self.y = self.y.min(height.saturating_sub(self.height));
        self.pressed = None;
        self.ensure_visible();
    }

    fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        if self.width < MIN_WIDTH || self.height < 5 || x <= self.x || x >= self.x + self.width - 1
        {
            return None;
        }
        let row = y.checked_sub(self.y + 2)?;
        let range = self.visible_items();
        (usize::from(row) < range.len())
            .then_some(range.start + usize::from(row))
            .filter(|index| *index != SEPARATOR)
    }

    pub fn visible_items(&self) -> std::ops::Range<usize> {
        let count = usize::from(self.height.saturating_sub(4)).min(ITEMS.len());
        let start = self.first_visible;
        start..(start + count).min(ITEMS.len())
    }

    fn ensure_visible(&mut self) {
        let count = usize::from(self.height.saturating_sub(4))
            .min(ITEMS.len())
            .max(1);
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + count {
            self.first_visible = self.selected + 1 - count;
        }
        self.first_visible = self.first_visible.min(ITEMS.len().saturating_sub(count));
    }

    // Returns (close menu, selected action). Esc/outside click are not Hide.
    pub fn handle(&mut self, event: &Event) -> (bool, Option<Action>) {
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
            && key.modifiers.is_empty()
            && let KeyCode::Char(ch) = key.code
            && let Some(index) = SHORTCUTS
                .iter()
                .position(|shortcut| shortcut.len() == 1 && shortcut.starts_with(ch))
        {
            // Direct actions follow the same minimum-size guard as Enter.
            return if self.width >= MIN_WIDTH && self.height >= 5 {
                (true, ACTIONS[index])
            } else {
                (false, None)
            };
        }
        let activate = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return (true, None),
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = (self.selected + ITEMS.len() - 1) % ITEMS.len();
                    if self.selected == SEPARATOR {
                        self.selected -= 1;
                    }
                    self.ensure_visible();
                    false
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1) % ITEMS.len();
                    if self.selected == SEPARATOR {
                        self.selected += 1;
                    }
                    self.ensure_visible();
                    false
                }
                KeyCode::Enter => true,
                _ => false,
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.pressed = self.item_at(mouse.column, mouse.row);
                    if let Some(index) = self.pressed {
                        self.selected = index;
                    } else {
                        return (true, None);
                    }
                    false
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let hit = self.item_at(mouse.column, mouse.row);
                    if let Some(pressed) = self.pressed.take()
                        && Some(pressed) == hit
                    {
                        self.selected = pressed;
                        true
                    } else {
                        false
                    }
                }
                MouseEventKind::Moved => {
                    if let Some(index) = self.item_at(mouse.column, mouse.row) {
                        self.selected = index;
                    }
                    false
                }
                _ => false,
            },
            _ => false,
        };
        if !activate || self.width < MIN_WIDTH || self.height < 5 {
            return (false, None);
        }
        (true, ACTIONS[self.selected])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers, MouseEvent};
    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    }
    #[test]
    fn default_enter_is_read_only_and_cancel_never_hides_or_disables() {
        for code in [KeyCode::Char('m'), KeyCode::F(10)] {
            let mut menu = None;
            assert_eq!(route(&mut menu, &key(code), 80, 24), (true, None));
            assert_eq!(menu.as_ref().unwrap().selected, 0);
            assert_eq!(route(&mut menu, &key(KeyCode::Esc), 80, 24), (true, None));
            assert!(menu.is_none());
        }
        for (w, h) in [(80, 24), (24, 10), (8, 4), (0, 0)] {
            let mut menu = Menu::new(79, 23, w, h);
            assert!(menu.x + menu.width <= w && menu.y + menu.height <= h);
            assert_eq!(
                menu.handle(&mouse(MouseEventKind::Up(MouseButton::Right), 0, 0)),
                (false, None)
            );
            if h >= 5 && w >= 3 {
                assert_eq!(
                    menu.handle(&key(KeyCode::Enter)),
                    (true, Some(Action::Records))
                );
            }
            assert_eq!(menu.handle(&key(KeyCode::Esc)), (true, None));
        }
    }
    #[test]
    fn press_release_must_match_and_progress_is_separate_from_summary_request() {
        let mut menu = Menu::new(2, 3, 80, 24);
        assert_eq!(
            menu.handle(&mouse(MouseEventKind::Up(MouseButton::Left), 3, 5)),
            (false, None)
        );
        menu.handle(&mouse(MouseEventKind::Down(MouseButton::Left), 3, 5));
        assert_eq!(
            menu.handle(&mouse(MouseEventKind::Up(MouseButton::Left), 3, 6)),
            (false, None)
        );
        menu.handle(&mouse(MouseEventKind::Down(MouseButton::Left), 3, 8));
        assert_eq!(
            menu.handle(&mouse(MouseEventKind::Up(MouseButton::Left), 3, 8)),
            (true, Some(Action::Progress))
        );
        assert_eq!(ACTIONS[3], Some(Action::Progress));
    }
    #[test]
    fn all_explicit_actions_remain_reachable_in_a_short_menu_and_separator_is_skipped() {
        let mut menu = Menu::new(0, 0, 30, 8);
        let expected = [
            Action::Records,
            Action::Model,
            Action::Language,
            Action::Progress,
            Action::Links,
            Action::Evidence,
            Action::OpenLinks,
            Action::CatchUp,
            Action::History,
            Action::Retry,
            Action::RefreshLinks,
            Action::Hide,
            Action::Disable,
        ];
        for action in expected {
            assert!(menu.visible_items().contains(&menu.selected));
            assert_eq!(menu.handle(&key(KeyCode::Enter)), (true, Some(action)));
            menu.handle(&key(KeyCode::Down));
        }
        assert_eq!(menu.handle(&key(KeyCode::Enter)), (true, None));
    }
    #[test]
    fn advertised_shortcuts_activate_only_unmodified_in_usable_menus() {
        for (ch, action) in [
            ('h', Action::Records),
            ('l', Action::Links),
            ('e', Action::Evidence),
            ('o', Action::OpenLinks),
            ('s', Action::CatchUp),
            ('b', Action::History),
            ('r', Action::Retry),
            ('v', Action::RefreshLinks),
            ('q', Action::Hide),
        ] {
            let mut menu = Menu::new(0, 0, 80, 40);
            assert_eq!(menu.handle(&key(KeyCode::Char(ch))), (true, Some(action)));
            let modified = Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
            assert_eq!(menu.handle(&modified), (false, None));
            assert_eq!(
                Menu::new(0, 0, 2, 4).handle(&key(KeyCode::Char(ch))),
                (false, None)
            );
        }
        let mut menu = Menu::new(0, 0, 80, 40);
        menu.handle(&key(KeyCode::Down));
        assert_eq!(
            menu.handle(&key(KeyCode::Enter)),
            (true, Some(Action::Model))
        );
        menu.handle(&key(KeyCode::Down));
        assert_eq!(
            menu.handle(&key(KeyCode::Enter)),
            (true, Some(Action::Language))
        );
        assert_eq!(item(2), "언어 / Language…");
        for width in [0, 1, 2, 8, 30, 80] {
            let footer = footer(width);
            let cells = footer
                .chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                .sum::<usize>();
            assert!(cells <= width);
        }
    }

    #[test]
    fn hover_keeps_actions_under_the_pointer_when_menu_scrolls() {
        let mut menu = Menu::new(0, 0, 30, 8);
        for _ in 0..8 {
            menu.handle(&key(KeyCode::Down));
        }
        let visible = menu.visible_items();
        menu.handle(&mouse(MouseEventKind::Moved, 1, 2));
        assert_eq!(menu.selected, visible.start);
        assert_eq!(menu.visible_items(), visible);
        menu.handle(&mouse(MouseEventKind::Down(MouseButton::Left), 1, 2));
        assert_eq!(
            menu.handle(&mouse(MouseEventKind::Up(MouseButton::Left), 1, 2)),
            (true, ACTIONS[visible.start])
        );
    }
}
