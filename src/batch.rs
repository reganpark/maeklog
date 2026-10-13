//! Bound backlog reductions without splitting a completed turn or deleting queued text.
use crate::model::{EventKind, TurnEvent};

const MAX_EVENTS: usize = 8;
const MAX_CHARS: usize = 16_000;

/// Select the newest complete turns, but keep their internal chronological order.
pub fn newest(events: &[TurnEvent], small: bool) -> Vec<TurnEvent> {
    let mut turns = Vec::new();
    let mut start = 0;
    for (index, event) in events.iter().enumerate() {
        if index > start && events[index - 1].session_id != event.session_id {
            start = index; // Do not attach an abandoned prompt from another session.
        }
        if matches!(
            event.kind,
            EventKind::AssistantFinal | EventKind::Interrupted
        ) {
            turns.push(start..index + 1);
            start = index + 1;
        }
    }
    let Some(last) = turns.last() else {
        return Vec::new();
    };
    let end = last.end;
    let mut start = end;
    let mut chars = 0;
    for turn in turns.iter().rev() {
        if events[turn.start].session_id != events[end - 1].session_id || turn.end != start {
            break;
        }
        let size = events[turn.clone()]
            .iter()
            .map(|e| e.content.chars().count())
            .sum::<usize>();
        if start < end && (end - turn.start > MAX_EVENTS || chars + size > MAX_CHARS) {
            break;
        }
        start = turn.start;
        chars += size;
        if small || end - start >= MAX_EVENTS || chars >= MAX_CHARS {
            break;
        }
    }
    events[start..end].to_vec()
}

#[cfg(test)]
pub fn select(events: &[TurnEvent], small: bool) -> Vec<TurnEvent> {
    let mut end = 0;
    let mut chars = 0;
    for (index, event) in events.iter().enumerate() {
        chars += event.content.chars().count();
        if matches!(
            event.kind,
            EventKind::AssistantFinal | EventKind::Interrupted
        ) {
            if end > 0 && (index + 1 > MAX_EVENTS || chars > MAX_CHARS) {
                break;
            }
            end = index + 1;
            if small || end >= MAX_EVENTS || chars >= MAX_CHARS {
                break;
            }
            if events
                .get(end)
                .is_some_and(|next| next.session_id != event.session_id)
            {
                break;
            }
        }
    }
    // One oversized completed turn is retained in full; never silently discard its tail.
    events[..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(seq: u64, kind: EventKind) -> TurnEvent {
        TurnEvent {
            seq,
            kind,
            session_id: "a".into(),
            turn_id: None,
            model: None,
            content: "hello".into(),
            truncated: false,
            recorded_at_unix: seq,
            cwd: "/repo".into(),
        }
    }
    #[test]
    fn newest_selects_the_tail_without_reversing_or_consuming_an_open_turn() {
        let mut events: Vec<_> = (1..=21)
            .map(|seq| {
                event(
                    seq,
                    if seq % 2 == 0 {
                        EventKind::AssistantFinal
                    } else {
                        EventKind::UserPrompt
                    },
                )
            })
            .collect();
        assert_eq!(newest(&events, false), events[12..20]);
        assert_eq!(newest(&events, true), events[18..20]);
        assert!(newest(&events[20..], false).is_empty());
        events[18].session_id = "new".into();
        events[19].session_id = "new".into();
        assert_eq!(newest(&events, false), events[18..20]);
        events[18].content = "가".repeat(MAX_CHARS + 1);
        assert_eq!(newest(&events, false), events[18..20]);
    }
    #[test]
    fn newest_obeys_character_limits_and_excludes_an_abandoned_other_session_prompt() {
        let mut events: Vec<_> = (1..=10)
            .map(|seq| event(seq, EventKind::AssistantFinal))
            .collect();
        for e in &mut events {
            e.content = "가".repeat(4_000);
        }
        assert_eq!(newest(&events, false), events[6..]);
        events[7].kind = EventKind::UserPrompt;
        events[8].session_id = "new".into();
        events[9].session_id = "new".into();
        assert_eq!(newest(&events, false), events[8..]);
    }
    #[test]
    fn bounds_backlog_and_retries_one_turn_without_consuming_open_prompt() {
        let events: Vec<_> = (1..=21)
            .map(|seq| {
                event(
                    seq,
                    if seq % 2 == 1 {
                        EventKind::UserPrompt
                    } else {
                        EventKind::AssistantFinal
                    },
                )
            })
            .collect();
        assert_eq!(select(&events[..16], false).last().unwrap().seq, 8);
        assert_eq!(select(&events, false).last().unwrap().seq, 8);
        assert_eq!(select(&events, true).last().unwrap().seq, 2);
        assert!(select(&events[20..], false).is_empty());
    }
    #[test]
    fn backlog_keeps_character_and_event_caps_and_retry_fallback() {
        let mut events: Vec<_> = (1..=40)
            .map(|seq| event(seq, EventKind::AssistantFinal))
            .collect();
        assert_eq!(select(&events, false).len(), 8);
        assert_eq!(select(&events, true).len(), 1);
        for event in &mut events {
            event.content = "가".repeat(2_000);
        }
        assert_eq!(select(&events, false).len(), 8);
        events[3].content.push('가');
        assert_eq!(select(&events, false).len(), 7);
        assert_eq!(select(&events, true).len(), 1);
    }
    #[test]
    fn keeps_large_turn_whole_and_stops_at_session_boundary() {
        let mut events = vec![
            event(1, EventKind::UserPrompt),
            event(2, EventKind::AssistantFinal),
            event(3, EventKind::UserPrompt),
            event(4, EventKind::AssistantFinal),
        ];
        events[0].content = "가".repeat(MAX_CHARS + 1);
        assert_eq!(select(&events, false), events[..2]);
        events[0].content.clear();
        events[2].session_id = "b".into();
        events[3].session_id = "b".into();
        assert_eq!(select(&events, false), events[..2]);
    }
}
