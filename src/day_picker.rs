//! Explicit date selection; merely opening or navigating never authorizes work.
use crate::panel_style::{Line, Role, wrap};
use crate::{AppResult, daily, storage::SpaceStore};
use crossterm::event::{Event, KeyCode, KeyEventKind};
#[derive(Default)]
pub struct Picker {
    pub dates: Vec<(String, usize)>,
    pub selected: usize,
    pub open: bool,
}
impl Picker {
    pub fn show(&mut self, store: &SpaceStore) -> AppResult<()> {
        self.dates = daily::queue(store)?.manual.into_iter().rev().collect();
        self.selected = 0;
        self.open = true;
        Ok(())
    }
    pub fn handle(&mut self, store: &SpaceStore, event: &Event) -> Option<AppResult<()>> {
        if !self.open {
            return None;
        }
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q' | 'b') => self.open = false,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.dates.len().saturating_sub(1));
                }
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Enter => {
                    self.open = false;
                    return Some(
                        self.dates
                            .get(self.selected)
                            .map_or(Ok(()), |(day, _)| crate::worker::request_day(store, day)),
                    );
                }
                _ => {}
            }
        }
        Some(Ok(()))
    }
    pub fn footer(&self, width: usize) -> String {
        let choices = if self.dates.is_empty() {
            vec![
                crate::tr!(
                    "Esc 취소 · 선택할 날짜 없음",
                    "Esc cancel · no dates to select"
                ),
                "Esc",
            ]
        } else {
            vec![
                crate::tr!(
                    "↑↓ 선택 · Enter 선택 날짜만 실행 · Esc 취소",
                    "↑↓ select · Enter run selected date · Esc cancel"
                ),
                crate::tr!("↑↓ · Enter 실행 · Esc 취소", "↑↓ · Enter run · Esc cancel"),
                "Enter · Esc",
                "Esc",
            ]
        };
        choices
            .into_iter()
            .find(|text| {
                text.chars()
                    .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                    .sum::<usize>()
                    <= width
            })
            .map_or_else(|| "Esc".chars().take(width).collect(), str::to_owned)
    }
    pub fn lines(&self, width: usize) -> Vec<Line> {
        if !self.open {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut put = |text: &str, role| {
            wrap(&mut lines, text, width.saturating_sub(2).max(2), role);
        };
        put(
            crate::tr!(
                "이전/보류 대화 요약 — 날짜를 선택하세요",
                "Summarize earlier/held conversations — Select a date"
            ),
            Role::Heading,
        );
        if self.dates.is_empty() {
            put(
                crate::tr!(
                    "사용자 요청 대기 중인 완료 대화 없음",
                    "No completed conversations waiting for a request"
                ),
                Role::Muted,
            );
        }
        for (index, (day, count)) in self
            .dates
            .iter()
            .enumerate()
            .skip(self.selected.saturating_sub(5))
            .take(12)
        {
            put(
                &crate::tr!("{} {} · {count}턴", "{} {} · Turns: {count}";
                    if index == self.selected { "▶" } else { " " },
                    if day == "unknown" {
                        crate::tr!("시각 미확인/자정 걸친 대화", "Unknown time / crosses midnight")
                    } else {
                        day
                    }
                ),
                if index == self.selected {
                    Role::Selected
                } else {
                    Role::Body
                },
            );
        }
        put(
            crate::tr!(
                "모델 사용량 발생 · 조회/선택만으로는 호출하지 않음",
                "Uses the model when run · Browsing does not call it"
            ),
            Role::Muted,
        );
        lines
    }
}
pub fn queue_lines(
    queue: &daily::Queue,
    status: &crate::model::WorkerStatus,
    width: usize,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut put = |text: &str, role| {
        wrap(&mut lines, text, width.saturating_sub(2).max(2), role);
    };
    let state = if queue.today_turns == 0 && queue.requested.is_none() {
        crate::tr!(
            "완료/새 대화 대기",
            "Waiting for completion/new conversation"
        )
    } else {
        match status.state.as_str() {
            "summarizing" => crate::tr!("요약 중", "Summarizing"),
            "error" => crate::tr!(
                "요약 실패 · r 또는 날짜 선택으로 재시도",
                "Summary failed · r or select a date to retry"
            ),
            "retry_wait" => crate::tr!("실패 후 재시도 대기", "Waiting to retry after failure"),
            _ => crate::tr!("요약 대기", "Waiting to summarize"),
        }
    };
    let role = if queue.today_turns == 0 && queue.requested.is_none() {
        Role::Title // idle/empty is not proof of success
    } else {
        match status.state.as_str() {
            "error" => Role::Error,
            "retry_wait" => Role::Warning,
            _ => Role::Heading,
        }
    };
    put(
        &crate::tr!("오늘 {} · 미요약 {}턴 · {state}", "Today {} · Unsummarized turns: {} · {state}";
            queue.today, queue.today_turns
        ),
        role,
    );
    put(
        &crate::tr!("이전/보류 {}턴 · 사용자 요청 대기 · 자동 처리 안 함", "Earlier/held turns: {} · Waiting for your request";
            queue.manual.values().sum::<usize>()
        ),
        Role::Muted,
    );
    if queue.open_events > 0 {
        put(
            &crate::tr!("완료 대기 중인 대화 이벤트 {}개", "Conversation events awaiting completion: {}"; queue.open_events),
            Role::Muted,
        );
    }
    if let Some(day) = &queue.requested {
        put(
            &crate::tr!("사용자가 선택한 날짜: {day}", "Selected date: {day}";),
            Role::Heading,
        );
    }
    put(
        crate::tr!(
            "[b 이전/보류 대화 요약…] · h는 저장된 기록 보기",
            "[b summarize earlier/held conversations…] · h saved records"
        ),
        Role::Muted,
    );
    if matches!(status.state.as_str(), "error" | "retry_wait") {
        put(&status.message, Role::Body);
    }
    lines.push(Line::default());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_and_selection_use_status_not_body_keywords() {
        let mut queue = daily::Queue {
            today: "2026-09-14".into(),
            today_turns: 0,
            manual: std::collections::BTreeMap::default(),
            open_events: 1,
            requested: None,
        };
        let mut status = crate::model::WorkerStatus {
            state: "error".into(),
            ..Default::default()
        };
        assert_eq!(queue_lines(&queue, &status, 80)[0].role, Role::Title);
        queue.today_turns = 1;
        assert_eq!(queue_lines(&queue, &status, 80)[0].role, Role::Error);
        status.state = "retry_wait".into();
        assert_eq!(queue_lines(&queue, &status, 80)[0].role, Role::Warning);
        let picker = Picker {
            open: true,
            selected: 0,
            dates: vec![("2026-09-13".into(), 2), ("2026-09-12".into(), 1)],
        };
        let lines = picker.lines(80);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("▶") && l.role == Role::Selected)
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("2026-09-12") && l.role == Role::Body)
        );
    }
}
